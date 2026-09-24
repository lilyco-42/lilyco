//! `lletsgal` — 故事 DSL → LetsGal Studio 工程编译器。
//!
//! 「一域一二进制 × 四端」：同一份 `Registry`，CLI 子命令 / TUI 选择页 /
//! Web `?cmd=` / MCP `tools/list`。三个命令对应创作流水线的三步：
//!
//! ```bash
//! lletsgal init --dir ./game --name 我的家乡     # 建工程骨架（T1 写盘）
//! lletsgal build --story story.txt --dir ./game  # DSL → 工程 + 自校验（T1 写盘）
//! lletsgal validate --dir ./game                 # 结构校验，只读（T0）
//! ```
//!
//! DSL 规范见 letsgal-ai 仓库 docs/STORY-DSL.md；`--schema` 打印整张注册表
//! 清单（agent 侧契约面），`--mcp` 起服务器由 agent 直调。

use lilyco::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lilyco_letsgal::{init_project, parse_story, validate_project, write_story_project};

/// 构建注册表（策略必须在 register 前就位 —— 门在注册那一刻包住 handler）
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<Build>(),
        RegisteredCommand::from_app::<Init>(),
        RegisteredCommand::from_app::<Validate>(),
    ];
    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 按调用面选择安全策略：agent 面拒 T1+，人面放行 T0+T1
fn policy_for(backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    match backend {
        lilyco::Backend::Mcp => Arc::new(DenyElevated),
        #[allow(unreachable_patterns)]
        _ => Arc::new(Interactive),
    }
}

fn main() {
    let backend = lilyco::detect_registry_backend();
    let reg = build_registry_with_policy(policy_for(backend));
    lilyco::run_registry_with("lletsgal", reg, backend);
}

/// 建工程骨架：chapters/ assets/ config/ + project.json + characters.json + scenes.json
#[derive(App)]
#[app(
    name = "init",
    run = "run_init",
    safety = "t1",
    about = "Create a LetsGal Studio project skeleton at --dir: chapters/, assets/, config/ directories plus project.json, characters.json, scenes.json and assets/.manifest.json (never overwrites existing files, matching the Node letsgal-ai init). Writes files (safety T1); returns { ok, dir, name }."
)]
struct Init {
    /// 工程输出目录
    dir: PathBuf,

    /// 游戏标题（缺省「新游戏」）
    #[arg(default_value = "新游戏")]
    name: Option<String>,
}

fn run_init(app: &Init, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let name = app.name.clone().unwrap_or_else(|| "新游戏".into());
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("初始化工程 {name}")),
    });
    init_project(&app.dir, &name).map_err(AppError::Runtime)?;
    ctx.log(LogLevel::Info, format!("已初始化: {}", app.dir.display()));
    let result = serde_json::json!({
        "ok": true,
        "dir": app.dir.display().to_string(),
        "name": name,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// DSL → 工程：解析、注册角色/场景、写章节、自校验
#[derive(App)]
#[app(
    name = "build",
    run = "run_build",
    safety = "t1",
    about = "Compile a story DSL file into a LetsGal project at --dir: parses the DSL (# chapter, ## fragment, !scene name uri, !bgm uri vol=45, !se uri, !curtain close, !wait 800, !particle LIGHT_SNOW particles/snow.png, !camera zoom=1.1 dur=3000, !choice opt->frag | opt2->frag, !call frag, Char(expr): line, 旁白：line, （stage action）; characters and scenes are auto-registered with deterministic stableId ids matching the Node letsgal-ai), upserts characters and scenes, clears expression/showCharacter for characters without portrait assets (phone/broadcast voices), writes chapters/*.json plus project.json chapterOrder with structured choices kept in lockstep with legacy optionsJson, then validates the project in place. Writes files (safety T1); returns { ok, chapters, characters, scenes, issues, warnings } where ok is true iff issues is empty."
)]
struct Build {
    /// Story DSL 文件
    story: PathBuf,

    /// 工程输出目录
    dir: PathBuf,

    /// 游戏标题（写进 project.json 的 name）
    name: Option<String>,
}

fn run_build(app: &Build, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let dsl = std::fs::read_to_string(&app.story)
        .map_err(|e| AppError::Runtime(format!("读取故事失败: {e}")))?;
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some("解析故事 DSL…".into()),
    });
    let mut story = parse_story(&dsl);
    ctx.tick(1, Some(2), "写入工程…");
    init_project(&app.dir, app.name.as_deref().unwrap_or("新游戏")).map_err(AppError::Runtime)?;
    write_story_project(&app.dir, &mut story, app.name.as_deref()).map_err(AppError::Runtime)?;
    let (issues, warnings) = validate_project(&app.dir).map_err(AppError::Runtime)?;
    let result = serde_json::json!({
        "ok": issues.is_empty(),
        "chapters": story.chapters.iter().map(|c| c["name"].clone()).collect::<Vec<_>>(),
        "characters": story.characters.iter().map(|c| c["name"].clone()).collect::<Vec<_>>(),
        "scenes": story.scenes.iter().map(|s| s["name"].clone()).collect::<Vec<_>>(),
        "issues": issues,
        "warnings": warnings,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 结构校验（只读）：文件齐全性 + 章节顺序 + 资产清单警告
#[derive(App)]
#[app(
    name = "validate",
    run = "run_validate",
    safety = "t0",
    about = "Validate a LetsGal project at --dir without writing anything: checks that project.json, characters.json, scenes.json, assets/.manifest.json and chapters/ exist, that every chapterOrder entry has a matching chapters/<name>.json, and collects warnings for scene/sound URIs that are not registered in the asset manifest. Read-only (safety T0); returns { ok, issues, warnings } where ok is true iff issues is empty."
)]
struct Validate {
    /// 工程目录
    dir: PathBuf,
}

fn run_validate(app: &Validate, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let (issues, warnings) = validate_project(&app.dir).map_err(AppError::Runtime)?;
    let result = serde_json::json!({
        "ok": issues.is_empty(),
        "issues": issues,
        "warnings": warnings,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::SafetyTier;

    /// 命令表是四端共同契约面：改名 = 破坏 agent 侧调用。
    /// Registry.commands 是 HashMap → 排序后比集合。
    #[test]
    fn registry_lists_exactly_the_three_pipeline_commands() {
        let mut names: Vec<String> = build_registry_with_policy(Arc::new(Interactive))
            .visible()
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        assert_eq!(names, vec!["build", "init", "validate"]);
    }

    /// 写盘命令必须 T1；validate 只读必须 T0 —— 分级错了 agent 面的门就错了
    #[test]
    fn safety_tiers_match_side_effects() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        for c in reg.visible() {
            let want = match c.name.as_str() {
                "validate" => SafetyTier::ReadOnly,
                _ => SafetyTier::Confirm,
            };
            assert_eq!(c.schema.safety, want, "`{}` 的分级与副作用不符", c.name);
        }
    }
}
