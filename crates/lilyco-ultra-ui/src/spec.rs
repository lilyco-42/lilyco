//! # Lilyco Ultra UI — Spec
//!
//! Excel 风格的声明式 UI 描述格式。
//! 用户只需写 JSON，无需 Rust 代码，即可生成完整的 React 前端界面。

use serde::{Deserialize, Serialize};

/// UI 规格定义 — 整个界面的根描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSpec {
    #[serde(rename = "窗口")]
    pub window: WindowSpec,
}

/// 窗口定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSpec {
    #[serde(rename = "标题", default = "WindowSpec::default_title")]
    pub title: String,
    #[serde(rename = "大小", default)]
    pub size: WindowSize,
    #[serde(rename = "元素", default)]
    pub elements: Vec<ElementSpec>,
    /// 布局模式：流式（默认）或绝对定位（设计器产出）
    #[serde(rename = "布局", default)]
    pub layout: LayoutMode,
    /// 绝对定位坐标，与 `elements` 一一对应（流式布局时为空）
    #[serde(rename = "位置", default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<Position>,
}

impl WindowSpec {
    fn default_title() -> String {
        "Lilyco Ultra UI".into()
    }
}

/// 窗口大小：小 / 中等 / 大 / 全屏（默认中等）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WindowSize {
    小,
    #[default]
    中等,
    大,
    全屏,
}

impl WindowSize {
    pub fn css_class(&self) -> &'static str {
        match self {
            WindowSize::小 => "small",
            WindowSize::中等 => "medium",
            WindowSize::大 => "large",
            WindowSize::全屏 => "fullscreen",
        }
    }
}

/// 布局模式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    /// 文档流（默认，与旧 JSON 兼容）
    #[default]
    Flow,
    /// 绝对定位（拖拽设计器产出）
    Absolute,
}

/// 一个元素的绝对定位（设计器写入，generator 读取）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// 文本颜色（可选，设计器属性面板）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// 按钮样式变体
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
}

impl ButtonVariant {
    pub fn css_class(&self) -> &'static str {
        match self {
            ButtonVariant::Primary => "",
            ButtonVariant::Secondary => "secondary",
            ButtonVariant::Danger => "danger",
        }
    }
}

/// 界面元素 — 使用内部标签枚举，JSON 中通过 "类型" 字段区分
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "类型")]
pub enum ElementSpec {
    #[serde(rename = "文本")]
    Text {
        #[serde(rename = "内容")]
        content: String,
    },

    #[serde(rename = "标题")]
    Heading {
        #[serde(rename = "内容")]
        content: String,
        #[serde(rename = "级别", default = "default_heading_level")]
        level: u8,
    },

    #[serde(rename = "按钮")]
    Button {
        #[serde(rename = "文本")]
        text: String,
        #[serde(rename = "动作", default)]
        action: String,
        #[serde(rename = "样式", default)]
        variant: ButtonVariant,
    },

    #[serde(rename = "输入框")]
    Input {
        #[serde(rename = "标签")]
        label: String,
        #[serde(rename = "变量")]
        var_name: String,
        #[serde(rename = "默认", default)]
        default: Option<String>,
        #[serde(rename = "占位符", default)]
        placeholder: Option<String>,
    },

    #[serde(rename = "数字框")]
    Number {
        #[serde(rename = "标签")]
        label: String,
        #[serde(rename = "变量")]
        var_name: String,
        #[serde(rename = "默认", default)]
        default: Option<f64>,
        #[serde(rename = "最小", default)]
        min: Option<f64>,
        #[serde(rename = "最大", default)]
        max: Option<f64>,
    },

    #[serde(rename = "选择框")]
    Select {
        #[serde(rename = "标签")]
        label: String,
        #[serde(rename = "变量")]
        var_name: String,
        #[serde(rename = "选项")]
        options: Vec<String>,
        #[serde(rename = "默认", default)]
        default: Option<String>,
    },

    #[serde(rename = "复选框")]
    Checkbox {
        #[serde(rename = "标签")]
        label: String,
        #[serde(rename = "变量")]
        var_name: String,
        #[serde(rename = "默认", default)]
        default: Option<bool>,
    },

    #[serde(rename = "文本域")]
    Textarea {
        #[serde(rename = "标签")]
        label: String,
        #[serde(rename = "变量")]
        var_name: String,
        #[serde(rename = "默认", default)]
        default: Option<String>,
        #[serde(rename = "行数", default = "default_textarea_rows")]
        rows: u8,
    },

    #[serde(rename = "图片")]
    Image {
        #[serde(rename = "链接")]
        src: String,
        #[serde(rename = "宽度", default = "default_image_width")]
        width: String,
    },

    #[serde(rename = "分隔线")]
    Divider,

    #[serde(rename = "进度条")]
    Progress {
        #[serde(rename = "进度", default = "default_progress")]
        percent: f64,
        #[serde(rename = "标签", default)]
        label: Option<String>,
    },

    #[serde(rename = "链接")]
    Link {
        #[serde(rename = "文本")]
        text: String,
        #[serde(rename = "链接")]
        href: String,
    },

    /// 计算器 — 内置计算器组件
    #[serde(rename = "计算器")]
    Calculator {
        #[serde(rename = "变量", default = "default_calc_var")]
        var_name: String,
        #[serde(rename = "模式", default = "default_calc_mode")]
        mode: CalcMode,
    },
}

fn default_heading_level() -> u8 {
    2
}
fn default_textarea_rows() -> u8 {
    4
}
fn default_image_width() -> String {
    "100%".into()
}
fn default_progress() -> f64 {
    0.0
}
fn default_calc_var() -> String {
    "calc_result".into()
}
fn default_calc_mode() -> CalcMode {
    CalcMode::Standard
}

/// 计算器模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CalcMode {
    #[default]
    Standard,
    Scientific,
    Programmer,
}

impl UiSpec {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.window.title.is_empty() {
            errors.push("窗口标题不能为空".into());
        }
        if self.window.elements.is_empty() {
            errors.push("窗口元素列表为空 — 至少需要一个元素".into());
        }
        if self.window.layout == LayoutMode::Absolute
            && self.window.positions.len() != self.window.elements.len()
        {
            errors.push(format!(
                "绝对布局要求位置数量与元素一致：元素 {} 个，位置 {} 个",
                self.window.elements.len(),
                self.window.positions.len()
            ));
        }
        let mut seen_vars = std::collections::HashSet::new();
        for (i, el) in self.window.elements.iter().enumerate() {
            if let Some(var) = el.var_name() {
                if !var.is_empty() && !seen_vars.insert(var.to_string()) {
                    errors.push(format!("元素 #{}: 变量名 \"{}\" 重复", i + 1, var));
                }
            }
            if let ElementSpec::Select { options, .. } = el {
                if options.is_empty() {
                    errors.push(format!("元素 #{}: 选择框的选项列表不能为空", i + 1));
                }
            }
            if let ElementSpec::Heading { level, .. } = el {
                if !(*level >= 1 && *level <= 6) {
                    errors.push(format!(
                        "元素 #{}: 标题级别必须在 1~6 之间，当前为 {}",
                        i + 1,
                        level
                    ));
                }
            }
        }
        errors
    }
}

impl ElementSpec {
    pub fn var_name(&self) -> Option<&str> {
        match self {
            ElementSpec::Input { var_name, .. }
            | ElementSpec::Number { var_name, .. }
            | ElementSpec::Select { var_name, .. }
            | ElementSpec::Checkbox { var_name, .. }
            | ElementSpec::Textarea { var_name, .. }
            | ElementSpec::Calculator { var_name, .. } => Some(var_name),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            ElementSpec::Text { .. } => "文本",
            ElementSpec::Heading { .. } => "标题",
            ElementSpec::Button { .. } => "按钮",
            ElementSpec::Input { .. } => "输入框",
            ElementSpec::Number { .. } => "数字框",
            ElementSpec::Select { .. } => "选择框",
            ElementSpec::Checkbox { .. } => "复选框",
            ElementSpec::Textarea { .. } => "文本域",
            ElementSpec::Image { .. } => "图片",
            ElementSpec::Divider => "分隔线",
            ElementSpec::Progress { .. } => "进度条",
            ElementSpec::Link { .. } => "链接",
            ElementSpec::Calculator { .. } => "计算器",
        }
    }
}

/// 返回默认示例 JSON
pub fn default_example_json() -> String {
    r#"{
  "窗口": {
    "标题": "图片压缩工具",
    "大小": "中等",
    "元素": [
      {"类型": "标题", "内容": "图片压缩", "级别": 1},
      {"类型": "文本", "内容": "选择图片文件并设置压缩参数"},
      {"类型": "分隔线"},
      {"类型": "输入框", "标签": "输入文件", "变量": "input", "占位符": "/path/to/image.png"},
      {"类型": "数字框", "标签": "质量 (1-100)", "变量": "quality", "默认": 75, "最小": 1, "最大": 100},
      {"类型": "选择框", "标签": "输出格式", "变量": "format", "选项": ["jpeg", "png", "webp"], "默认": "jpeg"},
      {"类型": "复选框", "标签": "预览模式（不写入文件）", "变量": "dry_run"},
      {"类型": "分隔线"},
      {"类型": "按钮", "文本": "开始压缩", "动作": "compress", "样式": "primary"},
      {"类型": "按钮", "文本": "重置", "动作": "reset", "样式": "secondary"}
    ]
  }
}"#.into()
}

/// 返回计算器示例 JSON
pub fn calculator_example_json() -> String {
    r#"{
  "窗口": {
    "标题": "科学计算器",
    "大小": "中等",
    "元素": [
      {"类型": "标题", "内容": "计算器", "级别": 2},
      {"类型": "文本", "内容": "支持标准、科学、程序员模式"},
      {"类型": "分隔线"},
      {"类型": "计算器", "变量": "calc_result", "模式": "scientific"},
      {"类型": "分隔线"},
      {"类型": "按钮", "文本": "复制结果", "动作": "copy_result", "样式": "secondary"}
    ]
  }
}"#
    .into()
}
