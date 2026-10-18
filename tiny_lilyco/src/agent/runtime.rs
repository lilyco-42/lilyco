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

use crate::agent::llm::{Llm, Step};
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
    /// Final phase label, for the trace.
    pub phase: &'static str,
}

impl Outcome {
    /// True when nothing went wrong.
    pub const fn clean(&self) -> bool {
        self.failures == 0 && !self.rolled_back
    }
}

/// Drive the loop until the model stops, the budget runs out, or policy trips.
pub fn drive(llm: &mut dyn Llm, registry: &Registry, host: &mut dyn Host, budget: Budget) -> Outcome {
    let mut machine = Machine::new(budget.max_steps, budget.max_failures);
    let mut obs = ObsBuf::new();
    let mut outcome = Outcome {
        calls: 0,
        failures: 0,
        checkpoints: 0,
        finished: false,
        rolled_back: false,
        phase: "observe",
    };
    let mut stopped = false;
    while !stopped && !machine.exhausted() {
        stopped = one_turn(&mut machine, llm, registry, host, &mut obs, &mut outcome);
    }
    outcome.phase = machine.phase().label();
    outcome.finished = stopped || machine.exhausted() || machine.phase() == Phase::Done;
    outcome
}

/// Returns true when the loop should stop.
fn one_turn(
    machine: &mut Machine,
    llm: &mut dyn Llm,
    registry: &Registry,
    host: &mut dyn Host,
    obs: &mut ObsBuf,
    outcome: &mut Outcome,
) -> bool {
    if machine.begin_orient().is_err() || machine.begin_decide().is_err() {
        return true;
    }
    match llm.plan(machine.step(), Some(obs.as_str())) {
        Step::Done | Step::Say(_) => {
            machine.finish();
            true
        }
        Step::Call(call) => {
            let Some(tool) = registry.find(call.tool) else {
                machine.finish();
                return true;
            };
            let spec = tool.spec();
            let action = Action {
                tool: &spec,
                args: call.args.as_str(),
            };
            if !authorize(machine, &action, host, outcome) {
                return true;
            }
            obs.clear();
            let result = host.call(call.tool, call.args.as_str(), obs);
            outcome.calls = outcome.calls.saturating_add(1);
            let ok = result.is_ok();
            if !ok {
                outcome.failures = outcome.failures.saturating_add(1);
            }
            let _ = machine.finish_act(ok);
            if machine.wants_rollback() {
                let _ = host.undo();
                outcome.rolled_back = true;
                let _ = machine.recovered();
            }
            let _ = machine.finish_verify();
            false
        }
    }
}

/// Ask policy whether the call may run, taking a checkpoint when required.
fn authorize(
    machine: &mut Machine,
    action: &Action<'_>,
    host: &mut dyn Host,
    outcome: &mut Outcome,
) -> bool {
    let permit = match machine.begin_act(action) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if permit == ActPermit::RunNow {
        return true;
    }
    if host.checkpoint(action.tool.name).is_err() {
        return false;
    }
    outcome.checkpoints = outcome.checkpoints.saturating_add(1);
    machine.checkpointed().is_ok()
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
        let out = drive(&mut llm, &registry(), &mut host, Budget::mvp());
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
        let out = drive(&mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.checkpoints, 1);
        assert_eq!(host.checkpoints, 1);
        assert_eq!(out.calls, 1);
    }

    #[test]
    fn read_only_call_takes_no_checkpoint() {
        let mut llm = ScriptedLlm::new();
        llm.push("read_file", "x").unwrap();
        let mut host = FakeHost::default();
        let out = drive(&mut llm, &registry(), &mut host, Budget::mvp());
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
        let out = drive(&mut llm, &registry(), &mut host, Budget::new(16, 2));
        assert!(out.rolled_back);
        assert!(host.undos >= 1);
        assert!(out.failures >= 2);
    }

    #[test]
    fn unknown_tool_stops_cleanly() {
        let mut llm = ScriptedLlm::new();
        llm.push("nope", "").unwrap();
        let mut host = FakeHost::default();
        let out = drive(&mut llm, &registry(), &mut host, Budget::mvp());
        assert_eq!(out.calls, 0);
        assert!(out.finished);
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
        let out = drive(&mut llm, &registry(), &mut host, Budget::new(3, 9));
        assert_eq!(out.calls, 3);
    }
}
