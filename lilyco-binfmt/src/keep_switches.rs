//! 「这一段与下一页的关系」那四个开关：一家写在段上（元素在场），一家一跳在样式里（而且不是一个开关）
//!
//! OOXML 把四个开关坐在 `w:pPr` 下面：`w:keepNext`（与下段同页）、`w:keepLines`（段中不分页）、
//! `w:pageBreakBefore`（段前分页）、`w:widowControl`（孤行控制）。前三个**没有值就是开着**，
//! 而 `w:widowControl` 这一族常反过来写 `w:val="0"` 表示关掉 —— 所以「在场」与「开着」不是一回事，
//! 两样都交：`present` 是元素在不在，`val` 是文件自己写的那个值（没写交 null，不兜 false）。
//!
//! ODF 一处都不写在段上：段只点一个样式名，那四个开关变成 `style:paragraph-properties` 上的
//! `fo:keep-with-next` / `fo:keep-together` / `fo:break-before` / `fo:widows`+`fo:orphans`。
//! 最后那两个是关键的一条：孤行控制在这一族**不是一个开关，而是两个数**（实测关掉它写
//! `fo:widows="0"` 配 `fo:orphans="0"`），所以两族形状不折算，各交各的。
//!
//! 实测（`keep.docx` 由 python-docx 的四条真 API 写，五段各开一个开关；另两份是 LibreOffice 转的）：
//! 1. 基线那段在 docx 里 `w:pPr` 整个没有，在 ODF 里点的是 `Standard` 而那份样式这四个词都没写
//!    —— 「没写」在两族都是**没有这个元素/属性**，交 null 而不是 false。
//! 2. `widow_control = False` 落进文件是 `w:val="0"`（写出来的关），而另外三个开关写出来是**空的
//!    元素**（值整个没有）—— 三种状态（没元素 / 有元素没值 / 有元素有值）分得开才敢数「几个开着」。
//! 3. RTF 那一族写 `\keepn` / `\pagebb` / `\nowidctlpar`，而同一份件里样式表自己就带
//!    `\keepn` 与 `\widctlpar`（实测 11 条 `\keepn` 里只有一条在正文段上）—— 归属判不住，
//!    这一支不读，规则先记在这里。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// 一个开关的三种状态：元素在不在、文件给没给值、按给的词算「开着」还是「关着」
fn switch(present: bool, val: Option<&str>) -> Value {
    let off = matches!(val, Some("0") | Some("false") | Some("off") | Some("none"));
    json!({
        "present": present,
        "val": val.map(String::from),
        "on_written": present && !off,
        "off_written": present && off,
    })
}

const DOCX_SWITCHES: [(&str, &str); 4] = [
    ("keepNext", "keep_next"),
    ("keepLines", "keep_lines"),
    ("pageBreakBefore", "page_break_before"),
    ("widowControl", "widow_control"),
];

/// OOXML 那一份：段上那四个开关逐个交，一个也不合并成「这份文档会不会分页」
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut indexed: Vec<u64> = Vec::new();
    let mut states = serde_json::Map::new();
    let mut p_pr = 0usize;
    for (index, para) in body.descendants("p").iter().enumerate() {
        let holder = para.child("pPr");
        if holder.is_some() {
            p_pr += 1;
        }
        let mut entry = json!({"index": index, "has_pPr": holder.is_some()});
        let mut touched = false;
        for (name, key) in DOCX_SWITCHES {
            let found = holder.and_then(|had| had.children.iter().find(|kid| kid.local() == name));
            let val = found.and_then(|had| had.attr_local("val"));
            let one = switch(found.is_some(), val);
            if found.is_some() {
                touched = true;
                let grouped = format!(
                    "{} {}",
                    name,
                    if val.is_none() { "bare" } else { "with_value" }
                );
                let next = states.get(&grouped).and_then(Value::as_u64).unwrap_or(0) + 1;
                states.insert(grouped, json!(next));
            }
            if let Some(map) = entry.as_object_mut() {
                map.insert(key.to_string(), one);
            }
        }
        if touched {
            indexed.push(index as u64);
        }
        rows.push(entry);
    }
    let with_any = indexed.len();
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs_total": rows.len(),
        "p_pr_elements": p_pr,
        "paragraphs_with_any": with_any,
        // 「第几段有」单独交一串号：只数一个数就看不出是哪一段
        "paragraphs_indexed": indexed,
        "states_written": Value::Object(states),
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

const ODF_KEEP: [(&str, &str); 5] = [
    ("keep-with-next", "keep_with_next"),
    ("keep-together", "keep_together"),
    ("break-before", "break_before"),
    ("widows", "widows"),
    ("orphans", "orphans"),
];

/// ODF 那一份：段只点样式名，那四个词在跳到的那份样式的段落属性里（按写的词交，不折算）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<&Node> = vec![content];
    if let Some(extra) = styles {
        roots.push(extra);
    }
    let mut table: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for root in &roots {
        for one in root.descendants("style").into_iter() {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            let mut written: Vec<(String, String)> = Vec::new();
            if let Some(holder) = one.child("paragraph-properties") {
                for key in [
                    "keep-with-next",
                    "keep-together",
                    "break-before",
                    "widows",
                    "orphans",
                ] {
                    if let Some(raw) = holder.attr_local(key) {
                        written.push((key.to_string(), raw.to_string()));
                    }
                }
            }
            table.push((name.to_string(), written));
        }
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut words = serde_json::Map::new();
    let mut resolved = 0usize;
    let mut with_any = 0usize;
    for (index, para) in content.descendants("p").iter().enumerate() {
        let name = para.attr_local("style-name");
        let found = name.and_then(|want| table.iter().find(|had| had.0 == want));
        let held: Vec<(String, String)> = found.map(|had| had.1.clone()).unwrap_or_default();
        let mut entry = json!({
            "index": index,
            "style_written": name.map(String::from),
            "style_found": found.is_some(),
        });
        for (key, want) in ODF_KEEP {
            let raw = held
                .iter()
                .find(|had| had.0 == key)
                .map(|had| had.1.as_str());
            if let Some(value) = raw {
                let grouped = format!("{key}={value}");
                let next = words.get(&grouped).and_then(Value::as_u64).unwrap_or(0) + 1;
                words.insert(grouped, json!(next));
            }
            if let Some(map) = entry.as_object_mut() {
                map.insert(
                    want.to_string(),
                    json!({"written": raw.map(String::from), "present": raw.is_some()}),
                );
            }
        }
        if !held.is_empty() {
            with_any += 1;
        }
        if found.is_some() {
            resolved += 1;
        }
        rows.push(entry);
    }
    json!({
        "family": "odf",
        "available": true,
        "paragraphs_total": rows.len(),
        "resolved": resolved,
        "paragraphs_with_any": with_any,
        "styles_total": table.len(),
        "words_written": Value::Object(words),
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
