//! Minimal agent CLI: drives the loop over a workspace.
//!
//! Two modes, mirroring how the mature agents work:
//!
//! * **scripted** — `--call` replays a fixed plan (offline, deterministic, CI);
//! * **model** — `--provider` hands the loop to a real OpenAI-compatible model.
//!
//! Safety follows Claude Code / Codex: reads run by default, writes need
//! `--apply`, and every mutating call is preceded by a checkpoint.
//!
//! ```text
//! tiny-agent <root> --call list_dir: --call read_file:note.txt
//! tiny-agent <root> --provider openrouter --model <id> --goal "fix the build" --apply
//! ```

use std::path::Path;
use std::process::Command;

use tiny_lilyco::adapters::vcs_guard::{self, Backend, Op};
use tiny_lilyco::agent::llm::{Llm, ScriptedLlm};
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
        run_vcs(self.backend, Op::Checkpoint(label), "checkpoint")
    }

    fn undo(&mut self) -> Result<(), &'static str> {
        run_vcs(self.backend, Op::Undo, "rollback")
    }

    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let handle = self.registry.find(tool).ok_or(ToolError::UnknownTool)?;
        if handle.spec().mutating && !self.apply {
            println!("    {tool}({args}) -> refused (dry run, pass --apply)");
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

fn run_vcs(backend: Backend, op: Op<'_>, label: &str) -> Result<(), &'static str> {
    let mut buf = heapless::String::<200>::new();
    vcs_guard::render(backend, op, &mut buf)?;
    let argv: Vec<&str> = buf.as_str().split_whitespace().collect();
    println!("    {label}: {}", buf.as_str());
    Command::new(argv[0])
        .args(&argv[1..])
        .status()
        .map(|_| ())
        .map_err(|_| "vcs command failed")
}

fn main() {
    let code = match run() {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(msg) => {
            eprintln!("tiny-agent: {msg}");
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<bool, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(argv.first().map(String::as_str).unwrap_or(".")).to_path_buf();
    let apply = argv.iter().any(|a| a == "--apply");
    let provider = flag(&argv, "--provider");
    if argv.iter().any(|a| a == "--probe") {
        return probe(&argv);
    }
    let calls = collect_calls(&argv);
    if provider.is_none() && calls.is_empty() {
        return Err(
            "usage: tiny-agent <root> [--provider P --model M --goal G] [--call tool:args ...] [--apply]"
                .into(),
        );
    }

    let registry = build_registry(&root);
    let goal = flag(&argv, "--goal").unwrap_or_else(|| "run the requested tools".to_string());
    let mut llm: Box<dyn Llm> = match provider.as_deref() {
        Some("ccswitch" | "claude" | "anthropic") => build_anthropic(&argv, registry)?,
        Some(name) => build_http(name, &argv, registry)?,
        None => {
            let mut scripted = ScriptedLlm::new();
            for (tool, args) in &calls {
                scripted.push(tool, args).map_err(|e| e.to_string())?;
            }
            Box::new(scripted)
        }
    };

    let backend = detect_vcs();
    println!("root   : {}", root.display());
    println!("mode   : {}", if apply { "apply" } else { "dry-run" });
    println!("vcs    : {backend:?}");
    println!("llm    : {}", provider.as_deref().unwrap_or("scripted"));

    let mut host = StdHost {
        registry,
        backend,
        apply,
    };
    let outcome = drive(&goal, &mut *llm, registry, &mut host, Budget::mvp());
    report(&outcome);
    Ok(outcome.clean())
}

#[cfg(feature = "llm-http")]
fn build_http(
    provider: &str,
    argv: &[String],
    registry: &'static Registry,
) -> Result<Box<dyn Llm>, String> {
    use tiny_lilyco::agent::llm_http::{Config, HttpLlm};
    let config = Config::resolve(
        provider,
        flag(argv, "--base-url"),
        flag(argv, "--model"),
        flag(argv, "--api-key"),
    )?;
    println!("endpoint: {}", config.endpoint());
    Ok(Box::new(HttpLlm::new(config, registry)))
}

#[cfg(not(feature = "llm-http"))]
fn build_http(_p: &str, _a: &[String], _r: &'static Registry) -> Result<Box<dyn Llm>, String> {
    Err("built without the llm-http feature; rebuild with --features llm-http".into())
}

/// Value of `--name value`, if present.
fn flag(argv: &[String], name: &str) -> Option<String> {
    let at = argv.iter().position(|a| a == name)?;
    argv.get(at + 1).cloned()
}

fn collect_calls(argv: &[String]) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < argv.len() {
        if argv[i] != "--call" {
            i += 1;
            continue;
        }
        let (tool, args) = match argv[i + 1].split_once(':') {
            Some((t, a)) => (t, a.to_string()),
            None => (argv[i + 1].as_str(), String::new()),
        };
        let known = match tool {
            "read_file" => Some("read_file"),
            "list_dir" => Some("list_dir"),
            "edit_file" => Some("edit_file"),
            _ => None,
        };
        if let Some(name) = known {
            out.push((name, args));
        }
        i += 2;
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

#[cfg(feature = "llm-anthropic")]
fn anthropic_config(argv: &[String]) -> Result<(tiny_lilyco::agent::llm_anthropic::Config, String), String> {
    use tiny_lilyco::agent::llm_anthropic::{Config, SYSTEM};
    // Explicit flags win; cc-switch is only consulted for what is missing, so
    // the CLI stays usable on a machine without cc-switch installed.
    let active = tiny_lilyco::agent::ccswitch::load().ok();
    let pick = |name: &str, from: fn(&tiny_lilyco::agent::ccswitch::Active) -> String| {
        flag(argv, name).or_else(|| active.as_ref().map(from))
    };
    let base_url = pick("--base-url", |a| a.base_url.clone()).ok_or("missing --base-url")?;
    let api_key = pick("--api-key", |a| a.api_key.clone()).ok_or("missing --api-key")?;
    let model = pick("--model", |a| a.model.clone()).ok_or("missing --model")?;
    let provider = active.map(|a| a.provider).unwrap_or_else(|| "flags".to_string());
    let config = Config {
        base_url,
        api_key,
        model,
        system: SYSTEM.to_string(),
    };
    Ok((config, provider))
}

#[cfg(feature = "llm-anthropic")]
fn build_anthropic(argv: &[String], registry: &'static Registry) -> Result<Box<dyn Llm>, String> {
    use tiny_lilyco::agent::llm_anthropic::AnthropicLlm;
    let (config, provider) = anthropic_config(argv)?;
    println!("provider : {provider}");
    println!("endpoint : {}", config.endpoint());
    println!("model    : {}", config.model);
    Ok(Box::new(AnthropicLlm::new(config, registry)))
}

#[cfg(not(feature = "llm-anthropic"))]
fn build_anthropic(_a: &[String], _r: &'static Registry) -> Result<Box<dyn Llm>, String> {
    Err("built without the llm-anthropic feature".into())
}

/// One cheap round trip: tells a dead key from a dead endpoint.
#[cfg(feature = "llm-anthropic")]
fn probe(argv: &[String]) -> Result<bool, String> {
    use tiny_lilyco::agent::ccswitch::mask;
    use tiny_lilyco::agent::llm_anthropic::probe as round_trip;
    let (config, provider) = anthropic_config(argv)?;
    println!("provider : {provider}");
    println!("endpoint : {}", config.endpoint());
    println!("model    : {}", config.model);
    println!("key      : {}", mask(&config.api_key));
    match round_trip(&config) {
        Ok(text) => {
            println!("reply    : {text}");
            Ok(true)
        }
        Err(err) => {
            println!("error    : {err}");
            Ok(false)
        }
    }
}

#[cfg(not(feature = "llm-anthropic"))]
fn probe(_argv: &[String]) -> Result<bool, String> {
    Err("built without the llm-anthropic feature".into())
}