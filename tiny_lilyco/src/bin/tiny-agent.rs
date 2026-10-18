//! Minimal agent CLI: drives the loop over a workspace with a scripted plan.
//!
//! Borrowed structure, not complexity:
//!
//! * Claude Code's **permission modes** — writes are opt-in (`--apply`), so the
//!   default run is a read-only plan;
//! * Codex's **sandbox + approval split** — the host snapshots, the loop decides;
//! * opencode's **one binary, one tool table**;
//! * zerostack's **determinism** — same plan in, same trace out.
//!
//! ```text
//! tiny-agent <root> --call list_dir: --call read_file:note.txt [--apply]
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use tiny_lilyco::adapters::vcs_guard::{self, Backend, Op};
use tiny_lilyco::agent::llm::ScriptedLlm;
use tiny_lilyco::agent::registry::Registry;
use tiny_lilyco::agent::runtime::{drive, Budget, Host, Outcome};
use tiny_lilyco::agent::tool::{ObsBuf, ObsKind, Tool, ToolError};
use tiny_lilyco::agent::tools::{EditFile, ListDir, ReadFile};

struct StdHost {
    registry: &'static Registry,
    backend: Backend,
    apply: bool,
}

impl Host for StdHost {
    fn checkpoint(&mut self, label: &str) -> Result<(), &'static str> {
        let mut buf = heapless::String::<160>::new();
        vcs_guard::render(self.backend, Op::Checkpoint(label), &mut buf)?;
        let argv: Vec<&str> = buf.as_str().split_whitespace().collect();
        println!("    checkpoint: {}", buf.as_str());
        Command::new(argv[0])
            .args(&argv[1..])
            .status()
            .map(|_| ())
            .map_err(|_| "checkpoint command failed")
    }

    fn undo(&mut self) -> Result<(), &'static str> {
        let mut buf = heapless::String::<160>::new();
        vcs_guard::render(self.backend, Op::Undo, &mut buf)?;
        let argv: Vec<&str> = buf.as_str().split_whitespace().collect();
        println!("    rollback: {}", buf.as_str());
        Command::new(argv[0])
            .args(&argv[1..])
            .status()
            .map(|_| ())
            .map_err(|_| "undo command failed")
    }

    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let handle = self.registry.find(tool).ok_or(ToolError::UnknownTool)?;
        let spec = handle.spec();
        if spec.mutating && !self.apply {
            println!("    {tool}({args}) -> refused (dry run)");
            return Err(ToolError::NotPermitted("dry run: pass --apply to mutate"));
        }
        let kind = handle.call(args, out)?;
        println!("    {tool}({args}) -> {}", kind.label());
        for line in out.as_str().lines().take(6) {
            println!("      {line}");
        }
        Ok(kind)
    }
}

fn main() {
    let code = match run() {
        Ok(clean) => {
            if clean {
                0
            } else {
                1
            }
        }
        Err(msg) => {
            eprintln!("tiny-agent: {msg}");
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<bool, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(argv.first().cloned().unwrap_or_else(|| ".".to_string()));
    let apply = argv.iter().any(|a| a == "--apply");
    let calls = collect_calls(&argv);
    if calls.is_empty() {
        return Err("usage: tiny-agent <root> --call <tool>:<args> [--call ...] [--apply]".into());
    }

    let registry = build_registry(&root);
    let mut llm = ScriptedLlm::new();
    for (tool, args) in &calls {
        llm.push(tool, args).map_err(|e| e.to_string())?;
    }
    let backend = detect_vcs();
    println!("root   : {}", root.display());
    println!("mode   : {}", if apply { "apply" } else { "dry-run" });
    println!("vcs    : {backend:?}");

    let mut host = StdHost {
        registry,
        backend,
        apply,
    };
    let outcome = drive(&mut llm, registry, &mut host, Budget::mvp());
    report(&outcome);
    Ok(outcome.clean())
}

/// Parse `--call tool:args` pairs, keeping order.
fn collect_calls(argv: &[String]) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < argv.len() {
        if argv[i] == "--call" {
            let raw = &argv[i + 1];
            let (tool, args) = match raw.split_once(':') {
                Some((t, a)) => (t, a.to_string()),
                None => (raw.as_str(), String::new()),
            };
            let name = match tool {
                "read_file" => "read_file",
                "list_dir" => "list_dir",
                "edit_file" => "edit_file",
                _ => "",
            };
            if !name.is_empty() {
                out.push((name, args));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

fn build_registry(root: &Path) -> &'static Registry {
    let read: &'static dyn Tool = Box::leak(Box::new(ReadFile::new(root)));
    let list: &'static dyn Tool = Box::leak(Box::new(ListDir::new(root)));
    let edit: &'static dyn Tool = Box::leak(Box::new(EditFile::new(root)));
    let reg: &'static mut Registry = Box::leak(Box::new(Registry::new()));
    let _ = reg.register(read);
    let _ = reg.register(list);
    let _ = reg.register(edit);
    reg
}

fn detect_vcs() -> Backend {
    let has = |bin: &str| {
        Command::new(bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    vcs_guard::detect(has("jj"), has("git"))
}

fn report(outcome: &Outcome) {
    println!(
        "result : {} calls, {} failures, {} checkpoints, rolled_back={}",
        outcome.calls, outcome.failures, outcome.checkpoints, outcome.rolled_back
    );
    println!("phase  : {}", outcome.phase);
}
