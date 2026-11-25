//! The loop that makes it an agent rather than a chat window.
//!
//! First principles, expressed as code:
//!
//! ```text
//! decide (Llm)  ->  execute (Host)  ->  observe (ObsBuf)  ->  protect (Machine)
//!      ^                                                          |
//!      +----------------------- next iteration -------------------+
//! ```
//!
//! The loop owns no I/O and no allocation: a real host plugs in `std::process`
//! and `std::fs`, a test host plugs in counters. Policy — "snapshot before a
//! mutating call, roll back after repeated failure" — lives in [`Machine`] and
//! cannot be bypassed by a host.

use crate::agent::llm::{Decision, Llm, Step};
use crate::agent::ooda::{ActPermit, Action, Machine, Phase};
use crate::agent::registry::Registry;
use crate::agent::tool::{ObsBuf, ObsKind, ToolError};

/// Loop limits. Small by default: an agent that cannot finish in 16 calls is
/// usually an agent that is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Maximum tool calls before the loop stops.
    pub max_steps: u32,
    /// Consecutive failures tolerated before rolling back.
    pub max_failures: u32,
}

impl Budget {
    /// Explicit limits.
    pub const fn new(max_steps: u32, max_failures: u32) -> Self {
        Budget {
            max_steps,
            max_failures,
        }
    }

    /// The MVP defaults: 16 calls, roll back after 3 failures.
    pub const fn mvp() -> Self {
        Budget::new(16, 3)
    }
}

/// Everything the loop is not allowed to do itself.
///
/// Implemented by the CLI over `std`, and by fakes in tests.
pub trait Host {
    /// Persist a recovery point before a mutating call.
    fn checkpoint(&mut self, label: &str) -> Result<(), &'static str>;
    /// Persist a recovery point for this invocation, including its validated argument shape.
    fn checkpoint_action(&mut self, action: &Action<'_>) -> Result<(), &'static str> {
        self.checkpoint(action.tool.name)
    }

    /// Restore the most recent checkpoint.
    fn undo(&mut self) -> Result<(), &'static str>;
    /// Run one tool call and write its observation into `out`.
    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError>;
}

/// What happened over one whole run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Completed tool calls.
    pub calls: u32,
    /// Calls that returned an error.
    pub failures: u32,
    /// Checkpoints the host was asked to take.
    pub checkpoints: u32,
    /// True when the loop ended because the model stopped or the budget ran out.
    pub finished: bool,
    /// True when a rollback was performed at least once.
    pub rolled_back: bool,
    /// Terminal policy or recovery error, when the run stopped without completing.
    pub stopped_by: Option<&'static str>,
    /// Final phase label, for the trace.
    pub phase: &'static str,
}

impl Outcome {
    /// True when nothing went wrong.
    pub const fn clean(&self) -> bool {
        self.failures == 0 && !self.rolled_back && self.stopped_by.is_none()
    }
}

/// Drive the loop until the model stops, the budget runs out, or policy trips.
pub fn drive(
    goal: &str,
    llm: &mut dyn Llm,
    registry: &Registry,
    host: &mut dyn Host,
    budget: Budget,
) -> Outcome {
    let mut machine = Machine::new(budget.max_steps, budget.max_failures);
    let mut obs = ObsBuf::new();
    let mut outcome = Outcome {
        calls: 0,
        failures: 0,
        checkpoints: 0,
        finished: false,
        rolled_back: false,
        stopped_by: None,
        phase: "observe",
    };
    let mut state = Turn {
        machine: &mut machine,
        obs: &mut obs,
        outcome: &mut outcome,
    };
    let mut planner = Planner {
        goal,
        llm,
        registry,
    };
    while !state.machine.exhausted() && state.machine.phase() != Phase::Done {
        if let Err(reason) = one_turn(&mut planner, host, &mut state) {
            state.outcome.stopped_by = Some(reason);
            break;
        }
    }
    outcome.phase = machine.phase().label();
    outcome.finished =
        outcome.stopped_by.is_none() && (machine.exhausted() || machine.phase() == Phase::Done);
    outcome
}

struct Turn<'a> {
    machine: &'a mut Machine,
    obs: &'a mut ObsBuf,
    outcome: &'a mut Outcome,
}

struct Planner<'a> {
    goal: &'a str,
    llm: &'a mut dyn Llm,
    registry: &'a Registry,
}

/// Shared execution loop. No platform calls or allocations occur here.
fn one_turn(
    planner: &mut Planner<'_>,
    host: &mut dyn Host,
    state: &mut Turn<'_>,
) -> Result<(), &'static str> {
    state
        .machine
        .begin_orient()
        .map_err(|error| error.message())?;
    state
        .machine
        .begin_decide()
        .map_err(|error| error.message())?;
    let ctx = Decision {
        goal: planner.goal,
        step: state.machine.step(),
        observation: state.obs.as_str(),
    };
    let Step::Call(call) = planner.llm.plan(ctx) else {
        state.machine.finish();
        return Ok(());
    };
    let spec = planner
        .registry
        .find(call.tool.as_str())
        .ok_or(ToolError::UnknownTool.message())?
        .spec();
    let action = Action {
        tool: &spec,
        args: call.args.as_str(),
    };
    run_action(host, state, &action)
}

fn run_action(
    host: &mut dyn Host,
    state: &mut Turn<'_>,
    action: &Action<'_>,
) -> Result<(), &'static str> {
    let machine = &mut state.machine;
    if machine.begin_act(action).map_err(|error| error.message())? == ActPermit::CheckpointFirst {
        host.checkpoint_action(action)?;
        machine.checkpointed().map_err(|error| error.message())?;
        state.outcome.checkpoints = state.outcome.checkpoints.saturating_add(1);
    }

    state.obs.clear();
    let result = host.call(action.tool.name, action.args, state.obs);
    state.outcome.calls = state.outcome.calls.saturating_add(1);
    if let Err(error) = result {
        state.outcome.failures = state.outcome.failures.saturating_add(1);
        state.obs.clear();
        use core::fmt::Write as _;
        write!(state.obs, "{}: {}", error.code(), error.message())
            .map_err(|_| ToolError::ObservationTooLarge.message())?;
    }
    machine
        .finish_act(result.is_ok())
        .map_err(|error| error.message())?;
    if machine.wants_rollback() {
        host.undo()?;
        state.outcome.rolled_back = true;
        return machine.recovered().map_err(|error| error.message());
    }
    machine.finish_verify().map_err(|error| error.message())
}

/// Marker used by hosts that refuse to checkpoint (dry runs).
pub const DRY_RUN_NOTE: &str = "dry run: checkpoint skipped";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::ScriptedLlm;
    use crate::agent::tool::{Tool, ToolError, ToolSpec};

    struct Probe {
        name: &'static str,
        mutating: bool,
    }

    impl Tool for Probe {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new(self.name, "probe", self.mutating)
        }

        fn call(&self, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
            out.push_str(args)
                .map_err(|_| ToolError::ObservationTooLarge)?;
            Ok(ObsKind::Text)
        }
    }

    static READ: Probe = Probe {
        name: "read_file",
        mutating: false,
    };
    static EDIT: Probe = Probe {
        name: "edit_file",
        mutating: true,
    };

    #[derive(Default)]
    struct FakeHost {
        calls: u32,
        checkpoints: u32,
        undos: u32,
        fail: bool,
    }

    impl Host for FakeHost {
        fn checkpoint(&mut self, _label: &str) -> Result<(), &'static str> {
            self.checkpoints += 1;
            Ok(())
        }

        fn undo(&mut self) -> Result<(), &'static str> {
            self.undos += 1;
            Ok(())
        }

        fn call(&mut self, _t: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
            self.calls += 1;
            out.push_str(args)
                .map_err(|_| ToolError::ObservationTooLarge)?;
            if self.fail {
                return Err(ToolError::HostFailure("boom"));
            }
            Ok(ObsKind::Text)
        }
    }

    fn registry() -> Registry {
        let mut reg = Registry::new();
        reg.register(&READ).unwrap();
        reg.register(&EDIT).unwrap();
        reg
    }

    #[test]
    fn runs_every_scripted_call() {
        let mut llm = ScriptedLlm::new();
        llm.push("read_file", "a").unwrap();
        llm.push("read_file", "b").unwrap();
        let mut host = FakeHost::default();
        let out = drive("", &mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.calls, 2);
        assert_eq!(out.failures, 0);
        assert!(out.finished);
        assert!(out.clean());
        assert_eq!(host.calls, 2);
    }

    #[test]
    fn mutating_call_forces_a_checkpoint() {
        let mut llm = ScriptedLlm::new();
        llm.push("edit_file", "x").unwrap();
        let mut host = FakeHost::default();
        let out = drive("", &mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.checkpoints, 1);
        assert_eq!(host.checkpoints, 1);
        assert_eq!(out.calls, 1);
    }

    #[test]
    fn read_only_call_takes_no_checkpoint() {
        let mut llm = ScriptedLlm::new();
        llm.push("read_file", "x").unwrap();
        let mut host = FakeHost::default();
        let out = drive("", &mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.checkpoints, 0);
    }

    #[test]
    fn repeated_failure_rolls_back() {
        let mut llm = ScriptedLlm::new();
        for _ in 0..5 {
            llm.push("read_file", "x").unwrap();
        }
        let mut host = FakeHost {
            fail: true,
            ..Default::default()
        };
        let out = drive("", &mut llm, &registry(), &mut host, Budget::new(16, 2));
        assert!(out.rolled_back);
        assert!(host.undos >= 1);
        assert!(out.failures >= 2);
    }

    #[test]
    fn failed_checkpoint_blocks_mutation_and_is_visible() {
        struct Denied;
        impl Host for Denied {
            fn checkpoint(&mut self, _: &str) -> Result<(), &'static str> {
                Err("disk unavailable")
            }
            fn undo(&mut self) -> Result<(), &'static str> {
                unreachable!()
            }
            fn call(&mut self, _: &str, _: &str, _: &mut ObsBuf) -> Result<ObsKind, ToolError> {
                unreachable!()
            }
        }
        let mut llm = ScriptedLlm::new();
        llm.push("edit_file", "x").unwrap();
        let out = drive("", &mut llm, &registry(), &mut Denied, Budget::mvp());
        assert_eq!(out.stopped_by, Some("disk unavailable"));
        assert_eq!(out.calls, 0);
        assert!(!out.clean());
    }

    #[test]
    fn failed_undo_is_not_reported_as_recovered() {
        struct Broken;
        impl Host for Broken {
            fn checkpoint(&mut self, _: &str) -> Result<(), &'static str> {
                Ok(())
            }
            fn undo(&mut self) -> Result<(), &'static str> {
                Err("recovery failed")
            }
            fn call(&mut self, _: &str, _: &str, _: &mut ObsBuf) -> Result<ObsKind, ToolError> {
                Err(ToolError::HostFailure("call failed"))
            }
        }
        let mut llm = ScriptedLlm::new();
        llm.push("edit_file", "x").unwrap();
        let out = drive("", &mut llm, &registry(), &mut Broken, Budget::new(4, 1));
        assert_eq!(out.stopped_by, Some("recovery failed"));
        assert_eq!(out.phase, "rollback");
        assert!(!out.rolled_back);
        assert!(!out.finished);
    }

    #[test]
    fn unknown_tool_is_a_visible_error() {
        let mut llm = ScriptedLlm::new();
        llm.push("nope", "").unwrap();
        let mut host = FakeHost::default();
        let out = drive("", &mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.calls, 0);
        assert!(!out.finished);
        assert!(!out.clean());
        assert_eq!(out.stopped_by, Some("unknown tool"));
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn budget_caps_the_run() {
        let mut llm = ScriptedLlm::new();
        // A plan holds at most 8 calls; 6 is enough to prove the cap at 3.
        for _ in 0..6 {
            llm.push("read_file", "x").unwrap();
        }
        let mut host = FakeHost::default();
        let out = drive("", &mut llm, &registry(), &mut host, Budget::new(3, 9));
        assert_eq!(out.calls, 3);
    }
}
