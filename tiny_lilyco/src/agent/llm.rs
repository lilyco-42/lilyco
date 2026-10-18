//! The decision surface: what the model is allowed to say next.
//!
//! Deliberately tiny. A model either asks for one tool call or stops. Everything
//! else (retries, planning, chain-of-thought) is the model's business, not the
//! framework's — which is what keeps this layer `no_std` and testable.

use heapless::{String, Vec};

/// Longest argument string a single call may carry.
pub const MAX_ARGS: usize = 256;

/// Longest spoken line the loop keeps.
pub const MAX_SPEECH: usize = 256;

/// One requested tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Tool name, resolved against the registry.
    pub tool: &'static str,
    /// Raw arguments, validated by the tool itself.
    pub args: String<MAX_ARGS>,
}

/// What the model decided to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Invoke exactly one tool, then observe.
    Call(Call),
    /// Nothing left to do; `text` is the final answer.
    Say(String<MAX_SPEECH>),
    /// Stop without an answer (budget, refusal, dead end).
    Done,
}

/// A source of decisions.
///
/// Real implementations wrap an HTTP model; [`ScriptedLlm`] replays a fixed plan
/// so the loop can be exercised offline and in CI.
pub trait Llm {
    /// Decide the next step. `step` is the number of completed tool calls, and
    /// `observation` is the text produced by the previous one (`None` at start).
    fn plan(&mut self, step: u32, observation: Option<&str>) -> Step;
}

/// Replays a fixed list of calls, then stops.
///
/// This is the MVP's default "model": it proves the machinery end to end
/// without a network, a key, or a flaky dependency.
#[derive(Debug, Default)]
pub struct ScriptedLlm {
    plan: Vec<Call, 8>,
    at: usize,
}

impl ScriptedLlm {
    /// Empty plan.
    pub const fn new() -> Self {
        ScriptedLlm {
            plan: Vec::new(),
            at: 0,
        }
    }

    /// Append one call to the plan.
    pub fn push(&mut self, tool: &'static str, args: &str) -> Result<(), &'static str> {
        let mut buf = String::new();
        buf.push_str(args).map_err(|_| "args too long")?;
        self.plan
            .push(Call { tool, args: buf })
            .map_err(|_| "plan is full")
    }

    /// How many calls are still unplayed.
    pub fn remaining(&self) -> usize {
        self.plan.len().saturating_sub(self.at)
    }
}

impl Llm for ScriptedLlm {
    fn plan(&mut self, _step: u32, _observation: Option<&str>) -> Step {
        let Some(call) = self.plan.get(self.at) else {
            return Step::Done;
        };
        self.at += 1;
        Step::Call(call.clone())
    }
}

/// A model that never does anything — useful as a CLI default.
#[derive(Debug, Default)]
pub struct IdleLlm;

impl Llm for IdleLlm {
    fn plan(&mut self, _step: u32, _observation: Option<&str>) -> Step {
        Step::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_plan_plays_in_order_then_stops() {
        let mut llm = ScriptedLlm::new();
        llm.push("read_file", "src/main.rs").unwrap();
        llm.push("edit_file", "a->b").unwrap();
        assert_eq!(llm.remaining(), 2);
        let first = llm.plan(0, None);
        assert!(matches!(first, Step::Call(ref c) if c.tool == "read_file"));
        let _ = llm.plan(1, Some("ok"));
        assert_eq!(llm.remaining(), 0);
        assert_eq!(llm.plan(2, None), Step::Done);
    }

    #[test]
    fn oversized_args_are_rejected() {
        let mut llm = ScriptedLlm::new();
        let mut long: heapless::String<512> = heapless::String::new();
        for _ in 0..=MAX_ARGS {
            long.push('x').unwrap();
        }
        assert!(llm.push("t", long.as_str()).is_err());
    }

    #[test]
    fn idle_model_stops_immediately() {
        assert_eq!(IdleLlm.plan(0, None), Step::Done);
    }
}
