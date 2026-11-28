//! Typed HTTP handlers share their request and response types with OpenAPI.

use std::future::Future;

use axum::{
    Json,
    extract::{
        Path, Query,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    routing::{get, post},
};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use utoipa::openapi::{
    Content, Required,
    path::{HttpMethod, OperationBuilder, ParameterIn},
    request_body::RequestBodyBuilder,
    schema::Schema,
};

use crate::{
    Api, ApiError, StartupError,
    error::Problem,
    openapi::{parameter, response, schema},
};

impl Api {
    /// Register a GET handler, using its return type as the response schema.
    pub fn get<O, F, R>(&mut self, path: &str, handler: F) -> Result<(), StartupError>
    where
        O: Serialize + JsonSchema + 'static,
        F: Fn() -> R + Clone + Send + Sync + 'static,
        R: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.check_endpoint(path, false)?;
        let operation = endpoint::<O>("get", path)?;
        self.openapi.endpoint(path, HttpMethod::Get, operation)?;
        let invoke = move || async move { handler().await.map(Json) };
        self.add_endpoint(path, get(invoke));
        Ok(())
    }

    /// Extract query parameters into a Serde struct and document each field.
    pub fn get_query<I, O, F, R>(&mut self, path: &str, handler: F) -> Result<(), StartupError>
    where
        I: DeserializeOwned + JsonSchema + Send + 'static,
        O: Serialize + JsonSchema + 'static,
        F: Fn(I) -> R + Clone + Send + Sync + 'static,
        R: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.check_endpoint(path, false)?;
        let operation = parameters::<I>(endpoint::<O>("get", path)?, ParameterIn::Query, path)?;
        self.openapi.endpoint(path, HttpMethod::Get, operation)?;
        let invoke = move |input: Result<Query<I>, QueryRejection>| async move {
            let Query(value) = input.map_err(|_| ApiError::EndpointQuery)?;
            handler(value).await.map(Json)
        };
        self.add_endpoint(path, get(invoke));
        Ok(())
    }

    /// Extract path parameters into a Serde struct with matching field names.
    pub fn get_path<I, O, F, R>(&mut self, path: &str, handler: F) -> Result<(), StartupError>
    where
        I: DeserializeOwned + JsonSchema + Send + 'static,
        O: Serialize + JsonSchema + 'static,
        F: Fn(I) -> R + Clone + Send + Sync + 'static,
        R: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.check_endpoint(path, true)?;
        let operation = parameters::<I>(endpoint::<O>("get", path)?, ParameterIn::Path, path)?;
        self.openapi.endpoint(path, HttpMethod::Get, operation)?;
        let invoke = move |input: Result<Path<I>, PathRejection>| async move {
            let Path(value) = input.map_err(|_| ApiError::EndpointPath)?;
            handler(value).await.map(Json)
        };
        self.add_endpoint(path, get(invoke));
        Ok(())
    }

    /// Accept and return typed JSON. Malformed, oversized and mistyped bodies
    /// use the same error format as generated resources.
    pub fn post_json<I, O, F, R>(&mut self, path: &str, handler: F) -> Result<(), StartupError>
    where
        I: DeserializeOwned + JsonSchema + Send + 'static,
        O: Serialize + JsonSchema + 'static,
        F: Fn(I) -> R + Clone + Send + Sync + 'static,
        R: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.check_endpoint(path, false)?;
        let body = RequestBodyBuilder::new()
            .required(Some(Required::True))
            .content("application/json", Content::new(Some(schema::<I>()?)))
            .build();
        let operation = endpoint::<O>("post", path)?
            .request_body(Some(body))
            .response("413", response("Body is too large", schema::<Problem>()?))
            .response(
                "415",
                response("JSON content type required", schema::<Problem>()?),
            )
            .response("422", response("Invalid body", schema::<Problem>()?));
        self.openapi.endpoint(path, HttpMethod::Post, operation)?;
        let invoke = move |input: Result<Json<I>, JsonRejection>| async move {
            let Json(value) = input.map_err(ApiError::Body)?;
            handler(value).await.map(Json)
        };
        self.add_endpoint(path, post(invoke));
        Ok(())
    }

    fn add_endpoint(
        &mut self,
        path: &str,
        handler: axum::routing::MethodRouter<crate::routes::State>,
    ) {
        self.router = std::mem::take(&mut self.router).route(path, handler);
    }

    fn check_endpoint(&self, path: &str, dynamic: bool) -> Result<(), StartupError> {
        let Some(rest) = path.strip_prefix("/api/") else {
            return Err(StartupError::InvalidEndpoint(path.to_owned()));
        };
        let mut found_parameter = false;
        for segment in rest.split('/') {
            let segment = if let Some(name) = placeholder(segment) {
                found_parameter = true;
                name
            } else {
                segment
            };
            if segment.is_empty() || !segment.bytes().all(is_segment_byte) {
                return Err(StartupError::InvalidEndpoint(path.to_owned()));
            }
        }
        if found_parameter != dynamic {
            return Err(StartupError::InvalidEndpoint(path.to_owned()));
        }
        Ok(())
    }
}

fn is_segment_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
}

pub(crate) fn placeholder(segment: &str) -> Option<&str> {
    segment.strip_prefix('{')?.strip_suffix('}')
}

fn endpoint<O: JsonSchema>(method: &str, path: &str) -> Result<OperationBuilder, StartupError> {
    Ok(OperationBuilder::new()
        .operation_id(Some(format!("{method}:{path}")))
        .response("200", response("Successful response", schema::<O>()?))
        .response("400", response("Invalid request", schema::<Problem>()?))
        .response("422", response("Failed validation", schema::<Problem>()?)))
}

fn parameters<I: JsonSchema>(
    mut operation: OperationBuilder,
    location: ParameterIn,
    path: &str,
) -> Result<OperationBuilder, StartupError> {
    let Schema::Object(input) = schema::<I>()? else {
        return Err(StartupError::InvalidParameters(path.to_owned()));
    };
    if location == ParameterIn::Path {
        let mut names = std::collections::BTreeSet::new();
        for name in path.split('/').filter_map(placeholder) {
            if !names.insert(name) || !input.properties.contains_key(name) {
                return Err(StartupError::InvalidParameters(path.to_owned()));
            }
        }
        if names.len() != input.properties.len() {
            return Err(StartupError::InvalidParameters(path.to_owned()));
        }
    }
    for (name, property) in input.properties {
        let required = location == ParameterIn::Path || input.required.contains(&name);
        operation = operation.parameter(parameter(&name, location.clone(), property, required));
    }
    Ok(operation)
}
