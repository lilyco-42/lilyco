//! 页面渲染：`GET /` 把 CommandSchema 摊成一张表单（多命令模式带切换下拉）。
//!
//! 这里是全 crate 唯一的 HTML 出口，所以两件事集中在此：
//! - **转义纪律**：任何插值进模板的动态内容先过 `html_escape`；进 JSON 元数据的先序列化
//!   再把 `</` 断掉。
//! - **组件目录**：一个 `ArgKind` 一个组件函数（`widget_*`），标签与布局在 `field_shell`，
//!   页面骨架是 `assets/index.html`。每个组件的构成、七态与用到的令牌见
//!   `lilyco-gui/DESIGN.md` §10 —— 改组件要同时改那张表，两边不许各说各话。
//!
//! 组件在 DOM 上自报家门：`data-component="<名字>"` 是**单测与调试**的锚点（行为层的装配
//! 锚点是 `assets/index.html` 里 `BOOT` 那份 `init*` 清单，不是这个属性）。
//! HTML 片段一律用 raw string 写 —— 满屏 `\"` 转义正是这类代码最容易看错的地方。

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};

use lilyco_core::registry::{RegisteredCommand, Registry};
use lilyco_core::schema::{ArgKind, ArgSchema};

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

/// `GET /`：装配一份命令表单。这里只做「取 schema → 拼组件 → 填模板 → 挂响应头」，
/// 具体每个控件长什么样一律在 `widget_*` 里
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

    let cmd_nav = state
        .registry
        .as_ref()
        .map(|reg| command_nav(reg, &schema.name))
        .unwrap_or_default();
    let fields_html: String = schema.args.iter().map(render_field).collect();
    let field_meta: Vec<serde_json::Value> = schema
        .args
        .iter()
        .map(|arg| {
            serde_json::json!({
                "name": arg.name,
                "kind": kind_name(&arg.kind),
                "required": arg.required,
            })
        })
        .collect();

    // JSON 元数据（含 </script> 防 breakout 转义）
    let meta_json = serde_json::to_string(&field_meta)
        .unwrap_or_else(|_| "[]".into())
        .replace("</", "<\\/");

    // 只有注册表模式的执行路径会登记取消句柄（见 run::run_progress）；自定义 RunnerFn
    // 没有句柄可查，/cancel 只能回 404 —— 那就别画那颗按钮。
    // 静态标志赶在动态内容之前填：参数说明里真写出这串占位符也不会被误换。
    let cancellable = if state.registry.is_some() {
        "true"
    } else {
        "false"
    };
    let html = HTML_TEMPLATE
        .replace("__CSS__", include_str!("../assets/app.css"))
        .replace("__CANCEL_JS__", cancellable)
        .replace("__CMD_NAV__", &cmd_nav)
        .replace("__FIELDS__", &fields_html)
        .replace("__ABOUT__", &html_escape(&schema.about))
        .replace("__CMD_NAME__", &html_escape(&schema.name))
        .replace("__CMD_JS__", &serde_json::json!(schema.name).to_string())
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
    // 整页自包含且随进程变（schema、令牌、重编译后的资产都会变），缓存下来只会看到旧控制台：
    // 实测过一次 —— 改了页面 JS 重编重跑，浏览器还在发上一版，页面上半数组件是死的。
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    resp
}

// ── 字段外壳 ───────────────────────────────────────────────
//
// 一个参数在页面上永远是「外壳 + 一个控件」。外壳管标签、必填标记与布局；
// 控件只认自己的 ArgKind。Flag 例外：它的标签就是控件那一行。

/// 渲染一个参数：外壳 + 对应 ArgKind 的控件
fn render_field(arg: &ArgSchema) -> String {
    let f = Field::new(arg);
    let widget = match &arg.kind {
        ArgKind::Flag => widget_flag(&f),
        ArgKind::Text => widget_text(&f),
        ArgKind::Number { min, max } => widget_number(&f, *min, *max),
        ArgKind::Enum { values } => widget_enum(&f, values),
        ArgKind::Path { must_exist } => widget_path(&f, *must_exist),
        ArgKind::List { item } => widget_list(&f, item),
    };
    field_shell(&f, widget)
}

/// 渲染上下文：同一个参数的名字、标签、必填标记会被多处片段反复用到，
/// 一次算好（含转义）比在每个组件里各转一遍安全 —— 漏一处就是 XSS。
struct Field<'a> {
    arg: &'a ArgSchema,
    /// 已转义的参数名，用于 id 与 data-* 插值
    esc_name: String,
    /// 已转义的标签文案（不含必填星号）
    label: String,
    req_a: &'static str,
}

impl<'a> Field<'a> {
    fn new(arg: &'a ArgSchema) -> Self {
        Self {
            arg,
            esc_name: html_escape(&arg.name),
            label: html_escape(&arg.about),
            req_a: if arg.required { " required" } else { "" },
        }
    }

    /// 字符串型参数的默认值（未给就是空串）
    fn default_text(&self) -> String {
        html_escape(
            self.arg
                .default
                .as_ref()
                .and_then(|d| d.as_str())
                .unwrap_or(""),
        )
    }

    /// 占位文案：控件里也拿 about 当提示
    fn placeholder(&self) -> String {
        html_escape(&self.arg.about)
    }

    /// 外壳用的标签：文案 + 必填星号
    fn label_with_mark(&self) -> String {
        let mark = if self.arg.required {
            r##"<span class="req-mark" aria-hidden="true">*</span>"##
        } else {
            ""
        };
        format!("{}{}", self.label, mark)
    }
}

/// 外壳：标签 + 控件。Flag 走整行布局，其余一律「标签在上、控件在下」
fn field_shell(f: &Field, widget: String) -> String {
    if matches!(f.arg.kind, ArgKind::Flag) {
        return format!(r##"<div class="field field-flag">{widget}</div>"##) + "\n";
    }
    format!(
        r##"<div class="field"><label for="field-{esc}">{label}</label>{widget}</div>"##,
        esc = f.esc_name,
        label = f.label_with_mark()
    ) + "\n"
}

// ── 六种控件 ───────────────────────────────────────────────

/// `Flag`：复选框即标签，整行可点
fn widget_flag(f: &Field) -> String {
    let checked = matches!(&f.arg.default, Some(serde_json::Value::Bool(true)))
        .then_some(" checked")
        .unwrap_or("");
    format!(
        r##"<label class="flag-row" data-component="flag"><input type="checkbox" id="field-{esc}"{checked}> <span>{label}</span></label>"##,
        esc = f.esc_name,
        label = f.label
    )
}

/// `Text`：单行输入
fn widget_text(f: &Field) -> String {
    format!(
        r##"<input type="text" id="field-{esc}" data-component="text" placeholder="{ph}"{req} value="{dv}">"##,
        esc = f.esc_name,
        ph = f.placeholder(),
        req = f.req_a,
        dv = f.default_text()
    )
}

/// `Number` 的属性片段（区间 + 允许小数）。`List{item:Number}` 的行也用它 ——
/// 「数字框长什么样」只许有一个定义处。
fn number_attrs(min: Option<f64>, max: Option<f64>) -> String {
    let min_a = min.map(|m| format!(r##" min="{m}""##)).unwrap_or_default();
    let max_a = max.map(|m| format!(r##" max="{m}""##)).unwrap_or_default();
    format!(r##"{min_a}{max_a} step="any""##)
}

/// 下拉的 `<option>` 列表；`current` 决定哪一项预选中
fn options_html(values: &[String], current: Option<&str>) -> String {
    values
        .iter()
        .map(|v| {
            let ev = html_escape(v);
            let sel = if current == Some(v.as_str()) {
                " selected"
            } else {
                ""
            };
            format!(r##"<option value="{ev}"{sel}>{ev}</option>"##)
        })
        .collect()
}

/// `Number`：带 min/max 的数字输入。区间既写进 HTML 属性（校验与原生 UI 用），
/// 也渲染成一行可见提示并用 `aria-describedby` 挂上 —— 只放属性的话，
/// 读屏用户听不到「0 到 51」这个约束。**单边区间也要说**：只有 `min=0` 的
/// 参数（很常见：数量、尺寸）此前一个字都不报，只有浏览器弹的原生提示知道。
fn widget_number(f: &Field, min: Option<f64>, max: Option<f64>) -> String {
    let dv = f
        .arg
        .default
        .as_ref()
        .and_then(|d| d.as_f64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| min.map(|m| m.to_string()).unwrap_or_default());
    let esc = &f.esc_name;
    let range = match (min, max) {
        (Some(lo), Some(hi)) => Some(format!("取值 {lo} – {hi}")),
        (Some(lo), None) => Some(format!("取值 >= {lo}")),
        (None, Some(hi)) => Some(format!("取值 <= {hi}")),
        (None, None) => None,
    };
    let (described, hint) = match range {
        Some(text) => (
            format!(r##" aria-describedby="hint-{esc}""##),
            format!(r##"<span class="field-hint" id="hint-{esc}">{text}</span>"##),
        ),
        None => (String::new(), String::new()),
    };
    format!(
        r##"<input type="number" id="field-{esc}" data-component="number" value="{dv}"{attrs}{req}{described}>{hint}"##,
        dv = html_escape(&dv),
        attrs = number_attrs(min, max),
        req = f.req_a,
        described = described,
        hint = hint
    )
}

/// `Enum`：下拉，默认值预选中
fn widget_enum(f: &Field, values: &[String]) -> String {
    format!(
        r##"<select id="field-{esc}" data-component="enum"{req}>{opts}</select>"##,
        esc = f.esc_name,
        req = f.req_a,
        opts = options_html(values, f.arg.default.as_ref().and_then(|d| d.as_str()))
    )
}

/// `Path`：手填路径 + 拖拽上传 + 本机选择器三件套。
///
/// 三条路都只往同一个 `input` 回填路径，所以命令侧永远只看 `--path`：
/// - 拖拽/点击 → `POST /upload`，拿到服务端**副本**路径（受 `data-max-upload` 限制）
/// - 「本机」→ `POST /pick`，服务端弹系统对话框，拿到**原始**路径（不复制、不限大小）
/// - 手填 → 用户自己的字符串
fn widget_path(f: &Field, must_exist: bool) -> String {
    let hint = if must_exist {
        "拖到这里，或点「选择文件」—— 上传后自动回填服务端路径"
    } else {
        "可选：拖拽上传，或直接手填路径"
    };
    format!(
        r##"<input type="text" id="field-{esc}" class="mono" data-component="path" placeholder="{ph}"{req} value="{dv}" spellcheck="false" aria-describedby="up-{esc}">\
{dz}"##,
        esc = f.esc_name,
        ph = f.placeholder(),
        req = f.req_a,
        dv = f.default_text(),
        dz = dropzone(f, hint)
    )
}

/// 拖拽区是个**容器**，不是按钮：里面有三个各自能点的控件（选文件 / 本机 / 清除），
/// 所以外层不许挂 `role="button"` 或 `tabindex` —— 那会把一次点击变成两个动作，
/// 读屏也数不清到底有几个控件。键盘可达由「选择文件」这个真按钮负责。
///
/// `data-max-upload` 由服务端注入 —— 页面据此在**读文件之前**拦下超大文件，
/// 上限因此只有一个来源（`crate::files::MAX_UPLOAD_BYTES`）。
/// 状态行 `role="status"` 并挂在路径框的 `aria-describedby` 上，上传结果不必聚焦也能读到。
///
/// 这里**不再**重复参数名（`data-browse`）或 `must_exist`：前者 JS 从 `.dropzone[data-target]`
/// 就拿得到，后者已经变成下面那句提示文案 —— DOM 上不留没人读的副本。
fn dropzone(f: &Field, hint: &str) -> String {
    let esc = &f.esc_name;
    format!(
        r##"<div class="dropzone" data-component="dropzone" data-target="{esc}" data-max-upload="{MAX_UPLOAD_BYTES}">\
<input type="file" class="visually-hidden" tabindex="-1" aria-hidden="true">\
<span class="dz-icon" aria-hidden="true">⇪</span><span class="dz-hint">{hint}</span>\
<span class="dz-status" id="up-{esc}" role="status" aria-live="polite"></span>\
<button type="button" class="btn-icon dz-browse" data-component="browse">选择文件</button>{pick}\
<button type="button" class="file-chip" id="chip-{esc}" hidden></button></div>"##,
        pick = pick_button(f)
    )
}

/// 「本机」按钮：只在开了 `pick` 特性的构建里画，否则点了只能得到一句报错
fn pick_button(f: &Field) -> String {
    if !cfg!(feature = "pick") {
        return String::new();
    }
    format!(
        r##"<button type="button" class="btn-icon dz-pick" data-component="pick" data-pick="{esc}" aria-label="用系统选择器挑本机文件" title="用系统选择器挑本机文件（只回填真实路径，不上传副本）">本机</button>"##,
        esc = f.esc_name
    )
}

/// `List`：动态行（默认 2 行）。增删都由 JS 的 `initList` 负责 ——
/// 组件不在 HTML 里塞内联 `onclick`，行为归行为层。
///
/// **行里的控件由 item 类型决定**：以前一律画成文本框，于是 `Vec<u32>` 这种参数
/// 从 Web 提交的是字符串数组，`core::validate_kind` 的 `Number` 分支要的是
/// `as_f64`，直接回一句「需要数字」—— CLI / TUI / MCP 都能跑，只有 Web 不行。
/// 容器上的 `data-item` 给 JS 收值时用（同一份 `kind_name`，不另立映射表），
/// 每行的 `data-item-kind` 给 `resetItem` 用 —— 复制行、清行都只认服务端标的类型。
fn widget_list(f: &Field, item: &ArgKind) -> String {
    let esc = &f.esc_name;
    format!(
        r##"<div class="list-rows" id="list-{esc}" data-component="list" data-list="{esc}" data-item="{kind}">{rows}</div>\
<button type="button" class="btn-icon list-add" data-component="list-add" data-list-add="{esc}">＋ 添加一项</button>"##,
        esc = esc,
        kind = kind_name(item),
        rows = list_row(f, item) + &list_row(f, item)
    )
}

/// 一行列表项：按 item 类型挑控件。
/// `Path` 只给一个等宽文本框（每行再挂一套拖拽区会把表单撑爆），
/// `List` 嵌套不支持 —— 退化成文本，交给服务端的校验说话。
fn list_row(f: &Field, item: &ArgKind) -> String {
    let ph = f.placeholder();
    let control = match item {
        ArgKind::Number { min, max } => format!(
            r##"<input type="number" class="mono" data-list-item="{esc}" data-item-kind="Number" placeholder="{ph}" aria-label="{ph}"{attrs}>"##,
            esc = f.esc_name,
            ph = ph,
            attrs = number_attrs(*min, *max)
        ),
        ArgKind::Enum { values } => format!(
            r##"<select data-list-item="{esc}" data-item-kind="Enum" aria-label="{ph}">{opts}</select>"##,
            esc = f.esc_name,
            ph = ph,
            opts = options_html(values, None)
        ),
        ArgKind::Flag => format!(
            r##"<label class="flag-row"><input type="checkbox" data-list-item="{esc}" data-item-kind="Flag"><span>{ph}</span></label>"##,
            esc = f.esc_name,
            ph = ph
        ),
        ArgKind::Path { .. } => format!(
            r##"<input type="text" class="mono" data-list-item="{}" data-item-kind="Path" placeholder="{ph}" aria-label="{ph}" spellcheck="false">"##,
            f.esc_name
        ),
        _ => format!(
            r##"<input type="text" class="mono" data-list-item="{esc}" data-item-kind="Text" placeholder="{ph}" aria-label="{ph}">"##,
            esc = f.esc_name,
            ph = ph
        ),
    };
    format!(
        r##"<div class="list-row">{control}<button type="button" class="btn-icon row-del" aria-label="删除该行">✕</button></div>"##
    )
}

/// 命令切换下拉：只在多命令且可见命令 > 1 时出现。
/// 换了命令要整页重载（表单是服务端按 schema 摊出来的），跳转逻辑在 `initCommandNav`
fn command_nav(registry: &Registry, current: &str) -> String {
    let visible: Vec<&RegisteredCommand> = registry.visible().collect();
    if visible.len() < 2 {
        return String::new();
    }
    let opts: String = visible
        .iter()
        .map(|c| {
            let name = html_escape(&c.schema.name);
            let sel = if c.schema.name == current {
                " selected"
            } else {
                ""
            };
            format!(r##"<option value="{name}"{sel}>{name}</option>"##)
        })
        .collect();
    format!(
        r##"<select id="cmd-nav" data-component="command-nav" aria-label="切换命令">{opts}</select>"##,
        opts = opts
    )
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

    /// 渲染一份 schema，返回页面 HTML
    async fn page(args: Vec<ArgSchema>) -> String {
        let state = state_with(schema_of("demo", "demo", args));
        body_of(index(State(state), Query(HashMap::new())).await).await
    }

    /// 取某个属性的值（测试自用）
    fn attr(html: &str, name: &str) -> String {
        let key = format!(r##"{name}=""##);
        let rest = html
            .split(&key)
            .nth(1)
            .unwrap_or_else(|| panic!("页面里没有属性 {name}"));
        rest.split('"').next().unwrap().to_string()
    }

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
        let body = page(vec![
            arg(
                "file",
                "文件",
                ArgKind::Path { must_exist: false },
                true,
                None,
            ),
            arg(
                "quality",
                "质量",
                ArgKind::Number {
                    min: Some(0.0),
                    max: Some(51.0),
                },
                false,
                Some(serde_json::json!(23)),
            ),
        ])
        .await;
        assert!(body.contains("dropzone"), "Path 参数要有拖拽区");
        if cfg!(feature = "pick") {
            assert!(
                body.contains(r##"data-pick="file""##) && body.contains(r##"fetch("/pick""##),
                "原生选择器按钮或它的请求没了"
            );
        }
        assert!(
            !body.contains(r##"data-pick="quality""##),
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
        let body = body_of(index(State(state), Query(HashMap::new())).await).await;
        assert!(!body.contains("<script>alert"), "about must be escaped");
        assert!(!body.contains("<img src=x>"), "about must be escaped");
        assert!(!body.contains("\"><svg>"), "default must be escaped");
    }

    /// 页面不许被缓存：整页自包含，且内容随进程变（schema、令牌、重编译后的资产）。
    /// 踩过一次 —— 改了页面 JS 重编重跑，浏览器还在发上一版，半数组件是死的，
    /// 看上去像新代码有 bug。
    #[tokio::test]
    async fn index_forbids_caching() {
        let state = state_with(schema_of("demo", "demo", vec![]));
        let resp = index(State(state), Query(HashMap::new())).await;
        assert_eq!(
            resp.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "没有 no-store，重编译后浏览器还在发旧控制台"
        );
        assert!(
            resp.headers().contains_key(header::CONTENT_SECURITY_POLICY),
            "CSP 头不能丢"
        );
    }

    #[tokio::test]
    async fn index_renders_dropzone_for_path_args() {
        let body = page(vec![arg(
            "file",
            "文件",
            ArgKind::Path { must_exist: true },
            true,
            None,
        )])
        .await;
        assert!(body.contains("dropzone"), "Path 参数必须有拖拽上传组件");
        // must_exist 落在**话术**上，不再另存一份 data-must-exist 让 JS 之外的人猜
        assert!(
            body.contains("拖到这里，或点「选择文件」"),
            "must_exist 的 Path 要告诉用户怎么给这个文件"
        );
        assert!(body.contains(r##"type="file""##), "必须有文件选择入口");
        // 上限只有一个来源：服务端常量注入 DOM，页面读它来拦超大文件。
        // 谁把 200 抄进 JS 或文档，这条就会红。
        assert!(
            body.contains(&format!(r##"data-max-upload="{MAX_UPLOAD_BYTES}""##)),
            "拖拽区没带上服务端注入的体积上限"
        );

        // 另一边也要说得过去：`must_exist = false` 是可以手填的目标路径，
        // 不能拿「拖进来就回填服务端副本」那句话去骗人（真页面上 lbin/lfiles 全是 true，
        // 所以这一半只能由测试覆盖）
        let out = page(vec![arg(
            "dest",
            "输出",
            ArgKind::Path { must_exist: false },
            false,
            None,
        )])
        .await;
        assert!(
            out.contains("可选：拖拽上传，或直接手填路径"),
            "must_exist=false 的 Path 话术要允许手填"
        );
        assert!(
            !out.contains("拖到这里，或点「选择文件」"),
            "两种 must_exist 的话术串了"
        );
    }

    /// 页面 JS 拿 `$("field-" + dz.dataset.target)` 找输入框，所以 `data-target` 必须是
    /// **裸参数名**。这里曾经发的是 `field-path`，JS 拼成 `field-field-path` 取到 null，
    /// 上传明明成功了，页面却报「上传失败：Cannot set properties of null」——
    /// 只有真在浏览器里拖一个文件进去才看得见。
    #[tokio::test]
    async fn dropzone_ids_match_what_the_page_looks_up() {
        let body = page(vec![arg(
            "file",
            "文件",
            ArgKind::Path { must_exist: true },
            true,
            None,
        )])
        .await;
        let target = attr(&body, "data-target");
        assert_eq!(target, "file", "data-target 要的是裸参数名，不是 id");
        assert!(
            body.contains(&format!(r##"id="field-{target}""##)),
            "输入框 id 对不上 JS 的拼法"
        );
        assert!(
            body.contains(&format!(r##"id="up-{target}""##)),
            "状态行 id 对不上 /pick 的拼法"
        );
        assert!(
            !cfg!(feature = "pick") || body.contains(&format!(r##"data-pick="{target}""##)),
            "本机按钮的 data-pick 与 data-target 得用同一套名字"
        );
    }

    /// 页面内联 JS 的正文（骨架里的 `<script>` 块，占位符已填）
    fn inline_script() -> String {
        HTML_TEMPLATE
            .split("<script>")
            .nth(1)
            .expect("页面里没有 <script> 块")
            .split("</script>")
            .next()
            .unwrap()
            .to_string()
    }

    /// HTML 里出现过的所有 `data-*` 属性名（去重、按出现顺序）
    fn data_names(html: &str) -> Vec<String> {
        let b = html.as_bytes();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i + 5 <= b.len() {
            if &b[i..i + 5] == b"data-" {
                let mut j = i + 5;
                while j < b.len()
                    && (b[j].is_ascii_lowercase() || b[j] == b'-' || b[j].is_ascii_digit())
                {
                    j += 1;
                }
                let name = String::from_utf8_lossy(&b[i..j]).to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }

    /// `data-item-kind` → `itemKind`：JS 摸 dataset 时用的是驼峰，不是横线名
    fn to_camel(attr: &str) -> String {
        attr.trim_start_matches("data-")
            .split('-')
            .enumerate()
            .map(|(n, part)| {
                if n == 0 {
                    part.to_string()
                } else {
                    let mut c = part.chars();
                    c.next()
                        .map(|x| x.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                }
            })
            .collect()
    }

    /// DOM 上不留没人读的副本。每个 `data-*` 要么内联 JS 提它（横线名或 dataset 驼峰名），
    /// 要么 CSS 拿它当选择器钩子，要么在下面白名单里说清用处。
    ///
    /// 起因（2026-09-23 复核）：`data-browse` / `data-must-exist` / `data-placeholder`
    /// 画在页面上半年，一个人都没读过，而 DESIGN.md 写着「JS 靠它们决定行为」——
    /// 属性与文档各说各话，读代码的人只能猜。
    #[tokio::test]
    async fn no_data_attribute_goes_unread() {
        let body = page(vec![
            arg(
                "num",
                "数量",
                ArgKind::Number {
                    min: Some(0.0),
                    max: None,
                },
                false,
                None,
            ),
            arg(
                "path",
                "路径",
                ArgKind::Path { must_exist: true },
                true,
                None,
            ),
            arg(
                "nums",
                "尺寸",
                ArgKind::List {
                    item: Box::new(ArgKind::Number {
                        min: Some(1.0),
                        max: Some(9.0),
                    }),
                },
                false,
                None,
            ),
        ])
        .await;
        let js = inline_script();
        let css = include_str!("../assets/app.css");
        let read_by_someone = |name: &str| {
            name == "data-component" // markup 自报家门：测试与调试的锚点，不声称 JS 读它
                || js.contains(name)
                || js.contains(&to_camel(name))
                || css.contains(name)
        };
        let offenders: Vec<String> = data_names(&body)
            .into_iter()
            .filter(|n| !read_by_someone(n))
            .collect();
        assert!(
            offenders.is_empty(),
            "这些 data-* 画了没人读：{offenders:?}"
        );
    }

    /// 单边区间也要说出来。以前只有 min 和 max 都在才画提示，
    /// 于是「取值 >= 0」这种最常见的约束一个字都不报，读屏用户听到的只是一个数字框。
    #[tokio::test]
    async fn number_bounds_are_spoken_whichever_side_they_come_from() {
        let lo = page(vec![arg(
            "n",
            "数量",
            ArgKind::Number {
                min: Some(0.0),
                max: None,
            },
            false,
            None,
        )])
        .await;
        assert!(
            lo.contains(r##"min="0""##) && lo.contains("取值 >= 0"),
            "只有 min 时也要念出来"
        );
        let hi = page(vec![arg(
            "n",
            "上限",
            ArgKind::Number {
                min: None,
                max: Some(9.0),
            },
            false,
            None,
        )])
        .await;
        assert!(
            hi.contains(r##"max="9""##) && hi.contains("取值 <= 9"),
            "只有 max 时也要念出来"
        );
        assert!(
            hi.contains(r##"aria-describedby="hint-n""##),
            "提示得挂在框上"
        );
        // 什么区间都没有就别硬凑一行提示
        let none = page(vec![arg(
            "n",
            "任意数",
            ArgKind::Number {
                min: None,
                max: None,
            },
            false,
            None,
        )])
        .await;
        // 比的是画出来的片段，不是 CSS 里那条 `.field-hint` 规则（整页永远含着它）
        assert!(
            !none.contains(r##"<span class="field-hint""##) && !none.contains(r##"id="hint-n""##),
            "无区间却画了一行空提示"
        );
    }

    /// 「取消」按钮不能是个假承诺：只有 `Registry` 那条执行路会登记取消句柄，
    /// 自定义 RunnerFn（`serve`）拿不到句柄，页面就得连按钮都不画。
    /// `serve_app` 因此内部就是单命令注册表（见 lib.rs）。
    #[tokio::test]
    async fn the_cancel_button_only_promises_what_the_server_can_do() {
        let runner_mode = state_with(schema_of("demo", "demo", vec![]));
        let body = body_of(index(State(runner_mode), Query(HashMap::new())).await).await;
        assert!(
            body.contains("const CANCELABLE=false;"),
            "单命令 RunnerFn 没有取消句柄，页面却还是画了「取消」"
        );
        let reg = registry_state();
        let body = body_of(index(State(reg), Query(HashMap::new())).await).await;
        assert!(
            body.contains("const CANCELABLE=true;"),
            "注册表模式可以取消，页面却把按钮藏了"
        );
    }

    /// 六种 ArgKind 各自必须出现，并带上组件自报家门的 `data-component`。
    /// 这条是「组件目录」（DESIGN.md §10）与 DOM 之间的对账：少一个组件、
    /// 或组件忘了自报，红的就是这里。
    #[tokio::test]
    async fn every_arg_kind_renders_its_own_component() {
        let body = page(vec![
            arg(
                "flag",
                "开关",
                ArgKind::Flag,
                false,
                Some(serde_json::json!(true)),
            ),
            arg(
                "text",
                "文本",
                ArgKind::Text,
                false,
                Some(serde_json::json!("dv")),
            ),
            arg(
                "num",
                "数字",
                ArgKind::Number {
                    min: Some(0.0),
                    max: Some(51.0),
                },
                true,
                Some(serde_json::json!(23)),
            ),
            arg(
                "mode",
                "模式",
                ArgKind::Enum {
                    values: vec!["fast".into(), "best".into()],
                },
                false,
                Some(serde_json::json!("best")),
            ),
            arg(
                "path",
                "路径",
                ArgKind::Path { must_exist: true },
                true,
                None,
            ),
            arg(
                "tags",
                "标签",
                ArgKind::List {
                    item: Box::new(ArgKind::Text),
                },
                false,
                None,
            ),
        ])
        .await;

        for kind in ["flag", "text", "number", "enum", "path", "list"] {
            assert!(
                body.contains(&format!(r##"data-component="{kind}""##)),
                "缺 {kind} 组件"
            );
        }
        assert!(
            body.contains(r##"<input type="checkbox" id="field-flag" checked"##),
            "Flag 的默认值要真的勾上"
        );
        assert!(body.contains(r##"value="dv""##), "Text 默认值");
        assert!(
            body.contains(r##"min="0" max="51""##) && body.contains(r##"id="hint-num""##),
            "Number 的区间既进属性也要能被读屏念到"
        );
        assert!(
            body.contains(r##"<option value="best" selected>best</option>"##),
            "Enum 默认值要预选中"
        );
        assert_eq!(
            body.matches(r##"class="list-row""##).count(),
            2,
            "List 默认两行"
        );
    }

    /// 行为全归 JS 层：服务端吐出来的 HTML 里一个内联事件都不许有。
    /// 内联事件是「 markup 与行为各说各话」的头号来源 —— 改了 `init*` 忘了改属性，
    /// 页面不会报错，只会点了没反应。
    #[tokio::test]
    async fn no_inline_event_handlers_anywhere() {
        let body = page(vec![
            arg(
                "path",
                "路径",
                ArgKind::Path { must_exist: false },
                false,
                None,
            ),
            arg(
                "tags",
                "标签",
                ArgKind::List {
                    item: Box::new(ArgKind::Text),
                },
                false,
                None,
            ),
        ])
        .await;
        for hook in ["onclick=", "onchange=", "oninput=", "onsubmit=", "onload="] {
            assert!(
                !body.contains(hook),
                "页面里出现了内联事件 {hook}，行为该写在 init* 里"
            );
        }
    }

    /// 拖拽区里住着三个各自能点的控件（选文件 / 本机 / 清除）。它们确实在拖拽区内，
    /// 所以外层**不许**是 role=button —— 否则点「本机」会同时弹出浏览器的文件选择框，
    /// 而点击分流只能由 initDropzone 负责，不许退回内联事件。
    #[tokio::test]
    async fn interactive_children_live_inside_the_dropzone() {
        let body = page(vec![arg(
            "file",
            "文件",
            ArgKind::Path { must_exist: true },
            true,
            None,
        )])
        .await;
        let dz = dropzone_markup(&body);
        assert!(
            !dz.contains(r##"role="button""##) && !dz.contains("tabindex=\"0\""),
            "拖拽区是容器，不能自己冒充按钮"
        );
        assert!(
            dz.contains(r##"class="btn-icon dz-browse""##),
            "键盘用户要有一个真的「选择文件」按钮可用"
        );
        if cfg!(feature = "pick") {
            assert!(dz.contains("data-pick="), "本机按钮在拖拽区内");
        }
        assert!(
            dz.contains(r##"<button type="button" class="file-chip""##),
            "chip 得是 button：可聚焦、可回车清除"
        );
        assert!(
            !dz.contains("onclick=") && !dz.contains("onchange="),
            "拖拽区内部不许有内联事件，点击分流归 initDropzone"
        );
    }

    /// JS 里定义的每个 `init*` 都必须进 BOOT：加了组件忘了装配，页面不报错，
    /// 只是那个组件永远不动。BOOT 里也不能有不存在的函数，那会让整个 boot 抛异常。
    #[test]
    fn every_js_component_init_is_bootted() {
        let page = HTML_TEMPLATE;
        let defined = js_init_fns(page);
        assert!(
            defined.len() >= 9,
            "组件数量掉到 {defined:?}，装配清单被动过了？"
        );
        let boot = page
            .split("const BOOT=[")
            .nth(1)
            .expect("BOOT 清单没了")
            .split(']')
            .next()
            .unwrap();
        for f in &defined {
            assert!(
                boot.split(['[', ']', ',', '\n'])
                    .any(|x| x.trim() == f.as_str()),
                "{f} 定义了却没进 BOOT"
            );
        }
        for entry in boot.split(',').map(str::trim).filter(|x| !x.is_empty()) {
            assert!(
                defined.iter().any(|f| f == entry),
                "BOOT 装配了不存在的 {entry}，boot 会直接抛异常"
            );
        }
    }

    /// 内联 JS 没有编译期检查：一个括号打错，整台控制台静默死掉 —— Rust 侧全是绿的，
    /// 页面却一个按钮都不响应。本机有 node 就把 `<script>` 块丢给它 parse 一遍。
    /// 没有 node 只跳过这一条（不误伤离线开发），CI 两台 runner 都带 node 所以闸门有效。
    #[test]
    fn inline_script_is_syntactically_valid() {
        let js = HTML_TEMPLATE
            .split("<script>")
            .nth(1)
            .expect("页面里没有 <script> 块")
            .split("</script>")
            .next()
            .unwrap()
            .replace("__CMD_JS__", "\"demo\"")
            .replace("__CANCEL_JS__", "true")
            .replace("__META__", "[]");
        let mut path = std::env::temp_dir();
        path.push(format!("lilyco-gui-inline-{}.js", std::process::id()));
        std::fs::write(&path, &js).expect("临时 JS 写不出去");
        let check = std::process::Command::new("node")
            .arg("--check")
            .arg(&path)
            .output();
        let _ = std::fs::remove_file(&path);
        let Ok(out) = check else {
            eprintln!("跳过：本机没有 node，内联 JS 语法无人守");
            return;
        };
        assert!(
            out.status.success(),
            "内联 JS 语法不过：{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 装配循环必须是脚本里最后一段：跑在它后面的顶层 `const`/`let` 还没初始化，
    /// 组件一读就撞 TDZ。真实事故：`quoteIfSpaced` 声明在 BOOT 之后，
    /// `initCliPreview` 抛 ReferenceError → 拖拽区、List、运行、结果全没装配，
    /// 页面看着完全正常，点了不动，控制台里连个错误都找不到。
    #[test]
    fn boot_is_the_last_thing_in_the_script() {
        let js = HTML_TEMPLATE
            .split("<script>")
            .nth(1)
            .expect("页面里没有 <script> 块")
            .split("</script>")
            .next()
            .unwrap();
        let boot_at = js.rfind("for(const init of BOOT)").expect("装配循环没了");
        let offenders: Vec<&str> = js[boot_at..]
            .lines()
            .filter(|l| l.starts_with("const ") || l.starts_with("let "))
            .collect();
        assert!(
            offenders.is_empty(),
            "顶层声明跑到了装配循环之后，装配期读它会撞 TDZ：{offenders:?}"
        );
    }

    /// chip 是按钮，可访问名必须带上「清除」这个动作 —— 只念得出文件名和大小时，
    /// 读屏用户听到的是一个看不出作用的按钮。超限话术必须报字节：只报 MB 的话，
    /// 209,715,201 B 与 209,715,200 B 都显示成「200.0 MB」，用户读到的是
    /// 「200 MB 超过 200 MB」这种自相矛盾的句子。两条都是 2026-09-23 真在页面上读出来的。
    #[test]
    fn the_chip_and_the_oversize_message_say_what_they_mean() {
        let page = HTML_TEMPLATE;
        assert!(
            page.contains(r##"setAttribute("aria-label","清除已上传的 "##),
            "chip 的可访问名不再说明它清除的是什么了"
        );
        assert!(
            page.contains("bytes(file.size)") && page.contains("bytes(max)"),
            "超限话术退回只报 MB，会念出「200 MB 超过 200 MB」"
        );
    }

    /// `prefers-reduced-motion` 把动画全停之后，「进度不确定」与「按钮在忙」这两件事
    /// 必须有静态替代 —— 关掉动画不等于关掉状态（40% 的条会看着像卡住，转圈的按钮
    /// 会看着像空的）。这台浏览器工具没法模拟那条媒体查询，所以至少把规则钉在测试里：
    /// 谁删了静态替代，这里红。
    #[test]
    fn reduced_motion_keeps_a_non_animated_state_signal() {
        let css = include_str!("../assets/app.css");
        let block = css
            .split("@media (prefers-reduced-motion: reduce)")
            .nth(1)
            .expect("reduced-motion 那段没了");
        for need in [
            "width: 100%", // 不确定态：铺满 + 压淡
            "opacity: 0.5",
            "visibility: visible", // loading：标签留着
            "display: none",       // 静止的转圈别再占位
        ] {
            assert!(
                block.contains(need),
                "reduced-motion 里少了静态替代：{need}"
            );
        }
    }

    /// 体积闸门（DESIGN.md §9）：这页要 `include_str!` 进每个域二进制的 `.exe`，
    /// 上限只写在这一个地方 —— 表格里再抄一份就是第二张需要人记着的平行表。
    /// 涨过线要么删点什么，要么改这里并说清换来什么。
    ///
    /// 量的是**提交进仓库的字节**：Windows runner 按 `core.autocrlf` 检出时会给每行补一个
    /// `\r`，同一个文件就地胖 441 B —— 2026-09-23 就是这样在 CI 上红的（本地 20,751 B、
    /// CI 21,192 B）。预算要管的是资源本身，不是某台机器的检出方式。
    #[test]
    fn assets_stay_within_the_documented_budget() {
        const HTML_BUDGET: usize = 21_000;
        const CSS_BUDGET: usize = 22_500;
        let html = HTML_TEMPLATE.replace("\r\n", "\n").len();
        let css = include_str!("../assets/app.css")
            .replace("\r\n", "\n")
            .len();
        assert!(
            html <= HTML_BUDGET,
            "index.html 已经 {html} B，超过 {HTML_BUDGET} B 上限（DESIGN.md §9）"
        );
        assert!(
            css <= CSS_BUDGET,
            "app.css 已经 {css} B，超过 {CSS_BUDGET} B 上限（DESIGN.md §9）"
        );
    }

    /// 「设计系统即数字 / 只此一张表」的机器版：`app.css` 里出现的每一个颜色字面量，
    /// 都必须能在 `DESIGN.md` 里找到 —— 样式表里悄悄多一个色，就是多了一张没人记的表。
    /// 终端区那一串（§1.3）也在文档里逐个列全，所以这里不需要例外名单。
    #[test]
    fn every_css_color_is_in_the_design_table() {
        let css = include_str!("../assets/app.css");
        let doc = include_str!("../DESIGN.md").to_ascii_lowercase();
        let b = css.as_bytes();
        let mut offenders: Vec<String> = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'#' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            // 只认 3/4/6/8 位那一族：`#result` 这类选择器的下一个字母不是十六进制位
            if matches!(j - i - 1, 3 | 4 | 6 | 8) {
                let hex = String::from_utf8_lossy(&b[i + 1..j]).to_ascii_lowercase();
                if !doc.contains(&format!("#{hex}")) {
                    offenders.push(format!("#{hex}"));
                }
            }
            i = j;
        }
        assert!(
            offenders.is_empty(),
            "app.css 里这些颜色没写进 DESIGN.md：{offenders:?}（§1 是唯一一张表）"
        );
    }

    /// `List` 的行控件由 item 类型决定。以前不管 item 是什么都画文本框，于是 `Vec<u32>`
    /// 这种参数从 Web 交上去的是字符串数组，而 schema 声明的是 `Number`
    /// （`validate_kind` 只认 `as_f64`）→ 一句「需要数字」。
    /// 注：`Vec<u32>` 目前连编译都过不去（derive 取值一律 `as_str().collect()`，
    /// 见 DESIGN.md §10.1 那段注记），所以这条是按 **schema 承诺**画的，
    /// 等 `lilyco-macros` 修好取值分支就直接对上。
    #[tokio::test]
    async fn list_rows_follow_their_item_kind() {
        let body = page(vec![
            arg(
                "nums",
                "数字清单",
                ArgKind::List {
                    item: Box::new(ArgKind::Number {
                        min: Some(1.0),
                        max: Some(9.0),
                    }),
                },
                true,
                None,
            ),
            arg(
                "modes",
                "模式清单",
                ArgKind::List {
                    item: Box::new(ArgKind::Enum {
                        values: vec!["a".into(), "b".into()],
                    }),
                },
                false,
                None,
            ),
        ])
        .await;
        assert!(
            body.contains(r##"data-item="Number""##) && body.contains(r##"data-item="Enum""##),
            "容器得把 item 类型带给 JS，复制行时才知道画什么控件"
        );
        assert_eq!(
            body.matches(r##"data-item-kind="Number""##).count(),
            2,
            "数字项的两行都是 number 框（数属性而不是 type=\"number\"，那串在 CSS 里也有）"
        );
        assert!(
            body.contains(r##"<input type="number" class="mono" data-list-item="nums""##),
            "行里确实是 number 输入，不是文本框"
        );
        assert!(
            body.contains(r##"min="1" max="9""##),
            "行内数字框带着区间（与独立 Number 同一个定义）"
        );
        assert!(
            body.contains(r##"<select data-list-item="modes""##)
                && body.contains(r##"<option value="a">a</option>"##),
            "Enum 项要画下拉，不是文本框"
        );
    }

    /// 数一遍 JS 里 `function initXxx(` 定义出来的组件
    fn js_init_fns(src: &str) -> Vec<String> {
        const MARK: &str = "function init";
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find(MARK) {
            rest = &rest[i + MARK.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(format!("init{name}"));
            }
        }
        out
    }

    /// 取第一个拖拽区的完整标记
    fn dropzone_markup(body: &str) -> String {
        body.split(r##"<div class="dropzone""##)
            .nth(1)
            .expect("页面里没有拖拽区")
            .split("</div>")
            .next()
            .unwrap()
            .to_string()
    }
}
