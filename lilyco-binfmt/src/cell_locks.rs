//! 单元格样式自己那两枚锁定位：`xl/styles.xml` 里 `protection` 住在哪一本、挂在第几个格式上。
//!
//! 与 `protect.rs` 那一本不是一件事。那一本答「这张表锁了没、这份文档锁了没」，读的是
//! `sheetProtection` / `documentProtection` / `table:protected`；这一本读的是**格式层**：
//! `xf`（以及条件格式的 `dxf`）里那一枚 `protection`，写着 `@locked` 与 `@hidden`。
//! 表没锁时这些位一个都不生效，所以「表没锁」与「格式没设锁定位」是两句话，实测也确实分家：
//! 41 份 `.xlsx` 里 **18 份**写这一层（全出自 LibreOffice 那一路，共 63 枚），openpyxl 那 23 份
//! 一枚都不写；而写着的每一枚都同时交两枚属性，值全是 `locked="true"` / `hidden="false"` ——
//! LibreOffice 把 ECMA 的默认对也逐条写出来，而同一族在 `xl/worksheets` 那层的
//! `sheetProtection` 写的是 `1` / `0`。拼法按各层自己的实测交，不换算也不合并。
//!
//! 三本容器各记一笔（`cellStyleXfs` 样式那本、`cellXfs` 格式那本、`dxfs` 条件格式那本），
//! 除枚数还交「第几个孩子带着 protection」的下标 —— 那是「哪一格用的格式有锁定位」的入口。
//! `dxfs` 这一本只在 4 份件里在场，而**一位都不写**（63 枚全在两本 `*Xfs` 里）。
//!
//! 只读 OOXML 的表格那一家。`.ods` / `.xls` 的整份输出里查不到这个键：ODF 没有格式层的锁定
//! 属性（`table:style:protected` 那东西不存在，锁只在 `table:table` 一层，已由 `protect.rs` 交），
//! `.xls` 的锁定位在 BIFF 的 `XF` 记录里（那是 #90 那一本按表交的 PROTECT，不是逐格式的位，
//! 本机也没有第二个读者能核对那些位段）。缺键 = 这一族没这一层，不交一本零格的账冒充读过。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PART: &str = "xl/styles.xml";
/// 容器三本，按文件里出现的序逐本交（与标准库读者的 `list(styleSheet)` 同一口径）
const CONTAINERS: [&str; 3] = ["cellStyleXfs", "cellXfs", "dxfs"];
/// 认得的属性；不在这一列里的进 `unknown_attrs` 而不是悄悄丢掉
const KNOWN_ATTRS: [&str; 2] = ["locked", "hidden"];

fn kids(node: Option<&Node>) -> Vec<&Node> {
    match node {
        Some(one) => one
            .children
            .iter()
            .filter(|kid| kid.name != "#text")
            .collect(),
        None => Vec::new(),
    }
}

fn attrs_of(node: &Node) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, (*value).to_string());
    }
    out
}

fn table(map: &BTreeMap<String, u64>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in map.iter() {
        out.insert(key.clone(), json!(value));
    }
    Value::Object(out)
}

fn styles_root(bytes: &[u8]) -> Option<Node> {
    let member = zipread::member(bytes, PART, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// xlsx：`xf` / `dxf` 里那一枚 `protection` 的账，属性值一律按文件写的字面交
pub(crate) fn xlsx(bytes: &[u8]) -> Value {
    let absent = json!({"family": "ooxml", "available": false});
    let doc = match styles_root(bytes) {
        Some(one) => one,
        None => return absent,
    };
    // `parse_str` 交的是伪根 `#doc`，真正的 `<styleSheet>` 在它下面一层
    let sheet = match kids(Some(&doc))
        .into_iter()
        .find(|one| one.local() == "styleSheet")
    {
        Some(one) => one,
        None => return absent,
    };

    let mut containers: Vec<Value> = Vec::new();
    let mut on_formats: Vec<Value> = Vec::new();
    let mut attrs_written: BTreeMap<String, u64> = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut total = 0u64;
    let mut empty_elements = 0u64;

    for book in kids(Some(sheet)) {
        let name = book.local();
        if !CONTAINERS.contains(&name) {
            continue;
        }
        let entries = kids(Some(book));
        let mut mine = 0u64;
        for (pos, entry) in entries.iter().enumerate() {
            // `protection` 是 xf / dxf 的孩子，不是容器的孩子：跳这一层，别把结构猜错
            let mut holders: Vec<&Node> = vec![*entry];
            holders.extend(kids(Some(*entry)));
            for holder in holders {
                if holder.local() != "protection" {
                    continue;
                }
                let had = attrs_of(holder);
                mine += 1;
                total += 1;
                on_formats.push(json!([name, pos as u64]));
                if had.is_empty() {
                    empty_elements += 1;
                }
                for (key, value) in had.iter() {
                    *attrs_written.entry(key.clone()).or_insert(0) += 1;
                    if !KNOWN_ATTRS.contains(&key.as_str()) && !unknown.contains(key) {
                        unknown.push(key.clone());
                    }
                    *values
                        .entry(key.clone())
                        .or_insert_with(BTreeMap::new)
                        .entry(value.clone())
                        .or_insert(0) += 1;
                }
            }
        }
        containers.push(json!({
            "name": name,
            "children_total": entries.len(),
            "protection_elements": mine,
        }));
    }

    let mut value_book = serde_json::Map::new();
    for (key, shelf) in values.iter() {
        value_book.insert(key.clone(), table(shelf));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "part": PART,
        "containers": containers,
        "protection_total": total,
        "empty_elements": empty_elements,
        "attrs_written": table(&attrs_written),
        "values": Value::Object(value_book),
        "on_formats": on_formats,
        "unknown_attrs": unknown,
    })
}
