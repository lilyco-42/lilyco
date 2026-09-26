//! 这一串字是横着走还是竖着走：一家把话写在格子/段/节自己身上，一家全部写在样式上
//!
//! OOXML 有五处可以说话，形状还不一样：格上 `w:tcPr/w:textDirection/@w:val`（五个枚举值，
//! 实测 lrTb / tbRl / btLr / lrTbV / tbRlV）、表上 `w:tblPr/w:bidiVisual`（开关）、
//! 段上 `w:pPr/w:bidi`（开关）、字上 `w:rPr/w:rtl`（开关）、节上 `w:sectPr` 的 `w:bidi`（开关）
//! 与 `w:textDirection`（值）。开关那几枚「在场」与「开着」不是一回事（`w:val="0"` 是关掉），
//! 所以每枚都交 `present` + `val` + 算出来的两态 —— 与分页那四个开关同一套读法。
//!
//! ODF 一处都不写在正文元素上：格子与表只点一个样式名（`table:style-name`）、段点
//! `text:style-name`，走向坐在那份样式的 `style:table-cell-properties` / `style:table-properties` /
//! `style:paragraph-properties` 上，而页面那一处挂在 `style:page-layout` 的
//! `style:page-layout-properties` 上（不在 `style:style` 下面 —— 与另外三处的父元素不同名）。
//!
//! 三条只有这一族才有的讲究，都是量出来的：
//! 1. 值有**两种词法**：`style:writing-mode` 与 LibreOffice 扩展的 `loext:writing-mode`，
//!    局部名一模一样 —— 只按局部名收就会互相盖掉（`dir-cell.odt` 的 `bt-lr` 那一格走的就是
//!    `loext:` 这一路）。所以每行都交 `vocabulary`，两家读者不许私下合并。
//! 2. 枚举值也不通用：ODF 写 `rl-tb` / `tb-rl` / `bt-lr` / `lr-tb` / `page`，OOXML 写
//!    `tbRl` / `tbRlV` / `btLr` / `lrTb` / `lrTbV` —— 各按文件写的词交，不互相翻译。
//!    只有 `page` 这一个值在 OOXML 那边根本没有：它是「照页面走」，即这张表自己不说、由页面那处定。
//! 3. 样式分两处住：自动样式（`office:automatic-styles`，生产者按地址起名 `表格1.A1`）与
//!    命名样式（`office:styles`，如 `Standard`）。**同一枚 `lr-tb` 从哪一处来是两个问题**：
//!    实测 `dir-cell.odt` 14 段里 13 段是靠命名样式 `Standard` 那枚默认值才「说了话」，
//!    只有第 2 段是自己那份 `P1` 写着 `rl-tb` —— 所以每级都交 `*_from`（automatic / named / other）。
//!
//! 跨族对照（同一份 `dir-cell.docx` 转成 ODF 再读）里最值钱的两条也交在原样：
//! OOXML 把「这张表从右往左」写在表上的 `w:bidiVisual`，ODF 把它写在表样式那枚
//! `style:writing-mode="rl-tb"` 上；而 OOXML 写在**节**上的 `w:bidi`，到 ODF 变成了**页面**那一条
//! （实测 `dir-sect.odt` 的 `Mpm1` 写着 `rl-tb`）—— 同一句「整份文档倒过来」在两族落在不同的层。
//!
//! 生产者差别（`dir-cell.docx` 与它的 LibreOffice 重写）：python-docx 那份五格各写一个值、
//! 一段说 `w:bidi`、一节不说任何事；重写那一份**把 `lrTb` 与 `lrTbV` 两格整个不再写**
//! （说了等于没说的那两格没了，`tbRlV` 被换成 `tbRl`），反过来给 10 个段落各补一句
//! `w:bidi w:val="0"`，还在节上补了一枚 `w:textDirection w:val="lrTb"` —— 交着话的段从 2 段变 11 段。
//!
//! RTF 与 legacy .doc 这一格不交：那一族把同一件事写成 `\cltxtbrl`（格，实测 ×2）、
//! `\cltxbtlr`（格，×1）、`\rtlrow`（行，×2 —— OOXML 写在表上的一句在这里落到**每一行**）、
//! `\rtlpar`（段，×1）与 `\ltrpar`（×27，默认值被逐段重发）。归属要按行群与格群切开才判得住，
//! 而本读者在这一族连「几张表」都判不住（见 fact 100），所以先把规则记在这里、不交一个猜的数。

use crate::xmlscan::Node;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一个开关的三种状态：元素在不在、文件给没给值、按给的词算「开着」还是「关着」
fn switch(present: bool, val: Option<&str>) -> Value {
    let off = matches!(val, Some("0") | Some("false") | Some("off") | Some("none"));
    json!({
        "present": present,
        "val": val.map(String::from),
        "on_written": present && !off,
        "off_written": present && off,
    })
}

/// 元素里第一个局部名等于 `want` 的直接孩子
fn kid<'a>(node: Option<&'a Node>, want: &str) -> Option<&'a Node> {
    node.and_then(|one| one.child(want))
}

fn val_of(node: Option<&Node>) -> Option<&str> {
    node.and_then(|one| one.attr_local("val"))
}

fn bump(values: &mut serde_json::Map<String, Value>, grouped: String) {
    let next = values.get(&grouped).and_then(Value::as_u64).unwrap_or(0) + 1;
    values.insert(grouped, json!(next));
}

/// 「在场而没带值」与「写了值」两种拼法分开数（与分页那四个开关同一套词）
fn bump_switch(values: &mut serde_json::Map<String, Value>, name: &str, raw: Option<&str>) {
    bump(
        values,
        format!(
            "{} {}",
            name,
            if raw.is_none() { "bare" } else { "with_value" }
        ),
    );
}

/// OOXML 那一份：五处各交各的，一处也不合并成「这份文档是不是从右往左」
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut values: serde_json::Map<String, Value> = serde_json::Map::new();
    let tables = body.descendants("tbl");
    let mut cell_rows: Vec<Value> = Vec::new();
    let mut table_rows: Vec<Value> = Vec::new();
    let mut cells_total = 0usize;
    for (at, tbl) in tables.iter().enumerate() {
        let marked = kid(tbl.child("tblPr"), "bidiVisual");
        let raw = val_of(marked);
        if marked.is_some() {
            bump_switch(&mut values, "bidiVisual", raw);
        }
        let rows = tbl.all("tr");
        table_rows.push(json!({
            "at": at,
            "rows": rows.len(),
            "bidi_visual": switch(marked.is_some(), raw),
        }));
        for (row, tr) in rows.iter().enumerate() {
            for (col, tc) in tr.all("tc").into_iter().enumerate() {
                cells_total += 1;
                let Some(found) = kid(tc.child("tcPr"), "textDirection") else {
                    continue;
                };
                let got = found.attr_local("val");
                bump(
                    &mut values,
                    format!("textDirection={}", got.unwrap_or("none")),
                );
                cell_rows.push(json!({"at": at, "row": row, "col": col, "val": got}));
            }
        }
    }
    let mut para_rows: Vec<Value> = Vec::new();
    let mut indexed: Vec<u64> = Vec::new();
    for (index, para) in body.descendants("p").iter().enumerate() {
        let marked = kid(para.child("pPr"), "bidi");
        let raw = val_of(marked);
        if marked.is_some() {
            bump_switch(&mut values, "bidi", raw);
        }
        let mut rtl = 0usize;
        for run in para.descendants("r") {
            let Some(found) = kid(run.child("rPr"), "rtl") else {
                continue;
            };
            rtl += 1;
            bump_switch(&mut values, "rtl", found.attr_local("val"));
        }
        if marked.is_some() || rtl > 0 {
            indexed.push(index as u64);
        }
        para_rows.push(json!({
            "index": index,
            "bidi": switch(marked.is_some(), raw),
            "rtl_runs": rtl,
        }));
    }
    let mut sect_rows: Vec<Value> = Vec::new();
    for (index, sect) in body.descendants("sectPr").iter().enumerate() {
        let marked = sect.child("bidi");
        let raw = val_of(marked);
        if marked.is_some() {
            bump_switch(&mut values, "sectPr bidi", raw);
        }
        let held = sect.child("textDirection");
        let got = val_of(held);
        if held.is_some() {
            bump(
                &mut values,
                format!("sectPr textDirection={}", got.unwrap_or("none")),
            );
        }
        sect_rows.push(json!({
            "index": index,
            "bidi": switch(marked.is_some(), raw),
            "text_direction": got,
            "text_direction_present": held.is_some(),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "tables_total": tables.len(),
        "cells_total": cells_total,
        "cells_written": cell_rows.len(),
        "cells": cell_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "tables": table_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "paragraphs_total": para_rows.len(),
        "paragraphs_written": indexed.len(),
        "paragraphs_indexed": indexed,
        "paragraphs": para_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "sections_total": sect_rows.len(),
        "sections": sect_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "values_written": Value::Object(values),
    })
}

/// 一处样式说过的话：值、词法（`style` / `loext`）、住在哪个部件、父样式名、住在哪个样式列表里
struct Stated {
    value: String,
    vocab: String,
    part: String,
    parent: Option<String>,
    declared: String,
}

const PROPS: [&str; 4] = [
    "table-properties",
    "table-cell-properties",
    "paragraph-properties",
    "page-layout-properties",
];

/// 按文件写的那个前缀认词法：两种词法的局部名都是 `writing-mode`，合并就丢了一个词法
fn writing_mode(node: Option<&Node>) -> Option<(String, String)> {
    let one = node?;
    for name in ["style:writing-mode", "loext:writing-mode"] {
        if let Some(raw) = one.attr(name) {
            return Some((raw.to_string(), name.split(':').next()?.to_string()));
        }
    }
    None
}

/// 按文档顺序找到那三个样式列表（列表不互相嵌套，找到一个就不再深入）
fn style_lists<'a>(root: &'a Node, out: &mut Vec<&'a Node>) {
    if matches!(
        root.local(),
        "automatic-styles" | "styles" | "master-styles"
    ) {
        out.push(root);
        return;
    }
    for one in &root.children {
        style_lists(one, out);
    }
}

/// 「点了名」= 文件真写了一个非空的样式名；空串是这一族说「没有」的写法，不算点过
fn spoke(name: Option<&str>) -> bool {
    name.is_some_and(|one| !one.is_empty())
}

/// 同一枚值来自自动样式、命名样式（默认值那一份）还是别处，三个各数各的
#[derive(Default)]
struct Origin(usize, usize, usize);

impl Origin {
    fn add(&mut self, declared: &str) {
        match declared {
            "automatic-styles" => self.0 += 1,
            "styles" => self.1 += 1,
            _ => self.2 += 1,
        }
    }

    fn tally(&self) -> Value {
        json!({"automatic": self.0, "named": self.1, "other": self.2})
    }
}

/// 两问分开答：那点名的样式**在不在**（这一跳断没断），和它**提没提起走向**
///
/// 「没提」有两种：样式找到了但那一处属性没写走向，与样式根本不在。实测两份件里各有一种的
/// 邻居，混成一个数就看不出是哪一种。找到的那枚值顺手记进总账（两处词法各记各的）。
fn look<'a>(
    books: &'a BTreeMap<&'static str, BTreeMap<String, Stated>>,
    known: &[String],
    values: &mut serde_json::Map<String, Value>,
    props_name: &'static str,
    name: Option<&str>,
) -> (Option<&'a Stated>, bool) {
    if !spoke(name) {
        return (None, false);
    }
    let want = name.unwrap_or_default();
    let got = books.get(props_name).and_then(|one| one.get(want));
    if let Some(one) = got {
        bump(values, format!("{}:writing-mode={}", one.vocab, one.value));
    }
    (got, known.iter().any(|had| had == want))
}

/// ODF 那一份：四处全在样式上，正文元素上一个字都不写（按文件自己的词交）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<(&Node, &str)> = vec![(content, "content.xml")];
    if let Some(extra) = styles {
        roots.push((extra, "styles.xml"));
    }
    // 四类属性块各一本（一本不会串到另一本）；样式名**在不在**另说，所以 known 单独一份
    let mut books: BTreeMap<&'static str, BTreeMap<String, Stated>> = BTreeMap::new();
    for name in PROPS {
        books.insert(name, BTreeMap::new());
    }
    let mut known: Vec<String> = Vec::new();
    for (root, part) in &roots {
        let mut holders: Vec<&Node> = Vec::new();
        style_lists(root, &mut holders);
        for holder in holders {
            let where_key = holder.local().to_string();
            for one in &holder.children {
                if !matches!(one.local(), "style" | "page-layout") {
                    continue;
                }
                // 页面那一处挂在 `style:page-layout` 自己身上，父元素与另外三处不同名
                let Some(name) = one.attr_local("name") else {
                    continue;
                };
                if !known.iter().any(|had| had == name) {
                    known.push(name.to_string());
                }
                let parent = one.attr_local("parent-style-name").map(String::from);
                for props_name in PROPS {
                    let Some((value, vocab)) = writing_mode(one.child(props_name)) else {
                        continue;
                    };
                    let book = books.get_mut(props_name).expect("四类属性块都在表里");
                    if book.contains_key(name) {
                        // 先写的赢：同一份件里重名的定义不替后来者圆场
                        continue;
                    }
                    book.insert(
                        name.to_string(),
                        Stated {
                            value,
                            vocab,
                            part: part.to_string(),
                            parent: parent.clone(),
                            declared: where_key.clone(),
                        },
                    );
                }
            }
        }
    }
    let mut values: serde_json::Map<String, Value> = serde_json::Map::new();

    let tables = content.descendants("table");
    let mut cell_rows: Vec<Value> = Vec::new();
    let mut table_rows: Vec<Value> = Vec::new();
    let mut cells_total = 0usize;
    let mut cells_named = 0usize;
    let mut cells_found = 0usize;
    let mut cells_from = Origin::default();
    let mut tables_from = Origin::default();
    let mut used_styles: Vec<String> = Vec::new();
    for (at, tbl) in tables.iter().enumerate() {
        let name = tbl.attr_local("style-name");
        let (got, had) = look(&books, &known, &mut values, "table-properties", name);
        let rows = tbl.all("table-row");
        if let Some(one) = got {
            tables_from.add(&one.declared);
        }
        table_rows.push(json!({
            "at": at,
            "name": tbl.attr_local("name"),
            "style": name,
            "rows": rows.len(),
            "cols": tbl.all("table-column").len(),
            "value": got.map(|one| one.value.clone()),
            "vocabulary": got.map(|one| one.vocab.clone()),
            "style_part": got.map(|one| one.part.clone()),
            "declared_in": got.map(|one| one.declared.clone()),
            "style_found": had,
            "written": got.is_some(),
        }));
        for (row, tr) in rows.iter().enumerate() {
            let kids: Vec<&Node> = tr
                .children
                .iter()
                .filter(|one| matches!(one.local(), "table-cell" | "covered-table-cell"))
                .collect();
            for (col, tc) in kids.into_iter().enumerate() {
                cells_total += 1;
                let held = tc.attr_local("style-name");
                let (got, had) = look(&books, &known, &mut values, "table-cell-properties", held);
                if spoke(held) {
                    cells_named += 1;
                    cells_found += usize::from(had);
                }
                let Some(one) = got else { continue };
                if !used_styles.iter().any(|had| held == Some(had.as_str())) {
                    if let Some(want) = held {
                        used_styles.push(want.to_string());
                    }
                }
                cells_from.add(&one.declared);
                cell_rows.push(json!({
                    "at": at, "row": row, "col": col,
                    "covered": tc.local() == "covered-table-cell",
                    "style": held,
                    "value": one.value.clone(),
                    "vocabulary": one.vocab.clone(),
                    "style_part": one.part.clone(),
                    "parent": one.parent.clone(),
                    "declared_in": one.declared.clone(),
                }));
            }
        }
    }
    let mut para_rows: Vec<Value> = Vec::new();
    let mut p_indexed: Vec<u64> = Vec::new();
    let mut paras_named = 0usize;
    let mut paras_found = 0usize;
    let mut paras_from = Origin::default();
    for (index, para) in content.descendants("p").iter().enumerate() {
        let name = para.attr_local("style-name");
        let (got, had) = look(&books, &known, &mut values, "paragraph-properties", name);
        if spoke(name) {
            paras_named += 1;
            paras_found += usize::from(had);
        }
        if let Some(one) = got {
            p_indexed.push(index as u64);
            paras_from.add(&one.declared);
        }
        para_rows.push(json!({
            "index": index,
            "style": name,
            "value": got.map(|one| one.value.clone()),
            "vocabulary": got.map(|one| one.vocab.clone()),
            "style_part": got.map(|one| one.part.clone()),
            "declared_in": got.map(|one| one.declared.clone()),
            "style_found": had,
            "written": got.is_some(),
        }));
    }
    // 页面那一处没有正文可点：只交「哪些定义写了它」，母版页那一跳没量过，所以不判落在哪一页
    let mut page_rows: Vec<Value> = Vec::new();
    for (name, one) in books
        .get("page-layout-properties")
        .expect("四类属性块都在表里")
    {
        bump(
            &mut values,
            format!("{}:writing-mode={}", one.vocab, one.value),
        );
        page_rows.push(json!({
            "style": name,
            "value": one.value.clone(),
            "vocabulary": one.vocab.clone(),
            "part": one.part.clone(),
            "parent": one.parent.clone(),
            "declared_in": one.declared.clone(),
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "tables_total": tables.len(),
        "cells_total": cells_total,
        "cells_named": cells_named,
        "cells_found": cells_found,
        "cells_written": cell_rows.len(),
        "distinct_cell_styles": used_styles.len(),
        "cells_from": cells_from.tally(),
        "cells": cell_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "tables_written": tables_from.0 + tables_from.1 + tables_from.2,
        "tables_found": table_rows.iter().filter(|one| one.get("style_found") == Some(&Value::Bool(true))).count(),
        "tables_from": tables_from.tally(),
        "tables": table_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "paragraphs_total": para_rows.len(),
        "paragraphs_named": paras_named,
        "paragraphs_found": paras_found,
        "paragraphs_written": p_indexed.len(),
        "paragraphs_indexed": p_indexed,
        "paragraphs_from": paras_from.tally(),
        "paragraphs": para_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "page_definitions_total": page_rows.len(),
        "page_definitions": page_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "values_written": Value::Object(values),
    })
}
