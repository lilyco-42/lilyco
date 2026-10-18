//! The OODA state machine: observe, orient, decide, act, verify.
//!
//! The machine owns *policy*, not execution. It refuses to let a mutating
//! action run before the host has recorded a checkpoint, which is what makes
//! `jj`/`git` rollback trustworthy rather than decorative.

use crate::agent::tool::ToolSpec;

/// Where the loop currently stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Reading files, logs, pages, images.
    Observe,
    /// Turning raw evidence into a hypothesis.
    Orient,
    /// Choosing one tool call.
    Decide,
    /// Executing exactly one tool call.
    Act,
    /// Checking the observation against the stated goal.
    Verify,
    /// Restoring the last checkpoint after repeated failure.
    Rollback,
    /// Terminal state.
    Done,
}

impl Phase {
    /// Stable label for trace output.
    pub const fn label(self) -> &'static str {
        match self {
            Phase::Observe => "observe",
            Phase::Orient => "orient",
            Phase::Decide => "decide",
            Phase::Act => "act",
            Phase::Verify => "verify",
            Phase::Rollback => "rollback",
            Phase::Done => "done",
        }
    }
}

/// One proposed call, already resolved against the registry.
#[derive(Debug, Clone, Copy)]
pub struct Action<'a> {
    /// Declaration of the tool being invoked.
    pub tool: &'a ToolSpec,
    /// Raw argument text, validated by the tool.
    pub args: &'a str,
}

/// Illegal phase transition, kept explicit so the host can surface it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionError {
    pub from: Phase,
    pub to: Phase,
    pub reason: &'static str,
}

impl TransitionError {
    pub const fn message(self) -> &'static str {
        self.reason
    }
}

/// What the host must do before running the action it was just handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActPermit {
    /// Safe to run immediately: the call cannot mutate the workspace.
    RunNow,
    /// Record a checkpoint first, then call `checkpointed`.
    CheckpointFirst,
}

/// Loop state. Holds no allocation so it runs under `no_std`.
#[derive(Debug, Clone, Copy)]
pub struct Machine {
    phase: Phase,
    step: u32,
    failures: u32,
    checkpoints: u32,
    awaiting_checkpoint: bool,
    max_steps: u32,
    max_failures: u32,
}

impl Machine {
    /// Fresh machine with explicit budgets.
    pub const fn new(max_steps: u32, max_failures: u32) -> Self {
        Machine {
            phase: Phase::Observe,
            step: 0,
            failures: 0,
            checkpoints: 0,
            awaiting_checkpoint: false,
            max_steps,
            max_failures,
        }
    }

    /// Conservative default: 64 steps, roll back after 3 failures.
    pub const fn default_budget() -> Self {
        Machine::new(64, 3)
    }

    /// Current phase.
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Number of completed tool calls.
    pub const fn step(&self) -> u32 {
        self.step
    }

    /// Consecutive failures so far.
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    /// Checkpoints recorded through this machine.
    pub const fn checkpoints(&self) -> u32 {
        self.checkpoints
    }

    /// True when the step budget is spent.
    pub const fn exhausted(&self) -> bool {
        self.step >= self.max_steps
    }

    /// True when the machine wants the workspace restored.
    pub const fn wants_rollback(&self) -> bool {
        matches!(self.phase, Phase::Rollback)
    }

    /// Observe -> Orient. Also the re-entry point after a verify.
    pub fn begin_orient(&mut self) -> Result<(), TransitionError> {
        self.transition(Phase::Observe, Phase::Orient)
    }

    /// Orient -> Decide.
    pub fn begin_decide(&mut self) -> Result<(), TransitionError> {
        self.transition(Phase::Orient, Phase::Decide)
    }

    /// Decide -> Act, returning the host's obligation.
    pub fn begin_act(&mut self, action: &Action<'_>) -> Result<ActPermit, TransitionError> {
        self.transition(Phase::Decide, Phase::Act)?;
        if action.tool.mutating {
            self.awaiting_checkpoint = true;
            return Ok(ActPermit::CheckpointFirst);
        }
        Ok(ActPermit::RunNow)
    }

    /// Host confirms the checkpoint exists; the action may now run.
    pub fn checkpointed(&mut self) -> Result<(), TransitionError> {
        if !self.awaiting_checkpoint {
            return Err(self.error(Phase::Act, Phase::Act, "no checkpoint was requested"));
        }
        self.awaiting_checkpoint = false;
        self.checkpoints = self.checkpoints.saturating_add(1);
        Ok(())
    }

    /// Act -> Verify, recording whether the call succeeded.
    pub fn finish_act(&mut self, ok: bool) -> Result<(), TransitionError> {
        let permit = self.awaiting_checkpoint;
        if permit {
            return Err(self.error(Phase::Act, Phase::Verify, "checkpoint not yet recorded"));
        }
        self.transition(Phase::Act, Phase::Verify)?;
        self.step = self.step.saturating_add(1);
        if ok {
            self.failures = 0;
            return Ok(());
        }
        self.failures = self.failures.saturating_add(1);
        let cap = self.max_failures;
        if self.failures >= cap {
            self.phase = Phase::Rollback;
        }
        Ok(())
    }

    /// Verify -> Observe, closing one OODA turn.
    pub fn finish_verify(&mut self) -> Result<(), TransitionError> {
        if self.exhausted() {
            self.phase = Phase::Done;
            return Ok(());
        }
        self.transition(Phase::Verify, Phase::Observe)
    }

    /// Mark the run finished from any live phase.
    pub fn finish(&mut self) {
        self.phase = Phase::Done;
    }

    /// Rollback -> Observe, after the host restored the checkpoint.
    pub fn recovered(&mut self) -> Result<(), TransitionError> {
        self.transition(Phase::Rollback, Phase::Observe)?;
        self.failures = 0;
        Ok(())
    }

    /// Render a one-line trace such as `step 3 | act | edit_file`.
    pub fn render_trace<const N: usize>(
        &self,
        tool: &str,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        let cur = self.phase.label();
        ::core::write!(out, "step {} | {} | {}", self.step, cur, tool)
            .map_err(|_| "buffer full")
    }

    fn transition(&mut self, from: Phase, to: Phase) -> Result<(), TransitionError> {
        if self.phase != from {
            return Err(self.error(self.phase, to, "unexpected phase"));
        }
        self.phase = to;
        Ok(())
    }

    const fn error(&self, from: Phase, to: Phase, reason: &'static str) -> TransitionError {
        TransitionError { from, to, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tool::ToolSpec;

    const READ: ToolSpec = ToolSpec::new("read_file", "read", false);
    const EDIT: ToolSpec = ToolSpec::new("edit_file", "edit", true);

    fn action(tool: &'static ToolSpec) -> Action<'static> {
        Action { tool, args: "" }
    }

    #[test]
    fn happy_path_walks_every_phase() {
        let mut m = Machine::default_budget();
        assert!(m.begin_orient().is_ok());
        assert!(m.begin_decide().is_ok());
        assert_eq!(m.begin_act(&action(&READ)), Ok(ActPermit::RunNow));
        assert!(m.finish_act(true).is_ok());
        assert!(m.finish_verify().is_ok());
        assert_eq!(m.phase(), Phase::Observe);
        assert_eq!(m.step(), 1);
    }

    #[test]
    fn mutation_requires_checkpoint() {
        let mut m = Machine::default_budget();
        let _ = m.begin_orient();
        let _ = m.begin_decide();
        assert_eq!(m.begin_act(&action(&EDIT)), Ok(ActPermit::CheckpointFirst));
        assert!(m.finish_act(true).is_err());
        assert!(m.checkpointed().is_ok());
        assert!(m.finish_act(true).is_ok());
        assert_eq!(m.checkpoints(), 1);
    }

    #[test]
    fn repeated_failure_requests_rollback() {
        let mut m = Machine::new(10, 2);
        let mut rounds = 0u32;
        while !m.wants_rollback() && rounds < 5 {
            rounds += 1;
            let _ = m.begin_orient();
            let _ = m.begin_decide();
            let _ = m.begin_act(&action(&READ));
            let _ = m.finish_act(false);
            if !m.wants_rollback() {
                let _ = m.finish_verify();
            }
        }
        assert!(m.wants_rollback());
        assert_eq!(m.failures(), 2);
    }

    #[test]
    fn step_budget_terminates_loop() {
        let mut m = Machine::new(1, 9);
        let _ = m.begin_orient();
        let _ = m.begin_decide();
        let _ = m.begin_act(&action(&READ));
        let _ = m.finish_act(true);
        let _ = m.finish_verify();
        assert!(m.exhausted());
        assert_eq!(m.phase(), Phase::Done);
    }

    #[test]
    fn trace_is_readable() {
        let m = Machine::default_budget();
        let mut buf: heapless::String<64> = heapless::String::new();
        assert!(m.render_trace("read_file", &mut buf).is_ok());
        assert_eq!(buf.as_str(), "step 0 | observe | read_file");
    }
}
