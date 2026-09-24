//! `lbin office-slide` — 演示文稿的结构：每页标题、备注、版式与母版、媒体与切换。
//!
//! 一份 pptx 的「页」散在三个地方：`ppt/presentation.xml` 里的 sldId 列表决定**放映顺序**
//! （部件名里的数字不是顺序！`slide12.xml` 可能排在第 2 页），每页的版式与母版靠关系表指，
//! 备注页是另一组部件。这三件事混在一起最容易做出的错答案就是「按文件名当放映顺序」。
//!
//! ODF 演示文稿（odp）走另一条路：页是 `draw:page`，页名在 `draw:name` 上，
//! 备注在 `presentation:notes` 里那个 `presentation:class="notes"` 的框里 ——
//! 那个框旁边还坐着页码占位（样字「<编号>」）与缩略图，混着读就等于把占位符当正文。
//! 尺寸也不在页上：`draw:master-page-name` → 样式文件里的 `style:master-page`
//! → `style:page-layout-name` → 那个版式的 `style:page-layout-properties`。
//!
//! 遗留的 `.ppt`（PowerPoint 97）连包都不是，是一棵记录树：页按 `recType 0x03EE` 的容器
//! 归（一页一个），这条对应关系是拿同一份文件的 pptx 逐张对出来的 —— 见 `ppt.rs`。

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
    about = "Report a presentation's structure in show order: presentation.xml's sldId list decides that order (component filenames are NOT the order - slide12.xml can be the second slide), each slide is resolved through the package relationships to its own layout and, through the layout, to its master. Per slide it lists the title (the a:t text of the shape whose placeholder type is title/ctrTitle), every other paragraph with its placeholder type, shape/picture/table/chart counts, notes text from its notesSlide, transitions and whether the slide is hidden. Also reports slide size (cx/cy as numbers in EMU plus the file's own type attribute), the master and layout inventories, media, embedded fonts, themes and any embedded OLE objects. ODP answers with its own ladder: pages are draw:page (name on draw:name), the title comes from the frame whose presentation:class is title, speaker notes are the presentation:class=notes frame inside presentation:notes - the page-number placeholder sitting next to it holds the literal sample text <编号> and is never reported as slide content - and the page size is resolved through draw:master-page-name to styles.xml's style:master-page and then its style:page-layout. A file may name a presentation page layout (presentation-page-layout-name) without carrying any definition for it, which this command reports instead of inventing one. Legacy .ppt is a PowerPoint 97 record tree rather than a package: it reports the record / container / text-atom counts and one entry per slide, because containers of recType 0x03EE occur exactly one per slide and their subtrees hold that slide's text atoms - a correspondence this reader measured against the very same document's .pptx form (count, order, and every line), not a name it copied from the spec, which is why the entries carry record offsets and not spec names. Text that belongs to no such container (master and layout placeholder wording) is counted but not attributed to a page. Returns { path, format, kind, order, slides, size, masters, layouts, media, notes, fonts, tables, watch }."
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
                    "cx": emu(one.attr("cx")),
                    "cy": emu(one.attr("cy")),
                    // 文件自己写的那个属性就叫 type；这里若也叫 format，
                    // 同一个 JSON 里「format」就会一会儿指文件类型、一会儿指画幅。
                    "type": one.attr("type").unwrap_or("custom"),
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
                let rid = rel_id(one)?;
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
        // 页面尺寸不在页上：draw:page 只写 master-page-name，尺寸在样式文件里
        // style:master-page → style:page-layout-name → 那个版式的 page-layout-properties。
        // 四跳，跟 xlsx 的 `s=` 绕 cellXfs 是同一类账。
        let mut layout_of_master: Vec<(String, String)> = Vec::new();
        let mut size_of_layout: Vec<(String, String, String, String)> = Vec::new();
        if let Some(styles) = xml(bytes, "styles.xml") {
            let sheet = xmlscan::parse_str(&styles.as_text());
            for one in sheet.descendants("master-page") {
                let name = crate::odsheet::attr_of(one, "name")
                    .unwrap_or_default()
                    .to_string();
                let layout = crate::odsheet::attr_of(one, "page-layout-name")
                    .unwrap_or_default()
                    .to_string();
                layout_of_master.push((name, layout));
            }
            for one in sheet.descendants("page-layout") {
                let name = crate::odsheet::attr_of(one, "name")
                    .unwrap_or_default()
                    .to_string();
                if let Some(props) = one.child("page-layout-properties") {
                    size_of_layout.push((
                        name,
                        crate::odsheet::attr_of(props, "page-width")
                            .unwrap_or_default()
                            .to_string(),
                        crate::odsheet::attr_of(props, "page-height")
                            .unwrap_or_default()
                            .to_string(),
                        crate::odsheet::attr_of(props, "print-orientation")
                            .unwrap_or_default()
                            .to_string(),
                    ));
                }
            }
        }
        let pages = root.descendants("page");
        let mut slides: Vec<Value> = Vec::new();
        let mut masters: Vec<String> = Vec::new();
        let mut layouts: Vec<String> = Vec::new();
        let mut size = Value::Null;
        for (index, one) in pages.iter().enumerate() {
            let master = crate::odsheet::attr_of(one, "master-page-name")
                .unwrap_or_default()
                .to_string();
            let layout = crate::odsheet::attr_of(one, "presentation-page-layout-name")
                .unwrap_or_default()
                .to_string();
            if !master.is_empty() && !masters.iter().any(|had| *had == master) {
                masters.push(master.clone());
            }
            if !layout.is_empty() && !layouts.iter().any(|had| *had == layout) {
                layouts.push(layout.clone());
            }
            // 备注那一块（presentation:notes）里的字不是页面上的字：混进 texts，
            // 读的人会以为幻灯片上写着「<编号>」。页与备注块里的 `draw:frame` 都是
            // 各自的直接孩子，所以按孩子取就分得清，不用比指针。
            let notes_node = one.child("notes");
            let visible = one.all("frame");
            let in_notes = notes_node
                .map(|owner| owner.all("frame"))
                .unwrap_or_default();
            let mut texts: Vec<String> = Vec::new();
            let mut placeholders: Vec<String> = Vec::new();
            let mut notes_text = String::new();
            let mut title = String::new();
            let lines_of = |frame: &xmlscan::Node| -> Vec<String> {
                frame
                    .descendants("p")
                    .iter()
                    .map(|had| crate::office_text::paragraph_text(had))
                    .filter(|had| !had.is_empty())
                    .collect()
            };
            for frame in &visible {
                let frame = *frame;
                let class = crate::odsheet::attr_of(frame, "class").unwrap_or_default();
                if !class.is_empty() && !placeholders.iter().any(|had| had == class) {
                    placeholders.push(class.to_string());
                }
                let lines = lines_of(frame);
                if class == "title" && lines.first().is_some() && title.is_empty() {
                    title = lines.first().cloned().unwrap_or_default();
                }
                texts.extend(lines);
            }
            for frame in &in_notes {
                let frame = *frame;
                if crate::odsheet::attr_of(frame, "class") == Some("notes") {
                    notes_text = lines_of(frame).join("\n");
                }
            }
            let title = if title.is_empty() {
                // 没有 title 占位的页：退回第一段。这是退回来，不是文件这么标的
                texts.first().cloned().unwrap_or_default()
            } else {
                title
            };
            if size == Value::Null {
                size = layout_of_master
                    .iter()
                    .find(|(name, _)| *name == master)
                    .and_then(|(_, layout)| {
                        size_of_layout.iter().find(|(name, _, _, _)| name == layout)
                    })
                    .map(|(_, width, height, orientation)| {
                        json!({
                            "page_width": width,
                            "page_height": height,
                            "orientation": orientation,
                            "master_page": master.as_str(),
                        })
                    })
                    .unwrap_or(Value::Null);
            }
            slides.push(json!({
                "index": index,
                "name": crate::odsheet::attr_of(one, "name").unwrap_or_default(),
                "title": title,
                "master": master,
                "layout": layout,
                "placeholders": placeholders,
                "texts": texts,
                "notes": notes_text,
                "paragraph_total": texts.len(),
                "frames": one.descendants("frame").len(),
                "pictures": one.descendants("image").len(),
                "tables": one.descendants("table").len(),
            }));
        }
        notes.push(format!(
            "ODP 的层级是 draw:page → master-page-name → 样式文件里的 page-layout：\
             这份用到 {} 个母版页、{} 个版式名；尺寸是按这条链从 styles.xml 读出来的\
             （{}），不在页上",
            masters.len(),
            layouts.len(),
            if size.is_null() {
                "读不出来"
            } else {
                "读出来了"
            }
        ));
        notes.push(
            "页上写的 presentation-page-layout-name 只是名字：这份文件里没有对应的版式定义，\
             所以那条链报不了每格放在哪"
                .to_string(),
        );
        if masters.is_empty() {
            notes.push("这些页没写 master-page-name".to_string());
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-presentation",
            "order": Value::Array((0..pages.len()).map(|one| json!({"show_index": one})).collect()),
            "slides": slides,
            "size": size,
            "masters": masters,
            "layouts": layouts,
            "media": doc.entries.iter().filter(|one| one.name.starts_with("Pictures/")).count(),
            "media_parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("Pictures/")).collect::<Vec<String>>(),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.format == "ppt" {
        // 97 的 .ppt 是一棵记录树。按页归位这条不是照规范背的（recType 的名字我
        // 没有出处）：它是拿同一份文档的另一副面孔对出来的 —— 流里 recType 0x03EE
        // 的容器恰好一页一个，各自子树里的文字原子与 deck.pptx 的
        // ppt/slides/slideN.xml 逐张一致（张数、顺序、每行的字都对得上）。
        let mut deck_json = Value::Null;
        let mut ppt_slides: Vec<Value> = Vec::new();
        match doc
            .compound
            .as_ref()
            .map(|cfb| crate::ppt::read(cfb, bytes))
        {
            Some(Ok(deck)) => {
                for (index, one) in deck.slides.iter().enumerate() {
                    ppt_slides.push(json!({
                        "index": index,
                        "title": one.lines.first().cloned().unwrap_or_default(),
                        "texts": one.lines,
                        "paragraph_total": one.lines.len(),
                        "record_offset": one.offset,
                        "depth": one.depth,
                        "text_atoms": one.atoms,
                        "layout_name": one.name,
                    }));
                }
                notes.push(format!(
                    "记录树走过 {} 条记录，文本原子 {} 个；按页归好 {} 页 \
                     （recType 0x03EE 的容器一页一个，这个对应关系是与同一份文件的 \
                     pptx 逐张对出来的，不是照规范命名 —— 那份规范我手上没有）；\
                     没归进页的那 {} 个原子是备注页的字与母版、版式里的占位文字，\
                     逐条见 office-text",
                    deck.records,
                    deck.atoms.len(),
                    deck.slides.len(),
                    deck.atoms
                        .len()
                        .saturating_sub(deck.slides.iter().map(|one| one.atoms).sum::<usize>()),
                ));
                notes.extend(deck.notes.iter().cloned());
                deck_json = json!({
                    "records": deck.records,
                    "text_atoms": deck.atoms.len(),
                    "containers": deck.containers,
                    "slide_containers": deck.slide_containers,
                    "grouped_pages": deck.slides.len(),
                    "atoms": deck.atoms.iter().map(|one| json!({
                        "kind": one.kind, "depth": one.depth, "text": one.text,
                    })).collect::<Vec<Value>>(),
                });
            }
            Some(Err(why)) => notes.push(why),
            None => notes.push("复合文档打不开（头或 FAT 读不出），记录树也无从谈起".to_string()),
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "powerpoint-binary",
            "slides": ppt_slides,
            "record_tree": deck_json,
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
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

/// `<p:sldId id="256" r:id="rId2"/>`：两个属性的**局部名**都叫 `id`，按局部名去找
/// 就会拿到放映序号当关系号（CI 上真这么错了）。关系号一定带前缀 —— `r:` 这个名字
/// 是文档自己声明的，但前缀总在 —— 所以判据是「不是裸 `id`，且以 `:id` 结尾」。
fn rel_id(node: &xmlscan::Node) -> Option<&str> {
    node.attrs
        .iter()
        .find(|(key, _)| key != "id" && key.ends_with(":id"))
        .map(|(_, value)| value.as_str())
}

/// 画幅是十进制整数字符串（EMU）：能数出来就给数，数不出来照原样给，不假装有值
fn emu(raw: Option<&str>) -> Value {
    match raw.and_then(|one| one.parse::<u64>().ok()) {
        Some(one) => json!(one),
        None => raw.map(|one| json!(one)).unwrap_or(Value::Null),
    }
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
        assert_eq!(out["size"]["type"], "screen4x3");
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

    /// ODP 是 LibreOffice 写的那份：页、版式引用、备注与尺寸各有来历，
    /// 而 `presentation:notes` 里的字**不算页面上的字**（期望值来自 `odp_facts()`）
    #[test]
    fn odp_reports_pages_notes_and_the_master_hop() {
        let out = run("deck.odp");
        assert_eq!(out["kind"], "opendocument-presentation");
        let slides = out["slides"].as_array().expect("是数组");
        assert_eq!(slides.len(), 2, "{out}");
        assert_eq!(slides[0]["name"], "预算评审", "页名在 draw:name 上：{out}");
        assert_eq!(slides[0]["title"], "预算评审");
        assert_eq!(slides[0]["master"], "Title_20_and_20_Content");
        assert_eq!(slides[0]["layout"], "AL1T11");
        assert_eq!(
            slides[0]["texts"],
            json!(["预算评审", "新增两台 64 核应用服务器", "第二条要点"]),
            "{}",
            slides[0]
        );
        assert_eq!(
            slides[0]["notes"], "评审时先讲口径再讲数字",
            "备注单独一条，不混进页面上的字"
        );
        assert_eq!(
            slides[0]["placeholders"],
            json!(["title", "outline"]),
            "页上的占位类别：{}",
            slides[0]
        );
        assert_eq!(slides[1]["title"], "第二页：数字");
        assert!(slides[1]["texts"]
            .as_array()
            .expect("是数组")
            .iter()
            .any(|one| one.as_str().unwrap_or("").contains("124000")));
        assert_eq!(slides[1]["notes"], "", "第二页没写备注");
        assert_eq!(slides[1]["tables"], 1, "那张表在页里：{}", slides[1]);
        // 「<编号>」是页码占位里的样字，不是这页写了什么
        let flat = serde_json::to_string(&slides).expect("序列化");
        assert!(!flat.contains("编号"), "页码占位的样字不许当正文：{flat}");
        let masters = out["masters"].as_array().expect("是数组");
        assert_eq!(masters.len(), 2, "{out}");
        let layouts = out["layouts"].as_array().expect("是数组");
        assert_eq!(layouts.len(), 2, "{layouts:?}");
        assert_eq!(out["size"]["page_width"], "25.4cm", "{out}");
        assert_eq!(out["size"]["page_height"], "19.05cm");
        assert_eq!(out["size"]["orientation"], "landscape");
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            note.contains("版式定义"),
            "文件里没有版式定义要说出来：{note}"
        );
    }

    /// .ppt 按页归位：recType 0x03EE 的容器一页一个，页里的文字与**同一份文档的
    /// pptx 那一副面孔**逐张一致（上面那个 pptx 测试里的两张标题、每行的字）。
    /// 归属关系有这份实测撑着，recType 的**规范名字**没有出处，所以只报数值
    #[test]
    fn legacy_ppt_groups_its_record_tree_one_entry_per_slide() {
        let out = run("deck.ppt");
        assert_eq!(out["kind"], "powerpoint-binary");
        assert_eq!(out["record_tree"]["text_atoms"], 67, "{out}");
        assert_eq!(out["record_tree"]["records"], 1427);
        assert_eq!(out["record_tree"]["slide_containers"], 11);
        let slides = out["slides"].as_array().expect("是数组");
        assert_eq!(slides.len(), 2, "{out}");
        assert_eq!(slides[0]["title"], "预算评审");
        assert_eq!(slides[1]["title"], "第二页：数字");
        assert_eq!(
            slides[0]["texts"],
            json!(["预算评审", "新增两台 64 核应用服务器", "第二条要点"]),
            "{slides[0]}"
        );
        assert_eq!(
            slides[1]["texts"],
            json!(["第二页：数字", "科目", "金额", "服务器", "124000"]),
            "{slides[1]}"
        );
        // 每页那条 CString 是 LibreOffice 写的版式名，不能混进正文行
        assert_eq!(slides[0]["layout_name"], "___PPT10");
        for one in slides.iter() {
            let lines = one["texts"].as_array().expect("texts 是数组");
            assert!(
                !lines
                    .iter()
                    .any(|line| line.as_str().unwrap_or("").starts_with("___PPT")),
                "版式名不算页面上的字：{one}"
            );
        }
        assert_eq!(slides[0]["record_offset"], 48900);
        assert_eq!(slides[1]["record_offset"], 50474);
        // 母版与版式里的占位文字不归任何一页：按页归好的原子总数比全流少
        let grouped: usize = slides
            .iter()
            .map(|one| one["text_atoms"].as_u64().unwrap_or(0) as usize)
            .sum();
        assert_eq!(grouped, 9, "{slides}");
        assert!(grouped < out["record_tree"]["text_atoms"].as_u64().unwrap_or(0) as usize);
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("记录树"), "{note}");
        assert!(
            note.contains("我手上没有"),
            "recType 的名字没有出处这件事要写明：{note}"
        );
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
