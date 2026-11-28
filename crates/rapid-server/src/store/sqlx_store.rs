//! Relational hosted backend, using SQLx for engines, connections and binding.
//! Statements here belong to the portable record store's schema.

use std::time::Duration;

use sqlx::{
    AnyConnection, AnyPool, Row,
    any::{AnyPoolOptions, AnyRow},
};

use crate::{
    config::{EngineKind, Storage},
    data::Snapshot,
};

use super::{Engine, StoreError, Stored, environment, identity};

pub struct Sql {
    pool: AnyPool,
    dialect: Dialect,
}

#[derive(Clone, Copy)]
pub(super) enum Dialect {
    Postgres,
    Mysql,
    Sqlite,
}

impl Sql {
    pub async fn connect(config: &Storage) -> Result<Self, StoreError> {
        let (dialect, url) = connection(config)?;
        sqlx::any::install_default_drivers();
        let busy_ms = config.busy_timeout_ms.get();
        let pool = AnyPoolOptions::new()
            .max_connections(config.connection.max_connections.get())
            .acquire_timeout(Duration::from_millis(busy_ms.into()))
            .after_connect(move |db, _| Box::pin(configure_connection(db, dialect, busy_ms)))
            .connect(&url)
            .await
            .map_err(|error| remote("connect", error))?;
        if let Some(sql) = schema_migration(dialect) {
            sqlx::raw_sql(sql)
                .execute(&pool)
                .await
                .map_err(|error| remote("initialize schema", error))?;
        }
        Ok(Self { pool, dialect })
    }

    fn statement(&self, postgres: &'static str, mysql: &'static str) -> &'static str {
        match self.dialect {
            Dialect::Postgres => postgres,
            Dialect::Mysql | Dialect::Sqlite => mysql,
        }
    }

    fn record_query(&self) -> &'static str {
        self.statement(
            "SELECT id, payload FROM records WHERE collection = $1 AND id = $2",
            "SELECT id, payload FROM records WHERE collection = ? AND id = ?",
        )
    }
}

async fn configure_connection(
    db: &mut AnyConnection,
    dialect: Dialect,
    busy_ms: u32,
) -> Result<(), sqlx::Error> {
    if matches!(dialect, Dialect::Sqlite) {
        // The only interpolation is a NonZeroU32 from typed config.
        let sql = format!("PRAGMA busy_timeout = {busy_ms}; PRAGMA synchronous = FULL;");
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(db).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl Engine for Sql {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        super::sqlx_transfer::export(&self.pool, self.dialect).await
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        super::sqlx_transfer::import(&self.pool, self.dialect, snapshot).await
    }

    async fn ready(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT id, collection, payload FROM records LIMIT 0")
            .fetch_all(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| remote("readiness", error))
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        let sql = self.statement(
            "SELECT id, payload FROM records WHERE collection = $1 AND id > $2 ORDER BY id LIMIT $3",
            "SELECT id, payload FROM records WHERE collection = ? AND id > ? ORDER BY id LIMIT ?",
        );
        let rows = sqlx::query(sql)
            .bind(collection)
            .bind(i64::from(after))
            .bind(i64::from(count))
            .fetch_all(&self.pool)
            .await
            .map_err(|error| remote("list", error))?;
        rows.into_iter().map(decode_row).collect()
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        sqlx::query(self.record_query())
            .bind(collection)
            .bind(i64::from(id))
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| remote("read", error))?
            .map(decode_row)
            .transpose()
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| remote("begin create", error))?;
        let id = match self.dialect {
            Dialect::Postgres | Dialect::Sqlite => {
                let sql = self.statement(
                    "INSERT INTO records (collection, payload) VALUES ($1, $2) RETURNING id",
                    "INSERT INTO records (collection, payload) VALUES (?, ?) RETURNING id",
                );
                let row: (i64,) = sqlx::query_as(sql)
                    .bind(collection)
                    .bind(payload)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|error| remote("create", error))?;
                row.0
            }
            Dialect::Mysql => {
                sqlx::query("INSERT INTO records (collection, payload) VALUES (?, ?)")
                    .bind(collection)
                    .bind(payload)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| remote("create", error))?
                    .last_insert_id()
                    .ok_or(StoreError::IdentityRange)?
            }
        };
        let id = identity(id)?;
        tx.commit()
            .await
            .map_err(|error| remote("commit create", error))?;
        Ok(id)
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| remote("begin replace", error))?;
        let sql = self.statement(
            "UPDATE records SET payload = $1 WHERE collection = $2 AND id = $3",
            "UPDATE records SET payload = ? WHERE collection = ? AND id = ?",
        );
        let replaced = sqlx::query(sql)
            .bind(payload)
            .bind(collection)
            .bind(i64::from(id))
            .execute(&mut *tx)
            .await
            .map_err(|error| remote("replace", error))?
            .rows_affected();
        let found = replaced > 0
            || sqlx::query(self.record_query())
                .bind(collection)
                .bind(i64::from(id))
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| remote("check replacement", error))?
                .is_some();
        tx.commit()
            .await
            .map_err(|error| remote("commit replace", error))?;
        Ok(found)
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        let sql = self.statement(
            "DELETE FROM records WHERE collection = $1 AND id = $2",
            "DELETE FROM records WHERE collection = ? AND id = ?",
        );
        Ok(sqlx::query(sql)
            .bind(collection)
            .bind(i64::from(id))
            .execute(&self.pool)
            .await
            .map_err(|error| remote("delete", error))?
            .rows_affected()
            > 0)
    }
}

pub(super) fn connection(config: &Storage) -> Result<(Dialect, String), StoreError> {
    if config.engine == EngineKind::Sqlite {
        return Ok((Dialect::Sqlite, sqlite_url(config)?));
    }
    let key = &config.connection.url_env;
    let url = environment(key)?;
    let dialect = match config.engine {
        EngineKind::Postgres
            if url.starts_with("postgres://") || url.starts_with("postgresql://") =>
        {
            Dialect::Postgres
        }
        EngineKind::Mysql if url.starts_with("mysql://") => Dialect::Mysql,
        _ => return Err(StoreError::InvalidUrl(key.clone())),
    };
    Ok((dialect, url))
}

fn sqlite_url(config: &Storage) -> Result<String, StoreError> {
    // This initializer is also used by the original driver. Keeping it here
    // preserves the SQLite schema, WAL mode and user_version across switches.
    super::sqlite::Sqlite::open(config)?;
    let path = std::path::absolute(&config.path)?;
    let url = url::Url::from_file_path(path).map_err(|()| StoreError::SqlitePath)?;
    let mut encoded = url
        .as_str()
        .strip_prefix("file://")
        .ok_or(StoreError::SqlitePath)?;
    if cfg!(windows) && url.host_str().is_none() && encoded.as_bytes().get(2) == Some(&b':') {
        encoded = encoded.strip_prefix('/').ok_or(StoreError::SqlitePath)?;
    } else if url.host_str().is_some() {
        return Ok(format!("sqlite:////{encoded}"));
    }
    Ok(format!("sqlite://{encoded}"))
}

fn schema_migration(dialect: Dialect) -> Option<&'static str> {
    match dialect {
        Dialect::Postgres => Some(include_str!("../../migrations/001_postgres.sql")),
        Dialect::Mysql => Some(include_str!("../../migrations/001_mysql.sql")),
        Dialect::Sqlite => None,
    }
}

fn decode_row(row: AnyRow) -> Result<Stored, StoreError> {
    let id = row
        .try_get("id")
        .map_err(|error| remote("decode ID", error))?;
    let payload = row
        .try_get("payload")
        .map_err(|error| remote("decode data", error))?;
    Ok(Stored {
        id: identity(id)?,
        payload,
    })
}

pub(super) fn remote(operation: &'static str, error: sqlx::Error) -> StoreError {
    // Keep connection strings, server-provided messages and SQL values out of logs.
    let kind = match error {
        sqlx::Error::Configuration(_) => "configuration",
        sqlx::Error::PoolTimedOut => "pool timeout",
        sqlx::Error::PoolClosed | sqlx::Error::Io(_) | sqlx::Error::Tls(_) => {
            "connection unavailable"
        }
        sqlx::Error::Database(_) => "database error",
        _ => "protocol or codec error",
    };
    StoreError::Remote { operation, kind }
}
