//! 一份 .docx 里到底有几份样式表（`styles_parts`，只在 OOXML 文字那一家交）
//!
//! 平时说的「样式表」是 `word/styles.xml`，可 Word 自己还会塞一份
//! `word/stylesWithEffects.xml` —— 那是给旧读者看的第二份样式表，**两份各有各的关系**
//! （`word/_rels/document.xml.rels` 里 `Type=.../styles` 指向 `styles.xml`，
//! `Type=.../stylesWithEffects` 指向 `stylesWithEffects.xml`）。本仓 95 份 .docx 里
//! **45 份带第二份、50 份只有一份**；带第二份的 45 份里**逐字节相同的一份都没有**，
//! 而 LibreOffice 自己写的那一路（`*-lo.docx`）只写一份、rels 里也只有 `styles` 一条。
//!
//! 更要紧的是方向：直觉会说「带 effects 的那份是超集」，量出来相反 ——
//! 样式**条数更少**（`alternate.docx`：164 对 160）而**字节更多**（349,458 对 438,131）：
//! 每个样式自己写的东西变了（`basedOn` 158→154、`link` 38→34、`tab` 11→7、`uiPriority` 163→159），
//! 条数反而少四条。所以「哪一份更完整」不是一句话，逐本计数各交各的。
//!
//! 这一本同时划清本仓其它样式类账本的适用范围：`latent_styles` / `table_style_branches` /
//! `doc_defaults` / 字符样式那一本**只走 `word/styles.xml`** —— 在 45 份件上，那些数说的
//! 可能不是当前读者真正拿到的那一份。这句话要能被机器看见，而不是靠人记得。

use crate::office_doc::local_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MAIN: &str = "word/styles.xml";

/// 这几本元素各数一遍 —— 「两份差在哪」由计数说话，不比对整棵树
const COUNTED: [&str; 16] = [
    "latentStyles",
    "docDefaults",
    "rPrDefault",
    "pPrDefault",
    "lsdException",
    "basedOn",
    "link",
    "next",
    "aliases",
    "uiPriority",
    "semiHidden",
    "qFormat",
    "tblPr",
    "tblStylePr",
    "name",
    "tab",
];

/// `word/_rels/document.xml.rels` 里指向样式部件的那几条关系（Type 与 Target 按写的交）
fn rel_targets(bytes: &[u8]) -> Vec<Value> {
    let member =
        match zipread::member(bytes, "word/_rels/document.xml.rels", DEFAULT_MEMBER_CAP).ok() {
            Some(one) => one,
            None => return Vec::new(),
        };
    let root = xmlscan::parse_str(&member.as_text());
    let mut out: Vec<Value> = Vec::new();
    for one in root.descendants("Relationship") {
        let kind = one
            .attr("Type")
            .map(|raw| raw.rsplit('/').next().unwrap_or(raw).to_string())
            .unwrap_or_default();
        if !kind.to_lowercase().contains("styles") {
            continue;
        }
        out.push(json!({"type": kind, "target": one.attr("Target")}));
    }
    out.sort_by(|a, b| {
        let key = |one: &Value| {
            (
                one["type"].as_str().unwrap_or("").to_string(),
                one["target"].as_str().unwrap_or("").to_string(),
            )
        };
        key(a).cmp(&key(b))
    });
    out
}

/// 一份样式部件的账：字节数、按 `w:type` 的样式条数、那几本元素的计数、根元素自己写的属性
fn part_ledger(raw: &[u8], name: &str) -> Value {
    let text = String::from_utf8_lossy(raw).to_string();
    let root: Node = xmlscan::parse_str(&text);
    let styles = root.descendants("style");
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    for one in &styles {
        let kind = one.attr_local("type").unwrap_or("<无 type>").to_string();
        *kinds.entry(kind).or_insert(0) += 1;
    }
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for want in COUNTED {
        let found = root.descendants(want).len();
        if found > 0 {
            counts.insert(want.to_string(), found as u64);
        }
    }
    // `parse_str` 交的是 #doc 伪根：它的属性不是部件的属性，真正的根元素是第一个子元素
    let written = root
        .children
        .first()
        .map(local_attrs)
        .unwrap_or_else(|| json!({}));
    json!({
        "part": name,
        "present": true,
        "bytes": raw.len(),
        "styles_total": styles.len(),
        "styles_by_kind": kinds,
        "counts": counts,
        "written": written,
    })
}

/// 包里一份样式部件都没有时的账：键全给，条数全 0（0 是「看过了没有」）
fn missing() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "main_part": false,
        "alt_parts": [],
        "listed_parts": 0,
        "cut": false,
        "unread_parts": [],
        "rels": [],
        "differs_from_main": Value::Null,
        "styles_delta": Value::Null,
        "bytes_delta": Value::Null,
        "main": Value::Null,
        "alt": Value::Null,
        "entries": [],
    })
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/styles") && one.ends_with(".xml"))
        .collect();
    if names.is_empty() {
        return missing();
    }
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut raws: Vec<(String, Vec<u8>)> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    for name in &names {
        match zipread::member(bytes, name, DEFAULT_MEMBER_CAP).ok() {
            Some(member) => {
                raws.push((name.clone(), member.data.clone()));
                rows.push(part_ledger(&member.data, name));
            }
            None => unread.push(name.clone()),
        }
    }
    let main = rows.iter().find(|one| one["part"] == json!(MAIN)).cloned();
    let alt = rows.iter().find(|one| one["part"] != json!(MAIN)).cloned();
    // 比字节而不是比长度：等长而不同样的两份也得判为不同
    let main_raw = raws
        .iter()
        .find(|(name, _)| name == MAIN)
        .map(|(_, data)| data.clone());
    let alt_raw = raws
        .iter()
        .find(|(name, _)| name != MAIN)
        .map(|(_, data)| data.clone());
    let mut differs = Value::Null;
    let mut styles_delta = Value::Null;
    let mut bytes_delta = Value::Null;
    if let (Some(a), Some(b), Some(mine), Some(other)) =
        (&main, &alt, main_raw.as_ref(), alt_raw.as_ref())
    {
        differs = json!(mine != other);
        let a_styles = a["styles_total"].as_u64().unwrap_or(0) as i64;
        let b_styles = b["styles_total"].as_u64().unwrap_or(0) as i64;
        styles_delta = json!(b_styles - a_styles);
        let a_bytes = a["bytes"].as_u64().unwrap_or(0) as i64;
        let b_bytes = b["bytes"].as_u64().unwrap_or(0) as i64;
        bytes_delta = json!(b_bytes - a_bytes);
    }
    let listed = rows.len().min(limit);
    json!({
        "family": "ooxml",
        "available": true,
        "main_part": main.is_some(),
        // 「包里有几份样式表」「列出了几份」与「哪条关系指着它们」是三句话
        "alt_parts": rows
            .iter()
            .filter(|one| one["part"] != json!(MAIN))
            .filter_map(|one| one["part"].as_str().map(String::from))
            .collect::<Vec<String>>(),
        "listed_parts": listed,
        "cut": rows.len() > listed,
        "unread_parts": unread,
        "rels": rel_targets(bytes),
        // 带符号的差一律是「第二份 减 主那份」
        "differs_from_main": differs,
        "styles_delta": styles_delta,
        "bytes_delta": bytes_delta,
        "main": main.unwrap_or(Value::Null),
        "alt": alt.unwrap_or(Value::Null),
        "entries": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
