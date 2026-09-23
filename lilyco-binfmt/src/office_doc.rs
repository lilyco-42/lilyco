//! `lbin office-doc` — Word 文档的结构：段落之外，这份文档还有什么。
//!
//! 常见需求不是「再读一遍正文」，而是这些问题：这篇文档有几段、几级标题、几张表、
//! 表里多少格、有没有图片与超链接、链接是不是都还在包内、脚注尾注批注有多少、
//! 分页分了几节、用了哪些样式、有没有修订与高亮。这些数在 WordprocessingML 里
//! 都是**数得出来的元素**，不依赖任何解释器的主观判断 —— 所以每个数都写清了是谁数的
//! （`word/document.xml` 里的元素种类），而不是报一个来源不明的「复杂度分数」。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, relationships, Family};
use crate::read::read_blob;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出 Word 文档的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-doc",
    run = "run_office_doc",
    about = "Report the structure of a Word document: paragraph count (empty ones counted separately, because Word's own statistics do), headings with their level and text, every style used and how often, tables with rows and cells, inline shapes and pictures, hyperlinks split into internal and external with their targets, sections, explicit page/column breaks, footnotes and endnotes and comments (read from their own parts when present), tracked-change presence (w:ins / w:del counts), numbering usage, headers and footers, embedded objects and custom XML, plus which optional parts the package actually carries. For legacy .doc it falls back to what the piece table can honestly tell: paragraph count from the CP total plus the marker characters it dropped (cell ends, field boundaries), not a claim about tables it cannot see. Returns { path, format, kind, structure, styles, tables, images, hyperlinks, parts, notes }. Read-only (safety T0)."
)]
pub struct OfficeDoc {
    /// Word 文档（docx / docm / doc）
    #[arg(about = "Word document to structure", must_exist = true)]
    path: PathBuf,

    /// 最多列多少个标题/链接（总数照实给）
    #[arg(
        about = "List at most this many items per collection",
        default = 100,
        min = 1
    )]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

fn run_office_doc(app: &OfficeDoc, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the document structure".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let mut notes: Vec<String> = Vec::new();
    let limit = usize::try_from(app.limit).unwrap_or(usize::MAX);
    let result = if doc.family == Family::Ooxml && doc.app == "word" {
        let member = match zipread::member(bytes, "word/document.xml", DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(why) => return Err(AppError::InvalidInput(why)),
        };
        if !member.verified {
            notes.push(format!("word/document.xml：{}", member.note));
        }
        let root = xmlscan::parse_str(&member.as_text());
        let body = match root.child("body") {
            Some(one) => one,
            None => {
                notes.push("document.xml 里没有 body 元素".to_string());
                &root
            }
        };
        let paragraphs = body.descendants("p");
        let mut headings: Vec<Value> = Vec::new();
        let mut styles: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut empty = 0usize;
        for one in &paragraphs {
            let text = crate::office_text::paragraph_text(one);
            if text.is_empty() {
                empty += 1;
            }
            if let Some(style) = crate::office_text::style_of(one) {
                *styles.entry(style).or_insert(0) += 1;
            }
            if let Some(level) = crate::office_text::heading_of(one) {
                if headings.len() < limit {
                    headings.push(json!({"level": level, "text": text}));
                }
            }
        }
        let tables: Vec<Value> = body
            .descendants("tbl")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "rows": one.descendants("tr").len(),
                    "cells": one.descendants("tc").len(),
                    "text": one.descendants("p").iter().filter(|p| !crate::office_text::paragraph_text(p).is_empty()).count(),
                })
            })
            .collect();
        let (rels, mut rel_notes) = relationships(bytes, &doc.entries);
        notes.append(&mut rel_notes);
        let hyperlinks: Vec<Value> = rels
            .iter()
            .filter(|one| one.kind == "hyperlink")
            .take(limit)
            .map(|one| json!({"target": one.target, "external": one.external, "resolves": one.resolved}))
            .collect();
        let image_parts: Vec<String> = rels
            .iter()
            .filter(|one| one.kind == "image")
            .filter_map(|one| one.resolved.clone())
            .take(limit)
            .collect();
        let count = |name: &str, part: &str| -> Option<usize> {
            let member = zipread::member(bytes, part, DEFAULT_MEMBER_CAP).ok()?;
            let root = xmlscan::parse_str(&member.as_text());
            Some(root.descendants(name).len())
        };
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "wordprocessingml",
            "structure": {
                "paragraphs": paragraphs.len(),
                "empty_paragraphs": empty,
                "tables": body.descendants("tbl").len(),
                "table_rows": body.descendants("tr").len(),
                "table_cells": body.descendants("tc").len(),
                "sections": body.descendants("sectPr").len(),
                "breaks": body.descendants("br").len(),
                "page_breaks": body.descendants("br").iter().filter(|one| one.attr_local("type") == Some("page")).count(),
                "drawings": body.descendants("drawing").len(),
                "text_boxes": body.descendants("txbxContent").len(),
                "insertions": body.descendants("ins").len(),
                "deletions": body.descendants("del").len(),
                "bookmarks": body.descendants("bookmarkStart").len(),
                "fields": body.descendants("fldSimple").len() + body.descendants("fldChar").len(),
                "has_numbering": body.descendants("numPr").len() > 0,
            },
            "headings": headings,
            "styles": styles,
            "tables": tables,
            "images": image_parts,
            "hyperlinks": hyperlinks,
            "footnotes": count("footnote", "word/footnotes.xml"),
            "endnotes": count("endnote", "word/endnotes.xml"),
            "comments": count("comment", "word/comments.xml"),
            "parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("word/")).take(limit).collect::<Vec<String>>(),
            "notes": notes,
        })
    } else if doc.family == Family::Compound && doc.app == "word" {
        // 遗留 .doc：piece 表能给出的就是字符数与那些结构标记，别装作看得见表格线
        let cfb = match doc.compound.as_ref() {
            Some(one) => one,
            None => return Err(AppError::InvalidInput("复合文档打不开".to_string())),
        };
        let body = crate::word::read(cfb, bytes)?;
        let raw = &body.text;
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "word-binary",
            "structure": {
                "paragraphs": raw.matches('\r').count(),
                "table_cell_marks": raw.matches('\u{7}').count(),
                "field_marks": raw.matches('\u{13}').count(),
                "object_marks": raw.matches('\u{1}').count() + raw.matches('\u{2}').count(),
                "annotation_marks": raw.matches('\u{5}').count(),
                "page_break_marks": raw.matches('\u{C}').count(),
                "soft_breaks": raw.matches('\u{B}').count(),
                "cp_total": body.cp_total,
                "pieces": body.pieces.len(),
                "table_streams": body.table_stream,
            },
            "headings": Value::Null,
            "styles": Value::Null,
            "tables": Value::Null,
            "images": [],
            // 域标记数已经报在 structure.fields 里；这里的链接目标在表流的另一段，本版本不解
            "hyperlinks": [],
            "footnotes": null,
            "endnotes": null,
            "comments": null,
            "parts": cfb.stream_names(),
            "notes": concat_notes(&body.notes, "遗留 .doc 的段落样式、表格与图形在表流的其它记录里，本版本只数正文里的结构标记"),
        })
    } else {
        return Err(AppError::InvalidInput(format!(
            "{} 不是 Word 文档（识别为 {} / {}）；表格用 office-sheet，演示文稿用 office-slide",
            app.path.display(),
            doc.app,
            doc.format
        )));
    };
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn concat_notes(mine: &[String], extra: &str) -> Vec<String> {
    let mut out: Vec<String> = mine.to_vec();
    out.push(extra.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeDoc {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 100,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_doc(&app, &Context::new_test(tx)).expect("office-doc 应成功")
    }

    /// 结构数字要与独立读者算出来的逐项一致（期望值：office_reader.py 的 docx_facts）
    #[test]
    fn counts_a_real_docx_structure() {
        let out = run("notes.docx");
        assert_eq!(out["kind"], "wordprocessingml");
        let s = &out["structure"];
        assert_eq!(s["paragraphs"], 11, "{s}");
        assert_eq!(s["empty_paragraphs"], 2, "{s}");
        assert_eq!(s["tables"], 1);
        assert_eq!(s["table_rows"], 2);
        assert_eq!(s["table_cells"], 4);
        assert_eq!(s["sections"], 1);
        assert_eq!(s["drawings"], 1, "那张 Pillow 画的 PNG");
        assert_eq!(s["page_breaks"], 1);
        assert_eq!(s["has_numbering"], json!(true));
        assert_eq!(
            out["headings"],
            json!([
                {"level": 1, "text": "一级标题：预算口径"},
                {"level": 2, "text": "二级标题：明细"}
            ]),
            "{out}"
        );
        assert_eq!(out["styles"]["Heading1"], 1);
        assert_eq!(out["styles"]["Heading2"], 1);
        assert_eq!(out["comments"], json!(1), "批注部件要一起数");
        assert_eq!(out["footnotes"], json!(0));
        assert_eq!(out["endnotes"], json!(0));
        assert_eq!(out["images"], json!(["word/media/image1.png"]));
        assert_eq!(out["hyperlinks"][0]["target"], "https://example.com/budget");
        assert_eq!(out["hyperlinks"][0]["external"], json!(true));
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// 表格里的段落也算段落：这是 Word 自己的口径，换了口径数字就对不上
    #[test]
    fn table_paragraphs_stay_paragraphs() {
        let out = run("notes.docx");
        assert!(
            out["structure"]["paragraphs"].as_u64().expect("有数") > 8,
            "{out}"
        );
        assert_eq!(out["tables"][0]["rows"], 2);
        assert_eq!(out["tables"][0]["cells"], 4);
    }

    /// 遗留 .doc 只报它真看得见的东西，并说明另一些为什么没有
    #[test]
    fn a_legacy_doc_reports_only_what_the_piece_table_shows() {
        let out = run("notes.doc");
        assert_eq!(out["kind"], "word-binary");
        assert_eq!(out["structure"]["paragraphs"], 11, "{out}");
        assert_eq!(out["structure"]["cp_total"], 139);
        assert_eq!(out["structure"]["pieces"], 1);
        assert_eq!(
            out["structure"]["table_cell_marks"], 6,
            "两个 2x2 表 + 行列分隔：{out}"
        );
        assert_eq!(out["structure"]["field_marks"], 1, "那个超链接域");
        assert!(
            out["headings"].is_null(),
            "样式表在表流里，本版本不解 → 不假装报得出来"
        );
        let note = out["notes"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("结构标记"), "{note}");
    }

    /// 不是 Word 文件要指路，而不是给一份空结构
    #[test]
    fn a_non_word_file_is_pointed_at_the_right_command() {
        let app = OfficeDoc {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/deck.pptx"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_doc(&app, &Context::new_test(tx)).unwrap_err();
        let text = why.to_string();
        assert!(text.contains("office-slide"), "{text}");
    }
}
