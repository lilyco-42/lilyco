//! `lbin office-sheet` — 电子表格的结构：有几张表、哪张被藏起来、格子里装的是什么。
//!
//! 表格文件的常见问题跟文档不一样，它问的是**布局**：这张工作簿里有几张表（隐藏的也算，
//! 因为隐藏表往往是别人不让你看的那张）、每张自报的范围是多少、多少格子有值、
//! 其中多少是公式（公式的**缓存结果**在文件里可能存在也可能不存在，报出来时要说清读到的是哪一个）、
//! 有多少合并格、命名区域指向哪里、有没有外部工作簿与图表。
//!
//! OOXML 走 `xl/workbook.xml` + 每张表的 `xl/worksheets/sheetN.xml` + 共享字符串表；
//! 遗留 `.xls` 走 [`crate::biff`]（BIFF8 的记录表）。两条路的单元格都按引用（A1 形式）给出。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, resolve_target, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出工作簿的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-sheet",
    run = "run_office_sheet",
    about = "Report a spreadsheet's layout: every sheet with its workbook-order index, sheetId, relationship target, r:id and visibility (hidden and very-hidden sheets are listed, not skipped - they are usually the ones worth knowing about), each sheet's self-declared dimension, and per sheet the cell count, formula count, numeric/shared/inline-string split, merged ranges, hidden rows and columns. Also reports defined names (with what they point at), table parts (names, ranges, header rows), external-link workbook parts, chart and picture parts, styles/conditional formatting presence, and whether a calcChain exists. Shared strings are resolved so LABELSST cells carry their text; a formula cell reports the formula and says whether the file also cached a result (openpyxl-written files do not, and inventing a value there is exactly what this command refuses to do). Each cell also carries its number format: the style index on the cell is a row of xl/styles.xml cellXfs (not a format id), so a date is only a date once that hop is taken - the format code and, for date/time-formatted numeric cells, the ISO reading of the serial number are reported, honouring workbook.xml date1904 and reporting Excel's non-existent 1900-02-29 as written. A text cell like "12/23/2013" stays text. Legacy .xls goes through the BIFF8 record reader, and its hidden rows and columns come out of the two records the flags actually live in: bit 0x20 of the ROW record, and bit 0 of the COLINFO record (which states a range, expanded here - LibreOffice writes that record under the older id 0x007D while MS-XLS names 0x07D0 for BIFF8, so both ids are accepted). Which ROW bit means hidden was measured rather than recalled: three comparison files separate the two variables - row heights from 4pt to 250pt leave that bit alone, while hiding a single row sets exactly that bit. Hidden cells still count as cells. Spreadsheet comments are another hop: they are not in sheetN.xml at all - the sheet's own relationship part names the comments part, and the two producers measured here put it in two different places (openpyxl `xl/comments/comment1.xml` reached through an absolute target, LibreOffice `xl/comments1.xml` through `../comments1.xml`), with the author's name indexed through the `<authors>` list rather than written on the comment; ODF instead keeps the comment INSIDE the cell as `office:annotation`, which is exactly why the cell's own text skips that subtree. Authoring timestamps come back null on both producers because neither wrote one. Legacy .xls is a fourth spelling and stays inside the same stream: one record kind carries the text (offset 10 of its body is the character count it states for itself, and the first CONTINUE right after it opens with an encoding byte - 0 means one byte per character, 1 means two, which is the OPPOSITE of the BIFF8 fCompressed convention), while another record at the end of that sheet's own substream says which cell the note is on and who wrote it. The two lists are paired in order of appearance, every entry carries whole (were both self-stated counts satisfied), and both record counts are published per sheet so a mismatch shows up as data instead of a silently truncated list. This family writes no authoring timestamp, so date is null there; the reading was measured on LibreOffice-written .xls, and those two record numbers are not given spec names because MS-XLS assigns 0x001C to something else entirely. Whether a sheet can still be edited is reported per format, because the three spellings do not map onto one another: xlsx keeps two layers (workbookProtection plus each sheet's own sheetProtection, switches read in both the 1/0 and true/false spellings with an omitted one left omitted rather than false), .ods writes table:protected on the table itself together with the digest URI, and .xls has no workbook layer at all - PROTECT (0x0012), PASSWORD (0x0013) and SCENPROTECT (0x00DD) sit inside the locked sheet's own substream, so they are attributed per sheet and their raw values kept. ODF spreadsheets (.ods) are read on their own terms: cells carry an explicit value-type with office:value / date-value / boolean-value (no serial-number epoch to guess), positions are accumulated through table:number-columns-repeated runs (which routinely stand for 16000+ empty columns and are not counted), covered cells are tallied apart from content, merges come from the span attributes, a sheet's visibility is resolved through the automatic style it names, and hidden rows/columns are counted from table:visibility="collapse" on the element or in the row/column style it names (multiplying number-columns-repeated, so one element standing for three collapsed columns reports 3, not 1); each ODS cell additionally carries the number format it inherits - cell style, then style:data-style-name, then that number:*-style element (which lives in content.xml or styles.xml, and is reached through parent-style-name when the cell style itself names none) - reported as format_kind (taken from the element's own name, so a ¥ written as a literal text token stays a number-style), plus decimals, currency_symbol and a faithful format_tokens transcription; ODF has no format string, so none is invented. With --csv it also renders one sheet (by name, or by the 0-based index this command reports; --sheet picks it, default first) as RFC4180 CSV under { csv: {sheet, index, rows, columns, cells_skipped, line_end, text} } - date cells go out as the ISO reading of the serial number (legacy .xls takes the same hop too - the cell's ixfe indexes the XF records, whose format number names either a FORMAT record or a built-in id, and the epoch comes from DATEMODE; a file that never wrote DATEMODE gets the serial rather than a guessed 1900), a formula cell with no cached result goes out empty rather than guessed, holes are empty fields, and cells whose reference cannot be parsed as A1 are left out. Each xlsx sheet additionally carries its own print setup: the three elements `pageMargins`, `pageSetup` and `printOptions` are reported separately, and an element the file never wrote stays null rather than turning into false - openpyxl writes only the margins, while LibreOffice rewrites the same sheet with twelve `pageSetup` attributes (paperSize 9 and both dpi values included). Margins are handed over exactly as written, in the unit that family uses (`margin_unit`: inch, said once per sheet), rather than normalised to the 0.01mm integers office-doc reports - 0.5 versus 0.511811023622047 is the two producers' difference, and converting it away would erase the thing worth seeing. ODS and .xls print setup is deliberately not read: five LibreOffice-written .ods files name no page layout on the table element (the only link left is that producer's own `PageStyle_<sheet>` naming convention, not a spec hop), and while every .xls sheet does write a SETUP (0x00A1) record, nothing here can be a second reader for the field offsets inside it. A sheet's charts are two more hops: the sheet's own relationship part names a drawing part, and that drawing's relationships name the chart parts (openpyxl writes those targets absolute, LibreOffice relative - `resolve_target` eats both). So charts are attributed per sheet (`charts` and `chart_list`, and a sheet with none reports 0 rather than omitting the key): each entry reports its part, its title (a literal through `c:rich`, or a cell reference), whether the file cached any plotted values, and per plot group the kind (`barChart` / `lineChart`), every direct child's `val` as written (`barDir`, `grouping`, `gapWidth`...) with the axis ids kept separate because those are the producer's own numbering. The cached flag matters: openpyxl writes not one `c:pt`, so what the picture actually plots stays unknown and only the reference is handed over, while LibreOffice caches every point and self-states `ptCount` - that count and the number of points really found are both published, with whole saying whether they agree. Reference strings go out exactly as the file wrote them: the same cells read `'数据'!B1` in one producer's file and `数据!$B$1` in the other's, and the same text categories are a `c:numRef` in one and a `c:strRef` in the other - normalising either would be inventing what the file did not say. ODS keeps charts as embedded objects (`Object 1/` parts) and .xls through the BIFF object chain; neither is read yet. Two more everyday things are read per sheet, because both answer questions people actually ask. Conditional formatting: each `conditionalFormatting` block keeps its `sqref` as written (one block may name two ranges), and each `cfRule` hands over every attribute the file wrote plus type/priority/operator named separately, its formula texts (entities decoded, so a written `$B2&gt;200` reads as `$B2>200`) and the shape inside `iconSet` / `colorScale` / `dataBar` - each `cfvo`'s type and val, each color's rgb exactly as written, since openpyxl writes 00FFFFFF where LibreOffice writes FFFFFFFF for the same white. `dxfId` is a hop: the rule names an index into `dxfs` in xl/styles.xml, so each rule says what it wrote, whether that index resolves, and which element paths that dxf contains (`font/b`, `font/color`...) - LibreOffice rewrites the same dxf with five entries where openpyxl wrote two, so nothing is folded into a shared bold-and-red shape. Priorities are each producer's own numbering (the same four rules read 1/2/3/4 in one file and 2/3/4/5 in the other), so they are reported rather than compared. Data validation: the `dataValidations` container's self-stated count is published next to how many were found and whether they agree, and each entry reports its range, type, operator, the formula1/formula2 texts as written and every attribute - including the two boolean spellings (allowBlank 1 versus true), the operator LibreOffice adds to a list rule, the formula2 it fills with 0, and the = it strips from a custom formula. ODS keeps these in number and table styles, .xls in BIFF records; neither is read, so the rules key is absent for those families rather than empty. Returns { path, format, sheets, protection, csv, workbook, defined_names, tables, external_links, parts, notes }."
)]
pub struct OfficeSheet {
    /// 表格文件（xlsx / xlsm / xls / ods）
    #[arg(about = "Spreadsheet to inspect", must_exist = true)]
    path: PathBuf,

    /// 每张表最多列多少个格子（总数照实给）
    #[arg(
        about = "List at most this many cells per sheet",
        default = 200,
        min = 1
    )]
    limit: u64,

    /// 顺带交一份 CSV（RFC4180：带逗号/引号/换行的字段加引号，日期给 ISO）
    #[arg(about = "Also render one sheet as CSV")]
    csv: bool,

    /// `--csv` 要哪张表：表名，或 `index` 那个从 0 起的序号；不给就是第一张
    #[arg(about = "Sheet for --csv: name or 0-based index", default = "")]
    sheet: String,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 200;

fn run_office_sheet(app: &OfficeSheet, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the workbook layout".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    if doc.family == Family::Ooxml && doc.app == "excel" {
        let workbook = xml(bytes, "xl/workbook.xml")
            .ok_or_else(|| AppError::InvalidInput("包里读不到 xl/workbook.xml".to_string()))?;
        let root = xmlscan::parse_str(&workbook.as_text());
        // 工作簿的顺序是真相：r:id → 部件路径，靠 workbook.xml.rels 对上
        let mut by_id: Vec<(String, String)> = Vec::new();
        if let Some(rels) = xml(bytes, "xl/_rels/workbook.xml.rels") {
            let rel_root = xmlscan::parse_str(&rels.as_text());
            for one in rel_root.descendants("Relationship") {
                let id = one.attr("Id").unwrap_or_default().to_string();
                let target = one.attr("Target").unwrap_or_default();
                by_id.push((id, resolve_target("xl", target)));
            }
        }
        let shared = shared_strings(bytes);
        // 格子写的 `s="3"` 是 `cellXfs` 的**下标**，不是格式号：不绕这一层，
        // 一个日期永远只是「一个数」（41631）。
        let styles = crate::numfmt::read_styles(bytes);
        let mut notes = doc.notes.clone();
        notes.extend(styles.notes.iter().cloned());
        let mut sheets: Vec<Value> = Vec::new();
        let mut grid_names: Vec<String> = Vec::new();
        let mut grids: Vec<Vec<(usize, usize, String)>> = Vec::new();
        let mut grid_skipped = 0usize;
        // 条件格式的规则只写一个 dxfId，真样式在 styles.xml 的 dxfs 那一跳上
        let (dxf_written, dxfs) = dxf_table(bytes);
        let dxf_whole = match &dxf_written {
            None => true,
            Some(raw) => raw.trim().parse::<usize>().ok() == Some(dxfs.len()),
        };
        let mut totals = json!({
            "cells": 0, "formulas": 0, "numeric": 0, "shared_strings": 0,
            "inline_strings": 0, "merged": 0, "hidden_rows": 0, "hidden_cols": 0,
            "dates": 0, "comments": 0, "charts": 0, "conditional_rules": 0, "validations": 0,
        });
        for (index, one) in root.descendants("sheet").iter().enumerate() {
            let name = one.attr("name").unwrap_or_default().to_string();
            let rid = one.attr_local("id").unwrap_or_default().to_string();
            let part = by_id
                .iter()
                .find(|(one_id, _)| *one_id == rid)
                .map(|(_, path)| path.clone())
                .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", index + 1));
            let state = match one.attr("state").unwrap_or("visible") {
                "hidden" => "hidden",
                "veryHidden" => "very-hidden",
                _ => "visible",
            };
            // 批注那两跳：表 → 它自己的关系表 → 批注部件
            let comments = sheet_comments(bytes, &part, limit);
            let counted = comments.len();
            let mut entry = json!({
                "index": index,
                "name": name,
                "sheet_id": one.attr("sheetId"),
                "state": state,
                "r_id": rid,
                "part": part,
                "comments": counted,
                "comment_list": comments,
            });
            bump(&mut totals, "comments", counted);
            match xml(bytes, &part) {
                Some(member) => {
                    let sheet_root = xmlscan::parse_str(&member.as_text());
                    let dimension = sheet_root
                        .descendants("dimension")
                        .first()
                        .and_then(|one| one.attr("ref"))
                        .unwrap_or_default()
                        .to_string();
                    // 打印那份设置是这张表自己的（三个元素各自在不在）
                    entry["print_setup"] = xlsx_print_setup(&sheet_root);
                    // 图要再跳两跳才到：表 → 自己的关系表 → 画法部件 → 它的关系表 → 图
                    let charts = sheet_charts(bytes, &part, limit);
                    let charted = charts.len();
                    let uncached = charts
                        .iter()
                        .filter(|one| one["cached"].as_bool() != Some(true))
                        .count();
                    let rules = sheet_rules(&sheet_root, &dxfs, limit);
                    let conditioned = rules["conditional"]
                        .as_array()
                        .map(|one| {
                            one.iter()
                                .map(|had| had["rules"].as_u64().unwrap_or(0) as usize)
                                .sum::<usize>()
                        })
                        .unwrap_or_default();
                    let validated = rules["validations"]["list"]
                        .as_array()
                        .map(|one| one.len())
                        .unwrap_or_default();
                    bump(&mut totals, "conditional_rules", conditioned);
                    bump(&mut totals, "validations", validated);
                    entry["rules"] = rules;
                    entry["charts"] = json!(charted);
                    entry["chart_list"] = Value::Array(charts);
                    bump(&mut totals, "charts", charted);
                    if uncached > 0 {
                        notes.push(format!(
                            "这张表上 {} 张图的数值没有缓存（图里画的是哪些数判不住，只能交引用）",
                            uncached
                        ));
                    }
                    let mut cells: Vec<Value> = Vec::new();
                    let mut count = 0usize;
                    let mut formulas = 0usize;
                    let mut numeric = 0usize;
                    let mut shared_count = 0usize;
                    let mut inline = 0usize;
                    let mut cached = 0usize;
                    let mut dates = 0usize;
                    let mut grid: Vec<(usize, usize, String)> = Vec::new();
                    for cell in sheet_root.descendants("c") {
                        count += 1;
                        let reference = cell.attr("r").unwrap_or_default().to_string();
                        let kind = cell.attr("t").unwrap_or("n").to_string();
                        let style_index = cell
                            .attr("s")
                            .and_then(|raw| raw.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        let value = cell.child("v").map(|one| one.text().trim().to_string());
                        let formatted =
                            styles.cell_format(style_index, value.as_deref(), kind.as_str());
                        if matches!(
                            formatted["format_kind"].as_str().unwrap_or(""),
                            "date" | "datetime" | "time"
                        ) {
                            dates += 1;
                        }
                        let formula = cell.child("f").map(|one| one.text().trim().to_string());
                        if let Some(raw) = &formula {
                            formulas += 1;
                            let _ = raw;
                        }
                        if cell.child("is").is_some() {
                            inline += 1;
                        }
                        if kind == "s" {
                            shared_count += 1;
                        } else if let Some(raw) = &value {
                            if raw.parse::<f64>().is_ok() {
                                numeric += 1;
                            }
                        }
                        let text = match kind.as_str() {
                            "s" => value
                                .as_ref()
                                .and_then(|raw| raw.parse::<usize>().ok())
                                .map(|which| {
                                    shared
                                        .get(which)
                                        .cloned()
                                        .unwrap_or_else(|| format!("#SST 索引 {which} 越界"))
                                }),
                            "inlineStr" => {
                                cell.child("is").map(|one| one.text().trim().to_string())
                            }
                            "str" => value.clone(),
                            "e" => value.clone().map(|one| format!("#错误 {one}")),
                            "b" => value.clone().map(|one| {
                                if one == "1" {
                                    "TRUE".to_string()
                                } else {
                                    "FALSE".to_string()
                                }
                            }),
                            _ => match value.as_ref().and_then(|raw| raw.parse::<f64>().ok()) {
                                Some(one) => Some(format!("{one}")),
                                None => value.clone(),
                            },
                        };
                        // `<v></v>` 是「有标签、没值」：openpyxl 就是这么写的，
                        // 把空串当成缓存结果等于替文件编一个数
                        if formula.is_some()
                            && value.as_ref().is_some_and(|raw| !raw.trim().is_empty())
                        {
                            cached += 1;
                        }
                        if grid.len() < MAX_GRID_CELLS {
                            if let Some((row, col)) = split_ref(&reference) {
                                let display = match formatted["as_date"].as_str() {
                                    Some(one) => one.to_string(),
                                    None => text.clone().unwrap_or_default(),
                                };
                                grid.push((row, col, display));
                            }
                        } else {
                            grid_skipped += 1;
                        }
                        if cells.len() < limit {
                            cells.push(merge(
                                json!({
                                    "ref": reference,
                                    "kind": kind,
                                    "value": numeric_or_text(text),
                                    "formula": formula,
                                }),
                                formatted,
                            ));
                        }
                    }
                    entry["dimension"] = json!(dimension);
                    entry["cells"] = json!(count);
                    let hidden_rows = sheet_root
                        .descendants("row")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .count();
                    let hidden_cols = sheet_root
                        .descendants("col")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .map(|one| col_span(one))
                        .sum::<usize>();
                    entry["hidden_rows"] = json!(hidden_rows);
                    entry["hidden_cols"] = json!(hidden_cols);
                    entry["listed"] = json!(cells.len());
                    entry["formulas"] = json!(formulas);
                    entry["numeric"] = json!(numeric);
                    entry["shared_string_cells"] = json!(shared_count);
                    entry["inline_strings"] = json!(inline);
                    entry["date_cells"] = json!(dates);
                    entry["formula_cells_with_cached_value"] = json!(cached);
                    entry["merged"] = json!(sheet_root.descendants("mergeCell").len());
                    entry["rows"] = json!(sheet_root.descendants("row").len());
                    entry["cell_list"] = json!(cells);
                    grid_names.push(name.clone());
                    grids.push(grid);
                    bump(&mut totals, "cells", count);
                    bump(&mut totals, "formulas", formulas);
                    bump(&mut totals, "numeric", numeric);
                    bump(&mut totals, "shared_strings", shared_count);
                    bump(&mut totals, "inline_strings", inline);
                    bump(&mut totals, "dates", dates);
                    bump(&mut totals, "hidden_rows", hidden_rows);
                    bump(&mut totals, "hidden_cols", hidden_cols);
                    bump(
                        &mut totals,
                        "merged",
                        sheet_root.descendants("mergeCell").len(),
                    );
                }
                None => {
                    entry["error"] = json!("表的工作流部件读不出来");
                }
            }
            sheets.push(entry);
        }
        let defined: Vec<Value> = root
            .descendants("definedName")
            .iter()
            .take(limit)
            .map(|one| json!({"name": one.attr("name").unwrap_or_default(), "text": one.text()}))
            .collect();
        let external: Vec<String> = doc
            .entries
            .iter()
            .map(|one| one.name.clone())
            .filter(|one| one.starts_with("xl/externalLinks/externalLink"))
            .collect();
        // 保护这份账：工作簿一层、每张表一层，两层的开关还各说各的话
        // （openpyxl 写 `sheet="1"`，LibreOffice 重写同一份东西写 `sheet="true"`）
        let mut locks: Vec<Value> = Vec::new();
        for one in &sheets {
            let name = one["name"].as_str().unwrap_or_default().to_string();
            let part = one["part"].as_str().unwrap_or_default().to_string();
            match xml(bytes, &part) {
                Some(member) => {
                    let sheet_root = xmlscan::parse_str(&member.as_text());
                    locks.push(crate::protect::xlsx_sheet(&sheet_root, &name));
                }
                None => locks.push(json!({"name": name, "element": false, "protected": false})),
            }
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "spreadsheetml",
            "workbook": {
                "sheets": sheets.len(),
                "hidden_sheets": sheets.iter().filter(|one| one["state"] != "visible").count(),
                "date1904": json!(styles.year1904),
                "shared_strings": shared.len(),
                "views": root.descendants("workbookView").len(),
                "calculation_mode": root.descendants("calcPr").first().and_then(|one| one.attr("fullCalcOnLoad")).map(|one| one.to_string()),
                "has_calc_chain": xml(bytes, "xl/calcChain.xml").is_some(),
                "dxfs": {"written": dxf_written, "found": dxfs.len(), "whole": dxf_whole},
                "styles_part": xml(bytes, "xl/styles.xml").is_some(),
                "tables": doc.entries.iter().filter(|one| one.name.starts_with("xl/tables/")).count(),
                "charts": doc.entries.iter().filter(|one| one.name.starts_with("xl/charts/")).count(),
                "media": doc.entries.iter().filter(|one| one.name.starts_with("xl/media/")).count(),
                "totals": totals,
            },
            "sheets": sheets,
            "protection": json!({
                "workbook": crate::protect::xlsx_workbook(&root),
                "sheets": locks,
            }),
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, grid_skipped),
            "defined_names": defined,
            "external_links": external,
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.family == Family::Odf && doc.app == "excel" {
        let book = crate::odsheet::read(bytes);
        let mut notes = book.notes.clone();
        // 格式在另一跳上：格子 → 单元格样式 → `style:data-style-name` → `number:*-style`，
        // 而那棵元素树可能坐在 content.xml，也可能坐在 styles.xml
        let styles = crate::odstyle::Styles::read(bytes);
        notes.extend(styles.notes.iter().cloned());
        notes.push(
            "ODF 的数字格式是一棵元素树，不是 Excel 那种格式串：这里逐条抄成 \
             `format_tokens`（`year`、`text:-`…），不替它重构 `yyyy-mm-dd`；\
             类别只看样式元素自己的名字 —— 屏上带 ¥ 字面量的那份其实是个 number-style"
                .to_string(),
        );
        notes.push(
            "ODF 的格子里不写序列数：日期就是 `office:date-value` 那个 ISO 串，\
             所以这边没有 1900 / 1904 那套基准要猜；显示文本（如 `12.5%`）与值是两样东西"
                .to_string(),
        );
        let grid_names: Vec<String> = book.sheets.iter().map(|one| one.name.clone()).collect();
        let grids: Vec<Vec<(usize, usize, String)>> = book
            .sheets
            .iter()
            .map(|one| {
                one.cells
                    .iter()
                    .filter_map(|had| {
                        split_ref(&had.reference).map(|(row, col)| (row, col, ods_display(had)))
                    })
                    .collect()
            })
            .collect();
        let mut sheets: Vec<Value> = Vec::new();
        let mut totals = json!({
            "cells": 0, "formulas": 0, "dates": 0, "merged": 0, "covered": 0,
            "hidden_rows": 0, "hidden_cols": 0, "comments": 0,
        });
        for (index, one) in book.sheets.iter().enumerate() {
            let mut types = serde_json::Map::new();
            for had in &one.cells {
                let key = had.value_type.clone();
                let next = types.get(&key).and_then(|one| one.as_u64()).unwrap_or(0) + 1;
                types.insert(key, json!(next));
            }
            sheets.push(json!({
                "index": index,
                "name": one.name,
                "state": if one.visible { "visible" } else { "hidden" },
                "rows": one.rows,
                "columns": one.columns,
                "cells": one.cells.len(),
                "formulas": one.formulas(),
                "date_cells": one.date_cells(),
                "merged": one.merged,
                "covered": one.covered,
                "hidden_rows": one.hidden_rows,
                "hidden_cols": one.hidden_cols,
                "comments": one.comments.len(),
                "comment_list": one.comments.iter().cloned().take(limit).collect::<Vec<Value>>(),
                "value_types": Value::Object(types),
                "cell_list": one.cells.iter().take(limit).map(|had| merge(had.to_json(), styles.for_cell(had.style_name.as_deref()))).collect::<Vec<Value>>(),
            }));
            bump(&mut totals, "cells", one.cells.len());
            bump(&mut totals, "formulas", one.formulas());
            bump(&mut totals, "dates", one.date_cells());
            bump(&mut totals, "merged", one.merged);
            bump(&mut totals, "covered", one.covered);
            bump(&mut totals, "hidden_rows", one.hidden_rows);
            bump(&mut totals, "hidden_cols", one.hidden_cols);
            bump(&mut totals, "comments", one.comments.len());
        }
        let cut = book
            .sheets
            .iter()
            .any(|one| one.cells.len() > limit)
            .then(|| "格子只列前 --limit 个")
            .unwrap_or_default();
        if !cut.is_empty() {
            notes.push(cut.to_string());
        }
        // ODF 的表保护写在 `table:table` 自己身上的属性里（不像 OOXML 那样另有一层）
        let mut locks: Vec<Value> = Vec::new();
        if let Some(member) = xml(bytes, "content.xml") {
            let content_root = xmlscan::parse_str(&member.as_text());
            for one in content_root.descendants("table") {
                locks.push(crate::protect::ods_table(
                    one,
                    crate::odsheet::attr_of(one, "name").unwrap_or_default(),
                ));
            }
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-spreadsheet",
            "workbook": {
                "sheets": book.sheets.len(),
                "hidden_sheets": book.sheets.iter().filter(|one| !one.visible).count(),
                "cell_total": book.cell_total(),
                "styles": {
                    "cell_styles": styles.counts().0,
                    "data_styles": styles.counts().1,
                },
                "totals": totals,
            },
            "sheets": sheets,
            "protection": json!({
                "sheets": locks,
                "workbook": json!({"element": false}),
            }),
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, 0),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.family == Family::Compound && doc.app == "excel" {
        let cfb = doc
            .compound
            .as_ref()
            .ok_or_else(|| AppError::InvalidInput("复合文档打不开".to_string()))?;
        let book = crate::biff::read(cfb, bytes).map_err(AppError::InvalidInput)?;
        let mut notes = book.notes.clone();
        notes.push(
            "批注在这一族是第四种存法：不在另一个部件里，也不在格子里面，而在**同一条流**\
             的两类记录上 —— 一条给字（正文偏移 10 自报字数，紧跟的第一条 CONTINUE 首字节\
             说编码：0 是一格一字节、1 是一格两字节，这一位与 BIFF8 那个 fCompressed 的惯例\
             相反），一条给「哪个格子 + 谁写的」（住在这张表子流的末尾）。两类按出现顺序配，\
             所以每条都带 whole 说两边自报的字数是不是都正好切出来。这一族不写作者时间，\
             date 一律 null。这套读法是在 LibreOffice 写的 .xls 上量的，记录名也不替它们编"
                .to_string(),
        );
        let grid_names: Vec<String> = book.sheets.iter().map(|one| one.name.clone()).collect();
        let grids: Vec<Vec<(usize, usize, String)>> = book
            .sheets
            .iter()
            .map(|sheet| {
                book.cells
                    .iter()
                    .filter(|had| had.sheet.as_deref() == Some(sheet.name.as_str()))
                    .filter_map(|had| {
                        split_ref(&had.reference()).map(|(row, col)| (row, col, book.shown(had)))
                    })
                    .collect()
            })
            .collect();
        let cells: Vec<Value> = book
            .cells
            .iter()
            .take(limit)
            .map(|one| {
                merge(
                    json!({
                        "ref": one.reference(),
                        "kind": one.kind,
                        "sheet": one.sheet,
                        "text": one.text,
                        "number": one.number,
                    }),
                    book.cell_format(one).unwrap_or(Value::Null),
                )
            })
            .collect();
        // 这一族的锁写在**被锁那张表自己的子流**里，不在工作簿那层：与 xlsx 的两层、
        // ODF 的属性各是一种写法，所以这里按表交账
        let locks: Vec<Value> = book
            .sheets
            .iter()
            .map(|one| crate::protect::xls_sheet(&one.name, &one.protection))
            .collect();
        if book.sheets.iter().any(|one| !one.protection.is_empty()) {
            notes.push(
                "这一族没有「整本工作簿一层」的表锁：那几条记录住在表自己的子流里，\
                 锁哪张记在哪张名下（与 xlsx 的 workbookProtection + sheetProtection 两层不同）"
                    .to_string(),
            );
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "biff8",
            "workbook": {
                "sheets": book.sheets.len(),
                "hidden_sheets": book.sheets.iter().filter(|one| one.state != "visible").count(),
                "shared_strings": book.strings.len(),
                "bofs": book.bofs.len(),
                "records": book.records,
                // 数字格式那一跳的两张表：XF 表只列格式号，FORMAT 表只有自定义号有串
                "date1904": book.date1904,
                "xfs": book.xfs,
                "formats": book.formats,
                "totals": {
                    "cells": book.cells.len(),
                    "formulas": book.formula_cells,
                    "hidden_rows": book.sheets.iter().map(|one| one.hidden_rows.len()).sum::<usize>(),
                    "hidden_cols": book.sheets.iter().map(|one| one.hidden_cols.len()).sum::<usize>(),
                    "comments": book.sheets.iter().map(|one| one.comments.len()).sum::<usize>(),
                },
            },
            "protection": json!({"kind": "biff8", "sheets": locks}),
            "sheets": book.sheets.iter().map(|one| json!({
                "name": one.name,
                "state": one.state,
                "record_start": one.record_start,
                // 隐藏的是「看不见」，那些格子照样算在 cells 里；只报条数，
                // 与 xlsx / ods 那两支同一个形状（位置在 biff 的单测里逐条断言）
                "hidden_rows": one.hidden_rows.len(),
                "hidden_cols": one.hidden_cols.len(),
                "comments": one.comments.len(),
                "comment_list": one.comments.iter().take(limit).map(|had| json!({
                    "ref": had.reference,
                    "author": had.author,
                    "date": Value::Null,
                    "text": had.text,
                    "whole": had.whole,
                })).collect::<Vec<Value>>(),
                // 两类记录各自有几条：配的条数只到小的那个，所以对不上时这里看得见
                "note_text_records": one.note_text_records,
                "note_cell_records": one.note_cell_records,
                "cells": book.cells.iter().filter(|had| had.sheet.as_deref() == Some(one.name.as_str())).count(),
            })).collect::<Vec<Value>>(),
            "cells": cells,
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, 0),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    Err(AppError::InvalidInput(format!(
        "{} 不是电子表格（识别为 {} / {}）；文档用 office-doc，演示文稿用 office-slide",
        app.path.display(),
        doc.app,
        doc.format
    )))
}

/// OOXML 的打印那份设置：每张表自己一份，**元素不在就是 null**。
/// 量过的两个生产者正好相反：openpyxl 只写 `pageMargins`，`pageSetup` 与 `printOptions`
/// 整个不存在（那些开关是「没说」，不是 false）；LibreOffice 重写同一份东西把十二个
/// `pageSetup` 属性全写出来，连 `paperSize="9"` 与两个 dpi 都不省
fn xlsx_print_setup(sheet_root: &xmlscan::Node) -> Value {
    let written = |name: &str| -> Value {
        let Some(node) = sheet_root.descendants(name).into_iter().next() else {
            return Value::Null;
        };
        written_attrs(node)
    };
    json!({
        // 边距在这一族是「英寸的浮点串」，照文件写的交出去，不换算成 0.01mm：
        // 0.5 与 0.511811023622047 正是两个生产者的差，换成整数就抹平了
        "margins": written("pageMargins"),
        "setup": written("pageSetup"),
        "options": written("printOptions"),
        "margin_unit": "inch",
    })
}

/// 一个元素上写着的属性，按局部名原样交出去（名字去掉前缀，值不做任何解释）
fn written_attrs(node: &xmlscan::Node) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in &node.attrs {
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    Value::Object(out)
}

/// `xl/styles.xml` 里 `dxfs` 那一跳：条件格式的规则只写一个下标（`dxfId`），真正的字色
/// 住在这里。`count` 自报的数与实际条数一起交；每条 dxf 交它里面出现的元素路径
/// （`font/b` 这种一层到底的写法）—— openpyxl 那一份写两个元素，LibreOffice 重写同一份
/// 东西给五个（多出的 `name` / `family` / `sz` 是它自己补的），所以只交出现过的名字，
/// 不替两边凑成「加粗的深红」那种共同的形状
fn dxf_table(bytes: &[u8]) -> (Option<String>, Vec<Vec<String>>) {
    let Some(member) = xml(bytes, "xl/styles.xml") else {
        return (None, Vec::new());
    };
    let root = xmlscan::parse_str(&member.as_text());
    let holder = root.descendants("dxfs").into_iter().next();
    let written = holder.and_then(|one| one.attr("count")).map(String::from);
    let kinds: Vec<Vec<String>> = holder
        .iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local() == "dxf")
        .map(|one| {
            let mut out: Vec<String> = Vec::new();
            for kid in &one.children {
                if kid.children.is_empty() {
                    out.push(kid.local().to_string());
                } else {
                    for deep in &kid.children {
                        out.push(format!("{}/{}", kid.local(), deep.local()));
                    }
                }
            }
            out
        })
        .collect();
    (written, kinds)
}

/// 这一张表上的规则：条件格式（`conditionalFormatting` 一块一套范围，里面若干条 `cfRule`）
/// 与数据验证（`dataValidations` 那个容器自报 `count`）。两边的属性全部按文件写的交，
/// 只把 `type` / `priority` / `operator` / `sqref` 另外点名，因为断言要看的就是这几个
fn sheet_rules(sheet_root: &xmlscan::Node, dxfs: &[Vec<String>], limit: usize) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    for block in sheet_root.descendants("conditionalFormatting") {
        let mut rules: Vec<Value> = Vec::new();
        for rule in block
            .children
            .iter()
            .filter(|one| one.local() == "cfRule")
            .take(limit)
        {
            let index = rule
                .attr("dxfId")
                .and_then(|raw| raw.trim().parse::<usize>().ok());
            let dxf = match (rule.attr("dxfId"), index.and_then(|at| dxfs.get(at))) {
                (Some(raw), Some(kinds)) => json!({"written": raw, "found": true, "kinds": kinds}),
                (Some(raw), None) => {
                    json!({"written": raw, "found": false, "kinds": []})
                }
                (None, _) => json!({"written": Value::Null, "found": false, "kinds": []}),
            };
            // 图标集与色阶那两种把形状写在子元素里（`cfvo` 的 type/val 与 color 的 rgb）
            let detail = rule
                .children
                .iter()
                .find(|one| matches!(one.local(), "iconSet" | "colorScale" | "dataBar" | "extLst"));
            let scale = match detail {
                Some(one) if one.local() != "extLst" => json!({
                    "kind": one.local(),
                    "written": written_attrs(one),
                    "cfvo": one.children.iter().filter(|had| had.local() == "cfvo")
                        .map(written_attrs).collect::<Vec<Value>>(),
                    "colors": one.children.iter().filter(|had| had.local() == "color")
                        .filter_map(|had| had.attr("rgb")).map(String::from)
                        .collect::<Vec<String>>(),
                }),
                _ => Value::Null,
            };
            rules.push(json!({
                "type": rule.attr("type"),
                "priority": rule.attr("priority"),
                "operator": rule.attr("operator"),
                "dxf": dxf,
                "formulas": rule.children.iter().filter(|one| one.local() == "formula")
                    .map(|one| one.text().trim().to_string()).collect::<Vec<String>>(),
                "scale": scale,
                "written": written_attrs(rule),
            }));
        }
        blocks.push(json!({
            "sqref": block.attr("sqref"),
            "rules": rules.len(),
            "rule_list": rules,
        }));
    }
    let holder = sheet_root.descendants("dataValidations").into_iter().next();
    let list: Vec<Value> = holder
        .iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local() == "dataValidation")
        .take(limit)
        .map(|one| {
            json!({
                "sqref": one.attr("sqref"),
                "type": one.attr("type"),
                "operator": one.attr("operator"),
                "formulas": one.children.iter()
                    .filter(|had| matches!(had.local(), "formula1" | "formula2"))
                    .map(|had| had.text().trim().to_string()).collect::<Vec<String>>(),
                "written": written_attrs(one),
            })
        })
        .collect();
    let written = holder.and_then(|one| one.attr("count")).map(String::from);
    let found = list.len();
    let whole = match &written {
        None => true,
        Some(raw) => raw.trim().parse::<usize>().ok() == Some(found),
    };
    json!({
        "conditional": blocks,
        "validations": {
            "written": written,
            "found": found,
            "whole": whole,
            "list": list,
        },
    })
}

/// 这张表的批注。它们**不在** `sheetN.xml` 里：要靠这张表自己的关系表
/// （`xl/worksheets/_rels/sheet1.xml.rels`，Type 结尾是 `/comments` 那一条）
/// 跳到批注部件。两个生产者把那个部件放在两个地方 —— openpyxl 写
/// `xl/comments/comment1.xml`（Target 还是绝对的 `/xl/...`），LibreOffice 写
/// `xl/comments1.xml`（Target 是相对的 `../comments1.xml`）—— `resolve_target` 两种都吃。
/// 作者名也不在格子上，只有一个 `authorId` 下标，指向同一部件开头 `<authors>` 那个列表
fn sheet_comments(bytes: &[u8], part: &str, limit: usize) -> Vec<Value> {
    let dir = match part.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    };
    let base = part.rsplit('/').next().unwrap_or(part);
    let rels_name = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(rels) = xml(bytes, &rels_name) else {
        return Vec::new();
    };
    let rel_root = xmlscan::parse_str(&rels.as_text());
    let Some(target) = rel_root
        .descendants("Relationship")
        .iter()
        .filter(|one| one.attr("TargetMode") != Some("External"))
        .find(|one| one.attr("Type").unwrap_or_default().ends_with("/comments"))
        .and_then(|one| one.attr("Target"))
    else {
        return Vec::new();
    };
    let located = resolve_target(&dir, target);
    let Some(member) = xml(bytes, &located) else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&member.as_text());
    let authors: Vec<String> = root
        .descendants("author")
        .iter()
        .map(|one| one.text().trim().to_string())
        .collect();
    let mut out: Vec<Value> = Vec::new();
    for one in root.descendants("comment").iter() {
        let index = one
            .attr("authorId")
            .and_then(|had| had.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        out.push(json!({
            "ref": one.attr("ref"),
            "author": authors.get(index).cloned(),
            "date": one.attr("date"),
            "text": one
                .descendants("t")
                .iter()
                .map(|had| had.text())
                .collect::<Vec<String>>()
                .join(""),
        }));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// 能当数看的就当数给出：xlsx 的 `<v>` 与 .xls 的 RK 是同一个数，别一个报字串一个报浮点
fn numeric_or_text(text: Option<String>) -> Value {
    match text {
        Some(one) => match one.parse::<f64>() {
            Ok(raw) => json!(raw),
            Err(_) => json!(one),
        },
        None => Value::Null,
    }
}

fn xml(bytes: &[u8], want: &str) -> Option<zipread::Member> {
    zipread::member(bytes, want, DEFAULT_MEMBER_CAP).ok()
}

/// 这个部件自己的关系表：交回「Type 结尾那个名字」与「解析成包内全名的 Target」。
/// 批注、图这些都从这一张表上跳；两种生产者的 Target 写法不同（绝对的 `/xl/...`
/// 与相对的 `../...`）由 `resolve_target` 吃掉
pub(crate) fn rels_of(bytes: &[u8], part: &str) -> Vec<(String, String)> {
    let dir = match part.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    };
    let base = part.rsplit('/').next().unwrap_or(part);
    let rels_name = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(rels) = xml(bytes, &rels_name) else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&rels.as_text());
    root.descendants("Relationship")
        .iter()
        .filter(|one| one.attr("TargetMode") != Some("External"))
        .filter_map(|one| {
            let kind = one.attr("Type").unwrap_or_default();
            let tail = kind.rsplit('/').next().unwrap_or(kind).to_string();
            let target = one.attr("Target")?;
            Some((tail, resolve_target(&dir, target)))
        })
        .collect()
}

/// 那张表上的图：要跳三跳 —— 表 →（自己的关系表）→ 画法部件 →（它的关系表）→ 图部件。
/// 两个生产者三种 Target 写法都在样本里（`/xl/drawings/…`、`../drawings/…`、`../charts/…`）
fn sheet_charts(bytes: &[u8], part: &str, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for (_, drawing) in rels_of(bytes, part) {
        if !drawing.ends_with(".xml") || !drawing.contains("/drawings/") {
            continue;
        }
        for (kind, target) in rels_of(bytes, &drawing) {
            if kind != "chart" || !target.contains("/charts/") {
                continue;
            }
            let Some(member) = xml(bytes, &target) else {
                continue;
            };
            let root = xmlscan::parse_str(&member.as_text());
            out.push(chart_one(&root, &target));
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

/// 一个引用（`c:tx` / `c:cat` / `c:val` 里面那一个）。走法只有三种：`strRef`、`numRef`
/// 与 openpyxl 那种没有引用的 `rich`；引用串**照文件写的交**（`'数据'!B1` 与 `数据!$B$1`
/// 是两个生产者对同一段格子的两种写法，替它们归一化就是替文件编东西）。
/// 缓存那份另说：openpyxl 一个字都不写（于是「图里画的是哪些数」判不住），
/// LibreOffice 写 `ptCount` 自报的点数与实际点数一起交，对不上就看得见
pub(crate) fn chart_ref(node: &xmlscan::Node) -> Value {
    let Some(one) = node
        .children
        .iter()
        .find(|had| had.local().ends_with("Ref"))
    else {
        let joined: String = node
            .descendants("t")
            .iter()
            .map(|had| had.text())
            .collect::<Vec<String>>()
            .join("");
        let text = joined.trim().to_string();
        return json!({
            "via": if text.is_empty() { Value::Null } else { json!("text") },
            "ref": Value::Null,
            "text": if text.is_empty() { Value::Null } else { json!(text) },
            "cache": null_cache(),
        });
    };
    let reference = one
        .descendants("f")
        .first()
        .map(|had| had.text().trim().to_string());
    let cache = one
        .descendants("ptCount")
        .first()
        .and_then(|had| had.attr("val"));
    let points: Vec<Value> = one
        .descendants("pt")
        .iter()
        .map(|had| {
            had.child("v")
                .map(|inner| json!(numeric_or_text(Some(inner.text().trim().to_string()))))
                .unwrap_or(Value::Null)
        })
        .collect();
    let written = cache.map(|raw| raw.trim().to_string());
    let whole = match &written {
        None => true,
        Some(raw) => raw.parse::<usize>().ok() == Some(points.len()),
    };
    json!({
        "via": one.local(),
        "ref": reference,
        "text": Value::Null,
        "cache": {"written": written, "points": points.len(), "values": points, "whole": whole},
    })
}

fn null_cache() -> Value {
    json!({"written": Value::Null, "points": 0usize, "values": [], "whole": true})
}

/// 整个引用不在（这张图没写 `c:cat`，或标题是个空 `c:title`）：形状与「写了引用但
/// 没有缓存」要能分开，所以 via/ref/text 三个都是 null，而不是缺键
fn no_ref() -> Value {
    json!({"via": Value::Null, "ref": Value::Null, "text": Value::Null, "cache": null_cache()})
}

/// 一张图：类型那一组（`barChart` 与 `lineChart` 这些）、标题的两种写法、每条系列
pub(crate) fn chart_one(root: &xmlscan::Node, part: &str) -> Value {
    let Some(chart) = root.descendants("chart").first().copied() else {
        return json!({"part": part, "present": false});
    };
    // 标题的三种写法都从 `c:title/c:tx` 那一个元素上分：`c:strRef`（引用一个格子）、
    // `c:rich`（字面量，两个生产者目前都走这条）与整个 title 不在
    let title = match chart.child("title").and_then(|one| one.child("tx")) {
        Some(tx) => chart_ref(tx),
        None => match chart.child("title") {
            Some(node) => chart_ref(node),
            None => no_ref(),
        },
    };
    let mut groups: Vec<Value> = Vec::new();
    let mut any_cached = false;
    for group in chart
        .child("plotArea")
        .into_iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local().ends_with("Chart"))
    {
        let mut written = serde_json::Map::new();
        let mut axis_ids: Vec<String> = Vec::new();
        let mut series: Vec<Value> = Vec::new();
        for one in &group.children {
            match one.local() {
                "ser" => series.push(json!({
                    "index": one.child("idx").and_then(|had| had.attr("val")).map(String::from),
                    "order": one.child("order").and_then(|had| had.attr("val")).map(String::from),
                    "name": one.child("tx").map(chart_ref).unwrap_or_else(no_ref),
                    "cat": one.child("cat").map(chart_ref).unwrap_or_else(no_ref),
                    "val": one.child("val").map(chart_ref).unwrap_or_else(no_ref),
                })),
                "axId" => {
                    if let Some(raw) = one.attr("val") {
                        axis_ids.push(raw.to_string());
                    }
                }
                _ => {
                    if let Some(raw) = one.attr("val") {
                        written.insert(one.local().to_string(), json!(raw));
                    }
                }
            }
        }
        // 「图里画的是哪些数」只有文件自己缓存过才算判得住：openpyxl 一条 pt 都不写
        any_cached |= series
            .iter()
            .any(|one| one["val"]["cache"]["points"].as_u64().unwrap_or(0) > 0);
        groups.push(json!({
            "kind": group.local(),
            "written": written,
            "axis_ids": axis_ids,
            "series": series.len(),
            "series_list": series,
        }));
    }
    json!({
        "part": part,
        "present": true,
        "title": title,
        "cached": any_cached,
        "groups": groups,
    })
}

fn shared_strings(bytes: &[u8]) -> Vec<String> {
    match xml(bytes, "xl/sharedStrings.xml") {
        Some(member) => {
            let root = xmlscan::parse_str(&member.as_text());
            root.descendants("si")
                .iter()
                .map(|one| one.text().trim().to_string())
                .collect()
        }
        None => Vec::new(),
    }
}

/// 一格最多铺进 CSV 的总数上限：坏文件可以自报几百万格，铺完就成了内存事故
const MAX_GRID_CELLS: usize = 200_000;

/// `hidden` 这个开关两种写法都有：openpyxl 写 `hidden="1"`，LibreOffice 写
/// `hidden="true"`（而且没隐藏的行也照样写 `hidden="false"`）
fn hidden_on(node: &xmlscan::Node) -> bool {
    matches!(
        node.attr("hidden")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true"
    )
}

/// `<col>` 是带跨度的：LibreOffice 把连续三列并成一条 `min="3" max="5"`，
/// 按元素个数数就会少报两列。只写一端就按一列算（另一端规范里默认与之同值）
fn col_span(node: &xmlscan::Node) -> usize {
    let min = node
        .attr("min")
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    let max = node
        .attr("max")
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    match (min, max) {
        (Some(first), Some(last)) if last >= first => last - first + 1,
        _ => 1,
    }
}

/// `B4` → (行 3, 列 1)。不是 A1 形状就交回 None —— 位置猜不出来就不铺。
fn split_ref(reference: &str) -> Option<(usize, usize)> {
    let raw = reference.trim();
    let split = raw.find(|ch: char| ch.is_ascii_digit())?;
    let letters = &raw[..split];
    let digits = &raw[split..];
    if letters.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut col = 0usize;
    for ch in letters.chars() {
        let letter = ch.to_ascii_uppercase();
        if !('A'..='Z').contains(&letter) {
            return None;
        }
        col = col.checked_mul(26)? + usize::from(letter as u8 - b'A') + 1;
    }
    let row: usize = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

/// CSV 里的一格：RFC4180 —— 带逗号、引号、换行就整体加引号，里面的引号翻倍
fn csv_field(raw: &str) -> String {
    if raw.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

/// 把 (行, 列, 文本) 铺成一张网再拼成 CSV：空洞是空字段，尾部不裁（空格子也是格子）
fn render_csv(cells: &[(usize, usize, String)]) -> (String, usize, usize) {
    let rows = cells.iter().map(|one| one.0 + 1).max().unwrap_or(0);
    let cols = cells.iter().map(|one| one.1 + 1).max().unwrap_or(0);
    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); cols]; rows];
    for (row, col, text) in cells {
        if *row < rows && *col < cols {
            grid[*row][*col] = text.clone();
        }
    }
    let mut out = String::new();
    for line in &grid {
        out.push_str(
            &line
                .iter()
                .map(|one| csv_field(one.as_str()))
                .collect::<Vec<String>>()
                .join(","),
        );
        out.push('\n');
    }
    (out, rows, cols)
}

/// ODS 一格进 CSV 该写什么：跟 xlsx 那条同一口径 —— **给值，不给显示格式**，
/// 日期给 ISO（ODF 本来就写着 ISO），布尔给 TRUE/FALSE，文本给文本。
/// 于是同一份数据在 xlsx 与 ods 两边拼出来的 CSV 是一样的，
/// 而不是 `12.5%` 与 `0.125` 各来一份。
fn ods_display(one: &crate::odsheet::Cell) -> String {
    if let Some(raw) = &one.date_value {
        return raw.clone();
    }
    match one.value_type.as_str() {
        "boolean" => {
            if one.boolean_value.as_deref() == Some("true") {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        "float" | "percentage" | "currency" => one
            .value
            .as_ref()
            .and_then(|raw| raw.parse::<f64>().ok())
            .map(|value| format!("{value}"))
            .unwrap_or_else(|| one.text.clone()),
        _ => one.text.clone(),
    }
}

/// `--sheet` 说的是序号还是表名：先试序号（命令报的 `index` 从 0 起），再按表名找
fn pick_sheet(names: &[String], want: &str) -> Option<usize> {
    let want = want.trim();
    if want.is_empty() {
        return if names.is_empty() { None } else { Some(0) };
    }
    if let Some(index) = want.parse::<usize>().ok().filter(|one| *one < names.len()) {
        return Some(index);
    }
    names.iter().position(|one| one == want)
}

/// `--csv` 的那份账：没开就整个键都不给（不给一份空 CSV 装作有）
fn csv_report(
    want: bool,
    names: &[String],
    grids: &[Vec<(usize, usize, String)>],
    pick: &str,
    skipped: usize,
) -> Value {
    if !want {
        return Value::Null;
    }
    let Some(index) = pick_sheet(names, pick) else {
        return json!({"error": format!("没有这张表「{}」；这份文件里的表是 {:?}", pick, names)});
    };
    let (text, rows, columns) =
        render_csv(grids.get(index).map(|one| &one[..]).unwrap_or_default());
    json!({
        "sheet": names[index],
        "index": index,
        "rows": rows,
        "columns": columns,
        "cells_skipped": skipped,
        "line_end": "LF",
        "text": text,
    })
}

/// 把格式账并到格子那条记录上（`cell_format` 交回的是一个小对象）
fn merge(base: Value, extra: Value) -> Value {
    let mut map = base.as_object().cloned().unwrap_or_default();
    if let Some(more) = extra.as_object() {
        for (key, value) in more {
            map.insert(key.clone(), value.clone());
        }
    }
    Value::Object(map)
}

fn bump(totals: &mut Value, key: &str, add: usize) {
    if let (Some(old), Some(map)) = (totals[key].as_u64(), totals.as_object_mut()) {
        map.insert(key.to_string(), json!(old + add as u64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
            csv: false,
            sheet: String::new(),
        };
        let (tx, _rx) = mpsc::channel();
        run_office_sheet(&app, &Context::new_test(tx)).expect("office-sheet 应成功")
    }

    /// 保护这份账分两层，还有两种拼法（期望值来自 `lyco_protect.py`）：
    /// openpyxl 写 `sheet="1" formatCells="0"`，LibreOffice 重写同一份东西写
    /// `sheet="true" formatCells="false"` 并把等于默认的开关省掉
    #[test]
    fn protection_is_accounted_for_at_both_levels() {
        let hand = run("locked-sheet.xlsx");
        let prot = &hand["protection"];
        assert_eq!(prot["workbook"]["element"], json!(true), "{prot}");
        assert_eq!(prot["workbook"]["lock_structure"], json!(true));
        assert_eq!(prot["workbook"]["book_password"], json!(true));
        let sheets = prot["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 3, "{prot}");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["protected"], json!(true));
        assert_eq!(sheets[0]["written"]["formatCells"], json!(false));
        assert_eq!(sheets[0]["written"]["insertRows"], json!(true));
        assert_eq!(sheets[1]["element"], json!(false), "没写的表要说「没写」");
        assert_eq!(sheets[1]["protected"], json!(false));

        let lo = run("locked-sheet-lo.xlsx");
        let two = &lo["protection"];
        assert_eq!(
            two["sheets"][0]["protected"],
            json!(true),
            "两种拼法同一个结论"
        );
        assert_eq!(two["sheets"][0]["written"]["formatCells"], json!(false));
        assert_eq!(
            two["sheets"][0]["written"].get("insertRows"),
            None,
            "省掉的不补上"
        );
        // LibreOffice 导出 xlsx 时把结构锁丢成了一个空元素 —— 照实报，不替它补
        assert_eq!(two["workbook"]["element"], json!(true));
        assert_eq!(two["workbook"]["lock_structure"], Value::Null);
        assert_eq!(two["workbook"]["book_password"], json!(false));

        // openpyxl 本来就在 book.xlsx 里留了一个空的 workbookProtection：
        // 元素在场不等于锁上
        let plain = run("book.xlsx");
        assert_eq!(plain["protection"]["workbook"]["element"], json!(true));
        assert_eq!(
            plain["protection"]["workbook"]["lock_structure"],
            Value::Null
        );
        assert!(plain["protection"]["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["protected"] == json!(false)));
    }

    /// ODF 的表保护是 `table:table` 身上的属性，摘要那条 URI 只留最后一段
    #[test]
    fn opendocument_table_locks_come_off_the_table() {
        let out = run("locked-sheet.ods");
        let sheets = out["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 3, "{out}");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["protected"], json!(true));
        assert_eq!(
            sheets[0]["password"],
            json!(true),
            "table:protection-key 在"
        );
        assert_eq!(sheets[0]["digest"], json!("legacy-hash-excel"));
        assert_eq!(sheets[1]["protected"], json!(false));
        assert_eq!(sheets[1]["digest"], Value::Null);
        assert_eq!(
            out["protection"]["workbook"]["element"],
            json!(false),
            "ODF 没有工作簿那一层"
        );
    }

    /// `.xls` 那一族的锁按表记：两份件唯一的差别是锁在第一张还是第二张表，
    /// 而那三条记录跟着挪窝。期望值来自 `lyco_legacy.py` 对同两份件的读取；
    /// 「这位为真就是锁上了」另有一证 —— LibreOffice 自己 import 回 .ods 时
    /// 只在同一张表上写 `table:protected="true"`
    #[test]
    fn legacy_xls_attributes_its_lock_to_the_sheet_that_carries_it() {
        let hand = run("locked-sheet.xls");
        let locks = hand["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(locks.len(), 3, "{hand}");
        assert_eq!(locks[0]["name"], json!("预算表"));
        assert_eq!(locks[0]["protected"], json!(true));
        assert_eq!(locks[0]["password"], json!(true));
        assert_eq!(locks[0]["password_hash"], json!("6e4e"));
        assert_eq!(
            locks[0]["records"],
            json!({"0x0012": 1, "0x0013": 28238, "0x00dd": 1}),
            "原值也要留着：0x00DD 这一条没有第二个读者认得，不替它编开关名"
        );
        assert_eq!(locks[1]["protected"], json!(false), "锁不在第二张");
        assert_eq!(locks[1]["records"], json!({}));
        assert_eq!(locks[2]["records"], json!({}));

        let second = run("locked-second.xls");
        let moved = second["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(moved[0]["protected"], json!(false), "{second}");
        assert_eq!(moved[1]["name"], json!("说明"));
        assert_eq!(moved[1]["protected"], json!(true));
        assert_eq!(moved[1]["password_hash"], json!("6e4e"));
        assert_eq!(moved[2]["records"], json!({}));

        let plain = run("book.xls");
        assert!(plain["protection"]["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["protected"] == json!(false) && one["records"] == json!({})));
        assert!(plain["notes"]
            .as_array()
            .expect("说明")
            // 这里查的是「没锁就别谈锁」，所以要用只有那条锁说明才会用的词。
            // 先前查的是「子流」，而批注那条说明（每次都交）也说注住在表子流末尾 ——
            // 一个偶然同词就把这条守卫弄红了，换一个专有的
            .iter()
            .all(|one| !one.as_str().unwrap_or_default().contains("表锁")));
    }

    /// `.xls` 的数字格式那一跳：格子的 ixfe 是 XF 记录的**出现序号**，XF 自报的格式号
    /// 在正文偏移 2，自定义号（>=164）的串在 FORMAT 记录里、内置号查那张内置表。
    /// 换算出来的日期与 LibreOffice 自己把这份 .xls 读回 .ods 交出的 date-value 逐格一致
    #[test]
    fn legacy_xls_takes_the_same_style_hop_as_xlsx() {
        let out = run("formats.xls");
        assert_eq!(
            out["workbook"]["date1904"],
            json!(false),
            "DATEMODE 说的是 1900"
        );
        assert_eq!(out["workbook"]["xfs"].as_array().map(Vec::len), Some(28));
        assert_eq!(out["workbook"]["formats"]["165"], json!("yyyy\\-mm\\-dd"));
        let one = |sheet: &str, reference: &str| -> Value {
            out["cells"]
                .as_array()
                .and_then(|list| {
                    list.into_iter()
                        .find(|had| had["sheet"] == json!(sheet) && had["ref"] == json!(reference))
                        .cloned()
                })
                .unwrap_or(Value::Null)
        };
        let day = one("格式", "C1");
        assert_eq!(day["num_fmt"], json!(165));
        assert_eq!(day["format_kind"], json!("date"));
        assert_eq!(
            day["as_date"],
            json!("2013-12-23"),
            "格式串说它是日期，序列数就换算"
        );
        let stamp = one("格式", "C2");
        assert_eq!(stamp["format_kind"], json!("datetime"));
        assert_eq!(stamp["as_date"], json!("2013-12-23T15:15:00"));
        let part = one("格式", "C3");
        assert_eq!(part["format_kind"], json!("percent"));
        assert!(part.get("as_date").is_none(), "百分比不是日期");
        // 币符在 BIFF 里写成转义的字面量（\\¥），与 ODF 那种「字面量写在格式里」同一种存法：
        // 照字面判成数，不替它认成货币
        assert_eq!(one("格式", "C4")["format_kind"], json!("number"));
        assert_eq!(
            one("格式", "C5")["as_date"],
            json!("2013-12-23"),
            "汉字字面量不挡判定"
        );
        assert_eq!(
            one("格式", "C6")["num_fmt"],
            json!(164),
            "General 在这份件里是自定义号，不是内置 0"
        );
        assert_eq!(
            one("格式", "C7")["format_kind"],
            json!("text"),
            "文本格不拿格式串猜它是什么"
        );
        assert_eq!(one("另一张", "A1")["as_date"], json!("2026-09-23"));

        // 同一判断也管 CSV：日期格出去的是 ISO，不是序列数
        let grid = run_csv("formats.xls", "格式");
        let shown = grid["csv"]["text"].as_str().unwrap_or_default();
        assert!(shown.contains("2013-12-23"), "{shown}");
        assert!(!shown.contains("41631"), "换了就不该再看见序列数：{shown}");
    }

    /// 开 `--csv` 的那条路：`sheet` 是给 `--sheet` 的原样字符串（空=第一张）
    fn run_csv(name: &str, sheet: &str) -> Value {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
            csv: true,
            sheet: sheet.to_string(),
        };
        let (tx, _rx) = mpsc::channel();
        run_office_sheet(&app, &Context::new_test(tx)).expect("office-sheet 应成功")
    }

    /// openpyxl 那份工作簿：三张表（含一张隐藏）、各自的范围与格子数
    /// （期望值来自 `office_reader.py` 的 xlsx_facts）
    #[test]
    fn lists_every_sheet_including_the_hidden_one() {
        let out = run("book.xlsx");
        assert_eq!(out["kind"], "spreadsheetml");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        let sheets = out["sheets"].as_array().expect("是数组");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["dimension"], "A1:B5", "{}", sheets[0]);
        assert_eq!(sheets[0]["cells"], 9);
        assert_eq!(sheets[0]["formulas"], 1);
        assert_eq!(sheets[0]["merged"], 1);
        assert_eq!(sheets[0]["state"], "visible");
        assert_eq!(sheets[2]["name"], "草稿");
        assert_eq!(sheets[2]["state"], "hidden");
        // openpyxl 不用共享字符串表，也不写缓存结果：两件事都要报出来而不是被含糊过去
        assert_eq!(out["workbook"]["shared_strings"], 0);
        assert_eq!(
            sheets[0]["formula_cells_with_cached_value"], 0,
            "文件里没有缓存值"
        );
        assert_eq!(out["workbook"]["has_calc_chain"], json!(false));
        assert_eq!(out["workbook"]["tables"], 1);
        // 没开 --csv 时这个键是 null，而不是一份空 CSV 装作有
        assert!(
            matches!(out.get("csv"), Some(one) if one.is_null()),
            "{out}"
        );
        assert_eq!(out["defined_names"][0]["text"], "'预算表'!$B$4", "{out}");
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// 内联字符串要还原成文字，不能只报一个 SST 索引
    #[test]
    fn inline_strings_become_their_text() {
        let out = run("book.xlsx");
        let cells = out["sheets"][0]["cell_list"].as_array().expect("是数组");
        let first = &cells[0];
        assert_eq!(first["ref"], "A1");
        assert_eq!(first["value"], "科目", "{first}");
        assert_eq!(
            cells[3]["value"],
            json!(124000.0),
            "数字格子要报成数：{}",
            cells[3]
        );
        assert_eq!(
            cells[7]["formula"], "SUM(B2:B3)",
            "公式格子报公式：{}",
            cells[7]
        );
    }

    /// 遗留 .xls 走 BIFF8：表名、可见性、字符串表与格子都读得出来，
    /// 并按表归位（BOUNDSHEET 自报的子流起点）
    #[test]
    fn reads_a_legacy_workbook() {
        let out = run("book.xls");
        assert_eq!(out["kind"], "biff8");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        assert_eq!(out["workbook"]["shared_strings"], 8);
        assert_eq!(out["workbook"]["totals"]["cells"], 11, "{out}");
        assert_eq!(out["workbook"]["totals"]["formulas"], 1);
        assert_eq!(out["sheets"][0]["cells"], 9, "{out}");
        assert_eq!(out["sheets"][1]["cells"], 1);
        assert_eq!(out["sheets"][2]["cells"], 1, "隐藏表也有格子");
        let named: Vec<String> = out["cells"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter_map(|one| one["sheet"].as_str().map(|one| one.to_string()))
            .collect();
        assert_eq!(
            named.iter().filter(|one| *one == "预算表").count(),
            9,
            "{named:?}"
        );
        assert!(named.iter().any(|one| one == "草稿"), "{named:?}");
    }

    /// 数字格式：`s=` 是 `cellXfs` 的下标。日期格要认出来并换算，而 `12/23/2013`
    /// 那种长得像日期的**文本**不许被猜成日期（期望值来自 `lyco_formats.py`）
    #[test]
    fn a_cell_number_is_not_a_date_until_the_style_says_so() {
        let out = run("formats.xlsx");
        assert_eq!(out["kind"], "spreadsheetml");
        assert_eq!(out["workbook"]["date1904"], json!(false), "{out}");
        assert_eq!(out["workbook"]["totals"]["dates"], 4, "{out}");
        let cells = out["sheets"][0]["cell_list"].as_array().expect("是数组");
        let find = |want: &str| {
            cells
                .iter()
                .find(|one| one["ref"] == want)
                .cloned()
                .unwrap_or(Value::Null)
        };
        assert_eq!(find("C1")["format_kind"], "date", "{out}");
        assert_eq!(find("C1")["as_date"], "2013-12-23", "{}", find("C1"));
        assert_eq!(find("C2")["as_date"], "2013-12-23T15:15:00");
        assert_eq!(find("C3")["format_kind"], "percent");
        assert_eq!(find("C4")["format_kind"], "currency");
        assert_eq!(
            find("C5")["format"],
            "yyyy\"年\"m\"月\"d\"日\"",
            "{}",
            find("C5")
        );
        assert_eq!(find("C6")["format_kind"], "general");
        let trap = find("C7");
        assert_eq!(
            trap["format_kind"], "text",
            "长得像日期的文本仍是文本：{trap}"
        );
        assert!(trap.get("as_date").is_none(), "{trap}");
        assert_eq!(find("A1")["style"], 0, "没写 s 就是 0 号样式");
        assert_eq!(out["sheets"][1]["date_cells"], 1, "{out}");
    }

    /// ODS 走的是另一套账：格子内不写序列数，位置靠重复计数累加，
    /// 隐藏表的可见性在它引的自动样式里（期望值来自 `ods_facts()`）
    #[test]
    fn an_opendocument_spreadsheet_is_read_on_its_own_terms() {
        let out = run("book.ods");
        assert_eq!(out["kind"], "opendocument-spreadsheet");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        assert_eq!(
            out["workbook"]["cell_total"], 11,
            "生产者自己写的 cell-count"
        );
        assert_eq!(out["workbook"]["totals"]["formulas"], 1);
        assert_eq!(out["workbook"]["totals"]["covered"], 1);
        let states: Vec<&str> = out["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["state"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(states, ["visible", "visible", "hidden"], "{out}");
        let first = &out["sheets"][0];
        assert_eq!(first["columns"], 2, "行尾那 16382 个空格不许算列：{first}");
        assert_eq!(first["value_types"]["string"], 6, "{first}");
        assert_eq!(first["value_types"]["float"], 3, "{first}");
        let listed = first["cell_list"].as_array().expect("是数组");
        let merged = listed
            .iter()
            .find(|one| one["ref"] == "A5")
            .expect("A5 那一格跨两列");
        assert_eq!(merged["columns_spanned"], 2, "{merged}");
        assert_eq!(merged["text"], "口径：含税");

        let formats = run("formats.ods");
        assert_eq!(formats["workbook"]["totals"]["dates"], 4, "{formats}");
        let kinds: Vec<&str> = formats["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            kinds,
            [
                "string",
                "string",
                "date",
                "date",
                "percentage",
                "float",
                "date",
                "float",
                "string",
                "boolean"
            ],
            "{formats}"
        );
    }

    /// `--csv`：一份铺平的网格（期望文本逐字来自 `office_reader.py` 的 csv_facts）
    #[test]
    fn csv_renders_the_grid_of_a_sheet() {
        let out = run_csv("book.ods", "");
        let csv = &out["csv"];
        assert_eq!(csv["sheet"], "预算表");
        assert_eq!(csv["index"], 0);
        assert_eq!(csv["rows"], 5, "{csv}");
        assert_eq!(csv["columns"], 2);
        assert_eq!(csv["cells_skipped"], 0);
        assert_eq!(csv["line_end"], "LF");
        assert_eq!(
            csv["text"], "科目,金额\n服务器,124000\n网络,18000\n合计,142000\n口径：含税,\n",
            "{csv}"
        );
    }

    /// 公式格没有缓存值时 CSV 里那一格是空的：这不是漏，是文件本来就没给数
    #[test]
    fn a_formula_without_a_cached_result_renders_empty() {
        let out = run_csv("book.xlsx", "");
        assert_eq!(
            out["csv"]["text"],
            "科目,金额\n服务器,124000\n网络,18000\n合计,\n口径：含税,\n"
        );
        assert_eq!(out["csv"]["rows"], 5);
        assert_eq!(out["sheets"][0]["formula_cells_with_cached_value"], 0);
    }

    /// `--sheet` 认表名也认命令报出的序号，认不出来就把候选说清楚
    #[test]
    fn csv_picks_a_sheet_by_name_or_by_index() {
        let by_name = run_csv("book.ods", "草稿");
        assert_eq!(by_name["csv"]["sheet"], "草稿");
        assert_eq!(by_name["csv"]["index"], 2);
        assert_eq!(by_name["csv"]["text"], "隐藏的草稿表\n");
        let by_index = run_csv("book.ods", "1");
        assert_eq!(by_index["csv"]["sheet"], "说明", "序号从 0 起");
        let missing = run_csv("book.ods", "没这张");
        assert!(
            missing["csv"]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("预算表"),
            "{missing}"
        );
        assert!(
            run_csv("book.ods", "9")["csv"]["error"].is_string(),
            "序号越界也要走 error 那条话，不能默默给第一张"
        );
    }

    /// 日期格进 CSV 给 ISO 读法，百分数与货币给值不给 `12.5%` 那一份
    /// （与 `--csv` 之外那套格式账同源，两边不许各编一个数）
    #[test]
    fn csv_gives_values_not_display_strings() {
        let out = run_csv("formats.xlsx", "");
        assert_eq!(
            out["csv"]["text"],
            "标签,日期,2013-12-23\n,,2013-12-23T15:15:00\n,,0.125\n,,124000\n\
             ,,2013-12-23\n,,1234.5\n,,12/23/2013\n,,TRUE\n",
            "{out}"
        );
        assert_eq!(out["csv"]["rows"], 8);
        assert_eq!(out["csv"]["columns"], 3);
    }

    /// 同一份数据在 xlsx 与 ods 两边拼出来的 CSV 必须逐字相同 —— 这两套账
    /// 一个是序列数 + 格式码、一个是显式 value-type，能对上才说明两边都没猜
    #[test]
    fn the_same_workbook_renders_the_same_csv_in_both_formats() {
        for sheet in ["", "另一张"] {
            let xlsx = run_csv("formats.xlsx", sheet);
            let ods = run_csv("formats.ods", sheet);
            assert_eq!(xlsx["csv"]["text"], ods["csv"]["text"], "sheet={sheet}");
        }
        for sheet in ["", "说明", "草稿"] {
            let xlsx = run_csv("book.xlsx", sheet);
            let ods = run_csv("book.ods", sheet);
            if sheet.is_empty() {
                // 只有第一张差一格：那边公式没缓存值，这边 LibreOffice 写了 142000
                assert_ne!(xlsx["csv"]["text"], ods["csv"]["text"]);
            } else {
                assert_eq!(xlsx["csv"]["text"], ods["csv"]["text"], "sheet={sheet}");
            }
        }
    }

    /// 表格里的批注**不在** `sheetN.xml` 里，要靠这张表自己的关系表跳到批注部件。
    /// 两个生产者把那个部件放在两个地方（openpyxl `xl/comments/comment1.xml`、
    /// LibreOffice `xl/comments1.xml`），关系 Target 一个绝对一个相对，条目顺序还不一样
    /// —— 三份件（两份 xlsx 加一份 .ods）必须读出同一批 (格子, 作者, 字)。
    /// 期望值来自 `office_reader.py` 的 `xlsx_comments()` 与 `ods_facts()`
    #[test]
    fn spreadsheet_comments_are_reached_through_the_sheet_relationships() {
        let expect: Vec<(String, String, String)> = vec![
            (
                "A3".to_string(),
                "李四".to_string(),
                "第二张单已确认".to_string(),
            ),
            (
                "B2".to_string(),
                "张三".to_string(),
                "这里要补上不含税口径".to_string(),
            ),
            (
                "B3".to_string(),
                "李四".to_string(),
                "同一个作者再来一条".to_string(),
            ),
        ];
        for name in ["cell-notes.xlsx", "cell-notes-lo.xlsx", "cell-notes.ods"] {
            let out = run(name);
            let sheet = &out["sheets"][0];
            assert_eq!(sheet["comments"], 3, "{name}：{sheet}");
            let mut got: Vec<(String, String, String)> = sheet["comment_list"]
                .as_array()
                .expect("是数组")
                .iter()
                .map(|one| {
                    (
                        one["ref"].as_str().unwrap_or_default().to_string(),
                        one["author"].as_str().unwrap_or_default().to_string(),
                        one["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            got.sort();
            assert_eq!(got, expect.clone(), "{name}");
            assert_eq!(out["workbook"]["totals"]["comments"], 3, "{name} 的总账");
        }
        // ODF 那份尤其容易混：注就坐在格子里面，一锅端取字就会把它当这一格的内容
        let ods = run("cell-notes.ods");
        let noted = ods["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .find(|one| one["ref"] == json!("B2"))
            .expect("有 B2 这一格");
        assert_eq!(noted["text"], json!("124000"), "{noted}");
        // 两个生产者都没往批注里写时间：那就交回 null，不替它编一个
        let dated = run("cell-notes.xlsx")["sheets"][0]["comment_list"]
            .as_array()
            .expect("是数组")
            .clone();
        assert!(dated.iter().all(|one| one["date"].is_null()), "{dated:?}");
        // 反面对照：没有批注的那份报 0，而不是没有这个键
        assert_eq!(run("book.xlsx")["workbook"]["totals"]["comments"], 0);
    }

    /// 第四种存法：`.xls` 的注既不在另一个部件里，也不在格子里面，而在**同一条流**的
    /// 两类记录上 —— 一条给字（正文偏移 10 是它自报的字数，紧跟的第一条 CONTINUE 首字节
    /// 说编码），一条给「哪个格子 + 谁写的」（住在这张表子流的末尾）。两份列表按出现顺序配。
    /// 这份件是一次只改一个变量造出来的：作者名有 ASCII 的也有中文的（那位编码旗标与 BIFF8
    /// 的 fCompressed 惯例相反）、字数有 2 也有 3、注的字带换行、格子拉到 AA100 让列名进
    /// 两位数、注还分到两张表上。期望值来自 `lyco_legacy.py` 的 `biff_workbook()`，
    /// 而 (表, 格子, 字) 这一列另与 LibreOffice 自己把这份 .xls 转回 .xlsx 后读到的对得上
    #[test]
    fn xls_comments_are_paired_out_of_two_record_kinds() {
        let out = run("cell-notes-many.xls");
        assert_eq!(out["workbook"]["totals"]["comments"], 4, "{out}");
        let expect: Vec<Value> = vec![
            json!({"sheet": "预算表", "ref": "A1", "author": "AB", "text": "one"}),
            json!({
                "sheet": "预算表",
                "ref": "C5",
                "author": "张三",
                "text": "第一行\n第二行"
            }),
            json!({
                "sheet": "预算表",
                "ref": "AA100",
                "author": "欧阳锋",
                "text": "这里要补上不含税口径"
            }),
            json!({
                "sheet": "第二张",
                "ref": "B2",
                "author": "李四",
                "text": "第二张单已确认"
            }),
        ];
        let mut got: Vec<Value> = Vec::new();
        for sheet in out["sheets"].as_array().expect("是数组") {
            for one in sheet["comment_list"].as_array().expect("是数组") {
                got.push(json!({
                    "sheet": sheet["name"],
                    "ref": one["ref"],
                    "author": one["author"],
                    "text": one["text"],
                }));
            }
        }
        assert_eq!(got, expect, "{out}");
        for sheet in out["sheets"].as_array().expect("是数组") {
            // 两类记录各自的条数一起交：配的条数只到小的那个，所以对不上时这里看得见
            assert_eq!(sheet["note_text_records"], sheet["comments"], "{sheet}");
            assert_eq!(sheet["note_cell_records"], sheet["comments"], "{sheet}");
            let list = sheet["comment_list"].as_array().expect("是数组");
            assert!(!list.is_empty(), "{sheet}");
            // 两边自报的字数都正好切出来（切不出来会留 whole=false，不装作读全了）
            assert!(
                list.iter().all(|one| one["whole"] == json!(true)),
                "{sheet}"
            );
            // 这一族的三条记录里没有作者时间那个字段，所以交回 null 而不是编一个
            assert!(list.iter().all(|one| one["date"].is_null()), "{sheet}");
        }
        // 不按子流归位就会把第二张表的注挂到第一张上
        assert_eq!(out["sheets"][1]["name"], "第二张", "{out}");
        assert_eq!(out["sheets"][1]["comments"], 1, "{out}");
        assert_eq!(out["sheets"][0]["comments"], 3, "{out}");
        // 反面对照：四份没有注的 .xls 报 0 条，键在、值为零
        assert_eq!(run("book.xls")["workbook"]["totals"]["comments"], 0);
        assert_eq!(run("hidden.xls")["workbook"]["totals"]["comments"], 0);
    }

    /// 隐藏的行与列有四种写法：openpyxl 一列一条 `hidden="1"`、LibreOffice 把连续
    /// 三列并成一条 `min="3" max="5" hidden="true"`、ODF 用
    /// `table:visibility="collapse"` 压在一整条 `number-columns-repeated="3"` 上，
    /// .xls 把它们写成 ROW 记录的一个位与 COLINFO 的一段范围。
    /// 四种存法必须报出同一个数
    #[test]
    fn hidden_rows_and_columns_are_counted_whichever_way_they_are_written() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods", "hidden.xls"] {
            let out = run(name);
            let sheet = &out["sheets"][0];
            assert_eq!(sheet["name"], "预算表", "{name}");
            assert_eq!(sheet["hidden_rows"], 2, "{name}：{sheet}");
            assert_eq!(sheet["hidden_cols"], 3, "{name}：{sheet}");
            assert_eq!(sheet["cells"], 13, "藏起来的格子还是格子：{name}");
            // 合计那份也要一致：CI 上第一轮就是「每张表都对、总账是 0」被抓出来的
            assert_eq!(out["workbook"]["totals"]["hidden_rows"], 2, "{name} 的总账");
            assert_eq!(out["workbook"]["totals"]["hidden_cols"], 3, "{name} 的总账");
        }
    }

    /// 隐藏只是「看不见」，不是「没有」：那些字要照样出现在 CSV 里
    #[test]
    fn hidden_columns_still_carry_their_text() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods", "hidden.xls"] {
            let text = run_csv(name, "")["csv"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                text.contains("第一列批注") && text.contains("第二列批注"),
                "{name}：{text}"
            );
        }
    }

    /// ODS 的格式在样式那一跳后面（期望值来自 `office_reader.py` 的 ods_styles）：
    /// 格子只写一个样式名，样式再指数据样式，数据样式才说这是日期、百分数还是布尔
    #[test]
    fn an_ods_cell_reports_the_data_style_it_inherits() {
        let out = run("formats.ods");
        let cells = out["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| {
                (
                    one["ref"].as_str().unwrap_or_default().to_string(),
                    one.clone(),
                )
            })
            .collect::<std::collections::HashMap<String, Value>>();
        let cell = |want: &str| cells.get(want).cloned().unwrap_or(Value::Null);
        let plain = cell("C1");
        assert_eq!(plain["cell_style"], "ce1", "{plain}");
        assert_eq!(plain["data_style"], "N49");
        assert_eq!(plain["format_kind"], "date");
        assert_eq!(
            plain["format_tokens"],
            json!(["year", "text:-", "month", "text:-", "day"]),
            "元素树逐条抄，不替它拼格式串：{plain}"
        );
        let percent = cell("C3");
        assert_eq!(percent["format_kind"], "percent", "{percent}");
        assert_eq!(percent["decimals"], 1);
        let money = cell("C4");
        assert_eq!(
            money["format_kind"], "number",
            "¥ 是字面量，不是 currency-style"
        );
        assert_eq!(money["currency_symbol"], Value::Null, "{money}");
        let flag = cell("C8");
        assert_eq!(flag["format_kind"], "bool", "{flag}");
        // 没写样式的格子不交一份空对象：那一格本来就没有这一跳
        assert!(cell("C6").get("cell_style").is_none(), "{}", cell("C6"));
        assert!(
            out["workbook"]["styles"]["data_styles"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    }

    /// 每张表的打印设置：只报文件自己写的，缺的元素交 null（不是 false），单位按原样交。
    /// 期望值逐字来自 `.scratch/shadow_sheet_print.py` —— python 侧同算法的镜子
    #[test]
    fn print_setup_reports_only_what_each_sheet_written() {
        let hand = run("hidden.xlsx");
        let mine = &hand["sheets"][0]["print_setup"];
        assert_eq!(mine["margin_unit"], "inch", "{mine}");
        assert_eq!(mine["margins"]["left"], "0.75", "{mine}");
        assert_eq!(mine["margins"]["top"], "1");
        assert_eq!(mine["margins"]["header"], "0.5");
        assert_eq!(mine["setup"], Value::Null, "openpyxl 整个不写 pageSetup");
        assert_eq!(
            mine["options"],
            Value::Null,
            "那些开关是「没说」，不是 false"
        );

        let lo = run("hidden-lo.xlsx");
        let theirs = &lo["sheets"][0]["print_setup"];
        assert_eq!(
            theirs["margins"]["header"], "0.511811023622047",
            "边距原样交，换成整数就把两个生产者的差抹平了：{theirs}"
        );
        assert_eq!(theirs["setup"]["paperSize"], "9", "{theirs}");
        assert_eq!(theirs["setup"]["orientation"], "portrait");
        assert_eq!(theirs["setup"]["horizontalDpi"], "300");
        assert_eq!(theirs["setup"]["copies"], "1");
        assert_eq!(theirs["options"]["gridLinesSet"], "true");
        assert_eq!(theirs["options"]["gridLines"], "false");

        // 表多的那份：每张各自一份，不是文档级共用一条
        let many = run("cell-notes-many.xlsx");
        let sheets = many["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 2, "{many}");
        for one in sheets {
            assert_eq!(one["print_setup"]["margins"]["left"], "0.75", "{one}");
            assert_eq!(one["print_setup"]["setup"], Value::Null, "{one}");
        }

        // 另两族还没读：ODF 的表不点名自己的版式（五份件全量过，那一跳文件里根本没有），
        // .xls 有 SETUP 记录但字段位没量过 —— 键整个不在，而不是空对象
        for name in ["hidden.ods", "hidden.xls", "book.xls"] {
            let out = run(name);
            for one in out["sheets"].as_array().expect("是数组") {
                assert!(
                    one.get("print_setup").is_none(),
                    "{name} 这一族没读，别交出空对象：{one}"
                );
            }
        }
    }

    /// 图那份账：三跳找到图部件，两个生产者的引用写法与缓存值各交各的。
    /// 期望值逐字来自 `.scratch/measure_chart_parts.py` 与 python 侧的镜像 `xlsx_charts()`
    #[test]
    fn charts_are_reached_through_the_drawing_and_report_only_cached_values() {
        let hand = run("chart.xlsx");
        let sheets = hand["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 2, "{sheets:?}");
        assert_eq!(sheets[0]["charts"], 2, "两张图挂在同一张表上");
        assert_eq!(sheets[1]["charts"], 0, "没有图的那张表报 0，不是缺这个键");
        assert_eq!(hand["workbook"]["totals"]["charts"], 2);

        let first = &sheets[0]["chart_list"][0];
        assert_eq!(first["part"], "xl/charts/chart1.xml", "{first}");
        assert_eq!(first["present"], json!(true));
        assert_eq!(first["cached"], json!(false), "openpyxl 一条 pt 都不缓存");
        assert_eq!(
            first["title"]["via"], "text",
            "字面标题走 `c:rich`：{first}"
        );
        assert_eq!(first["title"]["text"], "逐月收支");
        assert_eq!(first["title"]["ref"], Value::Null);
        let group = &first["groups"][0];
        assert_eq!(group["kind"], "barChart", "{group}");
        assert_eq!(group["written"]["barDir"], "col");
        assert_eq!(group["written"]["grouping"], "clustered");
        assert_eq!(group["written"]["gapWidth"], "150");
        assert_eq!(group["axis_ids"], json!(["10", "100"]));
        assert_eq!(group["series"], 2);
        let ser = &group["series_list"][0];
        assert_eq!(ser["index"], "0", "{ser}");
        assert_eq!(ser["order"], "0", "idx 与 order 各是文件自己写的那一个号");
        assert_eq!(group["series_list"][1]["index"], "1");
        assert_eq!(ser["name"]["via"], "strRef");
        assert_eq!(ser["name"]["text"], Value::Null);
        let line = &sheets[0]["chart_list"][1];
        assert_eq!(line["groups"][0]["kind"], "lineChart", "{line}");
        assert_eq!(line["groups"][0]["series"], 1);
        assert_eq!(line["title"]["text"], "收入折线");
        // 判不住的那一句要说出来：这张表的图没有缓存，图里画的是哪些数只能交引用
        assert!(
            hand["notes"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .any(|one| one.as_str().unwrap_or_default().contains("没有缓存")),
            "{:?}",
            hand["notes"]
        );

        let lo = run("chart-lo.xlsx");
        assert_eq!(lo["workbook"]["totals"]["charts"], 2);
        let one = &lo["sheets"][0]["chart_list"][0];
        assert_eq!(one["cached"], json!(true), "{one}");
        assert_eq!(
            one["title"]["text"], "逐月收支",
            "标题两家都写成字面量：{one}"
        );
        let mine = &one["groups"][0]["series_list"][0];
        assert_eq!(mine["index"], "0");
        assert_eq!(
            mine["name"]["ref"], "数据!$B$1",
            "同一段格子的第二种写法：{mine}"
        );
        assert_eq!(mine["name"]["cache"]["written"], "1");
        assert_eq!(mine["name"]["cache"]["values"], json!(["收入"]));
        assert_eq!(
            mine["cat"]["via"], "strRef",
            "类目在第二家换成了 strRef：{mine}"
        );
        assert_eq!(mine["cat"]["ref"], "数据!$A$2:$A$3");
        assert_eq!(mine["cat"]["cache"]["points"], 2);
        assert_eq!(mine["cat"]["cache"]["values"], json!(["一月", "二月"]));
        assert_eq!(mine["val"]["via"], "numRef");
        assert_eq!(mine["val"]["cache"]["written"], "2");
        assert_eq!(mine["val"]["cache"]["values"], json!([10.0, 25.0]));
        assert_eq!(mine["val"]["cache"]["whole"], json!(true));
        // 轴 id 是生产者自己编的号：两家都是两个，但值本来就不可比，所以只比个数
        assert_eq!(
            one["groups"][0]["axis_ids"]
                .as_array()
                .expect("是数组")
                .len(),
            2
        );
        // 同一批字的两份件：系列数、类目引用与图类型必须一致（引用串除外）
        let count = |one: &Value| {
            one["groups"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|had| had["series"].as_u64().unwrap_or(0))
                .collect::<Vec<u64>>()
        };
        assert_eq!(count(first), count(one), "两家喂的是同一批格子");
        let kinds = |list: &Value| {
            list.as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|had| {
                    had["groups"]
                        .as_array()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .map(|one| one["kind"].as_str().unwrap_or_default().to_string())
                        .collect::<Vec<String>>()
                })
                .collect::<Vec<Vec<String>>>()
        };
        assert_eq!(
            kinds(&sheets[0]["chart_list"]),
            kinds(&lo["sheets"][0]["chart_list"]),
            "两张图的类型与先后次序两家一致"
        );

        // 没挂图的表报 0（数过了没有）；这一族另两家的图还没读，键整个不在
        for one in run("hidden.xlsx")["sheets"].as_array().expect("是数组") {
            assert_eq!(one["charts"], 0, "没有图就说没有：{one}");
        }
        for name in ["book.ods", "hidden.xls"] {
            let out = run(name);
            for one in out["sheets"].as_array().expect("是数组") {
                assert!(
                    one.get("charts").is_none(),
                    "{name} 这一族的图还没读：{one}"
                );
            }
        }
    }

    /// 规则那两份账：条件格式的样式在 dxf 那一跳上，数据验证的开关两种拼法
    /// （期望值逐字来自 `.scratch/mk_rules_fixture.py` 之后 python 侧的 `sheet_rules()`）
    #[test]
    fn rules_report_the_dxf_hop_and_every_switch_as_written() {
        let hand = run("rules.xlsx");
        let sheet = &hand["sheets"][0]["rules"];
        assert_eq!(hand["workbook"]["dxfs"]["written"], "2", "{sheet}");
        assert_eq!(hand["workbook"]["dxfs"]["found"], 2);
        assert_eq!(hand["workbook"]["dxfs"]["whole"], json!(true));
        assert_eq!(hand["workbook"]["totals"]["conditional_rules"], 4);
        assert_eq!(hand["workbook"]["totals"]["validations"], 3);
        assert_eq!(sheet["conditional"].as_array().expect("是数组").len(), 3);

        let block = &sheet["conditional"][0];
        assert_eq!(block["sqref"], "B2:B6", "{block}");
        assert_eq!(block["rules"], 2, "一块范围里两条规则");
        let cell = &block["rule_list"][0];
        assert_eq!(cell["type"], "cellIs", "{cell}");
        assert_eq!(cell["priority"], "1");
        assert_eq!(cell["operator"], "greaterThan");
        assert_eq!(cell["formulas"], json!(["100"]));
        assert_eq!(cell["dxf"]["written"], "0");
        assert_eq!(cell["dxf"]["found"], json!(true), "dxfId 那一跳指得到东西");
        assert_eq!(cell["dxf"]["kinds"], json!(["font/b", "font/color"]));
        assert_eq!(cell["scale"], Value::Null, "cellIs 不写子形状");

        let both = &sheet["conditional"][1];
        assert_eq!(
            both["sqref"], "A2:A6 B2:B4",
            "一条 sqref 里塞两段区间：{both}"
        );
        let scale = &both["rule_list"][0];
        assert_eq!(scale["type"], "colorScale");
        assert_eq!(scale["scale"]["kind"], "colorScale");
        assert_eq!(
            scale["scale"]["colors"],
            json!(["00FFFFFF", "00FFEB84", "00F8696B"]),
            "颜色串按文件写的交，alpha 那两位不归一化"
        );
        assert_eq!(scale["scale"]["cfvo"].as_array().expect("是数组").len(), 3);
        assert_eq!(scale["dxf"]["written"], Value::Null, "色阶不指 dxf");

        let icon = &block["rule_list"][1];
        assert_eq!(icon["type"], "iconSet", "{icon}");
        assert_eq!(icon["scale"]["written"]["iconSet"], "3Arrows");
        assert_eq!(
            icon["scale"]["cfvo"][2],
            json!({"type": "percent", "val": "67"}),
            "cfvo 的 type 与 val 一起交"
        );
        let expr = &sheet["conditional"][2]["rule_list"][0];
        assert_eq!(expr["type"], "expression");
        assert_eq!(
            expr["formulas"],
            json!(["$B2>200"]),
            "那个 > 是实体：{expr}"
        );
        assert_eq!(expr["dxf"]["kinds"], json!(["font/i"]));

        let dv = &sheet["validations"];
        assert_eq!(dv["written"], "3", "容器自报的 count：{dv}");
        assert_eq!(dv["found"], 3);
        assert_eq!(dv["whole"], json!(true));
        let list = &dv["list"][0];
        assert_eq!(list["type"], "list", "{list}");
        assert_eq!(list["sqref"], "D2:D6");
        assert_eq!(
            list["operator"],
            Value::Null,
            "openpyxl 给 list 不写 operator"
        );
        assert_eq!(list["formulas"], json!(["\"红,黄,绿\""]));
        assert_eq!(list["written"]["allowBlank"], "1");
        assert_eq!(list["written"]["showDropDown"], "0");
        assert_eq!(list["written"]["prompt"], "下拉里有三个颜色");
        assert_eq!(dv["list"][2]["formulas"], json!(["=ISNUMBER(B2)"]));

        // LibreOffice 重写同一批规则：优先级是它自己排的、开关换成 true/false、
        // 给 list 补了 operator、给 custom 补了 formula2、把公式开头的 = 去掉，
        // 而且给同一条 dxf 多补了三个元素
        let lo = run("rules-lo.xlsx");
        let theirs = &lo["sheets"][0]["rules"];
        assert_eq!(lo["workbook"]["dxfs"]["found"], 2);
        let other = &theirs["conditional"][0]["rule_list"][0];
        assert_eq!(other["priority"], "2", "优先级各排各的：{other}");
        assert_eq!(
            other["dxf"]["kinds"].as_array().expect("是数组").len(),
            5,
            "多出的 name / family / sz 是它自己补的：{other}"
        );
        assert_eq!(
            other["written"]["aboveAverage"], "0",
            "等于默认的开关它也写：{other}"
        );
        let one_list = &theirs["validations"]["list"][0];
        assert_eq!(one_list["operator"], "equal", "list 也被补了 operator");
        assert_eq!(
            one_list["written"]["allowBlank"], "true",
            "同一条开关的第二种拼法：{one_list}"
        );
        assert_eq!(one_list["written"]["errorStyle"], "stop");
        assert_eq!(
            one_list["formulas"],
            json!(["\"红,黄,绿\"", "0"]),
            "formula2 是它补的默认值"
        );
        assert_eq!(
            theirs["validations"]["list"][2]["formulas"],
            json!(["ISNUMBER(B2)", "0"])
        );
        assert_eq!(
            theirs["conditional"][1]["rule_list"][0]["scale"]["colors"],
            json!(["FFFFFFFF", "FFFFEB84", "FFF8696B"]),
            "同一种色，alpha 那两位两家写法不同"
        );

        // 第二张表两样都没有：报 0 与「没写」，而不是缺键
        let clean = &hand["sheets"][1]["rules"];
        assert_eq!(
            clean["conditional"].as_array().expect("是数组").len(),
            0,
            "{clean}"
        );
        assert_eq!(clean["validations"]["written"], Value::Null);
        assert_eq!(clean["validations"]["found"], 0);
        assert_eq!(clean["validations"]["whole"], json!(true));
    }

    /// 不是表格的文件要指路
    #[test]
    fn a_word_document_is_not_a_sheet() {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
            limit: 10,
            max_bytes: 1 << 20,
            csv: false,
            sheet: String::new(),
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_sheet(&app, &Context::new_test(tx)).unwrap_err();
        assert!(why.to_string().contains("office-doc"), "{why}");
    }
}
