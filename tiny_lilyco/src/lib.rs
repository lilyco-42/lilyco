//! `tiny_lilyco` — a `no_std`, allocation-free agent core.
//!
//! One struct describes an app; one tool surface describes what the agent may
//! do; one OODA machine decides when a checkpoint is mandatory. The same code
//! drives the CLI, the desktop shell, the web UI and the MCP server.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod adapters;
pub mod agent;
pub mod core;
pub mod prelude;
pub mod ui;

use crate::core::progress::Progress;
use crate::core::schema::CommandSchema;

/// An application: one declaration, four front-ends.
pub trait TinyApp {
    /// Parsed arguments.
    type Args;
    /// Value returned on success.
    type Output;
    /// Failure type.
    type Error;

    /// Declarative surface shared by CLI, TUI, Web and MCP.
    fn schema() -> CommandSchema;

    /// Validate arguments before any work starts.
    fn validate(args: &Self::Args) -> Result<(), Self::Error>;

    /// Run the app, reporting progress through `progress`.
    fn execute<F>(&self, args: Self::Args, progress: F) -> Result<Self::Output, Self::Error>
    where
        F: FnMut(Progress);
}
