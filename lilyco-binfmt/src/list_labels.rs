//! 这一级列表的标签摆在哪、后面跟什么：`style:list-level-properties` 与它的孩子
//!
//! 与第二读者 `office_reader.odf_list_labels` 同一条口径。父元素只说一种模式
//! （本仓 4770 处写的**全都**是 `label-alignment`），数值全住在孩子
//! `style:list-level-label-alignment` 上：`label-followed-by`（`listtab` 4255 /
//! `nothing` 515）、`list-tab-stop-position`、`fo:margin-left`、`fo:text-indent`。
//! 只数父元素那一层会一个数都量不到 —— 那是本仓第一次读这层时踩过的坑。
//!
//! `nothing` 那 515 处一枚数都不写（`with_numbers` 4255 与它是精确互补）：
//! 「标签后面什么也不跟」与「跟一个制表位并把正文缩进来」是两种排版，
//! 所以 `props` 与 `label` 两张表各交各的，不折成一个布尔。
//! 另一种模式（`label-placement`，值写在父元素的 `style:min-label-width` 上）本仓
//! 一份都没写 —— `min_label_widths` 是空清单而不是缺键。
//!
//! 宿主有两种：`style:list-style`（列表样式自己，名字是 `@style:name`）直接套
//! `list-level-style-number` / `-bullet` / `-image`，而 `style:outline-style`
//! （大纲那一族）套 `outline-level-style`。`@text:level` 在层级元素上，
//! 本仓量到 1..10 十档。只认前一种会漏 470 处 —— 那正是写 `nothing` 又不带数的
//! 那一族，所以另交 `elements_total` 与 `not_under_holder` 让漏没漏由文件自己说。
//!
//! 与 docx 那一本的关系：OOXML 把同一件事写在 `w:lvl/w:pPr/w:ind` 与 `w:numFmt` /
//! `w:suff` 上（`numbering` 那本已交），ODF 摊在「位置与间距」这一层，两本不并账。
//! `--limit` 只截 `entries`，每一份计数仍说整份件。

use crate::xmlscan::Node;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const LL_PROPS: [&str; 5] = [
    "list-level-position-and-space-mode",
    "min-label-width",
    "min-label-distance",
    "space-before",
    "space-after",
];
const LL_LABEL: [&str; 7] = [
    "label-followed-by",
    "list-tab-stop-position",
    "margin-left",
    "text-indent",
    "margin-right",
    "space-before",
    "space-after",
];
const LL_LEVELS: [&str; 4] = [
    "list-level-style-number",
    "list-level-style-bullet",
    "list-level-style-image",
    "outline-level-style",
];
const NUMBERS: [&str; 3] = ["list-tab-stop-position", "margin-left", "text-indent"];

fn table(node: &Node, allow: &[&str]) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if allow.iter().any(|one| *one == local) {
            out.insert(local.to_string(), json!(value));
        }
    }
    out
}

fn names(map: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = map.keys().cloned().collect();
    out.sort();
    out
}

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

/// 一张表里某枚属性的字符串值（没写就 None）
fn text_of(map: &serde_json::Map<String, Value>, key: &str) -> Option<&str> {
    map.get(key).and_then(Value::as_str)
}

fn distinct(rows: &[Value], table_key: &str, attr: &str) -> Vec<String> {
    let empty = serde_json::Map::new();
    let mut out: Vec<String> = Vec::new();
    for one in rows.iter() {
        if let Some(value) = text_of(one[table_key].as_object().unwrap_or(&empty), attr) {
            if !out.iter().any(|had| had == value) {
                out.push(value.to_string());
            }
        }
    }
    out.sort();
    out
}

fn push_levels(out: &mut Vec<Value>, style: &Node, list_style: Option<&str>, part: &str) {
    for level in style
        .children
        .iter()
        .filter(|one| LL_LEVELS.contains(&one.local()))
    {
        let lab = level.attr_local("level").map(String::from);
        for props in level
            .children
            .iter()
            .filter(|one| one.local() == "list-level-properties")
        {
            let mut merged = table(props, &LL_PROPS);
            let mut kid: Option<&Node> = None;
            for inner in props.children.iter() {
                if inner.local() == "list-level-label-alignment" {
                    kid = Some(inner);
                } else if inner.local() == "list-level-properties" {
                    // 同层再套一层的写法本仓没量到，认出来就别当没有
                    for (key, value) in table(inner, &LL_PROPS) {
                        merged.insert(key, value);
                    }
                }
            }
            let theirs = match kid {
                Some(one) => table(one, &LL_LABEL),
                None => serde_json::Map::new(),
            };
            out.push(json!({
                "index": out.len(),
                "part": part,
                "list_style": list_style,
                "holder": level.local(),
                "level": lab.clone(),
                "props_written": names(&merged),
                "props": Value::Object(merged),
                "child": kid.is_some(),
                "child_written": names(&theirs),
                "label": Value::Object(theirs),
            }));
        }
    }
}

pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut elements_total = 0usize;
    for (part, root) in roots.iter() {
        elements_total += root.descendants("list-level-properties").len();
        for style in root.descendants("list-style") {
            push_levels(&mut rows, style, style.attr_local("name"), part);
        }
        for style in root.descendants("outline-style") {
            push_levels(&mut rows, style, style.attr_local("name"), part);
        }
    }
    let mut by_part: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_holder: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_mode: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_followed: BTreeMap<String, u64> = BTreeMap::new();
    let mut parts_seen: Vec<String> = Vec::new();
    let mut levels: Vec<String> = Vec::new();
    let mut with_child = 0usize;
    let mut with_numbers = 0usize;
    let mut nothing_bare = 0usize;
    let empty = serde_json::Map::new();
    for one in rows.iter() {
        let part = one["part"].as_str().unwrap_or_default();
        bump(&mut by_part, part);
        if !parts_seen.iter().any(|had| had == part) {
            parts_seen.push(part.to_string());
        }
        bump(&mut by_holder, one["holder"].as_str().unwrap_or_default());
        let props = one["props"].as_object().unwrap_or(&empty);
        if let Some(had) = text_of(props, "list-level-position-and-space-mode") {
            bump(&mut by_mode, had);
        }
        let label = one["label"].as_object().unwrap_or(&empty);
        if let Some(had) = text_of(label, "label-followed-by") {
            bump(&mut by_followed, had);
        }
        if one["child"].as_bool().unwrap_or(false) {
            with_child += 1;
        }
        if NUMBERS.iter().all(|key| label.contains_key(*key)) {
            with_numbers += 1;
        }
        if text_of(label, "label-followed-by") == Some("nothing")
            && !NUMBERS.iter().any(|key| label.contains_key(*key))
        {
            nothing_bare += 1;
        }
        if let Some(had) = one["level"].as_str() {
            if !levels.iter().any(|one2| one2 == had) {
                levels.push(had.to_string());
            }
        }
    }
    levels.sort();
    parts_seen.sort();
    let written = rows.len();
    json!({
        "family": "odf",
        "available": true,
        "elements_total": elements_total,
        "with_child": with_child,
        "with_numbers": with_numbers,
        "not_under_holder": elements_total.saturating_sub(written),
        "nothing_without_numbers": nothing_bare,
        "by_part": by_part,
        "by_holder": by_holder,
        "by_mode": by_mode,
        "by_followed": by_followed,
        "levels": levels,
        "tab_stops": distinct(&rows, "label", "list-tab-stop-position"),
        "indents": distinct(&rows, "label", "text-indent"),
        "margins": distinct(&rows, "label", "margin-left"),
        "min_label_widths": distinct(&rows, "props", "min-label-width"),
        "parts_seen": parts_seen,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "entries": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}
