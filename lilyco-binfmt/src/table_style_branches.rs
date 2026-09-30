//! OOXML 表格样式身上那批「条件分支」（`tbl_style_branches`，只在 .docx 交）
//!
//! 「这张表套的哪个样式」是 `table_styles` 那一本（名字与 `w:tblLook` 的位），而
//! **表头加粗、隔行底纹、四个角各长什么样**并不写在表身上，写在样式自己的
//! `w:tblStylePr` 里：一枚分支 = 一个条件（`w:type="firstRow"` / `band1Vert` / `nwCell` …）
//! 加上它要覆写的那几本盒子（`w:pPr` / `w:rPr` / `w:tblPr` / `w:tcPr`）。
//! 日常那两句话由这一本回答：「这份文档认得了哪些表格样式」与「它们各自声明了什么」。
//!
//! 量到的形状（本仓 98 份 .docx 里 84 份带这批分支，合计 54516 枚）：
//! 1. **`w:if` 一枚都没有**：自产件与本机 810 份真件的 12332 枚分支里，`w:if` 写了的 0 枚 ——
//!    条件全靠 `w:type` 那十个取值说话，所以 `if_written` 交 0 而不是猜一种读法。
//! 2. **`w:tblPr` 是空壳**：全库 49572 枚 `w:tblPr` 盒子**没有一枚写过属性或子元素**，
//!    而同一位置 `w:tcPr` 有 45864 枚是有内容的 —— 「表级属性在条件分支里改不动」这句话
//!    在文件里就是这一枚空壳。空壳与有内容分开数（`empty_box_names` / `nonempty_box_names`）。
//! 3. **一家写全、一家写漏**：同一份内容 LibreOffice 重写后 `w:tblPr` 从 546 枚变 649 枚
//!    （每一枚分支都补上空壳），并另留 7 枚**空壳 `w:rPr`**；`w:iCs` 那 28 枚整个不见了，
//!    `themeFillTint` 的大小写从 `3F` 变 `3f`。都按文件的原样交，不替谁圆场。
//! 4. 底纹有两种点法：`w:fill` 是实色，`w:themeFill` + `w:themeFillTint` 是主题那一格的指针
//!    加浓度 —— 这里只按写的交（解到实色是 `theme_ledger` / `color_refs` 那两本的事）。

use crate::office_doc::local_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 按局部名记一笔（与 `local_attrs` 同一条：`w:val` 与 `val` 是同一枚）
fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

/// 一枚盒子（`w:tblPr` 这一类装属性的元素）写了什么：空壳要能说出口
fn box_row(one: &Node) -> Value {
    let kids: Vec<String> = one
        .children
        .iter()
        .map(|kid| kid.local().to_string())
        .collect();
    let written = local_attrs(one);
    let bare = match written.as_object() {
        Some(map) => map.is_empty(),
        None => true,
    };
    json!({
        "present": true,
        "empty": kids.is_empty() && bare,
        "written": written,
        "children": kids,
    })
}

/// 一枚分支：条件是什么、挂了哪几本盒子、底纹与边框各写了什么
///
/// 盒子按**第一次出现**的那一枚交（同名多枚时后面的不覆盖前面的），而边框的边与底纹
/// 按文件顺序逐条列 —— 与标准库读者的同一条。
fn branch_row(one: &Node, sums: &mut Sums) -> Value {
    let mut boxes = serde_json::Map::new();
    let mut holders: Vec<&Node> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for kid in &one.children {
        let name = kid.local().to_string();
        if boxes.contains_key(&name) {
            continue;
        }
        let row = box_row(kid);
        let shell = row["empty"].as_bool().unwrap_or(false);
        if shell {
            bump(&mut sums.empty_boxes, &name);
        } else {
            bump(&mut sums.nonempty_boxes, &name);
        }
        bump(&mut sums.box_names, &name);
        holders.push(kid);
        names.push(name.clone());
        boxes.insert(name, row);
    }
    let mut shading: Option<Value> = None;
    let mut edges: Vec<Value> = Vec::new();
    let mut runs: Vec<Value> = Vec::new();
    let mut paras: Vec<Value> = Vec::new();
    for (index, kid) in holders.iter().enumerate() {
        match names[index].as_str() {
            "tcPr" => {
                for inner in &kid.children {
                    if inner.local() == "shd" {
                        shading = Some(local_attrs(inner));
                    } else if inner.local() == "tcBorders" {
                        for edge in &inner.children {
                            let written = local_attrs(edge);
                            if let Some(raw) = written.get("val").and_then(|v| v.as_str()) {
                                bump(&mut sums.border_vals, raw);
                            }
                            if let Some(raw) = written.get("themeColor").and_then(|v| v.as_str()) {
                                bump(&mut sums.border_themes, raw);
                            }
                            bump(&mut sums.border_edges, edge.local());
                            edges.push(json!({"edge": edge.local(), "written": written}));
                        }
                    }
                }
            }
            "rPr" => {
                for inner in &kid.children {
                    bump(&mut sums.run_props, inner.local());
                    runs.push(json!({"name": inner.local(),
                                     "written": local_attrs(inner)}));
                }
            }
            "pPr" => {
                for inner in &kid.children {
                    bump(&mut sums.para_props, inner.local());
                    paras.push(json!({"name": inner.local(),
                                      "written": local_attrs(inner)}));
                }
            }
            _ => {}
        }
    }
    sums.branches_total += 1;
    let kind = one.attr_local("type").map(String::from);
    bump(&mut sums.types, kind.as_deref().unwrap_or("<无 type>"));
    if let Some(raw) = one.attr_local("if") {
        sums.if_written += 1;
        bump(&mut sums.if_values, raw);
    }
    if let Some(had) = &shading {
        sums.shading_branches += 1;
        if let Some(map) = had.as_object() {
            if let Some(raw) = map.get("val").and_then(|v| v.as_str()) {
                bump(&mut sums.shd_vals, raw);
            }
            if let Some(raw) = map.get("fill").and_then(|v| v.as_str()) {
                bump(&mut sums.shd_fills, raw);
            }
            if let Some(raw) = map.get("themeFill").and_then(|v| v.as_str()) {
                bump(&mut sums.theme_fills, raw);
            }
            if let Some(raw) = map.get("themeFillTint").and_then(|v| v.as_str()) {
                bump(&mut sums.theme_tints, raw);
            }
        }
    }
    json!({
        "type": kind,
        "if": one.attr_local("if"),
        "written": local_attrs(one),
        "boxes": boxes,
        "shading": shading,
        "borders": edges,
        "run_props": runs,
        "para_props": paras,
    })
}

/// 整本的计数：与逐条行同时攒出来（截断只影响 entries，不影响这些账）
#[derive(Default)]
struct Sums {
    branches_total: usize,
    if_written: usize,
    shading_branches: usize,
    types: BTreeMap<String, u64>,
    if_values: BTreeMap<String, u64>,
    box_names: BTreeMap<String, u64>,
    empty_boxes: BTreeMap<String, u64>,
    nonempty_boxes: BTreeMap<String, u64>,
    shd_vals: BTreeMap<String, u64>,
    shd_fills: BTreeMap<String, u64>,
    theme_fills: BTreeMap<String, u64>,
    theme_tints: BTreeMap<String, u64>,
    border_edges: BTreeMap<String, u64>,
    border_vals: BTreeMap<String, u64>,
    border_themes: BTreeMap<String, u64>,
    run_props: BTreeMap<String, u64>,
    para_props: BTreeMap<String, u64>,
}

/// 没这一层时的账：键全给、条数全 0（0 是「看过了没有」，缺部件与没表格样式分两格）
fn empty(part: bool) -> Value {
    json!({
        "family": "ooxml",
        "available": part,
        "part": part,
        "table_styles_total": 0,
        "styles_with_branches": 0,
        "branches_total": 0,
        "distinct_types": 0,
        "branch_types": {},
        "if_written": 0,
        "if_values": {},
        "box_names": {},
        "empty_box_names": {},
        "nonempty_box_names": {},
        "shading_branches": 0,
        "shading_vals": {},
        "shading_fills": {},
        "theme_fill_names": {},
        "theme_fill_tints": {},
        "border_edges": {},
        "border_vals": {},
        "border_theme_colors": {},
        "run_prop_names": {},
        "para_prop_names": {},
        "style_attrs_written": {},
        "entries": [],
    })
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let member = match zipread::member(bytes, "word/styles.xml", DEFAULT_MEMBER_CAP).ok() {
        Some(one) => one,
        None => return empty(false),
    };
    let root = xmlscan::parse_str(&member.as_text());
    let found = root.descendants("style");
    let mut styles: Vec<&Node> = Vec::new();
    for one in &found {
        if one.attr_local("type") == Some("table") {
            styles.push(*one);
        }
    }
    if styles.is_empty() {
        return empty(true);
    }
    let mut sums = Sums::default();
    let mut style_attrs: BTreeMap<String, u64> = BTreeMap::new();
    let mut rows: Vec<Value> = Vec::new();
    for style in &styles {
        for (key, _) in &style.attrs {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            bump(
                &mut style_attrs,
                key.rsplit(':').next().unwrap_or(key.as_str()),
            );
        }
        let branches = style.descendants("tblStylePr");
        if branches.is_empty() {
            continue;
        }
        let mut named: Option<String> = None;
        for kid in &style.children {
            if kid.local() == "name" {
                named = kid.attr_local("val").map(String::from);
                break;
            }
        }
        let detail: Vec<Value> = branches
            .iter()
            .map(|one| branch_row(*one, &mut sums))
            .collect();
        rows.push(json!({
            "style_id": style.attr_local("styleId"),
            "name": named,
            "written": local_attrs(*style),
            "branches_total": detail.len(),
            "branches": detail,
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "part": true,
        // 「认得多少种」与「其中几种写了分支」是两句话
        "table_styles_total": styles.len(),
        "styles_with_branches": rows.len(),
        "branches_total": sums.branches_total,
        "distinct_types": sums.types.len(),
        "branch_types": sums.types,
        "if_written": sums.if_written,
        "if_values": sums.if_values,
        "box_names": sums.box_names,
        "empty_box_names": sums.empty_boxes,
        "nonempty_box_names": sums.nonempty_boxes,
        "shading_branches": sums.shading_branches,
        "shading_vals": sums.shd_vals,
        "shading_fills": sums.shd_fills,
        "theme_fill_names": sums.theme_fills,
        "theme_fill_tints": sums.theme_tints,
        "border_edges": sums.border_edges,
        "border_vals": sums.border_vals,
        "border_theme_colors": sums.border_themes,
        "run_prop_names": sums.run_props,
        "para_prop_names": sums.para_props,
        "style_attrs_written": style_attrs,
        "entries": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
