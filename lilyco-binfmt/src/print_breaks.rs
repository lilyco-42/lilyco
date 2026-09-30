//! 「这张表上手动插了哪几道分页符」——表格那一族的两种存法
//!
//! 量到的形状（openpyxl 手写一份，LibreOffice 把同一份内容重存成 `.xlsx` 与 `.ods`）：
//!
//! * OOXML 写在**工作表部件**上：`<rowBreaks count="3" manualBreakCount="3">` 里三条
//!   `<brk id="4" min="0" max="16383" man="1"/>`。两个声明值（`count` 与 `manualBreakCount`）
//!   都在，而这一份真把同一道 `id=9` 写了两遍 —— 所以「声明几条」「找到几条」「几个不同的号」
//!   是三个数，这里逐条交原样，不替文件去重。LibreOffice 重写同一份时把 `man` 换成 `true`、
//!   把列那一段的 `max` 从 16383 换成 65535，并且**只剩两条**（去掉重复的那条）：
//!   转格式不是无损的，两家各交各的。
//! * ODF 没有「分页符」这种元素：断页写在**行/列的自动样式**上
//!   （`<style:table-row-properties fo:break-before="page"/>` 与
//!   `<style:table-column-properties fo:break-before="page"/>`，部件是 `content.xml`），
//!   所以「这一行断不下一页」要跳一跳才知道，而 `table:style-name` 没写的行根本无从可问 ——
//!   那一格不进账（`elements_with_style` 数的就是说了话的那些）。同族还把 `auto` 明写在其余
//!   每一行上：`auto`（不断页）与「整条属性没写」是两件事，所以账里留着值本身。
//!
//! `.xls` 那一族的分页符在 BIFF 的 0x001B / 0x001A 记录里，本机没有第二个读者能核对那些
//! 16 位行号数组，所以该族**不交这个键**。

use crate::office_doc::{kept_attrs, local_attrs};
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// 一条轴上的分页符：声明值按原样字符串交，实数的是元素条数，不同号是第三个数
fn axis_ledger(found: Vec<Value>, written: Option<Value>) -> Value {
    let Some(book) = written else {
        return json!({
            "present": false,
            "written": Value::Null,
            "declared": Value::Null,
            "declared_manual": Value::Null,
            "found": 0,
            "distinct_ids": Value::Null,
            "whole": Value::Null,
            "man_values": {},
            "breaks": [],
        });
    };
    let mut ids: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut man_values: BTreeMap<String, u64> = BTreeMap::new();
    for one in &found {
        if let Value::String(raw) = &one["id"] {
            ids.push(raw.clone());
            if !seen.contains(&raw.as_str()) {
                seen.push(raw);
            }
        }
        let raw = one["written"]["man"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        *man_values.entry(raw).or_insert(0) += 1;
    }
    let declared = book.get("count").cloned().unwrap_or(Value::Null);
    // 声明与实数只在这一族里可问：`count` 写了才判得动，没写交 null 而不是 false
    let whole = match declared.as_str().and_then(|raw| raw.parse::<usize>().ok()) {
        Some(raw) => json!(raw == found.len()),
        None => Value::Null,
    };
    json!({
        "present": true,
        "written": book,
        "declared": declared,
        "declared_manual": book.get("manualBreakCount").cloned().unwrap_or(Value::Null),
        "found": found.len(),
        "distinct_ids": json!(seen.len()),
        "whole": whole,
        "man_values": man_values,
        "breaks": found,
    })
}

/// 一条轴上那些 `<brk>`：属性按局部名交（这一族的前缀不是契约的一部分）
fn brk_rows(holder: Option<&xmlscan::Node>, limit: usize) -> Vec<Value> {
    let Some(found) = holder else {
        return Vec::new();
    };
    found
        .all("brk")
        .into_iter()
        .take(limit)
        .map(|one| {
            let mine = local_attrs(one);
            json!({
                "id": mine.get("id").cloned().unwrap_or(Value::Null),
                "written": mine,
            })
        })
        .collect()
}

/// OOXML 那一份：按工作表部件归账，键是部件自己的名字（两族这份账都到簿级）
pub(crate) fn xlsx(bytes: &[u8], limit: usize) -> Value {
    let mut paths: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| {
            one.starts_with("xl/worksheets/") && one.ends_with(".xml") && !one.contains("/_rels/")
        })
        .collect();
    paths.sort();
    let mut out = Map::new();
    let mut with_rows = 0usize;
    let mut with_cols = 0usize;
    let mut row_total = 0usize;
    let mut col_total = 0usize;
    for path in paths {
        let Some(member) = zipread::member(bytes, &path, DEFAULT_MEMBER_CAP).ok() else {
            continue;
        };
        let root = xmlscan::parse_str(&member.as_text());
        let rows = brk_rows(root.descendants("rowBreaks").first().copied(), limit);
        let cols = brk_rows(root.descendants("colBreaks").first().copied(), limit);
        if root.descendants("rowBreaks").first().is_some() {
            with_rows += 1;
        }
        if root.descendants("colBreaks").first().is_some() {
            with_cols += 1;
        }
        row_total += rows.len();
        col_total += cols.len();
        let local = path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".xml")
            .to_string();
        out.insert(
            local,
            json!({
                "part": path,
                "rows": axis_ledger(
                    rows,
                    root.descendants("rowBreaks").first().copied().map(local_attrs),
                ),
                "columns": axis_ledger(
                    cols,
                    root.descendants("colBreaks").first().copied().map(local_attrs),
                ),
            }),
        );
    }
    json!({
        "family": "ooxml",
        "available": true,
        "sheets_with_ledger": out.len(),
        "with_rows": with_rows,
        "with_columns": with_cols,
        "row_break_total": row_total,
        "column_break_total": col_total,
        "entries": out,
    })
}

/// 这一族里说了话的行/列：断页在它们各自的自动样式上，一跳不到的记进 `style_missing`
fn spoken_rows(
    holder: &xmlscan::Node,
    styles: &BTreeMap<String, String>,
    want: &str,
    repeated_attr: &str,
    limit: usize,
) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut on_page = 0usize;
    let mut missing = 0usize;
    for one in holder.descendants(want) {
        let Some(name) = one.attr_local("style-name") else {
            continue;
        };
        let had = styles.get(name);
        let value = match had {
            Some(raw) => json!(raw),
            None => {
                missing += 1;
                Value::Null
            }
        };
        if had.map(String::as_str) == Some("page") {
            on_page += 1;
        }
        let written = kept_attrs(one);
        rows.push(json!({
            "name": one.attr_local("name"),
            "style_name": name,
            "break_before": value,
            "repeated": one.attr_local(repeated_attr),
            "written": written,
        }));
    }
    let spoken = rows.len();
    json!({
        "elements_with_style": spoken,
        "resolved": spoken - missing,
        "style_missing": missing,
        "on_page": on_page,
        "list": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 样式名 → 那条属性写的值：只收**写了这个属性**的样式（没说 = 不在表里）
fn style_breaks(root: &xmlscan::Node, want: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for style in root.descendants("style") {
        let Some(name) = style.attr_local("name") else {
            continue;
        };
        for one in style.descendants(want) {
            if let Some(raw) = one.attr_local("break-before") {
                if !out.contains_key(name) {
                    out.insert(name.to_string(), raw.to_string());
                }
            }
        }
    }
    out
}

/// ODF 那一份：按表归账，每表两条轴各一跳
pub(crate) fn ods(bytes: &[u8], limit: usize) -> Value {
    let holder = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return json!({"available": false}),
    };
    let root = xmlscan::parse_str(&holder.as_text());
    let row_styles = style_breaks(&root, "table-row-properties");
    let col_styles = style_breaks(&root, "table-column-properties");
    let mut tables: Vec<Value> = Vec::new();
    for table in root.descendants("table") {
        let name = table.attr_local("name").unwrap_or_default().to_string();
        tables.push(json!({
            "sheet": name,
            "rows": spoken_rows(table, &row_styles, "table-row", "number-rows-repeated", limit),
            "columns": spoken_rows(table, &col_styles, "table-column",
                                   "number-columns-repeated", limit),
        }));
    }
    let row_total: usize = tables
        .iter()
        .filter_map(|one| one["rows"]["on_page"].as_u64())
        .map(|one| one as usize)
        .sum();
    let col_total: usize = tables
        .iter()
        .filter_map(|one| one["columns"]["on_page"].as_u64())
        .map(|one| one as usize)
        .sum();
    json!({
        "family": "odf",
        "available": true,
        "tables_total": tables.len(),
        "rows_on_page": row_total,
        "columns_on_page": col_total,
        "tables": tables.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
