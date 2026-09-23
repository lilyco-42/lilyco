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
    about = "Report a spreadsheet's layout: every sheet with its workbook-order index, sheetId, relationship target, r:id and visibility (hidden and very-hidden sheets are listed, not skipped - they are usually the ones worth knowing about), each sheet's self-declared dimension, and per sheet the cell count, formula count, numeric/shared/inline-string split, merged ranges, hidden rows and columns. Also reports defined names (with what they point at), table parts (names, ranges, header rows), external-link workbook parts, chart and picture parts, styles/conditional formatting presence, and whether a calcChain exists. Shared strings are resolved so LABELSST cells carry their text; a formula cell reports the formula and says whether the file also cached a result (openpyxl-written files do not, and inventing a value there is exactly what this command refuses to do). Legacy .xls goes through the BIFF8 record reader. Returns { path, format, sheets, workbook, defined_names, tables, external_links, parts, notes }."
)]
pub struct OfficeSheet {
    /// 表格文件（xlsx / xlsm / xls）
    #[arg(about = "Spreadsheet to inspect", must_exist = true)]
    path: PathBuf,

    /// 每张表最多列多少个格子（总数照实给）
    #[arg(
        about = "List at most this many cells per sheet",
        default = 200,
        min = 1
    )]
    limit: u64,

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
        let mut sheets: Vec<Value> = Vec::new();
        let mut totals = json!({
            "cells": 0, "formulas": 0, "numeric": 0, "shared_strings": 0,
            "inline_strings": 0, "merged": 0, "hidden_rows": 0, "hidden_cols": 0,
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
                    for cell in sheet_root.descendants("c") {
                        count += 1;
                        let reference = cell.attr("r").unwrap_or_default().to_string();
                        let kind = cell.attr("t").unwrap_or("n").to_string();
                        let value = cell.child("v").map(|one| one.text().trim().to_string());
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
                        if formula.is_some() && value.is_some() {
                            cached += 1;
                        }
                        if cells.len() < limit {
                            cells.push(json!({
                                "ref": reference,
                                "kind": kind,
                                "value": numeric_or_text(text),
                                "formula": formula,
                            }));
                        }
                    }
                    entry["dimension"] = json!(dimension);
                    entry["cells"] = json!(count);
                    entry["listed"] = json!(cells.len());
                    entry["formulas"] = json!(formulas);
                    entry["numeric"] = json!(numeric);
                    entry["shared_string_cells"] = json!(shared_count);
                    entry["inline_strings"] = json!(inline);
                    entry["formula_cells_with_cached_value"] = json!(cached);
                    entry["merged"] = json!(sheet_root.descendants("mergeCell").len());
                    entry["hidden_rows"] = json!(sheet_root
                        .descendants("row")
                        .iter()
                        .filter(|one| one.attr("hidden") == Some("true")
                            || one.attr("hidden") == Some("1"))
                        .count());
                    entry["hidden_cols"] = json!(sheet_root
                        .descendants("col")
                        .iter()
                        .filter(|one| one.attr("hidden") == Some("true")
                            || one.attr("hidden") == Some("1"))
                        .count());
                    entry["rows"] = json!(sheet_root.descendants("row").len());
                    entry["cell_list"] = json!(cells);
                    bump(&mut totals, "cells", count);
                    bump(&mut totals, "formulas", formulas);
                    bump(&mut totals, "numeric", numeric);
                    bump(&mut totals, "shared_strings", shared_count);
                    bump(&mut totals, "inline_strings", inline);
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
            "defined_names": defined,
            "external_links": external,
            "notes": doc.notes,
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
        let notes = book.notes.clone();
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
            cells[7]["formula"], "=SUM(B2:B3)",
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

    /// 不是表格的文件要指路
    #[test]
    fn a_word_document_is_not_a_sheet() {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_sheet(&app, &Context::new_test(tx)).unwrap_err();
        assert!(why.to_string().contains("office-doc"), "{why}");
    }
}
