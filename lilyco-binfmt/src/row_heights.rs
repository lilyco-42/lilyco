//! 这一行多高 —— 同一问在两族里放的地方不一样，所以两份账分开交。
//!
//! OOXML：高度写在**行上**（`w:trPr/w:trHeight`），而且一件事分两个属性说：`@w:val` 是那个
//! twips 数，`@w:hRule` 说这数是「至少」还是「正好」。三种「没有」在这一族里各有形状：
//! 这一行没有 `w:trPr`、有 `trPr` 而里面没 `trHeight`、有 `trHeight` 而没写 `hRule`
//! （缺省是 atLeast 是**规范**说的，不是文件写的，所以这里只交 `h_rule_written: false`）。
//!
//! ODF：行只点一个样式名（`table:style-name`），高与最小高在那一跳的目的地里 ——
//! `family="table-row"` 的样式的 `style:table-row-properties`，两个键是 `style:row-height`
//! 与 `style:min-row-height`。那一跳断了交 `style_found: false` 而**不**替行补一个高度。
//!
//! 实测（`row-height.docx` = python-docx 的四行表：exact 2.4cm / atLeast 1.2cm / 什么都不写 /
//! 写零并把 `hRule` 摘掉；`row-height.odt` 是 LibreOffice 导出的 ODF；`row-height-lo.docx`
//! 是那份 odt 再导回 docx）：
//! 1. **零是一句说过的话**：`@w:val="0"` 是 Word 里真用的「藏一行」手法，它与「这一行没写
//!    高度」是两件事，所以 `zero_height` 单独数一个数，不并进 `rows_with_height` 的补集；
//! 2. 来回一趟把零改掉了：LibreOffice 那一转把它写成 `@w:val="1" @w:hRule="atLeast"`
//!    （1/1440 英寸 ≈ 0.0176mm —— 它不承认「零高」，而 `1` 是它自己挑的数，不是本仓算的），
//!    而那个「什么都不写」的行它回来时带着**一枚空壳** `w:trPr`（`has_tr_pr` true 而
//!    `tr_pr_children` 空表）——「壳在」与「壳里写了什么」是两个数；
//! 3. 单位换算是有损的：docx 那 1361 twips（正好 2.4cm）在 ODF 里是 `2.401cm`，
//!    680 在 ODF 里是 `1.199cm` —— 两边都按文件自己写的串交，**不换算也不比对**；
//! 4. ODF 那一面「没写」的那一行，样式里确实躺着一条 `keep-together="auto"`
//!    （`props_written` 1）—— 样式在、键不在，与「那一跳解不开」又是两种形状。
//!
//! 不做的事：**不判这一行装不装得下它的内容**（那要算字号与行距，不是文件写着的一个数）、
//! **不换算 twips 与厘米**、**不补规范缺省值**（`hRule` 没写就交 null 加一个 `*_written: false`）。

use crate::office_sheet::written_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// 这一族的表能住在的那些部件（正文之外，页眉页脚里的表也算一行）
const ROW_PARTS: [&str; 5] = [
    "word/document.xml",
    "word/header",
    "word/footer",
    "word/footnotes.xml",
    "word/endnotes.xml",
];

fn element_kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

fn first_named<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    element_kids(node)
        .into_iter()
        .find(|one| one.local() == want)
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

fn wanted_part(name: &str) -> bool {
    ROW_PARTS.iter().any(|head| name.starts_with(head))
}

/// OOXML 那一面：一张表一行一条
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.ends_with(".xml") && wanted_part(one))
        .collect();
    names.sort_unstable();
    let mut rows: Vec<Value> = Vec::new();
    let mut tables = 0usize;
    let mut scanned = 0usize;
    for name in names.iter().take(4000) {
        let root = match parse_member(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        let mut found = false;
        let mut index = 0usize;
        for tbl in root.descendants("tbl") {
            index += 1;
            tables += 1;
            let mut row_index = 0usize;
            for tr in element_kids(tbl)
                .into_iter()
                .filter(|one| one.local() == "tr")
            {
                row_index += 1;
                found = true;
                let pr = first_named(tr, "trPr");
                let height = pr.and_then(|had| first_named(had, "trHeight"));
                let attrs = height.map(written_attrs).unwrap_or(Value::Null);
                let written = height.is_some();
                let rule = attrs.get("hRule").cloned();
                rows.push(json!({
                    "part": name,
                    "table": index - 1,
                    "row": row_index - 1,
                    "has_tr_pr": pr.is_some(),
                    "tr_pr_children": match pr {
                        Some(had) => element_kids(had)
                            .into_iter()
                            .map(|one| json!(one.local()))
                            .collect::<Vec<Value>>(),
                        None => Vec::new(),
                    },
                    "height_written": written,
                    "height_children": match height {
                        Some(had) => element_kids(had)
                            .into_iter()
                            .map(|one| json!(one.local()))
                            .collect::<Vec<Value>>(),
                        None => Vec::new(),
                    },
                    "attrs": attrs,
                    "val": match height {
                        Some(had) => had.attr_local("val").map(Value::from).unwrap_or(Value::Null),
                        None => Value::Null,
                    },
                    "h_rule": rule,
                    "h_rule_written": match height {
                        Some(had) => had.attr_local("hRule").is_some(),
                        None => false,
                    },
                }));
            }
        }
        if found {
            scanned += 1;
        }
    }
    // 三种「没有」各记各的：没 `trPr`、有 `trPr` 没 `trHeight`、有 `trHeight` 没 `hRule`
    let mut rules = serde_json::Map::new();
    for one in rows.iter() {
        let key = match one["h_rule"].as_str() {
            Some(had) => had.to_string(),
            None => {
                if one["height_written"].as_bool().unwrap_or(false) {
                    "(没写)".to_string()
                } else {
                    "(没有 trHeight)".to_string()
                }
            }
        };
        let hits = rules.get(&key).and_then(Value::as_u64).unwrap_or(0);
        rules.insert(key, json!(hits + 1));
    }
    let total = rows.len();
    let with_height = rows
        .iter()
        .filter(|one| one["height_written"].as_bool().unwrap_or(false))
        .count();
    let without_pr = rows
        .iter()
        .filter(|one| !one["has_tr_pr"].as_bool().unwrap_or(false))
        .count();
    let zero = rows.iter().filter(|one| one["val"] == "0").count();
    let listed = total.min(limit);
    let kept = rows.len().min(limit);
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": scanned,
        "tables": tables,
        "rows": rows.into_iter().take(kept).collect::<Vec<Value>>(),
        "rows_total": total,
        "rows_with_height": with_height,
        "rows_without_tr_pr": without_pr,
        "zero_height": zero,
        "rules": Value::Object(rules),
        "listed": listed,
        "cut": total > listed,
    })
}

struct RowStyle {
    part: &'static str,
    props: Vec<(String, String)>,
    parent: Option<String>,
}

fn prop_of<'a>(table: &'a [(String, String)], want: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|one| one.0 == want)
        .map(|one| one.1.as_str())
}

fn row_styles(bytes: &[u8]) -> Vec<(String, RowStyle)> {
    let mut out: Vec<(String, RowStyle)> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("style") {
            if one.attr_local("family") != Some("table-row") {
                continue;
            }
            let name = match one.attr_local("name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            if out.iter().any(|had: &(String, RowStyle)| had.0 == name) {
                continue;
            }
            let props = match first_named(one, "table-row-properties") {
                Some(had) => had
                    .attrs
                    .iter()
                    .filter(|(key, _)| key != "xmlns" && !key.starts_with("xmlns:"))
                    .map(|(key, value)| {
                        (
                            key.rsplit(':').next().unwrap_or(key).to_string(),
                            value.to_string(),
                        )
                    })
                    .collect::<Vec<(String, String)>>(),
                None => Vec::new(),
            };
            let mut sorted = props;
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push((
                name,
                RowStyle {
                    part,
                    props: sorted,
                    parent: one.attr_local("parent-style-name").map(String::from),
                },
            ));
        }
    }
    out
}

/// ODF 那一面：行点的样式名 → 那一跳的目的地里那两个键
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let styles = row_styles(bytes);
    let mut rows: Vec<Value> = Vec::new();
    let mut tables = 0usize;
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for tbl in root.descendants("table") {
            tables += 1;
            let mut index = 0usize;
            for one in element_kids(tbl) {
                let local = one.local();
                let (holder, in_header) = if local == "table-row" {
                    (one, false)
                } else if local == "table-header-rows" || local == "table-footer-rows" {
                    // 表头那一组只是包着几行：按文档序摊开，组本身不算一行
                    for kid in element_kids(one)
                        .into_iter()
                        .filter(|had| had.local() == "table-row")
                    {
                        push_row(&mut rows, &styles, part, tables - 1, index, kid, true);
                        index += 1;
                    }
                    continue;
                } else {
                    continue;
                };
                push_row(
                    &mut rows,
                    &styles,
                    part,
                    tables - 1,
                    index,
                    holder,
                    in_header,
                );
                index += 1;
            }
        }
    }
    let total = rows.len();
    let listed = total.min(limit);
    json!({
        "family": "odf",
        "available": true,
        "tables": tables,
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "rows_total": total,
        "rows_written": rows.iter().filter(|one| one["written"].as_bool().unwrap_or(false)).count(),
        "rows_unwritten": rows.iter().filter(|one| !one["written"].as_bool().unwrap_or(false)).count(),
        "styles_unfound": rows.iter().filter(|one| !one["style_found"].as_bool().unwrap_or(false)).count(),
        "zero_height": rows.iter().filter(|one| {
            one["row_height"] == "0cm" || one["row_height"] == "0"
                || one["min_row_height"] == "0cm" || one["min_row_height"] == "0"
        }).count(),
        "repeated_rows": rows.iter().filter(|one| {
            !one["repeated"].is_null() && one["repeated"] != "1"
        }).count(),
        "listed": listed,
        "cut": total > listed,
    })
}

/// 一行一条：那一跳解不开时样式那几格全交 null，不替行猜一个高度
fn push_row(
    rows: &mut Vec<Value>,
    styles: &[(String, RowStyle)],
    part: &str,
    table: usize,
    index: usize,
    tr: &Node,
    in_header: bool,
) {
    let name = tr.attr_local("style-name");
    let mine = name.and_then(|want| styles.iter().find(|had| had.0 == want).map(|hit| &hit.1));
    let props: &[(String, String)] = match mine {
        Some(had) => &had.props,
        None => &[],
    };
    let row_height = prop_of(props, "row-height");
    let min_height = prop_of(props, "min-row-height");
    let mut table_props = serde_json::Map::new();
    for (key, value) in props.iter() {
        table_props.insert(key.clone(), json!(value));
    }
    rows.push(json!({
        "part": part,
        "table": table,
        "row": index,
        "in_header": in_header,
        "style_name": name,
        "style_found": mine.is_some(),
        "style_part": mine.map(|had| had.part),
        "parent_style": mine.and_then(|had| had.parent.clone()),
        "row_height": row_height,
        "min_row_height": min_height,
        "keep_together": prop_of(props, "keep-together"),
        "written": row_height.is_some() || min_height.is_some(),
        "props": match mine {
            Some(_) => Value::Object(table_props),
            None => Value::Null,
        },
        "props_written": props.len(),
        "repeated": tr.attr_local("number-rows-repeated"),
        "visibility": tr.attr_local("visibility"),
    }));
}
