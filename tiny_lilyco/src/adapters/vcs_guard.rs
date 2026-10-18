//! Version-control guard: the undo net that makes aggressive edits safe.
//!
//! The guard never touches the disk itself. It detects which VCS is present and
//! renders the exact argv a host should run. `jj` wins when both exist because
//! its operation log makes single-step undo trivial.

/// Which version-control system backs the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Jj,
    Git,
    None,
}

/// Operations the agent may need from the guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op<'a> {
    /// Record a recoverable snapshot; the message becomes the label.
    Checkpoint(&'a str),
    /// Undo the most recent operation, one step.
    Undo,
    /// Show the last `n` operations.
    Log(usize),
    /// Throw away uncommitted working-tree changes.
    Discard,
}

impl Op<'_> {
    /// True when the operation cannot change the workspace.
    pub const fn is_read_only(&self) -> bool {
        matches!(self, Op::Log(_))
    }
}

/// `jj` is preferred over `git` when both directories exist.
pub const fn detect(has_jj: bool, has_git: bool) -> Backend {
    if has_jj {
        Backend::Jj
    } else if has_git {
        Backend::Git
    } else {
        Backend::None
    }
}

/// Render the argv for one operation.
///
/// `git` checkpoints deliberately use `stash create`, which snapshots the
/// working tree without disturbing it; the host pins the returned id with
/// [`pin_ref`] so the snapshot is reachable by name.
pub fn render<const N: usize>(
    backend: Backend,
    op: Op<'_>,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    match backend {
        Backend::None => Err("no version control found"),
        Backend::Jj => render_jj(op, out),
        Backend::Git => render_git(op, out),
    }
}

fn render_jj<const N: usize>(op: Op<'_>, out: &mut heapless::String<N>) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    match op {
        Op::Checkpoint(msg) => {
            out.push_str("jj --no-pager new -m \"").map_err(|_| "buffer full")?;
            escape_into(msg, out)?;
            out.push('"').map_err(|_| "buffer full")
        }
        Op::Undo => out.push_str("jj --no-pager undo").map_err(|_| "buffer full"),
        Op::Log(n) => ::core::write!(out, "jj --no-pager op log --limit {} --no-graph", clamp_n(n))
            .map_err(|_| "buffer full"),
        Op::Discard => out.push_str("jj --no-pager restore").map_err(|_| "buffer full"),
    }
}

fn render_git<const N: usize>(op: Op<'_>, out: &mut heapless::String<N>) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    match op {
        Op::Checkpoint(_) => out.push_str("git stash create").map_err(|_| "buffer full"),
        Op::Undo => out
            .push_str("git reset --hard refs/lilyco/checkpoint")
            .map_err(|_| "buffer full"),
        Op::Log(n) => ::core::write!(out, "git reflog --no-color -n {}", clamp_n(n))
            .map_err(|_| "buffer full"),
        Op::Discard => out.push_str("git checkout -- .").map_err(|_| "buffer full"),
    }
}

/// Pin a `git stash create` result so it survives garbage collection.
pub fn pin_ref<const N: usize>(
    sha: &str,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    if sha.len() < 7 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("checkpoint id is not a hex object name");
    }
    ::core::write!(out, "git update-ref refs/lilyco/checkpoint {}", sha)
        .map_err(|_| "buffer full")
}

fn clamp_n(n: usize) -> usize {
    match n {
        0 => 5,
        v if v > 50 => 50,
        v => v,
    }
}

fn escape_into<const N: usize>(
    src: &str,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    for ch in src.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\').map_err(|_| "buffer full")?;
                out.push(ch).map_err(|_| "buffer full")?;
            }
            '\n' | '\r' => out.push(' ').map_err(|_| "buffer full")?,
            c => out.push(c).map_err(|_| "buffer full")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_str(backend: Backend, op: Op<'_>) -> heapless::String<128> {
        let mut buf = heapless::String::new();
        render(backend, op, &mut buf).unwrap();
        buf
    }

    #[test]
    fn jj_is_preferred() {
        assert_eq!(detect(true, true), Backend::Jj);
        assert_eq!(detect(false, true), Backend::Git);
        assert_eq!(detect(false, false), Backend::None);
    }

    #[test]
    fn checkpoint_is_quoted_and_escaped() {
        let cmd = render_str(Backend::Jj, Op::Checkpoint("fix \"quoted\""));
        assert_eq!(cmd.as_str(), "jj --no-pager new -m \"fix \\\"quoted\\\"\"");
    }

    #[test]
    fn undo_targets_the_operation_log() {
        assert_eq!(render_str(Backend::Jj, Op::Undo).as_str(), "jj --no-pager undo");
        assert_eq!(
            render_str(Backend::Git, Op::Undo).as_str(),
            "git reset --hard refs/lilyco/checkpoint"
        );
    }

    #[test]
    fn log_limit_is_clamped() {
        assert_eq!(
            render_str(Backend::Jj, Op::Log(0)).as_str(),
            "jj --no-pager op log --limit 5 --no-graph"
        );
        assert_eq!(
            render_str(Backend::Git, Op::Log(999)).as_str(),
            "git reflog --no-color -n 50"
        );
    }

    #[test]
    fn only_log_is_read_only() {
        assert!(Op::Log(3).is_read_only());
        assert!(!Op::Undo.is_read_only());
    }

    #[test]
    fn pin_ref_rejects_junk() {
        let mut buf = heapless::String::<128>::new();
        assert!(pin_ref("nope", &mut buf).is_err());
        assert!(pin_ref("abc1234", &mut buf).is_ok());
        assert_eq!(buf.as_str(), "git update-ref refs/lilyco/checkpoint abc1234");
    }

    #[test]
    fn missing_vcs_is_an_error() {
        let mut buf = heapless::String::<64>::new();
        assert!(render(Backend::None, Op::Undo, &mut buf).is_err());
    }
}
