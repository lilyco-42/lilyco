//! Tool surface: the only channel through which an agent may touch the world.

use crate::core::schema::CommandSchema;

/// Upper bound for one observation handed back to the model.
pub const OBS_BUF_LEN: usize = 8 * 1024;

/// Fixed-capacity observation buffer (no heap, no allocator).
pub type ObsBuf = heapless::String<OBS_BUF_LEN>;

/// Which stream a log line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    Console,
}

/// Shape of the payload a tool just wrote into the observation buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsKind {
    Text,
    Diff,
    Console(LogStream),
    Image {
        mime: &'static str,
        width: u32,
        height: u32,
    },
    Exit {
        code: i32,
    },
}

impl ObsKind {
    /// Stable label used in CLI traces and MCP payloads.
    pub const fn label(self) -> &'static str {
        match self {
            ObsKind::Text => "text",
            ObsKind::Diff => "diff",
            ObsKind::Console(LogStream::Stdout) => "stdout",
            ObsKind::Console(LogStream::Stderr) => "stderr",
            ObsKind::Console(LogStream::Console) => "console",
            ObsKind::Image { .. } => "image",
            ObsKind::Exit { .. } => "exit",
        }
    }
}

/// Every way a tool call may refuse or fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolError {
    UnknownTool,
    InvalidArgument(&'static str),
    NotPermitted(&'static str),
    HostFailure(&'static str),
    ObservationTooLarge,
    CheckpointRequired,
}

impl ToolError {
    /// Human and model readable message; never allocates.
    pub const fn message(self) -> &'static str {
        match self {
            ToolError::UnknownTool => "unknown tool",
            ToolError::InvalidArgument(r) => r,
            ToolError::NotPermitted(r) => r,
            ToolError::HostFailure(r) => r,
            ToolError::ObservationTooLarge => "observation exceeds buffer",
            ToolError::CheckpointRequired => "checkpoint required before mutation",
        }
    }

    /// Stable machine code, mirrors JSON-RPC error codes in the MCP adapter.
    pub const fn code(self) -> i32 {
        match self {
            ToolError::UnknownTool => -32601,
            ToolError::InvalidArgument(_) => -32602,
            ToolError::NotPermitted(_) => -32003,
            ToolError::HostFailure(_) => -32000,
            ToolError::ObservationTooLarge => -32004,
            ToolError::CheckpointRequired => -32005,
        }
    }
}

/// Declarative description of one tool, shared by CLI, TUI, Web and MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// Stable snake_case identifier the model calls.
    pub name: &'static str,
    /// One line the model reads when picking a tool.
    pub summary: &'static str,
    /// Argument budget, reuses the core schema validator.
    pub schema: CommandSchema,
    /// True when the call can change files or machine state.
    pub mutating: bool,
    /// True when the call needs a live browser session.
    pub needs_browser: bool,
}

impl ToolSpec {
    /// Convenience constructor for unit-like tools.
    pub const fn new(name: &'static str, summary: &'static str, mutating: bool) -> Self {
        ToolSpec {
            name,
            summary,
            schema: CommandSchema::new(name, 4096),
            mutating,
            needs_browser: false,
        }
    }
}

/// A single callable capability.
pub trait Tool {
    /// Declaration shown to the model.
    fn spec(&self) -> ToolSpec;

    /// Run one call and write the observation into `out`.
    ///
    /// Tools must stay pure with respect to policy: checkpointing, approval and
    /// rollback are host duties, never the tool's.
    fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(ToolError::UnknownTool.code(), -32601);
        assert_eq!(ToolError::CheckpointRequired.code(), -32005);
    }

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(ObsKind::Text.label(), "text");
        assert_eq!(ObsKind::Console(LogStream::Console).label(), "console");
    }

    #[test]
    fn spec_defaults_are_sane() {
        let spec = ToolSpec::new("read_file", "read a text file", false);
        assert_eq!(spec.schema.max_len, 4096);
        assert!(!spec.mutating);
        assert!(!spec.needs_browser);
    }
}
