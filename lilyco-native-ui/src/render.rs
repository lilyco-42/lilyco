//! 把 `ElementSpec` 的 13 个变体穷尽映射成 NSH 原生元素。
//!
//! 这里是一份类型、两个后端的关键：spec.rs 里加一个变体，本文件的
//! `match` 立刻编译报错，React 后端与原生后端都不会静默漏掉它。

use std::collections::BTreeMap;

use lilyco_ultra_ui::spec::{ButtonVariant, CalcMode, ElementSpec};
use nsh_base::{SelectItem, button, checkbox, input, progress, select, theme};
use nsh_core::prelude::*;

/// 渲染器声明覆盖的全部元素类型（供测试与文档核对）。
pub const COVERED: &[&str] = &[
    "文本",
    "标题",
    "按钮",
    "输入框",
    "数字框",
    "选择框",
    "复选框",
    "文本域",
    "图片",
    "分隔线",
    "进度条",
    "链接",
    "计算器",
];

/// 交互状态。两个独立信号：改表单值只更新读 values 的节点，
/// 触发动作只更新读 notice 的节点 —— 这正是原生后端的细粒度更新。
#[derive(Clone, Copy)]
pub struct UiState {
    pub values: Signal<BTreeMap<String, String>>,
    pub notice: Signal<Option<String>>,
}

pub fn fmt_map(m: &BTreeMap<String, String>) -> String {
    if m.is_empty() {
        return "（空）".to_owned();
    }
    m.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn render_elements(elements: &[ElementSpec], state: &UiState) -> Vec<AnyElement> {
    elements
        .iter()
        .map(|el| render_element(el, state))
        .collect()
}

/// 基准模式下记录原生 on_press 到达时刻（正常模式零开销）。
fn bench_press(action: &str) {
    if std::env::var_os("LILYCO_BENCH").is_some() {
        let us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or_default();
        eprintln!("LILYCO_NATIVE_PRESS {} {}", us, action);
    }
}
/// 一个变体 → 一个原生元素。类型擦除成 AnyElement 后放进异构列表。
pub fn render_element(el: &ElementSpec, state: &UiState) -> AnyElement {
    match el {
        ElementSpec::Text { content } => text(content.clone()).into_element(),

        ElementSpec::Heading { content, level } => text(content.clone())
            .text_size(px(heading_size(*level)))
            .font_weight(FontWeight::BOLD)
            .into_element(),

        ElementSpec::Button {
            text: label,
            action,
            variant,
        } => {
            let action = action.clone();
            let st = *state;
            button()
                .on_press(move || {
                    bench_press(&action);
                    let snapshot = st.values.get();
                    st.notice.set(Some(format!(
                        "动作 {action} 已触发 · {}",
                        fmt_map(&snapshot)
                    )));
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(match variant {
                            ButtonVariant::Primary => theme().primary,
                            ButtonVariant::Secondary => theme().secondary,
                            ButtonVariant::Danger => theme().destructive,
                        })
                        .child(text(label.clone()).text_color(match variant {
                            ButtonVariant::Primary => theme().primary_foreground,
                            ButtonVariant::Secondary => theme().secondary_foreground,
                            ButtonVariant::Danger => theme().primary_foreground,
                        })),
                )
                .into_element()
        }

        ElementSpec::Input {
            label,
            var_name,
            default,
            placeholder,
        } => {
            let var = var_name.clone();
            let st = *state;
            let mut inp = input().on_change(move |t: &str| {
                st.values.update(|m| {
                    m.insert(var.clone(), t.to_owned());
                });
            });
            if let Some(d) = default {
                inp = inp.default_value(d.clone());
            }
            if let Some(p) = placeholder {
                inp = inp.placeholder(p.clone());
            }
            field(label, inp.h(px(34.0)).w_full().into_element())
        }

        ElementSpec::Number {
            label,
            var_name,
            default,
            min,
            max,
        } => {
            let var = var_name.clone();
            let st = *state;
            let mut inp = input()
                .keyboard_type(KeyboardType::Decimal)
                .on_change(move |t: &str| {
                    st.values.update(|m| {
                        m.insert(var.clone(), t.to_owned());
                    });
                });
            if let Some(d) = default {
                inp = inp.default_value(d.to_string());
            }
            let hint = match (min, max) {
                (Some(a), Some(b)) => format!("{a} ~ {b}"),
                (Some(a), None) => format!(">= {a}"),
                (None, Some(b)) => format!("<= {b}"),
                (None, None) => String::new(),
            };
            if !hint.is_empty() {
                inp = inp.placeholder(hint);
            }
            field(label, inp.h(px(34.0)).w_full().into_element())
        }

        ElementSpec::Select {
            label,
            var_name,
            options,
            default,
        } => {
            let items: Vec<SelectItem> = options
                .iter()
                .map(|o| SelectItem::new(o.clone(), o.clone()))
                .collect();
            let var = var_name.clone();
            let st = *state;
            let mut sel = select()
                .items(items)
                .on_change(move |v: &Option<SharedString>| {
                    let chosen = v
                        .as_ref()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();
                    st.values.update(|m| {
                        m.insert(var.clone(), chosen);
                    });
                });
            if let Some(d) = default {
                sel = sel.default_value(SharedString::from(d.clone()));
            }
            let trigger_label = default.clone().unwrap_or_else(|| "请选择".to_owned());
            sel = sel.trigger(
                view()
                    .w_full()
                    .h(px(34.0))
                    .flex_row()
                    .items_center()
                    .child(text(trigger_label)),
            );
            field(label, sel.into_element())
        }

        ElementSpec::Checkbox {
            label,
            var_name,
            default,
        } => {
            let var = var_name.clone();
            let st = *state;
            checkbox()
                .checked(default.unwrap_or(false))
                .on_change(move |next: &bool| {
                    st.values.update(|m| {
                        m.insert(var.clone(), next.to_string());
                    });
                })
                .child(text(label.clone()))
                .into_element()
        }

        ElementSpec::Textarea {
            label,
            var_name,
            default,
            rows,
        } => {
            let var = var_name.clone();
            let st = *state;
            let mut ta = input()
                .multiline(true)
                .scrollbar(false)
                .h(px(*rows as f32 * 22.0))
                .w_full()
                .on_change(move |t: &str| {
                    st.values.update(|m| {
                        m.insert(var.clone(), t.to_owned());
                    });
                });
            if let Some(d) = default {
                ta = ta.default_value(d.clone());
            }
            field(label, ta.into_element())
        }

        ElementSpec::Image { src, width } => {
            let mut img = image(src.clone()).w_full();
            if let Some(h) = parse_px(width) {
                img = img.h(px(h));
            }
            img.into_element()
        }

        ElementSpec::Divider => view().h(px(1.0)).w_full().bg(theme().border).into_element(),

        ElementSpec::Progress { percent, label } => {
            let pct = (percent * 100.0).clamp(0.0, 100.0);
            let caption = label.clone().unwrap_or_else(|| format!("{}%", pct as i32));
            view()
                .flex_col()
                .gap_2()
                .child(text(caption).text_size(px(13.0)))
                .child(
                    progress()
                        .value(pct as f32)
                        .w_full()
                        .track_style(|s| s.h(px(8.0)).rounded_md().bg(theme().border))
                        .indicator_style(|s| s.h_full().rounded_md().bg(theme().primary)),
                )
                .into_element()
        }

        ElementSpec::Link { text: label, href } => {
            let href = href.clone();
            button()
                .on_press(move || {
                    // 不关心是否成功打开：按 NSH 文档显式 drop 这个 Answer。
                    drop(open_url(href.clone()));
                })
                .child(text(label.clone()).text_color(move || theme().primary))
                .into_element()
        }

        ElementSpec::Calculator { var_name, mode } => {
            let expr = signal(String::new());
            let result = signal(String::new());
            let var = var_name.clone();
            let st = *state;
            view()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded_md()
                .bg(move || theme().card)
                .child(
                    text(format!("计算器 · {}", calc_mode_label(mode)))
                        .font_weight(FontWeight::BOLD),
                )
                .child(
                    input()
                        .placeholder("输入表达式，如 1+2*3")
                        .h(px(34.0))
                        .w_full()
                        .on_change(move |t: &str| expr.set(t.to_owned())),
                )
                .child(
                    button()
                        .on_press(move || {
                            let r = eval_expr(&expr.get());
                            st.values.update(|m| {
                                m.insert(var.clone(), r.clone());
                            });
                            result.set(r);
                        })
                        .child(
                            view()
                                .px_4()
                                .py_2()
                                .rounded_md()
                                .bg(move || theme().primary)
                                .child(text("计算")),
                        ),
                )
                .child(text(move || format!("结果: {}", result.get())).text_size(px(18.0)))
                .into_element()
        }
    }
}

fn field(label: &str, control: AnyElement) -> AnyElement {
    view()
        .flex_col()
        .gap_2()
        .child(text(label.to_owned()).text_size(px(13.0)))
        .child(control)
        .into_element()
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 28.0,
        2 => 22.0,
        3 => 18.0,
        4 => 16.0,
        5 => 15.0,
        _ => 14.0,
    }
}

fn calc_mode_label(mode: &CalcMode) -> &'static str {
    match mode {
        CalcMode::Standard => "标准",
        CalcMode::Scientific => "科学",
        CalcMode::Programmer => "程序员",
    }
}

fn parse_px(width: &str) -> Option<f32> {
    width.strip_suffix("px").and_then(|n| n.parse().ok())
}

/// 四则运算 + 括号的极简求值器：证明交互逻辑也能纯 Rust 跑在原生后端，
/// 不需要把表达式交给任何 JS 引擎。
fn eval_expr(src: &str) -> String {
    let b = src.as_bytes();
    let mut p = 0usize;
    match parse_sum(b, &mut p) {
        Ok(v)
            if {
                skip_ws(b, &mut p);
                p == b.len()
            } && v.is_finite() =>
        {
            format!("{v}")
        }
        _ => "表达式无效".to_owned(),
    }
}

fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && b[*p] == b' ' {
        *p += 1;
    }
}

fn parse_sum(b: &[u8], p: &mut usize) -> Result<f64, ()> {
    let mut v = parse_product(b, p)?;
    loop {
        skip_ws(b, p);
        if *p >= b.len() {
            break;
        }
        match b[*p] {
            b'+' => {
                *p += 1;
                v += parse_product(b, p)?;
            }
            b'-' => {
                *p += 1;
                v -= parse_product(b, p)?;
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_product(b: &[u8], p: &mut usize) -> Result<f64, ()> {
    let mut v = parse_atom(b, p)?;
    loop {
        skip_ws(b, p);
        if *p >= b.len() {
            break;
        }
        match b[*p] {
            b'*' => {
                *p += 1;
                v *= parse_atom(b, p)?;
            }
            b'/' => {
                *p += 1;
                v /= parse_atom(b, p)?;
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_atom(b: &[u8], p: &mut usize) -> Result<f64, ()> {
    skip_ws(b, p);
    if *p < b.len() && b[*p] == b'(' {
        *p += 1;
        let v = parse_sum(b, p)?;
        skip_ws(b, p);
        if *p >= b.len() || b[*p] != b')' {
            return Err(());
        }
        *p += 1;
        return Ok(v);
    }
    let start = *p;
    while *p < b.len() && (b[*p].is_ascii_digit() || b[*p] == b'.') {
        *p += 1;
    }
    if start == *p {
        return Err(());
    }
    std::str::from_utf8(&b[start..*p])
        .map_err(|_| ())?
        .parse::<f64>()
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::eval_expr;

    #[test]
    fn arithmetic() {
        assert_eq!(eval_expr("1+2*3"), "7");
        assert_eq!(eval_expr("(1+2)*3"), "9");
        assert_eq!(eval_expr("10/4"), "2.5");
        assert_eq!(eval_expr("1+"), "表达式无效");
    }
}
