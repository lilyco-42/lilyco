use axum::{
    Json,
    extract::{
        Path, Query, State as AxumState,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use rapid_core::Resource;
use serde::Deserialize;

use crate::{
    config::Resources,
    error::ApiError,
    store::{Page, Record, Store},
};

#[derive(Clone)]
pub struct State {
    pub store: Store,
    pub rules: Resources,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    #[serde(default)]
    after: u32,
}

pub async fn ready(AxumState(state): AxumState<State>) -> Result<StatusCode, ApiError> {
    state.store.ready().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn id(path: Result<Path<u32>, PathRejection>) -> Result<u32, ApiError> {
    let Path(id) = path.map_err(|_| ApiError::InvalidId)?;
    if id == 0 {
        return Err(ApiError::InvalidId);
    }
    Ok(id)
}

fn validated<T: Resource>(
    payload: Result<Json<T>, JsonRejection>,
    rules: Resources,
) -> Result<T, ApiError> {
    let Json(data) = payload.map_err(ApiError::Body)?;
    data.validate(rules.max_text_bytes)
        .map_err(ApiError::Invalid)?;
    Ok(data)
}

pub async fn list<T: Resource>(
    AxumState(state): AxumState<State>,
    params: Result<Query<Cursor>, QueryRejection>,
) -> Result<Json<Page<T>>, ApiError> {
    let Query(cursor) = params.map_err(|_| ApiError::InvalidQuery)?;
    Ok(Json(
        state
            .store
            .list(cursor.after, state.rules.page_size.get())
            .await?,
    ))
}

pub async fn read<T: Resource>(
    AxumState(state): AxumState<State>,
    path: Result<Path<u32>, PathRejection>,
) -> Result<Json<Record<T>>, ApiError> {
    state
        .store
        .get(id(path)?)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn create<T: Resource>(
    AxumState(state): AxumState<State>,
    payload: Result<Json<T>, JsonRejection>,
) -> Result<(StatusCode, Json<Record<T>>), ApiError> {
    let data = validated(payload, state.rules)?;
    let row = state.store.insert(data).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn replace<T: Resource>(
    AxumState(state): AxumState<State>,
    path: Result<Path<u32>, PathRejection>,
    payload: Result<Json<T>, JsonRejection>,
) -> Result<Json<Record<T>>, ApiError> {
    let id = id(path)?;
    let data = validated(payload, state.rules)?;
    state
        .store
        .replace(id, data)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn delete<T: Resource>(
    AxumState(state): AxumState<State>,
    path: Result<Path<u32>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    if !state.store.delete::<T>(id(path)?).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
