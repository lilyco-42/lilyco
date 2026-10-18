//! The agent exposed as a Lilyco app.
//!
//! This is the ecosystem-compatibility seam: because it implements [`TinyApp`],
//! the same declaration is dispatchable by the CLI, the TUI, the Web shell and
//! the MCP server — `one struct, four front-ends` — exactly like every other
//! Lilyco application.

use std::boxed::Box;
use std::cell::RefCell;

use crate::agent::llm::Llm;
use crate::agent::registry::Registry;
use crate::agent::runtime::{drive, Budget, Host, Outcome};
use crate::core::progress::Progress;
use crate::core::schema::CommandSchema;
use crate::TinyApp;

/// A runnable agent: a decision source, a tool table, an execution host.
pub struct AgentApp {
    llm: RefCell<Box<dyn Llm>>,
    host: RefCell<Box<dyn Host>>,
    registry: Registry,
    budget: Budget,
}

impl AgentApp {
    /// Assemble an app.
    pub fn new(llm: Box<dyn Llm>, host: Box<dyn Host>, registry: Registry, budget: Budget) -> Self {
        AgentApp {
            llm: RefCell::new(llm),
            host: RefCell::new(host),
            registry,
            budget,
        }
    }

    /// The `goal` string is the only argument, which is all an MVP needs.
    pub const ARGUMENT: &'static str = "goal";
}

impl TinyApp for AgentApp {
    type Args = &'static str;
    type Output = Outcome;
    type Error = &'static str;

    fn schema() -> CommandSchema {
        CommandSchema::new("agent", 1024)
    }

    fn validate(args: &Self::Args) -> Result<(), Self::Error> {
        let schema = <AgentApp as TinyApp>::schema();
        schema.validate_input(args)
    }

    fn execute<F>(&self, args: Self::Args, mut progress: F) -> Result<Self::Output, Self::Error>
    where
        F: FnMut(Progress),
    {
        progress(Progress::Init);
        let mut llm = self.llm.borrow_mut();
        let mut host = self.host.borrow_mut();
        let outcome = drive(&mut **llm, &self.registry, &mut **host, self.budget);
        let done = if outcome.failures == 0 {
            Progress::Done
        } else {
            Progress::Error
        };
        progress(done);
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ScriptedLlm;
    use crate::agent::tool::{ObsBuf, ObsKind, Tool, ToolError, ToolSpec};

    struct Echo;

    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("echo", "echo", false)
        }

        fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
            out.push_str(args)
                .map_err(|_| ToolError::ObservationTooLarge)?;
            Ok(ObsKind::Text)
        }
    }

    static ECHO: Echo = Echo;

    #[derive(Default)]
    struct NoopHost {
        calls: u32,
    }

    impl Host for NoopHost {
        fn checkpoint(&mut self, _l: &str) -> Result<(), &'static str> {
            Ok(())
        }

        fn undo(&mut self) -> Result<(), &'static str> {
            Ok(())
        }

        fn call(&mut self, _t: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
            self.calls += 1;
            out.push_str(args)
                .map_err(|_| ToolError::ObservationTooLarge)?;
            Ok(ObsKind::Text)
        }
    }

    #[test]
    fn app_schema_is_dispatchable() {
        let schema = <AgentApp as TinyApp>::schema();
        assert_eq!(schema.name, "agent");
        assert!(<AgentApp as TinyApp>::validate(&"do the thing").is_ok());
        assert!(<AgentApp as TinyApp>::validate(&"").is_err());
    }

    #[test]
    fn app_runs_the_loop_through_the_lilyco_surface() {
        let mut llm = ScriptedLlm::new();
        llm.push("echo", "hi").unwrap();
        let mut registry = Registry::new();
        registry.register(&ECHO).unwrap();
        let app = AgentApp::new(
            Box::new(llm),
            Box::new(NoopHost::default()),
            registry,
            Budget::mvp(),
        );
        let mut seen: heapless::Vec<Progress, 4> = heapless::Vec::new();
        let outcome = app
            .execute("goal", |p| {
                let _ = seen.push(p);
            })
            .unwrap();
        assert_eq!(outcome.calls, 1);
        assert!(outcome.finished);
        assert_eq!(seen.first(), Some(&Progress::Init));
        assert_eq!(seen.last(), Some(&Progress::Done));
    }
}
