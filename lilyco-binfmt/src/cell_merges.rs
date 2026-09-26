//! 电子表格里「哪几格并成了一块」那一份账（`structure.merges` 的两种家族）。
//!
//! 老的 `merged` 只是一枚数：OOXML 数 `<mergeCell>` 的条数，ODF 数「有字且跨了行或列的格子」。
//! 那枚数回答不了日常的两个问题 —— **哪一块**被并了，以及**这块底下有没有字**。
//! 这一本把每一条区间都摊开，并且去对文件自己声明的那个 `count`。
//!
//! 两家的写法不是一回事，所以入口分两个，最后的账合成一份形状：
//! * OOXML：`<mergeCell ref="A1:D1"/>`，区间只在这个串里，`ref` 可以**没有冒号**
//!   （`ref="A12"` 是单格「合并」，是合法存法），也可以带 `$`；
//! * ODF：没有区间串。跨度写在格子自己的两个属性上
//!   （`table:number-columns-spanned` / `table:number-rows-spanned`），区间要从锚点加出来。
//!
//! 实测的三件事（三份 `merges*` 件，见 fixture README）：同一条区间写两遍 openpyxl 会自己去重、
//! 互相盖住的两条它照写、而 LibreOffice 重写同一份时把单格那条与重叠里较小的一条**一起丢掉**。

use serde_json::{json, Value};

/// 一条区间进账本前的公共形状：两家都先解成这个，再合成同一份账
#[derive(Debug, Clone)]
struct Row {
    /// OOXML 写着的那枚 `ref` 原样串（没写就是空串）；ODF 没有这种串，交 null
    written: Option<String>,
    /// 锚点与止点（0-based 行、列），解不出来时两者为 None
    span: Option<((usize, usize), (usize, usize))>,
    /// 锚点格里有没有字（两家的口径见 `ledger` 的注释）
    has_text: bool,
}

/// 把 `(行, 列)`（0-based）写成日常看到的那个地址：`A3`、`B7`
pub fn address(row: usize, col: usize) -> String {
    let mut letters = String::new();
    let mut at = col;
    loop {
        letters.insert(0, (b'A' + (at % 26) as u8) as char);
        if at < 26 {
            break;
        }
        at = at / 26 - 1;
    }
    format!("{}{}", letters, row + 1)
}

/// `A1` / `$A$1` / `a1` → 地址原样串去掉 `$` 与大写化之后的规范形；解不动交 None
fn normalize(raw: &str) -> Option<String> {
    let (row, col) = crate::office_sheet::split_ref(&raw.replace('$', ""))?;
    Some(address(row, col))
}

/// OOXML 那一条区间：`ref` 的原样串 + 这一格里有没有字（按锚点查 `filled`）
fn ooxml_row(raw: &str, filled: &[String]) -> Row {
    let clean = raw.replace('$', "");
    let head = clean.split(':').next().unwrap_or_default();
    let tail = clean.split(':').nth(1);
    let start = crate::office_sheet::split_ref(head);
    let stop = tail.as_deref().map(crate::office_sheet::split_ref);
    Row {
        written: Some(raw.to_string()),
        span: match (start, stop) {
            (Some(a), None) => Some((a, a)),
            (Some(a), Some(Some(b))) => Some((a, b)),
            _ => None,
        },
        has_text: normalize(head).is_some_and(|one| filled.iter().any(|had| had == &one)),
    }
}

/// ODF 那一条区间：锚点地址 + 行跨与列跨（跨度按写的数加出来，不做换算）
fn odf_row(anchor: &str, rows: usize, cols: usize, has_text: bool) -> Row {
    let start = crate::office_sheet::split_ref(anchor);
    Row {
        written: None,
        span: start.map(|(r, c)| {
            (
                (r, c),
                (r + rows.saturating_sub(1), c + cols.saturating_sub(1)),
            )
        }),
        has_text,
    }
}

/// OOXML：`refs` 是 `<mergeCell ref>` 的原样串（按文档顺序），`filled` 是有字的格的规范地址，
/// `declared` 是 `<mergeCells count>` 写的那个数（没写交 None）
pub fn ooxml(refs: &[String], filled: &[String], declared: Option<usize>, limit: usize) -> Value {
    ledger(
        refs.iter().map(|one| ooxml_row(one, filled)).collect(),
        declared,
        limit,
    )
}

/// ODF：`spans` 是 `(锚点地址, 行跨, 列跨, 有没有字)`，按文档顺序
pub fn odf(spans: &[(String, usize, usize, bool)], limit: usize) -> Value {
    ledger(
        spans
            .iter()
            .map(|(at, rows, cols, had)| odf_row(at, *rows, *cols, *had))
            .collect(),
        None,
        limit,
    )
}

/// 两家共用的那本账。三条口径在两边的算法一模一样：
/// * **有没有字**：OOXML 看这个 sheet 里有没有一枚 `<c r=锚点>` 带着内容（合并块底下
///   可以一个字都不写，也可以整格都没有）；ODF 看那枚格子自己有没有内容 ——
///   与老的 `merged` 那条「只数有字的合并格」不同，这一本把空锚点也数进来；
/// * **重叠**：按文档顺序，凡与在它之前某一条盖到同一格的就算重叠（只数后来那一条，
///   所以一对重叠是 1 不是 2）。Excel 不让你存成那样，别的写手会。
///   解开的区间都拿去做判定，包括本身重叠的那一条，所以三条互相盖住的是 2；
/// * **反着写**（`ref="D1:A1"` 这种止点在锚点之前的）：几何按 min/max 摊平照报，
///   另给一枚 `reversed: true`，但不进任何几何合计（覆盖格数、单格数、有字锚点数、
///   去重后的条数），因为那一份矩形在它自己那条记录里已经数过一次了。
///   它照旧参加重叠判定（那确实是两块盖在了一起），也因此会落进 `duplicated`
///   （那枚数是 `total - bad_refs - distinct`，而它进不了 `distinct`）—— 两本各说各的。
///   解析不动的（`bad_ref: true`）只留原样串与那枚有无字，几何全交 null。
fn ledger(rows: Vec<Row>, declared: Option<usize>, limit: usize) -> Value {
    let total = rows.len();
    let mut listed: Vec<Value> = Vec::new();
    let mut seen: Vec<((usize, usize), (usize, usize))> = Vec::new();
    let mut distinct_names: Vec<String> = Vec::new();
    let mut covered_cells = 0usize;
    let mut solo = 0usize;
    let mut with_text = 0usize;
    let mut overlapping = 0usize;
    let mut bad = 0usize;
    for (index, one) in rows.iter().enumerate() {
        let Some((start, stop)) = one.span else {
            bad += 1;
            if listed.len() < limit {
                listed.push(json!({
                    "index": index,
                    "written": one.written.clone(),
                    "anchor": Value::Null,
                    "end": Value::Null,
                    "rows": Value::Null,
                    "cols": Value::Null,
                    "cells": Value::Null,
                    "covered": Value::Null,
                    "solo": Value::Null,
                    "anchor_has_text": one.has_text,
                    "overlaps_earlier": false,
                    "reversed": Value::Null,
                    "bad_ref": true,
                }));
            }
            continue;
        };
        let (lo, hi) = (
            (start.0.min(stop.0), start.1.min(stop.1)),
            (start.0.max(stop.0), start.1.max(stop.1)),
        );
        let (rows_span, cols_span) = (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1);
        let cells = rows_span * cols_span;
        let flips = start.0 > stop.0 || start.1 > stop.1;
        let hits = seen.iter().any(|(was_start, was_stop)| {
            covers(*was_start, *was_stop, lo)
                || covers(*was_start, *was_stop, hi)
                || covers(lo, hi, *was_start)
                || covers(lo, hi, *was_stop)
        });
        if hits {
            overlapping += 1;
        }
        if !flips {
            seen.push((lo, hi));
            covered_cells += cells - 1;
            if rows_span == 1 && cols_span == 1 {
                solo += 1;
            }
            if one.has_text {
                with_text += 1;
            }
            let key = format!("{}:{}", address(lo.0, lo.1), address(hi.0, hi.1));
            if !distinct_names.iter().any(|had| had == &key) {
                distinct_names.push(key);
            }
        }
        if listed.len() < limit {
            listed.push(json!({
                "index": index,
                "written": one.written.clone(),
                "anchor": address(start.0, start.1),
                "end": address(stop.0, stop.1),
                "rows": rows_span,
                "cols": cols_span,
                "cells": cells,
                "covered": cells - 1,
                "solo": rows_span == 1 && cols_span == 1,
                "anchor_has_text": one.has_text,
                "overlaps_earlier": hits,
                "reversed": flips,
                "bad_ref": false,
            }));
        }
    }
    json!({
        "total": total,
        "listed": listed.len(),
        "cut": total > limit,
        "declared": declared,
        "declared_matches": declared.map(|want| want == total),
        "distinct": distinct_names.len(),
        "duplicated": total - bad - distinct_names.len(),
        "overlapping": overlapping,
        "solo": solo,
        "covered_cells": covered_cells,
        "anchors_with_text": with_text,
        "bad_refs": bad,
        "rows": listed,
    })
}

/// `(from, to)` 这块矩形里有没有 `(at)` 这一格
fn covers(from: (usize, usize), to: (usize, usize), at: (usize, usize)) -> bool {
    at.0 >= from.0 && at.0 <= to.0 && at.1 >= from.1 && at.1 <= to.1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手搓输入走 OOXML 那一支（生产者是做不出这些形状的：`$`、无冒号、反着写、
    /// 解不动的四样都塞在同一本账里），limit 用 400 与影子测量对齐
    fn by_ooxml(refs: &[&str], filled: &[&str], declared: Option<usize>) -> Value {
        ooxml(
            &refs
                .iter()
                .map(|one| one.to_string())
                .collect::<Vec<String>>(),
            &filled
                .iter()
                .map(|one| one.to_string())
                .collect::<Vec<String>>(),
            declared,
            400,
        )
    }

    /// 那本账的十二个合计，固定这个顺序好一次看全（缺键与零条是两件事）
    fn agg(ledger: &Value) -> Value {
        Value::Array(
            [
                "total",
                "listed",
                "cut",
                "declared",
                "declared_matches",
                "distinct",
                "duplicated",
                "overlapping",
                "solo",
                "covered_cells",
                "anchors_with_text",
                "bad_refs",
            ]
            .iter()
            .map(|key| ledger.get(*key).cloned().unwrap_or(Value::Null))
            .collect::<Vec<Value>>(),
        )
    }

    /// 一行账里那十三样，按「写着的串 → 两端 → 几何 → 三条判定」的顺序
    fn row(ledger: &Value, at: usize) -> Value {
        let one = &ledger["rows"][at];
        Value::Array(
            [
                "index",
                "written",
                "anchor",
                "end",
                "rows",
                "cols",
                "cells",
                "covered",
                "solo",
                "anchor_has_text",
                "overlaps_earlier",
                "reversed",
                "bad_ref",
            ]
            .iter()
            .map(|key| one.get(*key).cloned().unwrap_or(Value::Null))
            .collect::<Vec<Value>>(),
        )
    }

    #[test]
    fn addresses_read_as_the_everyday_labels() {
        // 期望值来自 `R.merge_address((row, col))`，第 27 列是 AA 而不是「进位到 B」
        for (at, want) in [
            ((0usize, 0usize), "A1"),
            ((2, 0), "A3"),
            ((6, 1), "B7"),
            ((0, 25), "Z1"),
            ((0, 26), "AA1"),
            ((0, 51), "AZ1"),
            ((0, 701), "ZZ1"),
            ((0, 702), "AAA1"),
            ((11, 0), "A12"),
        ] {
            assert_eq!(address(at.0, at.1), want, "{at:?}");
        }
    }

    /// 一条区间摊开成什么样：原样串留着（`$` 也留着），几何按解出来的两端算
    #[test]
    fn each_ooxml_range_opens_into_its_own_geometry() {
        let hand = by_ooxml(
            &[
                "A1:D1",
                "$E$2:$G$4",
                "A12",
                "B7:C9",
                "D1:A1",
                "nope",
                "H1:I1",
            ],
            &["A1", "B7", "H1"],
            Some(6),
        );
        assert_eq!(
            agg(&hand),
            json!([7, 7, false, 6, false, 5, 1, 1, 1, 17, 3, 1]),
            "{hand}"
        );
        assert_eq!(
            row(&hand, 0),
            json!([0, "A1:D1", "A1", "D1", 1, 4, 4, 3, false, true, false, false, false])
        );
        // 带 `$` 的那一条：串照文件写的交，地址按解出来的两端交
        assert_eq!(
            row(&hand, 1),
            json!([
                1,
                "$E$2:$G$4",
                "E2",
                "G4",
                3,
                3,
                9,
                8,
                false,
                false,
                false,
                false,
                false
            ])
        );
        // 没有冒号的 `A12` 是合法的单格「合并」：两端同为一格，`solo` 才是它的名字
        assert_eq!(
            row(&hand, 2),
            json!([2, "A12", "A12", "A12", 1, 1, 1, 0, true, false, false, false, false])
        );
        assert_eq!(
            row(&hand, 3),
            json!([3, "B7:C9", "B7", "C9", 3, 2, 6, 5, false, true, false, false, false])
        );
        assert_eq!(
            row(&hand, 6),
            json!([6, "H1:I1", "H1", "I1", 1, 2, 2, 1, false, true, false, false, false])
        );
    }

    /// 两种文件自己没规矩的写法各留一条明路的旗子，而不是偷偷修好
    #[test]
    fn a_reversed_or_unparsable_range_says_so() {
        let hand = by_ooxml(
            &[
                "A1:D1",
                "$E$2:$G$4",
                "A12",
                "B7:C9",
                "D1:A1",
                "nope",
                "H1:I1",
            ],
            &["A1", "B7", "H1"],
            Some(6),
        );
        // 反着写：几何仍按 min/max 摊平（1 行 4 列），但 `reversed` 立起来，
        // 并且因为它盖住了排在它前面的 `A1:D1`，`overlaps_earlier` 也是 true
        assert_eq!(
            row(&hand, 4),
            json!([4, "D1:A1", "D1", "A1", 1, 4, 4, 3, false, false, true, true, false])
        );
        // 解不动的那一条：只留原样串与那枚有无字，几何全交 null
        assert_eq!(
            row(&hand, 5),
            json!([5, "nope", null, null, null, null, null, null, null, false, false, null, true])
        );
        // 摊平的代价：反着的那一条不进 `distinct`，所以它算进了 `duplicated`（1），
        // 而它盖住的那条另算 `overlapping`（1）—— 两个数是两件事
        assert_eq!(hand["distinct"], 5);
        assert_eq!(hand["bad_refs"], 1);
    }

    /// `count` 是文件自己写的数，与找到的条数并排放，而不是拿来当结论
    #[test]
    fn the_self_stated_count_is_checked_not_believed() {
        let hand = by_ooxml(&["A1:D1", "B7:C9"], &["A1"], Some(9));
        assert_eq!(hand["total"], 2);
        assert_eq!(hand["declared"], 9);
        assert_eq!(
            hand["declared_matches"],
            json!(false),
            "文件说了 9 条，只有 2 条"
        );
        // ODF 根本没有这个数：交 null，别说成「声明了 0 条」
        let ods = odf(&[("A1".to_string(), 1usize, 4usize, true)], 400);
        assert_eq!(ods["declared"], Value::Null);
        assert_eq!(ods["declared_matches"], Value::Null);
        // 一家都没写合并块的表：这本账要在场，而且全零
        let none = by_ooxml(&[], &[], None);
        assert_eq!(
            agg(&none),
            json!([0, 0, false, null, null, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(none["rows"].as_array().expect("是数组").len(), 0);
    }

    /// 同一条区间写两遍：条数 2、去重后 1、重叠 1 —— 三本各数各的
    #[test]
    fn a_range_written_twice_is_counted_both_ways() {
        let twice = by_ooxml(&["A1:D1", "A1:D1"], &["A1"], Some(2));
        assert_eq!(
            agg(&twice),
            json!([2, 2, false, 2, true, 1, 1, 1, 0, 6, 2, 0]),
            "{twice}"
        );
        assert_eq!(
            row(&twice, 1),
            json!([1, "A1:D1", "A1", "D1", 1, 4, 4, 3, false, true, true, false, false]),
            "只数后来那一条"
        );
    }

    /// ODF 那一支：没有区间串，跨度从锚点加出来
    #[test]
    fn odf_spans_add_up_from_their_anchor() {
        let spans = vec![
            ("A1".to_string(), 1usize, 4usize, false),
            ("A3".to_string(), 3, 3, true),
            ("B7".to_string(), 3, 3, true),
            ("C4".to_string(), 1, 1, true),
            ("E1".to_string(), 2, 2, false),
        ];
        let ods = odf(&spans, 400);
        assert_eq!(
            agg(&ods),
            json!([5, 5, false, null, null, 5, 0, 1, 1, 22, 3, 0]),
            "{ods}"
        );
        // `written` 这一族永远是 null：文件里就没有区间这么个串
        assert_eq!(
            row(&ods, 0),
            json!([0, null, "A1", "D1", 1, 4, 4, 3, false, false, false, false, false])
        );
        assert_eq!(
            row(&ods, 2),
            json!([2, null, "B7", "D9", 3, 3, 9, 8, false, true, false, false, false])
        );
        // 一格跨度落在别人的矩形里：单格与重叠两枚旗子同时立起来
        assert_eq!(
            row(&ods, 3),
            json!([3, null, "C4", "C4", 1, 1, 1, 0, true, true, true, false, false])
        );
        assert_eq!(
            row(&ods, 4),
            json!([4, null, "E1", "F2", 2, 2, 4, 3, false, false, false, false, false])
        );
    }

    /// 条数一多就截断，但合计仍按全部算（截的是清单，不是账）
    #[test]
    fn truncation_cuts_the_list_and_not_the_arithmetic() {
        let refs = [
            "A1:D1",
            "$E$2:$G$4",
            "A12",
            "B7:C9",
            "D1:A1",
            "nope",
            "H1:I1",
        ];
        let filled = ["A1", "B7", "H1"];
        let wide = ooxml(
            &refs
                .iter()
                .map(|one| one.to_string())
                .collect::<Vec<String>>(),
            &filled
                .iter()
                .map(|one| one.to_string())
                .collect::<Vec<String>>(),
            Some(6),
            3,
        );
        assert_eq!(
            agg(&wide),
            json!([7, 3, true, 6, false, 5, 1, 1, 1, 17, 3, 1]),
            "{wide}"
        );
        assert_eq!(wide["rows"].as_array().expect("是数组").len(), 3);
        assert_eq!(wide["rows"][2]["written"], "A12");
    }
}
