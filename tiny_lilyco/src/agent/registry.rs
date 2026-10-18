//! Fixed-capacity tool registry: one lookup table for all four front-ends.

use crate::agent::tool::{Tool, ToolError, ToolSpec};

/// How many tools a single agent build may expose.
pub const MAX_TOOLS: usize = 16;

/// Allocation-free registry of static tool objects.
#[derive(Default)]
pub struct Registry {
    tools: heapless::Vec<&'static dyn Tool, MAX_TOOLS>,
}

impl Registry {
    /// Empty registry.
    pub const fn new() -> Self {
        Registry {
            tools: heapless::Vec::new(),
        }
    }

    /// Register a tool. Duplicate names are rejected rather than shadowed.
    pub fn register(&mut self, tool: &'static dyn Tool) -> Result<(), ToolError> {
        if self.find(tool.spec().name).is_some() {
            return Err(ToolError::InvalidArgument("duplicate tool name"));
        }
        self.tools
            .push(tool)
            .map_err(|_| ToolError::InvalidArgument("registry full"))
    }

    /// Resolve by name. No aliasing: the model sees exactly one name.
    pub fn find(&self, name: &str) -> Option<&'static dyn Tool> {
        let hit = self.tools.iter().find(|t| t.spec().name == name);
        hit.copied()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// True when nothing has been registered yet.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Iterate over declarations, for `tools/list` and `--help`.
    pub fn specs(&self) -> impl Iterator<Item = ToolSpec> + '_ {
        self.tools.iter().map(|t| t.spec())
    }

    /// Render `name — summary` lines into a fixed buffer.
    pub fn render_catalog<const N: usize>(
        &self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        let mut first = true;
        for spec in self.specs() {
            if !first {
                out.push('\n').map_err(|_| "buffer full")?;
            }
            first = false;
            ::core::write!(out, "{} - {}", spec.name, spec.summary)
                .map_err(|_| "buffer full")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tool::ObsKind;

    struct Echo;

    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("echo", "echo the argument", false)
        }

        fn call(&self, args: &str, out: &mut crate::agent::tool::ObsBuf) -> Result<ObsKind, ToolError> {
            out.push_str(args)
                .map_err(|_| ToolError::ObservationTooLarge)?;
            Ok(ObsKind::Text)
        }
    }

    static ECHO: Echo = Echo;

    #[test]
    fn registers_and_resolves() {
        let mut reg = Registry::new();
        assert!(reg.register(&ECHO).is_ok());
        assert!(reg.find("echo").is_some());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn rejects_duplicates() {
        let mut reg = Registry::new();
        let _ = reg.register(&ECHO);
        assert_eq!(
            reg.register(&ECHO),
            Err(ToolError::InvalidArgument("duplicate tool name"))
        );
    }

    #[test]
    fn missing_tool_is_none() {
        let reg = Registry::new();
        assert!(reg.is_empty());
        assert!(reg.find("nope").is_none());
    }
}
