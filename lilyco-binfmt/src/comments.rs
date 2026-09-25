//! 文档里那几条批注：谁写的、什么时候、字是什么，以及**锚在正文哪一段**
//!
//! 两族两处：OOXML 把批注的**内容**放在 `word/comments.xml`（`w:comment` 带 `w:id` /
//! `w:author` / `w:initials` / `w:date`），而**锚点**在正文里（`w:commentRangeStart` /
//! `w:commentRangeEnd` / `w:r/w:commentReference` 三处，都只带那个号）；ODF 把两者放在一处
//! （`text:annotation` 坐在它所属的那一段里面，作者是孩子元素 `dc:creator`、时间是 `dc:date`）。
//!
//! 实测三条（`doc-comments.docx` 由 python-docx 写，另两份是 LibreOffice 转的）：
//! 1. **同一份件里部件顺序与正文顺序不是一套**：python-docx 那份 `comments.xml` 三条按
//!    `0,1,2` 排，LibreOffice 重写同一份排成 `1,0,2`，而正文里那九个锚点一条没动 ——
//!    所以「第几条批注」必须说清是**按部件**还是**按正文**，两份都交。
//! 2. 时间两种写法：OOXML 写 `2026-09-25T05:45:49Z`（带 Z），ODF 写 `2026-09-25T05:45:49`
//!    （没有 Z）—— 时区是文件自己写的，读者不替它补。
//! 3. ODF 的批注段**也是段**：那份 .odt 全文 6 个 `text:p` = 正文 3 段 + 三条批注各 1 段，
//!    所以「几段」要分清包不包含批注里的那些（这一格两个数都交）。

use crate::office_text;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// OOXML 那一份：部件里的批注 + 正文里的三个锚点，两边按号配
pub(crate) fn docx(body: &Node, bytes: &[u8], limit: usize) -> Value {
    let mut from_part: Vec<Value> = Vec::new();
    let mut part_written = false;
    if let Ok(member) = zipread::member(bytes, "word/comments.xml", DEFAULT_MEMBER_CAP) {
        part_written = true;
        let root = xmlscan::parse_str(&member.as_text());
        for (index, one) in root.descendants("comment").iter().enumerate() {
            let paras = one.descendants("p");
            from_part.push(json!({
                "part_index": index,
                "id": one.attr_local("id"),
                "author": one.attr_local("author"),
                "initials": one.attr_local("initials"),
                "date": one.attr_local("date"),
                "paragraphs": paras.len(),
                "text": paras
                    .iter()
                    .map(|had| office_text::paragraph_text(had))
                    .collect::<Vec<String>>()
                    .join("\n"),
            }));
        }
    }
    // 三个锚点各按号数一遍：只有一段里那一条 `commentReference` 才算「这一页指着这条批注」
    let mut starts: Vec<String> = Vec::new();
    let mut ends: Vec<String> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    let mut host: Vec<Value> = Vec::new();
    for (index, para) in body.descendants("p").iter().enumerate() {
        let mut here: Vec<String> = Vec::new();
        for one in para.descendants("commentReference") {
            if let Some(id) = one.attr_local("id") {
                refs.push(id.to_string());
                here.push(id.to_string());
            }
        }
        for one in para.descendants("commentRangeStart") {
            if let Some(id) = one.attr_local("id") {
                starts.push(id.to_string());
            }
        }
        for one in para.descendants("commentRangeEnd") {
            if let Some(id) = one.attr_local("id") {
                ends.push(id.to_string());
            }
        }
        if !here.is_empty() {
            host.push(json!({"paragraph": index, "ids": here}));
        }
    }
    let ids_anchored: Vec<&str> = refs.iter().map(|one| one.as_str()).collect();
    let orphan_comments: Vec<Value> = from_part
        .iter()
        .filter(|one| match one["id"].as_str() {
            Some(id) => !ids_anchored.contains(&id),
            None => true,
        })
        .cloned()
        .collect();
    let mut dangling: Vec<String> = Vec::new();
    for id in &refs {
        if !from_part
            .iter()
            .any(|one| one["id"].as_str() == Some(id.as_str()))
            && !dangling.iter().any(|had| had == id)
        {
            dangling.push(id.clone());
        }
    }
    let mut authors: Vec<String> = Vec::new();
    for one in &from_part {
        if let Some(raw) = one["author"].as_str() {
            if !authors.iter().any(|had| had == raw) {
                authors.push(raw.to_string());
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "part_written": part_written,
        "comments_total": from_part.len(),
        "anchor_starts": starts.len(),
        "anchor_ends": ends.len(),
        "anchor_references": refs.len(),
        "range_asymmetric": starts.len() != ends.len(),
        "orphans_without_anchor": orphan_comments.len(),
        "anchors_without_comment": dangling.len(),
        "distinct_authors": authors,
        "comments": from_part.into_iter().take(limit).collect::<Vec<Value>>(),
        "hosts": host.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：批注坐在段里面，作者与时间是孩子元素；「几段」分两个数交
pub(crate) fn odf(root: &Node, limit: usize) -> Value {
    let paragraphs = root.descendants("p");
    let mut rows: Vec<Value> = Vec::new();
    let mut in_annotations = 0usize;
    for (index, para) in paragraphs.iter().enumerate() {
        for one in para.descendants("annotation") {
            let paras = one.descendants("p");
            in_annotations += paras.len();
            rows.push(json!({
                "order": rows.len(),
                "host_paragraph": index,
                "creator": one.child("creator").map(|had| had.text().trim().to_string()),
                "date": one.child("date").map(|had| had.text().trim().to_string()),
                "paragraphs": paras.len(),
                "text": paras
                    .iter()
                    .map(|had| office_text::paragraph_text(had))
                    .collect::<Vec<String>>()
                    .join("\n"),
            }));
        }
    }
    let mut creators: Vec<String> = Vec::new();
    for one in &rows {
        if let Some(raw) = one["creator"].as_str() {
            if !creators.iter().any(|had| had == raw) {
                creators.push(raw.to_string());
            }
        }
    }
    let with_annotation = rows
        .iter()
        .filter_map(|one| one["host_paragraph"].as_u64())
        .collect::<Vec<u64>>();
    json!({
        "family": "odf",
        "available": true,
        "annotations_total": rows.len(),
        "paragraphs_total": paragraphs.len(),
        "paragraphs_in_annotations": in_annotations,
        "paragraphs_body_only": paragraphs.len() - in_annotations,
        "hosted_in": with_annotation.len(),
        "distinct_creators": creators,
        "annotations": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
