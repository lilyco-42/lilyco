//! `lbin office-slide` — 演示文稿的结构：每页标题、备注、版式与母版、媒体与切换。
//!
//! 一份 pptx 的「页」散在三个地方：`ppt/presentation.xml` 里的 sldId 列表决定**放映顺序**
//! （部件名里的数字不是顺序！`slide12.xml` 可能排在第 2 页），每页的版式与母版靠关系表指，
//! 备注页是另一组部件。这三件事混在一起最容易做出的错答案就是「按文件名当放映顺序」。
//!
//! ODF 演示文稿（odp）没有母版/版式那套层级，页就是 `draw:page`，所以单独走一条路，
//! 并把「这条路给不出版式」写在 notes 里而不是硬凑一个空字段。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, resolve_target, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出演示文稿的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-slide",
    run = "run_office_slide",
    about = "Report a presentation's structure in show order: presentation.xml's sldId list decides that order (component filenames are NOT the order - slide12.xml can be the second slide), each slide is resolved through the package relationships to its own layout and, through the layout, to its master. Per slide it lists the title (the a:t text of the shape whose placeholder type is title/ctrTitle), every other paragraph with its placeholder type, shape/picture/table/chart counts, notes text from its notesSlide, transitions and whether the slide is hidden. Also reports slide size with its format name, the master and layout inventories, media, embedded fonts, themes and any embedded OLE objects. ODP is a different shape (pages are draw:page, there is no master/layout ladder) so it answers with what it has and says so. Legacy .ppt is identified here but its record tree is not parsed - the command reports that plainly. Returns { path, format, kind, order, slides, size, masters, layouts, media, notes, fonts, tables, watch }."
)]
pub struct OfficeSlide {
    /// 演示文稿（pptx / pptm / odp / ppt）
    #[arg(about = "Presentation to inspect", must_exist = true)]
    path: PathBuf,

    /// 每页最多列多少段落（总数照实给）
    #[arg(
        about = "List at most this many paragraphs per slide",
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

fn run_office_slide(app: &OfficeSlide, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the deck structure".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    let mut notes: Vec<String> = doc.notes.clone();

    if doc.family == Family::Ooxml && doc.app == "powerpoint" {
        let presentation = xml(bytes, "ppt/presentation.xml")
            .ok_or_else(|| AppError::InvalidInput("读不到 ppt/presentation.xml".to_string()))?;
        let root = xmlscan::parse_str(&presentation.as_text());
        let size = root
            .descendants("sldSz")
            .first()
            .map(|one| {
                json!({
                    "cx": one.attr("cx"),
                    "cy": one.attr("cy"),
                    "format": one.attr("type").unwrap_or("custom"),
                })
            })
            .unwrap_or(Value::Null);
        let rels = xml(bytes, "ppt/_rels/presentation.xml.rels")
            .map(|one| xmlscan::parse_str(&one.as_text()))
            .unwrap_or_else(|| xmlscan::parse_str(""));
        let id_to_part = |want: &str| -> Option<String> {
            rels.descendants("Relationship")
                .iter()
                .find(|one| one.attr("Id") == Some(want))
                .map(|one| resolve_target("ppt", one.attr("Target").unwrap_or_default()))
        };
        let order: Vec<Value> = root
            .descendants("sldId")
            .iter()
            .filter_map(|one| {
                let id = one.attr("id")?;
                let rid = one.attr_local("id")?;
                Some(json!({"show_index": id, "r_id": rid, "part": id_to_part(rid)}))
            })
            .collect();
        let mut slides: Vec<Value> = Vec::new();
        for entry in order.iter() {
            let part = match entry["part"].as_str() {
                Some(one) => one.to_string(),
                None => {
                    notes.push(format!("放映顺序里有一页指不到部件：{entry}"));
                    continue;
                }
            };
            let member = match xml(bytes, &part) {
                Some(one) => one,
                None => {
                    notes.push(format!("{part} 在放映顺序里，但部件读不出来"));
                    continue;
                }
            };
            let slide_root = xmlscan::parse_str(&member.as_text());
            let mut title = String::new();
            let mut paragraphs: Vec<Value> = Vec::new();
            let mut total_paragraphs = 0usize;
            for shape in slide_root.descendants("sp") {
                let kind = shape
                    .descendants("ph")
                    .first()
                    .and_then(|one| one.attr("type"))
                    .unwrap_or("other")
                    .to_string();
                for one in shape.descendants("p") {
                    total_paragraphs += 1;
                    let text = crate::office_text::paragraph_text(one);
                    if kind == "title" || kind == "ctrTitle" {
                        if title.is_empty() {
                            title = text.clone();
                        }
                        continue;
                    }
                    if text.is_empty() || paragraphs.len() >= limit {
                        continue;
                    }
                    paragraphs.push(json!({"placeholder": kind, "text": text}));
                }
            }
            // 备注页部件名是页部件名的固定换写：slides/slideN → notesSlides/notesSlideN
            let note_part = part.replace("/slides/slide", "/notesSlides/notesSlide");
            let note_text = xml(bytes, &note_part)
                .map(|one| {
                    let note_root = xmlscan::parse_str(&one.as_text());
                    note_root
                        .descendants("p")
                        .iter()
                        .map(|one| crate::office_text::paragraph_text(one))
                        .filter(|one| !one.is_empty())
                        .collect::<Vec<String>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let slide_rels = xml(bytes, &format!("{part}.rels")).map(|one| {
                let rel_root = xmlscan::parse_str(&one.as_text());
                rel_root
                    .descendants("Relationship")
                    .iter()
                    .filter_map(|one| {
                        let kind = one.attr("Type")?.rsplit('/').next()?.to_string();
                        let raw = one.attr("Target")?;
                        Some(json!({
                            "kind": kind,
                            "target": if one.attr("TargetMode") == Some("External") {
                                json!(raw)
                            } else {
                                json!(resolve_target("ppt/slides", raw))
                            },
                            "external": one.attr("TargetMode") == Some("External"),
                        }))
                    })
                    .collect::<Vec<Value>>()
            });
            slides.push(json!({
                "part": part,
                "show_index": entry["show_index"],
                "title": title,
                "paragraph_total": total_paragraphs,
                "paragraphs": paragraphs,
                "shapes": slide_root.descendants("sp").len(),
                "pictures": slide_root.descendants("pic").len(),
                "tables": slide_root.descendants("tbl").len(),
                "graphic_frames": slide_root.descendants("graphicFrame").len(),
                "media_frames": slide_root.descendants("videoFile").len()
                    + slide_root.descendants("audioCd").len(),
                "transition": slide_root.descendants("transition").len(),
                "hidden": slide_root
                    .descendants("sld")
                    .first()
                    .map(|one| one.attr("show") == Some("0"))
                    .unwrap_or(false),
                "notes": note_text,
                "relationships": slide_rels.unwrap_or_default(),
            }));
        }
        let masters = doc
            .entries
            .iter()
            .map(|one| one.name.clone())
            .filter(|one| one.starts_with("ppt/slideMasters/slideMaster") && one.ends_with(".xml"))
            .collect::<Vec<String>>();
        let layouts = doc
            .entries
            .iter()
            .map(|one| one.name.clone())
            .filter(|one| one.starts_with("ppt/slideLayouts/slideLayout") && one.ends_with(".xml"))
            .collect::<Vec<String>>();
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "presentationml",
            "order": order,
            "slides": slides,
            "size": size,
            "masters": masters,
            "layouts": layouts,
            "media": doc.entries.iter().filter(|one| one.name.starts_with("ppt/media/")).count(),
            "media_parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("ppt/media/")).collect::<Vec<String>>(),
            "fonts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("ppt/fonts/")).collect::<Vec<String>>(),
            "themes": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("ppt/theme/")).collect::<Vec<String>>(),
            "custom_slide_shows": root.descendants("custShow").len(),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.family == Family::Odf && doc.app == "powerpoint" {
        let content = xml(bytes, "content.xml")
            .ok_or_else(|| AppError::InvalidInput("读不到 content.xml".to_string()))?;
        let root = xmlscan::parse_str(&content.as_text());
        let pages = root.descendants("page");
        let mut slides: Vec<Value> = Vec::new();
        for (index, one) in pages.iter().enumerate() {
            let texts: Vec<String> = one
                .descendants("p")
                .iter()
                .map(|one| crate::office_text::paragraph_text(one))
                .filter(|one| !one.is_empty())
                .collect();
            slides.push(json!({
                "index": index,
                "name": one.attr("name").unwrap_or_default(),
                "title": texts.first().cloned().unwrap_or_default(),
                "texts": texts,
                "frames": one.descendants("frame").len(),
                "pictures": one.descendants("image").len(),
                "tables": one.descendants("table").len(),
            }));
        }
        notes.push("ODP 没有母版/版式那套层级，这条命令对它只报页与页里的文本框".to_string());
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-presentation",
            "order": Value::Array((0..pages.len()).map(|one| json!({"show_index": one})).collect()),
            "slides": slides,
            "size": root.descendants("presentation").first().and_then(|one| {
                one.attr("width").map(|width| json!({"width": width, "height": one.attr("height")}))
            }).unwrap_or(Value::Null),
            "masters": [],
            "layouts": [],
            "media": doc.entries.iter().filter(|one| one.name.starts_with("Pictures/")).count(),
            "media_parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("Pictures/")).collect::<Vec<String>>(),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.format == "ppt" {
        notes.push(
            "PowerPoint 97 的 .ppt 是记录树（PowerPoint Document 流）：本版本能识别与读属性，\
             还没解析其中的文本原子"
                .to_string(),
        );
        return Ok(json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "powerpoint-binary",
            "slides": [],
            "notes": notes,
        }));
    }
    Err(AppError::InvalidInput(format!(
        "{} 不是演示文稿（识别为 {} / {}）；表格用 office-sheet，文档用 office-doc",
        app.path.display(),
        doc.app,
        doc.format
    )))
}

fn xml(bytes: &[u8], want: &str) -> Option<zipread::Member> {
    zipread::member(bytes, want, DEFAULT_MEMBER_CAP).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeSlide {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 100,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_slide(&app, &Context::new_test(tx)).expect("office-slide 应成功")
    }

    /// python-pptx 那份两页的稿子：顺序、标题、备注、媒体、版式数
    /// （期望值来自 `office_reader.py` 的 pptx_facts）
    #[test]
    fn lists_two_slides_in_show_order() {
        let out = run("deck.pptx");
        assert_eq!(out["kind"], "presentationml");
        let slides = out["slides"].as_array().expect("是数组");
        assert_eq!(slides.len(), 2, "{out}");
        assert_eq!(slides[0]["part"], "ppt/slides/slide1.xml");
        assert_eq!(slides[0]["title"], "预算评审");
        assert_eq!(slides[1]["title"], "第二页：数字");
        assert_eq!(slides[0]["notes"], "评审时先讲口径再讲数字", "备注要单独读");
        assert_eq!(slides[0]["pictures"], 1);
        assert_eq!(slides[1]["tables"], 1, "第二页那张表：{}", slides[1]);
        assert_eq!(out["size"]["cx"], 9144000, "{out}");
        assert_eq!(out["size"]["format"], "screen4x3");
        assert_eq!(out["masters"].as_array().expect("是数组").len(), 1);
        assert_eq!(out["layouts"].as_array().expect("是数组").len(), 11);
        assert_eq!(out["media"], 1);
        assert_eq!(out["media_parts"][0], "ppt/media/image1.png");
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// 顺序以 presentation.xml 为准：sldId 的 r:id 要解析成真实部件
    #[test]
    fn the_order_comes_from_the_presentation_not_the_filename() {
        let out = run("deck.pptx");
        let order = out["order"].as_array().expect("是数组");
        assert_eq!(order.len(), 2);
        assert!(order.iter().all(|one| !one["part"].is_null()), "{order:?}");
        assert_eq!(order[0]["part"], "ppt/slides/slide1.xml");
        assert_eq!(order[1]["part"], "ppt/slides/slide2.xml");
        let parts = out["slides"].as_array().expect("是数组");
        assert_eq!(parts[0]["show_index"], order[0]["show_index"]);
    }

    /// ODP 是 LibreOffice 写的那份：只报它真有的东西，并把局限写在 notes 里
    #[test]
    fn odp_reports_pages_and_says_what_it_lacks() {
        let out = run("deck.odp");
        assert_eq!(out["kind"], "opendocument-presentation");
        let slides = out["slides"].as_array().expect("是数组");
        assert!(!slides.is_empty(), "{out}");
        assert_eq!(slides[0]["title"], "预算评审", "{}", slides[0]);
        assert!(slides[0]["texts"].as_array().expect("是数组").len() >= 2);
        assert!(slides[1]["texts"]
            .as_array()
            .expect("是数组")
            .iter()
            .any(|one| one.as_str().unwrap_or("").contains("124000")));
        assert!(out["masters"].as_array().expect("是数组").is_empty());
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("母版"), "{note}");
    }

    /// .ppt 只到「认出来」这一步，要明说而不是给一份空页表
    #[test]
    fn legacy_ppt_says_the_record_tree_is_not_parsed() {
        let out = run("deck.ppt");
        assert_eq!(out["kind"], "powerpoint-binary");
        assert!(out["slides"].as_array().expect("是数组").is_empty());
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("记录树"), "{note}");
    }

    #[test]
    fn a_workbook_is_not_a_deck() {
        let app = OfficeSlide {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/book.xlsx"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_slide(&app, &Context::new_test(tx)).unwrap_err();
        assert!(why.to_string().contains("office-sheet"), "{why}");
    }
}
