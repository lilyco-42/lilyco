//! OOXML 样式表里那份「内建样式清单」（`latent_styles`，只在 .docx 交）
//!
//! Word 在 `word/styles.xml` 顶部写一条 `w:latentStyles`：那六个 `def*` 是**默认值**（这份文档
//! 没点名的内建样式按它算），而每一条 `w:lsdException` 是一个内建样式的覆写 —— 名字、别名、
//! 下一段样式、`sortOrder` / `uiPriority`、`semiHidden` / `unhideWhenUsed` / `locked` / `qFormat`。
//! 日常那两句话由这一本回答：「这份文档认得哪些内建样式」与「哪些是被隐藏的」。
//!
//! 最要紧的一处形状：**自报的 `w:count` 与实际写出的条数不是一回事**。本仓 94 份 .docx 里 80 份
//! 带这一块，那 80 份的头属性一律是 `defUIPriority=99` / `defSemiHidden=1` /
//! `defUnhideWhenUsed=1` / `count=276`，而 `w:lsdException` 只写了 **137 条** ——
//! 276 是 Word 那份内建清单的总数，137 是这份文件真写了的覆写数。两个数都交、不互相圆场，
//! 另给一个 `declared_matches_written` 说这一份对不对得上（真件 118 份里 117 份写了 exceptions）。
//!
//! 每条覆写「写了哪几枚属性」也要逐条数：python-docx 那一路只写 `name` 与 `hidden` 一类的话，
//! LibreOffice 重写那一路写什么，都按文件的原样进来（`attrs_written` 是按属性名数的本账）。

use crate::office_doc::local_attrs;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一份件里没写这一块时的账：键全给，条数全 0（0 是「看过了没有」）
fn empty(reason: &str) -> Value {
    json!({
        "family": "ooxml",
        "available": reason != "no-part",
        "part": reason != "no-part",
        "block": false,
        "written": {},
        "declared_count": Value::Null,
        "exceptions_total": 0,
        "declared_matches_written": false,
        "distinct_names": 0,
        "attrs_written": {},
        "sample": [],
    })
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let member = match zipread::member(bytes, "word/styles.xml", DEFAULT_MEMBER_CAP).ok() {
        Some(one) => one,
        None => return empty("no-part"),
    };
    let root = xmlscan::parse_str(&member.as_text());
    let block = match root.descendants("latentStyles").first().copied() {
        Some(one) => one,
        None => return empty("no-block"),
    };
    let rows = block.descendants("lsdException");
    let mut attrs: BTreeMap<String, u64> = BTreeMap::new();
    let mut names: Vec<String> = Vec::new();
    for one in &rows {
        for pair in &one.attrs {
            let key = pair.0.as_str();
            let mine = key.rsplit(':').next().unwrap_or(key);
            *attrs.entry(mine.to_string()).or_insert(0) += 1;
        }
        if let Some(raw) = one.attr_local("name") {
            names.push(raw.to_string());
        }
    }
    names.sort();
    names.dedup();
    let declared = block.attr_local("count");
    json!({
        "family": "ooxml",
        "available": true,
        "part": true,
        "block": true,
        // 那六个 def* 是「没点名的那些内建样式按什么算」，原样交
        "written": local_attrs(block),
        "declared_count": declared,
        "exceptions_total": rows.len(),
        "declared_matches_written": declared
            .map(|raw| raw == rows.len().to_string())
            .unwrap_or(false),
        "distinct_names": names.len(),
        "attrs_written": attrs,
        "sample": rows
            .iter()
            .take(limit)
            .map(|one| json!({"written": local_attrs(one)}))
            .collect::<Vec<Value>>(),
    })
}
