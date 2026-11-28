//! Connect to Turso or libSQL over the Hrana HTTP API. The protocol types come
//! from libSQL and the transport uses Rapid's current Rustls version.

use std::{sync::Arc, time::Duration};

use libsql_hrana::proto::{
    Batch, BatchStreamReq, CloseStreamReq, ExecuteStreamReq, PipelineReqBody, PipelineRespBody,
    Row as DatabaseRow, Stmt, StmtResult, StreamRequest, StreamResponse, StreamResult, Value,
};
use reqwest::{Client, header::HeaderValue};
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

pub struct Turso {
    http: Client,
    endpoint: Url,
    auth: Option<HeaderValue>,
}

impl Turso {
    pub async fn connect(config: &Storage) -> Result<Self, StoreError> {
        let key = &config.connection.url_env;
        let endpoint =
            pipeline_url(&environment(key)?).ok_or_else(|| StoreError::InvalidUrl(key.clone()))?;
        let auth = authorization(&config.connection.token_env, endpoint.scheme() == "http")?;
        let http = client(Duration::from_millis(config.busy_timeout_ms.get().into()))?;
        let store = Self {
            http,
            endpoint,
            auth,
        };
        store.initialize().await?;
        Ok(store)
    }

    async fn initialize(&self) -> Result<(), StoreError> {
        let steps = include_str!("../../migrations/001_records.sql")
            .split(';')
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .map(|sql| Stmt::new(sql, false));
        let batch = StreamRequest::Batch(BatchStreamReq {
            batch: Batch::transactional(steps),
        });
        let mut stream = Stream::new(self);
        let response = stream.send("initialize schema", batch, true).await?;
        if let StreamResponse::Batch(batch) = response
            && batch.result.step_errors.iter().all(Option::is_none)
        {
            return Ok(());
        }
        Err(failure("initialize schema", "libSQL request failed"))
    }

    async fn execute(&self, operation: &'static str, stmt: Stmt) -> Result<StmtResult, StoreError> {
        Stream::new(self).execute(operation, stmt, true).await
    }

    async fn transaction<T>(
        &self,
        begin: &'static str,
        action: impl AsyncFnOnce(&mut Stream<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut stream = Stream::new(self);
        stream
            .execute("begin transfer", Stmt::new(begin, false), false)
            .await?;
        stream.require_baton("begin transfer")?;
        let result = action(&mut stream).await;
        match result {
            Ok(output) => {
                stream.require_baton("commit transfer")?;
                stream
                    .execute("commit transfer", Stmt::new("COMMIT", false), true)
                    .await?;
                Ok(output)
            }
            Err(error) => {
                stream.rollback().await?;
                Err(error)
            }
        }
    }
}

#[async_trait::async_trait]
impl Engine for Turso {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        self.transaction("BEGIN TRANSACTION READONLY", async |stream| {
            let records = transfer_rows(stream).await?;
            let sequence = watermark(stream)
                .await?
                .max(records.last().map_or(0, |row| row.id));
            Ok(Snapshot::new(sequence, records))
        })
        .await
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        self.transaction("BEGIN IMMEDIATE", async |stream| {
            restore(stream, snapshot).await
        })
        .await
    }

    async fn ready(&self) -> Result<(), StoreError> {
        self.execute(
            "readiness",
            Stmt::new("SELECT id, collection, payload FROM records LIMIT 0", false),
        )
        .await?;
        Ok(())
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        let stmt = statement(
            "SELECT id, payload FROM records WHERE collection = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
            true,
            [text(collection), number(after), number(count)],
        );
        self.execute("list", stmt)
            .await?
            .rows
            .into_iter()
            .map(stored)
            .collect()
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        let stmt = statement(
            "SELECT id, payload FROM records WHERE collection = ?1 AND id = ?2",
            true,
            [text(collection), number(id)],
        );
        self.execute("read", stmt)
            .await?
            .rows
            .into_iter()
            .next()
            .map(stored)
            .transpose()
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        let stmt = statement(
            "INSERT INTO records (collection, payload) VALUES (?1, ?2)",
            false,
            [text(collection), text(payload)],
        );
        let result = self.execute("create", stmt).await?;
        identity(result.last_insert_rowid.ok_or(StoreError::IdentityRange)?)
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        let stmt = statement(
            "UPDATE records SET payload = ?3 WHERE collection = ?1 AND id = ?2",
            false,
            [text(collection), number(id), text(payload)],
        );
        Ok(self.execute("replace", stmt).await?.affected_row_count > 0)
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        let stmt = statement(
            "DELETE FROM records WHERE collection = ?1 AND id = ?2",
            false,
            [text(collection), number(id)],
        );
        Ok(self.execute("delete", stmt).await?.affected_row_count > 0)
    }
}

struct Stream<'a> {
    client: &'a Turso,
    endpoint: Url,
    baton: Option<String>,
}

impl<'a> Stream<'a> {
    fn new(client: &'a Turso) -> Self {
        Self {
            client,
            endpoint: client.endpoint.clone(),
            baton: None,
        }
    }

    fn require_baton(&self, operation: &'static str) -> Result<(), StoreError> {
        if self.baton.is_none() {
            return Err(failure(operation, "transaction stream closed"));
        }
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), StoreError> {
        if self.baton.is_some() {
            self.execute("rollback transfer", Stmt::new("ROLLBACK", false), true)
                .await?;
        }
        Ok(())
    }

    async fn execute(
        &mut self,
        operation: &'static str,
        stmt: Stmt,
        close: bool,
    ) -> Result<StmtResult, StoreError> {
        let response = self
            .send(
                operation,
                StreamRequest::Execute(ExecuteStreamReq { stmt }),
                close,
            )
            .await?;
        if let StreamResponse::Execute(response) = response {
            return Ok(response.result);
        }
        Err(failure(operation, "unexpected libSQL response"))
    }

    async fn send(
        &mut self,
        operation: &'static str,
        request: StreamRequest,
        close: bool,
    ) -> Result<StreamResponse, StoreError> {
        let mut requests = vec![request];
        if close {
            requests.push(StreamRequest::Close(CloseStreamReq {}));
        }
        let body = PipelineReqBody {
            baton: self.baton.clone(),
            requests,
        };
        let mut request = self.client.http.post(self.endpoint.clone()).json(&body);
        if let Some(auth) = &self.client.auth {
            request = request.header(reqwest::header::AUTHORIZATION, auth);
        }
        let response = request
            .send()
            .await
            .map_err(|error| network(operation, error))?;
        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                return Err(failure(operation, "libSQL authorization failed"));
            }
            status if status.is_success() => {}
            _ => return Err(failure(operation, "libSQL HTTP request failed")),
        }
        let reply = response
            .json::<PipelineRespBody>()
            .await
            .map_err(|error| network(operation, error))?;
        self.baton = reply.baton;
        if let Some(base) = reply.base_url.filter(|_| !close) {
            self.endpoint = next_endpoint(&self.client.endpoint, &base)
                .ok_or_else(|| failure(operation, "untrusted libSQL stream endpoint"))?;
        }
        match reply.results.into_iter().next() {
            Some(StreamResult::Ok { response }) => Ok(response),
            Some(StreamResult::Error { .. }) => Err(failure(operation, "libSQL request failed")),
            _ => Err(failure(operation, "unexpected libSQL response")),
        }
    }
}

async fn restore(stream: &mut Stream<'_>, snapshot: Snapshot) -> Result<(), StoreError> {
    let stmt = Stmt::new("SELECT id FROM records LIMIT 1", true);
    if !stream
        .execute("check target", stmt, false)
        .await?
        .rows
        .is_empty()
    {
        return Err(StoreError::DestinationInUse);
    }
    let sequence = snapshot.watermark.max(watermark(stream).await?);
    insert_transfer_rows(stream, &snapshot.records).await?;
    update_watermark(stream, sequence).await?;
    if transfer_rows(stream).await? != snapshot.records {
        return Err(StoreError::Snapshot("destination verification failed"));
    }
    Ok(())
}

async fn insert_transfer_rows(stream: &mut Stream<'_>, records: &[Row]) -> Result<(), StoreError> {
    let sql = "INSERT INTO records (id, collection, payload)
        SELECT json_extract(value, '$.id'), json_extract(value, '$.collection'),
               json_extract(value, '$.payload') FROM json_each(?1)";
    stream.require_baton("insert transfer records")?;
    let stmt = statement(sql, false, [text(serde_json::to_string(records)?)]);
    stream
        .execute("insert transfer records", stmt, false)
        .await?;
    Ok(())
}

async fn transfer_rows(stream: &mut Stream<'_>) -> Result<Vec<Row>, StoreError> {
    stream.require_baton("read transfer rows")?;
    let stmt = Stmt::new(
        "SELECT id, collection, payload FROM records ORDER BY id",
        true,
    );
    stream
        .execute("read transfer rows", stmt, false)
        .await?
        .rows
        .into_iter()
        .map(transfer_row)
        .collect()
}

async fn watermark(stream: &mut Stream<'_>) -> Result<u32, StoreError> {
    stream.require_baton("read identity watermark")?;
    let stmt = Stmt::new(
        "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'records'), 0)",
        true,
    );
    let result = stream
        .execute("read identity watermark", stmt, false)
        .await?;
    let [row] = result.rows.as_slice() else {
        return Err(StoreError::IdentityRange);
    };
    let [Value::Integer { value }] = row.values.as_slice() else {
        return Err(StoreError::IdentityRange);
    };
    u32::try_from(*value).map_err(|_| StoreError::IdentityRange)
}

async fn update_watermark(stream: &mut Stream<'_>, sequence: u32) -> Result<(), StoreError> {
    if sequence == 0 {
        return Ok(());
    }
    stream.require_baton("set identity watermark")?;
    let stmt = statement(
        "UPDATE sqlite_sequence SET seq = ?1 WHERE name = 'records'",
        false,
        [number(sequence)],
    );
    let changed = stream
        .execute("set identity watermark", stmt, false)
        .await?
        .affected_row_count;
    if changed == 0 {
        stream.require_baton("initialize identity watermark")?;
        let stmt = statement(
            "INSERT INTO sqlite_sequence (name, seq) VALUES ('records', ?1)",
            false,
            [number(sequence)],
        );
        stream
            .execute("initialize identity watermark", stmt, false)
            .await?;
    }
    Ok(())
}

fn pipeline_url(raw: &str) -> Option<Url> {
    let raw = raw
        .strip_prefix("libsql://")
        .map_or_else(|| raw.to_owned(), |rest| format!("https://{rest}"));
    let mut url = endpoint(&raw)?;
    url.set_path(&format!("{}/v3/pipeline", url.path().trim_end_matches('/')));
    Some(url)
}

fn next_endpoint(original: &Url, base: &str) -> Option<Url> {
    let target = pipeline_url(base)?;
    let turso_host = |url: &Url| {
        url.scheme() == "https"
            && url.port().is_none()
            && url
                .domain()
                .is_some_and(|domain| domain.ends_with(".turso.io"))
    };
    (original.origin() == target.origin() || (turso_host(original) && turso_host(&target)))
        .then_some(target)
}

fn statement(sql: &str, want_rows: bool, args: impl IntoIterator<Item = Value>) -> Stmt {
    let mut stmt = Stmt::new(sql, want_rows);
    stmt.args.extend(args);
    stmt
}

fn stored(row: DatabaseRow) -> Result<Stored, StoreError> {
    let [Value::Integer { value: id }, Value::Text { value: payload }] = row.values.as_slice()
    else {
        return Err(failure("decode record", "unexpected libSQL response"));
    };
    Ok(Stored {
        id: identity(*id)?,
        payload: payload.to_string(),
    })
}

fn transfer_row(row: DatabaseRow) -> Result<Row, StoreError> {
    let [
        Value::Integer { value: id },
        Value::Text { value: collection },
        Value::Text { value: payload },
    ] = row.values.as_slice()
    else {
        return Err(failure("decode transfer row", "unexpected libSQL response"));
    };
    Ok(Row {
        id: identity(*id)?,
        collection: collection.to_string(),
        payload: payload.to_string(),
    })
}

fn text(value: impl Into<Arc<str>>) -> Value {
    Value::Text {
        value: value.into(),
    }
}

fn number(value: u32) -> Value {
    Value::Integer {
        value: i64::from(value),
    }
}

fn network(operation: &'static str, error: reqwest::Error) -> StoreError {
    let kind = if error.is_timeout() {
        "request timed out"
    } else {
        "HTTP or wire response failed"
    };
    failure(operation, kind)
}

#[cfg(test)]
mod tests {
    use super::{next_endpoint, pipeline_url};

    #[test]
    fn database_token_stays_on_configured_origin_or_turso_tls() {
        let initial = pipeline_url("libsql://project.turso.io").unwrap();
        assert_eq!(initial.as_str(), "https://project.turso.io/v3/pipeline");
        assert!(next_endpoint(&initial, "https://replica.turso.io").is_some());
        for untrusted in [
            "http://replica.turso.io",
            "https://turso.io.example.com",
            "https://replica.turso.io:8443",
            "https://other.example.com",
            "https://user:pass@replica.turso.io",
        ] {
            assert!(next_endpoint(&initial, untrusted).is_none());
        }
        let local = pipeline_url("http://127.0.0.1:8080").unwrap();
        assert!(next_endpoint(&local, "http://127.0.0.1:8080").is_some());
        assert!(next_endpoint(&local, "http://127.0.0.1:8081").is_none());
    }
}
