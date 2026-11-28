//! Stock SQLite adapter. Uses the existing on-disk schema and ID sequence.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    config::Storage,
    data::{Row, Snapshot},
};

use super::{Engine, StoreError, Stored};

#[derive(Clone)]
pub struct Sqlite(Arc<Mutex<Connection>>);

impl Sqlite {
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
                transaction.execute_batch(include_str!("../../migrations/001_records.sql"))?;
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
}

#[async_trait::async_trait]
impl Engine for Sqlite {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        self.run(|db| {
            let tx = db.unchecked_transaction()?;
            let records = all_records(&tx)?;
            let sequence = watermark(&tx)?;
            tx.commit()?;
            Ok(Snapshot::new(sequence, records))
        })
        .await
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        self.run(move |db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM records LIMIT 1)", [], |row| {
                row.get::<_, bool>(0)
            })? {
                return Err(StoreError::DestinationInUse);
            }
            let sequence = watermark(&tx)?.max(snapshot.watermark);
            for row in &snapshot.records {
                tx.execute(
                    "INSERT INTO records (id, collection, payload) VALUES (?1, ?2, ?3)",
                    params![row.id, row.collection, row.payload],
                )?;
            }
            update_watermark(&tx, sequence)?;
            if all_records(&tx)? != snapshot.records {
                return Err(StoreError::Snapshot("destination verification failed"));
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    async fn ready(&self) -> Result<(), StoreError> {
        self.run(|db| {
            db.prepare("SELECT id, collection, payload FROM records LIMIT 0")?;
            Ok(())
        })
        .await
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        self.run(move |db| {
            let mut query = db.prepare_cached(
                "SELECT id, payload FROM records WHERE collection = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
            )?;
            let rows = query.query_map(params![collection, after, count], read_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        self.run(move |db| {
            Ok(db
                .query_row(
                    "SELECT id, payload FROM records WHERE collection = ?1 AND id = ?2",
                    params![collection, id],
                    read_row,
                )
                .optional()?)
        })
        .await
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        self.run(move |db| {
            Ok(db.query_row(
                "INSERT INTO records (collection, payload) VALUES (?1, ?2) RETURNING id",
                params![collection, payload],
                |row| row.get(0),
            )?)
        })
        .await
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        self.run(move |db| {
            Ok(db.execute(
                "UPDATE records SET payload = ?3 WHERE collection = ?1 AND id = ?2",
                params![collection, id, payload],
            )? > 0)
        })
        .await
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        self.run(move |db| {
            Ok(db.execute(
                "DELETE FROM records WHERE collection = ?1 AND id = ?2",
                params![collection, id],
            )? > 0)
        })
        .await
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Stored> {
    Ok(Stored {
        id: row.get(0)?,
        payload: row.get(1)?,
    })
}

fn watermark(db: &Connection) -> Result<u32, StoreError> {
    Ok(db
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'records'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn update_watermark(db: &Connection, sequence: u32) -> Result<(), StoreError> {
    if sequence == 0 {
        return Ok(());
    }
    let updated = db.execute(
        "UPDATE sqlite_sequence SET seq = ?1 WHERE name = 'records'",
        [sequence],
    )?;
    if updated == 0 {
        db.execute(
            "INSERT INTO sqlite_sequence (name, seq) VALUES ('records', ?1)",
            [sequence],
        )?;
    }
    Ok(())
}

fn all_records(db: &Connection) -> Result<Vec<Row>, StoreError> {
    let mut statement = db.prepare("SELECT id, collection, payload FROM records ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok(Row {
            id: row.get(0)?,
            collection: row.get(1)?,
            payload: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
