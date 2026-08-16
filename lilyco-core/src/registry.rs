//! 命令注册表（借鉴 unilang 的 CommandRegistry 设计）
//!
//! 高内聚：注册 / 别名 / 隐藏 / 声明式加载全部收敛在此模块。
//! 低耦合：只依赖 `CommandSchema` / `Context` / `AppError`，不依赖任何渲染端。
//!
//! 设计要点（对应 unilang 的借鉴点）：
//! - 别名：运行时别名解析到规范名（unilang FR-REG-5）
//! - 隐藏命令：不出现在 help / MCP tools/list（unilang `hidden_from_list`）
//! - 声明式加载：运行期从 JSON 装配命令定义（unilang FR-REG-3）
//! - 校验：非空名 / 重名拒绝（unilang FR-REG-9 的"无静默失败"原则）

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::context::Context;
use crate::error::AppError;
use crate::schema::CommandSchema;

/// 命令处理器：接收参数 JSON，通过 ctx 上报进度，返回结果 JSON
pub type Handler =
    Arc<dyn Fn(&Context, &serde_json::Value) -> Result<serde_json::Value, AppError> + Send + Sync>;

/// 注册表中的一条命令（schema + 可选处理器 + 元数据）
///
/// `Debug` 手写：`dyn Fn` 不实现 `std::fmt::Debug`，handler 只显示占位符。
#[derive(Clone, Serialize, Deserialize)]
pub struct RegisteredCommand {
    /// 命令名（kebab-case），如 `img-compress`
    pub name: String,
    /// 别名：运行时解析到规范名
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 隐藏命令不出现在 help / tools/list 中
    #[serde(default)]
    pub hidden: bool,
    /// 完整 schema（CLI / TUI / GUI / MCP 都从这里渲染）
    pub schema: CommandSchema,
    /// 执行处理器；声明式加载（JSON）时不携带
    #[serde(skip)]
    pub handler: Option<Handler>,
}

impl std::fmt::Debug for RegisteredCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredCommand")
            .field("name", &self.name)
            .field("aliases", &self.aliases)
            .field("hidden", &self.hidden)
            .field("schema", &self.schema)
            .field("handler", &self.handler.as_ref().map(|_| "<handler>"))
            .finish()
    }
}

impl RegisteredCommand {
    /// 新建一个仅携带 schema 的命令（可后续 attach handler）
    pub fn new(name: impl Into<String>, schema: CommandSchema) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            hidden: false,
            schema,
            handler: None,
        }
    }

    /// 附加执行处理器
    pub fn with_handler(mut self, handler: Handler) -> Self {
        self.handler = Some(handler);
        self
    }

    /// 追加别名
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// 标记隐藏
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// 把实现了 `App` 的类型适配成可注册命令（零样板入口）
    ///
    /// 静态注册侧：`#[derive(App)]` 生成 schema / from_args / run，
    /// 这里把三者装进统一的 `Handler`，从此所有后端共用同一执行路径。
    pub fn from_app<A: App + Send + 'static>() -> Self {
        let schema = A::schema();
        let name = schema.name.clone();
        let handler: Handler = Arc::new(|ctx, args| {
            let obj = args
                .as_object()
                .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;
            let map: HashMap<String, serde_json::Value> =
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let app = A::from_args(&map)?;
            app.run(ctx)
        });
        Self {
            name,
            aliases: Vec::new(),
            hidden: false,
            schema,
            handler: Some(handler),
        }
    }
}

/// 注册表错误
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("command `{0}` is already registered")]
    Duplicate(String),
    #[error("command name must not be empty")]
    EmptyName,
    #[error("command `{0}` not found")]
    NotFound(String),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("command `{0}` has no handler")]
    NoHandler(String),
}

/// 命令注册表
///
/// 借鉴 unilang 的两层注册思想（静态 derive + 动态注册）：
/// - 静态侧：`#[derive(App)]` + [`RegisteredCommand::from_app`] 编译期装配
/// - 动态侧：运行期 [`Registry::register`] / [`Registry::register_from_json`]
///   （插件系统、AI 运行期注册新命令、REPL 均属此路径）
#[derive(Debug, Default)]
pub struct Registry {
    commands: HashMap<String, RegisteredCommand>,
    aliases: HashMap<String, String>,
}

impl Registry {
    /// 新建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一条命令。校验：非空名、无重名；别名索引同步建立。
    pub fn register(&mut self, cmd: RegisteredCommand) -> Result<(), RegistryError> {
        if cmd.name.is_empty() {
            return Err(RegistryError::EmptyName);
        }
        if self.commands.contains_key(&cmd.name) {
            return Err(RegistryError::Duplicate(cmd.name.clone()));
        }
        for alias in &cmd.aliases {
            self.aliases.insert(alias.clone(), cmd.name.clone());
        }
        self.commands.insert(cmd.name.clone(), cmd);
        Ok(())
    }

    /// 按名字或别名查找（别名解析到规范名后返回规范命令）
    pub fn get(&self, name: &str) -> Option<&RegisteredCommand> {
        self.commands
            .get(name)
            .or_else(|| self.aliases.get(name).and_then(|c| self.commands.get(c)))
    }

    /// 是否包含该名字或别名
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// 全部命令（含隐藏）
    pub fn iter(&self) -> impl Iterator<Item = &RegisteredCommand> {
        self.commands.values()
    }

    /// 对外可见的命令（排除 hidden）
    pub fn visible(&self) -> impl Iterator<Item = &RegisteredCommand> {
        self.commands.values().filter(|c| !c.hidden)
    }

    /// 规范名列表
    pub fn names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }

    /// 声明式加载（借鉴 unilang FR-REG-3）：运行期从 JSON 装配命令，无 handler
    ///
    /// ```json
    /// [{ "name": "ping", "schema": { "name": "ping", "about": "...", "args": [] } }]
    /// ```
    pub fn register_from_json(&mut self, json: &str) -> Result<(), RegistryError> {
        let cmds: Vec<RegisteredCommand> = serde_json::from_str(json)?;
        for cmd in cmds {
            self.register(cmd)?;
        }
        Ok(())
    }

    /// 导出为 JSON（跨进程分发 schema 清单 / 调试）
    pub fn to_json(&self) -> serde_json::Value {
        let cmds: Vec<&RegisteredCommand> = self.commands.values().collect();
        serde_json::to_value(cmds).unwrap_or(serde_json::Value::Null)
    }
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ArgKind, ArgSchema};

    fn simple_schema(name: &str) -> CommandSchema {
        CommandSchema {
            name: name.into(),
            about: "test command".into(),
            args: vec![ArgSchema {
                name: "x".into(),
                about: "arg x".into(),
                kind: ArgKind::Text,
                required: false,
                default: None,
            }],
            subcommands: vec![],
        }
    }

    #[test]
    fn register_and_get_by_name() {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new("ping", simple_schema("ping")))
            .unwrap();
        assert!(reg.contains("ping"));
        assert_eq!(reg.get("ping").unwrap().name, "ping");
        assert_eq!(reg.names(), vec!["ping".to_string()]);
    }

    #[test]
    fn alias_resolves_to_canonical() {
        let mut reg = Registry::new();
        reg.register(
            RegisteredCommand::new("img-compress", simple_schema("img-compress")).alias("imgc"),
        )
        .unwrap();
        // 别名命中，返回规范命令
        assert_eq!(reg.get("imgc").unwrap().name, "img-compress");
        assert!(reg.contains("imgc"));
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new("ping", simple_schema("ping")))
            .unwrap();
        let err = reg.register(RegisteredCommand::new("ping", simple_schema("ping")));
        assert!(matches!(err, Err(RegistryError::Duplicate(_))));
    }

    #[test]
    fn empty_name_rejected() {
        let mut reg = Registry::new();
        let err = reg.register(RegisteredCommand::new("", simple_schema("x")));
        assert!(matches!(err, Err(RegistryError::EmptyName)));
    }

    #[test]
    fn hidden_excluded_from_visible() {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new("open", simple_schema("open")))
            .unwrap();
        reg.register(RegisteredCommand::new("secret", simple_schema("secret")).hidden(true))
            .unwrap();
        let visible: Vec<&str> = reg.visible().map(|c| c.name.as_str()).collect();
        assert_eq!(visible, vec!["open"]);
        // 但 get 仍可访问隐藏命令
        assert!(reg.contains("secret"));
    }

    #[test]
    fn register_from_json_loads_commands() {
        let mut reg = Registry::new();
        let json = r#"
        [
            {
                "name": "ping",
                "aliases": ["p"],
                "hidden": false,
                "schema": {
                    "name": "ping",
                    "about": "ping the service",
                    "args": [],
                    "subcommands": []
                }
            }
        ]"#;
        reg.register_from_json(json).unwrap();
        assert_eq!(reg.get("p").unwrap().name, "ping");
        assert!(reg.get("ping").unwrap().handler.is_none());
    }

    #[test]
    fn register_from_json_rejects_invalid() {
        let mut reg = Registry::new();
        assert!(reg.register_from_json("not json").is_err());
    }

    #[test]
    fn to_json_roundtrip() {
        let mut reg = Registry::new();
        reg.register(RegisteredCommand::new("ping", simple_schema("ping")))
            .unwrap();
        let json = reg.to_json();
        let back: Vec<RegisteredCommand> = serde_json::from_value(json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "ping");
    }

    /// 手工实现 App（core 测试不依赖过程宏），验证 from_app 适配器
    struct Dummy;

    impl App for Dummy {
        fn schema() -> CommandSchema {
            simple_schema("dummy")
        }

        fn from_args(
            _args: &std::collections::HashMap<String, serde_json::Value>,
        ) -> Result<Self, AppError> {
            Ok(Dummy)
        }

        fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
            ctx.done(serde_json::json!({ "ok": true }), 1);
            Ok(serde_json::json!({ "ok": true }))
        }
    }

    #[test]
    fn from_app_adapts_app_type() {
        let cmd = RegisteredCommand::from_app::<Dummy>();
        assert_eq!(cmd.name, "dummy");
        assert!(cmd.handler.is_some());
    }
}
