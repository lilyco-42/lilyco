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

use lilyco_no_std::adapters::snapshot::EditJournal;
use lilyco_no_std::agent::llm::{Llm, ScriptedLlm};
use lilyco_no_std::agent::registry::Registry;
use lilyco_no_std::agent::runtime::{drive, Budget, Host, Outcome};
use lilyco_no_std::agent::tool::{ObsBuf, ObsKind, Tool, ToolError};
use lilyco_no_std::agent::tools::{AstGrep, EditFile, Glob, Grep, ListDir, ReadFile};
use lilyco_no_std::agent::Action;

struct StdHost {
    registry: &'static Registry,
    apply: bool,
    journal: EditJournal,
}

impl Host for StdHost {
    fn checkpoint(&mut self, _label: &str) -> Result<(), &'static str> {
        Err("checkpoint requires an edit invocation")
    }

    fn checkpoint_action(&mut self, action: &Action<'_>) -> Result<(), &'static str> {
        if !self.apply {
            return Err("dry run: pass --apply to edit files");
        }
        if action.tool.name != "edit_file" {
            return self.checkpoint(action.tool.name);
        }
        self.journal.before_edit(action.args)
    }

    fn undo(&mut self) -> Result<(), &'static str> {
        self.journal.undo()
    }

    fn call(&mut self, tool: &str, args: &str, out: &mut ObsBuf) -> Result<ObsKind, ToolError> {
        let handle = self.registry.find(tool).ok_or(ToolError::UnknownTool)?;
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

    let wasm_specs = collect_wasm(&argv);
    let registry = build_registry(&root, &wasm_specs);
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

    println!("root   : {}", root.display());
    println!("mode   : {}", if apply { "apply" } else { "dry-run" });
    println!("llm    : {}", provider.as_deref().unwrap_or("scripted"));

    let mut host = StdHost {
        registry,
        apply,
        journal: EditJournal::new(root),
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
    use lilyco_no_std::agent::llm_http::{Config, HttpLlm};
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

fn collect_calls(argv: &[String]) -> Vec<(String, String)> {
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
        // Any name is accepted: wasm plugins choose their own tool name.
        out.push((tool.to_string(), args));
        i += 2;
    }
    out
}

/// Collect `--wasm-tool name=path` pairs.
fn collect_wasm(argv: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < argv.len() {
        if argv[i] == "--wasm-tool" {
            if let Some((name, path)) = argv[i + 1].split_once('=') {
                out.push((name.to_string(), path.to_string()));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

fn build_registry(root: &Path, wasm: &[(String, String)]) -> &'static Registry {
    let read: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(ReadFile::new(root)));
    let list: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(ListDir::new(root)));
    let edit: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(EditFile::new(root)));
    let grep: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(Grep::new(root)));
    let glob: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(Glob::new(root)));
    let ast: &'static (dyn Tool + Send + Sync) =
        Box::leak(Box::new(AstGrep::new(root, AstGrep::locate())));
    let reg: &'static mut Registry = Box::leak(Box::new(Registry::new()));
    let _ = reg.register(read);
    let _ = reg.register(list);
    let _ = reg.register(edit);
    let _ = reg.register(grep);
    let _ = reg.register(glob);
    let _ = reg.register(ast);
    #[cfg(feature = "wasm-tools")]
    for (name, path) in wasm {
        let label: &'static str = Box::leak(name.clone().into_boxed_str());
        match lilyco_no_std::agent::wasm_tool::WasmTool::load(label, Path::new(path)) {
            Ok(tool) => {
                let leaked: &'static (dyn Tool + Send + Sync) = Box::leak(Box::new(tool));
                let _ = reg.register(leaked);
                println!("wasm   : {name} ({path})");
            }
            Err(err) => println!("wasm   : {name} failed to load · {err}"),
        }
    }
    let _ = wasm;
    reg
}

fn report(outcome: &Outcome) {
    println!(
        "result : {} calls, {} failures, {} checkpoints, rolled_back={}",
        outcome.calls, outcome.failures, outcome.checkpoints, outcome.rolled_back
    );
    println!("phase  : {}", outcome.phase);
    if let Some(reason) = outcome.stopped_by {
        eprintln!("stopped: {reason}");
    }
}

#[cfg(feature = "llm-anthropic")]
fn anthropic_config(
    argv: &[String],
) -> Result<(lilyco_no_std::agent::llm_anthropic::Config, String), String> {
    use lilyco_no_std::agent::llm_anthropic::{Config, SYSTEM};
    // Explicit flags win; cc-switch is only consulted for what is missing, so
    // the CLI stays usable on a machine without cc-switch installed.
    let active = lilyco_no_std::agent::ccswitch::load().ok();
    let pick = |name: &str, from: fn(&lilyco_no_std::agent::ccswitch::Active) -> String| {
        flag(argv, name).or_else(|| active.as_ref().map(from))
    };
    let base_url = pick("--base-url", |a| a.base_url.clone()).ok_or("missing --base-url")?;
    let api_key = pick("--api-key", |a| a.api_key.clone()).ok_or("missing --api-key")?;
    let model = pick("--model", |a| a.model.clone()).ok_or("missing --model")?;
    let provider = active
        .map(|a| a.provider)
        .unwrap_or_else(|| "flags".to_string());
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
    use lilyco_no_std::agent::llm_anthropic::AnthropicLlm;
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
    use lilyco_no_std::agent::ccswitch::mask;
    use lilyco_no_std::agent::llm_anthropic::probe as round_trip;
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
