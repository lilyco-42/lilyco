//! `lilyco doc` — 能力表生成器。
//!
//! 跑 app 的根级 `--schema`（`Registry` 的 JSON 清单，agent 侧的机器契约），
//! 产出两份材料：
//!
//! 1. `capabilities.json` —— app `--schema` 输出的**逐字**快照。这是给 agent
//!    的契约，逐字纪律与 helpfixtures 同源：改写 = 静默破坏调用方。
//! 2. `CAPABILITIES.md` —— 人读的渲染版：命令 × 安全分级 × 参数表。
//!
//! 为什么这个命令是生态心脏：cargo doc 给人看 API，lilyco doc 给 **AI** 看
//! 能力 —— 「给 AI 使用的软件」的第一入口就是这张表（MCP tools/list 的
//! 前置材料）。目标 app 必须是模板式 Registry 应用（根级 `--schema` 可用）。

use lilyco::prelude::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 用 `cargo metadata --no-deps` 发现项目里的 bin 目标（跨平台，免解析 toml）
pub fn discover_bins(project: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(project)
        .output()
        .map_err(|e| format!("无法启动 cargo: {e}"))?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let last = tail.lines().next_back().unwrap_or("");
        return Err(format!("cargo metadata 失败: {last}"));
    }
    let v: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("metadata JSON 解析失败: {e}"))?;
    let mut bins = Vec::new();
    if let Some(pkgs) = v["packages"].as_array() {
        for pkg in pkgs {
            if let Some(targets) = pkg["targets"].as_array() {
                for t in targets {
                    let is_bin = t["kind"]
                        .as_array()
                        .map(|k| k.iter().any(|x| x.as_str() == Some("bin")))
                        .unwrap_or(false);
                    if is_bin {
                        if let Some(n) = t["name"].as_str() {
                            bins.push(n.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(bins)
}

/// 跑 `<bin> --schema` 捕获注册表 JSON 清单（stdout 逐字返回）
pub fn capture_schema(project: &Path, bin: &str) -> Result<String, String> {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--bin", bin, "--", "--schema"])
        .current_dir(project)
        .output()
        .map_err(|e| format!("无法启动 cargo: {e}"))?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let last = tail.lines().next_back().unwrap_or("");
        return Err(format!(
            "`cargo run --bin {bin} -- --schema` 失败（exit {}）: {last}",
            out.status.code().unwrap_or(1)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if stdout.trim().is_empty() {
        return Err(format!(
            "`{bin} --schema` 没有输出 —— 目标是模板式 Registry 应用吗？（单命令应用没有根级 --schema）"
        ));
    }
    Ok(stdout)
}

/// 安全分级的展示标签（lenient：认不出就原样展示）
fn tier_label(v: &Value) -> String {
    match v.as_str().unwrap_or("") {
        "read_only" | "t0" => "T0 只读".to_string(),
        "confirm" | "t1" => "T1 需确认".to_string(),
        "token" | "t2" => "T2 需令牌".to_string(),
        "never_auto" | "t3" => "T3 禁止自动化".to_string(),
        "" => "T0 只读（schema 未标）".to_string(),
        other => format!("`{other}`"),
    }
}

/// 参数类型的展示标签（读 serde tag `type`，lenient）
fn kind_label(kind: &Value) -> String {
    let t = kind["type"].as_str().unwrap_or("text");
    match t {
        "flag" => "flag".to_string(),
        "text" => "text".to_string(),
        "number" => {
            let min = kind["min"].as_f64();
            let max = kind["max"].as_f64();
            match (min, max) {
                (Some(a), Some(b)) => format!("number[{a}..={b}]"),
                (Some(a), None) => format!("number[≥{a}]"),
                (None, Some(b)) => format!("number[≤{b}]"),
                _ => "number".to_string(),
            }
        }
        "enum" => {
            let vals: Vec<&str> = kind["values"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            format!("enum [{}]", vals.join("|"))
        }
        "path" => {
            if kind["must_exist"].as_bool().unwrap_or(false) {
                "path（须存在）".to_string()
            } else {
                "path".to_string()
            }
        }
        "list" => {
            // Index 已返回 &Value（缺失键是 &Value::Null，不 panic）——用 .get 拿 Option
            let inner = kind
                .get("item")
                .map(kind_label)
                .unwrap_or_else(|| "text".to_string());
            format!("list<{inner}>")
        }
        other => other.to_string(),
    }
}

/// 把 `--schema` 清单渲染成 markdown 能力表（lenient 解析：名字可落在
/// 条目顶层或 schema 里，字段缺省不炸 —— 契约形状小幅漂移时仍能出表）
pub fn render_markdown(manifest: &str, bin: &str) -> Result<String, String> {
    let entries: Vec<Value> = serde_json::from_str(manifest.trim())
        .map_err(|e| format!("manifest 不是 JSON 数组（app 的 --schema 输出）: {e}"))?;
    let mut md = String::new();
    md.push_str(&format!("# {bin} 能力表（capabilities）\n\n"));
    md.push_str("> 由 `lilyco doc` 生成。机器契约是同目录 `capabilities.json`（app `--schema` 的逐字输出），本文件是人读的渲染版。\n>\n> 安全分级：**T0** 只读自动放行 · **T1** 需人工确认 · **T2** 需能力令牌 · **T3** 禁止自动化执行。\n\n");
    md.push_str(&format!("共 {} 条命令。\n\n", entries.len()));
    for e in &entries {
        let schema = &e["schema"];
        let name = e["name"]
            .as_str()
            .or_else(|| schema["name"].as_str())
            .unwrap_or("?");
        let hidden = e["hidden"].as_bool().unwrap_or(false);
        let aliases: Vec<&str> = e["aliases"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let about = schema["about"].as_str().unwrap_or("");
        let tier = tier_label(&schema["safety"]);

        md.push_str(&format!("## `{name}` — {tier}\n\n"));
        if hidden {
            md.push_str("（隐藏命令：不出现在 help / TUI 选择页 / MCP tools/list，可直调）\n\n");
        }
        if !aliases.is_empty() {
            md.push_str(&format!("别名：{}\n\n", aliases.join("、")));
        }
        if !about.is_empty() {
            md.push_str(&format!("{about}\n\n"));
        }
        match schema["args"].as_array() {
            Some(args) if !args.is_empty() => {
                md.push_str("| 参数 | 类型 | 必填 | 缺省 | 说明 |\n|---|---|---|---|---|\n");
                for arg in args {
                    let aname = arg["name"].as_str().unwrap_or("?");
                    let klabel = kind_label(&arg["kind"]);
                    let req = if arg["required"].as_bool().unwrap_or(false) {
                        "✓"
                    } else {
                        ""
                    };
                    let dflt = match &arg["default"] {
                        Value::Null => "—".to_string(),
                        v => v.to_string(),
                    };
                    let desc = arg["about"].as_str().unwrap_or("").replace('|', "\\|");
                    md.push_str(&format!(
                        "| `{aname}` | {klabel} | {req} | {dflt} | {desc} |\n"
                    ));
                }
                md.push('\n');
            }
            _ => md.push_str("（无参数）\n\n"),
        }
    }
    Ok(md)
}

/// 写出两份材料到 `out_dir`（缺省 = project），返回 (json 路径, md 路径)
pub fn write_docs(
    project: &Path,
    out_dir: Option<&Path>,
    manifest: &str,
    bin: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let out = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project.to_path_buf());
    std::fs::create_dir_all(&out).map_err(|e| format!("建输出目录失败: {e}"))?;
    let json_path = out.join("capabilities.json");
    let md_path = out.join("CAPABILITIES.md");
    // 逐字纪律：capabilities.json 是 app --schema 的原样输出，一字不改
    std::fs::write(&json_path, manifest)
        .map_err(|e| format!("写 {} 失败: {e}", json_path.display()))?;
    let md = render_markdown(manifest, bin)?;
    std::fs::write(&md_path, &md).map_err(|e| format!("写 {} 失败: {e}", md_path.display()))?;
    Ok((json_path, md_path))
}

/// `lilyco doc` — 生成 app 的能力表
#[derive(App)]
#[app(
    name = "doc",
    run = "run_doc",
    safety = "t1",
    about = "Generate the capability table of a four-surface app: discovers binaries via `cargo metadata --no-deps`, runs the app's root --schema (registry-style apps from the domain template) and writes capabilities.json (verbatim schema manifest, the machine contract for agents) plus CAPABILITIES.md (rendered table: command x safety tier x args) into the project directory. --json instead prints the raw manifest to stdout without writing files. External process invocation plus file writes (safety T1); returns { bin, capabilities_json, capabilities_md } or { mode: stdout, bin }."
)]
pub struct Doc {
    /// 项目目录（缺省 = 当前目录）
    pub project: Option<PathBuf>,

    /// 目标二进制（目录里唯一 bin 时自动选，多 bin 必须指名）
    pub bin: Option<String>,

    /// 输出目录（缺省 = project）
    pub out_dir: Option<PathBuf>,

    /// 只把 manifest 原样打到 stdout，不写文件
    pub json: bool,
}

fn run_doc(app: &Doc, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let project = app.project.clone().unwrap_or_else(|| PathBuf::from("."));
    ctx.emit(Progress::Started {
        total: Some(3),
        message: Some("discovering binaries (cargo metadata)".to_string()),
    });
    let bins = discover_bins(&project).map_err(AppError::Runtime)?;
    let bin = match &app.bin {
        Some(b) => b.clone(),
        None => match bins.as_slice() {
            [one] => one.clone(),
            [] => {
                return Err(AppError::Runtime(
                    "没有发现任何二进制（cargo metadata）—— 这是 lilyco 应用项目吗？".to_string(),
                ))
            }
            many => {
                return Err(AppError::InvalidArg(format!(
                    "发现多个二进制，用 --bin 指名：{}",
                    many.join(", ")
                )))
            }
        },
    };
    ctx.tick(1, Some(3), &format!("capturing schema of {bin}"));
    let manifest = capture_schema(&project, &bin).map_err(AppError::Runtime)?;
    ctx.tick(2, Some(3), "writing capability table");
    if app.json {
        println!("{manifest}");
        let result = json!({ "mode": "stdout", "bin": bin });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    let (json_path, md_path) =
        write_docs(&project, app.out_dir.as_deref(), &manifest, &bin).map_err(AppError::Runtime)?;
    let result = json!({
        "bin": bin,
        "capabilities_json": json_path.to_string_lossy(),
        "capabilities_md": md_path.to_string_lossy(),
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 与 lilyco-core `Registry::to_json` 同形状的最小清单
    ///（RegisteredCommand: {name, aliases, hidden, schema}）
    fn sample_manifest() -> String {
        json!([
            {
                "name": "show",
                "aliases": ["s"],
                "hidden": false,
                "schema": {
                    "name": "show",
                    "about": "Count lines and bytes of a text file.",
                    "args": [
                        { "name": "path", "about": "File to show", "kind": { "type": "path", "must_exist": true }, "required": true, "default": null },
                        { "name": "lines", "about": "Max lines", "kind": { "type": "number", "min": null, "max": 100 }, "required": false, "default": 20 },
                        { "name": "mode", "about": "Output mode", "kind": { "type": "enum", "values": ["json", "text"] }, "required": false, "default": null },
                        { "name": "verbose", "about": "Verbose", "kind": { "type": "flag" }, "required": false, "default": null },
                        { "name": "tag", "about": "Tags", "kind": { "type": "list", "item": { "type": "text" } }, "required": false, "default": null }
                    ],
                    "subcommands": [],
                    "safety": "t1"
                }
            },
            {
                "name": "fix",
                "aliases": [],
                "hidden": true,
                "schema": {
                    "name": "fix",
                    "about": "Hidden maintenance command.",
                    "args": [],
                    "subcommands": [],
                    "safety": "read_only"
                }
            }
        ])
        .to_string()
    }

    #[test]
    fn tier_labels_cover_all_four_tiers() {
        assert_eq!(tier_label(&json!("read_only")), "T0 只读");
        assert_eq!(tier_label(&json!("t0")), "T0 只读");
        assert_eq!(tier_label(&json!("confirm")), "T1 需确认");
        assert_eq!(tier_label(&json!("token")), "T2 需令牌");
        assert_eq!(tier_label(&json!("never_auto")), "T3 禁止自动化");
        assert_eq!(tier_label(&json!("weird")), "`weird`", "认不出就原样展示");
    }

    #[test]
    fn kind_labels_cover_every_arg_kind() {
        assert_eq!(kind_label(&json!({"type": "flag"})), "flag");
        assert_eq!(kind_label(&json!({"type": "text"})), "text");
        assert_eq!(
            kind_label(&json!({"type": "number", "min": 1, "max": 9})),
            "number[1..=9]"
        );
        assert_eq!(
            kind_label(&json!({"type": "number", "min": null, "max": 9})),
            "number[≤9]"
        );
        assert_eq!(
            kind_label(&json!({"type": "enum", "values": ["a", "b"]})),
            "enum [a|b]"
        );
        assert_eq!(
            kind_label(&json!({"type": "path", "must_exist": true})),
            "path（须存在）"
        );
        assert_eq!(
            kind_label(&json!({"type": "list", "item": {"type": "text"}})),
            "list<text>"
        );
    }

    #[test]
    fn markdown_renders_commands_tiers_args_and_hidden_marker() {
        let md = render_markdown(&sample_manifest(), "ltodo").expect("渲染应成功");
        assert!(md.contains("# ltodo 能力表"), "标题带 bin 名: {md}");
        assert!(md.contains("## `show` — T1 需确认"));
        assert!(md.contains("## `fix` — T0 只读"));
        assert!(md.contains("隐藏命令"), "隐藏命令要有标记");
        assert!(md.contains("别名：s"));
        assert!(md.contains("| `path` | path（须存在） | ✓ | — |"));
        assert!(md.contains("| `lines` | number[≤100] |  | 20 |"));
        assert!(md.contains("enum [json|text]"));
        assert!(md.contains("list<text>"));
        assert!(
            md.contains("机器契约"),
            "要指明 capabilities.json 是机器契约"
        );
    }

    #[test]
    fn render_rejects_non_array_manifest() {
        assert!(render_markdown("not json", "x").is_err());
        assert!(
            render_markdown("{\"a\": 1}", "x").is_err(),
            "对象不是合法清单"
        );
    }

    #[test]
    fn write_docs_writes_verbatim_json_and_rendered_md() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest = sample_manifest();
        let (jp, mp) = write_docs(tmp.path(), None, &manifest, "ltodo").expect("写文档应成功");
        let on_disk = std::fs::read_to_string(&jp).expect("读 json");
        assert_eq!(
            on_disk, manifest,
            "capabilities.json 必须逐字等于 --schema 输出"
        );
        let md = std::fs::read_to_string(&mp).expect("读 md");
        assert!(md.contains("# ltodo 能力表"));
    }

    /// cargo metadata 在无 Cargo.toml 的目录必须报错（负路径冒烟，不真构建）
    #[test]
    fn discover_bins_errors_outside_a_cargo_project() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(discover_bins(tmp.path()).is_err());
    }
}
