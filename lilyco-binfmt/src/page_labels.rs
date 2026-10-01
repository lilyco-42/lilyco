//! PDF 目录里那一棵 `/PageLabels` 数字树：这一族管「第几张纸显示成几号」。
//!
//! 与页树自己的序号（`pages`）是两件事：区间写作 `[ 起点 字典 起点 字典 … ]` 的摊平
//! 数组，字典里 `/S` 是样式（规范七种：D R r A a H h，可选）、`/St` 是这一段的起始号
//! （不写就是 1）、`/P` 是前缀、`/PgNum` 说这一段的号从**哪一页**起算 —— 它可以与区间
//! 键号不相等，那正是「把键号当基准」与「照规范算」两种结果分开的那一格。
//!
//! 本机没有这一族的生产者：LibreOffice 会把 docx 分节里的 `w:pgNumType` 搬进 odt 的
//! `style:num-format`，但它导出的 PDF 目录里连 `/PageLabels` 这个键都不写（量过两轮）。
//! 凭据由 pikepdf（内嵌 qpdf）写出，第三读者 pypdf 6.19 逐页核对过 —— 它在三处与规范
//! 不同：完全不读 `/PgNum`、样式表里没有 `H`/`h`、`/S` 写成串时把整棵树的号退回物理页号。
//! 这份账按文件写出的形状交回，不替文件改写法，也不算它算不出的号。

use crate::pdf::Pdf;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 规范列的六个空白字符（与 `pdf::is_space` 同一套，这里自己写一份免得多一个 pub 口）
fn is_space(one: u8) -> bool {
    matches!(one, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0x00)
}

fn skip(body: &[u8], mut at: usize) -> usize {
    while at < body.len() && is_space(body[at]) {
        at += 1;
    }
    at
}

/// 对象正文开头那个换行不算内容：qpdf 写出的对象就是 `\n<< … >>`，
/// 按前两个字节判形状会把真字典读成「不像字典」
fn head(body: &[u8]) -> &[u8] {
    &body[skip(body, 0)..]
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < from + needle.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|one| one == needle)
        .map(|one| one + from)
}

fn is_name_char(one: u8) -> bool {
    one.is_ascii_alphanumeric() || matches!(one, b'.' | b'_' | b'+' | b'-')
}

// 这一族靠 dict_pairs 交替认键（`/P` 与 `/PgNum` 各自整段比对），所以不单独找键位。

fn dict_span(body: &[u8], at: usize) -> Option<Vec<u8>> {
    if body.get(at..at + 2) != Some(&b"<<"[..]) {
        return None;
    }
    let mut depth = 0usize;
    let mut i = at;
    while i + 1 < body.len() {
        if body[i..i + 2] == *b"<<" {
            depth += 1;
            i += 2;
            continue;
        }
        if body[i..i + 2] == *b">>" {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(body[at..i].to_vec());
            }
            continue;
        }
        if body[i] == b'(' {
            let (_raw, next) = crate::pdf::literal(body, i + 1);
            i = next;
            continue;
        }
        i += 1;
    }
    None
}

fn array_span(body: &[u8], at: usize) -> Option<Vec<u8>> {
    if body.get(at) != Some(&b'[') {
        return None;
    }
    let mut depth = 0usize;
    let mut i = at;
    while i < body.len() {
        let ch = body[i];
        if ch == b'(' {
            let (_raw, next) = crate::pdf::literal(body, i + 1);
            i = next;
            continue;
        }
        if ch == b'[' {
            depth += 1;
        } else if ch == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(body[at..i + 1].to_vec());
            }
        }
        i += 1;
    }
    None
}

/// 从 `at` 起取**一个**值的原始字节：字典、数组、串整段跳，间接引用算一个值
fn value_token(body: &[u8], at: usize) -> (Vec<u8>, usize) {
    if body.get(at..at + 2) == Some(&b"<<"[..]) {
        return match dict_span(body, at) {
            Some(one) => (one.clone(), at + one.len()),
            None => (Vec::new(), at + 2),
        };
    }
    if body.get(at) == Some(&b'[') {
        return match array_span(body, at) {
            Some(one) => (one.clone(), at + one.len()),
            None => (Vec::new(), at + 1),
        };
    }
    if body.get(at) == Some(&b'(') {
        let (_raw, next) = crate::pdf::literal(body, at + 1);
        return (body[at..next.min(body.len())].to_vec(), next);
    }
    if body.get(at) == Some(&b'<') {
        return match find(body, b">", at + 1) {
            Some(one) => (body[at..one + 1].to_vec(), one + 1),
            None => (Vec::new(), at + 1),
        };
    }
    if body.get(at) == Some(&b'/') {
        let mut i = at + 1;
        while i < body.len() && is_name_char(body[i]) {
            i += 1;
        }
        return (body[at..i].to_vec(), i);
    }
    let mut i = at;
    while i < body.len()
        && !is_space(body[i])
        && !matches!(body[i], b'[' | b']' | b'(' | b'/' | b'<' | b'>')
    {
        i += 1;
    }
    let one = body[at..i].to_vec();
    // 引用是三枚记号占一个值：`/PageLabels 4 0 R` 里那个值是 `4 0 R`，不是 `4`
    let rest = skip(body, i);
    let mut j = rest;
    let mut digits = 0usize;
    while j < body.len() && body[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    let after = skip(body, j);
    if digits > 0
        && body.get(after..after + 1) == Some(&b'R')
        && matches!(
            body.get(after + 1),
            None | Some(&0x20) | Some(&b'\t') | Some(&b'\r') | Some(&b'\n')
        )
        && one
            .iter()
            .all(|one| one.is_ascii_digit() || *one == b'-' || *one == b'+')
    {
        return (body[at..after + 1].to_vec(), after + 1);
    }
    (one, i)
}

/// 数组里的元素按写的顺序切出来（`raw` 含首尾方括号）
fn array_items(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let stop = raw.len().saturating_sub(1);
    let mut i = 1usize;
    while i < stop {
        i = skip(raw, i);
        if i >= stop {
            break;
        }
        let (one, next) = value_token(raw, i);
        if one.is_empty() && raw[i] != b'(' {
            i += 1;
            continue;
        }
        out.push(one);
        i = next;
    }
    out
}

/// 字典**自己那一层**的 (键名, 值的原始字节)：认键必须连着把值整段跳过去 ——
/// `<</S /r>>` 里的 `/r` 是一个名字值，把它当键就凭空多出一枚不存在的键
fn dict_pairs(body: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        if body.get(i..i + 2) == Some(&b"<<"[..]) || body.get(i..i + 2) == Some(&b">>"[..]) {
            i += 2;
            continue;
        }
        if body[i] != b'/' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < body.len() && is_name_char(body[j]) {
            j += 1;
        }
        let name = String::from_utf8_lossy(&body[i + 1..j]).into_owned();
        let (raw, next) = value_token(body, skip(body, j));
        out.push((name, raw));
        i = next.max(j + 1);
    }
    out
}

fn pick<'a>(pairs: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    pairs
        .iter()
        .find(|one| one.0 == name)
        .map(|one| one.1.as_slice())
}

/// 值的形状与原文。`None` 只有一种意思：**这个键整个没写** —— 那与「写了个空值」是两件事
fn value_form(raw: Option<&[u8]>) -> (Option<&'static str>, Option<String>) {
    let Some(one) = raw else {
        return (None, None);
    };
    if one.is_empty() {
        return (Some("empty"), None);
    }
    if one[0] == b'/' {
        return (
            Some("name"),
            Some(String::from_utf8_lossy(&one[1..]).into_owned()),
        );
    }
    if one[0] == b'(' {
        let (raw, _next) = crate::pdf::literal(one, 1);
        return (Some("string"), Some(crate::pdf::decode_text(&raw)));
    }
    if head(one).get(..2) == Some(&b"<<"[..]) {
        return (Some("dict"), None);
    }
    if head(one).first() == Some(&b'[') {
        return (Some("array"), None);
    }
    if one[0] == b'<' {
        let stop = find(one, b">", 1).unwrap_or(one.len());
        let inner = &one[1..stop];
        let packed: Vec<u8> = inner
            .iter()
            .copied()
            .filter(|one| !is_space(*one))
            .collect();
        if packed.len() % 2 != 0 || !packed.iter().all(|one| one.is_ascii_hexdigit()) {
            return (Some("hexstring"), Some(String::new()));
        }
        let mut bytes: Vec<u8> = Vec::new();
        for pair in packed.chunks(2) {
            let hi = (pair[0] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (pair[1] as char).to_digit(16).unwrap_or(0) as u8;
            bytes.push(hi * 16 + lo);
        }
        return (Some("hexstring"), Some(crate::pdf::decode_text(&bytes)));
    }
    if body_is_int(one) {
        return (
            Some("number"),
            Some(String::from_utf8_lossy(one).into_owned()),
        );
    }
    (
        Some("other"),
        Some(String::from_utf8_lossy(one).into_owned()),
    )
}

fn body_is_int(one: &[u8]) -> bool {
    let mut i = 0usize;
    if one.first() == Some(&b'-') || one.first() == Some(&b'+') {
        i = 1;
    }
    i < one.len() && one[i..].iter().all(|one| one.is_ascii_digit())
}

fn parse_int(one: &[u8]) -> Option<i64> {
    let text = String::from_utf8_lossy(one).into_owned();
    text.trim().parse::<i64>().ok()
}

/// 罗马数字：非正数交回空串（规范里这一族的取值本来就是正整数，不算一个号）
fn roman(value: i64, upper: bool) -> String {
    if value <= 0 || value > 3999 {
        return String::new();
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    let mut rest = value;
    for (amount, glyph) in TABLE {
        while rest >= amount {
            out.push_str(glyph);
            rest -= amount;
        }
    }
    if upper {
        out
    } else {
        out.to_lowercase()
    }
}

/// 字母编号：A..Z 之后是 AA..ZZ（26 进制但没有零位）
fn letters(value: i64, upper: bool) -> String {
    if value <= 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut rest = value;
    while rest > 0 {
        let rem = (rest - 1) % 26;
        rest = (rest - 1) / 26;
        out.insert(0, (b'A' + rem as u8) as char);
    }
    if upper {
        out
    } else {
        out.to_lowercase()
    }
}

fn hexed(value: i64, upper: bool) -> String {
    if value <= 0 {
        return String::new();
    }
    if upper {
        format!("{value:X}")
    } else {
        format!("{value:x}")
    }
}

/// 样式认不出来时交回 None，不编一个号；没有 `/S` 时只有前缀（规范里 /S 是可选的）
fn label_of(st: i64, style: Option<&str>, prefix: &str) -> Option<String> {
    let tail = match style {
        None => return Some(prefix.to_string()),
        Some("D") => st.to_string(),
        Some("R") => roman(st, true),
        Some("r") => roman(st, false),
        Some("A") => letters(st, true),
        Some("a") => letters(st, false),
        Some("H") => hexed(st, true),
        Some("h") => hexed(st, false),
        Some(_unknown) => return None,
    };
    Some(format!("{prefix}{tail}"))
}

struct Node {
    depth: usize,
    limits: Value,
    items: Vec<Vec<u8>>,
    keys: Vec<String>,
}

/// 数树的节点表，按「先父后子、兄弟按写出的顺序」摊平。规范允许两种存法：一层里直接
/// 写 `/Nums`，或写 `/Kids` 把区间分给几只孩子、每只自己用 `/Limits` 说盖住哪一段；
/// 孩子本身可以是内联字典也可以是间接引用，所以两种都认。走到第八层就停。
fn number_tree_nodes(doc: &Pdf, span: &[u8]) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    let mut queue: Vec<(Vec<u8>, usize, Value)> = vec![(span.to_vec(), 0usize, Value::Null)];
    let mut steps = 0usize;
    while !queue.is_empty() {
        let (one, depth, limits) = queue.remove(0);
        steps += 1;
        if steps > 64 {
            break;
        }
        let pairs = dict_pairs(&one);
        let nums = pick(&pairs, "Nums");
        let items = match nums {
            Some(raw) if head(raw).first() == Some(&b'[') => array_items(raw),
            _ => Vec::new(),
        };
        let keys: Vec<String> = pairs.iter().map(|one| one.0.clone()).collect();
        out.push(Node {
            depth,
            limits,
            items,
            keys,
        });
        if depth >= 8 {
            continue;
        }
        let kids = match pick(&pairs, "Kids") {
            Some(raw) if head(raw).first() == Some(&b'[') => array_items(head(raw)),
            _ => continue,
        };
        for el in kids {
            let got = if head(&el).get(..2) == Some(&b"<<"[..]) {
                head(&el).to_vec()
            } else {
                match first_ref(&el) {
                    Some(id) => match doc.object(id) {
                        Some(held) => head(&held.dict).to_vec(),
                        None => continue,
                    },
                    None => continue,
                }
            };
            if head(&got).get(..2) != Some(&b"<<"[..]) {
                continue;
            }
            let kid_pairs = dict_pairs(&got);
            let bounds = match pick(&kid_pairs, "Limits") {
                Some(raw) if head(raw).first() == Some(&b'[') => {
                    let nums: Vec<i64> = array_items(head(raw))
                        .iter()
                        .filter_map(|one| parse_int(one))
                        .collect();
                    json!(nums)
                }
                _ => Value::Null,
            };
            queue.push((got, depth + 1, bounds));
        }
    }
    out
}

/// `4 0 R` 里那个号；写法不对交回 None（不猜一个号）
fn first_ref(one: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(one).into_owned();
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() == 3 && parts[2] == "R" {
        return parts[0].parse::<u64>().ok();
    }
    None
}

/// 引用指向那个对象的字典部分；对象读不到就交回空字节（不替文件猜一本字典）
fn refer_dict(doc: &Pdf, id: u64) -> Vec<u8> {
    match doc.object(id) {
        Some(one) => head(&one.dict).to_vec(),
        None => Vec::new(),
    }
}

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

/// 行号与节点号：`serde_json::Value` 没有 `as_usize`，按无符号取再转
fn index_of(one: &Value, key: &str) -> usize {
    one.get(key).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn empty(page_count: usize, catalogs: usize, keys_total: u64) -> Value {
    json!({
        "present": false,
        "catalogs_total": catalogs,
        "label_keys_total": keys_total,
        "written_via": null,
        "target_object": null,
        "tree_shape": null,
        "keys_written": {},
        "nodes_total": 0,
        "odd_nodes": 0,
        "nums_present": false,
        "nums_length": 0,
        "nums_length_total": 0,
        "pairs_total": 0,
        "range_starts": [],
        "keys_ascending": true,
        "styles_written": {},
        "style_forms": {},
        "value_forms": {},
        "st_missing": 0,
        "prefixes_found": 0,
        "ranges_without_style": 0,
        "unknown_styles": [],
        "beyond_pages": [],
        "page_count": page_count,
        "uncovered_pages": 0,
        "labels_null": 0,
        "node_keys": [],
        "nodes_limits": [],
        "ranges": [],
        "ranges_listed": 0,
        "ranges_cut": 0,
        "labels": null,
        "listed": 0,
        "cut": 0,
    })
}

pub(crate) fn labels(doc: &Pdf, limit: usize) -> Value {
    let (order, _) = doc.page_order();
    let pages = order.len();
    let catalogs = doc.catalogs();
    let mut keys_total = 0u64;
    let mut chosen: Option<(String, Vec<u8>, Option<u64>)> = None;
    for id in &catalogs {
        let Some(one) = doc.object(*id) else {
            continue;
        };
        for (name, raw) in dict_pairs(&one.dict) {
            if name != "PageLabels" {
                continue;
            }
            keys_total += 1;
            if chosen.is_some() {
                continue;
            }
            let body = head(&raw).to_vec();
            if body.get(..2) == Some(&b"<<"[..]) {
                chosen = Some(("inline".to_string(), body, None));
                continue;
            }
            if body.get(..2) == Some(&b"()"[..]) {
                chosen = Some(("string".to_string(), body, None));
                continue;
            }
            if body.first() == Some(&b'<') {
                chosen = Some(("hexstring".to_string(), body, None));
                continue;
            }
            match first_ref(&body) {
                Some(target) => {
                    let held = refer_dict(doc, target);
                    chosen = Some(("reference".to_string(), held, Some(target)));
                }
                None => chosen = Some(("other".to_string(), Vec::new(), None)),
            }
        }
    }
    let Some((via, text, target)) = chosen else {
        return empty(pages, catalogs.len(), keys_total);
    };
    let text = head(&text).to_vec();
    let root_is_dict = text.get(..2) == Some(&b"<<"[..]);
    let nodes = if root_is_dict {
        number_tree_nodes(doc, &text)
    } else {
        Vec::new()
    };
    let root = if root_is_dict {
        dict_pairs(&text)
    } else {
        Vec::new()
    };
    let mut keys_written: BTreeMap<String, u64> = BTreeMap::new();
    for one in &root {
        bump(&mut keys_written, &one.0);
    }
    let has_nums = root.iter().any(|one| one.0 == "Nums");
    let has_kids = root.iter().any(|one| one.0 == "Kids");
    let shape = if has_nums && has_kids {
        "both"
    } else if has_nums {
        "nums"
    } else if has_kids {
        "kids"
    } else {
        "empty"
    };

    let mut rows: Vec<Value> = Vec::new();
    let mut starts: Vec<Value> = Vec::new();
    let mut styles_written: BTreeMap<String, u64> = BTreeMap::new();
    let mut style_forms: BTreeMap<String, u64> = BTreeMap::new();
    let mut value_forms: BTreeMap<String, u64> = BTreeMap::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut st_missing = 0u64;
    let mut prefixes_found = 0u64;
    let mut without_style = 0u64;
    let mut odd_nodes = 0u64;
    let mut items_total = 0u64;
    let mut pairs_total = 0u64;
    for (node_index, node) in nodes.iter().enumerate() {
        items_total += node.items.len() as u64;
        pairs_total += (node.items.len() / 2) as u64;
        if node.items.len() % 2 == 1 {
            odd_nodes += 1;
        }
        let mut index = 0usize;
        while index < node.items.len() {
            let key_raw = node.items[index].clone();
            let start = parse_int(&key_raw);
            starts.push(match start {
                Some(one) => json!(one),
                None => Value::Null,
            });
            let mut row = json!({
                "node": node_index,
                "depth": node.depth,
                "limits": node.limits,
                "index": index / 2,
                "start": start,
                "start_written": String::from_utf8_lossy(&key_raw).into_owned(),
                "value_form": Value::Null,
                "value_target": Value::Null,
                "dict": false,
                "keys": [],
                "style": Value::Null,
                "style_form": Value::Null,
                "st_written": Value::Null,
                "st": Value::Null,
                "prefix": Value::Null,
                "prefix_form": Value::Null,
                "pg_num_written": Value::Null,
                "pg_num_form": Value::Null,
                "pg_num": Value::Null,
                "covers": 0,
            });
            let held = node.items.get(index + 1);
            let form_text = match held {
                None => {
                    bump(&mut value_forms, "missing");
                    let done = row.as_object_mut();
                    if let Some(done) = done {
                        done.insert("value_form".to_string(), json!("missing"));
                    }
                    rows.push(row);
                    index += 2;
                    continue;
                }
                Some(one) => {
                    let body = head(one);
                    if body.get(..2) == Some(&b"<<"[..]) {
                        bump(&mut value_forms, "dict");
                        (json!("dict"), Some(body.to_vec()))
                    } else {
                        match first_ref(body) {
                            Some(id) => {
                                bump(&mut value_forms, "reference");
                                let held = refer_dict(doc, id);
                                let pairs = if held.get(..2) == Some(&b"<<"[..]) {
                                    Some(held)
                                } else {
                                    None
                                };
                                (json!("reference"), pairs)
                            }
                            None => {
                                let (form, _raw) = value_form(Some(body));
                                bump(&mut value_forms, form.unwrap_or("other"));
                                (json!(form.unwrap_or("other")), None)
                            }
                        }
                    }
                }
            };
            let (form_value, pairs) = form_text;
            let sub: Vec<(String, Vec<u8>)> = match &pairs {
                Some(one) => dict_pairs(one),
                None => Vec::new(),
            };
            let is_dict = pairs.is_some();
            let (s_form, style) = value_form(pick(&sub, "S"));
            let (st_form, st_raw) = value_form(pick(&sub, "St"));
            let (p_form, prefix) = value_form(pick(&sub, "P"));
            let (pg_form, pg_raw) = value_form(pick(&sub, "PgNum"));
            let st = match (&st_form, &st_raw) {
                (Some("number"), Some(one)) => one.parse::<i64>().ok(),
                _ => None,
            };
            let pg_num = match (&pg_form, &pg_raw) {
                (Some("number"), Some(one)) => one.parse::<i64>().ok(),
                _ => None,
            };
            if is_dict {
                if s_form.is_none() {
                    without_style += 1;
                    bump(&mut style_forms, "missing");
                    bump(&mut styles_written, "");
                } else {
                    bump(&mut style_forms, s_form.unwrap_or("other"));
                    bump(&mut styles_written, style.as_deref().unwrap_or(""));
                    if !matches!(
                        style.as_deref(),
                        Some("D")
                            | Some("R")
                            | Some("r")
                            | Some("A")
                            | Some("a")
                            | Some("H")
                            | Some("h")
                    ) {
                        unknown.push(style.clone().unwrap_or_default());
                    }
                }
                if st_form.is_none() {
                    st_missing += 1;
                }
                if prefix.is_some() {
                    prefixes_found += 1;
                }
            }
            let keys: Vec<String> = sub.iter().map(|one| one.0.clone()).collect();
            let done = row.as_object_mut();
            if let Some(done) = done {
                done.insert("value_form".to_string(), form_value);
                done.insert(
                    "value_target".to_string(),
                    match held {
                        Some(one) => json!(first_ref(head(one))),
                        None => Value::Null,
                    },
                );
                done.insert("dict".to_string(), json!(is_dict));
                done.insert("keys".to_string(), json!(keys));
                done.insert("style".to_string(), json!(style));
                done.insert("style_form".to_string(), json!(s_form));
                done.insert("st_written".to_string(), json!(st_raw));
                done.insert("st".to_string(), json!(st));
                done.insert("prefix".to_string(), json!(prefix));
                done.insert("prefix_form".to_string(), json!(p_form));
                done.insert("pg_num_written".to_string(), json!(pg_raw));
                done.insert("pg_num_form".to_string(), json!(pg_form));
                done.insert("pg_num".to_string(), json!(pg_num));
            }
            rows.push(row);
            index += 2;
        }
    }

    let real: Vec<i64> = rows
        .iter()
        .filter(|one| one.get("dict").and_then(Value::as_bool) == Some(true))
        .filter_map(|one| one.get("start").and_then(Value::as_i64))
        .collect();
    let ascending = real.windows(2).all(|one| one[0] < one[1]);
    let beyond: Vec<i64> = rows
        .iter()
        .filter(|one| one.get("dict").and_then(Value::as_bool) == Some(true))
        .filter_map(|one| one.get("start").and_then(Value::as_i64))
        .filter(|one| *one >= pages as i64)
        .collect();

    // 逐页算标签：一段盖住从它的起点到「下一段起点之前」，写在后面的段覆盖前面的
    let mut labels: Vec<Value> = Vec::new();
    let mut holder: Vec<Option<(usize, usize)>> = Vec::new();
    for page in 0..pages {
        let mut found: Option<usize> = None;
        for (at, one) in rows.iter().enumerate() {
            if one.get("dict").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let start = match one.get("start").and_then(Value::as_i64) {
                Some(one) => one,
                None => continue,
            };
            if start <= page as i64 {
                found = Some(at);
            }
        }
        let Some(at) = found else {
            labels.push(Value::Null);
            holder.push(None);
            continue;
        };
        let one = &rows[at];
        let node = index_of(one, "node");
        let index = index_of(one, "index");
        let start = one.get("start").and_then(Value::as_i64).unwrap_or(0);
        let st = one.get("st").and_then(Value::as_i64).unwrap_or(1);
        let base = one.get("pg_num").and_then(Value::as_i64).unwrap_or(start);
        let style = one.get("style").and_then(Value::as_str).map(String::from);
        let prefix = one
            .get("prefix")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let number = st + (page as i64 - base);
        labels.push(json!(label_of(number, style.as_deref(), &prefix)));
        holder.push(Some((node, index)));
    }
    for row in &mut rows {
        let node = index_of(&*row, "node");
        let index = index_of(&*row, "index");
        let hits = holder
            .iter()
            .filter(|one| **one == Some((node, index)))
            .count();
        if let Some(done) = row.as_object_mut() {
            done.insert("covers".to_string(), json!(hits));
        }
    }
    let nums_length = match nodes.first() {
        Some(one) => one.items.len(),
        None => 0,
    };
    json!({
        "present": true,
        "catalogs_total": catalogs.len(),
        "label_keys_total": keys_total,
        "written_via": via,
        "target_object": target,
        "tree_shape": shape,
        "keys_written": keys_written,
        "nodes_total": nodes.len(),
        "odd_nodes": odd_nodes,
        "nums_present": has_nums,
        "nums_length": nums_length,
        "nums_length_total": items_total,
        "pairs_total": pairs_total,
        "range_starts": starts,
        "keys_ascending": ascending,
        "styles_written": styles_written,
        "style_forms": style_forms,
        "value_forms": value_forms,
        "st_missing": st_missing,
        "prefixes_found": prefixes_found,
        "ranges_without_style": without_style,
        "unknown_styles": unknown,
        "beyond_pages": beyond,
        "page_count": pages,
        "uncovered_pages": holder.iter().filter(|one| one.is_none()).count(),
        "labels_null": labels.iter().filter(|one| one.is_null()).count(),
        "node_keys": nodes.iter().map(|one| json!(one.keys)).collect::<Vec<Value>>(),
        "nodes_limits": nodes.iter().map(|one| one.limits.clone()).collect::<Vec<Value>>(),
        "ranges": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "ranges_listed": rows.len().min(limit),
        "ranges_cut": rows.len().saturating_sub(limit),
        "labels": labels.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "listed": labels.len().min(limit),
        "cut": labels.len().saturating_sub(limit),
    })
}
