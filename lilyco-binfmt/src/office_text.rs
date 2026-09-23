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
//!   RTF 走 [`crate::rtf`] 的目标群感知提取，不是「把控制字删掉」就算完。
//!
//! 遗留二进制格式（.doc / .xls / .ppt）的正文在 `WordDocument` 流的 piece table 与
//! `Workbook` 流的 BIFF SST 里，是另一套记录式结构：这一层没实现时就照实返回
//! `kind: "unsupported"` 加一句为什么 —— 把读不出来报成「文档是空的」是假答案。

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
    about = "Read the human-readable text an office document actually contains. docx/docm: one entry per w:p in word/document.xml (Word's own paragraph notion, table cells included, w:tab and w:br restored). pptx/pptm: slides in numeric order, one entry per paragraph inside each shape, entries flagged title when the shape has a:ph type=title and separately flagged notes for notesSlideN.xml. xlsx/xlsm: one entry per valued cell with its reference and sheet name, covering both inline strings and the shared-string table, with formula cells reported as the formula because the file carries no cached result. odt/ods/odp: text:p and text:h from content.xml with outline levels. rtf: a destination-aware extractor that drops font/color/stylesheet tables and field instructions instead of leaking control words into the text. Returns { path, format, app, kind, paragraphs: [{index, text, heading?, style?, slide?, sheet?, ref?, notes?, part}], line_count, chars, total_paragraphs, total_chars, cut, parts_read, notes } and cuts output at max_chars while still reporting full totals, so a silent truncation is impossible. Legacy .doc/.xls/.ppt answer kind=unsupported with the reason (their text lives in FIB piece tables / BIFF SST), never as empty text. Read-only (safety T0): parts are inflated in memory only."
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
        Family::Ooxml if doc.app == "word" => {
            kind = "paragraphs";
            let part = "word/document.xml";
            match read(bytes, part) {
                Some(member) => {
                    parts_read.push(part.to_string());
                    let root = xmlscan::parse_str(&member.as_text());
                    for (index, one) in root.descendants("p").iter().enumerate() {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            index,
                            &run_text(one),
                            json!({"heading": heading_level(one), "style": paragraph_style(one), "part": part}),
                        );
                    }
                }
                None => notes.push("包里读不到 word/document.xml".to_string()),
            }
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
                let mut index = 0usize;
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
                            index,
                            &run_text(one),
                            json!({"slide": slide, "heading": if title { json!(1) } else { Value::Null }, "part": part}),
                        );
                        index += 1;
                    }
                }
                // 表格 / 图表这些不在 sp 里的文字也要读到
                for frame in root.descendants("graphicFrame") {
                    for one in frame.descendants("p") {
                        push_paragraph(
                            &mut paragraphs,
                            app.keep_empty,
                            index,
                            &run_text(one),
                            json!({"slide": slide, "table": true, "part": part}),
                        );
                        index += 1;
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
                            paragraphs.len(),
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
        Family::Odf => {
            kind = "paragraphs";
            if let Some(member) = read(bytes, "content.xml") {
                parts_read.push("content.xml".to_string());
                let root = xmlscan::parse_str(&member.as_text());
                // 段与标题按文档顺序一次走完：分开取两遍会把所有标题堆到末尾，
                // 读出来的就不是那份文档了
                let mut ordered: Vec<(&Node, bool)> = Vec::new();
                gather_text_nodes(&root, &mut ordered);
                for (index, (one, heading)) in ordered.into_iter().enumerate() {
                    let text = run_text(one);
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
                    paragraphs.push(json!({
                        "index": index,
                        "text": text,
                        "heading": if level > 0 { json!(level) } else { Value::Null },
                        "part": "content.xml",
                    }));
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
            for (index, line) in one.lines.iter().enumerate() {
                if line.is_empty() && !app.keep_empty {
                    continue;
                }
                paragraphs.push(json!({ "index": index, "text": line, "part": "rtf" }));
            }
        }
        Family::Compound => {
            notes.push(
                "遗留二进制格式（.doc/.xls/.ppt）的正文在 WordDocument 流的 piece table 与 \
                 Workbook 流的 BIFF SST 里，本版本还没实现这一层：宁可说不做，\
                 也不把读不出来的东西报成空文本"
                    .to_string(),
            );
        }
        _ => {
            notes.push(format!("{} 没有「正文」这一层可读", doc.format));
        }
    }
    ctx.tick(1, Some(1), "text extracted");

    let total_chars: usize = paragraphs
        .iter()
        .map(|one| one["text"].as_str().unwrap_or("").chars().count())
        .sum();
    let mut emitted: Vec<Value> = Vec::new();
    let mut used = 0usize;
    let mut cut = false;
    for one in paragraphs.iter() {
        let len = one["text"].as_str().unwrap_or("").chars().count();
        if used + len > app.max_chars as usize && !emitted.is_empty() {
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

fn push_paragraph(into: &mut Vec<Value>, keep_empty: bool, index: usize, text: &str, extra: Value) {
    if text.is_empty() && !keep_empty {
        return;
    }
    let mut one = extra;
    let index_value = json!(index);
    if let Some(map) = one.as_object_mut() {
        map.insert("index".to_string(), index_value);
        map.insert("text".to_string(), json!(text));
    }
    into.push(one);
}

/// 按文档顺序收集 ODF 的正文节点：`text:p` 是段，`text:h` 是带层级的标题
fn gather_text_nodes<'a>(node: &'a Node, into: &mut Vec<(&'a Node, bool)>) {
    for one in &node.children {
        if one.local() == "p" || one.local() == "h" {
            into.push((one, one.local() == "h"));
            continue;
        }
        gather_text_nodes(one, into);
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
            ],
            "{out}"
        );
        assert_eq!(out["total_paragraphs"], 11, "空段也算段，Word 就是这么数的");
        assert_eq!(out["paragraphs"][0]["heading"], json!(1));
        assert_eq!(out["paragraphs"][2]["heading"], json!(2));
        assert_eq!(out["paragraphs"][1]["heading"], Value::Null);
        assert_eq!(out["paragraphs"][0]["style"], "Heading1");
        assert_eq!(out["total_chars"], 67, "{out}");
        assert_eq!(out["cut"], json!(false));
    }

    /// `keep_empty` 时那两个只放分页符的空段也要露面：默认藏起来是为了读得顺
    #[test]
    fn keep_empty_shows_the_paragraphs_nobody_sees() {
        let out = run("notes.docx", 20000, true);
        assert_eq!(out["line_count"], 11, "{out}");
        assert!(texts(&out).iter().any(|one| one.is_empty()));
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
            firsts.contains(("预算表".to_string(), "A1".to_string(), "科目".to_string())),
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
        let items = out["paragraphs"].as_array().expect("是数组");
        assert!(
            items.iter().any(|one| one["heading"] != Value::Null),
            "标题要有层级：{items:?}"
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

    /// 遗留格式：照实说做不到，而不是给一份空文本
    #[test]
    fn legacy_formats_say_what_is_missing() {
        for name in ["notes.doc", "book.xls", "deck.ppt"] {
            let out = run(name, 20000, false);
            assert_eq!(out["kind"], "unsupported", "{name}: {out}");
            assert_eq!(out["paragraphs"].as_array().expect("是数组").len(), 0);
            let note = out["notes"]
                .as_array()
                .expect("有 notes")
                .iter()
                .map(|one| one.as_str().unwrap_or(""))
                .collect::<Vec<&str>>()
                .join(" ");
            assert!(
                note.contains("piece table") || note.contains("SST"),
                "{name}: {note}"
            );
        }
    }

    /// 上限截断时同时给出 cut 与全量计数：悄悄砍短是最坏的失败方式
    #[test]
    fn a_small_char_budget_cuts_but_still_counts_everything() {
        let out = run("notes.docx", 12, false);
        assert_eq!(out["cut"], json!(true), "{out}");
        assert_eq!(out["total_paragraphs"], 11);
        assert_eq!(out["total_chars"], 67, "全量字符数不许跟着上限变：{out}");
        assert!(out["line_count"].as_u64().expect("有 line_count") < 9);
    }
}
