//! 项目 / 方案：本机 SQLite 持久化的轻量计划数据。
//!
//! 数据形状刻意保持扁平，方便面板直接渲染、脚本直接断言：
//!
//! ```text
//! project { id, name, description, status: active|paused|done,
//!           created_at, updated_at, plans: [plan] }
//! plan    { id, project_id, title, detail, status: draft|doing|done,
//!           created_at, updated_at }
//! ```
//!
//! 所有写入口都在 `main.rs` 的 projects 路由中间件之后；本模块只做校验与
//! 数据库操作，不接触 HTTP 请求头。状态码作为契约钉死：校验失败 400、
//! 目标不存在 404、数据库错误 500。

use axum::http::StatusCode;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROJECT_STATUSES: [&str; 3] = ["active", "paused", "done"];
pub const PLAN_STATUSES: [&str; 3] = ["draft", "doing", "done"];

const MAX_NAME: usize = 120;
const MAX_DESCRIPTION: usize = 2000;
const MAX_TITLE: usize = 160;
const MAX_DETAIL: usize = 4000;

pub mod project {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub description: String,
        pub status: String,
        pub created_at: String,
        pub updated_at: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod plan {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "plans")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub project_id: i32,
        pub title: String,
        pub detail: String,
        pub status: String,
        pub created_at: String,
        pub updated_at: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[derive(Debug, Deserialize)]
pub struct ProjectInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlanInput {
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlanPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// 建表在连接时执行，测试内存库与真实 `connect_db` 走同一份 DDL。
pub async fn ensure_tables(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .await?;
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS plans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'draft',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .await?;
    db.execute_unprepared("CREATE INDEX IF NOT EXISTS plans_project_id ON plans (project_id);")
        .await?;
    Ok(())
}

pub async fn list(db: &DatabaseConnection) -> Result<Value, StatusCode> {
    let projects = project::Entity::find()
        .order_by_desc(project::Column::Id)
        .all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let plans = plan::Entity::find()
        .order_by_asc(plan::Column::Id)
        .all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items = projects
        .into_iter()
        .map(|row| {
            let nested: Vec<Value> = plans
                .iter()
                .filter(|plan| plan.project_id == row.id)
                .map(plan_json)
                .collect();
            project_json(row, nested)
        })
        .collect::<Vec<_>>();
    Ok(json!({"projects": items, "count": items.len()}))
}

pub async fn get(db: &DatabaseConnection, id: i32) -> Result<Value, StatusCode> {
    let row = find_project(db, id).await?;
    let plans = plans_of(db, id).await?;
    Ok(json!({"project": project_json(row, plans)}))
}

pub async fn create(db: &DatabaseConnection, input: ProjectInput) -> Result<Value, StatusCode> {
    let name = clean_required(&input.name, MAX_NAME)?;
    let description = clean_optional(input.description.as_deref(), MAX_DESCRIPTION)?;
    let status = clean_status(input.status.as_deref(), &PROJECT_STATUSES, "active")?;
    let now = now_millis();
    let inserted = project::ActiveModel {
        name: Set(name),
        description: Set(description),
        status: Set(status),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(json!({"project": project_json(inserted, Vec::new())}))
}

pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    patch: ProjectPatch,
) -> Result<Value, StatusCode> {
    let row = find_project(db, id).await?;
    let mut active: project::ActiveModel = row.into();
    if let Some(name) = patch.name.as_deref() {
        active.name = Set(clean_required(name, MAX_NAME)?);
    }
    if let Some(description) = patch.description.as_deref() {
        active.description = Set(clean_optional(Some(description), MAX_DESCRIPTION)?);
    }
    if let Some(status) = patch.status.as_deref() {
        active.status = Set(clean_status(Some(status), &PROJECT_STATUSES, "active")?);
    }
    active.updated_at = Set(now_millis());
    let updated = active
        .update(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let plans = plans_of(db, id).await?;
    Ok(json!({"project": project_json(updated, plans)}))
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<Value, StatusCode> {
    find_project(db, id).await?;
    let deleted_plans = plan::Entity::delete_many()
        .filter(plan::Column::ProjectId.eq(id))
        .exec(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .rows_affected;
    project::Entity::delete_by_id(id)
        .exec(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(json!({"ok": true, "deleted_plans": deleted_plans}))
}

pub async fn create_plan(
    db: &DatabaseConnection,
    project_id: i32,
    input: PlanInput,
) -> Result<Value, StatusCode> {
    find_project(db, project_id).await?;
    let title = clean_required(&input.title, MAX_TITLE)?;
    let detail = clean_optional(input.detail.as_deref(), MAX_DETAIL)?;
    let status = clean_status(input.status.as_deref(), &PLAN_STATUSES, "draft")?;
    let now = now_millis();
    let inserted = plan::ActiveModel {
        project_id: Set(project_id),
        title: Set(title),
        detail: Set(detail),
        status: Set(status),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    touch_project(db, project_id).await?;
    Ok(json!({"plan": plan_json(&inserted)}))
}

pub async fn update_plan(
    db: &DatabaseConnection,
    id: i32,
    patch: PlanPatch,
) -> Result<Value, StatusCode> {
    let row = plan::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let project_id = row.project_id;
    let mut active: plan::ActiveModel = row.into();
    if let Some(title) = patch.title.as_deref() {
        active.title = Set(clean_required(title, MAX_TITLE)?);
    }
    if let Some(detail) = patch.detail.as_deref() {
        active.detail = Set(clean_optional(Some(detail), MAX_DETAIL)?);
    }
    if let Some(status) = patch.status.as_deref() {
        active.status = Set(clean_status(Some(status), &PLAN_STATUSES, "draft")?);
    }
    active.updated_at = Set(now_millis());
    let updated = active
        .update(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    touch_project(db, project_id).await?;
    Ok(json!({"plan": plan_json(&updated)}))
}

pub async fn delete_plan(db: &DatabaseConnection, id: i32) -> Result<Value, StatusCode> {
    let row = plan::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let project_id = row.project_id;
    plan::Entity::delete_by_id(id)
        .exec(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    touch_project(db, project_id).await?;
    Ok(json!({"ok": true}))
}

async fn find_project(db: &DatabaseConnection, id: i32) -> Result<project::Model, StatusCode> {
    project::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
}

async fn plans_of(db: &DatabaseConnection, project_id: i32) -> Result<Vec<Value>, StatusCode> {
    let rows = plan::Entity::find()
        .filter(plan::Column::ProjectId.eq(project_id))
        .order_by_asc(plan::Column::Id)
        .all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(rows.iter().map(plan_json).collect())
}

async fn touch_project(db: &DatabaseConnection, id: i32) -> Result<(), StatusCode> {
    let Some(row) = project::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Ok(());
    };
    let mut active: project::ActiveModel = row.into();
    active.updated_at = Set(now_millis());
    active
        .update(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

fn project_json(row: project::Model, plans: Vec<Value>) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "description": row.description,
        "status": row.status,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "plans": plans,
    })
}

fn plan_json(row: &plan::Model) -> Value {
    json!({
        "id": row.id,
        "project_id": row.project_id,
        "title": row.title,
        "detail": row.detail,
        "status": row.status,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

fn clean_required(raw: &str, limit: usize) -> Result<String, StatusCode> {
    let value = raw.trim();
    if value.is_empty() || value.chars().count() > limit {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(value.to_string())
}

fn clean_optional(raw: Option<&str>, limit: usize) -> Result<String, StatusCode> {
    let value = raw.unwrap_or("").trim();
    if value.chars().count() > limit {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(value.to_string())
}

fn clean_status(raw: Option<&str>, allowed: &[&str], default: &str) -> Result<String, StatusCode> {
    let value = raw.unwrap_or(default).trim();
    if !allowed.contains(&value) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(value.to_string())
}

fn now_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_length_validation_is_explicit() {
        assert_eq!(clean_required("  ", 10), Err(StatusCode::BAD_REQUEST));
        assert_eq!(
            clean_required(&"x".repeat(11), 10),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(clean_required(" 发布 ", 10).unwrap(), "发布");
        assert_eq!(clean_optional(Some("  "), 4).unwrap(), "");
        assert_eq!(
            clean_optional(Some("12345"), 4),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            clean_status(None, &PROJECT_STATUSES, "active").unwrap(),
            "active"
        );
        assert_eq!(
            clean_status(Some("blocked"), &PROJECT_STATUSES, "active"),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            clean_status(Some("done"), &PROJECT_STATUSES, "active").unwrap(),
            "done"
        );
    }
}
