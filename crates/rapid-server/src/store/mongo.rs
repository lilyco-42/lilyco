//! MongoDB persistence with transactional ID allocation and snapshots.

use std::{net::IpAddr, time::Duration};

use mongodb::{
    Client, ClientSession, Collection, IndexModel,
    bson::{Document, doc},
    error::{Error, ErrorKind, Result as MongoResult},
    options::{
        ClientOptions, ReadConcern, ReturnDocument, ServerAddress, Tls, TlsOptions, WriteConcern,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    config::Storage,
    data::{Row, Snapshot},
};

use super::{Engine, StoreError, Stored, environment, identity};

pub struct Mongo {
    client: Client,
    records: Collection<MongoRow>,
    counters: Collection<Counter>,
}

#[derive(Deserialize, Serialize)]
struct MongoRow {
    #[serde(rename = "_id")]
    id: i64,
    collection: String,
    payload: String,
}

#[derive(Deserialize)]
struct Counter {
    watermark: i64,
}

enum TransferError {
    Identity,
    Occupied,
    Verify,
}

impl Mongo {
    pub async fn connect(storage: &Storage) -> Result<Self, StoreError> {
        let key = &storage.connection.url_env;
        let url = environment(key)?;
        if !url.starts_with("mongodb://") && !url.starts_with("mongodb+srv://") {
            return Err(StoreError::InvalidUrl(key.clone()));
        }
        let mut settings = ClientOptions::parse(url)
            .await
            .map_err(|error| failure("connect", error))?;
        secure_transport(&mut settings).map_err(|()| StoreError::InvalidUrl(key.clone()))?;
        let deadline = Duration::from_millis(storage.busy_timeout_ms.get().into());
        settings.connect_timeout = Some(deadline);
        settings.server_selection_timeout = Some(deadline);
        settings.max_pool_size = Some(storage.connection.max_connections.get());
        settings.write_concern = Some(WriteConcern::majority());
        let client = Client::with_options(settings).map_err(|error| failure("connect", error))?;
        let database = client.default_database().ok_or(StoreError::MongoTopology)?;
        let hello = database
            .run_command(doc! { "hello": 1 })
            .await
            .map_err(|error| failure("detect MongoDB topology", error))?;
        if hello.get_str("setName").is_err() && hello.get_str("msg").ok() != Some("isdbgrid") {
            return Err(StoreError::MongoTopology);
        }
        let store = Self {
            client,
            records: database.collection("rapid_records"),
            counters: database.collection("rapid_counters"),
        };
        store
            .initialize()
            .await
            .map_err(|error| failure("initialize MongoDB schema", error))?;
        Ok(store)
    }

    async fn initialize(&self) -> MongoResult<()> {
        self.records
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "collection": 1, "_id": 1 })
                    .build(),
            )
            .await?;
        self.counters
            .update_one(
                global(),
                doc! { "$setOnInsert": { "watermark": 0_i64, "revision": 0_i64 } },
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    async fn session(&self) -> Result<ClientSession, StoreError> {
        self.client
            .start_session()
            .await
            .map_err(|error| failure("start MongoDB session", error))
    }

    async fn transfer_rows(&self, session: &mut ClientSession) -> MongoResult<Vec<Row>> {
        let mut cursor = self
            .records
            .find(doc! {})
            .sort(doc! { "_id": 1 })
            .session(&mut *session)
            .await?;
        let mut records = Vec::new();
        while let Some(row) = cursor.next(session).await {
            let row = row?;
            records.push(Row {
                id: checked_id(row.id)?,
                collection: row.collection,
                payload: row.payload,
            });
        }
        Ok(records)
    }

    async fn export(&self, session: &mut ClientSession) -> MongoResult<Snapshot> {
        let records = self.transfer_rows(session).await?;
        let watermark = self
            .watermark(session)
            .await?
            .max(records.last().map_or(0, |row| row.id));
        Ok(Snapshot::new(watermark, records))
    }

    async fn watermark(&self, session: &mut ClientSession) -> MongoResult<u32> {
        let counter = self
            .counters
            .find_one(global())
            .session(session)
            .await?
            .ok_or_else(|| Error::custom(TransferError::Identity))?;
        u32::try_from(counter.watermark).map_err(|_| Error::custom(TransferError::Identity))
    }

    async fn import(&self, snapshot: &Snapshot, session: &mut ClientSession) -> MongoResult<()> {
        // Writes and imports both acquire the same counter document inside their
        // transaction. A conflicting transaction is retried by the driver.
        self.counters.update_one(global(), doc! { "$inc": { "revision": 1_i64 }, "$max": { "watermark": i64::from(snapshot.watermark) } }).session(&mut *session).await?;
        if self
            .records
            .find_one(doc! {})
            .session(&mut *session)
            .await?
            .is_some()
        {
            return Err(Error::custom(TransferError::Occupied));
        }
        if !snapshot.records.is_empty() {
            self.records
                .insert_many(snapshot.records.iter().map(MongoRow::from))
                .session(&mut *session)
                .await?;
        }
        if self.transfer_rows(session).await? != snapshot.records {
            return Err(Error::custom(TransferError::Verify));
        }
        Ok(())
    }

    async fn create(
        &self,
        collection: &str,
        payload: &str,
        session: &mut ClientSession,
    ) -> MongoResult<u32> {
        let mut filter = global();
        filter.insert("watermark", doc! { "$lt": i64::from(u32::MAX) });
        let counter = self
            .counters
            .find_one_and_update(filter, doc! { "$inc": { "watermark": 1_i64 } })
            .return_document(ReturnDocument::After)
            .session(&mut *session)
            .await?
            .ok_or_else(|| Error::custom(TransferError::Identity))?;
        let id = checked_id(counter.watermark)?;
        self.records
            .insert_one(MongoRow {
                id: counter.watermark,
                collection: collection.to_owned(),
                payload: payload.to_owned(),
            })
            .session(session)
            .await?;
        Ok(id)
    }
}

#[async_trait::async_trait]
impl Engine for Mongo {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        self.session()
            .await?
            .start_transaction()
            .read_concern(ReadConcern::snapshot())
            .and_run(self, |session, store| Box::pin(store.export(session)))
            .await
            .map_err(|error| failure("export MongoDB snapshot", error))
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        self.session()
            .await?
            .start_transaction()
            .and_run((self, snapshot), |session, (store, snapshot)| {
                Box::pin(store.import(snapshot, session))
            })
            .await
            .map_err(|error| failure("import MongoDB snapshot", error))
    }

    async fn ready(&self) -> Result<(), StoreError> {
        self.counters
            .find_one(global())
            .await
            .map_err(|error| failure("readiness", error))?
            .map(|_| ())
            .ok_or(StoreError::MongoTopology)
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        let mut cursor = self
            .records
            .find(doc! { "collection": collection, "_id": { "$gt": i64::from(after) } })
            .sort(doc! { "_id": 1 })
            .limit(i64::from(count))
            .await
            .map_err(|error| failure("list", error))?;
        let mut results = Vec::new();
        while cursor
            .advance()
            .await
            .map_err(|error| failure("list", error))?
        {
            let row = cursor
                .deserialize_current()
                .map_err(|error| failure("decode record", error))?;
            results.push(row.stored()?);
        }
        Ok(results)
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        self.records
            .find_one(record(collection, id))
            .await
            .map_err(|error| failure("read", error))?
            .map(MongoRow::stored)
            .transpose()
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        self.session()
            .await?
            .start_transaction()
            .and_run(
                (self, collection, payload),
                |session, (store, name, payload)| Box::pin(store.create(name, payload, session)),
            )
            .await
            .map_err(|error| failure("create", error))
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        let result = self
            .records
            .update_one(
                record(collection, id),
                doc! { "$set": { "payload": payload } },
            )
            .await
            .map_err(|error| failure("replace", error))?;
        Ok(result.matched_count > 0)
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        let result = self
            .records
            .delete_one(record(collection, id))
            .await
            .map_err(|error| failure("delete", error))?;
        Ok(result.deleted_count > 0)
    }
}

impl MongoRow {
    fn stored(self) -> Result<Stored, StoreError> {
        Ok(Stored {
            id: identity(self.id)?,
            payload: self.payload,
        })
    }
}

impl From<&Row> for MongoRow {
    fn from(row: &Row) -> Self {
        Self {
            id: i64::from(row.id),
            collection: row.collection.clone(),
            payload: row.payload.clone(),
        }
    }
}

fn global() -> Document {
    doc! { "_id": "global" }
}

fn record(collection: &str, id: u32) -> Document {
    doc! { "collection": collection, "_id": i64::from(id) }
}

fn checked_id(id: i64) -> MongoResult<u32> {
    identity(id).map_err(|_| Error::custom(TransferError::Identity))
}

fn secure_transport(settings: &mut ClientOptions) -> Result<(), ()> {
    let local = settings.direct_connection == Some(true) && settings.hosts.iter().all(loopback);
    match &settings.tls {
        Some(Tls::Enabled(options)) if options.allow_invalid_certificates == Some(true) => Err(()),
        Some(Tls::Disabled) if !local => Err(()),
        None if !local => {
            settings.tls = Some(TlsOptions::default().into());
            Ok(())
        }
        _ => Ok(()),
    }
}

fn loopback(address: &ServerAddress) -> bool {
    let ServerAddress::Tcp { host, .. } = address else {
        return false;
    };
    host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn failure(operation: &'static str, error: Error) -> StoreError {
    match error.get_custom::<TransferError>() {
        Some(TransferError::Identity) => return StoreError::IdentityRange,
        Some(TransferError::Occupied) => return StoreError::DestinationInUse,
        Some(TransferError::Verify) => {
            return StoreError::Snapshot("destination verification failed");
        }
        None => {}
    }
    let kind = match error.kind.as_ref() {
        ErrorKind::InvalidArgument { .. } => "configuration",
        ErrorKind::ServerSelection { .. } | ErrorKind::Io(_) => "connection unavailable",
        ErrorKind::Authentication { .. } => "authentication failed",
        _ => "MongoDB request failed",
    };
    StoreError::Remote { operation, kind }
}
