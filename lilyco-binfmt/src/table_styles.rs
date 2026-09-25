//! 「这张表套的是哪个样式」——同一问在两家是两种东西，而一家的缓存值与位不是一套
//!
//! OOXML 把样式名写在表自己身上（`w:tblPr/w:tblStyle/@w:val`，那是一个**样式 id**，不是给人看的
//! 名字），另有一枚 `w:tblLook`：六个 `w:firstRow`… 的位 + 一个 `w:val` 的十六进制缓存值。
//! ODF 只写一个 `table:style-name`（点在一份 family=table 的样式上），**没有那枚 look**。
//!
//! 实测三条（`table-style.docx` 由 python-docx 写四张表，一次只改一个变量）：
//! 1. `w:tblLook` 的两本账可以不一致：把 `w:firstRow` 改成 `0` 之后，python-docx **不动** `w:val`
//!    （还是 `04A0`），而 LibreOffice 重写同一份时把它重算成 `0480` —— 两边都按写的交，
//!    不判谁对，也不拿一个去顶替另一个。
//! 2. 同一个缓存值两家的**大小写**不同（`04A0` 对 `04a0`）—— 那是写法，不是两个不同的位图。
//! 3. 转成 ODF 之后 `LightGrid-Accent1` 这个名字**整个不见了**：四张表各自点一份自动样式
//!    （`表格1`…`表格4`，都没有父样式）—— 样式那一路的信息在这一转里是丢了的，交出来而不是补。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// OOXML 那一份：逐表交「点了哪个样式 id」与「`w:tblLook` 写了哪几个位」
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut styles: Vec<String> = Vec::new();
    let mut with_style = 0usize;
    let mut with_look = 0usize;
    for (index, table) in body.descendants("tbl").iter().enumerate() {
        let holder = table.children.iter().find(|kid| kid.local() == "tblPr");
        let named =
            holder.and_then(|had| had.children.iter().find(|kid| kid.local() == "tblStyle"));
        let style_written = named
            .and_then(|had| had.attr_local("val"))
            .map(String::from);
        let look = holder.and_then(|had| had.children.iter().find(|kid| kid.local() == "tblLook"));
        let mut written: serde_json::Map<String, Value> = serde_json::Map::new();
        if let Some(one) = look {
            for (key, value) in &one.attrs {
                if key == "xmlns" || key.starts_with("xmlns:") {
                    continue;
                }
                let local = key.rsplit(':').next().unwrap_or(key).to_string();
                written.insert(local, json!(value));
            }
        }
        if style_written.is_some() {
            with_style += 1;
            if !styles
                .iter()
                .any(|had| *had == style_written.clone().unwrap_or_default())
            {
                styles.push(style_written.clone().unwrap_or_default());
            }
        }
        if look.is_some() {
            with_look += 1;
        }
        rows.push(json!({
            "index": index,
            "style_written": style_written,
            "has_tblPr": holder.is_some(),
            "has_tbl_look": look.is_some(),
            "look_written": Value::Object(written),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "tables_total": rows.len(),
        "with_style_written": with_style,
        "with_look": with_look,
        "distinct_styles": styles,
        "tables": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份的 family=table 样式表（两份件都扫，同名先到的一条算数）
fn table_styles(roots: &[&Node]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for root in roots {
        for one in root.descendants("style").into_iter() {
            if one.attr_local("family") != Some("table") {
                continue;
            }
            if let Some(name) = one.attr_local("name") {
                if !out.iter().any(|had| *had == *name) {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// ODF 那一份：表只点一个样式名；那枚 look 在这一族不存在，所以不交 0 也不交 null
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<&Node> = vec![content];
    if let Some(extra) = styles {
        roots.push(extra);
    }
    let known = table_styles(&roots);
    let mut rows: Vec<Value> = Vec::new();
    let mut found_count = 0usize;
    for (index, table) in content.descendants("table").iter().enumerate() {
        let name = table.attr_local("style-name");
        let found = name
            .map(|want| known.iter().any(|had| *had == *want))
            .unwrap_or(false);
        if found {
            found_count += 1;
        }
        rows.push(json!({
            "index": index,
            "table_name": table.attr_local("name").map(String::from),
            "style_written": name.map(String::from),
            "style_found": found,
            "parent_style_written": name
                .and_then(|want| {
                    roots
                        .iter()
                        .flat_map(|had| had.descendants("style"))
                        .find(|had| had.attr_local("name") == Some(want)
                            && had.attr_local("family") == Some("table"))
                })
                .and_then(|had| had.attr_local("parent-style-name"))
                .map(String::from),
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "tables_total": rows.len(),
        "with_style_written": rows.iter().filter(|one| !one["style_written"].is_null()).count(),
        "style_found_total": found_count,
        "styles_total": known.len(),
        "tables": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
