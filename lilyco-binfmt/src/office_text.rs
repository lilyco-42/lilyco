//! `lbin office-text` — 把办公文件读成人能读的文本。
//!
//! 四条路，各按各的格式说「一段」是什么：
//! - **WordprocessingML**（docx / docm）：`word/document.xml` 里每个 `w:p` 一段，段内按
//!   文档顺序拼 `w:t`，并把 `w:tab` / `w:br` 还原成制表与换行 —— 与 Word 自己的段落口径
//!   一致（表格单元格里的段也算段，Word 统计字数时同样数它们）；
//! - **PresentationML**（pptx / pptm）：按幻灯片编号顺序，逐个形状（`p:sp`）出段，
//!   形状带 `<a:ph type="title">` 的那一段标成标题；表格形状（`p:graphicFrame`）里的文字
//!   也读，备注页（`notesSlideN.xml`）单独标 `notes` —— 演讲者看到的字与页面上的字是两回事；
//! - **SpreadsheetML**（xlsx / xlsm）：网格没有「段落」，给的是**有值的单元格**
//!   （含内联字符串与共享字符串两种存法），并说明公式单元格里读到的是公式而不是缓存值；
//! - **ODF / RTF**：ODF 读 `content.xml` 的 `text:p` 与 `text:h`（层级看 `text:outline-level`）；
//!   `.ods` 例外——它跟 xlsx 一样按格子交账（表名 + A1 位置 + 值类型），
//!   因为「一份表格文件的正文」就是它那些格子；
//!   RTF 走 [`crate::rtf`] 的目标群感知提取，不是「把控制字删掉」就算完。
//!
//! 遗留二进制格式走另两条路：`.doc` 的正文位置在 FIB 指向的 **piece 表** 里（见 [`crate::word]`），
//! `.xls` 的文本集中在 **SST**（可跨 CONTINUE 边界，见 [`crate::biff]`）；`.ppt` 的文本
//! 散在一棵记录树的原子（0x0FA0 / 0x0FA8 / 0x0FBA）里，见 [`crate::ppt]` —— 那里连「这是
//! 不是一层容器」都要靠「正文能不能再铺成一条完整记录流」来判。读不出来的东西照实返回
//! `kind: "unsupported"` 加一句为什么，
//! 把读不出来报成「文档是空的」是假答案。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, Family};
use crate::read::read_blob;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, Member, DEFAULT_MEMBER_CAP};

/// 读出办公文件的正文文本（T0 只读）
#[derive(App)]
#[app(
    name = "office-text",
    run = "run_office_text",
    about = "Read the human-readable text an office document actually contains. docx/docm: one entry per w:p in word/document.xml (Word's own paragraph notion, table cells included, w:tab and w:br restored), then the parts that are not in the body at all - comments, footnotes, endnotes, headers and footers - each entry carrying from/part/author/date so a comment never reads like body text. pptx/pptm: slides in numeric order, one entry per paragraph inside each shape, entries flagged title when the shape has a:ph type=title and separately flagged notes for notesSlideN.xml. xlsx/xlsm: one entry per valued cell with its reference and sheet name, covering both inline strings and the shared-string table, with formula cells reported as the formula because the file carries no cached result. odt/ods/odp: text:p and text:h from content.xml with outline levels - except .ods, which answers like a spreadsheet does: one entry per non-empty cell with its sheet name, A1-style reference and declared value type (the text shown in the cell, not office:value). ODF comments are text:annotation elements nested INSIDE a body paragraph (docx keeps them in a separate part), so they are emitted as their own entries carrying from/author/date read from their meta:creator and meta:date children, and the paragraph that holds one reports only its own text. An .odt's page headers and footers are not in content.xml either - they sit in styles.xml under style:master-page (a document with two sections has two master pages, and left/right/first-page variants are separate slots), so they are read there and flagged from=header/footer with the master-page name and slot. rtf: a destination-aware extractor that drops font/color/stylesheet tables and field instructions instead of leaking control words into the text; its page headers and footers live in the SAME stream as the body (only the destination groups named header / headerl / headerf / footer say which), so they are separated out and flagged from=header/footer with the slot name - one header often appears in several slots, which is reported as the file writes it rather than merged away. Returns { path, format, app, kind, paragraphs: [{index, text, heading?, style?, slide?, sheet?, ref?, notes?, part}], line_count, chars, total_paragraphs, total_chars, cut, parts_read, notes } and cuts output at max_chars while still reporting full totals, so a silent truncation is impossible. Legacy .doc answers with its real paragraphs by walking the FIB piece table (the per-piece compression bit halves fc); legacy .xls answers with the shared-string table plus its sheet list and visibility; a .ppt (PowerPoint 97 record tree) answers kind=record-tree with every text atom found by walking the tree (master placeholders included, because they really are in the file). A .pdf answers kind=pages: content streams are inflated, glyph codes mapped through each font's /ToUnicode CMap, and lines rebuilt from the text positions (BT resets the matrix, glyph advance moves x only, a TJ number pushes the pen the opposite way), so a Chinese heading comes back as words rather than as its characters shuffled; an encrypted PDF returns no paragraphs and says why, because its streams are ciphertext and this domain does not decrypt. "
)]
pub struct OfficeText {
    /// 办公文件
    #[arg(about = "Office document to read", must_exist = true)]
    path: PathBuf,

    /// 最多交出来多少字符（总数照实给）
    #[arg(about = "Emit at most this many characters", default = 20000, min = 1)]
    max_chars: u64,

    /// 连空段一起给（默认只给有字的段）
    #[arg(about = "Include empty paragraphs too")]
    keep_empty: bool,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// 与 `#[arg(default = 20000)]` 同一个数：Web / MCP 省略时不能变成「0 个字符」
const MAX_CHARS_DEFAULT: usize = 20000;

fn run_office_text(app: &OfficeText, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the document text".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let mut paragraphs: Vec<Value> = Vec::new();
    let mut parts_read: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut kind: &'static str = "unsupported";

    match doc.family {
        Family::Other if crate::pdf::is_pdf(bytes) => {
            // PDF 不是容器，是一张对象表 —— 读法在 `crate::pdf`，那条路径上的
            // 「按页树的顺序 + 字形码经 /ToUnicode + 按位置拼行」全在同一处实现。
            // 这里接过来，是因为拿一份 .pdf 问「它写了什么」是最常见的一问，
            // 回一句「认不出来」不是诚实，是漏答。
            kind = "pages";
            let book = crate::pdf::Pdf::read(bytes);
            let (pages, from_tree) = book.page_texts(bytes);
            if !from_tree {
                notes.push("页树走不通（缺 /Root 或 /Kids）：页序退成按对象号排".to_string());
            }
            for (id, one) in &pages {
                for line in one.lines() {
                    push_paragraph(
                        &mut paragraphs,
                        app.keep_empty,
                        line,
                        json!({"page_object": id, "part": "content stream"}),
                    );
                }
            }
            parts_read.push(format!("{} 页内容流", pages.len()));
            if book.encryption.is_some() {
                notes.push(
                    "加密的 PDF：内容流是密文，正文解不出来（本域不解密，也没有口令）".to_string(),
                );
            } else if paragraphs.is_empty() {
                notes.push("没读到字：这份 PDF 的正文可能是图（扫描件），本域不做 OCR".to_string());
            }
        }
        Family::Ooxml if doc.app == "word" => {
            kind = "paragraphs";
            let part = "word/document.xml";
            match read(bytes, part) {
                Some(member) => {
                    parts_read.push(part.to_string());
                    let root = xmlscan::parse_str(&member.as_text());
                    for one in root.descendants("p").iter() {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            &run_text(one),
                            json!({"heading": heading_level(one), "style": paragraph_style(one), "part": part}),
                        );
                    }
                }
                None => notes.push("包里读不到 word/document.xml".to_string()),
            }
            // 正文之外的那几类部件：审阅的人要的常常就是批注与脚注，而它们各自是
            // 一个部件，不在 document.xml 里。页眉页脚同理（按部件名字里的数字排）。
            let mut side: Vec<(String, &'static str)> = Vec::new();
            for one in doc.entries.iter() {
                let name = one.name.as_str();
                let Some(base) = name.rsplit('/').next() else {
                    continue;
                };
                if !name.starts_with("word/") || !base.ends_with(".xml") {
                    continue;
                }
                let stem = &base[..base.len() - 4];
                let what = if stem == "footnotes" {
                    "footnote"
                } else if stem == "endnotes" {
                    "endnote"
                } else if stem == "comments" {
                    "comment"
                } else if stem.starts_with("header") {
                    "header"
                } else if stem.starts_with("footer") {
                    "footer"
                } else {
                    continue;
                };
                side.push((name.to_string(), what));
            }
            side.sort();
            for (part, what) in side.iter() {
                let what: &'static str = *what;
                let Some(member) = read(bytes, part) else {
                    notes.push(format!("{part} 读不出来"));
                    continue;
                };
                parts_read.push(part.clone());
                let root = xmlscan::parse_str(&member.as_text());
                // 批注/脚注/尾注是「容器元素 + 里面的段」，作者与时间挂在容器上；
                // 页眉页脚没有这一层，直接走它的段。
                let owners: Vec<&xmlscan::Node> = match what {
                    "comment" | "footnote" | "endnote" => root.descendants(what),
                    _ => vec![&root],
                };
                for owner in owners.iter() {
                    // 分隔符与续分符不是「一条注」：Word 与 LibreOffice 都在这个部件里
                    // 白放两条，它们没有正文，一开 --keep-empty 就凭空多出两条脚注
                    if matches!(
                        owner.attr_local("type").unwrap_or_default(),
                        "separator" | "continuationSeparator"
                    ) {
                        continue;
                    }
                    let author = owner.attr_local("author");
                    let stamp = owner.attr_local("date");
                    for one in owner.descendants("p") {
                        let text = run_text(one);
                        if text.is_empty() && !app.keep_empty {
                            continue;
                        }
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            &text,
                            json!({
                                "from": what,
                                "part": part,
                                "author": author,
                                "date": stamp,
                                "style": paragraph_style(one),
                            }),
                        );
                    }
                }
            }
            let mut kinds: Vec<&'static str> = side.iter().map(|(_, one)| *one).collect();
            kinds.sort_unstable();
            kinds.dedup();
            notes.push(format!(
                "正文之外的部件读了 {} 个（{}）；包里没写的种类不会出现",
                side.len(),
                if kinds.is_empty() {
                    "无".to_string()
                } else {
                    kinds.join("、")
                },
            ));
        }
        Family::Ooxml if doc.app == "powerpoint" => {
            kind = "slides";
            let mut slides: Vec<String> = doc
                .entries
                .iter()
                .map(|one| one.name.clone())
                .filter(|one| one.starts_with("ppt/slides/slide") && one.ends_with(".xml"))
                .collect();
            slides.sort_by_key(|one| slide_number(one));
            for part in slides {
                let Some(member) = read(bytes, &part) else {
                    notes.push(format!("{part} 读不出来"));
                    continue;
                };
                parts_read.push(part.clone());
                let root = xmlscan::parse_str(&member.as_text());
                let slide = slide_number(&part);
                // 先按形状走：标题形状里的段才敢标 heading
                for shape in root.descendants("sp") {
                    let title = shape
                        .descendants("ph")
                        .into_iter()
                        .any(|one| matches!(one.attr("type"), Some("title") | Some("ctrTitle")));
                    for one in shape.descendants("p") {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            &run_text(one),
                            json!({"slide": slide, "heading": if title { json!(1) } else { Value::Null }, "part": part}),
                        );
                    }
                }
                // 表格 / 图表这些不在 sp 里的文字也要读到
                for frame in root.descendants("graphicFrame") {
                    for one in frame.descendants("p") {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            &run_text(one),
                            json!({"slide": slide, "table": true, "part": part}),
                        );
                    }
                }
                let note_part = part.replace("/slides/slide", "/notesSlides/notesSlide");
                if let Some(member) = read(bytes, &note_part) {
                    parts_read.push(note_part.clone());
                    let root = xmlscan::parse_str(&member.as_text());
                    for one in root.descendants("p") {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            &run_text(one),
                            json!({"slide": slide, "notes": true, "part": note_part}),
                        );
                    }
                }
            }
        }
        Family::Ooxml if doc.app == "excel" => {
            kind = "cells";
            let shared = match read(bytes, "xl/sharedStrings.xml") {
                Some(member) => {
                    parts_read.push("xl/sharedStrings.xml".to_string());
                    xmlscan::parse_str(&member.as_text())
                        .descendants("si")
                        .iter()
                        .map(|one| run_text(one))
                        .collect::<Vec<String>>()
                }
                None => Vec::new(),
            };
            let names = sheet_names(bytes);
            let mut sheets: Vec<String> = doc
                .entries
                .iter()
                .map(|one| one.name.clone())
                .filter(|one| one.starts_with("xl/worksheets/sheet") && one.ends_with(".xml"))
                .collect();
            sheets.sort_by_key(|one| slide_number(one));
            for part in sheets {
                let Some(member) = read(bytes, &part) else {
                    notes.push(format!("{part} 读不出来"));
                    continue;
                };
                parts_read.push(part.clone());
                let label = names
                    .iter()
                    .find(|(part_name, _)| *part_name == part)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| part.clone());
                let root = xmlscan::parse_str(&member.as_text());
                for cell in root.descendants("c") {
                    let reference = cell.attr("r").unwrap_or_default().to_string();
                    let kind_attr = cell.attr("t").unwrap_or("").to_string();
                    let mut text = String::new();
                    let mut formula = false;
                    if let Some(one) = cell.child("is") {
                        text = run_text(one);
                    } else if let Some(one) = cell.child("v") {
                        let raw = run_text(one);
                        if kind_attr == "s" {
                            let pos = raw.parse::<usize>().unwrap_or(usize::MAX);
                            text = shared
                                .get(pos)
                                .cloned()
                                .unwrap_or_else(|| format!("#共享字符串索引 {raw} 越界"));
                        } else {
                            text = raw;
                        }
                    }
                    if let Some(one) = cell.child("f") {
                        let body = run_text(one);
                        if text.is_empty() && !body.is_empty() {
                            formula = true;
                            text = format!("={body}");
                        }
                    }
                    if text.is_empty() && !app.keep_empty {
                        continue;
                    }
                    let index = paragraphs.len();
                    paragraphs.push(json!({
                        "index": index,
                        "text": text,
                        "ref": reference,
                        "sheet": label,
                        "formula": formula,
                        "part": part,
                    }));
                }
            }
            notes.push(
                "公式单元格读到的是公式本身：这份文件里没有缓存结果，报数值就是猜".to_string(),
            );
        }
        Family::Odf if doc.app == "excel" => {
            // ODS 的正文就是格子：按表按位置交出来，跟 xlsx 那条同一个口径
            kind = "cells";
            let book = crate::odsheet::read(bytes);
            notes.extend(book.notes.iter().cloned());
            parts_read.push("content.xml".to_string());
            for one in &book.sheets {
                for had in &one.cells {
                    let extra = json!({
                        "sheet": one.name,
                        "ref": had.reference,
                        "value_type": had.value_type,
                        "part": "content.xml",
                    });
                    push_paragraph(&mut paragraphs, app.keep_empty, &had.text, extra);
                }
            }
            notes.push(
                "ODS 的格子里交出来的是给人看的那一份（`12.5%`、`2013年12月23日`），\
                 真正的值在 office:value / date-value —— 要两份账用 office-sheet"
                    .to_string(),
            );
        }
        Family::Odf => {
            kind = "paragraphs";
            if let Some(member) = read(bytes, "content.xml") {
                parts_read.push("content.xml".to_string());
                let root = xmlscan::parse_str(&member.as_text());
                // 段与标题按文档顺序一次走完：分开取两遍会把所有标题堆到末尾，
                // 读出来的就不是那份文档了
                let mut ordered: Vec<(&Node, bool)> = Vec::new();
                gather_text_nodes(&root, &mut ordered);
                for (one, heading) in ordered {
                    let text = if one.descendants("annotation").is_empty() {
                        run_text(one)
                    } else {
                        // 批注嵌在这一段里面：它的字要单独交账，不能混进正文
                        text_skipping(one, "annotation")
                    };
                    if text.is_empty() && !app.keep_empty {
                        continue;
                    }
                    let level = if heading {
                        one.attr("text:outline-level")
                            .and_then(|raw| raw.parse::<u32>().ok())
                            .unwrap_or(1)
                    } else {
                        0
                    };
                    let index = paragraphs.len();
                    paragraphs.push(json!({
                        "index": index,
                        "text": text,
                        "heading": if level > 0 { json!(level) } else { Value::Null },
                        "part": "content.xml",
                    }));
                }
                // 批注（ODF 里叫 text:annotation）：作者与时间挂在它自己的 meta:* 孩子上，
                // 不是属性 —— 这跟 docx 的 w:comment 正好相反，两边都得照文件读。
                let annotations = root.descendants("annotation");
                for owner in &annotations {
                    let author = owner
                        .child("creator")
                        .map(|had| had.text().trim().to_string())
                        .unwrap_or_default();
                    let stamp = owner
                        .child("date")
                        .map(|had| had.text().trim().to_string())
                        .unwrap_or_default();
                    for one in owner.descendants("p") {
                        let text = run_text(one);
                        if text.is_empty() && !app.keep_empty {
                            continue;
                        }
                        let index = paragraphs.len();
                        paragraphs.push(json!({
                            "index": index,
                            "text": text,
                            "from": "annotation",
                            "author": if author.is_empty() { Value::Null } else { json!(author) },
                            "date": if stamp.is_empty() { Value::Null } else { json!(stamp) },
                            "part": "content.xml",
                        }));
                    }
                }
                if !annotations.is_empty() {
                    notes.push(format!(
                        "正文之外还读了 {} 条批注（text:annotation，作者与时间挂在它自己的 meta:* 上）",
                        annotations.len()
                    ));
                }
                // 页眉与页脚不在 content.xml：ODF 把它们放在 styles.xml 的 master-page 里，
                // 左右页与首页还可以各有一套（style:header-left / style:header-first …）
                if doc.app == "word" {
                    match read(bytes, "styles.xml") {
                        Some(member) => {
                            parts_read.push("styles.xml".to_string());
                            let styles = xmlscan::parse_str(&member.as_text());
                            let mut slots = 0usize;
                            for page in styles.descendants("master-page") {
                                let master =
                                    page.attr_local("name").unwrap_or_default().to_string();
                                for slot in &page.children {
                                    let local = slot.local();
                                    let what = match local {
                                        "header" | "header-left" | "header-first" => "header",
                                        "footer" | "footer-left" | "footer-first" => "footer",
                                        _ => continue,
                                    };
                                    for one in slot.descendants("p") {
                                        let text = run_text(one);
                                        if !text.is_empty() {
                                            slots += 1;
                                        }
                                        let extra = json!({
                                            "from": what,
                                            "part": "styles.xml",
                                            "master": master.as_str(),
                                            "slot": local,
                                            // 页眉页脚没有作者与时间：这两个键在 docx 那边也是空的，
                                            // 键的集合两边一致，读的人不必按家族分两套代码
                                            "author": Value::Null,
                                            "date": Value::Null,
                                        });
                                        push_paragraph(
                                            &mut paragraphs,
                                            app.keep_empty,
                                            &text,
                                            extra,
                                        );
                                    }
                                }
                            }
                            notes.push(format!(
                                "页眉与页脚在 styles.xml 的 master-page 里，读了 {slots} 条有字的"
                            ));
                        }
                        None => notes.push("styles.xml 读不到：这份的页眉页脚没看".to_string()),
                    }
                }
            } else {
                notes.push("包里读不到 content.xml".to_string());
            }
        }
        Family::Rtf => {
            kind = "rtf-lines";
            let one = crate::rtf::extract(bytes);
            notes.extend(one.notes.iter().cloned());
            parts_read.push("(rtf stream)".to_string());
            for line in &one.lines {
                if line.is_empty() && !app.keep_empty {
                    continue;
                }
                let index = paragraphs.len();
                paragraphs.push(json!({ "index": index, "text": line, "part": "rtf" }));
            }
            // 页眉与页脚的字与正文在同一个流里，靠目标群分开；一条会同时写进
            // `\header`、`\headerf` 好几个口袋，所以每条都带着自己是哪个口袋
            for (what, found) in [("header", &one.headers), ("footer", &one.footers)] {
                for had in found.iter() {
                    let text = had["text"].as_str().unwrap_or_default();
                    if text.is_empty() && !app.keep_empty {
                        continue;
                    }
                    let index = paragraphs.len();
                    paragraphs.push(json!({
                        "index": index,
                        "text": text,
                        "from": what,
                        "slot": had["slot"],
                        "part": "rtf",
                    }));
                }
            }
            if one.page_destinations > 0 {
                notes.push(format!(
                    "页眉页脚是从 {} 个目标群里读出来的（它们与正文混在同一个流里）",
                    one.page_destinations
                ));
            }
        }
        Family::Compound => match doc.compound.as_ref() {
            // 遗留格式没有「包」可走：.doc 的正文位置在 piece 表里、.xls 的文本在 SST 里，
            // 各走各的读取器。.ppt 的 97 记录树这版还没做，照实说，不给一份空文本。
            Some(cfb) if cfb.find("WordDocument").is_some() => {
                kind = "paragraphs";
                match crate::word::read(cfb, bytes) {
                    Ok(body) => {
                        parts_read.push(format!("WordDocument + {}", body.table_stream));
                        notes.extend(body.notes.iter().cloned());
                        for (index, line) in body.lines.iter().enumerate() {
                            paragraphs.push(json!({
                                "index": index,
                                "text": line,
                                "part": "WordDocument",
                            }));
                        }
                    }
                    Err(why) => {
                        kind = "unsupported";
                        notes.push(why);
                    }
                }
            }
            Some(cfb) if cfb.find("Workbook").is_some() || cfb.find("Book").is_some() => {
                kind = "shared-strings";
                match crate::biff::read(cfb, bytes) {
                    Ok(book) => {
                        parts_read.push("Workbook".to_string());
                        notes.extend(book.notes.iter().cloned());
                        notes.push(format!(
                            "这份工作簿有 {} 张表（{}）；按表归位的单元格见 office-sheet",
                            book.sheets.len(),
                            book.sheets
                                .iter()
                                .map(|one| format!("{}:{}", one.name, one.state))
                                .collect::<Vec<_>>()
                                .join("、")
                        ));
                        for (index, text) in book.strings.iter().enumerate() {
                            paragraphs.push(json!({
                                "index": index,
                                "text": text,
                                "part": "Workbook/SST",
                            }));
                        }
                    }
                    Err(why) => {
                        kind = "unsupported";
                        notes.push(why);
                    }
                }
            }
            Some(cfb) if cfb.find("PowerPoint Document").is_some() => {
                kind = "record-tree";
                match crate::ppt::read(cfb, bytes) {
                    Ok(deck) => {
                        parts_read.push("PowerPoint Document".to_string());
                        notes.extend(deck.notes.iter().cloned());
                        notes.push(format!(
                            "97 记录树里文本原子有 {} 个（走过 {} 条记录）；母版、备注与幻灯片正文同在一棵树里，\
                             这一条按流的顺序全列，按页归位在 office-slide（它按 recType 0x03EE 的容器分组，\
                             那个对应关系是与同一份文件的 pptx 逐张对出来的）",
                            deck.atoms.len(),
                            deck.records,
                        ));
                        for (index, line) in deck.lines().iter().enumerate() {
                            paragraphs.push(json!({
                                "index": index,
                                "text": line,
                                "part": "PowerPoint Document",
                            }));
                        }
                    }
                    Err(why) => {
                        kind = "unsupported";
                        notes.push(why);
                    }
                }
            }
            Some(_) => {
                notes.push(
                    "这份复合文档既没有 WordDocument 也没有 Workbook / PowerPoint Document 流，\
                     所以没有「正文」这一层可指"
                        .to_string(),
                );
            }
            None => {
                notes.push("复合文档打不开（头或 FAT 读不出），正文也就无从谈起".to_string());
            }
        },
        _ => {
            notes.push(format!("{} 没有「正文」这一层可读", doc.format));
        }
    }
    ctx.tick(1, Some(1), "text extracted");

    let total_chars: usize = paragraphs
        .iter()
        .map(|one| one["text"].as_str().unwrap_or("").chars().count())
        .sum();
    let budget = crate::opack::take_limit(app.max_chars, MAX_CHARS_DEFAULT);
    let mut emitted: Vec<Value> = Vec::new();
    let mut used = 0usize;
    let mut cut = false;
    for one in paragraphs.iter() {
        let len = one["text"].as_str().unwrap_or("").chars().count();
        if used + len > budget && !emitted.is_empty() {
            cut = true;
            break;
        }
        used += len;
        emitted.push(one.clone());
    }
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "format": doc.format,
        "app": doc.app,
        "kind": kind,
        "paragraphs": emitted,
        "line_count": emitted.len(),
        "chars": used,
        "total_paragraphs": paragraphs.len(),
        "total_chars": total_chars,
        "cut": cut,
        "parts_read": parts_read,
        "notes": notes,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn read(bytes: &[u8], want: &str) -> Option<Member> {
    zipread::member(bytes, want, DEFAULT_MEMBER_CAP).ok()
}

/// 交出来的段是「读到的顺序」，`index` 就是它在这份列表里的位置：
/// 各分支（正文 / 批注 / 每页形状 / 每页备注）共用一个计数器，才会出现
/// 一条 9 夹在两条 10 之间、或两页各自从 0 数起 —— 那不是一个序号，是噪声。
fn push_paragraph(into: &mut Vec<Value>, keep_empty: bool, text: &str, extra: Value) {
    if text.is_empty() && !keep_empty {
        return;
    }
    let mut one = extra;
    let index_value = json!(into.len());
    if let Some(map) = one.as_object_mut() {
        map.insert("index".to_string(), index_value);
        map.insert("text".to_string(), json!(text));
    }
    into.push(one);
}

/// 按文档顺序收集 ODF 的正文节点：`text:p` 是段，`text:h` 是带层级的标题。
/// `text:annotation` 整块跳过 —— ODF 的批注就嵌在正文段里面，里面的 `text:p`
/// 不是页面上的一段字，它单独交账（带作者与时间）。
fn gather_text_nodes<'a>(node: &'a Node, into: &mut Vec<(&'a Node, bool)>) {
    for one in &node.children {
        if one.local() == "annotation" {
            continue;
        }
        if one.local() == "p" || one.local() == "h" {
            into.push((one, one.local() == "h"));
            continue;
        }
        gather_text_nodes(one, into);
    }
}

/// 一段里挖掉某类子树之后的字（ODF 用它把批注从正文里摘出去）
fn text_skipping(node: &Node, skip: &str) -> String {
    let mut out = String::new();
    collect_skipping(node, skip, &mut out);
    out.trim().to_string()
}

fn collect_skipping(node: &Node, skip: &str, out: &mut String) {
    let name = node.local();
    if name == skip {
        return;
    }
    if name == "tab" {
        out.push('\t');
        return;
    }
    if name == "br" || name == "cr" {
        out.push('\n');
        return;
    }
    out.push_str(&node.direct);
    for child in &node.children {
        collect_skipping(child, skip, out);
    }
}

/// 给 office-doc 复用：一个段落的文本（口径与 office-text 交出去的一致）
pub fn paragraph_text(node: &Node) -> String {
    run_text(node)
}

/// 给 office-doc 复用：ODF 的**正文段**清单 —— 批注子树里的 `text:p` 不算。
/// docx 把批注放在另一个部件里，天然不会混进来；ODF 是把 `text:annotation`
/// 嵌在正文段**里面**的，所以这条排除必须由读者自己做，不然段数比生产者多一。
pub fn odf_paragraphs<'a>(node: &'a Node, into: &mut Vec<&'a Node>) {
    for one in &node.children {
        if one.local() == "annotation" {
            continue;
        }
        if one.local() == "p" {
            into.push(one);
            continue;
        }
        odf_paragraphs(one, into);
    }
}

/// 给 office-doc 复用：ODF 段落口径 —— 批注里的字不算这一段的字
pub fn odf_paragraph_text(node: &Node) -> String {
    text_skipping(node, "annotation")
}

/// 给 office-doc 复用：段落样式名
pub fn style_of(node: &Node) -> Option<String> {
    paragraph_style(node)
}

/// 给 office-doc 复用：标题层级（`heading_level` 报的是 JSON，这里要的是 Option<u32>）
pub fn heading_of(node: &Node) -> Option<u32> {
    match heading_level(node) {
        Value::Number(one) => one.as_u64().map(|raw| raw as u32),
        _ => None,
    }
}

/// 一个段落里的文本：按文档顺序取每一段文字，并把 tab / br 还原成制表与换行
fn run_text(node: &Node) -> String {
    let mut out = String::new();
    collect(node, &mut out);
    out.trim().to_string()
}

fn collect(node: &Node, out: &mut String) {
    let name = node.local();
    if name == "tab" {
        out.push('\t');
        return;
    }
    if name == "br" || name == "cr" {
        out.push('\n');
        return;
    }
    out.push_str(&node.direct);
    for child in &node.children {
        collect(child, out);
    }
}

fn paragraph_style(node: &Node) -> Option<String> {
    let found = node.descendants("pStyle").into_iter().next()?;
    found.attr_local("val").map(|one| (*one).to_string())
}

/// 标题层级：样式名里的数字（Heading2 / 标题 2），否则看 outlineLvl（0 起，报的时候 +1）
fn heading_level(node: &Node) -> Value {
    if let Some(style) = paragraph_style(node) {
        let lower = style.to_lowercase();
        if lower.starts_with("heading") || style.starts_with("标题") {
            let digits: String = style.chars().filter(|one| one.is_ascii_digit()).collect();
            return json!(digits.parse::<u32>().unwrap_or(1).max(1));
        }
    }
    if let Some(one) = node.descendants("outlineLvl").into_iter().next() {
        if let Some(raw) = one
            .attr_local("val")
            .and_then(|one| one.parse::<u32>().ok())
        {
            return json!(raw + 1);
        }
    }
    Value::Null
}

/// `xl/workbook.xml` 的表名 + `xl/_rels/workbook.xml.rels` 的 rId→部件，两张表对起来
fn sheet_names(bytes: &[u8]) -> Vec<(String, String)> {
    let Some(workbook) = read(bytes, "xl/workbook.xml") else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&workbook.as_text());
    let mut by_id: Vec<(String, String)> = Vec::new();
    if let Some(rels) = read(bytes, "xl/_rels/workbook.xml.rels") {
        let rel_root = xmlscan::parse_str(&rels.as_text());
        for one in rel_root.descendants("Relationship") {
            let id = one.attr("Id").unwrap_or_default().to_string();
            let target = one.attr("Target").unwrap_or_default().to_string();
            let normalized = if target.starts_with('/') {
                target[1..].to_string()
            } else if target.starts_with("worksheets/") {
                format!("xl/{target}")
            } else {
                target
            };
            by_id.push((id, normalized));
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for one in root.descendants("sheet") {
        let name = one.attr("name").unwrap_or_default().to_string();
        let id = one.attr_local("id").unwrap_or_default().to_string();
        if let Some((_, part)) = by_id.iter().find(|(one_id, _)| *one_id == id) {
            out.push((part.clone(), name));
        }
    }
    out
}

/// `ppt/slides/slide12.xml` / `xl/worksheets/sheet3.xml` → 12 / 3；没有数字就排最前
fn slide_number(name: &str) -> usize {
    let last = name.rsplit('/').next().unwrap_or("");
    let digits: String = last.chars().filter(|one| one.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str, max_chars: u64, keep_empty: bool) -> Value {
        let app = OfficeText {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            max_chars,
            keep_empty,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_text(&app, &Context::new_test(tx)).expect("office-text 应成功")
    }

    fn texts(out: &Value) -> Vec<String> {
        out["paragraphs"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["text"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// docx：九段有字的段落，顺序与 Word 里看到的一致（期望值来自 office_reader.py）
    #[test]
    fn reads_word_paragraphs_in_order() {
        let out = run("notes.docx", 20000, false);
        assert_eq!(out["kind"], "paragraphs");
        assert_eq!(out["format"], "docx");
        assert_eq!(
            texts(&out),
            vec![
                "一级标题：预算口径",
                "第三季度服务器预算为十二万四千元",
                "二级标题：明细",
                "科目",
                "金额",
                "服务器",
                "124000",
                "口径见 预算制度",
                "最后一页说明：数字为含税口径",
                // 批注不在 document.xml 里，但它是这份文件的文字，排在正文之后
                "这里要补上不含税口径",
            ],
            "{out}"
        );
        // 这里数的是「这条命令给出的段」：只放分页符的两段被 trim 成空，默认不露面
        assert_eq!(out["total_paragraphs"], 10, "{out}");
        assert_eq!(out["paragraphs"][0]["heading"], json!(1));
        assert_eq!(out["paragraphs"][2]["heading"], json!(2));
        assert_eq!(out["paragraphs"][1]["heading"], Value::Null);
        assert_eq!(out["paragraphs"][0]["style"], "Heading1");
        assert_eq!(out["total_chars"], 77, "正文 67 + 批注 10：{out}");
        assert_eq!(out["cut"], json!(false));
    }

    /// `keep_empty` 时那两个只放分页符的空段也要露面：默认藏起来是为了读得顺
    #[test]
    fn keep_empty_shows_the_paragraphs_nobody_sees() {
        let out = run("notes.docx", 20000, true);
        // 正文 11 段（含只放分页符的两段）+ 批注 1 段
        assert_eq!(out["line_count"], 12, "{out}");
        assert!(texts(&out).iter().any(|one| one.is_empty()));
    }

    /// 批注这一类「正文之外」的段落要带着出处与作者出来，不然读者分不清它在哪儿
    #[test]
    fn a_comment_is_reported_as_a_comment() {
        let out = run("notes.docx", 20000, false);
        let items = out["paragraphs"].as_array().expect("是数组");
        let comment = items
            .iter()
            .find(|one| one["from"] == "comment")
            .expect("这份 docx 有一条批注");
        assert_eq!(comment["text"], "这里要补上不含税口径", "{comment}");
        assert_eq!(comment["part"], "word/comments.xml");
        assert_eq!(comment["author"], "liuqi", "批注要说是谁写的");
        assert!(
            comment["date"].as_str().unwrap_or("").starts_with("2026-"),
            "{comment}"
        );
        // 正文那几段不该带上 from：读了什么就要分得清
        let body = &items[0];
        assert_eq!(body["part"], "word/document.xml");
        assert_eq!(body["from"], Value::Null, "{body}");
        assert!(
            out["notes"]
                .as_array()
                .expect("有 notes")
                .iter()
                .any(|one| one.as_str().unwrap_or("").contains("comment")),
            "{out}"
        );
        // index 是「在这份列表里的第几条」：正文与批注共用一条数，
        // 所以它必须 0、1、2 连续，不能一条 9 夹在 7 与 10 之间。
        let listed: Vec<u64> = items
            .iter()
            .map(|one| one["index"].as_u64().expect("每条都有 index"))
            .collect();
        assert_eq!(
            listed,
            (0..listed.len() as u64).collect::<Vec<u64>>(),
            "{listed:?}"
        );
    }

    /// pptx：两页都要有，标题形状标 heading，表格与备注各归各
    #[test]
    fn reads_slides_with_their_numbers() {
        let out = run("deck.pptx", 20000, false);
        assert_eq!(out["kind"], "slides");
        let items = out["paragraphs"].as_array().expect("是数组");
        let slides: Vec<u64> = items
            .iter()
            .filter_map(|one| one["slide"].as_u64())
            .collect();
        assert!(slides.contains(&1) && slides.contains(&2), "{slides:?}");
        assert_eq!(out["paragraphs"][0]["text"], "预算评审");
        assert_eq!(out["paragraphs"][0]["heading"], json!(1));
        let all = texts(&out).join(" ");
        assert!(all.contains("新增两台 64 核应用服务器"), "{all}");
        assert!(all.contains("第二页：数字"), "{all}");
        assert!(all.contains("124000"), "表格里的字也是内容：{all}");
        assert!(
            all.contains("评审时先讲口径再讲数字"),
            "备注页要单独读出来：{all}"
        );
        assert!(items.iter().any(|one| one["notes"] == json!(true)));
        assert!(items.iter().any(|one| one["table"] == json!(true)));
    }

    /// xlsx：内联字符串（openpyxl 不用共享字符串表）也要读到，并带单元格位置
    #[test]
    fn reads_workbook_cells_with_their_references() {
        let out = run("book.xlsx", 20000, false);
        assert_eq!(out["kind"], "cells");
        let items = out["paragraphs"].as_array().expect("是数组");
        let firsts: Vec<(String, String, String)> = items
            .iter()
            .map(|one| {
                (
                    one["sheet"].as_str().unwrap_or("").to_string(),
                    one["ref"].as_str().unwrap_or("").to_string(),
                    one["text"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        assert!(
            firsts.contains(&("预算表".to_string(), "A1".to_string(), "科目".to_string(),)),
            "{firsts:?}"
        );
        assert!(
            firsts.iter().any(|(_, _, text)| text == "隐藏的草稿表"),
            "{firsts:?}"
        );
        assert!(
            firsts.iter().any(|(_, _, text)| text == "=SUM(B2:B3)"),
            "公式单元格要报公式：{firsts:?}"
        );
        assert!(items.iter().any(|one| one["formula"] == json!(true)));
    }

    /// odt：段与标题按文档顺序交替出现，标题带层级
    #[test]
    fn reads_opendocument_paragraphs_in_order() {
        let out = run("notes.odt", 20000, false);
        assert_eq!(out["kind"], "paragraphs");
        assert_eq!(out["format"], "odt");
        let all = texts(&out);
        assert!(all.contains(&"一级标题：预算口径".to_string()), "{all:?}");
        assert!(all.contains(&"第三季度服务器预算为十二万四千元".to_string()));
        // 逐条钉住：标题插在两段之间，按文档顺序走 —— 分开取「所有段」「所有标题」
        // 会把它堆到末尾，读出来的就不是这份文档；最后一条才是批注
        assert_eq!(
            all,
            vec![
                "一级标题：预算口径",
                "第三季度服务器预算为十二万四千元",
                "二级标题：明细",
                "科目",
                "金额",
                "服务器",
                "124000",
                "口径见 预算制度",
                "最后一页说明：数字为含税口径",
                "这里要补上不含税口径",
            ],
            "{all:?}"
        );
        let items = out["paragraphs"].as_array().expect("是数组");
        assert!(
            items.iter().any(|one| one["heading"] != Value::Null),
            "标题要有层级：{items:?}"
        );
        // index 是「这份列表里的第几条」，跳号就说明有条目被默默丢掉了
        let listed: Vec<u64> = items
            .iter()
            .map(|one| one["index"].as_u64().expect("每条都有 index"))
            .collect();
        assert_eq!(
            listed,
            (0..listed.len() as u64).collect::<Vec<u64>>(),
            "{listed:?}"
        );
    }

    /// ODS 的正文就是格子：按表、按位置交出来，口径与 xlsx 那条一致
    /// （期望值来自 `ods_facts()`）
    #[test]
    fn an_opendocument_spreadsheet_reads_as_cells() {
        let out = run("book.ods", 20000, false);
        assert_eq!(out["kind"], "cells", "{out}");
        let items = out["paragraphs"].as_array().expect("是数组");
        assert_eq!(items.len(), 11, "{out}");
        assert_eq!(items[0]["text"], "科目");
        assert_eq!(items[0]["sheet"], "预算表");
        assert_eq!(items[0]["ref"], "A1");
        assert_eq!(items[7]["ref"], "B4", "公式格给的是文件里算出来的那一个数");
        assert_eq!(items[7]["text"], "142000", "{out}");
        assert_eq!(items[7]["value_type"], "float");
        assert_eq!(items[10]["sheet"], "草稿", "隐藏表的内容也在");
        let listed: Vec<u64> = items
            .iter()
            .map(|one| one["index"].as_u64().expect("每条都有 index"))
            .collect();
        assert_eq!(
            listed,
            (0..listed.len() as u64).collect::<Vec<u64>>(),
            "{listed:?}"
        );
        assert!(
            out["notes"]
                .as_array()
                .expect("有 notes")
                .iter()
                .any(|one| one.as_str().unwrap_or("").contains("office:value")),
            "{out}"
        );
    }

    /// ODF 的批注嵌在正文段**里面**（docx 是另一个部件）：它自己单独一条，
    /// 带 from/author/date，而且不许混进它所在那一段的字里
    #[test]
    fn an_odf_annotation_is_a_separate_entry_not_body_text() {
        let out = run("notes.odt", 20000, false);
        let items = out["paragraphs"].as_array().expect("是数组");
        let found = items
            .iter()
            .find(|one| one["from"] == "annotation")
            .expect("这份 odt 有一条批注");
        assert_eq!(found["text"], "这里要补上不含税口径", "{found}");
        assert_eq!(
            found["author"], "liuqi",
            "作者挂在 meta:creator 那个孩子上：{found}"
        );
        assert_eq!(found["part"], "content.xml");
        assert!(
            found["date"]
                .as_str()
                .unwrap_or_default()
                .starts_with("2026-"),
            "{found}"
        );
        // 它所在那一段只留自己的字
        let holder = items
            .iter()
            .find(|one| {
                one["text"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("最后一页说明")
            })
            .expect("批注后面那段正文还在");
        assert_eq!(holder["text"], "最后一页说明：数字为含税口径", "{holder}");
        assert_eq!(holder["from"], Value::Null);
        assert_eq!(
            out["total_paragraphs"], 10,
            "7 段正文 + 2 个标题 + 1 条批注：{out}"
        );
    }

    /// 同一批字换 ODF 的存法（`notes-hf.odt` 由 LibreOffice 从 `notes-hf.docx` 转来）：
    /// 页眉页脚在 styles.xml 的 master-page 里，两个节各有一个 master-page，
    /// 所以四条都在，而且各自说得出挂在哪个页型上
    /// （期望值来自 `office_reader.py` 的 odf_page_text）
    #[test]
    fn odf_headers_and_footers_come_from_the_master_pages() {
        let out = run("notes-hf.odt", 20000, false);
        let items = out["paragraphs"].as_array().expect("是数组");
        let side: Vec<(&str, &str, &str, &str)> = items
            .iter()
            .filter(|one| one["from"] != Value::Null)
            .map(|one| {
                (
                    one["from"].as_str().unwrap_or_default(),
                    one["master"].as_str().unwrap_or_default(),
                    one["slot"].as_str().unwrap_or_default(),
                    one["text"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            side,
            vec![
                ("header", "Standard", "header", "公司机密 · 预算评审"),
                ("footer", "Standard", "footer", "第 1 页 / 共 3 页"),
                ("header", "Converted1", "header", "第二节的页眉不一样"),
                ("footer", "Converted1", "footer", "第 1 页 / 共 3 页"),
            ],
            "{side:?}"
        );
        let body = items
            .iter()
            .filter(|one| one["from"] == Value::Null)
            .count();
        assert_eq!(body, 3, "正文三条，页眉页脚不混进去：{out}");
        assert_eq!(out["total_paragraphs"], 7, "{out}");
        let listed: Vec<u64> = items
            .iter()
            .map(|one| one["index"].as_u64().expect("每条都有 index"))
            .collect();
        assert_eq!(listed, (0..7).collect::<Vec<u64>>(), "{listed:?}");
    }

    /// RTF 的页眉页脚与正文在**同一个流**里，只靠目标群（`\headerl …}`）分开：
    /// 以前这些字会被当成正文行交出去。一条页眉会同时写进 `\header` 与 `\headerf`
    /// 好几个口袋，所以口袋名一起报，不替文件合并
    /// （期望值来自 `lyco_rtf.py` 的 rtf_text）
    #[test]
    fn rtf_page_headers_are_not_body_text() {
        let out = run("notes-hf.rtf", 20000, false);
        let items = out["paragraphs"].as_array().expect("是数组");
        let body: Vec<&str> = items
            .iter()
            .filter(|one| one["from"] == Value::Null)
            .map(|one| one["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            body,
            vec![
                "带页眉的一页",
                "正文只有一句：这一份是用来测页眉页脚那条分支的。",
                "换节之后的一段正文。",
            ],
            "{out}"
        );
        let side: Vec<(&str, &str, &str)> = items
            .iter()
            .filter(|one| one["from"] != Value::Null)
            .map(|one| {
                (
                    one["from"].as_str().unwrap_or_default(),
                    one["slot"].as_str().unwrap_or_default(),
                    one["text"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            side,
            vec![
                ("header", "header", "公司机密 · 预算评审"),
                ("header", "header", "第二节的页眉不一样"),
                ("header", "headerf", "第二节的页眉不一样"),
                ("footer", "footer", "第 1 页 / 共 3 页"),
                ("footer", "footer", "第 1 页 / 共 3 页"),
            ],
            "{side:?}"
        );
        // 没有页眉的那份一份都不许多
        let plain = run("notes.rtf", 20000, false);
        assert_eq!(
            plain["paragraphs"].as_array().expect("是数组").len(),
            7,
            "{plain}"
        );
        assert!(
            plain["paragraphs"]
                .as_array()
                .expect("是数组")
                .iter()
                .all(|one| one["from"] == Value::Null),
            "{plain}"
        );
    }

    /// 脚注部件里那两条分隔符（`separator` / `continuationSeparator`）不交出来：
    /// LibreOffice 与 Word 都在 `word/footnotes.xml` 里白放两条没有字的 `w:footnote`，
    /// 一开 `--keep-empty` 就会凭空多出两条「空脚注」
    /// （期望值来自 `office_reader.py` 的 `side_texts()`）
    #[test]
    fn footnote_separators_never_show_up_as_notes() {
        let out = run("notes-foot.docx", 20000, true);
        let side: Vec<(&str, &str)> = out["paragraphs"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter(|one| one["from"] != Value::Null)
            .map(|one| {
                (
                    one["from"].as_str().unwrap_or_default(),
                    one["text"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            side,
            [
                ("footnote", "Footnote: the numbers are gross."),
                ("footnote", "Second footnote: see the budget policy."),
            ],
            "{side:?}"
        );
        assert_eq!(out["total_paragraphs"], 6, "4 段正文 + 2 条脚注：{out}");
    }

    /// 页眉与页脚第一次有真件可走：两个节的页眉不一样，而它们按部件名排在正文后面
    /// （期望值来自 `office_reader.py` 的 `side_texts()`）
    #[test]
    fn headers_and_footers_come_out_labelled_as_such() {
        let out = run("notes-hf.docx", 20000, false);
        let items = out["paragraphs"].as_array().expect("是数组");
        let side: Vec<(&str, &str, &str)> = items
            .iter()
            .filter(|one| one["from"] != Value::Null)
            .map(|one| {
                (
                    one["from"].as_str().unwrap_or_default(),
                    one["part"].as_str().unwrap_or_default(),
                    one["text"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            side,
            [
                ("footer", "word/footer1.xml", "第 1 页 / 共 3 页"),
                ("header", "word/header1.xml", "公司机密 · 预算评审"),
                ("header", "word/header2.xml", "第二节的页眉不一样"),
            ],
            "{out}"
        );
        // 正文 5 段里只有 3 段有字（另两段只放分页符与节标记）
        assert_eq!(
            out["total_paragraphs"], 6,
            "3 段正文 + 页眉页脚 3 条：{out}"
        );
        assert_eq!(out["line_count"], 6);
        let body: Vec<&str> = items
            .iter()
            .filter(|one| one["from"] == Value::Null)
            .map(|one| one["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            body,
            [
                "带页眉的一页",
                "正文只有一句：这一份是用来测页眉页脚那条分支的。",
                "换节之后的一段正文。"
            ],
            "{out}"
        );
        for part in ["word/header1.xml", "word/header2.xml", "word/footer1.xml"] {
            assert!(
                out["parts_read"]
                    .as_array()
                    .expect("是数组")
                    .iter()
                    .any(|one| one.as_str() == Some(part)),
                "{part} 该被读到：{out}"
            );
        }
        // 页眉页脚没有作者这一说：那属性只挂在批注/脚注/尾注的容器上
        assert!(
            items
                .iter()
                .filter(|one| one["from"] != Value::Null)
                .all(|one| one["author"] == Value::Null),
            "{out}"
        );
    }

    /// rtf：控制字、字体表、域指令都不许混进正文
    #[test]
    fn reads_rtf_without_control_words() {
        let out = run("notes.rtf", 20000, false);
        assert_eq!(out["kind"], "rtf-lines");
        let all = texts(&out);
        assert_eq!(all[0], "一级标题：预算口径", "{all:?}");
        assert_eq!(all[6], "最后一页说明：数字为含税口径");
        for one in &all {
            assert!(!one.contains("\\p"), "控制字漏进正文: {one}");
            assert!(!one.contains("fonttbl"), "字体表漏进正文: {one}");
        }
    }

    /// 遗留的 .doc：piece 表读出来的正文必须与 docx 的段落对得上
    /// （期望值来自 `lyco_legacy.py` 对同一份文件的独立读取）
    #[test]
    fn reads_a_legacy_word_document() {
        let out = run("notes.doc", 20000, false);
        assert_eq!(out["kind"], "paragraphs", "{out}");
        let all = texts(&out);
        assert_eq!(all[0], "一级标题：预算口径", "{all:?}");
        assert_eq!(all[1], "第三季度服务器预算为十二万四千元");
        assert!(
            all[3].contains("服务器") && all[3].contains("124000"),
            "{all:?}"
        );
        assert!(
            all.iter().any(|one| one.contains("这里要补上不含税口径")),
            "批注正文也在文档里：{all:?}"
        );
        assert_eq!(out["parts_read"], json!(["WordDocument + 1Table"]), "{out}");
    }

    /// 纯 ASCII 的 .doc 走同一张表：内容不同，读取路径也要不同才说明不是一处巧合
    #[test]
    fn reads_a_legacy_word_document_in_english() {
        let out = run("notes-en.doc", 20000, false);
        assert_eq!(out["kind"], "paragraphs");
        assert_eq!(texts(&out)[0], "Quarterly budget note", "{out}");
    }

    /// 遗留的 .xls：SST 的字符串 + 表清单与可见性
    #[test]
    fn reads_a_legacy_workbook_strings() {
        let out = run("book.xls", 20000, false);
        assert_eq!(out["kind"], "shared-strings", "{out}");
        let all = texts(&out);
        assert_eq!(all.len(), 8, "{all:?}");
        assert!(all.contains(&"科目".to_string()) && all.contains(&"隐藏的草稿表".to_string()));
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("草稿:hidden"), "表的可见性要报出来：{note}");
    }

    /// .ppt 的 97 记录树：文本原子全部读出来（期望值来自 `lyco_legacy.py` 的 `ppt_text`）。
    /// 母版占位文字也在里面 —— 它确实是文件里的文字，不藏
    #[test]
    fn reads_a_legacy_presentation_record_tree() {
        let out = run("deck.ppt", 20000, false);
        assert_eq!(out["kind"], "record-tree", "{out}");
        let lines: Vec<&str> = out["paragraphs"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["text"].as_str().unwrap_or(""))
            .collect();
        for want in [
            "预算评审",
            "新增两台 64 核应用服务器",
            "第二条要点",
            "第二页：数字",
            "科目",
            "金额",
            "服务器",
            "评审时先讲口径再讲数字",
        ] {
            assert!(lines.iter().any(|one| *one == want), "缺 {want}");
        }
        // 幻灯片正文与母版文字的顺序也要对：正文在母版之后出现
        let first_body = lines
            .iter()
            .position(|one| *one == "预算评审")
            .expect("有正文");
        let first_master = lines
            .iter()
            .position(|one| *one == "Click to edit Master title style")
            .expect("有母版");
        assert!(first_master < first_body, "母版在前、正文在后：{lines:?}");
        assert_eq!(out["line_count"], lines.len());
    }

    /// Web / MCP 端省略 `max-chars` 时 derive 给的是 0，不是 schema 的 default：
    /// 0 必须回退成 20000，否则「省略」会悄悄变成「一个字都不给」
    #[test]
    fn an_omitted_char_budget_falls_back_to_the_default() {
        let app = OfficeText {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
            max_chars: 0,
            keep_empty: false,
            max_bytes: 0,
        };
        let (tx, _rx) = mpsc::channel();
        let omitted = run_office_text(&app, &Context::new_test(tx)).expect("省略两个上限也要能跑");
        let stated = run("notes.docx", 20000, false);
        assert_eq!(omitted, stated, "0 与写明的缺省值必须给出同一份答案");
    }

    /// 上限截断时同时给出 cut 与全量计数：悄悄砍短是最坏的失败方式
    #[test]
    fn a_small_char_budget_cuts_but_still_counts_everything() {
        let out = run("notes.docx", 12, false);
        assert_eq!(out["cut"], json!(true), "{out}");
        assert_eq!(out["total_paragraphs"], 10);
        assert_eq!(out["total_chars"], 77, "全量字符数不许跟着上限变：{out}");
        assert!(out["line_count"].as_u64().expect("有 line_count") < 9);
    }

    /// 拿一份 .pdf 问「它写了什么」也答得出来 —— 内容流与 /ToUnicode 那一层在
    /// `crate::pdf` 里，这里的期望值同样来自 `lyco_pdf.py` 的实测，
    /// 并逐行与 `pdftotext`（xpdf 系，第三套代码）对过
    #[test]
    fn a_pdf_answers_the_text_question_in_reading_order() {
        let out = run("notes.pdf", 20000, false);
        assert_eq!(out["kind"], "pages", "{out}");
        let lines: Vec<&str> = out["paragraphs"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["text"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(lines.len(), 7, "{lines:?}");
        assert_eq!(lines[0], "一级标题：预算口径", "{lines:?}");
        assert_eq!(lines[1], "第三季度服务器预算为十二万四千元");
        assert_eq!(lines[6], "最后一页说明：数字为含税口径");
        assert!(
            out["notes"]
                .as_array()
                .map(|one| one.is_empty())
                .unwrap_or(false),
            "读到字就不该有说明：{:?}",
            out["notes"]
        );
    }

    #[test]
    fn an_encrypted_pdf_says_it_could_not_read_the_text() {
        let out = run("locked.pdf", 20000, false);
        assert_eq!(out["paragraphs"].as_array().map(Vec::len), Some(0));
        assert!(out["notes"]
            .as_array()
            .expect("说明")
            .iter()
            .any(|one| one.as_str().unwrap_or_default().contains("密文")));
    }
}
