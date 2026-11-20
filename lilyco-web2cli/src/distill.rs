//! 网页内容蒸馏器：将任意 HTML / JSON 压缩为紧凑、带编号的 CLI 视图与结构化数据。
//!
//! 灵感来自 `only-cli/oc` 的低 token 编号视图：
//! - 提取标题、正文分段、表格、`[1]..[N]` 可交互元素（链接、输入框、按钮、下拉框）
//! - 支持 token 预算截断、关键词检索（`find`）、分段阅读（`read <n>`）与 Markdown 导出（`raw`）
//! - 对 JSON API 响应自动摘要数组条目与差异字段

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dom::{collapse_ws, DomTree};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Link,
    Input,
    Select,
    Button,
    Form,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberedAction {
    pub index: usize,
    pub kind: ActionKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledPage {
    pub url: String,
    pub title: String,
    pub is_json: bool,
    pub actions: Vec<NumberedAction>,
    pub sections: Vec<String>,
    pub tables: Vec<TableData>,
    pub markdown: String,
    pub compact_view: String,
}

/// 蒸馏 HTML 或 JSON 响应为结构化 `DistilledPage`。
pub fn distill_response(
    url: &str,
    content_type: &str,
    body: &str,
    budget_chars: usize,
) -> DistilledPage {
    let trimmed = body.trim_start();
    if content_type.contains("application/json")
        || content_type.contains("+json")
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
    {
        if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
            return distill_json(url, &json_val, budget_chars);
        }
    }
    distill_html(url, body, budget_chars)
}

fn distill_json(url: &str, val: &Value, budget_chars: usize) -> DistilledPage {
    let mut actions = Vec::new();
    let mut sections = Vec::new();
    let mut lines = vec![format!("# JSON Endpoint: {url}")];

    match val {
        Value::Array(arr) => {
            lines.push(format!("Items: {}", arr.len()));
            for (i, item) in arr.iter().take(50).enumerate() {
                let summary = summarize_json_item(item);
                let line = format!("[{}] {}", i + 1, summary);
                lines.push(line.clone());
                sections.push(serde_json::to_string_pretty(item).unwrap_or(summary.clone()));
                actions.push(NumberedAction {
                    index: i + 1,
                    kind: ActionKind::Link,
                    label: summary,
                    href: extract_url_field(item),
                    name: None,
                    input_type: None,
                });
            }
        }
        Value::Object(map) => {
            lines.push(format!("Keys: {}", map.len()));
            for (i, (k, v)) in map.iter().enumerate() {
                let summary = summarize_json_item(v);
                lines.push(format!("[{}] {k}: {summary}", i + 1));
                sections.push(format!(
                    "{k}: {}",
                    serde_json::to_string_pretty(v).unwrap_or(summary.clone())
                ));
                actions.push(NumberedAction {
                    index: i + 1,
                    kind: ActionKind::Link,
                    label: format!("{k}: {summary}"),
                    href: extract_url_field(v),
                    name: Some(k.clone()),
                    input_type: None,
                });
            }
        }
        other => {
            let s = other.to_string();
            lines.push(s.clone());
            sections.push(s);
        }
    }

    let markdown = serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string());
    let compact_view = truncate_budget(&lines.join("\n"), budget_chars);

    DistilledPage {
        url: url.to_string(),
        title: format!("JSON ({url})"),
        is_json: true,
        actions,
        sections,
        tables: Vec::new(),
        markdown,
        compact_view,
    }
}

fn summarize_json_item(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut parts = Vec::new();
            for (k, val) in map.iter().take(5) {
                let val_s = match val {
                    Value::String(s) => truncate_inline(s, 48),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    Value::Array(a) => format!("[{} items]", a.len()),
                    Value::Object(o) => format!("{{{} keys}}", o.len()),
                };
                parts.push(format!("{k}={val_s}"));
            }
            if map.len() > 5 {
                parts.push(format!("+{} more", map.len() - 5));
            }
            parts.join(", ")
        }
        Value::Array(a) => format!("[{} items]", a.len()),
        Value::String(s) => truncate_inline(s, 80),
        other => other.to_string(),
    }
}

fn extract_url_field(v: &Value) -> Option<String> {
    let Value::Object(map) = v else {
        return None;
    };
    for key in ["url", "html_url", "href", "link", "uri"] {
        if let Some(Value::String(s)) = map.get(key) {
            if s.starts_with("http://") || s.starts_with("https://") {
                return Some(s.clone());
            }
        }
    }
    None
}

pub fn distill_html(url: &str, html: &str, budget_chars: usize) -> DistilledPage {
    let dom = DomTree::parse(html);
    let base = url::Url::parse(url).ok();

    let title = dom
        .select("title")
        .first()
        .map(|&id| dom.text(id))
        .filter(|t| !t.is_empty())
        .or_else(|| dom.select("h1").first().map(|&id| dom.text(id)))
        .unwrap_or_else(|| url.to_string());

    let mut actions = Vec::new();
    let mut seen_links = std::collections::HashSet::new();

    // Collect interactive elements in document order
    for id in dom.iter_ids() {
        let Some(tag) = dom.tag(id) else {
            continue;
        };
        match tag {
            "a" => {
                let Some(href) = dom.attr(id, "href") else {
                    continue;
                };
                let href = href.trim();
                if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
                    continue;
                }
                let resolved = resolve_url(base.as_ref(), href);
                let label = dom.text(id);
                let label = if label.is_empty() {
                    dom.attr(id, "aria-label")
                        .or_else(|| dom.attr(id, "title"))
                        .unwrap_or(&resolved)
                        .to_string()
                } else {
                    label
                };
                if label.is_empty() {
                    continue;
                }
                let dedupe_key = format!("{resolved}|{label}");
                if seen_links.insert(dedupe_key) {
                    actions.push(NumberedAction {
                        index: actions.len() + 1,
                        kind: ActionKind::Link,
                        label: truncate_inline(&label, 80),
                        href: Some(resolved),
                        name: None,
                        input_type: None,
                    });
                }
            }
            "input" => {
                let itype = dom.attr(id, "type").unwrap_or("text").to_ascii_lowercase();
                if itype == "hidden" {
                    continue;
                }
                let name = dom.attr(id, "name").map(String::from);
                let label = dom
                    .attr(id, "placeholder")
                    .or_else(|| dom.attr(id, "aria-label"))
                    .or_else(|| dom.attr(id, "name"))
                    .or_else(|| dom.attr(id, "value"))
                    .unwrap_or("input")
                    .to_string();
                let kind = if matches!(itype.as_str(), "submit" | "button" | "reset") {
                    ActionKind::Button
                } else {
                    ActionKind::Input
                };
                actions.push(NumberedAction {
                    index: actions.len() + 1,
                    kind,
                    label: truncate_inline(&label, 60),
                    href: None,
                    name,
                    input_type: Some(itype),
                });
            }
            "textarea" => {
                let name = dom.attr(id, "name").map(String::from);
                let label = dom
                    .attr(id, "placeholder")
                    .or_else(|| dom.attr(id, "aria-label"))
                    .or_else(|| dom.attr(id, "name"))
                    .unwrap_or("textarea")
                    .to_string();
                actions.push(NumberedAction {
                    index: actions.len() + 1,
                    kind: ActionKind::Input,
                    label: truncate_inline(&label, 60),
                    href: None,
                    name,
                    input_type: Some("textarea".to_string()),
                });
            }
            "select" => {
                let name = dom.attr(id, "name").map(String::from);
                let label = dom
                    .attr(id, "aria-label")
                    .or_else(|| dom.attr(id, "name"))
                    .unwrap_or("select")
                    .to_string();
                actions.push(NumberedAction {
                    index: actions.len() + 1,
                    kind: ActionKind::Select,
                    label: truncate_inline(&label, 60),
                    href: None,
                    name,
                    input_type: Some("select".to_string()),
                });
            }
            "button" => {
                let label = dom.text(id);
                let label = if label.is_empty() {
                    dom.attr(id, "aria-label").unwrap_or("button").to_string()
                } else {
                    label
                };
                actions.push(NumberedAction {
                    index: actions.len() + 1,
                    kind: ActionKind::Button,
                    label: truncate_inline(&label, 60),
                    href: None,
                    name: dom.attr(id, "name").map(String::from),
                    input_type: dom.attr(id, "type").map(String::from),
                });
            }
            _ => {}
        }
    }

    // Extract tables
    let mut tables = Vec::new();
    for tid in dom.select("table") {
        let mut headers = Vec::new();
        let mut rows = Vec::new();
        for &child in &dom.node(tid).children {
            collect_table_rows(&dom, child, &mut headers, &mut rows);
        }
        if !headers.is_empty() || !rows.is_empty() {
            tables.push(TableData { headers, rows });
        }
    }

    // Extract readable markdown & sections
    let mut md_lines = vec![format!("# {title}")];
    let mut sections = Vec::new();
    for id in dom.iter_ids() {
        let Some(tag) = dom.tag(id) else {
            continue;
        };
        match tag {
            "h1" | "h2" | "h3" | "h4" => {
                let txt = dom.text(id);
                if !txt.is_empty() {
                    let level = tag[1..].parse::<usize>().unwrap_or(2);
                    let hashes = "#".repeat(level);
                    md_lines.push(format!("\n{hashes} {txt}"));
                    sections.push(txt);
                }
            }
            "p" | "li" | "blockquote" | "pre" => {
                let txt = dom.text(id);
                if txt.len() >= 12 {
                    let line = if tag == "li" {
                        format!("- {txt}")
                    } else if tag == "blockquote" {
                        format!("> {txt}")
                    } else {
                        txt.clone()
                    };
                    md_lines.push(line);
                    sections.push(txt);
                }
            }
            _ => {}
        }
    }
    if sections.is_empty() {
        let body_txt = dom
            .select("body")
            .first()
            .map(|&id| dom.text(id))
            .unwrap_or_else(|| dom.text(dom.root()));
        if !body_txt.is_empty() {
            md_lines.push(body_txt.clone());
            sections.push(body_txt);
        }
    }
    let markdown = md_lines.join("\n");

    // Build compact CLI view
    let mut view = Vec::new();
    view.push(format!("# {title}"));
    view.push(format!("URL: {url}"));
    if !actions.is_empty() {
        view.push("\n## Actions".to_string());
        for a in actions.iter().take(40) {
            let tag = match a.kind {
                ActionKind::Link => "link",
                ActionKind::Input => "input",
                ActionKind::Select => "select",
                ActionKind::Button => "btn",
                ActionKind::Form => "form",
            };
            if let Some(href) = &a.href {
                view.push(format!(
                    "[{}] ({tag}) {} -> {}",
                    a.index,
                    a.label,
                    truncate_inline(href, 60)
                ));
            } else {
                view.push(format!("[{}] ({tag}) {}", a.index, a.label));
            }
        }
        if actions.len() > 40 {
            view.push(format!("... (+{} more actions)", actions.len() - 40));
        }
    }
    if !sections.is_empty() {
        view.push("\n## Content".to_string());
        for (i, sec) in sections.iter().take(15).enumerate() {
            view.push(format!("({}) {}", i + 1, truncate_inline(sec, 180)));
        }
    }
    view.push("\nactions: do <n> | read <n> | find <query> | raw | inspect".to_string());
    let compact_view = truncate_budget(&view.join("\n"), budget_chars);

    DistilledPage {
        url: url.to_string(),
        title,
        is_json: false,
        actions,
        sections,
        tables,
        markdown,
        compact_view,
    }
}

fn collect_table_rows(
    dom: &DomTree,
    node_id: usize,
    headers: &mut Vec<String>,
    rows: &mut Vec<Vec<String>>,
) {
    match dom.tag(node_id) {
        Some("tr") => {
            let mut ths = Vec::new();
            let mut tds = Vec::new();
            for &c in &dom.node(node_id).children {
                match dom.tag(c) {
                    Some("th") => ths.push(collapse_ws(&dom.text(c))),
                    Some("td") => tds.push(collapse_ws(&dom.text(c))),
                    _ => {}
                }
            }
            if !ths.is_empty() && headers.is_empty() {
                *headers = ths;
            }
            if !tds.is_empty() {
                rows.push(tds);
            }
        }
        Some("thead" | "tbody" | "tfoot") => {
            for &c in &dom.node(node_id).children {
                collect_table_rows(dom, c, headers, rows);
            }
        }
        _ => {}
    }
}

pub fn resolve_url(base: Option<&url::Url>, rel: &str) -> String {
    if let Some(b) = base {
        if let Ok(u) = b.join(rel) {
            return u.to_string();
        }
    }
    rel.to_string()
}

fn truncate_inline(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn truncate_budget(s: &str, budget_chars: usize) -> String {
    if budget_chars == 0 || s.chars().count() <= budget_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(budget_chars).collect();
    out.push_str("\n... [truncated; use `raw` or increase --budget]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distills_html_links_forms_and_tables() {
        let html = r#"
        <html>
          <head><title>Example Portal</title></head>
          <body>
            <h1>Welcome</h1>
            <p>This is a concise description of the example portal for testing.</p>
            <a href="/docs">Documentation</a>
            <form action="/search">
              <input type="text" name="q" placeholder="Search docs">
              <button type="submit">Search</button>
            </form>
            <table>
              <tr><th>Name</th><th>Role</th></tr>
              <tr><td>Alice</td><td>Admin</td></tr>
            </table>
          </body>
        </html>
        "#;
        let page = distill_response("https://example.com/", "text/html", html, 4000);
        assert_eq!(page.title, "Example Portal");
        assert!(!page.is_json);
        assert!(page
            .actions
            .iter()
            .any(|a| a.href.as_deref() == Some("https://example.com/docs")));
        assert!(page.actions.iter().any(|a| a.name.as_deref() == Some("q")));
        assert_eq!(page.tables.len(), 1);
        assert_eq!(page.tables[0].headers, vec!["Name", "Role"]);
        assert_eq!(page.tables[0].rows[0], vec!["Alice", "Admin"]);
        assert!(page.compact_view.contains("[1]"));
    }

    #[test]
    fn distills_json_array_response() {
        let json = r#"[{"id": 1, "name": "lilyco", "url": "https://example.com/1"}]"#;
        let page = distill_response(
            "https://api.example.com/items",
            "application/json",
            json,
            4000,
        );
        assert!(page.is_json);
        assert_eq!(page.actions.len(), 1);
        assert_eq!(
            page.actions[0].href.as_deref(),
            Some("https://example.com/1")
        );
    }
}
