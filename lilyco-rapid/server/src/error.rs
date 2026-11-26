use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rapid_core::FieldError;
use schemars::JsonSchema;
use serde::Serialize;

use crate::store::StoreError;

#[derive(Debug, Serialize, JsonSchema)]
pub struct Problem {
    code: &'static str,
    message: &'static str,
    field: Option<&'static str>,
}

pub enum ApiError {
    Body(JsonRejection),
    Invalid(FieldError),
    InvalidId,
    InvalidQuery,
    NotFound,
    MethodNotAllowed,
    Store(StoreError),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            Self::Body(error) => match error.status() {
                StatusCode::PAYLOAD_TOO_LARGE => (
                    error.status(),
                    "body_too_large",
                    "Reduce the request body size.",
                ),
                StatusCode::UNSUPPORTED_MEDIA_TYPE => (
                    error.status(),
                    "json_required",
                    "Set Content-Type to application/json.",
                ),
                status => (
                    status,
                    "invalid_json",
                    "Send JSON that matches the documented request schema.",
                ),
            },
            Self::Invalid(error) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_field",
                error.reason,
            ),
            Self::InvalidId => (
                StatusCode::BAD_REQUEST,
                "invalid_id",
                "Use a positive 32-bit record ID.",
            ),
            Self::InvalidQuery => (
                StatusCode::BAD_REQUEST,
                "invalid_query",
                "Use a nonnegative 32-bit cursor in the after parameter.",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Check the resource path and record ID.",
            ),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Choose a method listed in /openapi.json.",
            ),
            Self::Store(error) => storage_problem(error),
        }
    }
}

fn storage_problem(error: &StoreError) -> (StatusCode, &'static str, &'static str) {
    use rusqlite::ErrorCode::{DatabaseBusy, DatabaseLocked};
    if matches!(error, StoreError::Sql(rusqlite::Error::SqliteFailure(details, _))
        if matches!(details.code, DatabaseBusy | DatabaseLocked))
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_busy",
            "Retry after the database becomes available.",
        );
    }
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "storage_failure",
        "Check the server logs and database volume.",
    )
}

impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if let Self::Store(error) = &self {
            tracing::error!(%code, %error, "request storage operation failed");
        }
        let field = match self {
            Self::Invalid(error) => Some(error.field),
            _ => None,
        };
        (
            status,
            Json(Problem {
                code,
                message,
                field,
            }),
        )
            .into_response()
    }
}
