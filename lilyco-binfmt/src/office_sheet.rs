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
    about = "Report a spreadsheet's layout: every sheet with its workbook-order index, sheetId, relationship target, r:id and visibility (hidden and very-hidden sheets are listed, not skipped - they are usually the ones worth knowing about), each sheet's self-declared dimension, and per sheet the cell count, formula count, numeric/shared/inline-string split, merged ranges, hidden rows and columns. Also reports defined names (with what they point at), table parts (names, ranges, header rows), external-link workbook parts, chart and picture parts, styles/conditional formatting presence, and whether a calcChain exists. Shared strings are resolved so LABELSST cells carry their text; a formula cell reports the formula and says whether the file also cached a result (openpyxl-written files do not, and inventing a value there is exactly what this command refuses to do). Each cell also carries its number format: the style index on the cell is a row of xl/styles.xml cellXfs (not a format id), so a date is only a date once that hop is taken - the format code and, for date/time-formatted numeric cells, the ISO reading of the serial number are reported, honouring workbook.xml date1904 and reporting Excel's non-existent 1900-02-29 as written. A text cell like "12/23/2013" stays text. Legacy .xls goes through the BIFF8 record reader. ODF spreadsheets (.ods) are read on their own terms: cells carry an explicit value-type with office:value / date-value / boolean-value (no serial-number epoch to guess), positions are accumulated through table:number-columns-repeated runs (which routinely stand for 16000+ empty columns and are not counted), covered cells are tallied apart from content, merges come from the span attributes, a sheet's visibility is resolved through the automatic style it names, and hidden rows/columns are counted from table:visibility="collapse" on the element or in the row/column style it names (multiplying number-columns-repeated, so one element standing for three collapsed columns reports 3, not 1); each ODS cell additionally carries the number format it inherits - cell style, then style:data-style-name, then that number:*-style element (which lives in content.xml or styles.xml, and is reached through parent-style-name when the cell style itself names none) - reported as format_kind (taken from the element's own name, so a ¥ written as a literal text token stays a number-style), plus decimals, currency_symbol and a faithful format_tokens transcription; ODF has no format string, so none is invented. With --csv it also renders one sheet (by name, or by the 0-based index this command reports; --sheet picks it, default first) as RFC4180 CSV under { csv: {sheet, index, rows, columns, cells_skipped, line_end, text} } - date cells go out as the ISO reading of the serial number (for legacy .xls there is no style hop yet, so a date goes out as the serial and a note says so), a formula cell with no cached result goes out empty rather than guessed, holes are empty fields, and cells whose reference cannot be parsed as A1 are left out. Returns { path, format, sheets, csv, workbook, defined_names, tables, external_links, parts, notes }."
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
        let mut totals = json!({
            "cells": 0, "formulas": 0, "numeric": 0, "shared_strings": 0,
            "inline_strings": 0, "merged": 0, "hidden_rows": 0, "hidden_cols": 0,
            "dates": 0,
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
            let mut entry = json!({
                "index": index,
                "name": name,
                "sheet_id": one.attr("sheetId"),
                "state": state,
                "r_id": rid,
                "part": part,
            });
            match xml(bytes, &part) {
                Some(member) => {
                    let sheet_root = xmlscan::parse_str(&member.as_text());
                    let dimension = sheet_root
                        .descendants("dimension")
                        .first()
                        .and_then(|one| one.attr("ref"))
                        .unwrap_or_default()
                        .to_string();
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
                    entry["listed"] = json!(cells.len());
                    entry["formulas"] = json!(formulas);
                    entry["numeric"] = json!(numeric);
                    entry["shared_string_cells"] = json!(shared_count);
                    entry["inline_strings"] = json!(inline);
                    entry["date_cells"] = json!(dates);
                    entry["formula_cells_with_cached_value"] = json!(cached);
                    entry["merged"] = json!(sheet_root.descendants("mergeCell").len());
                    entry["hidden_rows"] = json!(sheet_root
                        .descendants("row")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .count());
                    entry["hidden_cols"] = json!(sheet_root
                        .descendants("col")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .map(|one| col_span(one))
                        .sum::<usize>());
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
                "styles_part": xml(bytes, "xl/styles.xml").is_some(),
                "tables": doc.entries.iter().filter(|one| one.name.starts_with("xl/tables/")).count(),
                "charts": doc.entries.iter().filter(|one| one.name.starts_with("xl/charts/")).count(),
                "media": doc.entries.iter().filter(|one| one.name.starts_with("xl/media/")).count(),
                "totals": totals,
            },
            "sheets": sheets,
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
        let styles = crate::odstyle::read(bytes);
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
            "hidden_rows": 0, "hidden_cols": 0,
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
        if app.csv {
            // 这条路上没有「查 cellXfs 拿格式码」那一步：日期格只会给序列数
            notes.push(
                "CSV 里 .xls 的日期格给的是序列数：BIFF 这一支还没解样式，\
                 不替它换算成 ISO"
                    .to_string(),
            );
        }
        let grid_names: Vec<String> = book.sheets.iter().map(|one| one.name.clone()).collect();
        let grids: Vec<Vec<(usize, usize, String)>> = book
            .sheets
            .iter()
            .map(|sheet| {
                book.cells
                    .iter()
                    .filter(|had| had.sheet.as_deref() == Some(sheet.name.as_str()))
                    .filter_map(|had| {
                        split_ref(&had.reference()).map(|(row, col)| {
                            (
                                row,
                                col,
                                had.text.clone().unwrap_or_else(|| {
                                    had.number.map(|one| format!("{one}")).unwrap_or_default()
                                }),
                            )
                        })
                    })
                    .collect()
            })
            .collect();
        let cells: Vec<Value> = book
            .cells
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "ref": one.reference(),
                    "kind": one.kind,
                    "sheet": one.sheet,
                    "text": one.text,
                    "number": one.number,
                })
            })
            .collect();
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
                "totals": {"cells": book.cells.len(), "formulas": book.formula_cells},
            },
            "sheets": book.sheets.iter().map(|one| json!({
                "name": one.name,
                "state": one.state,
                "record_start": one.record_start,
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

    /// 隐藏的行与列有三种写法：openpyxl 一列一条 `hidden="1"`、LibreOffice 把连续
    /// 三列并成一条 `min="3" max="5" hidden="true"`、ODF 用
    /// `table:visibility="collapse"` 压在一整条 `number-columns-repeated="3"` 上。
    /// 三种存法必须报出同一个数
    #[test]
    fn hidden_rows_and_columns_are_counted_whichever_way_they_are_written() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods"] {
            let out = run(name);
            let sheet = &out["sheets"][0];
            assert_eq!(sheet["name"], "预算表", "{name}");
            assert_eq!(sheet["hidden_rows"], 2, "{name}：{sheet}");
            assert_eq!(sheet["hidden_cols"], 3, "{name}：{sheet}");
            assert_eq!(sheet["cells"], 13, "藏起来的格子还是格子：{name}");
        }
    }

    /// 隐藏只是「看不见」，不是「没有」：那些字要照样出现在 CSV 里
    #[test]
    fn hidden_columns_still_carry_their_text() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods"] {
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
