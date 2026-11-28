//! 语义视图组件协议
//!
//! 后端只声明「这是什么组件、带什么数据」，不关心前端怎么画。
//! 内置 GUI 渲染为令牌化的 HTML，React + shadcn/ui 或其他 UI 可按同一份
//! JSON 自行实现；协议层不含任何前端依赖。
//!
//! ```ignore
//! ctx.view("download", Component::Progress {
//!     label: Some("下载中".into()),
//!     current: Some(3),
//!     total: Some(10),
//! });
//! ```

use serde::{Deserialize, Serialize};

use crate::progress::LogLevel;

/// 语义色调：前端映射到自己的 success / warning / destructive 变体
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Success,
    Warn,
    Danger,
}

/// 单条聊天消息
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageEntry {
    pub author: String,
    pub time: String,
    pub body: String,
    #[serde(default)]
    pub edited: bool,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default)]
    pub mine: bool,
}

/// 输入区模式
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerMode {
    #[default]
    Normal,
    Insert,
}

/// 按钮的视觉权重（语义，不是具体配色）
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonVariant {
    Primary,
    Outline,
    Ghost,
    Danger,
}

/// 图表类型
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
}

/// Sheet 的贴边方向
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheetSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// 折叠面板的一项
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccordionItem {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub open: bool,
}

/// 标签页的一项
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub children: Vec<Component>,
}

/// 导航项（侧栏 / 顶部导航）
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavItem {
    pub id: String,
    pub label: String,
}

/// 轮播的一项
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarouselItem {
    pub title: Option<String>,
    #[serde(default)]
    pub children: Vec<Component>,
}

/// 键值对
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyValueItem {
    pub key: String,
    pub value: String,
}

/// 日志行
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

/// 菜单项
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    /// 要触发的命令（与 `Component::Button::action` 同义）
    pub action: Option<String>,
    #[serde(default)]
    pub danger: bool,
}

/// 菜单分组
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuGroup {
    pub label: String,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

/// 命令面板项
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandItem {
    pub label: String,
    pub action: String,
    pub hint: Option<String>,
}

/// 一个视图组件。序列化形如 `{"kind":"progress","current":3,"total":10}`。
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Component {
    // ── 容器 / 布局 ──────────────────────────────
    Card {
        title: Option<String>,
        description: Option<String>,
        #[serde(default)]
        children: Vec<Component>,
    },
    Section {
        title: Option<String>,
        #[serde(default)]
        children: Vec<Component>,
    },
    Separator {
        label: Option<String>,
    },
    ScrollArea {
        max_height: Option<String>,
        #[serde(default)]
        children: Vec<Component>,
    },
    AspectRatio {
        ratio: f64,
        #[serde(default)]
        children: Vec<Component>,
    },
    Collapsible {
        id: String,
        title: String,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
    Accordion {
        #[serde(default)]
        items: Vec<AccordionItem>,
    },
    Tabs {
        #[serde(default)]
        items: Vec<TabItem>,
        active: Option<String>,
    },
    Breadcrumb {
        #[serde(default)]
        items: Vec<String>,
    },
    Pagination {
        page: u64,
        pages: u64,
    },
    Sidebar {
        #[serde(default)]
        items: Vec<NavItem>,
        active: Option<String>,
    },
    Resizable {
        #[serde(default = "half")]
        ratio: f64,
        #[serde(default)]
        start: Vec<Component>,
        #[serde(default)]
        end: Vec<Component>,
    },
    Carousel {
        #[serde(default)]
        index: u64,
        #[serde(default)]
        items: Vec<CarouselItem>,
    },

    // ── 数据展示 ────────────────────────────────
    Progress {
        label: Option<String>,
        current: Option<u64>,
        total: Option<u64>,
    },
    Badge {
        text: String,
        #[serde(default = "neutral")]
        tone: Tone,
    },
    Alert {
        title: String,
        description: Option<String>,
        #[serde(default = "neutral")]
        tone: Tone,
    },
    Toast {
        text: String,
        #[serde(default = "neutral")]
        tone: Tone,
    },
    Skeleton {
        #[serde(default = "three")]
        lines: u32,
    },
    Avatar {
        name: String,
        src: Option<String>,
    },
    KeyValue {
        #[serde(default)]
        items: Vec<KeyValueItem>,
    },
    Table {
        caption: Option<String>,
        #[serde(default)]
        columns: Vec<String>,
        #[serde(default)]
        rows: Vec<Vec<String>>,
        #[serde(default)]
        sortable: bool,
    },
    Chart {
        chart_kind: ChartKind,
        #[serde(default)]
        labels: Vec<String>,
        #[serde(default)]
        series: Vec<f64>,
    },
    Log {
        #[serde(default)]
        lines: Vec<LogLine>,
    },
    Result {
        ok: bool,
        title: String,
        value: Option<serde_json::Value>,
    },

    // ── 操作 ────────────────────────────────────
    Button {
        id: String,
        label: String,
        action: String,
        #[serde(default = "outline")]
        variant: ButtonVariant,
        #[serde(default)]
        disabled: bool,
    },
    Toggle {
        id: String,
        label: String,
        #[serde(default)]
        pressed: bool,
    },
    ToggleGroup {
        id: String,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        selected: Vec<String>,
    },
    Tooltip {
        text: String,
        #[serde(default)]
        children: Vec<Component>,
    },
    HoverCard {
        trigger: String,
        title: String,
        body: String,
    },

    // ── 表单控件（表单参数优先用 ArgKind） ──────
    Input {
        name: String,
        label: Option<String>,
        #[serde(default)]
        value: String,
        placeholder: Option<String>,
    },
    Textarea {
        name: String,
        label: Option<String>,
        #[serde(default)]
        value: String,
        #[serde(default = "three")]
        rows: u32,
    },
    Select {
        name: String,
        label: Option<String>,
        #[serde(default)]
        value: String,
        #[serde(default)]
        options: Vec<String>,
    },
    Checkbox {
        name: String,
        label: String,
        #[serde(default)]
        checked: bool,
    },
    RadioGroup {
        name: String,
        label: Option<String>,
        #[serde(default)]
        options: Vec<String>,
        selected: Option<String>,
    },
    Switch {
        name: String,
        label: String,
        #[serde(default)]
        checked: bool,
    },
    Slider {
        name: String,
        label: Option<String>,
        #[serde(default)]
        min: f64,
        #[serde(default = "hundred")]
        max: f64,
        #[serde(default)]
        value: f64,
    },
    Combobox {
        name: String,
        label: Option<String>,
        #[serde(default)]
        value: String,
        #[serde(default)]
        options: Vec<String>,
    },
    InputOtp {
        name: String,
        label: Option<String>,
        #[serde(default)]
        value: String,
        #[serde(default = "six")]
        length: u32,
    },
    Calendar {
        month: String,
        selected: Option<String>,
        #[serde(default)]
        marked: Vec<String>,
    },
    DatePicker {
        name: String,
        label: Option<String>,
        value: Option<String>,
    },
    Form {
        #[serde(default)]
        children: Vec<Component>,
        submit: Option<String>,
    },
    Label {
        text: String,
        html_for: Option<String>,
    },

    // ── 浮层 / 导航 ─────────────────────────────
    Dialog {
        id: String,
        title: String,
        description: Option<String>,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
    Sheet {
        id: String,
        title: String,
        #[serde(default = "right")]
        side: SheetSide,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
    Drawer {
        id: String,
        title: String,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
    AlertDialog {
        id: String,
        title: String,
        description: Option<String>,
        #[serde(default)]
        open: bool,
        #[serde(default = "confirm")]
        confirm: String,
        #[serde(default = "cancel")]
        cancel: String,
    },
    Popover {
        id: String,
        trigger: String,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
    DropdownMenu {
        id: String,
        trigger: String,
        #[serde(default)]
        items: Vec<MenuItem>,
    },
    ContextMenu {
        #[serde(default)]
        items: Vec<MenuItem>,
    },
    Menubar {
        #[serde(default)]
        menus: Vec<MenuGroup>,
    },
    Command {
        query: Option<String>,
        #[serde(default)]
        items: Vec<CommandItem>,
    },
    NavigationMenu {
        #[serde(default)]
        items: Vec<NavItem>,
        active: Option<String>,
    },

    // ── 聊天 ────────────────────────────────────
    MessageList {
        entries: Vec<MessageEntry>,
        #[serde(default)]
        selected: Option<usize>,
        #[serde(default)]
        unread: Option<usize>,
    },
    Composer {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        mode: ComposerMode,
        #[serde(default)]
        attachments: Vec<String>,
    },
    Pane {
        id: String,
        title: String,
        #[serde(default)]
        active: bool,
        #[serde(default)]
        children: Vec<Component>,
    },
}

fn half() -> f64 {
    0.5
}
fn neutral() -> Tone {
    Tone::Neutral
}
fn outline() -> ButtonVariant {
    ButtonVariant::Outline
}
fn right() -> SheetSide {
    SheetSide::Right
}
fn three() -> u32 {
    3
}
fn six() -> u32 {
    6
}
fn hundred() -> f64 {
    100.0
}
fn confirm() -> String {
    "确认".into()
}
fn cancel() -> String {
    "取消".into()
}

/// 静态视图槽位：命令在页面里预留的一块区域
///
/// 由 `#[view(id = "...", title = "...", kind = "...")]` 生成；
/// 运行时用 [`crate::context::Context::view`] 按同一 `id` 填充。
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSlot {
    pub id: String,
    pub title: Option<String>,
    /// 预期组件类型（如 `progress`），供前端先画骨架
    pub kind: Option<String>,
}

impl ViewSlot {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: None,
            kind: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(c: Component) -> serde_json::Value {
        let s = serde_json::to_string(&c).expect("serialize");
        let back: Component = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(c, back);
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn progress_shape_is_stable() {
        let v = roundtrip(Component::Progress {
            label: Some("下载中".into()),
            current: Some(3),
            total: Some(10),
        });
        assert_eq!(v["kind"], "progress");
        assert_eq!(v["current"], 3);
        assert_eq!(v["total"], 10);
        assert_eq!(v["label"], "下载中");
    }

    #[test]
    fn indeterminate_progress_omits_numbers() {
        let v = roundtrip(Component::Progress {
            label: None,
            current: None,
            total: None,
        });
        assert_eq!(v["kind"], "progress");
        assert!(v["current"].is_null());
    }

    #[test]
    fn tone_and_variant_are_snake_case() {
        let v = roundtrip(Component::Badge {
            text: "就绪".into(),
            tone: Tone::Success,
        });
        assert_eq!(v["tone"], "success");
        let v = roundtrip(Component::Button {
            id: "run".into(),
            label: "运行".into(),
            action: "run".into(),
            variant: ButtonVariant::Primary,
            disabled: false,
        });
        assert_eq!(v["variant"], "primary");
    }

    #[test]
    fn containers_nest() {
        let v = roundtrip(Component::Card {
            title: Some("下载".into()),
            description: None,
            children: vec![
                Component::Progress {
                    label: None,
                    current: Some(1),
                    total: Some(2),
                },
                Component::Separator { label: None },
            ],
        });
        assert_eq!(v["kind"], "card");
        assert_eq!(v["children"][0]["kind"], "progress");
        assert_eq!(v["children"][1]["kind"], "separator");
    }

    #[test]
    fn table_rows_and_chart_series_survive() {
        let v = roundtrip(Component::Table {
            caption: Some("结果".into()),
            columns: vec!["文件".into(), "字节".into()],
            rows: vec![vec!["a.txt".into(), "12".into()]],
            sortable: true,
        });
        assert_eq!(v["rows"][0][1], "12");
        let v = roundtrip(Component::Chart {
            chart_kind: ChartKind::Line,
            labels: vec!["1".into(), "2".into()],
            series: vec![1.5, 2.5],
        });
        assert_eq!(v["kind"], "chart");
        assert_eq!(v["series"][1], 2.5);
    }

    #[test]
    fn defaults_fill_when_fields_absent() {
        let c: Component = serde_json::from_str(r#"{"kind":"badge","text":"x"}"#).unwrap();
        assert_eq!(
            c,
            Component::Badge {
                text: "x".into(),
                tone: Tone::Neutral
            }
        );
        let c: Component = serde_json::from_str(r#"{"kind":"skeleton"}"#).unwrap();
        assert_eq!(c, Component::Skeleton { lines: 3 });
    }

    #[test]
    fn chat_components_serialize_with_kinds() {
        let list = Component::MessageList {
            entries: vec![MessageEntry {
                author: "兰雀".into(),
                time: "12:00".into(),
                body: "你好".into(),
                edited: false,
                reply_to: None,
                tone: Tone::Neutral,
                mine: false,
            }],
            selected: Some(0),
            unread: Some(0),
        };
        let v = serde_json::to_value(&list).expect("serialize");
        assert_eq!(v["kind"], "message_list");
        assert_eq!(v["entries"][0]["tone"], "neutral");

        let v = serde_json::to_value(Component::Composer {
            value: "hi".into(),
            placeholder: "写点什么".into(),
            mode: ComposerMode::Insert,
            attachments: vec!["a.png".into()],
        })
        .expect("serialize");
        assert_eq!(v["kind"], "composer");
        assert_eq!(v["mode"], "insert");

        let v = serde_json::to_value(Component::Pane {
            id: "main".into(),
            title: "聊天".into(),
            active: true,
            children: vec![],
        })
        .expect("serialize");
        assert_eq!(v["kind"], "pane");
    }

    #[test]
    fn message_entry_defaults_fill_when_fields_absent() {
        let entry: MessageEntry =
            serde_json::from_str(r#"{"author":"a","time":"t","body":"b"}"#).expect("deserialize");
        assert!(!entry.edited);
        assert!(!entry.mine);
        assert_eq!(entry.reply_to, None);
        assert_eq!(entry.tone, Tone::Neutral);
    }

    #[test]
    fn view_slot_builder() {
        let s = ViewSlot::new("download").title("下载").kind("progress");
        assert_eq!(s.id, "download");
        assert_eq!(s.title.as_deref(), Some("下载"));
        assert_eq!(s.kind.as_deref(), Some("progress"));
    }
}
