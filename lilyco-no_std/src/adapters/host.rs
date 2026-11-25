//! Host bridge: the one module allowed to touch the operating system.
//!
//! Every planner in `adapters` renders an argv; this module runs it. Keeping the
//! split means the whole agent core stays portable and testable.

use ::std::process::Command;
use ::std::string::String;

/// Result of one finished child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    /// True when the child exited with status 0.
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// Why a child process could not be run or did not finish cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// Empty argv.
    Empty,
    /// The program is not installed or not executable.
    Spawn(&'static str),
    /// The process ran but exited non-zero.
    Failed(String),
}

/// Run argv directly, with no shell interpolation.
pub fn run(argv: &[&'static str]) -> Result<Outcome, HostError> {
    let (program, args) = argv.split_first().ok_or(HostError::Empty)?;
    let program = *program;
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|_| HostError::Spawn(program))?;
    Ok(Outcome {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Run argv and turn a non-zero exit into `HostError::Failed`.
pub fn run_checked(argv: &[&'static str]) -> Result<Outcome, HostError> {
    let out = run(argv)?;
    if out.ok() {
        return Ok(out);
    }
    Err(HostError::Failed(out.stderr))
}

/// True when a program is runnable on this machine.
pub fn available(program: &'static str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Convenience: pick the VCS backend by probing the machine.
pub fn detect_vcs() -> crate::adapters::vcs_guard::Backend {
    crate::adapters::vcs_guard::detect(available("jj"), available("git"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_argv_is_rejected() {
        assert_eq!(run(&[]), Err(HostError::Empty));
    }

    #[test]
    fn missing_program_is_spawn_error() {
        match run(&["definitely-not-a-real-binary-xyz"]) {
            Err(HostError::Spawn(_)) => {}
            other => panic!("unexpected: {:?}", other),
        }
    }
}
