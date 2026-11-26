use lilyco::prelude::*;

/// 视图组件协议示例：后端只声明 `Component`，前端自行渲染
///
/// ```bash
/// cargo run -p lilyco-example --example views -- --schema
/// cargo run -p lilyco-example --example views -- views --steps 3 --json-stream
/// ```
///
/// 页面渲染需要 `web` 特性；用 `../lilyco-template` 的 examples 看 GUI。
#[derive(App)]
#[app(
    name = "views",
    about = "演示后端声明组件、前端按协议渲染",
    run = "run_views"
)]
#[view(id = "job", title = "任务进度", kind = "progress")]
#[view(id = "summary", title = "结果表", kind = "table")]
struct Views {
    /// 步骤数
    #[arg(default = 5)]
    steps: u64,
}

fn run_views(app: &Views, ctx: &Context) -> Result<serde_json::Value, AppError> {
    for i in 0..=app.steps {
        ctx.view(
            "job",
            Component::Progress {
                label: Some(if i == app.steps {
                    "完成".into()
                } else {
                    format!("第 {i} 步")
                }),
                current: Some(i),
                total: Some(app.steps),
            },
        );
    }

    ctx.view(
        "summary",
        Component::Table {
            caption: Some("结果".into()),
            columns: vec!["项".into(), "值".into()],
            rows: vec![vec!["steps".into(), app.steps.to_string()]],
            sortable: false,
        },
    );
    ctx.view(
        "status",
        Component::Badge {
            text: "完成".into(),
            tone: Tone::Success,
        },
    );
    ctx.view(
        "note",
        Component::Alert {
            title: "提示".into(),
            description: Some("组件由后端声明，前端决定外观".into()),
            tone: Tone::Info,
        },
    );

    let r = serde_json::json!({ "steps": app.steps });
    ctx.done(r.clone(), 0);
    Ok(r)
}

fn main() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<Views>())
        .expect("注册 views");
    lilyco::run_registry("viewdemo", reg);
}
