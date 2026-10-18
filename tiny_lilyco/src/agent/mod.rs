//! The agent layer: decision, tool surface, policy, and the loop that joins them.
//!
//! Everything here is allocation-free and `no_std`-clean, so the exact same code
//! drives the desktop app, the CLI and the MCP server.

pub mod llm;
pub mod ooda;
pub mod registry;
pub mod runtime;
pub mod tool;

#[cfg(feature = "std")]
pub mod app;
#[cfg(feature = "std")]
pub mod tools;

#[cfg(feature = "std")]
pub use app::AgentApp;

pub use llm::{Call, IdleLlm, Llm, ScriptedLlm, Step, MAX_ARGS};
pub use ooda::{ActPermit, Action, Machine, Phase, TransitionError};
pub use registry::{Registry, MAX_TOOLS};
pub use runtime::{drive, Budget, Host, Outcome};
pub use tool::{LogStream, ObsBuf, ObsKind, Tool, ToolError, ToolSpec, OBS_BUF_LEN};
