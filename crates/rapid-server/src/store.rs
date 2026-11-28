//! Portable record operations used by generated CRUD routes. SQL execution
//! and database-specific identity allocation are owned by each adapter.

use std::sync::Arc;

use rapid_core::Resource;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    config::{Driver, EngineKind, Storage},
    data::Snapshot,
};

#[cfg(any(feature = "turso", feature = "surrealdb"))]
mod http;
#[cfg(feature = "mongodb")]
mod mongo;
#[cfg(feature = "seaorm")]
mod seaorm;
mod sqlite;
#[cfg(feature = "sqlx-server")]
mod sqlx_store;
#[cfg(feature = "sqlx-server")]
mod sqlx_transfer;
#[cfg(feature = "surrealdb")]
mod surreal;
#[cfg(feature = "turso")]
mod turso;

#[derive(Clone)]
pub struct Store(Arc<dyn Engine>);

#[derive(Debug, Serialize, JsonSchema)]
pub struct Record<T> {
    pub id: u32,
    pub data: T,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Page<T> {
    pub items: Vec<Record<T>>,
    pub next: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite request failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("record codec failed: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("storage task failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
    #[error("invalid transfer snapshot: {0}")]
    Snapshot(&'static str),
    #[error("destination already contains Rapid records; choose an empty database")]
    DestinationInUse,
    #[error("storage lock unavailable")]
    Lock,
    #[error("unsupported database version {0}")]
    Version(u32),
    #[error("the configured database engine and driver are incompatible")]
    IncompatibleDriver,
    #[error("the SeaORM driver requires the seaorm Cargo feature")]
    SeaormFeature,
    #[error("storage engine {0} requires the sqlx-server Cargo feature")]
    Feature(EngineKind),
    #[error("storage engine {0} requires asynchronous initialization")]
    RequiresConnect(EngineKind),
    #[cfg(any(
        feature = "sqlx-server",
        feature = "turso",
        feature = "mongodb",
        feature = "surrealdb"
    ))]
    #[error(
        "database environment variable name must contain only uppercase ASCII letters, digits and underscores"
    )]
    InvalidVariable,
    #[cfg(any(
        feature = "sqlx-server",
        feature = "turso",
        feature = "mongodb",
        feature = "surrealdb"
    ))]
    #[error("database environment variable {0} is missing or empty")]
    MissingVariable(String),
    #[cfg(any(
        feature = "sqlx-server",
        feature = "turso",
        feature = "mongodb",
        feature = "surrealdb"
    ))]
    #[error("database URL in {0} does not match the configured engine")]
    InvalidUrl(String),
    #[cfg(any(feature = "turso", feature = "surrealdb"))]
    #[error("database token in {0} contains invalid characters")]
    InvalidToken(String),
    #[cfg(not(feature = "turso"))]
    #[error("the Turso engine requires the turso Cargo feature")]
    TursoFeature,
    #[cfg(not(feature = "mongodb"))]
    #[error("the MongoDB engine requires the mongodb Cargo feature")]
    MongoFeature,
    #[cfg(feature = "mongodb")]
    #[error("MongoDB requires a named database on a replica set or sharded cluster")]
    MongoTopology,
    #[cfg(not(feature = "surrealdb"))]
    #[error("the SurrealDB engine requires the surrealdb Cargo feature")]
    SurrealFeature,
    #[cfg(any(
        feature = "sqlx-server",
        feature = "turso",
        feature = "mongodb",
        feature = "surrealdb"
    ))]
    #[error("database {operation} failed ({kind})")]
    Remote {
        operation: &'static str,
        kind: &'static str,
    },
    #[cfg(any(
        feature = "sqlx-server",
        feature = "turso",
        feature = "mongodb",
        feature = "surrealdb"
    ))]
    #[error("database identity exceeds the supported record ID range")]
    IdentityRange,
    #[cfg(feature = "sqlx-server")]
    #[error("cannot convert SQLite file path to a connection URL")]
    SqlitePath,
}

struct Stored {
    id: u32,
    payload: String,
}

#[async_trait::async_trait]
trait Engine: Send + Sync {
    async fn snapshot(&self) -> Result<Snapshot, StoreError>;
    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError>;
    async fn ready(&self) -> Result<(), StoreError>;
    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError>;
    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError>;
    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError>;
    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError>;
    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError>;
}

impl Store {
    pub fn open(config: &Storage) -> Result<Self, StoreError> {
        if config.engine != EngineKind::Sqlite || config.driver() != Driver::Rusqlite {
            return Err(StoreError::RequiresConnect(config.engine));
        }
        Ok(Self(Arc::new(sqlite::Sqlite::open(config)?)))
    }

    pub async fn connect(config: &Storage) -> Result<Self, StoreError> {
        match (config.engine, config.driver()) {
            (EngineKind::Sqlite, Driver::Rusqlite) => Self::open(config),
            #[cfg(feature = "turso")]
            (EngineKind::Turso, Driver::Libsql) => {
                Ok(Self(Arc::new(turso::Turso::connect(config).await?)))
            }
            #[cfg(not(feature = "turso"))]
            (EngineKind::Turso, Driver::Libsql) => Err(StoreError::TursoFeature),
            #[cfg(feature = "mongodb")]
            (EngineKind::Mongodb, Driver::Mongodb) => {
                Ok(Self(Arc::new(mongo::Mongo::connect(config).await?)))
            }
            #[cfg(not(feature = "mongodb"))]
            (EngineKind::Mongodb, Driver::Mongodb) => Err(StoreError::MongoFeature),
            #[cfg(feature = "surrealdb")]
            (EngineKind::Surrealdb, Driver::Surrealdb) => {
                Ok(Self(Arc::new(surreal::Surreal::connect(config).await?)))
            }
            #[cfg(not(feature = "surrealdb"))]
            (EngineKind::Surrealdb, Driver::Surrealdb) => Err(StoreError::SurrealFeature),
            (EngineKind::Turso | EngineKind::Mongodb | EngineKind::Surrealdb, _)
            | (_, Driver::Rusqlite | Driver::Libsql | Driver::Mongodb | Driver::Surrealdb) => {
                Err(StoreError::IncompatibleDriver)
            }
            #[cfg(feature = "seaorm")]
            (_, Driver::Seaorm) => Ok(Self(Arc::new(seaorm::Orm::connect(config).await?))),
            #[cfg(not(feature = "seaorm"))]
            (_, Driver::Seaorm) => Err(StoreError::SeaormFeature),
            #[cfg(feature = "sqlx-server")]
            _ => Ok(Self(Arc::new(sqlx_store::Sql::connect(config).await?))),
            #[cfg(not(feature = "sqlx-server"))]
            (engine, _) => Err(StoreError::Feature(engine)),
        }
    }

    pub(crate) async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        self.0.snapshot().await
    }

    pub(crate) async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        self.0.restore(snapshot).await
    }

    pub async fn ready(&self) -> Result<(), StoreError> {
        self.0.ready().await
    }

    pub async fn list<T: Resource>(&self, after: u32, limit: u16) -> Result<Page<T>, StoreError> {
        let mut rows = self
            .0
            .list(T::COLLECTION, after, u32::from(limit) + 1)
            .await?;
        let next = if rows.len() > usize::from(limit) {
            rows.pop();
            rows.last().map(|row| row.id)
        } else {
            None
        };
        let items = rows.into_iter().map(decode).collect::<Result<_, _>>()?;
        Ok(Page { items, next })
    }

    pub async fn get<T: Resource>(&self, id: u32) -> Result<Option<Record<T>>, StoreError> {
        self.0.get(T::COLLECTION, id).await?.map(decode).transpose()
    }

    pub async fn insert<T: Resource>(&self, data: T) -> Result<Record<T>, StoreError> {
        let id = self
            .0
            .insert(T::COLLECTION, serde_json::to_string(&data)?)
            .await?;
        Ok(Record { id, data })
    }

    pub async fn replace<T: Resource>(
        &self,
        id: u32,
        data: T,
    ) -> Result<Option<Record<T>>, StoreError> {
        let found = self
            .0
            .replace(T::COLLECTION, id, serde_json::to_string(&data)?)
            .await?;
        Ok(found.then_some(Record { id, data }))
    }

    pub async fn delete<T: Resource>(&self, id: u32) -> Result<bool, StoreError> {
        self.0.delete(T::COLLECTION, id).await
    }
}

#[cfg(any(
    feature = "sqlx-server",
    feature = "turso",
    feature = "mongodb",
    feature = "surrealdb"
))]
fn environment(key: &str) -> Result<String, StoreError> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(StoreError::InvalidVariable);
    }
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| StoreError::MissingVariable(key.to_owned()))
}

#[cfg(any(
    feature = "sqlx-server",
    feature = "turso",
    feature = "mongodb",
    feature = "surrealdb"
))]
fn identity(value: i64) -> Result<u32, StoreError> {
    u32::try_from(value)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(StoreError::IdentityRange)
}

fn decode<T: Resource>(row: Stored) -> Result<Record<T>, StoreError> {
    Ok(Record {
        id: row.id,
        data: serde_json::from_str(&row.payload)?,
    })
}
