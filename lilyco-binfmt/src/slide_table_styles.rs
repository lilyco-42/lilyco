//! 演示稿那张表的样式指针（`table_style_refs`，只在 .pptx 交）
//!
//! 页上那张表的格子、行高、合并由 `table_grid` 那一本交，而「表头算不算一行、隔行要不要底纹、
//! 这套长相指点给哪个表格样式」写在**表自己身上**：`a:tbl/a:tblPr` 的六枚开关
//! （`firstRow` / `lastRow` / `firstCol` / `lastCol` / `bandRow` / `bandCol`）加一枚
//! `a:tableStyleId` 子元素。包级另有一本 `ppt/tableStyles.xml`：根是 `a:tblStyleLst`，
//! 它只写一枚 `@def`，而**一条 `a:tableStyle` 声明都没有**（本仓 18 份与本机 102 份第三方件全是这样）。
//!
//! 量到的三件事，都按文件的原样交：
//! 1. **`a:tblPr` 有两种缺法**：第三方那 32 张表全部写成一枚**自闭合空壳** `<a:tblPr/>`
//!    （零属性、零子元素），自产件 22 张里 13 张同样空壳、9 张写满 `firstRow="1" bandRow="1"`
//!    与 `a:tableStyleId`。空壳不是「没这一层」，所以 `pr_present` / `pr_empty_shell` / `pr_missing`
//!    分三格数。
//! 2. **指针指不到包里的东西**：`a:tableStyleId` 与 `a:tblStyleLst/@def` 都是同一个
//!    `{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}`，而那本清单声明的样式条数为 **0** ——
//!    于是 `style_ids_declared` 与 `style_ids_resolved` 都交 0，另给 `style_ids_same_as_default`
//!    说「它等于包默认」这一件真事。样式长相住在应用自己的画廊里，不在包里，
//!    这是格式的形状，不是解析没走到。
//! 3. 开关的值按写的字符串交（`"1"` 与 `"0"`），不折成布尔 —— 两家生产者连写不写都不一样。

use crate::office_doc::local_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

/// 没有幻灯片部件时的账：键全给、条数全 0（0 是「看过了没有」）
fn empty() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "list_part": false,
        "list_root": Value::Null,
        "list_def": Value::Null,
        "list_entries_total": 0,
        "tables_total": 0,
        "pr_present": 0,
        "pr_empty_shell": 0,
        "pr_missing": 0,
        "with_switches": 0,
        "with_style_id": 0,
        "switch_names": {},
        "switch_values": {},
        "child_names": {},
        "style_id_refs": {},
        "style_ids_declared": 0,
        "style_ids_resolved": 0,
        "style_ids_same_as_default": 0,
        "notes_tables": 0,
        "unread_parts": 0,
        "listed": 0,
        "cut": false,
        "entries": [],
    })
}

/// 包里那本清单声明了哪些样式 id（本仓与第三方全为 0 条，所以指针一律解不到）
fn declared_ids(list: Option<&Node>) -> Vec<String> {
    let Some(root) = list else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for one in root.descendants("tableStyle") {
        if let Some(raw) = one.attr_local("styleId") {
            out.push(raw.to_string());
        }
    }
    out
}

pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| {
            (one.starts_with("ppt/slides/slide") || one.starts_with("ppt/notesSlides/"))
                && one.ends_with(".xml")
        })
        .collect();
    if names.is_empty() {
        return empty();
    }
    names.sort();
    let list = match zipread::member(bytes, "ppt/tableStyles.xml", DEFAULT_MEMBER_CAP).ok() {
        Some(member) => Some(xmlscan::parse_str(&member.as_text())),
        None => None,
    };
    let declared = declared_ids(list.as_ref());
    let package_default = list
        .as_ref()
        .and_then(|root| root.attr_local("def"))
        .map(String::from);
    let mut switch_names: BTreeMap<String, u64> = BTreeMap::new();
    let mut switch_values: BTreeMap<String, u64> = BTreeMap::new();
    let mut child_names: BTreeMap<String, u64> = BTreeMap::new();
    let mut style_refs: BTreeMap<String, u64> = BTreeMap::new();
    let mut rows: Vec<Value> = Vec::new();
    let mut tables_total = 0usize;
    let mut pr_present = 0usize;
    let mut pr_empty = 0usize;
    let mut pr_missing = 0usize;
    let mut with_switches = 0usize;
    let mut with_id = 0usize;
    let mut same_default = 0usize;
    let mut resolved_total = 0usize;
    let mut notes_tables = 0usize;
    let mut unread = 0usize;
    for name in &names {
        let member = match zipread::member(bytes, name, DEFAULT_MEMBER_CAP).ok() {
            Some(one) => one,
            None => {
                unread += 1;
                continue;
            }
        };
        let root = xmlscan::parse_str(&member.as_text());
        for table in root.descendants("tbl") {
            tables_total += 1;
            if name.starts_with("ppt/notesSlides/") {
                notes_tables += 1;
            }
            let holder = match table.children.iter().find(|kid| kid.local() == "tblPr") {
                Some(one) => one,
                None => {
                    pr_missing += 1;
                    continue;
                }
            };
            pr_present += 1;
            let written = local_attrs(holder);
            let bare = match written.as_object() {
                Some(map) => map.is_empty(),
                None => true,
            };
            let kids: Vec<String> = holder
                .children
                .iter()
                .map(|kid| kid.local().to_string())
                .collect();
            let shell = bare && kids.is_empty();
            if shell {
                pr_empty += 1;
            }
            let mut switches: BTreeMap<String, String> = BTreeMap::new();
            for (key, value) in &holder.attrs {
                if key == "xmlns" || key.starts_with("xmlns:") {
                    continue;
                }
                let local = key.rsplit(':').next().unwrap_or(key).to_string();
                bump(&mut switch_names, &local);
                bump(&mut switch_values, &format!("{}={}", local, value));
                switches.insert(local, value.clone());
            }
            if !switches.is_empty() {
                with_switches += 1;
            }
            let mut style_id: Option<String> = None;
            for kid in &holder.children {
                bump(&mut child_names, kid.local());
                if kid.local() == "tableStyleId" && style_id.is_none() {
                    let raw = kid.text();
                    let trimmed = raw.trim();
                    if !trimmed.is_empty() {
                        style_id = Some(trimmed.to_string());
                    }
                }
            }
            let mut declared_hit = false;
            let mut is_default = false;
            if let Some(raw) = &style_id {
                with_id += 1;
                bump(&mut style_refs, raw);
                declared_hit = declared.iter().any(|had| had == raw);
                if declared_hit {
                    resolved_total += 1;
                }
                is_default = package_default.as_deref() == Some(raw.as_str());
                if is_default {
                    same_default += 1;
                }
            }
            rows.push(json!({
                "part": name,
                "written": written,
                "switches": switches,
                "children": kids,
                "empty_shell": shell,
                "style_id": style_id,
                "style_declared": declared_hit,
                "same_as_package_default": is_default,
            }));
        }
    }
    let listed = rows.len().min(limit);
    json!({
        "family": "ooxml",
        "available": true,
        // 包级那本清单：根元素名与它自己写的 @def，声明条数按看到的交
        "list_part": list.is_some(),
        "list_root": list.as_ref().map(|root| root.local().to_string()),
        "list_def": package_default,
        "list_entries_total": declared.len(),
        "tables_total": tables_total,
        "pr_present": pr_present,
        "pr_empty_shell": pr_empty,
        "pr_missing": pr_missing,
        "with_switches": with_switches,
        "with_style_id": with_id,
        "switch_names": switch_names,
        "switch_values": switch_values,
        "child_names": child_names,
        "style_id_refs": style_refs,
        "style_ids_declared": declared.len(),
        "style_ids_resolved": resolved_total,
        "style_ids_same_as_default": same_default,
        "notes_tables": notes_tables,
        "unread_parts": unread,
        "listed": listed,
        "cut": rows.len() > listed,
        "entries": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
