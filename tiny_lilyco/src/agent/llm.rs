//! The decision surface: what the model is allowed to say next.
//!
//! Deliberately tiny. A model either asks for one tool call or stops. Everything
//! else (retries, planning, chain-of-thought) is the model's business, not the
//! framework's — which is what keeps this layer `no_std` and testable.

use heapless::{String, Vec};

/// Longest tool name the loop accepts.
pub const MAX_NAME: usize = 32;

/// Longest argument string a single call may carry.
pub const MAX_ARGS: usize = 256;

/// Longest spoken line the loop keeps.
pub const MAX_SPEECH: usize = 256;

/// Everything a model needs to decide, without exposing the registry.
#[derive(Debug, Clone, Copy)]
pub struct Decision<'a> {
    /// The user's goal, verbatim.
    pub goal: &'a str,
    /// Completed tool calls so far.
    pub step: u32,
    /// Text the previous call produced (empty on the first turn).
    pub observation: &'a str,
}

/// One requested tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Tool name, resolved against the registry. Owned, because a model returns
    /// it at runtime.
    pub tool: String<MAX_NAME>,
    /// Raw arguments, validated by the tool itself.
    pub args: String<MAX_ARGS>,
}

impl Call {
    /// Build a call, rejecting an over-long name or argument string.
    pub fn new(tool: &str, args: &str) -> Result<Self, &'static str> {
        let mut name = String::new();
        name.push_str(tool).map_err(|_| "tool name too long")?;
        let mut buf = String::new();
        buf.push_str(args).map_err(|_| "args too long")?;
        Ok(Call { tool: name, args: buf })
    }
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
    /// Decide the next step from the goal and the previous observation.
    fn plan(&mut self, ctx: Decision<'_>) -> Step;
}

/// Replays a fixed list of calls, then stops.
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
    pub fn push(&mut self, tool: &str, args: &str) -> Result<(), &'static str> {
        let call = Call::new(tool, args)?;
        self.plan.push(call).map_err(|_| "plan is full")
    }

    /// How many calls are still unplayed.
    pub fn remaining(&self) -> usize {
        self.plan.len().saturating_sub(self.at)
    }
}

impl Llm for ScriptedLlm {
    fn plan(&mut self, _ctx: Decision<'_>) -> Step {
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
    fn plan(&mut self, _ctx: Decision<'_>) -> Step {
        Step::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Decision<'static> {
        Decision {
            goal: "test",
            step: 0,
            observation: "",
        }
    }

    #[test]
    fn scripted_plan_plays_in_order_then_stops() {
        let mut llm = ScriptedLlm::new();
        llm.push("read_file", "src/main.rs").unwrap();
        llm.push("edit_file", "a->b").unwrap();
        assert_eq!(llm.remaining(), 2);
        let first = llm.plan(ctx());
        assert!(matches!(first, Step::Call(ref c) if c.tool.as_str() == "read_file"));
        let _ = llm.plan(ctx());
        assert_eq!(llm.remaining(), 0);
        assert_eq!(llm.plan(ctx()), Step::Done);
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
    fn oversized_tool_name_is_rejected() {
        let mut long: heapless::String<64> = heapless::String::new();
        for _ in 0..=MAX_NAME {
            long.push('n').unwrap();
        }
        assert!(Call::new(long.as_str(), "").is_err());
    }

    #[test]
    fn idle_model_stops_immediately() {
        assert_eq!(IdleLlm.plan(ctx()), Step::Done);
    }
}
