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
    about = "Report the structure of a Word document: paragraph count (empty ones counted separately, because Word's own statistics do), headings with their level and text, every style used and how often, tables with rows and cells, inline shapes and pictures, hyperlinks split into internal and external with their targets, sections, explicit page/column breaks, footnotes and endnotes and comments (read from their own parts when present), tracked-change presence (w:ins / w:del counts) plus a revision ledger (revisions): one entry per logical change with its kind, author, date, the paragraph index it sits in and the words it carries - elements are merged only when adjacent with the same kind/author/date/paragraph, because a producer writes one edit as several runs (LibreOffice splits the number from the unit into two w:ins), while the ODF export of the very same file states them as one changed-region, which is what this merge rule was measured against. Paragraph-mark insertions (w:pPr/w:rPr/w:ins) are counted apart from the paragraph's text and are not merged with it; ODF keeps deleted words inside the region and inserted words between text:change-start and text:change-end in the body, and both are read. Legacy .doc reports revisions as null rather than guess (the redline tables live in the table stream, not the piece table). protection (docx: w:documentProtection in word/settings.xml - w:edit says what kind of editing is restricted and w:enforcement says whether it is on; odt: the ProtectForm/ProtectBookmarks/ProtectFields config-items in settings.xml, which is a different place and does NOT carry the docx restriction across - the same document converted to .odt reports false for all three, measured); numbering usage, headers and footers, embedded objects and custom XML, plus which optional parts the package actually carries. statistics answers 'how many words/pages': ours (characters, characters_no_spaces and words_by_space - the last split on whitespace only, which is why it is named that way and not 'words') next to the producer's own numbers (docx docProps/app.xml, ODF meta.xml document-statistic) because the two disagree by design - python-docx writes app.xml with Words/Characters at 0 (it never counted), and LibreOffice counts Chinese words rather than whitespace runs, while on the same text our character counts match its character-count exactly. For legacy .doc it falls back to what the piece table can honestly tell: paragraph count from the CP total plus the marker characters it dropped (cell ends, field boundaries), not a claim about tables it cannot see. For .odt it reports the same account from content.xml's office:text (headings from text:outline-level, comments from text:annotation, pictures from draw:image, footnotes and endnotes split out of the single text:note element by its note:class) and additionally echoes meta.xml's own document-statistic so the producer's numbers are visible next to ours. `contents` answers 'is there a table of contents and how many levels does it pull in', reported per family because the two spellings share nothing: OOXML wraps a w:sdt whose docPartGallery reads Table of Contents (Word and LibreOffice both write it) and keeps the levels INSIDE the field instruction text - a form like TOC \\o \"1-2\" \\h, with the producer's own quoting - while the wrapper can also be absent and only the field present, so both are looked for; ODF keeps a text:table-of-content block whose name is on text:name and whose level is the source element's outline-level attribute, and LibreOffice additionally writes all ten entry templates whether or not they are used (entry_templates reports what is written, not what is used). A file without one reports present false - false, not missing; legacy .doc reports null because this reader does not look there. RTF is not a package but one stream, so that branch answers with only what the stream itself proves: structure.paragraphs is the lines the par control word cuts, footnotes and endnotes are counted apart from their destination groups (an endnote is a footnote group that additionally carries ftnalt), and pictures / embedded_objects / skipped_destinations / note_destinations / page_destinations come from the same walk, while sections, contents, comments, revisions and protection stay null - null means 'this reader did not look', 0 would mean 'there are none'. Styles and fonts are read out of the fonttbl and stylesheet groups with a lookahead: those groups are still skipped as far as the body is concerned (so skipped_destinations did not move when this was added), but their entries come back as styles (name to how many paragraphs use it, taken from the body's own \\sN) plus style_definitions / font_definitions counts and a font_list; a font name written in a non-ANSI charset with non-ASCII bytes comes back as name null with that charset number, because decoding those as cp1252 would be a made-up name. Tables are the interesting middle case there: table_rows and table_cells ARE reported (they are simply how many times the row and cell control words appear, and on two measured files those counts match the same document's docx and odt ledgers exactly), while tables itself stays null because the rule for grouping rows into separate tables was tried against one single-table file and one two-table file and counted two as one. Links come from a lookahead into the field group (the HYPERLINK address inside the instruction, plus the display text of the result group - which stays in the body, because that is what the page shows), and fields counts how many field groups the stream holds since page numbers and dates are fields too but are not links. Returns { path, format, kind, structure, styles, tables, images, hyperlinks, contents, revisions, protection, statistics, parts, notes }. Read-only (safety T0)."
)]
pub struct OfficeDoc {
    /// Word 文档（docx / docm / doc / odt / rtf）
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

/// 域指令里 `\o "1-2"` 那一对引号之间的字（`&quot;` 在解析时已经还原成 `"`）
fn switch_value(instruct: &str, switch: &str) -> Option<String> {
    let at = instruct.find(switch)? + switch.len();
    let rest = &instruct[at..];
    let start = rest.find('"')? + 1;
    let stop = rest[start..].find('"')? + start;
    Some(rest[start..stop].to_string())
}

/// 这份 docx 有没有目录、收了几级。OOXML 的目录有两种长相：
/// `w:sdt` 套着 `docPartGallery="Table of Contents"`（Word 与 LibreOffice 都这么写），
/// 或者一条 `TOC \o "1-2" \h` 的域指令（可以没有那个壳）。**「几级」写在域指令的文字里**，
/// 不是某个属性上 —— 与 ODF 那边是两种说法，所以两边各报各的，不强行统一
fn docx_contents(root: &xmlscan::Node) -> Value {
    let galleries: Vec<String> = root
        .descendants("docPartGallery")
        .iter()
        .filter_map(|one| one.attr_local("val").map(String::from))
        .collect();
    let mut instructions: Vec<String> = root
        .descendants("instrText")
        .iter()
        .map(|one| one.text().trim().to_string())
        .collect();
    for one in root.descendants("fldSimple").iter() {
        if let Some(had) = one.attr_local("instr") {
            instructions.push(had.trim().to_string());
        }
    }
    let fields: Vec<String> = instructions
        .into_iter()
        .filter(|one| one.to_uppercase().starts_with("TOC"))
        .collect();
    let gallery = galleries.iter().any(|one| one == "Table of Contents");
    json!({
        "present": gallery || !fields.is_empty(),
        "via": if gallery {
            json!("doc-part-gallery")
        } else if fields.is_empty() {
            Value::Null
        } else {
            json!("field")
        },
        "galleries": galleries,
        "fields": fields,
        "levels": fields.iter().find_map(|one| switch_value(one, r"\o ")),
        "sdt": root.descendants("sdt").len(),
    })
}

/// ODF 的目录：`text:table-of-content` 那一块。名字、受不受保护在元素属性上，
/// 「收几级」写在 `text:table-of-content-source` 的 `outline-level` 上 ——
/// 与 OOXML 把这一切塞进域指令文字正好是两种写法
fn odf_contents(root: &xmlscan::Node) -> Value {
    let blocks = root.descendants("table-of-content");
    if blocks.is_empty() {
        return json!({
            "present": false, "names": [], "outline_level": Value::Null,
            "entry_templates": 0, "title": Value::Null,
        });
    }
    json!({
        "present": true,
        "names": blocks
            .iter()
            .filter_map(|one| one.attr_local("name").map(String::from))
            .collect::<Vec<String>>(),
        "outline_level": root
            .descendants("table-of-content-source")
            .first()
            .and_then(|one| one.attr_local("outline-level"))
            .map(String::from),
        "entry_templates": root.descendants("table-of-content-entry-template").len(),
        "title": root
            .descendants("index-title-template")
            .first()
            .map(|one| one.text().trim().to_string())
            .filter(|had| !had.is_empty()),
    })
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
            "contents": docx_contents(&root),
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
            "contents": odf_contents(&root),
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
    } else if doc.family == Family::Rtf {
        // RTF 不是包，是一条流：能给的是段（`\par` 切的）、注、图与嵌入对象，
        // 样式名 / 表格线 / 分节归属 / 目录都不判，那些项给 null 而不是 0
        let one = crate::rtf::extract(bytes);
        let mut tally = Tally::default();
        for line in &one.lines {
            tally.add(line);
        }
        let footnotes = one
            .note_list
            .iter()
            .filter(|had| had["kind"] == json!("footnote"))
            .count();
        let endnotes = one
            .note_list
            .iter()
            .filter(|had| had["kind"] == json!("endnote"))
            .count();
        let mut notes = one.notes.clone();
        notes.push(
            "RTF 这一支只报流里数得清的东西：段落按 par 控制字切，注按目标群分\
             （尾注靠群里的 ftnalt 判），表给行数与格子数 —— 那两个数就是 row 与 cell \
             这两个控制字的条数（嵌套表的 nestrow / nestcell 另给，不混进去）。\
             「几张表」要判行与行之间的段落边界，这条规则拿一张表与两张表的对照件试过：\
             单表对、两张数成一张，所以 tables 留 null。链接是从 field 群里前瞻读出来的\
             （指令里的 HYPERLINK 地址与 fldrslt 的显示文字，显示文字照旧留在正文里），\
             域的总条数另给 fields。字体名与样式名从 fonttbl / stylesheet 那两群里读 \
             —— 那两群照旧整群跳过（一个字不进正文），所以 skipped_destinations 不因为这个\
             改动而变，只是读之前里面的名字从来没被交出来过。样式表写了多少条、字体表列了\
             几个字体也各交一个数；非 ANSI 字符集又含非 ASCII 字节的字体名交回 null\
             （条目自己那个 fcharset 号说明为什么），\
             不交一个我们按 cp1252 解出来的乱码。批注与目录也不判，\
             那些项同样是 null —— null 是「没看」或「判不住」，不是「这份文件没有」"
                .to_string(),
        );
        let mut style_tally: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for had in &one.style_uses {
            let name = had["name"].as_str().unwrap_or_default().to_string();
            *style_tally.entry(name).or_insert(0) +=
                usize::try_from(had["count"].as_u64().unwrap_or(0)).unwrap_or(0);
        }
        let hyperlinks: Vec<Value> = one
            .links
            .iter()
            .take(limit)
            .map(|had| {
                let target = had["target"].as_str().unwrap_or_default();
                json!({
                    "target": target,
                    // 站外与站内按地址自己说：这一族没有关系表可查
                    "external": target.contains("://"),
                    "text": had["text"],
                })
            })
            .collect();
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "rtf",
            "structure": {
                "paragraphs": one.lines.len(),
                "empty_paragraphs": Value::Null,
                // 「几张表」判不住（见上面那条说明）；行数与格子数是控制字的条数
                "tables": Value::Null,
                "table_rows": one.table_rows,
                "table_cells": one.table_cells,
                "table_row_defines": one.table_row_defines,
                "table_cell_paras": one.table_cell_paras,
                "nested_table_rows": one.nested_table_rows,
                "nested_table_cells": one.nested_table_cells,
                "sections": Value::Null,
                "breaks": Value::Null,
                "page_breaks": Value::Null,
                "drawings": Value::Null,
                "text_boxes": Value::Null,
                "bookmarks": Value::Null,
                // 域比链接多：页码与日期也是域，所以两个数分开交
                "fields": one.fields,
                "has_numbering": Value::Null,
                "numbering_part": Value::Null,
                "pictures": one.pictures,
                "embedded_objects": one.embedded_objects,
                "skipped_destinations": one.skipped_destinations,
                "note_destinations": one.note_destinations,
                "page_destinations": one.page_destinations,
                // 样式表写了多少条、字体表列了几个字体（用了几个样式看 styles）
                "style_definitions": one.styles.len(),
                "font_definitions": one.fonts.len(),
            },
            "headings": [],
            "styles": json!(style_tally),
            "font_list": one.fonts.iter().take(limit).cloned().collect::<Vec<Value>>(),
            "tables": [],
            "images": [],
            "hyperlinks": hyperlinks,
            "footnotes": footnotes,
            "endnotes": endnotes,
            "contents": Value::Null,
            "comments": Value::Null,
            // RTF 的修订（\strip / \on 那些）与红线都不在这一版里
            "revisions": Value::Null,
            "protection": Value::Null,
            "statistics": {
                "ours": tally.to_json(),
                "producer": Value::Null,
            },
            "parts": [],
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
            // 这一版不读 .doc 的目录，所以给 null —— 是「没看」，不是「这份文档没有目录」
            "contents": null,
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

    /// 尾注那一条分支第一次有真件可走。这份是 LibreOffice 的 docx 导出器写的
    /// （Writer 自己没有尾注概念，RTF 的 `\endnote` 在导入时就被摊进正文，
    /// 所以只有「注进去再让它照抄」这条路）：部件里三条 `w:endnote`，
    /// 它自己写的两条分隔符是 `<w:separator/>` / `<w:continuationSeparator/>` 那种写法。
    /// 期望值来自 `office_reader.py` 的 docx_facts
    #[test]
    fn endnotes_are_counted_from_a_part_the_producer_wrote() {
        let out = run("notes-end.docx");
        assert_eq!(out["endnotes"], json!(1), "{out}");
        assert_eq!(out["footnotes"], json!(2), "同一份文件里脚注仍占两条");
        assert_eq!(out["comments"], json!(0));
        assert_eq!(out["structure"]["paragraphs"], 4, "尾注的字不算正文");
        assert!(
            out["parts"]
                .as_array()
                .expect("parts 是数组")
                .iter()
                .any(|one| one == "word/endnotes.xml"),
            "{:?}",
            out["parts"]
        );
        // 同一件事在遗留 .doc 那一支只能说「看不见」：注住在表流的另一段，给 null
        assert!(
            run("notes.doc")["endnotes"].is_null(),
            ".doc 看不见就交回 null"
        );
        // ODF 那一支也第一次有真尾注：LibreOffice 的 ODT 导出器写
        // `text:note-class="endnote"`（编号还换成罗马数字），与 OOXML 那份同一笔账
        let odt = run("notes-end.odt");
        assert_eq!(odt["kind"], "opendocument-text", "{odt}");
        assert_eq!(odt["endnotes"], json!(1), "{odt}");
        assert_eq!(odt["footnotes"], json!(2), "同一份 ODT 里脚注仍占两条");
    }

    /// 「这份文档有没有目录、收了几级」的第一批真件。两家存法根本不同：
    /// OOXML 把级别写在域指令的文字里（`TOC \o "1-2" \h`，外面套一层
    /// `w:sdt` + `docPartGallery="Table of Contents"`），ODF 写在
    /// `text:table-of-content-source` 的 `outline-level` 属性上 —— 所以各报各的。
    /// 两份件都是 LibreOffice 的导出器写的（目录注进 docx 让它照抄，见 office_fixtures.py）
    /// （期望值来自 `office_reader.py` 的 `docx_contents()` / `odf_contents()`）
    #[test]
    fn a_table_of_contents_is_reported_whichever_way_the_file_keeps_it() {
        let doc = run("toc.docx");
        let contents = &doc["contents"];
        assert_eq!(contents["present"], json!(true), "{contents}");
        assert_eq!(contents["via"], json!("doc-part-gallery"), "{contents}");
        assert_eq!(
            contents["levels"],
            json!("1-2"),
            "几级写在域指令里：{contents}"
        );
        assert_eq!(contents["sdt"], json!(1), "{contents}");
        assert_eq!(
            contents["fields"],
            json!(["TOC \\o \"1-2\" \\h"]),
            "域指令原文照文件写的交（引号是 LO 写成 &quot; 的那对）：{contents}"
        );
        let odt = run("toc.odt");
        let got = &odt["contents"];
        assert_eq!(got["present"], json!(true), "{got}");
        assert_eq!(got["names"], json!(["目录1"]), "{got}");
        assert_eq!(
            got["outline_level"],
            json!("2"),
            "ODF 的级别在属性上：{got}"
        );
        assert_eq!(got["title"], json!("目录"), "{got}");
        assert_eq!(
            got["entry_templates"],
            json!(10),
            "LO 十级模板都写出来，不管用不用得上：{got}"
        );
        // 反面对照：没目录的件报 present=false（键在、值为假），而不是 null
        assert_eq!(run("notes.docx")["contents"]["present"], json!(false));
        assert_eq!(run("notes.odt")["contents"]["present"], json!(false));
        assert!(run("notes.doc")["contents"].is_null(), ".doc 没看就给 null");
    }

    /// RTF 也终于有这一问了：它不是包，是一条流 —— 数得清的是段（par 切的行）、
    /// 注（按目标群，尾注靠群里的 ftnalt）、图与嵌入对象；样式名、表格线、目录都不判，
    /// 那些项交回 null（「没看」）而不是 0（「没有」）
    /// （期望值来自 `lyco_rtf.py` 的 rtf_text）
    #[test]
    fn rtf_answers_structure_with_only_what_the_stream_proves() {
        let out = run("notes-end.rtf");
        assert_eq!(out["kind"], "rtf", "{out}");
        assert_eq!(out["structure"]["paragraphs"], 4, "{out}");
        assert_eq!(out["footnotes"], 2, "两条脚注：{out}");
        assert_eq!(out["endnotes"], 1, "一条尾注，靠 ftnalt 判：{out}");
        assert_eq!(out["structure"]["note_destinations"], 3, "{out}");
        assert_eq!(out["structure"]["pictures"], 0, "{out}");
        assert!(out["structure"]["tables"].is_null(), "没看就是 null：{out}");
        // 样式不再交 null：那一群里写着什么就用什么（样式表自己写了 16 条）
        assert_eq!(
            out["styles"],
            json!({"Normal": 4}),
            "正文里只用了 Normal：{out}"
        );
        assert_eq!(out["structure"]["style_definitions"], 16, "{out}");
        assert_eq!(out["structure"]["font_definitions"], 9, "{out}");
        assert!(out["contents"].is_null(), "{out}");
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 116, "characters_no_spaces": 100, "words_by_space": 20}),
            "{out}"
        );
        // 另一份带一张图的：注是零条，图数得出来，表的行与格子也数得出来
        let pic = run("notes.rtf");
        assert_eq!(pic["structure"]["pictures"], 1, "{pic}");
        assert_eq!(pic["structure"]["paragraphs"], 7, "{pic}");
        assert_eq!(pic["footnotes"], 0);
        assert_eq!(pic["endnotes"], 0);
        assert_eq!(pic["structure"]["table_rows"], 2, "{pic}");
        assert_eq!(pic["structure"]["table_cells"], 4, "{pic}");
        assert_eq!(pic["structure"]["table_row_defines"], 2, "{pic}");
        assert_eq!(pic["structure"]["table_cell_paras"], 4, "{pic}");
        // 页眉页脚那份：口袋数也要交，注仍是零，表也仍是零
        let hf = run("notes-hf.rtf");
        assert_eq!(hf["structure"]["page_destinations"], 6, "{hf}");
        assert_eq!(hf["structure"]["paragraphs"], 3, "{hf}");
        assert_eq!(hf["structure"]["table_rows"], 0, "{hf}");
        assert_eq!(hf["structure"]["table_cells"], 0, "{hf}");
        assert_eq!(hf["structure"]["fields"], 0, "{hf}");
        // 链接：那一个 field 群里的 HYPERLINK 地址与显示文字，
        // 显示文字照旧算正文的一行（少这一条就会「读到链接、丢了字」）
        let links = run("notes.rtf");
        assert_eq!(
            links["hyperlinks"][0]["target"], "https://example.com/budget",
            "{links}"
        );
        assert_eq!(links["hyperlinks"][0]["external"], json!(true), "{links}");
        assert_eq!(links["hyperlinks"][0]["text"], "预算制度", "{links}");
        assert_eq!(links["structure"]["fields"], 1, "{links}");
        assert!(
            links["notes"]
                .as_array()
                .expect("有说明")
                .iter()
                .any(|one| one.as_str().unwrap_or_default().contains("fldrslt")),
            "说明里要讲清链接是从哪读的：{links}"
        );
        // 同一批字的两种存法：地址必须一模一样
        assert_eq!(
            links["hyperlinks"][0]["target"],
            run("notes.docx")["hyperlinks"][0]["target"],
            "同一批字的 docx 与 rtf 两份链接账"
        );
    }

    /// RTF 的表：行数与格子数是控制字的条数（同一份文档的 docx 与 odt 两副账给一样的数），
    /// 「几张表」却判不住 —— 那条把连续的 row 定义算成一张表的规则，在一份单表件上对、
    /// 在一份两张表（3×2 与 2×2）的件上把两张数成一张，所以 tables 交回 null 并写明理由。
    /// 期望值来自 `lyco_rtf.py` 的 rtf_text 与 `office_reader.py` 的 docx_facts / odt_facts
    #[test]
    fn rtf_counts_table_rows_and_cells_but_not_tables() {
        let out = run("tables.rtf");
        assert_eq!(out["kind"], "rtf", "{out}");
        assert_eq!(out["structure"]["paragraphs"], 10, "{out}");
        assert_eq!(out["structure"]["table_rows"], 5, "五个 row 结束符：{out}");
        assert_eq!(out["structure"]["table_cells"], 10, "十个 cell：{out}");
        assert_eq!(
            out["structure"]["table_row_defines"], 5,
            "五个 trowd：{out}"
        );
        assert_eq!(
            out["structure"]["table_cell_paras"], 10,
            "十个 intbl：{out}"
        );
        assert_eq!(out["structure"]["nested_table_rows"], 0, "{out}");
        assert_eq!(out["structure"]["nested_table_cells"], 0, "{out}");
        assert!(
            out["structure"]["tables"].is_null(),
            "几张表判不住，就留 null：{out}"
        );
        // 同一批字的另两副账：那一份是敢报表数的
        for name in ["tables.docx", "tables.odt"] {
            let other = run(name);
            assert_eq!(other["structure"]["tables"], 2, "{name}：{other}");
            assert_eq!(other["structure"]["table_rows"], 5, "{name}");
            assert_eq!(other["structure"]["table_cells"], 10, "{name}");
        }
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
