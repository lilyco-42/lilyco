//! SeaORM entity adapter for Rapid's portable record format.

use std::time::Duration;

use sea_orm::{
    ActiveValue::NotSet, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DbErr,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait, sea_query::Expr,
};

use crate::{config::Storage, data::Snapshot};

use super::{
    Engine, StoreError, Stored,
    sqlx_store::{Sql, connection, identity},
};

mod entity {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "records")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub collection: String,
        pub payload: String,
    }

    #[derive(Clone, Copy, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

use entity::{ActiveModel, Column, Entity, Model};

pub struct Orm {
    database: DatabaseConnection,
    options: Storage,
}

impl Orm {
    pub async fn connect(storage: &Storage) -> Result<Self, StoreError> {
        // Initialize the same physical schema used by the SQLx and legacy drivers.
        drop(Sql::connect(storage).await?);
        let (_, url) = connection(storage)?;
        let deadline = Duration::from_millis(storage.busy_timeout_ms.get().into());
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(storage.connection.max_connections.get())
            .connect_timeout(deadline)
            .acquire_timeout(deadline)
            .sqlx_logging(false)
            .map_sqlx_sqlite_opts(move |options| {
                options
                    .busy_timeout(deadline)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
            });
        let database = Database::connect(options)
            .await
            .map_err(|error| failure("connect", error))?;
        Ok(Self {
            database,
            options: storage.clone(),
        })
    }
}

#[async_trait::async_trait]
impl Engine for Orm {
    async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        Sql::connect(&self.options).await?.snapshot().await
    }

    async fn restore(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        Sql::connect(&self.options).await?.restore(snapshot).await
    }

    async fn ready(&self) -> Result<(), StoreError> {
        Entity::find()
            .limit(1)
            .one(&self.database)
            .await
            .map(|_| ())
            .map_err(|error| failure("readiness", error))
    }

    async fn list(
        &self,
        collection: &'static str,
        after: u32,
        count: u32,
    ) -> Result<Vec<Stored>, StoreError> {
        Entity::find()
            .filter(Column::Collection.eq(collection))
            .filter(Column::Id.gt(i64::from(after)))
            .order_by_asc(Column::Id)
            .limit(u64::from(count))
            .all(&self.database)
            .await
            .map_err(|error| failure("list", error))?
            .into_iter()
            .map(stored)
            .collect()
    }

    async fn get(&self, collection: &'static str, id: u32) -> Result<Option<Stored>, StoreError> {
        row(collection, id)
            .one(&self.database)
            .await
            .map_err(|error| failure("read", error))?
            .map(stored)
            .transpose()
    }

    async fn insert(&self, collection: &'static str, payload: String) -> Result<u32, StoreError> {
        let tx = self
            .database
            .begin()
            .await
            .map_err(|error| failure("begin create", error))?;
        let result = Entity::insert(ActiveModel {
            id: NotSet,
            collection: Set(collection.to_owned()),
            payload: Set(payload),
        })
        .exec(&tx)
        .await
        .map_err(|error| failure("create", error))?;
        let id = identity(result.last_insert_id)?;
        tx.commit()
            .await
            .map_err(|error| failure("commit create", error))?;
        Ok(id)
    }

    async fn replace(
        &self,
        collection: &'static str,
        id: u32,
        payload: String,
    ) -> Result<bool, StoreError> {
        let tx = self
            .database
            .begin()
            .await
            .map_err(|error| failure("begin replace", error))?;
        let changed = Entity::update_many()
            .col_expr(Column::Payload, Expr::value(payload))
            .filter(Column::Collection.eq(collection))
            .filter(Column::Id.eq(i64::from(id)))
            .exec(&tx)
            .await
            .map_err(|error| failure("replace", error))?
            .rows_affected;
        let found = changed > 0
            || row(collection, id)
                .one(&tx)
                .await
                .map_err(|error| failure("check replacement", error))?
                .is_some();
        tx.commit()
            .await
            .map_err(|error| failure("commit replace", error))?;
        Ok(found)
    }

    async fn delete(&self, collection: &'static str, id: u32) -> Result<bool, StoreError> {
        let removed = Entity::delete_many()
            .filter(Column::Collection.eq(collection))
            .filter(Column::Id.eq(i64::from(id)))
            .exec(&self.database)
            .await
            .map_err(|error| failure("delete", error))?;
        Ok(removed.rows_affected > 0)
    }
}

fn row(collection: &'static str, id: u32) -> sea_orm::Select<Entity> {
    Entity::find_by_id(i64::from(id)).filter(Column::Collection.eq(collection))
}

fn stored(model: Model) -> Result<Stored, StoreError> {
    Ok(Stored {
        id: identity(model.id)?,
        payload: model.payload,
    })
}

fn failure(operation: &'static str, error: DbErr) -> StoreError {
    let kind = match error {
        DbErr::Conn(_) | DbErr::ConnectionAcquire(_) => "connection unavailable",
        DbErr::Query(_) | DbErr::Exec(_) => "database error",
        _ => "protocol or codec error",
    };
    StoreError::Remote { operation, kind }
}
