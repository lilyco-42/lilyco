#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub use rapid_core::{FieldError, Resource, validate_text};

include!(concat!(env!("OUT_DIR"), "/resources.rs"));