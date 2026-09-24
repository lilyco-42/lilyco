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
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出 Word 文档的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-doc",
    run = "run_office_doc",
    about = "Report the structure of a Word document: paragraph count (empty ones counted separately, because Word's own statistics do), headings with their level and text, every style used and how often, tables with rows and cells, inline shapes and pictures, hyperlinks split into internal and external with their targets, sections, explicit page/column breaks, footnotes and endnotes and comments (read from their own parts when present), tracked-change presence (w:ins / w:del counts) plus a revision ledger (revisions): one entry per logical change with its kind, author, date, the paragraph index it sits in and the words it carries - elements are merged only when adjacent with the same kind/author/date/paragraph, because a producer writes one edit as several runs (LibreOffice splits the number from the unit into two w:ins), while the ODF export of the very same file states them as one changed-region, which is what this merge rule was measured against. Paragraph-mark insertions (w:pPr/w:rPr/w:ins) are counted apart from the paragraph's text and are not merged with it; ODF keeps deleted words inside the region and inserted words between text:change-start and text:change-end in the body, and both are read. Legacy .doc reports revisions as null rather than guess (the redline tables live in the table stream, not the piece table). protection (docx: w:documentProtection in word/settings.xml - w:edit says what kind of editing is restricted and w:enforcement says whether it is on; odt: the ProtectForm/ProtectBookmarks/ProtectFields config-items in settings.xml, which is a different place and does NOT carry the docx restriction across - the same document converted to .odt reports false for all three, measured); numbering usage, headers and footers, embedded objects and custom XML, plus which optional parts the package actually carries. statistics answers 'how many words/pages': ours (characters, characters_no_spaces and words_by_space - the last split on whitespace only, which is why it is named that way and not 'words') next to the producer's own numbers (docx docProps/app.xml, ODF meta.xml document-statistic) because the two disagree by design - python-docx writes app.xml with Words/Characters at 0 (it never counted), and LibreOffice counts Chinese words rather than whitespace runs, while on the same text our character counts match its character-count exactly. For legacy .doc it falls back to what the piece table can honestly tell: paragraph count from the CP total plus the marker characters it dropped (cell ends, field boundaries), not a claim about tables it cannot see. For .odt it reports the same account from content.xml's office:text (headings from text:outline-level, comments from text:annotation, pictures from draw:image, footnotes and endnotes split out of the single text:note element by its note:class) and additionally echoes meta.xml's own document-statistic so the producer's numbers are visible next to ours. Returns { path, format, kind, structure, styles, tables, images, hyperlinks, revisions, protection, statistics, parts, notes }. Read-only (safety T0)."
)]
pub struct OfficeDoc {
    /// Word 文档（docx / docm / doc / odt）
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

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 100;

/// 字数与字符数：口径写在键名上。`words_by_space` 就是「按空白切的词」——
/// 一整段中文可能只算一个「词」，那不是数错，是这个口径对中文意义有限，
/// 所以它必须与 `characters_no_spaces` 一起看（Word/LibreOffice 自己的「字数」
/// 是另一套规则，这里不模仿，只把它自报的那份照抄在下面）
#[derive(Default)]
struct Tally {
    characters: usize,
    no_space: usize,
    words_by_space: usize,
}

impl Tally {
    fn add(&mut self, text: &str) {
        self.characters += text.chars().count();
        self.no_space += text.chars().filter(|one| !one.is_whitespace()).count();
        self.words_by_space += text.split_whitespace().count();
    }

    fn to_json(&self) -> Value {
        json!({
            "characters": self.characters,
            "characters_no_spaces": self.no_space,
            "words_by_space": self.words_by_space,
        })
    }
}

/// `docProps/app.xml` 里生产者自报的那几个数。python-docx 写的样本里
/// `Words` / `Characters` / `Paragraphs` 全是 0 —— 那是「它没数过」，不是
/// 「这份文档没有字」，所以两份账并排放，谁也不许盖掉谁。
fn producer_counts(bytes: &[u8]) -> Value {
    let Ok(member) = zipread::member(bytes, "docProps/app.xml", DEFAULT_MEMBER_CAP) else {
        return Value::Null;
    };
    let root = xmlscan::parse_str(&member.as_text());
    let mut out = serde_json::Map::new();
    for (key, want) in [
        ("words", "Words"),
        ("characters", "Characters"),
        ("paragraphs", "Paragraphs"),
        ("lines", "Lines"),
        ("pages", "Pages"),
    ] {
        let found = root.descendants(want);
        let Some(one) = found.first() else {
            continue;
        };
        let raw = one.text().trim().to_string();
        out.insert(
            key.to_string(),
            match raw.parse::<i64>() {
                Ok(number) => json!(number),
                Err(_) => json!(raw),
            },
        );
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
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
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    let result = if doc.family == Family::Ooxml && doc.app == "word" {
        let member = match zipread::member(bytes, "word/document.xml", DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(why) => return Err(AppError::InvalidInput(why)),
        };
        if !member.verified {
            notes.push(format!("word/document.xml：{}", member.note));
        }
        let root = xmlscan::parse_str(&member.as_text());
        // `#doc` 的直接孩子是 `<w:document>`，`w:body` 在它下面一层：
        // 只往下走一步就会永远找不到 body，然后所有计数都从伪根走 ——
        // 数字照样对（descendants 是全树），但那条「没有 body」的假话会一直留在 notes 里。
        let document = root.child("document").unwrap_or(&root);
        let body = match document.child("body") {
            Some(one) => one,
            None => {
                notes.push("document.xml 里没有 w:body 元素".to_string());
                document
            }
        };
        let paragraphs = body.descendants("p");
        let mut headings: Vec<Value> = Vec::new();
        let mut styles: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut empty = 0usize;
        let mut tally = Tally::default();
        for one in &paragraphs {
            let text = crate::office_text::paragraph_text(one);
            tally.add(&text);
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
        let count = |name: &str, part: &str| -> usize {
            // 部件不在包里就是零个：OOXML 的脚注 / 尾注 / 批注各自是一个部件，
            // 没写这个部件等于文档里没有这类东西。只有遗留 .doc 看不见它们，才给 null。
            zipread::member(bytes, part, DEFAULT_MEMBER_CAP)
                .map(|member| {
                    let root = xmlscan::parse_str(&member.as_text());
                    root.descendants(name)
                        .iter()
                        // 脚注与尾注部件里白坐着两条分隔符（separator 与
                        // continuationSeparator）：LibreOffice 与 Word 都写，
                        // 按元素个数数就会凭空多出两条「注」
                        .filter(|one| {
                            !matches!(
                                one.attr_local("type").unwrap_or_default(),
                                "separator" | "continuationSeparator"
                            )
                        })
                        .count()
                })
                .unwrap_or(0)
        };
        // 修订这份账。`word/settings.xml` 里的 `w:trackChanges` 说的是「往后还记不记」，
        // 与正文里已经存着的那些改动是两件事，所以两个都报
        let settings = zipread::member(bytes, "word/settings.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| xmlscan::parse_str(&one.as_text()));
        let revisions = crate::revise::docx_ledger(&paragraphs, settings.as_ref());
        // 保护与修订是两件事：一个是「这份文件让不让你改」，一个是「改过的那些痕迹」
        let protection = crate::protect::docx_document(settings.as_ref());
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
                // 部件存在 ≠ 文档用到了编号：notes.docx 带着 numbering.xml，
                // 正文里却一个 numPr 都没有（python-docx 没往里写列表）
                "has_numbering": body.descendants("numPr").len() > 0,
                "numbering_part": doc
                    .entries
                    .iter()
                    .any(|one| one.name == "word/numbering.xml"),
            },
            "headings": headings,
            "styles": styles,
            "tables": tables,
            "images": image_parts,
            "hyperlinks": hyperlinks,
            "footnotes": count("footnote", "word/footnotes.xml"),
            "endnotes": count("endnote", "word/endnotes.xml"),
            "comments": count("comment", "word/comments.xml"),
            "revisions": revisions.to_json(limit),
            "protection": protection,
            // 「多少字、多少页」这一问有两份账：自己数的与生产者自报的
            "statistics": {
                "ours": tally.to_json(),
                "producer": producer_counts(bytes),
            },
            "parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("word/")).take(limit).collect::<Vec<String>>(),
            "notes": notes,
        })
    } else if doc.family == Family::Odf && doc.app == "word" {
        // ODF 文字：正文在 office:body > office:text，属性都带前缀而前缀是文件自己声明的，
        // 所以按局部名取（但要躲开 LibreOffice 抄的那份 calcext: 副本）。
        let member = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(why) => return Err(AppError::InvalidInput(why)),
        };
        let href = |one: &xmlscan::Node| -> Option<String> {
            crate::odsheet::attr_of(one, "href").map(|one| one.to_string())
        };
        let root = xmlscan::parse_str(&member.as_text());
        let text_body = root
            .descendants("body")
            .into_iter()
            .find_map(|one| one.child("text"))
            .unwrap_or(&root);
        let mut paragraphs: Vec<&xmlscan::Node> = Vec::new();
        crate::office_text::odf_paragraphs(text_body, &mut paragraphs);
        let mut empty = 0usize;
        let mut tally = Tally::default();
        let mut styles: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for one in &paragraphs {
            let text = crate::office_text::odf_paragraph_text(one);
            tally.add(&text);
            if text.is_empty() {
                empty += 1;
            }
            if let Some(style) = crate::odsheet::attr_of(one, "style-name") {
                *styles.entry(style.to_string()).or_insert(0) += 1;
            }
        }
        // 标题在 ODF 里不是 `text:p` 而是 `text:h`：算字数要把它一起算，
        // 不然「这份文档多少字」会漏掉所有小标题
        for one in text_body.descendants("h") {
            tally.add(&crate::office_text::paragraph_text(one));
        }
        let headings: Vec<Value> = text_body
            .descendants("h")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "level": crate::odsheet::attr_of(one, "outline-level")
                        .and_then(|raw| raw.trim().parse::<u64>().ok()),
                    "text": crate::office_text::paragraph_text(one),
                })
            })
            .collect();
        let tables: Vec<Value> = text_body
            .descendants("table")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "name": crate::odsheet::attr_of(one, "name"),
                    "rows": one.all("table-row").len(),
                    "cells": one.descendants("table-cell").len(),
                    "covered": one.descendants("covered-table-cell").len(),
                    "text": one
                        .descendants("p")
                        .iter()
                        .filter(|p| !crate::office_text::paragraph_text(p).is_empty())
                        .count(),
                })
            })
            .collect();
        // 脚注与尾注在 ODF 里是同一个 `text:note`，靠 note:class 分家
        let notes_found: Vec<&xmlscan::Node> = text_body.descendants("note");
        let of_class = |want: &str| -> usize {
            notes_found
                .iter()
                .filter(|one| crate::odsheet::attr_of(one, "note-class") == Some(want))
                .count()
        };
        let unclassed = notes_found.len() - of_class("footnote") - of_class("endnote");
        if unclassed > 0 {
            notes.push(format!(
                "{} 个 text:note 没写 note:class，分不清脚注还是尾注",
                unclassed
            ));
        }
        let hyperlinks: Vec<Value> = text_body
            .descendants("a")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "target": href(one),
                    "text": one.text().trim(),
                })
            })
            .collect();
        let images: Vec<String> = text_body
            .descendants("image")
            .iter()
            .filter_map(|one| href(one))
            .take(limit)
            .collect();
        let statistic = zipread::member(bytes, "meta.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| {
                let meta = xmlscan::parse_str(&one.as_text());
                let mut out = json!({});
                if let Some(node) = meta.descendants("document-statistic").first() {
                    for (key, value) in &node.attrs {
                        let local = key.rsplit(':').next().unwrap_or(key).to_string();
                        out[local] = json!(value);
                    }
                }
                out
            })
            .unwrap_or_else(|| json!({}));
        notes.push(
            "ODF 的段落口径与 OOXML 一致：表格里也算段；`structure.sections` 是 text:section（内容分块），\
             不是 Word 那种分页设置"
                .to_string(),
        );
        // ODF 的修订存在两处：`text:changed-region` 是账（谁、什么时候、哪一类），
        // 删掉的字在 region 里，插入的字在正文那两个标记之间
        let revisions = crate::revise::odt_ledger(text_body, &paragraphs);
        // ODF 的文档级保护不在 content.xml 里，在 settings.xml 的那几个 config-item 上
        let protection = match zipread::member(bytes, "settings.xml", DEFAULT_MEMBER_CAP) {
            Ok(member) => {
                let settings_root = xmlscan::parse_str(&member.as_text());
                crate::protect::odt_document(&settings_root)
            }
            Err(_) => json!({"items": {}, "protected": false, "part": false}),
        };
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-text",
            "structure": {
                "paragraphs": paragraphs.len(),
                "empty_paragraphs": empty,
                "tables": tables.len(),
                "table_rows": text_body.descendants("table-row").len(),
                "table_cells": text_body.descendants("table-cell").len(),
                "covered_cells": text_body.descendants("covered-table-cell").len(),
                "sections": text_body.descendants("section").len(),
                "breaks": text_body.descendants("line-break").len(),
                "page_breaks": text_body.descendants("soft-page-break").len(),
                "drawings": text_body.descendants("frame").len(),
                "annotations": text_body.descendants("annotation").len(),
                "lists": text_body.descendants("list").len(),
                "list_styles": text_body.descendants("list-style").len(),
                "bookmarks": text_body.descendants("bookmark-start").len()
                    + text_body.descendants("bookmark").len(),
                "sequences": text_body.descendants("sequence-decl").len(),
                "tracked_changes": text_body.descendants("tracked-changes").len(),
                "hyperlinks": hyperlinks.len(),
                "images": images.len(),
            },
            "headings": headings,
            "styles": styles,
            "tables": tables,
            "images": images,
            "hyperlinks": hyperlinks,
            "footnotes": of_class("footnote"),
            "endnotes": of_class("endnote"),
            "comments": text_body.descendants("annotation").len(),
            "revisions": revisions.to_json(limit),
            "protection": protection,
            // 与 docx 那一份同一个形状：自己数的与生产者自报的并排
            // （ODF 的生产者账在 meta.xml 的 document-statistic，值全是字符串）
            "statistics": {
                "ours": tally.to_json(),
                "producer": statistic,
            },
            "parts": doc.entries.iter().map(|one| one.name.clone()).take(limit).collect::<Vec<String>>(),
            "notes": notes,
        })
    } else if doc.family == Family::Compound && doc.app == "word" {
        // 遗留 .doc：piece 表能给出的就是字符数与那些结构标记，别装作看得见表格线
        let cfb = match doc.compound.as_ref() {
            Some(one) => one,
            None => return Err(AppError::InvalidInput("复合文档打不开".to_string())),
        };
        let body = crate::word::read(cfb, bytes).map_err(AppError::InvalidInput)?;
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
            // 遗留 .doc 的修订在表流的 LVC/PAPX 那套结构里，piece 表给不出「谁改了什么」
            "revisions": null,
            "protection": null,
            "parts": cfb.stream_names(),
            "notes": concat_notes(&body.notes, "遗留 .doc 的段落样式、表格与图形在表流的其它记录里，本版本只数正文里的结构标记；修订那份账（谁、什么时候）也住在表流里，这里读不出，宁可给 null"),
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

    /// 「谁在什么时候改了哪一段」这一问。期望值全部来自 `lyco_revisions.py`，
    /// 而这份 fixture 是 LibreOffice 写的 OOXML：它把一次插入拆成两个 `w:ins`
    /// （数字与单位各一条），所以**元素数 6 与逻辑改动 4 不是一回事**，两个都要报
    #[test]
    fn the_revision_ledger_says_who_changed_what_where() {
        let out = run("revisions-lo.docx");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 4, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 3, "一次编辑被拆成两条");
        assert_eq!(rev["elements"]["deletions"], 2);
        assert_eq!(rev["elements"]["format_changes"], 1);
        assert_eq!(
            rev["paragraph_marks"], 0,
            "LibreOffice 导出时丢了段落标记那条"
        );
        assert_eq!(rev["track_changes"], json!(false), "文件自己说没开着记录");
        let changes = rev["changes"].as_array().expect("是数组");
        assert_eq!(
            changes[0],
            json!({
                "index": 0, "kind": "insertion", "author": "张三",
                "date": "2026-03-05T09:12:00Z", "paragraph": 2,
                "text": "124000 元", "elements": 2, "paragraph_mark": false,
            }),
            "第一处：{}",
            changes[0]
        );
        assert_eq!(changes[1]["kind"], "deletion");
        assert_eq!(changes[1]["author"], "李四");
        assert_eq!(changes[1]["text"], "89000 元");
        assert_eq!(changes[1]["elements"], 2);
        // 改格式这一条带着**被改了格式的那些字**：字没动，但「改了哪里的样子」就是这几个字。
        // 这几个字不在这个元素里（那装的是新的 rPr），在它所在的 run 身上 —— 不留这一步，
        // docx 这条永远是空的，而 ODF 那侧的 region 区间里有字，两份账就不平（CI 比出来的）
        assert_eq!(changes[2]["kind"], "format-change");
        assert_eq!(changes[2]["author"], "王五");
        assert_eq!(changes[2]["text"], "，请复核。");
        assert_eq!(changes[3]["text"], "整段是新加的。");
        assert_eq!(changes[3]["paragraph"], 2 + 1, "整段新加的是下一段");
        // 段落序号的基准与 structure.paragraphs 同一份列表：标题算第 0 段
        assert_eq!(out["structure"]["paragraphs"], 4);
        assert_eq!(out["structure"]["insertions"], 3);
        assert_eq!(out["structure"]["deletions"], 2);
        let note = rev["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("6 个修订元素合成 4 条"), "{note}");
    }

    /// 同一条规则的另一种形状：python-docx 手写的那份没被拆开，而且段落标记自己
    /// 也算一处改动 —— 它与同段同作者同时间的正文那条**不许合并**
    #[test]
    fn a_paragraph_mark_revision_stays_apart_from_its_text() {
        let out = run("revisions.docx");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 5, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 3);
        assert_eq!(rev["elements"]["deletions"], 1);
        assert_eq!(rev["paragraph_marks"], 1);
        let marks: Vec<&Value> = rev["changes"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter(|one| one["paragraph_mark"].as_bool() == Some(true))
            .collect();
        assert_eq!(marks.len(), 1, "{rev}");
        assert_eq!(marks[0]["text"], "", "段落标记自己没有正文");
        assert_eq!(marks[0]["author"], "张三");
        assert_eq!(marks[0]["paragraph"], 3);
        // 没被拆开，所以每条就是一个元素
        assert!(rev["changes"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["elements"] == 1));
        assert!(rev["notes"].as_array().expect("有 notes").is_empty());
    }

    /// 同一批改动在 ODF 里的样子：region 自己就是逻辑改动（LO 已经把拆开的那份合回去了），
    /// 所以 4 处与 OOXML 那边合成后的 4 条对上；日期少一个 `Z`、格式改动带着字，
    /// 是这两个格式自己的差别，不替文件统一
    #[test]
    fn the_same_revisions_read_from_the_opendocument_side() {
        let out = run("revisions.odt");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 4, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 2);
        assert_eq!(rev["elements"]["deletions"], 1);
        assert_eq!(rev["elements"]["format_changes"], 1);
        assert_eq!(rev["paragraph_marks"], 0);
        assert_eq!(rev["track_changes"], json!(false));
        let changes = rev["changes"].as_array().expect("是数组");
        assert_eq!(
            changes
                .iter()
                .map(|one| one["kind"].as_str().unwrap_or(""))
                .collect::<Vec<_>>(),
            vec!["insertion", "deletion", "format-change", "insertion"]
        );
        assert_eq!(changes[0]["text"], "124000 元", "插入的字在正文的区间里");
        assert_eq!(changes[0]["date"], "2026-03-05T09:12:00");
        assert_eq!(changes[1]["text"], "89000 元", "删掉的字在 region 里");
        assert_eq!(
            changes[2]["text"], "，请复核。",
            "ODF 的格式改动带着被改的字"
        );
        assert_eq!(changes[3]["text"], "整段是新加的。");
        assert_eq!(changes[0]["paragraph"], 1, "标题是 text:h，不占正文段的号");
        assert_eq!(changes[3]["paragraph"], 2);
        assert_eq!(
            out["structure"]["paragraphs"], 3,
            "region 里那份删掉的段不算正文"
        );
        let authors = rev["authors"].as_array().expect("是数组");
        assert_eq!(authors.len(), 3, "{authors:?}");
        assert_eq!(authors[0]["name"], "张三");
        assert_eq!(authors[0]["changes"], 2);
    }

    /// 「这份能动吗」：docx 的保护写在 `word/settings.xml`，与修订是两份账。
    /// 两份 fixture 都要读出来（一份 python-docx 手注入、一份 LibreOffice 重写）
    #[test]
    fn protection_says_which_kind_of_editing_is_restricted() {
        for name in ["protected.docx", "protected-lo.docx"] {
            let out = run(name);
            let one = &out["protection"];
            assert_eq!(one["element"], json!(true), "{name}: {one}");
            assert_eq!(one["protected"], json!(true), "{name}");
            assert_eq!(one["edit"], json!("readOnly"), "{name}");
            assert_eq!(one["password"], json!(true), "有 hash 就是设了口令：{name}");
            assert_eq!(one["algorithm"], json!("typeAny"), "{name}");
            assert_eq!(one["spin_count"], json!("100000"), "{name}");
        }
        assert_eq!(run("notes.docx")["protection"]["element"], json!(false));
    }

    /// ODF 那一侧：文档级保护在 `settings.xml` 的 config-item 上，而 docx 的编辑限制
    /// **不会**跟着转换过来（这条是 LibreOffice 实测，见 fixture README）
    #[test]
    fn the_odt_side_does_not_carry_a_docx_restriction() {
        let out = run("protected.odt");
        let one = &out["protection"];
        assert_eq!(one["protected"], json!(false), "{one}");
        assert_eq!(one["items"]["ProtectForm"], json!(false));
        assert_eq!(one["items"]["ProtectBookmarks"], json!(false));
        assert_eq!(one["items"]["ProtectFields"], json!(false));
        assert_eq!(run("notes.odt")["protection"]["protected"], json!(false));
        // 遗留 .doc 读不出：给 null，不假称「没保护」
        assert!(run("notes.doc")["protection"].is_null(), "读不出就说读不出");
    }

    /// LibreOffice 从 RTF 导入写出的那份脚注样本：`word/footnotes.xml` 里有四条
    /// `w:footnote`，其中两条是 `separator` / `continuationSeparator` —— 它们是
    /// 排版用的占位，不是文档里的注（期望值来自 `office_reader.py` 的 docx_facts）
    #[test]
    fn footnote_separators_are_not_counted_as_footnotes() {
        let out = run("notes-foot.docx");
        assert_eq!(out["footnotes"], 2, "{out}");
        assert_eq!(out["endnotes"], 0, "这份文件根本没有 endnotes.xml");
        assert_eq!(out["comments"], 0);
        assert_eq!(out["structure"]["paragraphs"], 4, "注的字不在正文里");
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
        assert_eq!(s["has_numbering"], json!(false), "正文里没用编号：{s}");
        assert_eq!(s["numbering_part"], json!(true), "包里有编号部件：{s}");
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
        // 「多少字」两份账：自己数的与生产者自报的。python-docx 写了 app.xml 却
        // 一个数都没数（Words/Characters 全是 0），所以那一份只能照抄不能当答案
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 67, "characters_no_spaces": 66, "words_by_space": 10}),
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["words"], 0);
        assert_eq!(out["statistics"]["producer"]["pages"], 1);
        assert_eq!(out["images"], json!(["word/media/image1.png"]));
        assert_eq!(out["hyperlinks"][0]["target"], "https://example.com/budget");
        assert_eq!(out["hyperlinks"][0]["external"], json!(true));
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// ODT 的结构账：段落口径与 OOXML 一致（表格里也算段），但**批注里的段不算** ——
    /// ODF 的 `text:annotation` 嵌在正文段里面，LibreOffice 自报 10 段正是因为它把
    /// 批注里那一段也数了进去；标题层级在 `text:outline-level`，
    /// 脚注与尾注共用一个 `text:note`（这份两个都没写所以是 0）。
    /// 期望值全部来自 `odt_structure()`。
    #[test]
    fn an_opendocument_text_document_is_accounted_for() {
        let out = run("notes.odt");
        assert_eq!(out["kind"], "opendocument-text");
        assert_eq!(
            out["structure"]["paragraphs"], 9,
            "正文段不含批注里那一段：{out}"
        );
        assert_eq!(out["structure"]["empty_paragraphs"], 2, "{out}");
        assert_eq!(
            out["statistics"]["producer"]["paragraph-count"], "10",
            "生产者的 10 = 我们的 9 + 批注里那一段：口径差要在两边都说得清"
        );
        // 字符数两边完全一致（我们与 LibreOffice 各数各的），词数不一致是口径：
        // 它按中文词切（61），我们只按空白切（10）—— 所以键名叫 words_by_space
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 67, "characters_no_spaces": 66, "words_by_space": 10}),
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["character-count"], "67");
        assert_eq!(out["statistics"]["producer"]["word-count"], "61");
        // 同一批字在 docx 与 odt 两边数出来必须一样（这份 odt 就是从那份 docx 转的）
        assert_eq!(
            out["statistics"]["ours"],
            run("notes.docx")["statistics"]["ours"]
        );
        assert_eq!(out["styles"]["Standard"], 8, "{out}");
        assert_eq!(out["styles"]["P2"], 1);
        assert_eq!(
            out["headings"],
            json!([
                {"level": 1, "text": "一级标题：预算口径"},
                {"level": 2, "text": "二级标题：明细"}
            ]),
            "{out}"
        );
        assert_eq!(out["structure"]["tables"], 1);
        assert_eq!(out["structure"]["table_rows"], 2);
        assert_eq!(out["structure"]["table_cells"], 4);
        assert_eq!(out["tables"][0]["name"], "表格1", "{out}");
        assert_eq!(out["comments"], 1, "text:annotation 就是批注");
        assert_eq!(out["footnotes"], 0, "一个 text:note 都没有");
        assert_eq!(out["endnotes"], 0);
        assert_eq!(out["hyperlinks"][0]["target"], "https://example.com/budget");
        assert_eq!(out["hyperlinks"][0]["text"], "预算制度");
        assert_eq!(out["structure"]["drawings"], 1, "那张图包在 draw:frame 里");
        assert_eq!(
            out["images"][0], "Pictures/1000000100000008000000088E4DF5D4.png",
            "{out}"
        );
        assert_eq!(out["structure"]["sequences"], 5, "五个页码/章节变量声明");
        assert_eq!(out["structure"]["tracked_changes"], 0);
        assert_eq!(
            out["statistics"]["producer"]["paragraph-count"], "10",
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["page-count"], "2");
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
        // 段落数 = 正文里的硬回车数：独立读者对着 piece 表还原出的 139 个字符数到 10 个 \r
        assert_eq!(out["structure"]["paragraphs"], 10, "{out}");
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
