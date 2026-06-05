use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use lilyco_core::schema::{ArgKind, ArgSchema};

// ── 表单字段值 ────────────────────────────────────────────

/// 单个表单字段的当前值
#[derive(Debug, Clone)]
pub enum FieldValue {
    Flag(bool),
    Text(String),
    Number(f64),
    Enum { values: Vec<String>, selected: usize },
    Path(String),
    List { values: Vec<String>, cursor: usize },
}

impl FieldValue {
    /// 从 ArgSchema 创建初始值
    pub fn from_schema(arg: &ArgSchema) -> Self {
        match &arg.kind {
            ArgKind::Flag => FieldValue::Flag(match &arg.default {
                Some(serde_json::Value::Bool(b)) => *b,
                _ => false,
            }),
            ArgKind::Text => FieldValue::Text(match &arg.default {
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => String::new(),
            }),
            ArgKind::Number { min, max: _ } => {
                let default = match &arg.default {
                    Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                    _ => min.unwrap_or(0.0),
                };
                FieldValue::Number(default)
            }
            ArgKind::Enum { values } => {
                let default = match &arg.default {
                    Some(serde_json::Value::String(s)) => {
                        values.iter().position(|v| v == s).unwrap_or(0)
                    }
                    _ => 0,
                };
                FieldValue::Enum {
                    values: values.clone(),
                    selected: default,
                }
            }
            ArgKind::Path { .. } => FieldValue::Path(match &arg.default {
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => String::new(),
            }),
            ArgKind::List { .. } => FieldValue::List {
                values: Vec::new(),
                cursor: 0,
            },
        }
    }
}

// ── 表单字段 ──────────────────────────────────────────────

/// 一个完整的表单字段（定义 + 当前值）
#[derive(Debug, Clone)]
pub struct FormField {
    pub name: String,
    pub about: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub value: FieldValue,
}

impl FormField {
    pub fn from_schema(arg: &ArgSchema) -> Self {
        Self {
            name: arg.name.clone(),
            about: arg.about.clone(),
            required: arg.required,
            default: arg.default.clone(),
            value: FieldValue::from_schema(arg),
        }
    }

    /// 渲染该字段的标签 + widget 到 buffer
    pub fn render(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        let focus_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        // 标签： [*] name（* 表示 required）
        let required = if self.required { "(*)" } else { "" };
        let label = format!(" {:>16} {} ", self.name, required);

        buf.set_string(area.x, area.y, &label, Style::default().add_modifier(Modifier::BOLD));

        let widget_x = area.x + label.len() as u16;

        match &self.value {
            FieldValue::Flag(val) => {
                let marker = if *val { "[x]" } else { "[ ]" };
                buf.set_string(widget_x, area.y, marker, focus_style);
            }
            FieldValue::Text(val) | FieldValue::Path(val) => {
                let display = if val.is_empty() {
                    format!("<{}>", &self.about)
                } else {
                    val.clone()
                };
                let style = if focused {
                    Style::default().fg(Color::Cyan).bg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                };
                buf.set_string(widget_x, area.y, &display, style);
            }
            FieldValue::Number(val) => {
                let display = format!("{val}");
                buf.set_string(widget_x, area.y, &display, focus_style);
            }
            FieldValue::Enum { values, selected } => {
                let all_str = values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if i == *selected {
                            format!("[{}]", v.to_uppercase())
                        } else {
                            format!(" {} ", v)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                buf.set_string(widget_x, area.y, &all_str, focus_style);
            }
            FieldValue::List { values, cursor: _ } => {
                let display = if values.is_empty() {
                    "<empty>".to_string()
                } else {
                    values.join(", ")
                };
                buf.set_string(widget_x, area.y, &display, focus_style);
            }
        }
    }

    /// 处理按键，返回 true 表示值发生了变化
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        use KeyCode::*;
        match &mut self.value {
            FieldValue::Flag(val) => match key.code {
                Char(' ') => {
                    *val = !*val;
                    true
                }
                _ => false,
            },
            FieldValue::Text(val) | FieldValue::Path(val) => match key.code {
                Char(c) => {
                    val.push(c);
                    true
                }
                Backspace => {
                    if !val.is_empty() {
                        val.pop();
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            },
            FieldValue::Number(val) => match key.code {
                Up => {
                    *val += 1.0;
                    true
                }
                Down => {
                    *val -= 1.0;
                    true
                }
                Char(c) if c.is_ascii_digit() || c == '.' || c == '-' => {
                    let s = format!("{val}{c}");
                    if let Ok(n) = s.parse::<f64>() {
                        *val = n;
                        true
                    } else {
                        false
                    }
                }
                Backspace => {
                    let s = val.to_string();
                    if s.len() > 1 {
                        *val = s[..s.len() - 1].parse().unwrap_or(0.0);
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            },
            FieldValue::Enum { values, selected } => match key.code {
                Left => {
                    if *selected > 0 {
                        *selected -= 1;
                        true
                    } else {
                        false
                    }
                }
                Right => {
                    if *selected + 1 < values.len() {
                        *selected += 1;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            },
            FieldValue::List { values, cursor: _ } => match key.code {
                Enter => {
                    values.push(String::new());
                    true
                }
                Delete => {
                    if !values.is_empty() {
                        values.pop();
                        true
                    } else {
                        false
                    }
                }
                Char(c) => {
                    if let Some(last) = values.last_mut() {
                        last.push(c);
                        true
                    } else {
                        values.push(c.to_string());
                        true
                    }
                }
                Backspace => {
                    if let Some(last) = values.last_mut() {
                        if !last.is_empty() {
                            last.pop();
                            true
                        } else {
                            values.pop();
                            true
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            },
        }
    }

    /// 生成 CLI 命令行参数片段
    pub fn to_cli_arg(&self) -> Option<String> {
        match &self.value {
            FieldValue::Flag(false) => None,
            FieldValue::Flag(true) => {
                // 如果 default 也是 true，则不显示（省略默认值）
                if let Some(serde_json::Value::Bool(true)) = self.default {
                    None
                } else {
                    Some(format!("--{}", self.name))
                }
            }
            FieldValue::Text(val) => {
                if val.is_empty() {
                    return None;
                }
                // 与默认值比较
                if let Some(serde_json::Value::String(ref d)) = self.default {
                    if d == val {
                        return None;
                    }
                }
                Some(format!("--{} {val}", self.name))
            }
            FieldValue::Number(val) => {
                if let Some(serde_json::Value::Number(ref d)) = self.default {
                    if let Some(dv) = d.as_f64() {
                        if (dv - val).abs() < f64::EPSILON {
                            return None;
                        }
                    }
                }
                Some(format!("--{} {val}", self.name))
            }
            FieldValue::Enum { values, selected } => {
                let current = &values[*selected];
                if let Some(serde_json::Value::String(ref d)) = self.default {
                    if d == current {
                        return None;
                    }
                }
                Some(format!("--{} {current}", self.name))
            }
            FieldValue::Path(val) => {
                if val.is_empty() {
                    return None;
                }
                // 路径可能含空格，加引号
                let q = if val.contains(' ') { "\"" } else { "" };
                if let Some(serde_json::Value::String(ref d)) = self.default {
                    if d == val {
                        return None;
                    }
                }
                Some(format!("--{} {q}{val}{q}", self.name))
            }
            FieldValue::List { values, .. } => {
                if values.is_empty() {
                    return None;
                }
                Some(
                    values
                        .iter()
                        .map(|v| format!("--{} {v}", self.name))
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
        }
    }
}

// ── re-export ──────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
