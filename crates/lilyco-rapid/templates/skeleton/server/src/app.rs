//! Application endpoints. Register a handler here and its types appear in the
//! existing API reference on the next development reload.

use std::num::NonZeroU16;

use rapid_core::validate_text;
use rapid_server::{Api, ApiError, StartupError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Greeting {
    name: String,
}

#[derive(Serialize, JsonSchema)]
struct Reply {
    message: String,
}

pub fn register(api: &mut Api, max_text_bytes: NonZeroU16) -> Result<(), StartupError> {
    api.get_query("/api/hello", move |input| greet(input, max_text_bytes))
}

async fn greet(input: Greeting, max_text_bytes: NonZeroU16) -> Result<Reply, ApiError> {
    validate_text("name", &input.name, max_text_bytes).map_err(ApiError::Invalid)?;
    Ok(Reply {
        message: format!("Hello, {}!", input.name.trim()),
    })
}