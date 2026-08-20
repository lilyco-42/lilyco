// ── main: one line（lilyco facade 四端自动分发）──
fn main() {
    lilyco::run::<Letsgal>();
}

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use lilyco_letsgal::{init_project, parse_story, validate_project, write_chapters};

/// 操作模式
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Build,
    Init,
    Validate,
}

/// 故事 DSL → LetsGal 工程生成器（AI 辅助创作）
#[derive(App)]
#[app(
    run = "run",
    about = "Compile a story DSL into a LetsGal Studio project. DSL: # chapter, ## fragment, !scene name uri, !bgm uri vol=45, !se uri, !curtain close, !wait 800, !particle LIGHT_SNOW particles/snow.png, !camera zoom=1.1 dur=3000, !choice opt->frag | opt2->frag, !call frag, Char(expr): line, 旁白：line, （stage action）. Characters and scenes are auto-registered."
)]
struct Letsgal {
    /// build | init | validate
    #[arg(about = "Operation mode")]
    mode: Mode,

    /// Story DSL 文件（build 用）
    #[arg(about = "Story DSL file (build)")]
    story: Option<PathBuf>,

    /// 工程目录（build/init 输出；validate 目标）
    #[arg(about = "Project directory")]
    dir: Option<PathBuf>,

    /// 游戏标题
    #[arg(about = "Game title")]
    name: Option<String>,
}

fn run(app: &Letsgal, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let dir = app
        .dir
        .clone()
        .ok_or_else(|| AppError::InvalidArg("--dir 必填".into()))?;
    match app.mode {
        Mode::Init => {
            let name = app.name.clone().unwrap_or_else(|| "新游戏".into());
            ctx.emit(Progress::Started {
                total: Some(1),
                message: Some(format!("初始化工程 {name}")),
            });
            init_project(&dir, &name).map_err(AppError::Runtime)?;
            ctx.log(LogLevel::Info, format!("已初始化: {}", dir.display()));
            let result =
                serde_json::json!({"ok": true, "dir": dir.display().to_string(), "name": name});
            ctx.done(result.clone(), start.elapsed().as_millis() as u64);
            Ok(result)
        }
        Mode::Build => {
            let story_path = app
                .story
                .clone()
                .ok_or_else(|| AppError::InvalidArg("build 需要 --story <dsl文件>".into()))?;
            let dsl = std::fs::read_to_string(&story_path)
                .map_err(|e| AppError::Runtime(format!("读取故事失败: {e}")))?;
            ctx.emit(Progress::Started {
                total: Some(2),
                message: Some("解析故事 DSL…".into()),
            });
            let story = parse_story(&dsl);
            ctx.tick(1, Some(2), "写入工程…");
            init_project(&dir, app.name.as_deref().unwrap_or("新游戏"))
                .map_err(AppError::Runtime)?;
            for c in &story.characters {
                let _ = lilyco_letsgal::upsert_character(&dir, c["name"].as_str().unwrap_or(""));
            }
            for s in &story.scenes {
                let _ = lilyco_letsgal::upsert_scene(&dir, s["name"].as_str().unwrap_or(""));
            }
            write_chapters(&dir, &story.chapters, app.name.as_deref())
                .map_err(AppError::Runtime)?;
            let (issues, warnings) = validate_project(&dir).map_err(AppError::Runtime)?;
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
        Mode::Validate => {
            let (issues, warnings) = validate_project(&dir).map_err(AppError::Runtime)?;
            let result = serde_json::json!({"ok": issues.is_empty(), "issues": issues, "warnings": warnings});
            ctx.done(result.clone(), start.elapsed().as_millis() as u64);
            Ok(result)
        }
    }
}
