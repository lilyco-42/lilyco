//! 页面渲染：`GET /` 把 CommandSchema 摊成一张表单（多命令模式带切换下拉）。
//!
//! 唯一的 HTML 出口，所以转义纪律集中在这里：任何插值进模板的动态内容先过
//! `html_escape`，进 JSON 元数据的先序列化再把 `</` 断掉。骨架见 `assets/index.html`。

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};

use lilyco_core::registry::{RegisteredCommand, Registry};
use lilyco_core::schema::ArgKind;

use crate::files::MAX_UPLOAD_BYTES;
use crate::state::AppState;
use crate::util::html_escape;

/// 按查询参数挑出要渲染的命令 schema（多命令模式）
///
/// `want` 必须命中可见命令（`registry.get` 含别名解析；隐藏命令不可导航），
/// 否则回退第一个可见命令。
pub(crate) fn pick_command<'r>(
    registry: &'r Registry,
    want: Option<&str>,
) -> &'r RegisteredCommand {
    want.and_then(|n| registry.get(n))
        .filter(|c| !c.hidden)
        .or_else(|| registry.visible().next())
        .expect("pick_command: registry has no visible commands")
}

pub(crate) async fn index(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // 多命令模式：?cmd= 决定渲染哪个命令的表单
    let schema = match &state.registry {
        Some(reg) => pick_command(reg, params.get("cmd").map(|x| x.as_str()))
            .schema
            .clone(),
        None => state.schema.as_ref().clone(),
    };

    // 命令切换下拉（可见命令 > 1 时出现）
    let mut cmd_nav = String::new();
    if let Some(reg) = &state.registry {
        let visible: Vec<&RegisteredCommand> = reg.visible().collect();
        if visible.len() > 1 {
            let mut opts = String::new();
            for c in &visible {
                let name = html_escape(&c.schema.name);
                let sel = if c.schema.name == schema.name {
                    " selected"
                } else {
                    ""
                };
                opts.push_str(&format!("<option value=\"{name}\"{sel}>{name}</option>"));
            }
            cmd_nav = format!(
                "<select id=\"cmd-nav\" aria-label=\"切换命令\" onchange=\"if(this.value)location='/?cmd='+encodeURIComponent(this.value)\">{opts}</select>"
            );
        }
    }

    let mut fields_html = String::new();
    let mut field_meta: Vec<serde_json::Value> = Vec::new();

    for arg in &schema.args {
        field_meta.push(serde_json::json!({
            "name": arg.name,
            "kind": kind_name(&arg.kind),
            "required": arg.required,
        }));

        let esc_name = html_escape(&arg.name);
        let req_mark = if arg.required {
            "<span class=\"req-mark\" aria-hidden=\"true\">*</span>"
        } else {
            ""
        };
        let label = format!("{}{}", html_escape(&arg.about), req_mark);
        let req_a = if arg.required { " required" } else { "" };

        let widget = match &arg.kind {
            ArgKind::Flag => {
                let ck = matches!(&arg.default, Some(serde_json::Value::Bool(true)))
                    .then_some(" checked")
                    .unwrap_or("");
                format!(
                    "<label class=\"flag-row\"><input type=\"checkbox\" id=\"field-{esc_name}\"{ck}> \
                     <span>{label}</span></label>"
                )
            }
            ArgKind::Text => {
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                format!(
                    "<input type=\"text\" id=\"field-{esc_name}\" placeholder=\"{}\"{req_a} value=\"{}\">",
                    html_escape(&arg.about),
                    html_escape(dv),
                )
            }
            ArgKind::Path { must_exist } => {
                // 文件输入组件：手动路径 + 拖拽/点击上传（上传后回填服务端暂存绝对路径）
                let dv = arg.default.as_ref().and_then(|d| d.as_str()).unwrap_or("");
                let must_attr = if *must_exist { "1" } else { "0" };
                let hint = if *must_exist {
                    "拖拽文件到此处，或点击选择 —— 上传后自动回填服务端路径"
                } else {
                    "可选：拖拽文件上传并回填路径，或直接手填"
                };
                // 「本机」按钮：让服务端弹系统选择器，回填磁盘上的原始路径（不上传副本）。
                // 没开 pick 特性的构建不画它，免得点了只得到一句报错。
                let pick_btn = if cfg!(feature = "pick") {
                    format!(
                        "<button type=\"button\" class=\"btn-icon dz-pick\" data-pick=\"{esc_name}\" \
                         aria-label=\"用系统选择器挑本机文件\" title=\"用系统选择器挑本机文件（只回填真实路径，不上传副本）\">本机</button>"
                    )
                } else {
                    String::new()
                };
                format!(
                    "<input type=\"text\" id=\"field-{esc_name}\" class=\"mono\" placeholder=\"{}\"{req_a} value=\"{}\" spellcheck=\"false\">\
                     <div class=\"dropzone\" data-target=\"{esc_name}\" data-must-exist=\"{must_attr}\" data-max-upload=\"{MAX_UPLOAD_BYTES}\" tabindex=\"0\" role=\"button\" aria-label=\"上传文件\">\
                     <input type=\"file\" class=\"visually-hidden\" data-file-for=\"{esc_name}\" tabindex=\"-1\">\
                     <span class=\"dz-icon\">⇪</span><span class=\"dz-hint\">{hint}</span>\
                     <span class=\"dz-status\" id=\"up-{esc_name}\" aria-live=\"polite\"></span>\
                     <span class=\"file-chip\" id=\"chip-{esc_name}\" hidden></span>{pick_btn}</div>",
                    html_escape(&arg.about),
                    html_escape(dv),
                )
            }
            ArgKind::Number { min, max } => {
                let dv = arg
                    .default
                    .as_ref()
                    .and_then(|d| d.as_f64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| min.map(|m| m.to_string()).unwrap_or_default());
                let min_a = min.map(|m| format!(" min=\"{m}\"")).unwrap_or_default();
                let max_a = max.map(|m| format!(" max=\"{m}\"")).unwrap_or_default();
                format!(
                    "<input type=\"number\" id=\"field-{esc_name}\" value=\"{}\" step=\"any\"{min_a}{max_a}{req_a}>",
                    html_escape(&dv),
                )
            }
            ArgKind::Enum { values } => {
                let mut opts = String::new();
                for v in values {
                    let ev = html_escape(v);
                    let sel = if arg.default.as_ref().and_then(|d| d.as_str()) == Some(v.as_str()) {
                        " selected"
                    } else {
                        ""
                    };
                    opts.push_str(&format!("<option value=\"{ev}\"{sel}>{ev}</option>"));
                }
                format!("<select id=\"field-{esc_name}\"{req_a}>{opts}</select>")
            }
            ArgKind::List { .. } => {
                // 动态行：默认 2 行 + 增删按钮（收集时按 data-list 前缀聚合）
                format!(
                    "<div class=\"list-rows\" id=\"list-{esc_name}\" data-list=\"{esc_name}\">\
                     <div class=\"list-row\"><input type=\"text\" class=\"mono\" data-list-item=\"{esc_name}\" placeholder=\"{}\"><button type=\"button\" class=\"btn-icon row-del\" aria-label=\"删除该行\" onclick=\"this.closest('.list-row').remove()\">✕</button></div>\
                     <div class=\"list-row\"><input type=\"text\" class=\"mono\" data-list-item=\"{esc_name}\" placeholder=\"{}\"><button type=\"button\" class=\"btn-icon row-del\" aria-label=\"删除该行\" onclick=\"this.closest('.list-row').remove()\">✕</button></div>\
                     </div>\
                     <button type=\"button\" class=\"btn-icon list-add\" data-list-add=\"{esc_name}\">＋ 添加一项</button>",
                    html_escape(&arg.about),
                    html_escape(&arg.about),
                )
            }
        };

        if matches!(&arg.kind, ArgKind::Flag) {
            fields_html.push_str(&format!("<div class=\"field field-flag\">{widget}</div>\n"));
        } else {
            fields_html.push_str(&format!(
                "<div class=\"field\"><label for=\"field-{esc_name}\">{label}</label>{widget}</div>\n"
            ));
        }
    }

    // JSON 元数据（含 </script> 防 breakout 转义）
    let meta_json = serde_json::to_string(&field_meta)
        .unwrap_or_else(|_| "[]".into())
        .replace("</", "<\\/");
    let cmd_json = serde_json::json!(schema.name).to_string();
    let about_html = html_escape(&schema.about);
    let cmd_html = html_escape(&schema.name);

    let html = HTML_TEMPLATE
        .replace("__CSS__", include_str!("../assets/app.css"))
        .replace("__CMD_NAV__", &cmd_nav)
        .replace("__FIELDS__", &fields_html)
        .replace("__ABOUT__", &about_html)
        .replace("__CMD_NAME__", &cmd_html)
        .replace("__CMD_JS__", &cmd_json)
        .replace("__META__", &meta_json)
        // token 最后注入且 token 为随机字母数字，无碰撞风险
        .replace("__TOKEN__", &state.token);

    let mut resp = Html(html).into_response();
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         connect-src 'self'; img-src data:; base-uri 'none'"
            .parse()
            .unwrap(),
    );
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    resp
}

fn kind_name(kind: &ArgKind) -> &'static str {
    match kind {
        ArgKind::Flag => "Flag",
        ArgKind::Text => "Text",
        ArgKind::Number { .. } => "Number",
        ArgKind::Enum { .. } => "Enum",
        ArgKind::Path { .. } => "Path",
        ArgKind::List { .. } => "List",
    }
}

/// 页面骨架（内嵌 CSS 占位 + 前端逻辑）。占位符用 __X__ 哨兵（避免与动态内容中的
/// 花括号冲突）；动态内容一律在服务端 html_escape / JSON 编码后才插入。
const HTML_TEMPLATE: &str = include_str!("../assets/index.html");

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::{Query, State};
    use tokio::sync::Mutex;

    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};

    use super::*;
    use crate::state::fixture::{
        arg, body_of, registry_state, schema_of, state_with, two_command_registry,
    };

    #[test]
    fn pick_command_defaults_to_first_visible() {
        let reg = two_command_registry();
        assert_eq!(pick_command(&reg, None).schema.name, "ping");
        assert_eq!(pick_command(&reg, Some("nope")).schema.name, "ping");
    }

    #[test]
    fn pick_command_hidden_falls_back() {
        let reg = two_command_registry();
        assert_eq!(pick_command(&reg, Some("secret")).schema.name, "ping");
    }

    #[tokio::test]
    async fn index_renders_selected_command_and_nav() {
        let state = registry_state();
        let mut params = HashMap::new();
        params.insert("cmd".to_string(), "secret".to_string());
        let resp = index(State(state.clone()), Query(params)).await;
        // hidden 不可导航 → 回退第一个可见命令
        let body = body_of(resp).await;
        assert!(body.contains("ping"), "fallback to first visible");
        assert!(body.contains("select"), "nav dropdown expected");
    }

    /// Path 字段除了拖拽区还要带「本机」按钮（拿原始路径，不走上传副本）；数字字段不带
    #[tokio::test]
    async fn path_fields_get_the_native_pick_button() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "pick".into(),
                about: "pick".into(),
                args: vec![
                    ArgSchema {
                        name: "file".into(),
                        about: "文件".into(),
                        kind: ArgKind::Path { must_exist: false },
                        required: true,
                        default: None,
                    },
                    ArgSchema {
                        name: "quality".into(),
                        about: "质量".into(),
                        kind: ArgKind::Number {
                            min: Some(0.0),
                            max: Some(51.0),
                        },
                        required: false,
                        default: Some(serde_json::json!(23)),
                    },
                ],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let body = body_of(index(State(state), Query(HashMap::new())).await).await;
        assert!(body.contains("dropzone"), "Path 参数要有拖拽区");
        if cfg!(feature = "pick") {
            assert!(
                body.contains("data-pick=\"file\"") && body.contains("fetch(\"/pick\""),
                "原生选择器按钮或它的请求没了"
            );
        }
        assert!(
            !body.contains("data-pick=\"quality\""),
            "非 Path 字段不该挂选择器"
        );
    }

    #[tokio::test]
    async fn index_escapes_malicious_about() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "evil".into(),
                about: "<script>alert(1)</script>".into(),
                args: vec![ArgSchema {
                    name: "p".into(),
                    about: "\"><img src=x>".into(),
                    kind: ArgKind::Text,
                    required: false,
                    default: Some(serde_json::json!("\"><svg>")),
                }],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let resp = index(State(state), Query(HashMap::new())).await;
        let body = body_of(resp).await;
        assert!(!body.contains("<script>alert"), "about must be escaped");
        assert!(!body.contains("<img src=x>"), "about must be escaped");
        assert!(!body.contains("\"><svg>"), "default must be escaped");
    }

    #[tokio::test]
    async fn index_renders_dropzone_for_path_args() {
        let state = Arc::new(AppState {
            schema: Arc::new(CommandSchema {
                name: "up".into(),
                about: "upload".into(),
                args: vec![ArgSchema {
                    name: "file".into(),
                    about: "文件".into(),
                    kind: ArgKind::Path { must_exist: true },
                    required: true,
                    default: None,
                }],
                subcommands: vec![],
                safety: lilyco_core::safety::SafetyTier::ReadOnly,
            }),
            registry: None,
            sessions: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            runner: Arc::new(|_, _| Box::pin(async {})),
            token: "t".into(),
        });
        let resp = index(State(state), Query(HashMap::new())).await;
        let body = body_of(resp).await;
        assert!(body.contains("dropzone"), "Path 参数必须有拖拽上传组件");
        assert!(
            body.contains("data-must-exist=\"1\""),
            "must_exist 语义保留"
        );
        assert!(body.contains("type=\"file\""), "必须有文件选择入口");
        // 上限只有一个来源：服务端常量注入 DOM，页面读它来拦超大文件。
        // 谁把 200 抄进 JS 或文档，这条就会红。
        assert!(
            body.contains(&format!("data-max-upload=\"{MAX_UPLOAD_BYTES}\"")),
            "拖拽区没带上服务端注入的体积上限"
        );
    }

    /// 页面 JS 拿 `$("field-" + dz.dataset.target)` 找输入框，所以 `data-target` 必须是
    /// **裸参数名**。这里曾经发的是 `field-path`，JS 拼成 `field-field-path` 取到 null，
    /// 上传明明成功了，页面却报「上传失败：Cannot set properties of null」——
    /// 只有真在浏览器里拖一个文件进去才看得见。
    #[tokio::test]
    async fn dropzone_ids_match_what_the_page_looks_up() {
        let state = state_with(schema_of(
            "up",
            "upload",
            vec![arg(
                "file",
                "文件",
                ArgKind::Path { must_exist: true },
                true,
                None,
            )],
        ));
        let body = body_of(index(State(state), Query(HashMap::new())).await).await;
        let target = body
            .split("data-target=\"")
            .nth(1)
            .expect("dropzone 要有 data-target")
            .split('"')
            .next()
            .unwrap()
            .to_string();
        assert_eq!(target, "file", "data-target 要的是裸参数名，不是 id");
        // JS 拼出来的那几个 id 必须真的在页面里
        assert!(
            body.contains(&format!("id=\"field-{target}\"")),
            "输入框 id 对不上 JS 的拼法"
        );
        assert!(
            body.contains(&format!("id=\"up-{target}\"")),
            "状态行 id 对不上 /pick 的拼法"
        );
        assert!(
            !cfg!(feature = "pick") || body.contains(&format!("data-pick=\"{target}\"")),
            "本机按钮的 data-pick 与 data-target 得用同一套名字"
        );
    }
}
