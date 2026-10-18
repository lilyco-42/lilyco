//! Adapters: pure planners for every external system the agent touches.
//!
//! Each submodule renders commands or frames and validates input. Only [`host`]
//! (behind the `std` feature) is allowed to execute anything.

pub mod browser_control;
pub mod fs_engine;
pub mod gui;
pub mod image_inspect;
pub mod mcp_uda;
pub mod uda;
pub mod vcs_guard;

#[cfg(feature = "std")]
pub mod host;
