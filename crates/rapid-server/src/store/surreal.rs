//! SurrealDB 4 HTTP RPC adapter. Bindings and replies are typed. The database
//! runs as an independent service, so its engine is absent from the Rust build.

use std::time::Duration;

use reqwest::{Client, header::HeaderValue};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use tokio::sync::Mutex;
use url::Url;

use crate::{
    config::Storage,
    data::{Row, Snapshot},
};

use super::{
    Engine, StoreError, Stored, environment,
    http::{authorization, client, endpoint, failure},
    identity,
};

const CREATE: &str = "BEGIN TRANSACTION;
    LET $number = (UPDATE ONLY rapid_counters:global SET watermark += 1 RETURN VALUE watermark);
    IF $number > 4294967295 { THROW 'rapid_identity_range' };
    CREATE ONLY type::record('rapid_records', $number) CONTENT { number: $number, collection: $collection, payload: $payload };
    RETURN $number;
    COMMIT TRANSACTION;";

const EXPORT: &str = "BEGIN TRANSACTION;
    RETURN {
        rows: (SELECT number, collection, payload FROM rapid_records ORDER BY number),
        watermark: (SELECT VALUE watermark FROM ONLY rapid_counters:global)
    };
    COMMIT TRANSACTION;";

// The counter write makes concurrent creates and imports contend on the same
// record, even when the snapshot's watermark equals the destination's value.
const IMPORT: &str = "BEGIN TRANSACTION;
    UPDATE ONLY rapid_counters:global SET watermark = math::max([watermark, $watermark]), revision += 1;
    IF array::len((SELECT VALUE number FROM rapid_records LIMIT 1)) > 0 { THROW 'rapid_destination_in_use' };
    FOR $row IN $rows { CREATE ONLY type::record('rapid_records', $row.number) CONTENT $row; };
    LET $actual = (SELECT number, collection, payload FROM rapid_records ORDER BY number);
    IF $actual != $rows { THROW 'rapid_destination_verification' };
    COMMIT TRANSACTION;";

pub struct Surreal {
    client: Client,
    endpoint: Url,
    auth: HeaderValue,
    writes: Mutex<()>,
}

#[derive(Serialize)]
struct Call<'a, T> {
    id: u8,
    method: &'static str,
    params: (&'a str, &'a T),
}

#[derive(Deserialize)]
struct Reply {
    result: Vec<Statement>,
}

#[derive(Deserialize)]
struct Statement {
    status: Status,
    result: Box<RawValue>,
    details: Option<Details>,
}

#[derive(Deserialize, PartialEq, Eq)]
enum Status {
    #[serde(rename = "OK")]
    Complete,
    #[serde(rename = "ERR")]
    Failed,
}

#[derive(Deserialize)]
struct Details {
    kind: String,
}

#[derive(Serialize, Deserialize)]
struct WireRow {
    number: i64,
    collection: String,
    payload: String,
}

#[derive(Serialize, Deserialize)]
struct Transfer {
    rows: Vec<WireRow>,
    watermark: i64,
}

#[derive(Deserialize)]
struct WireRecord {
    number: i64,
    payload: String,
}

#[derive(Serialize)]
struct PageKey {
    collection: &'static str,
    after: u32,
    count: u32,
}

#[derive(Serialize)]
struct RecordKey {
    collection: &'static str,
    id: u32,
}

#[derive(Serialize)]
struct Change {
    collection: &'static str,
    payload: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u32>,
}

#[derive(Serialize)]
struct NoArgs {}

impl Surreal {
    pub async fn connect(storage: &Storage) -> Result<Self, StoreError> {
        let key = &storage.connection.url_env;
        let endpoint = rpc_endpoint(&environment(key)?)
            .ok_or_else(|| StoreError::InvalidUrl(key.to_owned()))?;
        let key = &storage.connection.token_env;
        let auth = authorization(key, false)?
            .ok_or_else(|| StoreError::MissingVariable(key.to_owned()))?;
        let timeout = Duration::from_millis(storage.busy_timeout_ms.get().into());
        let store = Self {
            client: client(timeout)?,
            endpoint,
            auth,
            writes: Mutex::new(()),
        };
        store
            .query(
                "initialize SurrealDB schema",
                include_str!("../../migrations/001_surreal.surql"),
                &NoArgs {},
            )
            .await?;
        Ok(store)
    }

    async fn query<T: Serialize + Sync>(
        &self,
        operation: &'static str,
        sql: &str,
        args: &T,
    ) -> Result<Vec<Statement>, StoreError> {
        let call = Call {
            id: 1,
            method: "query",
            params: (sql, args),
        };
        for delay in [5, 10, 20, 40, 80, 160, 0] {
            let reply = self.send(operation, &call).await?;
            if !reply.result.iter().any(Statement::conflict) {
                return checked(reply.result, operation);
            }
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }
        Err(failure(operation, "SurrealDB transaction conflicted"))
    }

    async fn send<T: Serialize + Sync>(
        &self,
        operation: &'static str,
        call: &Call<'_, T>,
    ) -> Result<Reply, StoreError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(call)
            .send()
            .await
            .map_err(|error| {
                let kind = if error.is_timeout() {
                    "request timed out"
                } else {
                    "HTTP connection unavailable"
                };
                failure(operation, kind)
            })?;
        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                return Err(failure(operation, "SurrealDB authorization failed"));
            }
            reqwest::StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(failure(
                    operation,
                    "SurrealDB request exceeds its configured size limit",
                ));
            }
            status if status.is_success() => {}
            _ => return Err(failure(operation, "SurrealDB HTTP request failed")),
        }
        response
            .json()
            .await
            .map_err(|_| failure(operation, "SurrealDB RPC rejected the request"))
    }
}

#[async_trait::async_trait]
impl Engine for Surreal {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let response = self
            .query("export SurrealDB snapshot", EXPORT, &NoArgs {})
            .await?;
        let transfer: Transfer = value(&response, 1)?;
        let records = transfer
            .rows
            .into_iter()
            .map(Row::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let watermark = u32::try_from(transfer.watermark).map_err(|_| StoreError::IdentityRange)?;
        Ok(Snapshot::new(
            watermark.max(records.last().map_or(0, |row| row.id)),
            records,
        ))
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        let _write = self.writes.lock().await;
        let params = Transfer {
            rows: snapshot.records.into_iter().map(WireRow::from).collect(),
            watermark: i64::from(snapshot.watermark),
        };
        self.query("import SurrealDB snapshot", IMPORT, &params)
            .await?;
        Ok(())
    }

    async fn ready(&self) -> Result<(), StoreError> {
        let sql = "SELECT VALUE watermark FROM ONLY rapid_counters:global";
        let response = self.query("readiness", sql, &NoArgs {}).await?;
        let _: u32 = value(&response, 0)?;
        Ok(())
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        let sql = "SELECT number, payload FROM rapid_records WHERE collection = $collection AND number > $after ORDER BY number LIMIT $count";
        let response = self
            .query(
                "list records",
                sql,
                &PageKey {
                    collection,
                    after,
                    count,
                },
            )
            .await?;
        value::<Vec<WireRecord>>(&response, 0)?
            .into_iter()
            .map(WireRecord::stored)
            .collect()
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        let sql = "SELECT number, payload FROM type::record('rapid_records', $id) WHERE collection = $collection";
        let response = self
            .query("read record", sql, &RecordKey { collection, id })
            .await?;
        value::<Vec<WireRecord>>(&response, 0)?
            .into_iter()
            .next()
            .map(WireRecord::stored)
            .transpose()
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        // The database still serializes concurrent writers when requests come
        // from multiple processes. Avoid local contention on the counter.
        let _write = self.writes.lock().await;
        let response = self
            .query(
                "create record",
                CREATE,
                &Change {
                    collection,
                    payload,
                    id: None,
                },
            )
            .await?;
        identity(value(&response, 4)?)
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        let sql = "UPDATE type::record('rapid_records', $id) SET payload = $payload WHERE collection = $collection RETURN VALUE number";
        let params = Change {
            collection,
            payload,
            id: Some(id),
        };
        let response = self.query("replace record", sql, &params).await?;
        Ok(!value::<Vec<u32>>(&response, 0)?.is_empty())
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        let sql = "DELETE type::record('rapid_records', $id) WHERE collection = $collection RETURN BEFORE";
        let response = self
            .query("delete record", sql, &RecordKey { collection, id })
            .await?;
        Ok(!value::<Vec<WireRecord>>(&response, 0)?.is_empty())
    }
}

impl Statement {
    fn conflict(&self) -> bool {
        self.status == Status::Failed
            && self
                .details
                .as_ref()
                .is_some_and(|details| details.kind == "TransactionConflict")
    }
}

fn checked(
    statements: Vec<Statement>,
    operation: &'static str,
) -> Result<Vec<Statement>, StoreError> {
    if let Some(statement) = statements
        .iter()
        .filter(|statement| statement.status == Status::Failed)
        .min_by_key(|statement| {
            statement
                .details
                .as_ref()
                .is_some_and(|details| matches!(details.kind.as_str(), "Cancelled" | "NotExecuted"))
        })
    {
        let detail = serde_json::from_str::<String>(statement.result.get()).unwrap_or_default();
        let error = match detail.as_str() {
            "An error occurred: rapid_destination_in_use" => StoreError::DestinationInUse,
            "An error occurred: rapid_destination_verification" => {
                StoreError::Snapshot("destination verification failed")
            }
            "An error occurred: rapid_identity_range" => StoreError::IdentityRange,
            _ => failure(operation, "SurrealDB statement failed"),
        };
        return Err(error);
    }
    if statements.is_empty() {
        return Err(failure(operation, "SurrealDB returned no statements"));
    }
    Ok(statements)
}

fn value<T: DeserializeOwned>(statements: &[Statement], offset: usize) -> Result<T, StoreError> {
    let response = statements
        .get(offset)
        .ok_or_else(|| failure("decode results", "missing SurrealDB statement"))?;
    serde_json::from_str(response.result.get())
        .map_err(|_| failure("decode results", "unexpected SurrealDB result type"))
}

impl WireRecord {
    fn stored(self) -> Result<Stored, StoreError> {
        Ok(Stored {
            id: identity(self.number)?,
            payload: self.payload,
        })
    }
}

impl TryFrom<WireRow> for Row {
    type Error = StoreError;

    fn try_from(row: WireRow) -> Result<Self, StoreError> {
        Ok(Self {
            id: identity(row.number)?,
            collection: row.collection,
            payload: row.payload,
        })
    }
}

impl From<Row> for WireRow {
    fn from(row: Row) -> Self {
        Self {
            number: i64::from(row.id),
            collection: row.collection,
            payload: row.payload,
        }
    }
}

fn rpc_endpoint(raw: &str) -> Option<Url> {
    let mut url = endpoint(raw)?;
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/rpc").unwrap_or(path);
    let segments: Vec<_> = path.split('/').collect();
    let ["", "ns", namespace, "db", database] = segments.as_slice() else {
        return None;
    };
    if ![namespace, database].into_iter().all(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }) {
        return None;
    }
    let path = format!("{path}/rpc");
    url.set_path(&path);
    Some(url)
}
