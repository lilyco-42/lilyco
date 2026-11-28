//! SQLx snapshot transactions for the portable records table.

use sqlx::{Any, AnyConnection, AnyPool, Transaction};

use crate::data::{Row, Snapshot};

use super::{
    StoreError, identity,
    sqlx_store::{Dialect, remote},
};

pub(super) async fn export(pool: &AnyPool, dialect: Dialect) -> Result<Snapshot, StoreError> {
    let mut tx = begin(pool, dialect, false).await?;
    let records = records(&mut tx).await?;
    let highest = records.last().map_or(0, |record| record.id);
    let sequence = watermark(&mut tx, dialect).await?.max(highest);
    tx.commit()
        .await
        .map_err(|error| remote("commit export", error))?;
    Ok(Snapshot::new(sequence, records))
}

pub(super) async fn import(
    pool: &AnyPool,
    dialect: Dialect,
    snapshot: Snapshot,
) -> Result<(), StoreError> {
    let mut tx = begin(pool, dialect, true).await?;
    if matches!(dialect, Dialect::Postgres) {
        sqlx::raw_sql("LOCK TABLE records IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(|error| remote("lock destination", error))?;
    }
    let first = match dialect {
        Dialect::Mysql => "SELECT id FROM records ORDER BY id LIMIT 1 FOR UPDATE",
        _ => "SELECT id FROM records LIMIT 1",
    };
    if sqlx::query(first)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| remote("check target", error))?
        .is_some()
    {
        return Err(StoreError::DestinationInUse);
    }

    let sequence = snapshot.watermark.max(watermark(&mut tx, dialect).await?);
    for row in &snapshot.records {
        put(&mut tx, dialect, row).await?;
    }
    advance(&mut tx, dialect, sequence).await?;
    if records(&mut tx).await? != snapshot.records {
        return Err(StoreError::Snapshot("destination verification failed"));
    }
    tx.commit()
        .await
        .map_err(|error| remote("commit import", error))
}

async fn begin(
    pool: &AnyPool,
    dialect: Dialect,
    write: bool,
) -> Result<Transaction<'static, Any>, StoreError> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|error| remote("acquire", error))?;
    let command = match (dialect, write) {
        (Dialect::Postgres, false) => "BEGIN ISOLATION LEVEL REPEATABLE READ",
        (Dialect::Sqlite, true) => "BEGIN IMMEDIATE",
        (Dialect::Mysql, _) => {
            sqlx::raw_sql("SET SESSION information_schema_stats_expiry = 0")
                .execute(&mut *connection)
                .await
                .map_err(|error| remote("refresh identity statistics", error))?;
            let isolation = if write {
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"
            } else {
                "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
            };
            sqlx::raw_sql(isolation)
                .execute(&mut *connection)
                .await
                .map_err(|error| remote("set transfer isolation", error))?;
            "START TRANSACTION"
        }
        _ => "BEGIN",
    };
    Transaction::begin(connection, Some(sqlx::SqlStr::from_static(command)))
        .await
        .map_err(|error| remote("begin transfer", error))
}

async fn records(db: &mut AnyConnection) -> Result<Vec<Row>, StoreError> {
    sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, collection, payload FROM records ORDER BY id",
    )
    .fetch_all(db)
    .await
    .map_err(|error| remote("read transfer rows", error))?
    .into_iter()
    .map(|(id, collection, payload)| {
        Ok(Row {
            id: identity(id)?,
            collection,
            payload,
        })
    })
    .collect()
}

async fn watermark(db: &mut AnyConnection, dialect: Dialect) -> Result<u32, StoreError> {
    let sql = match dialect {
        Dialect::Postgres => {
            "SELECT CASE WHEN is_called THEN last_value ELSE last_value - 1 END FROM records_id_seq"
        }
        Dialect::Mysql => {
            "SELECT CAST(COALESCE(AUTO_INCREMENT, 1) AS SIGNED) - 1 FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'records'"
        }
        Dialect::Sqlite => {
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'records'), 0)"
        }
    };
    let (value,): (i64,) = sqlx::query_as(sql)
        .fetch_one(db)
        .await
        .map_err(|error| remote("read identity watermark", error))?;
    u32::try_from(value).map_err(|_| StoreError::IdentityRange)
}

async fn put(db: &mut AnyConnection, dialect: Dialect, row: &Row) -> Result<(), StoreError> {
    let sql = match dialect {
        Dialect::Postgres => "INSERT INTO records (id, collection, payload) VALUES ($1, $2, $3)",
        _ => "INSERT INTO records (id, collection, payload) VALUES (?, ?, ?)",
    };
    sqlx::query(sql)
        .bind(i64::from(row.id))
        .bind(&row.collection)
        .bind(&row.payload)
        .execute(db)
        .await
        .map(|_| ())
        .map_err(|error| remote("insert transfer record", error))
}

async fn advance(
    db: &mut AnyConnection,
    dialect: Dialect,
    watermark: u32,
) -> Result<(), StoreError> {
    if watermark == 0 {
        return Ok(());
    }
    match dialect {
        Dialect::Postgres => {
            // RESTART WITH does not accept bindings. Interpolate a bounded integer only.
            let sql = format!(
                "ALTER TABLE records ALTER COLUMN id RESTART WITH {}",
                u64::from(watermark) + 1
            );
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(db)
                .await
                .map(|_| ())
                .map_err(|error| remote("set identity watermark", error))
        }
        Dialect::Mysql => advance_mysql(db, watermark).await,
        Dialect::Sqlite => advance_sqlite(db, watermark).await,
    }
}

async fn advance_mysql(db: &mut AnyConnection, watermark: u32) -> Result<(), StoreError> {
    // MySQL implicitly commits ALTER TABLE. An explicit temporary ID inside
    // this transaction advances AUTO_INCREMENT while remaining invisible to readers.
    let exists = sqlx::query("SELECT id FROM records WHERE id = ?")
        .bind(i64::from(watermark))
        .fetch_optional(&mut *db)
        .await
        .map_err(|error| remote("check highest ID", error))?
        .is_some();
    if !exists {
        sqlx::query("INSERT INTO records (id, collection, payload) VALUES (?, '__rapid_transfer_mark', '{}')")
            .bind(i64::from(watermark)).execute(&mut *db).await.map_err(|error| remote("advance identity", error))?;
        sqlx::query("DELETE FROM records WHERE id = ?")
            .bind(i64::from(watermark))
            .execute(db)
            .await
            .map_err(|error| remote("remove identity marker", error))?;
    }
    Ok(())
}

async fn advance_sqlite(db: &mut AnyConnection, watermark: u32) -> Result<(), StoreError> {
    let result = sqlx::query("UPDATE sqlite_sequence SET seq = ? WHERE name = 'records'")
        .bind(i64::from(watermark))
        .execute(&mut *db)
        .await
        .map_err(|error| remote("set SQLite watermark", error))?;
    if result.rows_affected() == 0 {
        sqlx::query("INSERT INTO sqlite_sequence (name, seq) VALUES ('records', ?)")
            .bind(i64::from(watermark))
            .execute(db)
            .await
            .map_err(|error| remote("initialize SQLite watermark", error))?;
    }
    Ok(())
}
