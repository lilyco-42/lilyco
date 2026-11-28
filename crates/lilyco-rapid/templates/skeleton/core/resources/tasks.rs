use alloc::string::String;
use core::num::NonZeroU16;
use serde::{Deserialize, Serialize};

use crate::{FieldError, Resource, validate_text};

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub title: String,
    pub done: bool,
}

impl Resource for Model {
    const COLLECTION: &'static str = "tasks";

    fn validate(&self, max_text_bytes: NonZeroU16) -> Result<(), FieldError> {
        validate_text("title", &self.title, max_text_bytes)
    }
}