//! 这一页要用的那套三元组：`a:latin` / `a:ea` / `a:cs` 各点了什么字体
//!
//! 与第二读者 `office_reader.slide_font_sets_pptx` 同一条口径。只看写了这三枚之一的
//! `a:defRPr` / `a:rPr` / `a:endParaRPr`；住处按祖先认 —— 母版（`ppt/slideMasters/`）、
//! 版式（`ppt/slideLayouts/`）、正文页（`ppt/slides/`），层由 `a:lvlNpPr` 那一格给，
//! 样式角色由 `titleStyle` / `bodyStyle` / `otherStyle` 给。主题里 majorFont / minorFont
//! 那一份**不归这本**，`theme_ledger` 已经在那边交过。
//!
//! `typeface` 有两种活法：点一个真字体名，或回指主题方案（`+mj-lt` / `+mn-ea` 这类），
//! 所以原样交、并分头数 named / pointer / blank —— 把 `+mj-lt` 折成「有字体」就丢掉了
//! 「这份稿子自己没点名」那句话。三枚的在场组合另记一本（`by_state`）。
//!
//! 一处口径与镜像逐字对齐：这一枚元素在场但没写 `@typeface`，`how` 里交回的是 `absent`
//! 而不是 `blank`；但 `named / pointer / blank` 那三格只数**在场的**那一枚，所以「在场而
//! 没名字」两种写法（`typeface=""` 与压根没写这枚属性）都归进 `blank`。

use crate::office_doc::local_attrs;
use crate::xmlscan::Node;
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SF_TAGS: [&str; 3] = ["latin", "ea", "cs"];
const SF_PREFIXES: [&str; 3] = ["ppt/slideMasters/", "ppt/slideLayouts/", "ppt/slides/"];
const SF_CARRIERS: [&str; 3] = ["defRPr", "rPr", "endParaRPr"];
const SF_ROLES: [&str; 3] = ["titleStyle", "bodyStyle", "otherStyle"];

fn tag_of(node: &Node) -> String {
    node.local().to_string()
}

/// 一枚 typeface 的活法：没写这枚 / 写了空串 / 回指主题方案 / 点了真名字
fn how_of(got: Option<&str>) -> String {
    match got {
        None => "absent".to_string(),
        Some(one) if one.is_empty() => "blank".to_string(),
        Some(one) if one.starts_with('+') => "pointer".to_string(),
        Some(one) => "named".to_string(),
    }
}

fn kind_of(part: &str) -> String {
    if part.contains("slideMasters/") {
        "master".to_string()
    } else if part.contains("slideLayouts/") {
        "layout".to_string()
    } else {
        "slide".to_string()
    }
}

struct Book {
    carriers: u64,
    rows: Vec<Value>,
    by_part: BTreeMap<String, u64>,
    by_state: BTreeMap<String, u64>,
    named: u64,
    pointer: u64,
    blank: u64,
    missing_ea: u64,
    missing_cs: u64,
}

impl Book {
    fn new() -> Book {
        Book {
            carriers: 0,
            rows: Vec::new(),
            by_part: BTreeMap::new(),
            by_state: BTreeMap::new(),
            named: 0,
            pointer: 0,
            blank: 0,
            missing_ea: 0,
            missing_cs: 0,
        }
    }

    fn walk(&mut self, node: &Node, level: &str, role: &str, part: &str, kind: &str) {
        let name = tag_of(node);
        let mut level = level;
        let mut role = role;
        if SF_ROLES.contains(&name.as_str()) {
            role = name.as_str();
        } else if name.starts_with("lvl") && name.ends_with("pPr") {
            level = name.as_str();
        }
        if SF_CARRIERS.contains(&name.as_str()) {
            self.carriers += 1;
            let mut present = [false; 3];
            let mut types: [Option<&str>; 3] = [None; 3];
            for (idx, want) in SF_TAGS.iter().enumerate() {
                for kid in &node.children {
                    if tag_of(kid) == *want {
                        present[idx] = true;
                        types[idx] = kid.attr_local("typeface");
                        break;
                    }
                }
            }
            if present.iter().any(|one| *one) {
                let mut written: Vec<&str> = Vec::new();
                let mut how = serde_json::Map::new();
                let mut faces = serde_json::Map::new();
                for (idx, tag) in SF_TAGS.iter().copied().enumerate() {
                    let live = how_of(types[idx]);
                    how.insert(tag.to_string(), json!(live.as_str()));
                    faces.insert(
                        tag.to_string(),
                        json!({"present": present[idx], "typeface": types[idx]}),
                    );
                    if present[idx] {
                        written.push(tag);
                        // 在场但没点名（`typeface=""` 与压根没写这枚属性）算进 blank 那一格
                        match live.as_str() {
                            "named" => self.named += 1,
                            "pointer" => self.pointer += 1,
                            _ => self.blank += 1,
                        }
                    }
                }
                if !present[1] {
                    self.missing_ea += 1;
                }
                if !present[2] {
                    self.missing_cs += 1;
                }
                let key = written.join("-");
                *self.by_part.entry(kind.to_string()).or_insert(0) += 1;
                *self.by_state.entry(key.clone()).or_insert(0) += 1;
                self.rows.push(json!({
                    "index": self.rows.len(),
                    "part": part,
                    "kind": kind,
                    "carrier": name.clone(),
                    "level": level,
                    "style_role": role,
                    "written": written,
                    "state": key,
                    "how": Value::Object(how),
                    "faces": Value::Object(faces),
                    "attrs": local_attrs(node),
                }));
            }
        }
        for kid in &node.children {
            self.walk(kid, level, role, part, kind);
        }
    }
}

fn empty() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "parts_seen": [],
        "carriers_total": 0,
        "with_triple": 0,
        "by_part": {},
        "by_state": {},
        "named": 0,
        "pointer": 0,
        "blank": 0,
        "missing_ea": 0,
        "missing_cs": 0,
        "listed": 0,
        "cut": 0,
        "rows": [],
    })
}

pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let mut wanted: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.ends_with(".xml") && SF_PREFIXES.iter().any(|pre| one.starts_with(pre)))
        .collect();
    if wanted.is_empty() {
        return empty();
    }
    wanted.sort();
    let mut book = Book::new();
    let mut seen: Vec<String> = Vec::new();
    for part in wanted {
        let Ok(member) = zipread::member(bytes, &part, zipread::DEFAULT_MEMBER_CAP) else {
            continue;
        };
        seen.push(part.clone());
        let text = member.as_text();
        let parsed = crate::xmlscan::parse_str(&text);
        let kind = kind_of(&part);
        // `parse_str` 交的是 `#doc` 伪根：真正的根（p:sldMaster / p:sldLayout / p:sld）
        // 是它的第一个孩子，第一层的祖先都还是空
        for root in &parsed.children {
            book.walk(root, "", "", &part, &kind);
        }
    }
    let Book {
        carriers,
        rows,
        by_part,
        by_state,
        named,
        pointer,
        blank,
        missing_ea,
        missing_cs,
    } = book;
    let written = rows.len();
    json!({
        "family": "ooxml",
        "available": true,
        "parts_seen": seen,
        "carriers_total": carriers,
        "with_triple": written,
        "by_part": by_part,
        "by_state": by_state,
        "named": named,
        "pointer": pointer,
        "blank": blank,
        "missing_ea": missing_ea,
        "missing_cs": missing_cs,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}
