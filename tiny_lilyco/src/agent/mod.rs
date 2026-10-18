//! The agent layer: tool surface, registry and the OODA loop.
//!
//! Everything here is allocation-free and `no_std`-clean, so the exact same
//! code drives the desktop agent, the CLI and the MCP server.

pub mod ooda;
pub mod registry;
pub mod tool;

pub use ooda::{Action, ActPermit, Machine, Phase, TransitionError};
pub use registry::{Registry, MAX_TOOLS};
pub use tool::{LogStream, ObsBuf, ObsKind, Tool, ToolError, ToolSpec, OBS_BUF_LEN};
