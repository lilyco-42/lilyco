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

use crate::office_sheet::written_attrs;
use crate::opack::{open, resolve_target, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出演示文稿的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-slide",
    run = "run_office_slide",
    about = "Report a presentation's structure in show order: presentation.xml's sldId list decides that order (component filenames are NOT the order - slide12.xml can be the second slide), each slide is resolved through the package relationships to its own layout and, through the layout, to its master. Per slide it lists the title (the a:t text of the shape whose placeholder type is title/ctrTitle), every other paragraph with its placeholder type, shape/picture/table/chart counts, notes text from its notesSlide, transitions and whether the slide is hidden. Also reports slide size (cx/cy as numbers in EMU plus the file's own type attribute), the master and layout inventories, media, embedded fonts, themes and any embedded OLE objects. ODP answers with its own ladder: pages are draw:page (name on draw:name), the title comes from the frame whose presentation:class is title, speaker notes are the presentation:class=notes frame inside presentation:notes - the page-number placeholder sitting next to it holds the literal sample text <编号> and is never reported as slide content - and the page size is resolved through draw:master-page-name to styles.xml's style:master-page and then its style:page-layout. A file may name a presentation page layout (presentation-page-layout-name) without carrying any definition for it, which this command reports instead of inventing one. Legacy .ppt is a PowerPoint 97 record tree rather than a package: it reports the record / container / text-atom counts and one entry per slide, because containers of recType 0x03EE occur exactly one per slide and their subtrees hold that slide's text atoms - a correspondence this reader measured against the very same document's .pptx form (count, order, and every line), not a name it copied from the spec, which is why the entries carry record offsets and not spec names. Text that belongs to no such container (master and layout placeholder wording) is counted but not attributed to a page. A slide's charts are read from the page's own relationships (only entries whose Type ends in `chart`), never by listing ppt/charts/: LibreOffice drops style and colors parts into that same directory, so counting files there would report six charts where the page carries two. Each chart reports its part, title, whether any value was cached, and per plot group the kind (barChart / pieChart ...), the direct children's val attributes as written, the axis ids kept apart (each producer numbers them differently, and python-pptx even writes negative ones) and one entry per series with the reference string and the cached points. The reference strings are NOT comparable across producers here: python-pptx writes the real hop into the chart's own embedded workbook (`Sheet1!$B$1`), while LibreOffice's pptx export puts literal labels in the same place (`label 0`, `categories`, `0`) - the cached numbers survive that rewrite unchanged, which is why both are reported instead of a single reconciled answer. Two counters keep runs and paragraphs apart: paragraph_total counts a:p inside p:sp, text_runs counts a:t, and the same deck from the two producers reads 3/1 paragraphs with 3 versus 5 runs - joining runs into paragraph text is what makes the wording comparable at all. The slide size's own type attribute is reported only when written: python-pptx says screen4x3, LibreOffice omits it for the identical cx/cy, and it is left null rather than being called custom. ODF answers the same question its own way: a `draw:frame` holds a `draw:object` whose `xlink:href` names an `Object N/` directory - the notes frame holds no such thing, so it is not a chart - and there the type sits on each `chart:series` (`chart:bar`, and `chart:circle` for a pie) rather than on an outer plot group, points are self-stated with `chart:repeated`, and a range can name the chart's own `local-table` (`local-table.$B$2:.$B$3`) instead of the deck's data, so those strings go over as written. Frame names count the notes frame too, which is why a page's first chart can be called Chart 2. Legacy .ppt keeps charts inside the record tree and is not read. Per pptx slide the tables are a ledger of their own (`table_list`): the `a:tblPr` attributes as written next to whether that element exists at all (python-pptx writes firstRow/bandRow plus an `a:tableStyleId`, while LibreOffice rewrites the same table with an EMPTY tblPr - present, saying nothing), the grid columns with their EMU width as written plus its 0.01mm reading, each row's h the same way, and every cell with its a:tc attributes, its a:tcPr attributes and child element names, the a:bodyPr attributes kept separately because LibreOffice writes the cell margins a second time in there, the paragraph text, and how many paragraphs and runs it holds. Merging is a third convention in this family and the reason three counters exist: the covered cell STAYS in the file (marked hMerge or vMerge, with empty text) while the origin carries gridSpan / rowSpan, so one row of a three-column table holds 3 cells whose spans add up to 4 - cells, spans and grid columns go out side by side instead of being reconciled. Row heights do not survive the rewrite either: the same unspecified row is 609600 EMU in one file and 609480 in the other, and the .odp written in between says 1.693cm - all three land on 1693 in 0.01mm, which is what makes the EMU conversion something a reader can check rather than take on trust. ODP keeps page tables as table:table inside a draw:frame, and the frame is the only place that names one: measured on deck-tables.odp the table element itself writes NO attribute at all while the frame carries name, x, y, width and height (17.779cm - the same grid the two pptx files write as 6400800 EMU), so the odp entry reports the frame's attributes, the table's own (empty here) attribute map, and the very same size ledger .ods uses, because a page table and a spreadsheet table are the same element and their widths again sit one hop away in the column styles. Merging is the third spelling of the three: the covered cell gets its own covered-table-cell element while the origin says number-columns-spanned / number-rows-spanned. A cell's value type is NOT guessed: Impress writes office:value-type on none of the nine cell elements of deck-tables.odp, seven of which hold text, so every odp cell comes back with kind null. The same rule now covers .ods cells, where the two readers had been guessing differently - one from whether the cell had text, one always calling an unwritten type "empty" - and only agreed because every .ods cell that reaches the ledger does write the attribute (measured over all six files). A cell's own style is one hop and it is read: style_props resolves the name the cell wrote against the family=table-cell styles of BOTH parts: measured here all five cell styles sit in content.xml and styles.xml holds no table-cell style at all, while the covered cells' standard is a family=graphic style of another kind - walking only one part would turn this reader's guess about the producer into a rule. The three property elements are handed over separately and each carries its own written name: the fill, the vertical alignment and the four paddings sit on loext:graphic-properties, LibreOffice's own experimental namespace rather than style:, the border that same style carries sits on style:paragraph-properties, and the style:table-cell-properties an odt table uses is written by none of the nine table-family styles of deck-tables.odp. Attribute names keep their prefixes for the same reason fo:text-indent and loext:text-indent must never collide. style / found / part tell which of the three cases a cell is in - wrote no name, named one nobody defines, or resolved - and the table adds the same counts as a tally. Per slide `links` answers 'what can a reader click here', and the two families spell it differently. OOXML needs two hops: the run's `a:rPr` carries only `a:hlinkClick/@r:id`, the address lives in that slide's own relationship part, with `TargetMode` reported as written (absent stays null, not false); ODF writes the address on the word itself (`text:a/@xlink:href`), so there is no second hop and no external switch at all - `external` and `id` come back null there rather than being filled in. The ids are each producer's own numbering (one file starts at rId2, its rewrite at rId1) while the addresses survive unchanged, so the ids go over as written and are never compared. Walking only `draw:frame` for ODP would read all three links as zero: Impress turns a plain text box into `draw:custom-shape`, so the walk covers everything the page holds except its `presentation:notes` block - a link typed in the notes is not a link on the slide. Returns { path, format, kind, order, slides, size, masters, layouts, media, notes, fonts, tables, watch }."
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
                    // 没写就是没写：python-pptx 那份写 `type="screen4x3"`，LibreOffice 重写
                    // 同一份稿子时把这个属性整个省掉（尺寸还是同一个数），替它填一个
                    // "custom" 就是替文件编东西 —— 尺寸那两个数才是这一家说过的话。
                    "type": one.attr("type").map(String::from),
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
            // 这一页的图：只认页自己关系表里 kind 是 chart 的那几条。LibreOffice 往
            // `ppt/charts/` 里另塞了 style 与 colors 部件，按目录数就会多数；
            // 两家的 Target 都是相对的（`../charts/chartN.xml`），解法同一家族
            let page_charts: Vec<Value> = crate::office_sheet::rels_of(bytes, &part)
                .into_iter()
                .filter(|(kind, _)| kind == "chart")
                .take(limit)
                .filter_map(|(_, target)| {
                    let member = xml(bytes, &target)?;
                    Some(crate::office_sheet::chart_one(
                        &xmlscan::parse_str(&member.as_text()),
                        &target,
                    ))
                })
                .collect();
            slides.push(json!({
                "part": part,
                "show_index": entry["show_index"],
                "charts": page_charts.len(),
                "chart_list": page_charts,
                "title": title,
                "paragraph_total": total_paragraphs,
                "paragraphs": paragraphs,
                // run 的条数（`a:t`）：同一段字在两家手里可以是一个 run 也可以是三个，
                // 段落数一致而这一数不同，正是 paragraph_text 那一步在替两边对上
                "text_runs": slide_root.descendants("t").len(),
                "shapes": slide_root.descendants("sp").len(),
                "pictures": slide_root.descendants("pic").len(),
                "tables": slide_root.descendants("tbl").len(),
                "table_list": slide_tables(&slide_root, limit),
                // 页上的链接：两跳 —— run 里只有号，地址在这一页的关系表里
                "links": slide_links(bytes, &part, &slide_root, limit),
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
        // 页上那张表：表住在 `draw:frame` 里，**表名与位置都写在 frame 上**
        // （`table:table` 自己实测一个字都不写），所以这张表的账要两份数据：frame 那一份
        // 与 ODF 表自己那一份。表的读法与 .ods 完全同一条（`odsheet`），配对按文档顺序
        let workbook = crate::odsheet::read(bytes);
        // 格子点的那份样式另要一遍：三处 properties 的名字与值都在样式文件里，
        // 而页上的表只写一个名字（`table:style-name`）
        let cell_styles = odp_cell_styles(bytes);
        let mut table_at = 0usize;
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
            // 链接那一本要的是「页上除备注以外的那几块」：文本框在 Impress 手里会变成
            // `draw:custom-shape`，只认 frame 就会把页面上的链读成 0
            let page_owners: Vec<&xmlscan::Node> = one
                .children
                .iter()
                .filter(|kid| kid.local() != "notes")
                .collect();
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
            // 这一页上的图：ODP 与 ODS 同一家存法 —— frame 里那条 draw:object 指过去
            let charts = crate::odfchart::charts_in(bytes, one);
            // 这一页上带表的 frame：表名与位置在 frame 上，行的账与列的账在 ODF 表那一份上
            let mut page_tables: Vec<Value> = Vec::new();
            let mut on_page = 0usize;
            for holder in one
                .all("frame")
                .iter()
                .filter(|had| had.child("table").is_some())
                .take(limit)
            {
                let Some(tbl) = holder.child("table") else {
                    continue;
                };
                match workbook.sheets.get(table_at) {
                    Some(sheet) => {
                        table_at += 1;
                        page_tables.push(odp_page_table(
                            holder,
                            tbl,
                            sheet,
                            &cell_styles,
                            on_page,
                            limit,
                        ));
                        on_page += 1;
                    }
                    None => {
                        notes.push(format!(
                            "第 {} 页上还有带表的 frame，可表那一份只数到 {} 张，后面不配了",
                            index + 1,
                            table_at
                        ));
                        break;
                    }
                }
            }
            slides.push(json!({
                "index": index,
                "charts": charts.len(),
                "chart_list": charts,
                "table_list": page_tables,
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
                // 这一族的链接直接挂在字上，走页上除备注以外的那几块
                "links": odp_links(&page_owners, limit),
            }));
        }
        // 表那一份读的时候也有自己的话要说（格子元素太多、content.xml 读不出来…）
        notes.extend(workbook.notes.iter().cloned());
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

/// 「这一元素顶几个」：没写或写坏了按 1（与 .ods 那族 `number-columns-repeated` 同一口径）
fn span_of(node: &xmlscan::Node, want: &str) -> usize {
    node.attr_local(want)
        .and_then(|one| one.trim().parse::<usize>().ok())
        .filter(|one| *one > 0)
        .unwrap_or(1)
}

/// 换成 0.01mm 的那一份：数不出来就交 null（不替它猜单位）
fn emu_mm(node: &xmlscan::Node, want: &str) -> Value {
    match node.attr_local(want).and_then(|one| crate::paper::emu(one)) {
        Some(one) => json!(one),
        None => Value::Null,
    }
}

/// 这一页上那张表的网（pptx）：`a:tblPr`（表自己说的开关与那条 `tableStyleId`）、
/// `a:tblGrid/a:gridCol`（列宽是 EMU）、`a:tr`（行高也是 EMU）、每格 `a:tc` 与它的 `a:tcPr`。
///
/// 合并在这一族是**被合掉的那一格照样在场**（`hMerge` / `vMerge`，字是空的），
/// 起点那格写 `gridSpan` / `rowSpan` —— 所以「一行的几个格」「跨度之和」与网格的「几列」
/// 是三个数：实测这张三列的表第一行是 3 个格、跨度之和 4。三个都交，不替它们对成一个
fn slide_tables(slide_root: &xmlscan::Node, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for (index, tbl) in slide_root.descendants("tbl").into_iter().enumerate() {
        let holder = tbl.child("tblPr");
        let style = holder.and_then(|one| one.child("tableStyleId"));
        let mut grid: Vec<Value> = Vec::new();
        let mut columns = 0usize;
        let mut grid_sum: i64 = 0;
        if let Some(one) = tbl.child("tblGrid") {
            for column in one.all("gridCol") {
                columns += 1;
                if let Some(raw) = column
                    .attr_local("w")
                    .and_then(|one| one.trim().parse::<i64>().ok())
                {
                    grid_sum = grid_sum.saturating_add(raw);
                }
                if grid.len() < limit {
                    grid.push(json!({
                        "written": written_attrs(column),
                        "w": emu(column.attr_local("w")),
                        "mm": emu_mm(column, "w"),
                    }));
                }
            }
        }
        let mut rows: Vec<Value> = Vec::new();
        let mut row_elements = 0usize;
        let mut cell_elements = 0usize;
        let mut span_total = 0usize;
        let mut with_text = 0usize;
        let mut merged_from = 0usize;
        let mut spanning = 0usize;
        for row in tbl.all("tr") {
            row_elements += 1;
            let mut cells: Vec<Value> = Vec::new();
            let mut row_cells = 0usize;
            let mut row_spans = 0usize;
            let mut row_text = 0usize;
            for (at, tc) in row.all("tc").iter().enumerate() {
                row_cells += 1;
                let span_cols = span_of(tc, "gridSpan");
                let span_rows = span_of(tc, "rowSpan");
                row_spans += span_cols;
                if span_cols > 1 || span_rows > 1 {
                    spanning += 1;
                }
                if tc.attr_local("hMerge").is_some() || tc.attr_local("vMerge").is_some() {
                    merged_from += 1;
                }
                let props = tc.child("tcPr");
                let body = tc.child("txBody");
                let mut paragraphs: Vec<&xmlscan::Node> = Vec::new();
                if let Some(one) = body {
                    paragraphs = one.all("p");
                }
                let text = paragraphs
                    .iter()
                    .map(|one| crate::office_text::paragraph_text(one))
                    .collect::<Vec<String>>()
                    .join("\n");
                if !text.is_empty() {
                    with_text += 1;
                    row_text += 1;
                }
                let mut runs = 0usize;
                if let Some(one) = body {
                    runs = one.descendants("r").len();
                }
                let mut paths: Vec<String> = Vec::new();
                if let Some(one) = props {
                    paths = one
                        .children
                        .iter()
                        .filter(|kid| kid.local() != "#text")
                        .map(|kid| kid.local().to_string())
                        .collect();
                }
                let mut inner = Value::Null;
                if let Some(one) = body {
                    if let Some(holder) = one.child("bodyPr") {
                        inner = written_attrs(holder);
                    }
                }
                if cells.len() < limit {
                    cells.push(json!({
                        "at": at,
                        "written": written_attrs(tc),
                        "span_cols": span_cols,
                        "span_rows": span_rows,
                        "merge_from": tc.attr_local("hMerge").is_some()
                            || tc.attr_local("vMerge").is_some(),
                        "tcpr_present": props.is_some(),
                        "tcpr": match props {
                            Some(one) => written_attrs(one),
                            None => Value::Null,
                        },
                        "tcpr_paths": paths,
                        "body": inner,
                        "text": text,
                        "paragraphs": paragraphs.len(),
                        "runs": runs,
                    }));
                }
            }
            cell_elements += row_cells;
            span_total += row_spans;
            if rows.len() < limit {
                rows.push(json!({
                    "written": written_attrs(row),
                    "h": emu(row.attr_local("h")),
                    "mm": emu_mm(row, "h"),
                    "cells": row_cells,
                    "span_sum": row_spans,
                    "with_text": row_text,
                    "list": cells,
                }));
            }
        }
        out.push(json!({
            "at": index,
            "pr_present": holder.is_some(),
            "written": match holder {
                Some(one) => written_attrs(one),
                None => Value::Null,
            },
            "style_present": style.is_some(),
            "style_id": match style {
                Some(one) => json!(one.text().trim()),
                None => Value::Null,
            },
            "grid": grid,
            "column_elements": columns,
            "grid_sum": json!(grid_sum),
            "grid_sum_mm": match crate::paper::emu(&grid_sum.to_string()) {
                Some(one) => json!(one),
                None => Value::Null,
            },
            "rows": rows,
            "row_elements": row_elements,
            "cell_elements": cell_elements,
            "span_sum": span_total,
            "with_text": with_text,
            "merged_from": merged_from,
            "spanning": spanning,
        }));
    }
    out
}

/// 一处 properties：元素名按文件写的交（`loext:graphic-properties` 与
/// `style:graphic-properties` 是两回事），属性也按文件写的名字交（前缀留着）
#[derive(Clone, Default)]
struct OdpProps {
    element: String,
    attrs: Value,
}

/// 一格点的那份 `style:style`（family=table-cell）：三处 properties 分别住在哪儿、写了什么
#[derive(Clone, Default)]
struct OdpCellStyle {
    name: String,
    part: &'static str,
    family: Option<String>,
    parent: Option<String>,
    graphic: Option<OdpProps>,
    paragraph: Option<OdpProps>,
    table_cell: Option<OdpProps>,
}

fn odp_props_of(style: &xmlscan::Node, local: &str) -> Option<OdpProps> {
    let had = style.children.iter().find(|one| one.local() == local)?;
    Some(OdpProps {
        element: had.name.clone(),
        attrs: crate::office_doc::kept_attrs(had),
    })
}

/// 两份件里所有 family=table-cell 的样式。两份都走，而不是只走 content.xml：这一族的
/// 五份格子样式实测全在 content.xml，`styles.xml` 里一份 family=table-cell 都没有，
/// 而占位格点的那份 `standard` 是 **family=graphic** 的另一个东西（占位格不进账本，
/// 所以这里数不到它）。只读一份就等于替文件定规矩；同名先到的一条算数
fn odp_cell_styles(bytes: &[u8]) -> Vec<OdpCellStyle> {
    let mut out: Vec<OdpCellStyle> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let Some(member) = xml(bytes, part) else {
            continue;
        };
        let root = xmlscan::parse_str(&member.as_text());
        for one in root.descendants("style") {
            if crate::odsheet::attr_of(one, "family") != Some("table-cell") {
                continue;
            }
            let Some(name) = crate::odsheet::attr_of(one, "name") else {
                continue;
            };
            let name = name.to_string();
            if out.iter().any(|had: &OdpCellStyle| had.name == name) {
                continue;
            }
            out.push(OdpCellStyle {
                graphic: odp_props_of(one, "graphic-properties"),
                paragraph: odp_props_of(one, "paragraph-properties"),
                table_cell: odp_props_of(one, "table-cell-properties"),
                family: crate::odsheet::attr_of(one, "family").map(|had| had.to_string()),
                parent: crate::odsheet::attr_of(one, "parent-style-name")
                    .map(|had| had.to_string()),
                name,
                part,
            });
        }
    }
    out
}

/// 一格的样式那一跳：没点名、点了名却没有那份样式、点到了 —— 三件事都要看得出来，
/// 所以 `found` 之外还留着 `style` 那个名字，而找不着时下面几样一律 null（不是空对象）
fn odp_style_props(styles: &[OdpCellStyle], named: Option<&str>) -> Value {
    let Some(had) = named.and_then(|want| styles.iter().find(|one| one.name == want)) else {
        return json!({
            "style": named,
            "found": false,
            "part": Value::Null,
            "family": Value::Null,
            "parent": Value::Null,
            "graphic_element": Value::Null,
            "graphic": Value::Null,
            "paragraph_element": Value::Null,
            "paragraph": Value::Null,
            "table_cell_element": Value::Null,
            "table_cell": Value::Null,
        });
    };
    json!({
        "style": named,
        "found": true,
        "part": had.part,
        "family": had.family.clone(),
        "parent": had.parent.clone(),
        "graphic_element": had.graphic.as_ref().map(|one| one.element.clone()),
        "graphic": had.graphic.as_ref().map(|one| one.attrs.clone()),
        "paragraph_element": had.paragraph.as_ref().map(|one| one.element.clone()),
        "paragraph": had.paragraph.as_ref().map(|one| one.attrs.clone()),
        "table_cell_element": had.table_cell.as_ref().map(|one| one.element.clone()),
        "table_cell": had.table_cell.as_ref().map(|one| one.attrs.clone()),
    })
}

/// 这张表上的格子与那三处 properties：几格点了名、几格解开、几格根本没点，
/// 以及解开的那些里每一处 properties 各有几格（元素名按写的去重列出来）
fn odp_cell_tally(sheet: &crate::odsheet::Sheet, styles: &[OdpCellStyle]) -> Value {
    let mut named = 0usize;
    let mut resolved = 0usize;
    let mut unwritten = 0usize;
    let mut graphic = 0usize;
    let mut paragraph = 0usize;
    let mut table_cell = 0usize;
    let mut elements: Vec<String> = Vec::new();
    for had in &sheet.cells {
        let Some(want) = had.style_name.as_deref() else {
            unwritten += 1;
            continue;
        };
        named += 1;
        let Some(style) = styles.iter().find(|one| one.name == want) else {
            continue;
        };
        resolved += 1;
        if let Some(one) = &style.graphic {
            graphic += 1;
            if !elements.contains(&one.element) {
                elements.push(one.element.clone());
            }
        }
        if let Some(one) = &style.paragraph {
            paragraph += 1;
            if !elements.contains(&one.element) {
                elements.push(one.element.clone());
            }
        }
        if let Some(one) = &style.table_cell {
            table_cell += 1;
            if !elements.contains(&one.element) {
                elements.push(one.element.clone());
            }
        }
    }
    json!({
        "named": named,
        "resolved": resolved,
        "unwritten": unwritten,
        "with_graphic": graphic,
        "with_paragraph": paragraph,
        "with_table_cell_properties": table_cell,
        "elements": elements,
    })
}

/// 一条地址里 scheme 那一截：`https` / `mailto` …；没有冒号、冒号前是空的
/// （`#那一页` 这种站内跳法）、或者只有**一个字母**（那是 Windows 的盘符不是 scheme）、
/// 或者太长太怪的，一律交 null —— 这里只说文件写了什么，不猜它是哪一类
fn link_scheme(raw: &str) -> Option<String> {
    let (head, _rest) = raw.split_once(':')?;
    if head.len() < 2
        || head.len() > 8
        || !head
            .bytes()
            .all(|one| one.is_ascii_alphanumeric() || matches!(one, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(head.to_ascii_lowercase())
}

/// OOXML 演示稿：一页上的链接。地址**不在字里** —— 那个 run 的 `a:rPr` 里只写一个
/// `a:hlinkClick/@r:id`，地址在这一页自己的关系表里（与图与表对象同一类两跳找法）。
/// `TargetMode="External"` 按写的交（没写就是 null，不替它当成站内）；关系指不到时
/// `target` 与 `external` 都交 null，那条 `id` 仍留着 —— 「写了个指不到东西的号」
/// 是文件自己说的话，不该被读成「这一页没有链接」
fn slide_links(bytes: &[u8], part: &str, root: &xmlscan::Node, limit: usize) -> Value {
    let mut pool: Vec<(String, String, Option<bool>)> = Vec::new();
    if let Some(member) = xml(bytes, &format!("{part}.rels")) {
        let rel_root = xmlscan::parse_str(&member.as_text());
        for one in rel_root.descendants("Relationship") {
            if !one.attr("Type").unwrap_or_default().ends_with("/hyperlink") {
                continue;
            }
            let Some(id) = one.attr("Id") else { continue };
            let mode = one.attr("TargetMode").map(|had| had == "External");
            pool.push((
                id.to_string(),
                one.attr("Target").unwrap_or_default().to_string(),
                mode,
            ));
        }
    }
    let mut list: Vec<Value> = Vec::new();
    for run in root.descendants("r") {
        let Some(had) = run.descendants("hlinkClick").into_iter().next() else {
            continue;
        };
        let id = had.attr("id").unwrap_or_default().to_string();
        let hit = pool.iter().find(|one| one.0 == id);
        let text = crate::office_text::paragraph_text(run);
        let target = hit.map(|one| one.1.clone());
        list.push(json!({
            "text": text,
            "target": target,
            "scheme": target.as_deref().and_then(link_scheme),
            "external": hit.and_then(|one| one.2),
            "hop": "rels",
            "id": id,
        }));
        if list.len() >= limit {
            break;
        }
    }
    json!({
        "total": list.len(),
        "external": list.iter().filter(|one| one["external"] == json!(true)).count(),
        "unresolved": list.iter().filter(|one| one["target"].is_null()).count(),
        "list": list,
    })
}

/// ODF 演示稿：链接直接挂在字上（`text:a/@xlink:href`），没有第二跳，也没有
/// 「站内 / 站外」那个开关 —— 所以 `external` 与 `id` 都交 null，不是 false / 空串。
/// 走的是「页上除 `presentation:notes` 以外的那几块」：备注里的链不是页面上的链，
/// 而 OOXML 那边备注住在另一个部件里、本来就不会混，这一族要自己躲。
/// 不是只走 `draw:frame`：实测 python-pptx 那个文本框被 Impress 改写成了
/// `draw:custom-shape`，只认 frame 会把页面上三条链全读成 0
fn odp_links(owners: &[&xmlscan::Node], limit: usize) -> Value {
    let mut list: Vec<Value> = Vec::new();
    for owner in owners {
        for one in owner.descendants("a") {
            let Some(raw) = crate::odsheet::attr_of(one, "href") else {
                continue;
            };
            let target = raw.to_string();
            list.push(json!({
                "text": xmlscan::inline_text(one),
                "target": target,
                "scheme": link_scheme(&target),
                "external": Value::Null,
                "hop": "inline",
                "id": Value::Null,
            }));
            if list.len() >= limit {
                break;
            }
        }
    }
    json!({
        "total": list.len(),
        "external": 0usize,
        "unresolved": 0usize,
        "list": list,
    })
}

/// 一页 odp 上的那张表：`draw:frame` 那一份（名字、位置、大小都在这儿 —— 实测
/// `table:table` 自己**一个字都不写**，所以「这张表叫什么」只能从容器上拿）
/// + ODF 表那一份（行、列、格子、跨度与被盖住的那些）。
/// 行高列宽那本账与 .ods 是同一条（`ods_layout`），合并的第三种写法在这里：
/// 被合掉的那一格照样写成 `covered-table-cell`，起点那格写 `number-*-spanned`
fn odp_page_table(
    frame: &xmlscan::Node,
    tbl: &xmlscan::Node,
    sheet: &crate::odsheet::Sheet,
    styles: &[OdpCellStyle],
    at: usize,
    limit: usize,
) -> Value {
    json!({
        "at": at,
        "frame": written_attrs(frame),
        "written": written_attrs(tbl),
        "name": sheet.name,
        "state": if sheet.visible { "visible" } else { "hidden" },
        "rows": sheet.rows,
        "columns": sheet.columns,
        "cells": sheet.cells.len(),
        "covered": sheet.covered,
        "merged": sheet.merged,
        "cell_list": sheet
            .cells
            .iter()
            .take(limit)
            .map(|had| json!({
                "ref": had.reference,
                "text": had.text,
                "kind": had.value_type,
                "span_cols": had.columns_spanned,
                "span_rows": had.rows_spanned,
                "style": had.style_name,
                "style_props": odp_style_props(styles, had.style_name.as_deref()),
            }))
            .collect::<Vec<Value>>(),
        "cell_styles": odp_cell_tally(sheet, styles),
        "layout": crate::office_sheet::ods_layout(sheet, limit),
    })
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

    /// 演示稿上的图：一页两张（柱形与饼图），第二页一张也没有；
    /// 同一批格子在 LibreOffice 重写之后引用串不再是引用
    #[test]
    fn charts_on_a_slide_come_from_the_page_relationships() {
        let deck = run("deck-chart.pptx");
        let slides = deck["slides"].as_array().expect("是数组");
        assert_eq!(slides.len(), 2, "{:?}", deck["order"]);
        assert_eq!(slides[0]["charts"], 2, "一页两张图挂在同一页上");
        assert_eq!(slides[1]["charts"], 0, "没挂图的那页报 0");
        let bar = &slides[0]["chart_list"][0];
        assert_eq!(bar["part"], "ppt/charts/chart1.xml", "{bar}");
        assert_eq!(bar["present"], json!(true));
        assert_eq!(bar["cached"], json!(true), "python-pptx 把值缓存了");
        assert_eq!(bar["title"]["via"], Value::Null, "这份没写标题");
        let group = &bar["groups"][0];
        assert_eq!(group["kind"], "barChart", "{group}");
        assert_eq!(group["written"]["barDir"], "col");
        assert_eq!(group["written"]["grouping"], "clustered");
        assert_eq!(group["series"], 2);
        let ser = &group["series_list"][0];
        assert_eq!(
            ser["name"]["ref"], "Sheet1!$B$1",
            "引用指的是内嵌那张工作簿的表名：{ser}"
        );
        assert_eq!(ser["name"]["cache"]["values"], json!(["收入"]));
        assert_eq!(ser["cat"]["cache"]["values"], json!(["一月", "二月"]));
        assert_eq!(ser["val"]["cache"]["values"], json!([10.0, 25.0]));
        assert_eq!(ser["val"]["cache"]["whole"], json!(true));
        let pie = &slides[0]["chart_list"][1];
        assert_eq!(pie["groups"][0]["kind"], "pieChart", "{pie}");
        assert_eq!(pie["groups"][0]["axis_ids"], json!([]), "饼图不连轴");
        assert_eq!(pie["groups"][0]["written"]["varyColors"], "1");
        assert_eq!(
            pie["groups"][0]["series_list"][0]["val"]["cache"]["values"],
            json!([124000.0, 18000.0])
        );

        let lo = run("deck-chart-lo.pptx");
        let mine = &lo["slides"][0]["chart_list"][0]["groups"][0]["series_list"][0];
        assert_eq!(
            mine["name"]["ref"], "label 0",
            "重写之后 c:f 里写的已经不是引用：{mine}"
        );
        assert_eq!(mine["cat"]["ref"], "categories", "{mine}");
        assert_eq!(mine["val"]["ref"], "0", "{mine}");
        assert_eq!(
            mine["val"]["cache"]["values"],
            json!([10.0, 25.0]),
            "引用串丢了，缓存的数还在"
        );
        assert_eq!(
            lo["slides"][0]["chart_list"][1]["title"]["text"], "占比",
            "饼图的标题是 LO 那一份才写的"
        );
        // 两家同一页的图数与部件名一致；不认识的 style 与 colors 部件不算图
        assert_eq!(
            deck["slides"][0]["chart_list"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|one| one["part"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>(),
            lo["slides"][0]["chart_list"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|one| one["part"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>()
        );

        // 段落级的字与 run 的条数：同一段在一家是一个 run、在另一家是三个
        let plain = run("deck.pptx");
        let again = run("deck-lo.pptx");
        assert_eq!(plain["slides"][0]["paragraph_total"], 3, "{plain}");
        assert_eq!(
            again["slides"][0]["paragraph_total"], 3,
            "段落数不受 run 拆分影响"
        );
        assert_eq!(plain["slides"][0]["text_runs"], 3, "{plain}");
        assert_eq!(
            again["slides"][0]["text_runs"], 5,
            "同一页的字被切成五个 run：{again}"
        );
        assert_eq!(
            plain["slides"][0]["paragraphs"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|one| one["text"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>(),
            again["slides"][0]["paragraphs"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|one| one["text"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>()
        );
        // 那张纸的尺寸：两个数一致，而 type 这个属性只有一家有
        assert_eq!(
            plain["size"]["cx"], again["size"]["cx"],
            "{:?}",
            plain["size"]
        );
        assert_eq!(plain["size"]["cy"], again["size"]["cy"]);
        assert_eq!(plain["size"]["type"], "screen4x3");
        assert_eq!(
            again["size"]["type"],
            Value::Null,
            "LibreOffice 不写这个属性，就别替它编一个 custom"
        );
    }

    /// ODF 的图是嵌入对象：一页两张（饼图那一族的类名是 chart:circle），
    /// 而 frame 的编号把备注框也一起数进去了
    #[test]
    fn odp_pages_carry_their_charts_through_the_embedded_objects() {
        let out = run("deck-chart.odp");
        let slides = out["slides"].as_array().expect("是数组");
        assert_eq!(slides.len(), 2, "{:?}", out["order"]);
        assert_eq!(
            slides[0]["charts"], 2,
            "备注那个 frame 没有 draw:object，不算图"
        );
        assert_eq!(slides[1]["charts"], 0, "{slides:?}");
        let bar = &slides[0]["chart_list"][0];
        assert_eq!(bar["object"], "Object 1", "{bar}");
        assert_eq!(bar["present"], json!(true));
        assert_eq!(bar["frame"], "Chart 2", "frame 的号连备注框一起数：{bar}");
        assert_eq!(bar["preview"], json!(true), "LO 另写了一份预览图");
        assert_eq!(
            bar["class"],
            Value::Null,
            "ODF 不写外层类型，类型在每条系列上"
        );
        assert_eq!(bar["series"], 2);
        let first = &bar["series_list"][0];
        assert_eq!(first["class"], "chart:bar", "{first}");
        assert_eq!(
            first["values"], "local-table.$B$2:.$B$3",
            "转一圈回来，引用指的是图自己那张 local-table：{first}"
        );
        assert_eq!(first["label"], "local-table.$B$1");
        assert_eq!(first["point_elements"], 1);
        assert_eq!(first["points_written"], 2, "repeated 说这一条顶两个点");
        let rows = bar["local_table"].as_array().expect("是数组");
        assert_eq!(rows.len(), 3, "{bar}");
        assert_eq!(rows[1]["cells"][1]["value"], "10", "{rows:?}");
        assert_eq!(rows[2]["cells"][2]["value"], "9");
        let pie = &slides[0]["chart_list"][1];
        assert_eq!(pie["title"], "占比", "饼图的标题只有 LO 那一份写了：{pie}");
        assert_eq!(pie["series_list"][0]["class"], "chart:circle");
        assert_eq!(
            pie["series_list"][0]["point_elements"], 2,
            "这一条不用 repeated"
        );
        assert_eq!(
            pie["series_list"][0]["points_written"], 2,
            "两种写法同一个点数"
        );
        // 原来那两份件一张图也没有：报 0，不是缺键
        for name in ["deck.odp", "deck.pptx", "deck-lo.pptx"] {
            for one in run(name)["slides"].as_array().expect("是数组") {
                assert_eq!(one["charts"], 0, "{name} 这一页没有图：{one}");
            }
        }
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
            "第一页：{}",
            slides[0]
        );
        assert_eq!(
            slides[1]["texts"],
            json!(["第二页：数字", "科目", "金额", "服务器", "124000"]),
            "第二页：{}",
            slides[1]
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
        assert_eq!(grouped, 9, "按页归好的原子：{slides:?}");
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

    /// 页上那张表：一张表的两种写法（`deck-tables.pptx` 与 LibreOffice 重写的那份）。
    /// 期望值来自 `office_reader.py` 的 `slide_tables_of`（同一批字的两副读者）
    #[test]
    fn one_slide_table_is_written_two_ways_and_measured_three_ways() {
        let mine = run("deck-tables.pptx");
        let theirs = run("deck-tables-lo.pptx");
        let one = &mine["slides"][0]["table_list"][0];
        let two = &theirs["slides"][0]["table_list"][0];
        // 表自己说了什么：一家写两个开关外加一条样式 id，另一家元素在场而一个字没说
        assert_eq!(
            one["written"],
            json!({"firstRow": "1", "bandRow": "1"}),
            "{one}"
        );
        assert_eq!(one["style_id"], "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}");
        assert_eq!(two["pr_present"], true, "空元素也算在场");
        assert_eq!(two["written"], json!({}));
        assert_eq!(two["style_present"], false, "这一家干脆不写那条样式 id");
        // 列宽两家一字不差（EMU 与换算出来的 0.01mm 是同一批数）
        assert_eq!(
            (one["column_elements"].as_u64(), one["grid_sum"].as_u64()),
            (Some(3), Some(6400800))
        );
        assert_eq!(one["grid_sum_mm"], 17780);
        assert_eq!(two["grid_sum_mm"], 17780);
        assert_eq!(one["grid"][0]["mm"], 7620);
        assert_eq!(one["grid"][1]["mm"], 5080);
        // 行高：重写换了写法，可换算的数没换
        assert_eq!(one["rows"][0]["h"], 609600);
        assert_eq!(two["rows"][0]["h"], 609480);
        assert_eq!(one["rows"][0]["mm"], 1693);
        assert_eq!(two["rows"][0]["mm"], 1693);
        assert_eq!(one["rows"][1]["h"], 914400);
        assert_eq!(one["rows"][1]["mm"], 2540);
        // 三本账：几个格、跨度之和、网格几列
        assert_eq!(one["cell_elements"], 9);
        assert_eq!(one["span_sum"], 10, "三列的表跨度之和能比列数多：{one}");
        assert_eq!(one["rows"][0]["cells"], 3);
        assert_eq!(one["rows"][0]["span_sum"], 4);
        assert_eq!(one["merged_from"], 2);
        assert_eq!(one["spanning"], 2);
        assert_eq!(two["merged_from"], 2, "合并的两家写法一样");
        assert_eq!(two["spanning"], 2);
        // 被合掉的那一格照样在，字是空的、一个 run 也没有
        assert_eq!(one["rows"][0]["list"][0]["text"], "科目\n金额");
        assert_eq!(
            one["rows"][0]["list"][0]["written"],
            json!({"gridSpan": "2"})
        );
        assert_eq!(one["rows"][0]["list"][1]["merge_from"], true);
        assert_eq!(one["rows"][0]["list"][1]["text"], "");
        assert_eq!(one["rows"][0]["list"][1]["runs"], 0);
        assert_eq!(
            one["rows"][1]["list"][2]["written"],
            json!({"rowSpan": "2"})
        );
        assert_eq!(one["rows"][1]["list"][2]["span_rows"], 2);
        assert_eq!(one["with_text"], 7);
        assert_eq!(two["with_text"], 7);
        // 同一格的两段：段数与 run 数各一个键
        assert_eq!(one["rows"][2]["list"][0]["text"], "网络\n设备");
        assert_eq!(one["rows"][2]["list"][0]["paragraphs"], 2);
        // tcPr 里有什么是各家的事；边距还有第二份住在 a:bodyPr 上
        assert_eq!(one["rows"][0]["list"][0]["tcpr_present"], true);
        assert_eq!(one["rows"][0]["list"][0]["tcpr"], json!({}));
        assert_eq!(one["rows"][0]["list"][0]["tcpr_paths"], json!([]));
        assert_eq!(
            two["rows"][0]["list"][0]["tcpr_paths"],
            json!(["lnL", "lnR", "lnT", "lnB", "solidFill"])
        );
        assert_eq!(one["rows"][1]["list"][0]["body"], json!({}));
        assert_eq!(two["rows"][1]["list"][0]["body"]["rIns"], "45720");
        assert_eq!(two["rows"][1]["list"][0]["tcpr"]["anchor"], "b");
    }

    /// 没有表的页交空表而不是缺键 —— pptx 与 odp 两家都是这条口径
    #[test]
    fn a_page_without_tables_gets_an_empty_ledger_not_a_missing_one() {
        let deck = run("deck.pptx");
        let first = &deck["slides"][0]["table_list"];
        assert_eq!(
            first.as_array().expect("是数组").len(),
            0,
            "第一页一张表也没有"
        );
        assert_eq!(deck["slides"][1]["tables"], 1);
        assert_eq!(
            deck["slides"][1]["table_list"]
                .as_array()
                .expect("是数组")
                .len(),
            1
        );
        let odp = run("deck.odp");
        assert_eq!(odp["slides"][0]["tables"], 0, "第一页没有表");
        assert_eq!(
            odp["slides"][1]["table_list"]
                .as_array()
                .expect("是数组")
                .len(),
            1,
            "第二页那张表也交账：{}",
            odp["slides"][1]
        );
    }

    /// odp 是第三种写法：**表名与位置只在 `draw:frame` 上**（`table:table` 自己实测一个字都不写），
    /// 而且 Impress 不把列补齐 —— 同一份账在 .ods 那边每张表都是 16384 列
    #[test]
    fn an_odp_page_table_is_named_by_the_frame_that_holds_it() {
        let deck = run("deck-tables.odp");
        let one = &deck["slides"][0]["table_list"][0];
        assert_eq!(one["frame"]["name"], "Table 2", "表名只在容器上：{one}");
        assert_eq!(one["frame"]["width"], "17.779cm");
        assert_eq!(one["written"], json!({}), "表自己一个字都不写");
        assert_eq!(one["name"], "", "而表名那个位置是空的");
        assert_eq!(
            (one["rows"].as_u64(), one["columns"].as_u64()),
            (Some(3), Some(3))
        );
        assert_eq!(
            (
                one["cells"].as_u64(),
                one["covered"].as_u64(),
                one["merged"].as_u64()
            ),
            (Some(7), Some(2), Some(2)),
            "被盖住的两格另数一本：{one}"
        );
        let cols = &one["layout"]["columns"];
        assert_eq!(cols["elements"], 3);
        assert_eq!(
            cols["spans"], 3,
            "Impress 不把列补齐（.ods 那边每张表都是 16384）"
        );
        assert_eq!(
            cols["optimal"], 3,
            "而 use-optimal-column-width 在这里是写了的（false），.ods 的列上一个都没写"
        );
        assert_eq!(cols["list"][0]["size"], "7.62cm");
        assert_eq!(cols["list"][0]["size_mm"], 7620);
        assert_eq!(cols["list"][0]["optimal"], "false");
        assert_eq!(one["layout"]["rows"]["list"][0]["size_mm"], 1693);
        assert_eq!(one["layout"]["stated"]["number-columns"], Value::Null);
        assert_eq!(one["layout"]["unit"], "0.01mm");
        // 合并写在本格的属性上，字与 .ods 一样按段拼
        assert_eq!(one["cell_list"][0]["span_cols"], 2);
        assert_eq!(one["cell_list"][0]["text"], "科目\n金额");
        assert_eq!(
            one["cell_list"][0]["kind"],
            Value::Null,
            "Impress 一个 `office:value-type` 都不写：有字也没写，所以交 null 而不是替它推一个"
        );
        assert_eq!(one["cell_list"][4]["span_rows"], 2);
        assert_eq!(one["cell_list"][2]["style"], "ce2");
        assert_eq!(one["cell_list"][6]["style"], "ce5");
        assert_eq!(
            one["cell_list"][0]["style"],
            Value::Null,
            "四格没点任何样式"
        );
        // 那一跳到样式文件里拿：底色与垂直对齐在 `loext:graphic-properties`（LibreOffice
        // 自己的实验命名空间），边在 `style:paragraph-properties`，而 odt 表格用的
        // `style:table-cell-properties` 这一族一个都没有
        let props = &one["cell_list"][2]["style_props"];
        assert_eq!(props["style"], "ce2");
        assert_eq!(props["found"], json!(true));
        assert_eq!(props["part"], "content.xml");
        assert_eq!(props["family"], "table-cell");
        assert_eq!(props["parent"], Value::Null, "这份样式没有父链");
        assert_eq!(
            props["graphic_element"], "loext:graphic-properties",
            "前缀就是这一条的全部意义：不是 style: 那一族"
        );
        assert_eq!(props["graphic"]["draw:fill"], "solid");
        assert_eq!(props["graphic"]["draw:fill-color"], "#d0d8e7");
        assert_eq!(props["graphic"]["draw:textarea-vertical-align"], "bottom");
        assert_eq!(props["graphic"]["fo:padding-left"], "0.254cm");
        assert_eq!(props["paragraph_element"], "style:paragraph-properties");
        assert_eq!(props["paragraph"]["fo:border"], "0.48pt solid #ffffff");
        assert_eq!(props["table_cell_element"], Value::Null, "这一族没有那一处");
        assert_eq!(
            one["cell_list"][6]["style_props"]["graphic"]["draw:fill-color"], "#ffff00",
            "pptx 那面 a:tcPr 里那个黄，在 odp 是 ce5 的底色"
        );
        let tally = &one["cell_styles"];
        assert_eq!(tally["named"], json!(3));
        assert_eq!(tally["resolved"], json!(3), "点名的三条都解开了");
        assert_eq!(tally["unwritten"], json!(4));
        assert_eq!(tally["with_graphic"], json!(3));
        assert_eq!(tally["with_paragraph"], json!(3));
        assert_eq!(tally["with_table_cell_properties"], json!(0));
        assert_eq!(
            tally["elements"].as_array().expect("是数组").len(),
            2,
            "两处 properties：{tally}"
        );
        assert_eq!(
            one["cell_list"][0]["style_props"]["found"],
            json!(false),
            "没点名与点了找不到是两件事"
        );
        assert_eq!(one["cell_list"][0]["style_props"]["style"], Value::Null);
        assert_eq!(one["cell_list"][0]["style_props"]["graphic"], Value::Null);
        // 没有表的页交空表而不是缺键
        let charts = run("deck-chart.odp");
        assert!(charts["slides"][0]["table_list"]
            .as_array()
            .expect("是数组")
            .is_empty());
        let plain = run("deck.odp");
        assert_eq!(plain["slides"][0]["tables"], 0);
        assert_eq!(
            plain["slides"][1]["table_list"]
                .as_array()
                .expect("是数组")
                .len(),
            1
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

    /// 页上的链接：OOXML 要走两跳（run 里只有号，地址在这一页自己的关系表里），
    /// ODF 一跳就够（地址直接写在字上）—— 期望值全部来自 `office_reader.py`
    #[test]
    fn a_link_on_a_slide_is_two_hops_in_ooxml_and_one_in_odf() {
        let deck = run("deck-links.pptx");
        let one = &deck["slides"][0]["links"];
        assert_eq!(one["total"], json!(3));
        assert_eq!(one["external"], json!(3));
        assert_eq!(one["unresolved"], json!(0));
        let list = one["list"].as_array().expect("是数组");
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["text"], "第三季度的说明", "链上的字与地址是两件事");
        assert_eq!(list[0]["target"], "https://example.com/budget");
        assert_eq!(list[0]["scheme"], "https");
        assert_eq!(list[0]["external"], json!(true));
        assert_eq!(list[0]["hop"], "rels");
        assert_eq!(list[1]["scheme"], "mailto");
        assert_eq!(list[2]["text"], list[2]["target"], "这一条的字就是地址本身");
        assert_eq!(
            deck["slides"][1]["links"]["total"],
            json!(0),
            "没链的那页交 0"
        );

        let rewritten = run("deck-links-lo.pptx");
        let back = &rewritten["slides"][0]["links"];
        assert_eq!(back["total"], json!(3));
        assert_eq!(one["list"][0]["id"], "rId2");
        assert_eq!(
            back["list"][0]["id"], "rId1",
            "号是生产者自己排的，只交不比"
        );
        assert_eq!(
            back["list"][0]["target"], one["list"][0]["target"],
            "地址一字未变"
        );

        let odp = run("deck-links.odp");
        let page = &odp["slides"][0]["links"];
        assert_eq!(
            page["total"],
            json!(3),
            "文本框被 Impress 改写成了 custom-shape，还读得到"
        );
        assert_eq!(page["list"][0]["hop"], "inline", "这一族没有第二跳");
        assert_eq!(page["list"][0]["target"], "https://example.com/budget");
        assert_eq!(page["list"][0]["text"], "第三季度的说明");
        assert_eq!(
            page["list"][0]["external"],
            Value::Null,
            "这一族没有那个开关，不替它填 false"
        );
        assert_eq!(page["list"][0]["id"], Value::Null);
        assert_eq!(
            page["external"],
            json!(0),
            "没有那个开关，所以一条也判不出站外"
        );
        assert_eq!(odp["slides"][1]["links"]["total"], json!(0));
    }
}
