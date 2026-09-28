//! 公式那枚 `<f>` 自己写了什么 —— 共享公式的跟随格在文件里**没有公式正文**
//!
//! Excel 把一列里长得一样的公式存成一份共享组：主格写 `<f t="shared" ref="B1:B8" si="0">A1*2</f>`，
//! 跟随格只写 `<f t="shared" si="0"/>` —— 正文是空的，要按 `si` 找回主格再按行平移才知道它是
//! 什么。所以「几个格子有公式」「几个格子写了正文」「几个格子有缓存值」是三个数，
//! 合成一个「几格有公式」就把「文件没写公式正文」这件事实读没了。
//!
//! 实测三份件（`shared.xlsx` 手写的共享组：B 列八格一个组、C 列八格普通公式做对照；
//! `shared-lo.xlsx` 是 LibreOffice 的 xlsx 重写；`shared.ods` 是它的 ods 出口）：
//! 1. `shared.xlsx` 16 枚 `<f>`，其中 8 枚带属性（`t` / `ref` / `si`），**7 枚正文是空的**，
//!    而这 7 枚**全都有缓存值**（`empty_text_with_cached` 7）—— 屏幕上看见的数来自缓存，
//!    不是来自这一格写的公式；
//! 2. 两个生产者对同一件事写得不一样：openpyxl 那一份 `<f>` 一个属性都不写（`book.xlsx`
//!    1 枚带 0 属性），LibreOffice 每条都写 `aca="false"`（实测 16/16），
//!    而它**不用共享组** —— 读进去再导出，八条各写自己的正文（`shared_followers` 0）；
//! 3. ods 那一族没有这一层：公式是格子身上的一个属性，16 条**全带正文**
//!    （`of:=[.A2]*2` 这种逐行平移的写法），空正文 0 条 —— 这一族**不交 `si` / `ref` /
//!    `shared` 那几格**（缺键 = 这一族没有那个位置，不是 0）。
//! 4. `of:` 那个前缀按写的留着（`ooo:` 是另一族写的），交在 `formula_prefixes` 这一格 ——
//!    xlsx 那一族没有「前缀」这回事，所以那个键在那边**整个不出现**；`attr_values` 两家
//!    同一个意思：每个属性值的分布；
//! 5. ODF 里带公式的是**格子自己**（不是格子里的一个元素），所以 `attrs` 交那一格写着的
//!    属性全表，`sheet` 交它所在那张 `table:table` 写的名字（实测 `表一` / `预算表` / `错误`，
//!    **不是**样式名 —— 样式名单独交在 `style` 那一格），而 `cell` 逐条 `null`：
//!    这一族不写格子地址（列可以整个不写、行可以用 `number-rows-repeated` 顶好几行）。
//!
//! 界：这一本只看 `<f>` 元素自己，不展开共享组、也不算「跟随格其实等于什么」——
//! 那是重放公式语义，不在 T0 只读的范围里。老键 `formula` 继续按文件写的正文交（共享跟随格
//! 就是空串），这一本负责说明那个空是**怎么写出来的**。

use crate::opack;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 一枚元素的属性表（局部名，`xmlns` 那类声明不算属性）
fn attrs_of(node: &Node) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    out
}

fn attr_text(node: &Node, want: &str) -> Option<String> {
    node.attr_local(want).map(String::from)
}

fn first_child<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    node.children.iter().find(|one| one.local() == want)
}

fn count_children(node: &Node, want: &str) -> usize {
    node.children
        .iter()
        .filter(|one| one.local() == want)
        .count()
}

/// 元素的正文：xmlscan 把一段直接文本存成 `#text` **孩子**（为了保住顺序），
/// 所以这里必须走 `text()` 而不是读 `direct` —— 只读 `direct` 会把每条公式都读成空。
fn text_of(node: &Node) -> String {
    node.text().trim().to_string()
}

fn bump(values: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    let holder = values
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let bucket = holder.as_object_mut().expect("那一层是对象");
    let slot = bucket.entry(value.to_string()).or_insert_with(|| json!(0));
    let mine = slot.as_u64().unwrap_or(0) + 1;
    *slot = json!(mine);
}

/// OOXML 那一份：每张工作表按部件名排序，格子按文档序
pub(crate) fn xlsx(bytes: &[u8], limit: usize) -> Value {
    let doc = opack::open(bytes);
    let mut names: Vec<String> = doc
        .entries
        .iter()
        .map(|one| one.name.clone())
        .filter(|one| one.starts_with("xl/worksheets/sheet") && one.ends_with(".xml"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut attrs_seen: Vec<String> = Vec::new();
    let mut values: serde_json::Map<String, Value> = serde_json::Map::new();
    for name in names.iter() {
        let member = match zipread::member(bytes, name.as_str(), DEFAULT_MEMBER_CAP) {
            Ok(had) => had,
            Err(_) => continue,
        };
        let root = xmlscan::parse_str(&member.as_text());
        for cell in root.descendants("c") {
            let had = match first_child(cell, "f") {
                Some(one) => one,
                None => continue,
            };
            let table = attrs_of(had);
            let body = text_of(had);
            for (key, _) in had.attrs.iter() {
                if key == "xmlns" || key.starts_with("xmlns:") {
                    continue;
                }
                let local = key.rsplit(':').next().unwrap_or(key);
                if !attrs_seen.iter().any(|one| one == local) {
                    attrs_seen.push(local.to_string());
                }
            }
            for (key, value) in table.iter() {
                if let Some(text) = value.as_str() {
                    bump(&mut values, key, text);
                }
            }
            let cached = first_child(cell, "v").map(|one| text_of(one));
            let shared = table.get("t").and_then(|one| one.as_str()) == Some("shared");
            rows.push(json!({
                "sheet": json!(name.as_str()),
                "cell": attr_text(cell, "r"),
                "attrs": Value::Object(table.clone()),
                "text": body,
                "text_written": !body.is_empty(),
                "shared": shared,
                "si": match table.get("si").and_then(|one| one.as_str()) {
                    Some(text) => json!(text),
                    None => Value::Null,
                },
                "ref_written": match table.get("ref").and_then(|one| one.as_str()) {
                    Some(text) => json!(text),
                    None => Value::Null,
                },
                "cached_written": cached.is_some(),
                "cached": match &cached {
                    Some(text) => json!(text),
                    None => Value::Null,
                },
            }));
        }
    }
    let deps = cell_deps(&rows, limit);
    let written = rows
        .iter()
        .filter(|one| one["text_written"] == json!(true))
        .count();
    let empty = rows.len() - written;
    json!({
        "family": "ooxml",
        "available": true,
        "sheets_seen": names.len(),
        "formula_elems": rows.len(),
        "with_attrs": rows.iter().filter(|one| {
            one["attrs"].as_object().map(|had| !had.is_empty()).unwrap_or(false)
        }).count(),
        "attrs_seen": attrs_seen,
        "attr_values": Value::Object(values),
        "text_written": written,
        "empty_text": empty,
        "empty_text_with_cached": rows.iter().filter(|one| {
            one["text_written"] == json!(false) && one["cached_written"] == json!(true)
        }).count(),
        "shared_elems": rows.iter().filter(|one| one["shared"] == json!(true)).count(),
        "shared_masters": rows.iter().filter(|one| {
            one["shared"] == json!(true) && one["text_written"] == json!(true)
        }).count(),
        "shared_followers": rows.iter().filter(|one| {
            one["shared"] == json!(true) && one["text_written"] == json!(false)
        }).count(),
        "si_written": rows.iter().filter(|one| !one["si"].is_null()).count(),
        "ref_written_elems": rows.iter().filter(|one| !one["ref_written"].is_null()).count(),
        "cached_elems": rows.iter().filter(|one| one["cached_written"] == json!(true)).count(),
        "cell_deps": deps,
        "cells": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 一格地址的形状：1~3 个大写字母 + 1~7 位数字，`$` 可以插在字母前后
fn cell_len(chars: &[char], at: usize) -> Option<usize> {
    let mut i = at;
    if chars.get(i) == Some(&'$') {
        i += 1;
    }
    let letters_start = i;
    while i < chars.len() && chars[i].is_ascii_uppercase() && i - letters_start < 3 {
        i += 1;
    }
    if i == letters_start {
        return None;
    }
    if chars.get(i) == Some(&'$') {
        i += 1;
    }
    let digits_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() && i - digits_start < 7 {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    // 后面紧跟 `(` 的是函数名（`LOG10(` 的 `LOG1` 会撞进这个形状）；
    // 后面还是字母或数字的，说明这个 token 比一格地址长，也不算
    match chars.get(i) {
        Some(next) if next.is_ascii_alphanumeric() || *next == '_' || *next == '(' => None,
        _ => Some(i - at),
    }
}

fn bad_before(prev: Option<char>) -> bool {
    match prev {
        None => false,
        Some(one) => one.is_ascii_alphanumeric() || matches!(one, '_' | '$' | '!' | ':' | '.'),
    }
}

/// OOXML 那一族的引用：`B2`、`$A$1`、`Sheet1!B2`，区间 `B2:B3`
fn ooxml_refs(text: &str) -> Vec<String> {
    // 字符串字面量整段先剔掉：`HYPERLINK("https://example.com/x")` 里的片段不是引用。
    // 剔的时候用空格占位，保持每个字符的下标不变
    let mut clean: Vec<char> = text.chars().collect();
    let mut open = false;
    for slot in clean.iter_mut() {
        if *slot == '"' {
            open = !open;
            *slot = ' ';
        } else if open {
            *slot = ' ';
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut at = 0usize;
    while at < clean.len() {
        if bad_before(if at == 0 { None } else { Some(clean[at - 1]) }) {
            at += 1;
            continue;
        }
        let Some(head) = cell_len(&clean, at) else {
            at += 1;
            continue;
        };
        let mut end = at + head;
        // 区间：冒号后面那一段也可以带 `$`（`$B$2:$B$9`）
        if clean.get(end) == Some(&':') {
            if let Some(tail) = cell_len(&clean, end + 1) {
                end += 1 + tail;
            }
        }
        out.push(clean[at..end].iter().collect());
        at = end;
    }
    out
}

/// 方括号里确实长得像一格才算引用：`[.B2]`、`[.B2:.B3]`、`[.Sheet1.A1:.A2]`
fn odf_ref_body(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let mut at = 0usize;
    if chars.first() == Some(&'.') {
        at = 1;
    }
    loop {
        // 一段「表名.」前缀：以字母或下划线开头、以点收尾
        let start = at;
        if chars.get(at) == Some(&'.') {
            at += 1;
        }
        let name_start = at;
        while at < chars.len()
            && (chars[at].is_ascii_alphanumeric() || matches!(chars[at], '_' | '$' | '.'))
        {
            at += 1;
        }
        if at < chars.len() && chars[at] == '.' {
            if at > name_start {
                at += 1;
                continue;
            }
        }
        at = start;
        break;
    }
    let Some(head) = cell_len(&chars, at) else {
        return false;
    };
    let mut end = at + head;
    if chars.get(end) == Some(&':') {
        let mut next = end + 1;
        if chars.get(next) == Some(&'.') {
            next += 1;
        }
        match cell_len(&chars, next) {
            Some(tail) => end = next + tail,
            None => return false,
        }
    }
    end == chars.len()
}

/// ODF 那一族的引用穿在方括号里；括号里是别的（`#REF`、结构化引用、命名区域）另数一格
fn odf_refs(text: &str) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut other = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    while at < chars.len() {
        if chars[at] != '[' {
            at += 1;
            continue;
        }
        let mut end = at + 1;
        while end < chars.len() && chars[end] != ']' && chars[end] != '[' {
            end += 1;
        }
        if end >= chars.len() || chars[end] != ']' {
            at += 1;
            continue;
        }
        let body: String = chars[at + 1..end].iter().collect();
        if odf_ref_body(&body) {
            out.push(format!("[{}]", body));
        } else {
            other += 1;
        }
        at = end + 1;
    }
    (out, other)
}

/// 「这一格引用了哪些格子」——两族共用一份算术，拼法各按各的
///
/// 不展开区间、不去 `$`、不补表名：交的是文件写的那个 token 原样。「跨表」在这里判不住
/// （OOXML 手里只有部件名，没有表名可对 `Sheet1!` 那个前缀），所以那一格交 null。
fn cell_deps(rows: &[Value], limit: usize) -> Value {
    let mut listed: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut other_brackets = 0usize;
    let (mut rows_n, mut with_refs, mut literal_only) = (0usize, 0usize, 0usize);
    let (mut holders, mut refs_n, mut ranges_n, mut absolute_n) = (0usize, 0usize, 0usize, 0usize);
    let (mut qualified_n, mut self_refs) = (0usize, 0usize);
    for one in rows {
        let text = one["text"].as_str().unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let holder = one["cell"].as_str().unwrap_or("");
        let refs: Vec<String> = if text.starts_with("of:=") || text.contains("[.") {
            let (found, other) = odf_refs(text);
            other_brackets += other;
            found
        } else {
            ooxml_refs(text)
        };
        rows_n += 1;
        with_refs += usize::from(!refs.is_empty());
        literal_only += usize::from(refs.is_empty());
        holders += usize::from(!holder.is_empty());
        refs_n += refs.len();
        let ranges = refs.iter().filter(|raw| raw.contains(':')).count();
        let absolute = refs.iter().filter(|raw| raw.contains('$')).count();
        let qualified = refs
            .iter()
            .filter(|raw| raw.contains('!') || raw.matches('.').count() > 1)
            .count();
        ranges_n += ranges;
        absolute_n += absolute;
        qualified_n += qualified;
        let want = holder.to_ascii_uppercase();
        let hits_self = !holder.is_empty()
            && refs.iter().any(|raw| {
                let cleaned = raw.replace('$', "").replace("[.", "");
                let head = cleaned.split(':').next().unwrap_or("");
                head.to_ascii_uppercase() == want
            });
        self_refs += usize::from(hits_self);
        for raw in &refs {
            if !seen.contains(raw) {
                seen.push(raw.clone());
            }
        }
        listed.push(json!({
            "sheet": one["sheet"],
            "cell": one["cell"],
            "formula": text,
            "refs": refs,
            "ranges": ranges,
            "absolute": absolute,
            "qualified": qualified,
            "self_ref": hits_self,
        }));
    }
    json!({
        "available": true,
        "rows": rows_n,
        "rows_with_refs": with_refs,
        "rows_literal_only": literal_only,
        "holders_known": holders,
        "refs_total": refs_n,
        "distinct_refs": seen.len(),
        "ranges_total": ranges_n,
        "absolute_total": absolute_n,
        "qualified_total": qualified_n,
        "brackets_other": other_brackets,
        "self_refs": self_refs,
        "cross_sheet": null,
        "total": rows_n,
        "listed": listed.len().min(limit),
        "cut": rows_n > limit,
        "rows_list": listed.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：公式是格子身上的属性，每条都带正文；共享组那一层没有位置
///
/// 带公式的是**格子自己**，所以 `attrs` 交的是那一格写着的属性全表（`sheet` 那一栏因此
/// 不是样式名 —— 它是这一格所在那张 `table:table` 写的名字）。`cell` 逐条交 null：
/// 这一族不写格子地址（列可以整个不写、行可以用 `number-rows-repeated` 顶好几行）。
pub(crate) fn ods(bytes: &[u8], limit: usize) -> Value {
    let member = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
        Ok(had) => had,
        Err(_) => {
            return json!({"family": "odf", "available": false});
        }
    };
    let root = xmlscan::parse_str(&member.as_text());
    let mut cells: Vec<(&Node, Option<&str>)> = Vec::new();
    collect_cells(&root, None, &mut cells);
    let mut rows: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut values: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut prefixes: serde_json::Map<String, Value> = serde_json::Map::new();
    for (cell, sheet) in cells {
        let table = attrs_of(cell);
        let text = match table.get("formula").and_then(|one| one.as_str()) {
            Some(one) => one.to_string(),
            None => continue,
        };
        for (key, _) in cell.attrs.iter() {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            let local = key.rsplit(':').next().unwrap_or(key);
            if !seen.iter().any(|one| one == local) {
                seen.push(local.to_string());
            }
        }
        for (key, value) in table.iter() {
            if let Some(text) = value.as_str() {
                bump(&mut values, key, text);
            }
        }
        let head = match text.find(':') {
            Some(at) => text[..at].to_string(),
            None => String::new(),
        };
        bump(&mut prefixes, "formula-prefix", &head);
        let cached = attr_text(cell, "value");
        rows.push(json!({
            "sheet": match sheet {
                Some(one) => json!(one),
                None => Value::Null,
            },
            "cell": Value::Null,
            "style": match table.get("style-name").and_then(|one| one.as_str()) {
                Some(one) => json!(one),
                None => Value::Null,
            },
            "attrs": Value::Object(table.clone()),
            "text": text,
            "text_written": !text.is_empty(),
            "cached_written": cached.is_some(),
            "cached": match &cached {
                Some(one) => json!(one),
                None => Value::Null,
            },
            "paragraphs": count_children(cell, "p"),
        }));
    }
    let deps = cell_deps(&rows, limit);
    let written = rows
        .iter()
        .filter(|one| one["text_written"] == json!(true))
        .count();
    json!({
        "family": "odf",
        "available": true,
        "tables_seen": root.descendants("table").len(),
        "formula_elems": rows.len(),
        "with_attrs": rows.iter().filter(|one| {
            one["attrs"].as_object().map(|had| !had.is_empty()).unwrap_or(false)
        }).count(),
        "attrs_seen": seen,
        "attr_values": Value::Object(values),
        "formula_prefixes": match prefixes.remove("formula-prefix") {
            Some(had) => had,
            None => Value::Object(serde_json::Map::new()),
        },
        "text_written": written,
        "empty_text": rows.len() - written,
        "empty_text_with_cached": rows.iter().filter(|one| {
            one["text_written"] == json!(false) && one["cached_written"] == json!(true)
        }).count(),
        "cached_elems": rows.iter().filter(|one| one["cached_written"] == json!(true)).count(),
        "cell_deps": deps,
        "cells": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 文档序收集两种格子（跨名字的一次遍历 —— 与第二读者那条 `iter()` 同一条走法），
/// 并把外面那层 `table:table` 写的名字一起带下来（ODF 里「哪张表」就是「哪个 sheet」）
fn collect_cells<'a>(
    node: &'a Node,
    sheet: Option<&'a str>,
    out: &mut Vec<(&'a Node, Option<&'a str>)>,
) {
    for one in &node.children {
        let here = if one.local() == "table" {
            one.attr_local("name")
        } else {
            sheet
        };
        if matches!(one.local(), "table-cell" | "covered-table-cell") {
            out.push((one, here));
        }
        collect_cells(one, here, out);
    }
}
