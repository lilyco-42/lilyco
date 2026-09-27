//! 这一格的字贴哪一边 —— 而「整页的字在纸上居中吗」是同名另一个问。
//!
//! docx 里答案是**一枚元素**：`w:tcPr/w:vAlign` 带一枚 `@w:val`（ECMA 四态 `top` /
//! `center` / `bottom` / `just`）；同名的 `w:sectPr/w:vAlign` 回答的是另一问（这一节的字
//! 在页面里顶对齐还是居中），所以两本分开数。pptx 里答案是**一枚属性**：`a:tcPr/@anchor`
//! （`t` / `ctr` / `b` / `just`），另有一枚 `@anchorCtr`（连水平方向一起居中）。
//! ODF 是一跳：格点 `table:style-name`，值在 `family="table-cell"` 那份样式的 properties 上，
//! 词表又是第三套（`top` / `middle` / `bottom`）。
//!
//! 实测（`valign.docx` = python-docx 打底 + 按 ECMA 手写四格四态与一枚节上的；
//! `valign.pptx` = python-pptx 打底 + 按 ECMA 手写四枚 `@anchor` 与一枚 `@anchorCtr`；
//! `valign.odt` / `valign.odp` = LibreOffice 两转；`valign-lo.*` = 同格式重写）：
//! 1. 三族词表不同名而不换算：docx `center` 到 ODF 是 `middle`、到 pptx 是 `ctr`，
//!    本仓各按自己那族那串交，不折成一个标准词；
//! 2. **「没这枚元素」与「有这枚但没写值」与「写了空串」是三句话**：ODF 那一跳实测写出
//!    `style:vertical-align=""` 两枚（那是 LibreOffice 表达「这一格没定」的写法），
//!    于是账里有 `(空串)` 一格，而 pptx 的两枚属性都不写是另一格（`cells_silent`）；
//! 3. LibreOffice 重写这份 docx 时把 `top` 与 `just` **整枚丢掉**（4 格剩 2 格：`center` 与
//!    `bottom`），但节上那一枚 `center` 留着 —— 默认值不写是它的算法；
//! 4. LibreOffice 重写那份 pptx 时给每格都写上 `@anchor`（`just` 被换成 `t`、没写的两格
//!    补成 `t`），而 `@anchorCtr` **一枚都不剩**；
//! 5. odp 这一头：转换之后**没有任何一格带 `style:vertical-align`**（表还在、样式还在，
//!    这一条整层没写下来）—— 所以 odp 那本只有 0，不是读不出来；
//! 6. 真件普查（本机 32 份 .docx + 1 份 .docm、104 份 pptx）：docx 格级 `vAlign` 只出现
//!    `center` 2898 与 `bottom` 1，`top` 与 `just` **零条**，节上那一枚也**零条**；
//!    pptx 的 893 枚 `a:tcPr` 里 `@anchor` 只有 `ctr` 314 次、`@anchorCtr` 零次，
//!    其余 579 枚两枚都不写 —— 所以四态与 `anchorCtr` 与节上那一支都只在自产件里；
//! 7. 本机真件里没有一份 ODF（.odt / .ods / .odp 零份）—— ODF 那一头只有生产者的凭据。
//!
//! 不做的事：**不统一词表**（三族各交自己那串）、**不推默认**（docx 的 `top` 是排版默认，
//! 但「没写」不等于「写了 top」，那一格只说没写）、**不判渲染**（贴哪边与行高、边距怎么共同
//! 决定字落在哪，是排版的事）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

fn kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

fn attr_of<'a>(node: &'a Node, want: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(key, _)| key.rsplit(':').next().unwrap_or(key) == want)
        .map(|(_, value)| value.as_str())
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_i64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

/// `w:tcPr` / `w:sectPr` 里那枚 `w:vAlign` →（在不在，它写的串）
fn val_child(holder: &Node, want: &str) -> (bool, Option<String>) {
    for kid in kids(holder) {
        if kid.local() == want {
            return (true, attr_of(kid, "val").map(|one| one.to_string()));
        }
    }
    (false, None)
}

fn val_or_none(mine: &Option<String>) -> Value {
    match mine {
        Some(had) => json!(had.as_str()),
        None => Value::Null,
    }
}

/// docx / docm：格级那一本与节上那一本（同名两个问，分开交）
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/") && one.ends_with(".xml") && !one.ends_with(".rels"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut sect_rows: Vec<Value> = Vec::new();
    let mut cells_total = 0usize;
    let mut cells_with = 0usize;
    let mut vals = serde_json::Map::new();
    let mut sections_total = 0usize;
    let mut sections_with = 0usize;
    let mut sect_vals = serde_json::Map::new();
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        let mut index = 0usize;
        for cell in root.descendants("tc").iter() {
            let mut found = false;
            let mut got: Option<String> = None;
            for one in cell.descendants("tcPr").iter() {
                let (had, mine) = val_child(one, "vAlign");
                found = had;
                got = mine;
                break;
            }
            cells_total += 1;
            if found {
                cells_with += 1;
                let key = match &got {
                    Some(had) => had.clone(),
                    None => "(没写 val)".to_string(),
                };
                bump(&mut vals, key.as_str());
            }
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "cell": index,
                    "val": val_or_none(&got),
                    "said": found,
                }));
            }
            index += 1;
        }
        let mut sect_index = 0usize;
        for sect in root.descendants("sectPr").iter() {
            sections_total += 1;
            let (found, got) = val_child(sect, "vAlign");
            if found {
                sections_with += 1;
                let key = match &got {
                    Some(had) => had.clone(),
                    None => "(没写 val)".to_string(),
                };
                bump(&mut sect_vals, key.as_str());
            }
            if sect_rows.len() < limit {
                sect_rows.push(json!({
                    "part": name.as_str(),
                    "section": sect_index,
                    "val": val_or_none(&got),
                    "said": found,
                }));
            }
            sect_index += 1;
        }
    }
    let st_val = match parse_member(bytes, "word/styles.xml") {
        Some(root) => root.descendants("vAlign").len(),
        None => 0usize,
    };
    json!({
        "family": "ooxml",
        "available": true,
        "cells_total": cells_total,
        "cells_with_valign": cells_with,
        "cells_without_valign": cells_total - cells_with,
        "vals": Value::Object(vals),
        "sections_total": sections_total,
        "sections_with_valign": sections_with,
        "section_vals": Value::Object(sect_vals),
        "styles_part_valign": st_val,
        "rows": rows,
        "section_rows": sect_rows,
        "listed": rows.len(),
        "sect_listed": sect_rows.len(),
        "cut": cells_total > rows.len() || sections_total > sect_rows.len(),
    })
}

/// pptx / pptm：`a:tcPr` 上那两枚属性，两枚都不写才是真件里的常态
pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("ppt/slides/slide") && one.ends_with(".xml"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut parts_scanned = 0usize;
    let mut cells = 0usize;
    let mut with_anchor = 0usize;
    let mut silent = 0usize;
    let mut with_ctr = 0usize;
    let mut anchors = serde_json::Map::new();
    let mut ctr_vals = serde_json::Map::new();
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        parts_scanned += 1;
        for (index, holder) in root.descendants("tcPr").iter().enumerate() {
            cells += 1;
            let got = attr_of(holder, "anchor").map(|one| one.to_string());
            let ctr = attr_of(holder, "anchorCtr").map(|one| one.to_string());
            match &got {
                None => silent += 1,
                Some(had) => {
                    with_anchor += 1;
                    bump(&mut anchors, had.as_str());
                }
            }
            if let Some(had) = &ctr {
                with_ctr += 1;
                bump(&mut ctr_vals, had.as_str());
            }
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "cell": index,
                    "anchor": val_or_none(&got),
                    "anchor_ctr": val_or_none(&ctr),
                    "said": got.is_some(),
                }));
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": parts_scanned,
        "cells": cells,
        "cells_with_anchor": with_anchor,
        "cells_silent": silent,
        "cells_with_anchor_ctr": with_ctr,
        "anchors": Value::Object(anchors),
        "anchor_ctr_vals": Value::Object(ctr_vals),
        "rows": rows,
        "listed": rows.len(),
        "cut": cells > rows.len(),
    })
}

/// ODF 里垂直对齐会住在的三枚 properties（与 `table_borders` 同一批，实测前两枚各一）
const VAL_PROPS: [&str; 3] = [
    "table-cell-properties",
    "paragraph-properties",
    "graphic-properties",
];

/// 一份 `family="table-cell"` 样式：那一枚值住在哪枚孩子上、写的是什么串（空串照原样）
pub(crate) struct ValStyle {
    part: String,
    holder: Option<String>,
    val: Option<String>,
}

fn val_of_style(style: &Node) -> (Option<String>, Option<String>) {
    let mut holder: Option<String> = None;
    let mut got: Option<String> = None;
    for kid in kids(style) {
        if !VAL_PROPS.contains(&kid.local()) {
            continue;
        }
        if let Some(mine) = attr_of(kid, "vertical-align") {
            holder = Some(kid.local().to_string());
            got = Some(mine.to_string());
        }
    }
    (holder, got)
}

/// 跨两份部件收 table-cell 样式的那一枚值（`content.xml` 优先，与 `cell_margins` 同一口径）
pub(crate) fn odf_styles(bytes: &[u8]) -> Vec<(String, ValStyle)> {
    let mut out: Vec<(String, ValStyle)> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("style") {
            if one.attr_local("family") != Some("table-cell") {
                continue;
            }
            let name = match one.attr_local("name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            if out.iter().any(|had: &(String, ValStyle)| had.0 == name) {
                continue;
            }
            let (holder, val) = val_of_style(one);
            out.push((
                name,
                ValStyle {
                    part: part.to_string(),
                    holder,
                    val,
                },
            ));
        }
    }
    out
}

/// odt / odp：格点的名 → 那份样式的那一枚值（三族词表各按各的，空串是空串）
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let styles = odf_styles(bytes);
    let root = match parse_member(bytes, "content.xml") {
        Some(one) => one,
        None => {
            return json!({
                "family": "odf", "available": true, "cells": 0, "cells_named": 0,
                "cells_unnamed": 0, "styles_found": 0, "styles_unfound": 0,
                "cells_with_valign": 0, "vals": {}, "holders": {},
                "styles_defined": styles.len(), "rows": [], "listed": 0, "cut": false,
            })
        }
    };
    let mut all: Vec<&Node> = Vec::new();
    walk(&root, &mut all);
    let cells_here: Vec<&Node> = all
        .into_iter()
        .filter(|one| one.local() == "table-cell" || one.local() == "covered-table-cell")
        .collect();
    let mut rows: Vec<Value> = Vec::new();
    let mut cells = 0usize;
    let mut named = 0usize;
    let mut unnamed = 0usize;
    let mut found = 0usize;
    let mut unfound = 0usize;
    let mut with_val = 0usize;
    let mut vals = serde_json::Map::new();
    let mut holders = serde_json::Map::new();
    for one in cells_here.iter() {
        cells += 1;
        let want = one.attr_local("style-name");
        match want {
            None => unnamed += 1,
            Some(_) => named += 1,
        }
        let mine = want.and_then(|had| styles.iter().find(|one| one.0 == had).map(|hit| &hit.1));
        if want.is_some() && mine.is_none() {
            unfound += 1;
        }
        if mine.is_some() {
            found += 1;
        }
        let got: Option<String> = match mine {
            Some(had) => had.val.clone(),
            None => None,
        };
        let holder: Option<String> = match mine {
            Some(had) => had.holder.clone(),
            None => None,
        };
        if let Some(had) = &got {
            with_val += 1;
            let key = if had.is_empty() {
                "(空串)"
            } else {
                had.as_str()
            };
            bump(&mut vals, key);
            bump(&mut holders, holder.clone().unwrap_or_default().as_str());
        }
        if rows.len() < limit {
            rows.push(json!({
                "cell": cells - 1,
                "style_name": want,
                "style_found": mine.is_some(),
                "style_part": mine.map(|had| had.part.as_str()),
                "holder": holder,
                "val": val_or_none(&got),
                "written": got.is_some(),
            }));
        }
    }
    json!({
        "family": "odf",
        "available": true,
        "cells": cells,
        "cells_named": named,
        "cells_unnamed": unnamed,
        "styles_found": found,
        "styles_unfound": unfound,
        "cells_with_valign": with_val,
        "vals": Value::Object(vals),
        "holders": Value::Object(holders),
        "styles_defined": styles.len(),
        "rows": rows,
        "listed": rows.len(),
        "cut": cells > rows.len(),
    })
}

/// 按文档序走整棵树（`table-cell` 与 `covered-table-cell` 混排时两边同一序）
fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(node);
    for kid in node.children.iter() {
        if kid.name != "#text" {
            walk(kid, out);
        }
    }
}
