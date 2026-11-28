#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use core::{fmt, num::NonZeroU16};
use lilyco_no_std::prelude::CommandSchema;
use serde::{Serialize, de::DeserializeOwned};

pub trait Resource: Serialize + DeserializeOwned + Send + Sync + 'static {
    const COLLECTION: &'static str;

    fn validate(&self, max_text_bytes: NonZeroU16) -> Result<(), FieldError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldError {
    pub field: &'static str,
    pub reason: &'static str,
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

impl core::error::Error for FieldError {}

pub fn validate_text(
    field: &'static str,
    value: &str,
    limit: NonZeroU16,
) -> Result<(), FieldError> {
    let schema = CommandSchema::new(field, usize::from(limit.get()));
    schema
        .validate_input(value)
        .and_then(|()| schema.validate_input(value.trim()))
        .map_err(|reason| FieldError { field, reason })
}

include!(concat!(env!("OUT_DIR"), "/resources.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lilyco_validation_counts_utf8_bytes_and_rejects_blank_and_control_text() {
        let limit = NonZeroU16::new(6).unwrap();
        assert!(validate_text("title", "小猫", limit).is_ok());
        for title in ["小猫a", "  ", "A\u{7}", "  小猫"] {
            assert!(validate_text("title", title, limit).is_err());
        }
    }
}
