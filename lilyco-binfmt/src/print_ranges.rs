//! 「这张表打出来是哪几行几列、每页重复哪一行」——表格那一族里这个问题的两处存法
//!
//! OOXML 把它写成 `xl/workbook.xml` 的两条**保留名**（`_xlnm.Print_Area` /
//! `_xlnm.Print_Titles`），归属靠 `localSheetId` 那个序号；ODF 把它写成 `table:table`
//! 自己身上的一条属性，另有一份为了与 Excel 来回而留的 `table:named-range` /
//! `table:named-expression`。两族形状不同到没有可共用的中间表示，所以各交各的，
//! 只在「哪几段范围、按文件写的原样」这一问上对齐。

use crate::office_doc::kept_attrs;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

const AREA: &str = "_xlnm.Print_Area";
const TITLES: &str = "_xlnm.Print_Titles";

/// 一串范围按文件自己用的分隔符摊开：OOXML 用逗号，ODF 用空白
fn split_ranges(text: &str, blank: bool) -> Vec<String> {
    if blank {
        return text.split_whitespace().map(String::from).collect();
    }
    text.split(',')
        .filter(|one| !one.is_empty())
        .map(String::from)
        .collect()
}

/// 属性表里那个「文件写的名字」：`kept_attrs` 留着前缀，所以查名字也按留着的键查
fn kept_string(written: &Value, want: &str) -> Option<String> {
    written
        .get(want)
        .and_then(|had| had.as_str())
        .map(String::from)
}

/// OOXML 那一份：两条保留名与它们靠 `localSheetId` 落到的那张表
pub(crate) fn xlsx(root: &xmlscan::Node, limit: usize) -> Value {
    let mut order: Vec<String> = Vec::new();
    for one in root.descendants("sheet") {
        let written = kept_attrs(one);
        order.push(kept_string(&written, "name").unwrap_or_default());
    }
    let mut entries: Vec<Value> = Vec::new();
    for one in root.descendants("definedName") {
        let written = kept_attrs(one);
        let name = kept_string(&written, "name").unwrap_or_default();
        let index = one
            .attr_local("localSheetId")
            .and_then(|raw| raw.parse::<usize>().ok());
        // 序号越界与没写序号都交 null 的归属：那与「指着一张存在的表」是三件事
        let resolved = index.and_then(|raw| order.get(raw).cloned());
        let text = one.text().trim().to_string();
        entries.push(json!({
            "name": name,
            "reserved": name.starts_with("_xlnm."),
            "local_sheet_id_written": one.attr_local("localSheetId"),
            "local_sheet_id": index,
            "resolved_sheet": resolved,
            "in_range": resolved.is_some(),
            "text": text,
            "ranges": split_ranges(&text, false),
            "quoted_names": text.contains('\''),
            "written": written,
        }));
    }
    let mut by_name = serde_json::Map::new();
    for one in &entries {
        let key = one["name"].as_str().unwrap_or_default().to_string();
        let mine = by_name.get(&key).and_then(Value::as_u64).unwrap_or(0);
        by_name.insert(key, json!(mine + 1));
    }
    let print_entries = entries
        .iter()
        .filter(|one| one["reserved"].as_bool() == Some(true))
        .count();
    let unresolved = entries
        .iter()
        .filter(|one| {
            one["reserved"].as_bool() == Some(true) && one["in_range"].as_bool() != Some(true)
        })
        .count();
    let quoted_entries = entries
        .iter()
        .filter(|one| {
            one["reserved"].as_bool() == Some(true) && one["quoted_names"].as_bool() == Some(true)
        })
        .count();
    let defined_total = entries.len();
    let mut sheets: Vec<Value> = Vec::new();
    for (position, name) in order.iter().enumerate() {
        let mine: Vec<&Value> = entries
            .iter()
            .filter(|one| one["resolved_sheet"].as_str() == Some(name.as_str()))
            .collect();
        let flat = |want: &str| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for one in mine.iter().filter(|one| one["name"] == want) {
                if let Some(raw) = one["ranges"].as_array() {
                    for had in raw {
                        if let Some(one) = had.as_str() {
                            out.push(one.to_string());
                        }
                    }
                }
            }
            out
        };
        sheets.push(json!({
            "index": position,
            "name": name,
            "area_entries": mine.iter().filter(|one| one["name"] == AREA).count(),
            "titles_entries": mine.iter().filter(|one| one["name"] == TITLES).count(),
            "area_ranges": flat(AREA),
            "titles_ranges": flat(TITLES),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "sheets_total": order.len(),
        "defined_total": defined_total,
        "print_entries": print_entries,
        "unresolved": unresolved,
        "quoted_entries": quoted_entries,
        "by_name": by_name,
        "sheets": sheets.into_iter().take(limit).collect::<Vec<Value>>(),
        "entries": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 一条 `table:named-*`：三种地址写法各占一格，全按文件写的原样
fn named_row(one: &xmlscan::Node) -> Value {
    let written = kept_attrs(one);
    let name = one.attr_local("name").unwrap_or_default();
    json!({
        "element": one.name.clone(),
        "name": name,
        "built_in": name.starts_with("Excel_BuiltIn_"),
        "base_cell_address": one.attr_local("base-cell-address"),
        "cell_range_address": one.attr_local("cell-range-address"),
        "expression": one.attr_local("expression"),
        "usable_as": one.attr_local("range-usable-as"),
        "written": written,
    })
}

/// 按文件的先后收那两条 `table:named-*`（两种元素混在同一个容器里，分开两趟就换了顺序）
fn collect_named(node: &xmlscan::Node, into: &mut Vec<Value>) {
    for one in &node.children {
        if one.local() == "named-range" || one.local() == "named-expression" {
            into.push(named_row(one));
            continue;
        }
        collect_named(one, into);
    }
}

/// ODF 那一份：表自己身上的 `table:print-ranges`，加上那一份 Excel 来回用的 named-*
pub(crate) fn ods(bytes: &[u8], limit: usize) -> Value {
    let member = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return json!({"available": false}),
    };
    let root = xmlscan::parse_str(&member.as_text());
    let mut tables: Vec<Value> = Vec::new();
    for one in root.descendants("table") {
        let written = one.attr_local("print-ranges");
        let ranges = written
            .map(|raw| split_ranges(raw, true))
            .unwrap_or_default();
        let range_total = ranges.len();
        tables.push(json!({
            "name": one.attr_local("name"),
            "print_ranges_written": written,
            "ranges": ranges,
            "range_total": range_total,
        }));
    }
    let mut named: Vec<Value> = Vec::new();
    collect_named(&root, &mut named);
    let mut bases: Vec<String> = named
        .iter()
        .filter_map(|one| one["base_cell_address"].as_str().map(String::from))
        .collect();
    bases.sort();
    bases.dedup();
    let mut usable = serde_json::Map::new();
    for one in &named {
        if let Some(raw) = one["usable_as"].as_str() {
            let mine = usable.get(raw).and_then(Value::as_u64).unwrap_or(0);
            usable.insert(raw.to_string(), json!(mine + 1));
        }
    }
    json!({
        "family": "odf",
        "available": true,
        "tables_total": tables.len(),
        "with_print_ranges": tables.iter().filter(|one| !one["print_ranges_written"].is_null()).count(),
        "named_total": named.len(),
        "built_in_total": named.iter().filter(|one| one["built_in"].as_bool() == Some(true)).count(),
        "named_by_element": {
            "range": named.iter().filter(|one| one["element"] == "table:named-range").count(),
            "expression": named.iter().filter(|one| one["element"] == "table:named-expression").count(),
        },
        "distinct_base_addresses": bases.len(),
        "usable_as_written": usable,
        "tables": tables.into_iter().take(limit).collect::<Vec<Value>>(),
        "named": named.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
