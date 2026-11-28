//! Command schema: the single declarative surface shared by every front-end.

/// Declarative description of one command or tool argument set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSchema {
    /// Stable command name, used for dispatch and for the MCP tool name.
    pub name: &'static str,
    /// Maximum accepted argument length in bytes.
    pub max_len: usize,
}

impl CommandSchema {
    /// Build a schema with an explicit argument budget.
    pub const fn new(name: &'static str, max_len: usize) -> Self {
        CommandSchema { name, max_len }
    }

    /// Shared validation used by CLI, TUI, Web and MCP.
    ///
    /// Rejects empty input, over-long input and control characters. Non-ASCII
    /// text is allowed so Chinese arguments pass untouched.
    pub fn validate_input(&self, input: &str) -> Result<(), &'static str> {
        if input.is_empty() {
            return Err("input cannot be empty");
        }
        if input.len() > self.max_len {
            return Err("input exceeds max length");
        }
        self.check_printable(input)
    }

    fn check_printable(&self, input: &str) -> Result<(), &'static str> {
        let bad = input
            .bytes()
            .any(|b| b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r');
        if bad {
            return Err("input contains control characters");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_overlong() {
        let s = CommandSchema::new("demo", 4);
        assert!(s.validate_input("").is_err());
        assert!(s.validate_input("toolong").is_err());
        assert!(s.validate_input("ok").is_ok());
    }

    #[test]
    fn allows_cjk_and_rejects_control_chars() {
        let s = CommandSchema::new("demo", 64);
        assert!(s.validate_input("中文参数").is_ok());
        assert!(s.validate_input("bad\u{0007}").is_err());
    }
}
