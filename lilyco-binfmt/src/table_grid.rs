//! 表格里那一张网：按「这张表自己的行与格子」交出来，嵌套的表不算在这一张里。
//!
//! 这与 `structure.table_rows` / `table_cells` 那两本账是两回事：那两个数用 `descendants`
//! 数（嵌套表的格子会一起算进来，那本来就是「这份文件里一共有几个行/格标记」的问法），
//! 这里走的是**直接孩子** —— 一张表有几行、每一行有几个格子、哪一格被合并掉了，
//! 只有按直接孩子走才看得出来。
//!
//! 两家把「横向合并」写得不一样，同一张视觉上 2×3 的表，格子数就跟着不一样：
//! - OOXML 把合掉的那一格**整个不写**，只在留下的那一格上写 `w:gridSpan="2"` → 那一行 2 个格；
//! - ODF 把被盖住的那一格照样写出来（一个字没有的 `covered-table-cell`），同时在第一格上写
//!   `number-columns-spanned="2"` → 同一行 3 个格。
//! 所以「这一行几个格子」是**存储的数**，不是页面上那张表的数。出处是 fixture 里的
//! `tables-merged.docx` 与 LibreOffice 转出的 `tables-merged.odt`（同一批字的两副面孔）。
//!
//! 一格的字 = 它自己那几个段用换行拼起来（嵌在格子里的那张表的段不算这一格）。
//! 合并与重复的数只交文件写了的：没写就是 null，不是 1 ——
//! 「文件没说」与「文件说的就是一格」是两件事。

use serde_json::{json, Value};

use crate::xmlscan::Node;

/// 一张表的网格。`rows` 每一行是一格一格的数组（被 `limit` 截断时后面的行/格不出现在这里），
/// `cut` 说这次交出来的是不是被截过的
#[derive(Debug, Clone)]
pub struct Grid {
    pub rows: Vec<Value>,
    pub cut: bool,
}

impl Grid {
    pub fn to_json(&self) -> Value {
        json!({"rows": self.rows, "cut": self.cut})
    }
}

/// 属性里的数字；写了但不是数就 null（不替文件猜一个）
fn number(raw: Option<&str>) -> Option<i64> {
    raw.and_then(|one| one.trim().parse::<i64>().ok())
}

fn text_of(parts: Vec<String>) -> String {
    parts.join("\n").trim().to_string()
}

/// OOXML：`w:tr` → `w:tc`。横向合并写在 `w:tcPr/w:gridSpan/@w:val`，
/// 纵向合并写在 `w:tcPr/w:vMerge` —— 那个值可以整个不写，OOXML 里「不写」就是
/// 「接上面那一格」，所以这里补成字符串 `continue`（这是文件的规矩，不是我们的推断）
pub fn ooxml(table: &Node, limit: usize) -> Grid {
    let trs = table.all("tr");
    let mut cut = trs.len() > limit;
    let mut rows: Vec<Value> = Vec::new();
    for tr in trs.into_iter().take(limit) {
        let tcs = tr.all("tc");
        cut |= tcs.len() > limit;
        let cells: Vec<Value> = tcs
            .into_iter()
            .take(limit)
            .map(|tc| {
                let props = tc.child("tcPr");
                let span = props
                    .and_then(|one| one.child("gridSpan"))
                    .and_then(|one| number(one.attr_local("val")));
                let merge = props
                    .and_then(|one| one.child("vMerge"))
                    .map(|one| one.attr_local("val").unwrap_or("continue").to_string());
                json!({
                    "text": text_of(
                        tc.all("p")
                            .into_iter()
                            .map(|one| crate::office_text::paragraph_text(one))
                            .collect()
                    ),
                    "col_span": span,
                    "row_span": Value::Null,
                    "repeat": Value::Null,
                    "row_merge": merge,
                    // OOXML 没有「被盖住的格子」这种元素（合掉的那一格根本不在文件里），
                    // 所以这里每一格都是 false —— 与 ODF 那边同一个形状才好两边比
                    "covered": json!(false),
                    "paragraphs": tc.all("p").len(),
                })
            })
            .collect();
        rows.push(Value::Array(cells));
    }
    Grid { rows, cut }
}

/// ODF：`table:table-row` → `table:table-cell` 与 `table:covered-table-cell`（按文件写的顺序）。
/// 跨列 / 跨行 / 重复各有自己的属性，没写的整个不存在
pub fn odf(table: &Node, limit: usize) -> Grid {
    let trs: Vec<&Node> = table
        .children
        .iter()
        .filter(|one| one.is("table-row"))
        .collect();
    let mut cut = trs.len() > limit;
    let mut rows: Vec<Value> = Vec::new();
    for tr in trs.into_iter().take(limit) {
        let tcs: Vec<&Node> = tr
            .children
            .iter()
            .filter(|one| one.is("table-cell") || one.is("covered-table-cell"))
            .collect();
        cut |= tcs.len() > limit;
        let cells: Vec<Value> = tcs
            .into_iter()
            .map(|tc| {
                let parts: Vec<String> = tc
                    .children
                    .iter()
                    .filter(|one| one.is("p") || one.is("h"))
                    .map(|one| crate::office_text::odf_paragraph_text(one))
                    .collect();
                json!({
                    "text": text_of(parts),
                    "col_span": number(tc.attr_local("number-columns-spanned")),
                    "row_span": number(tc.attr_local("number-rows-spanned")),
                    "repeat": number(tc.attr_local("number-columns-repeated")),
                    "row_merge": Value::Null,
                    "covered": tc.is("covered-table-cell"),
                    "paragraphs": tc
                        .children
                        .iter()
                        .filter(|one| one.is("p") || one.is("h"))
                        .count(),
                })
            })
            .collect();
        rows.push(Value::Array(cells));
    }
    Grid { rows, cut }
}

/// `--csv`：把这张表铺成 RFC4180 的 CSV 交出去
///
/// 一行就是文件自己写着的几格，**不补成方格**：OOXML 把横向合并那一格整个不写，
/// ODF 照样写一枚空的 `covered-table-cell` —— 同一张视觉上 2×3 的表，首行在这两家
/// 分别是 2 格与 3 格，所以整张表的 `columns_per_row` 是 `[2, 3]` 与 `[3, 3]`。
/// `columns_per_row` 与 `ragged` 因此一起交，`covered_cells` 只数 ODF 那种被盖住的占位格。
/// 一格里的几个段用 `\n` 连着（就是 `text_of` 的拼法），进了 CSV 按 RFC4180 加引号 ——
/// 引法与 `office-sheet` 那本共用同一个 `csv_field`。
pub(crate) fn csv_of(want: bool, grids: &[Grid], pick: &str) -> Value {
    if !want {
        return Value::Null;
    }
    let index = if pick.trim().is_empty() {
        0usize
    } else {
        match pick.trim().parse::<usize>() {
            Ok(raw) => raw,
            Err(_) => {
                return json!({"error": format!("--table 要的是从 0 起的序号，收到「{}」", pick)})
            }
        }
    };
    let Some(grid) = grids.get(index) else {
        return json!({
            "error": format!("这份文件里没有第 {} 张表（一共 {} 张）", index, grids.len()),
        });
    };
    let mut out = String::new();
    let mut widths: Vec<usize> = Vec::new();
    let mut empty_cells = 0usize;
    let mut covered_cells = 0usize;
    for line in &grid.rows {
        let cells = line.as_array().cloned().unwrap_or_default();
        let fields: Vec<String> = cells
            .iter()
            .map(|one| {
                if one["covered"] == json!(true) {
                    covered_cells += 1;
                }
                if one["text"].as_str().unwrap_or_default().is_empty() {
                    empty_cells += 1;
                }
                one["text"].as_str().unwrap_or_default().to_string()
            })
            .collect();
        widths.push(fields.len());
        out.push_str(
            &fields
                .iter()
                .map(|one| crate::office_sheet::csv_field(one.as_str()))
                .collect::<Vec<String>>()
                .join(","),
        );
        out.push('\n');
    }
    let columns = widths.iter().max().copied().unwrap_or(0);
    json!({
        "table": index,
        "tables_total": grids.len(),
        "rows": widths.len(),
        "columns": columns,
        "columns_per_row": widths,
        // 有几行与最宽那行不一样长就是 true；一行也没有时是 false（数过了）
        "ragged": json!(widths.iter().any(|one| *one != columns)),
        "empty_cells": empty_cells,
        "covered_cells": covered_cells,
        "cut": grid.cut,
        "line_end": "LF",
        "text": out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn member(bytes: &[u8], name: &str) -> Node {
        let one = crate::zipread::member(bytes, name, crate::zipread::DEFAULT_MEMBER_CAP)
            .expect("部件读得出");
        crate::xmlscan::parse_str(&one.as_text())
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    fn docx_tables(name: &str) -> Vec<Grid> {
        let bytes = fixture(name);
        let root = member(&bytes, "word/document.xml");
        let body = root
            .child("document")
            .and_then(|one| one.child("body"))
            .expect("有 body");
        body.all("tbl")
            .into_iter()
            .map(|one| ooxml(one, 100))
            .collect()
    }

    fn odt_tables(name: &str) -> Vec<Grid> {
        let bytes = fixture(name);
        let root = member(&bytes, "content.xml");
        root.descendants("table")
            .into_iter()
            .map(|one| odf(one, 100))
            .collect()
    }

    fn cell(row: &Value, index: usize) -> Value {
        row.as_array().expect("一行")[index].clone()
    }

    /// 没有合并的那两份件：两家的网格一模一样（字与行列都对得上）
    #[test]
    fn tables_without_merges_read_the_same_in_both_families() {
        let docx = docx_tables("tables.docx");
        let odt = odt_tables("tables.odt");
        assert_eq!(docx.len(), 2, "{:?}", docx);
        assert_eq!(odt.len(), 2, "{:?}", odt);
        for (a, b) in docx.iter().zip(odt.iter()) {
            assert_eq!(a.rows, b.rows, "两家同一张表的网格必须一样");
            assert!(!a.cut && !b.cut, "{:?}", a.rows);
        }
        assert_eq!(docx[0].rows.len(), 3, "{:?}", docx[0].rows);
        assert_eq!(
            cell(&docx[0].rows[0], 0)["text"],
            "R0C0",
            "{:?}",
            docx[0].rows
        );
        assert_eq!(
            cell(&docx[0].rows[0], 0)["col_span"],
            Value::Null,
            "没写就是 null"
        );
        assert_eq!(docx[1].rows.len(), 2, "{:?}", docx[1].rows);
    }

    /// 合并格那一份：同一张 2×3 的表，OOXML 那一行写 2 个格、ODF 写 3 个格。
    /// 这不是谁读错了，是两家的存法不同 —— 网格按各自的文件交，不做调和
    #[test]
    fn a_horizontal_merge_is_written_differently_by_the_two_families() {
        let docx = docx_tables("tables-merged.docx");
        let odt = odt_tables("tables-merged.odt");
        // 表一第一行：合掉的那一格在 OOXML 里根本不存在
        assert_eq!(
            docx[0].rows[0].as_array().expect("一行").len(),
            2,
            "{:?}",
            docx[0].rows
        );
        assert_eq!(
            cell(&docx[0].rows[0], 0)["text"],
            "跨两列",
            "{:?}",
            docx[0].rows
        );
        assert_eq!(
            cell(&docx[0].rows[0], 0)["col_span"],
            json!(2),
            "gridSpan=2"
        );
        assert_eq!(
            cell(&docx[0].rows[0], 1)["text"],
            "第三列",
            "{:?}",
            docx[0].rows
        );
        // 同一行在 ODF 里是三个格：中间那个是被盖住的空格子
        assert_eq!(
            odt[0].rows[0].as_array().expect("一行").len(),
            3,
            "{:?}",
            odt[0].rows
        );
        assert_eq!(
            cell(&odt[0].rows[0], 0)["col_span"],
            json!(2),
            "{:?}",
            odt[0].rows
        );
        // 第二行两家都是三个格，字也一模一样
        assert_eq!(
            docx[0].rows[1], odt[0].rows[1],
            "{:?} vs {:?}",
            docx[0].rows[1], odt[0].rows[1]
        );
    }

    /// 纵向合并：OOXML 在下一格上写 `vMerge`（没写值 = continue），
    /// 那一格照样存在、字是空的；两家都是两行两格
    #[test]
    fn a_vertical_merge_keeps_the_cell_and_says_which_end_starts() {
        let docx = docx_tables("tables-merged.docx");
        let odt = odt_tables("tables-merged.odt");
        assert_eq!(
            cell(&docx[1].rows[0], 0)["text"],
            "跨两行",
            "{:?}",
            docx[1].rows
        );
        assert_eq!(
            cell(&docx[1].rows[0], 0)["row_merge"],
            json!("restart"),
            "起头那一格：{:?}",
            docx[1].rows
        );
        assert_eq!(
            cell(&docx[1].rows[1], 0)["row_merge"],
            json!("continue"),
            "接上去的那一格照样在：{:?}",
            docx[1].rows
        );
        assert_eq!(cell(&docx[1].rows[1], 0)["text"], "", "它一个字也没有");
        assert_eq!(docx[1].rows.len(), 2, "{:?}", docx[1].rows);
        assert_eq!(odt[1].rows.len(), 2, "{:?}", odt[1].rows);
        assert_eq!(
            cell(&odt[1].rows[0], 0)["row_span"],
            json!(2),
            "ODF 直接写跨了几行：{:?}",
            odt[1].rows
        );
        assert_eq!(
            cell(&odt[1].rows[1], 0)["row_span"],
            Value::Null,
            "{:?}",
            odt[1].rows
        );
    }

    /// 一张表里的段数：一格可以有几个段，嵌套在格子里的表的段不算这一格
    #[test]
    fn a_cell_counts_its_own_paragraphs() {
        let docx = docx_tables("notes.docx");
        assert_eq!(docx.len(), 1, "{:?}", docx);
        assert_eq!(
            cell(&docx[0].rows[0], 0)["text"],
            "科目",
            "{:?}",
            docx[0].rows
        );
        assert_eq!(
            cell(&docx[0].rows[0], 0)["paragraphs"],
            1,
            "{:?}",
            docx[0].rows
        );
        assert_eq!(
            cell(&docx[0].rows[1], 1)["text"],
            "124000",
            "{:?}",
            docx[0].rows
        );
    }

    /// `limit` 截断要看得见：只交一行时，`cut` 必须说「后面还有」
    #[test]
    fn a_cut_grid_says_so() {
        let bytes = fixture("tables.docx");
        let root = member(&bytes, "word/document.xml");
        let body = root
            .child("document")
            .and_then(|one| one.child("body"))
            .expect("有 body");
        let tbl = body.all("tbl").first().expect("有表").clone();
        let one = ooxml(tbl, 1);
        assert_eq!(one.rows.len(), 1, "{:?}", one.rows);
        assert!(one.cut, "三行只交了一行，cut 要为真：{:?}", one.rows);
        let all = ooxml(tbl, 100);
        assert!(!all.cut, "全交出来就不该说截过：{:?}", all.rows);
    }
}
