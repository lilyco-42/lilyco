use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rapid_core::Resource;
use rusqlite::{Connection, OptionalExtension, params};
use schemars::JsonSchema;
use serde::Serialize;

use crate::config::Storage;

#[derive(Clone)]
pub struct Store(Arc<Mutex<Connection>>);

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
    #[error("storage lock unavailable")]
    Lock,
    #[error("unsupported database version {0}")]
    Version(u32),
}

impl Store {
    pub fn open(config: &Storage) -> Result<Self, StoreError> {
        if let Some(parent) = config
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(&config.path)?;
        connection.busy_timeout(Duration::from_millis(config.busy_timeout_ms.get().into()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                transaction.execute_batch(include_str!("../migrations/001_records.sql"))?;
                transaction.pragma_update(None, "user_version", 1)?;
            }
            1 => {}
            other => return Err(StoreError::Version(other)),
        }
        transaction.commit()?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    async fn run<T, F>(&self, work: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let connection = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| StoreError::Lock)?;
            work(&connection)
        })
        .await?
    }

    pub async fn ready(&self) -> Result<(), StoreError> {
        self.run(|db| {
            db.prepare("SELECT id, collection, payload FROM records LIMIT 0")?;
            Ok(())
        })
        .await
    }

    pub async fn list<T: Resource>(&self, after: u32, limit: u16) -> Result<Page<T>, StoreError> {
        self.run(move |db| {
            let mut query = db.prepare(
                "SELECT id, payload FROM records WHERE collection = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
            )?;
            let rows = query.query_map(params![T::COLLECTION, after, u32::from(limit) + 1], read_row)?;
            let mut items = rows
                .map(|row| decode(row?))
                .collect::<Result<Vec<_>, _>>()?;
            let next = if items.len() > usize::from(limit) {
                items.pop();
                items.last().map(|row| row.id)
            } else {
                None
            };
            Ok(Page { items, next })
        })
        .await
    }

    pub async fn get<T: Resource>(&self, id: u32) -> Result<Option<Record<T>>, StoreError> {
        self.run(move |db| {
            db.query_row(
                "SELECT id, payload FROM records WHERE collection = ?1 AND id = ?2",
                params![T::COLLECTION, id],
                read_row,
            )
            .optional()?
            .map(decode)
            .transpose()
        })
        .await
    }

    pub async fn insert<T: Resource>(&self, data: T) -> Result<Record<T>, StoreError> {
        self.run(move |db| {
            let id = db.query_row(
                "INSERT INTO records (collection, payload) VALUES (?1, ?2) RETURNING id",
                params![T::COLLECTION, serde_json::to_string(&data)?],
                |row| row.get(0),
            )?;
            Ok(Record { id, data })
        })
        .await
    }

    pub async fn replace<T: Resource>(
        &self,
        id: u32,
        data: T,
    ) -> Result<Option<Record<T>>, StoreError> {
        self.run(move |db| {
            let count = db.execute(
                "UPDATE records SET payload = ?3 WHERE collection = ?1 AND id = ?2",
                params![T::COLLECTION, id, serde_json::to_string(&data)?],
            )?;
            Ok((count > 0).then_some(Record { id, data }))
        })
        .await
    }

    pub async fn delete<T: Resource>(&self, id: u32) -> Result<bool, StoreError> {
        self.run(move |db| {
            let count = db.execute(
                "DELETE FROM records WHERE collection = ?1 AND id = ?2",
                params![T::COLLECTION, id],
            )?;
            Ok(count > 0)
        })
        .await
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(u32, String)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn decode<T: Resource>((id, payload): (u32, String)) -> Result<Record<T>, StoreError> {
    Ok(Record {
        id,
        data: serde_json::from_str(&payload)?,
    })
}
