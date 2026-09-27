//! 表格的这一圈到底有没有线 —— 三族三个形状，所以三份账分开交。
//!
//! OOXML 文本那一族有**两个住处、形状一样**的两块：`w:tblPr/w:tblBorders` 是表级默认，
//! `w:tcPr/w:tcBorders` 是这一格自己改的。每一块是若干枚方向孩子（`top` / `left` / `bottom`
//! / `right`，表级另有 `insideH` / `insideV`，格级另有两枚对角线 `tl2br` / `tr2bl`，
//! LibreOffice 会把 `left` / `right` 改名成 `start` / `end`），每枚带七个属性：
//! `@w:val`（`single` / `double` 是有线，`none` 与 `nil` 是两种「没有」）、`@w:sz`
//! （八分之一磅）、`@w:space`（线离字多远）、`@w:color`（`auto` 是一句话而不是某个色值），
//! 再加主题那一套 `@w:themeColor` / `@w:themeTint` / `@w:themeShade`。
//!
//! DrawingML 把线挂在**格子自己的 properties 上**，而且一枚线不是属性而是一串：`a:lnL` /
//! `a:lnR` / `a:lnT` / `a:lnB` / `a:lnTlToBr` / `a:lnBlToTr`，带 `@w`（EMU）与 `@cap` /
//! `@cmpd` / `@algn`，孩子里有填法（`a:solidFill` 或 `a:noFill`）、`a:prstDash`（虚线预设）、
//! `a:round`（接头）、`a:headEnd` / `a:tailEnd`（端点）。
//!
//! ODF 是一跳：格只写 `table:style-name`，数在那份 `family="table-cell"` 样式的 properties 上。
//! 实测两族的 properties **不是同一枚孩子**：odt 住 `style:table-cell-properties`，而 odp 的表
//! 格把边框写在 `style:paragraph-properties` 上。写法有三种：短款 `fo:border`、四枚长款
//! `fo:border-*`，以及第三种 `style:border-line-width-*`（只写线宽，不写线型与颜色）。
//! 一张表是叠在一起画的还是各画各的，写在 `style:table-properties/@table:border-model`。
//!
//! 实测（`borders.docx` = python-docx 打底 + 按 ECMA 手写三张表；`borders.pptx` = python-pptx
//! 打底 + 按 ECMA 手写八枚线；`borders.odt` / `borders.odp` = LibreOffice 转出去的那两份；
//! `borders-lo.docx` / `borders-lo.pptx` = LibreOffice 同格式重写）：
//! 1. **表上没写不等于没有线**：`word/styles.xml` 里恒有 85 枚 `w:tblBorders` 与 406 枚
//!    `w:tcBorders`（python-docx 那张打底模板的表格样式各带一份），所以「正文这一处写了没有」
//!    与「样式表里有多少枚」分开交，不替文件挑一份样式；
//! 2. `@w:val="nil"` 与 `@w:val="none"` 是两句话（前者是「连继承来的那条也关掉」），而且
//!    `nil` 那些**不写** `sz` / `space` / `color` —— 缺属性是形状，不是漏读，所以 `no_sz`
//!    单独一格；
//! 3. LibreOffice 把这份 docx 重写时做了四件事：三张表并成一张、把表级那份**摊到每个格上**
//!    （`cells_with_block` 3 → 10，表级反而一份都不剩）、方向改名 `start` / `end`、把 `nil`
//!    与 `auto` 全换成写实的数（`vals` 只剩 `single` 与 `double`、`auto_color` 归零）；
//! 4. python-pptx 根本不碰线（本仓按 ECMA 手写），而 LibreOffice 重写时给每格补齐四条、
//!    把 `6350` 换成 `6480`、把我那条 25400 的**对角线整个丢掉**，还有一枚线**不写 `@w`**
//!    （`no_width` 1）—— 一枚都没有与有一枚但没宽度是两种「没说」；
//! 5. ODF 那一跳是有损的：`single` 八分之一磅回来是 `1pt solid #000000`，`double` 是
//!    `2.25pt double`，而 `auto` 与主题指针被换成实色（`#000000` / `#c0504d`），
//!    `nil` 与 `none` 在 ODF 这一格上**合成同一个 `none`** —— 只按交的回答，不替文件分辨；
//! 6. 真件普查（本机 32 份 .docx + 1 份 .docm、268 个 `word/*.xml` 部件；104 份 pptx、
//!    940 个 slide 部件）：docx 表级块 3067 枚、格级块 14345 枚，方向条目 80193 条**全部**带
//!    `@w:val`，其中 `nil` 39746、`single` 39483、`double` 952、`none` 12，而带 `sz` 的只有
//!    40446 条（正是 `single` 那一半加一点），`space` 恒为 `0`，`color` 里 `auto` 出现过
//!    4208 次，主题指针 34433 次；pptx 里 893 枚 `a:tcPr` **每一枚**都写满上下左右四条，
//!    `@w` 只有 `6350`（3444 次）与 `0`（128 次）两种，`cap` 恒 `flat`、`cmpd` 恒 `sng`、
//!    `algn` 恒 `ctr`，孩子一律是 solidFill + prstDash + round + headEnd + tailEnd 那一套 ——
//!    所以 `noFill`、虚线、对角线、一枚线都不写这四形只在自产件里；
//! 7. 本机真件里 **.odt / .ods / .odp 一份都没有** —— ODF 那一头只有生产者的凭据（LibreOffice
//!    的两转两写），不是「读不出来」，是「没有件可读」。
//!
//! 不做的事：**不换算单位**（八分之一磅 / EMU / `1pt` 都按写的串交）、**不合并线型语义**
//! （`nil` 与 `none` 各按各的交，ODF 那边合成一个 `none` 就是合成一个）、**不判渲染结果**
//! （表级与格级同时写了两条不同的线时谁赢是排版的事，两份都交）、**不替表挑样式**。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// 方向孩子的十个可能名字（`insideH` / `insideV` 只在表级，`tl2br` / `tr2bl` 只在格级，
/// `start` / `end` 是 LibreOffice 改名后的那一对）
const BOR_DIRS: [&str; 10] = [
    "top", "left", "bottom", "right", "insideH", "insideV", "tl2br", "tr2bl", "start", "end",
];
/// 每枚方向上的七个属性（按这个序交，缺的交 null）
const BOR_ATTRS: [&str; 7] = [
    "val",
    "sz",
    "space",
    "color",
    "themeColor",
    "themeTint",
    "themeShade",
];
/// `a:tcPr` 上那六枚线
const PPT_EDGES: [&str; 6] = ["lnL", "lnR", "lnT", "lnB", "lnTlToBr", "lnBlToTr"];
/// 一枚线的四枚属性
const PPT_EDGE_ATTRS: [&str; 4] = ["w", "cap", "cmpd", "algn"];
/// 只有这四枚都写了才算「上下左右都写了」
const PPT_FOUR: [&str; 4] = ["lnL", "lnR", "lnT", "lnB"];
/// ODF 里边框会住在的三枚 properties（odt 与 odp 各一枚，实测 odp 走 paragraph-properties）
const ODF_BOR_PROPS: [&str; 3] = [
    "table-cell-properties",
    "paragraph-properties",
    "graphic-properties",
];

fn kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

fn attr_of<'a>(node: &'a Node, want: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(key, _)| key.rsplit(':').next().unwrap_or(key) == want)
        .map(|(_, value)| value.as_str())
}

/// 按**文档序**走整棵树（`descendants` 一枚名字一枚名字地拿，两族名字混在一起时要它）
fn every<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(node);
    for kid in node.children.iter() {
        if kid.name != "#text" {
            every(kid, out);
        }
    }
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_i64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

/// 一枚方向孩子：七个属性各是「写了什么串」，缺的是 None（不是空串）
#[derive(Clone)]
struct BorLine {
    name: String,
    got: [Option<String>; 7],
}

impl BorLine {
    fn of(node: &Node) -> BorLine {
        let mut got: [Option<String>; 7] = [None, None, None, None, None, None, None];
        for (index, key) in BOR_ATTRS.iter().enumerate() {
            got[index] = attr_of(node, key).map(|one| one.to_string());
        }
        BorLine {
            name: node.local().to_string(),
            got,
        }
    }

    fn json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (index, key) in BOR_ATTRS.iter().enumerate() {
            let hit = match &self.got[index] {
                Some(had) => json!(had.as_str()),
                None => Value::Null,
            };
            out.insert((*key).to_string(), hit);
        }
        Value::Object(out)
    }
}

/// 一枚 `w:tblBorders` / `w:tcBorders` → 按文档序的方向孩子
fn bor_block(holder: &Node) -> Vec<BorLine> {
    kids(holder)
        .into_iter()
        .filter(|one| BOR_DIRS.contains(&one.local()))
        .map(BorLine::of)
        .collect()
}

fn bor_rows(lines: &[BorLine]) -> Value {
    let mut out = serde_json::Map::new();
    for one in lines.iter() {
        out.insert(one.name.clone(), one.json());
    }
    Value::Object(out)
}

fn bor_dirs(lines: &[BorLine]) -> Vec<String> {
    lines.iter().map(|one| one.name.clone()).collect()
}

/// 表级与格级同一段：把这些方向的数记进五本票里（`nil` 不写 sz 是形状，不是漏读）
fn bor_tally(
    lines: &[BorLine],
    dirs: &mut serde_json::Map<String, Value>,
    vals: &mut serde_json::Map<String, Value>,
    szs: &mut serde_json::Map<String, Value>,
    spaces: &mut serde_json::Map<String, Value>,
    colors: &mut serde_json::Map<String, Value>,
    counts: &mut [usize; 4],
) {
    for one in lines.iter() {
        bump(dirs, one.name.as_str());
        let kind = match &one.got[0] {
            Some(had) => had.clone(),
            None => "(没写 val)".to_string(),
        };
        bump(vals, kind.as_str());
        if one.got[0].is_none() {
            counts[0] += 1;
        }
        match &one.got[1] {
            None => counts[1] += 1,
            Some(had) => bump(szs, had.as_str()),
        }
        if let Some(had) = &one.got[2] {
            bump(spaces, had.as_str());
        }
        if let Some(had) = &one.got[3] {
            let mut hit = had.clone();
            hit.make_ascii_lowercase();
            bump(colors, hit.as_str());
            if hit == "auto" {
                counts[2] += 1;
            }
        }
        if one.got[4].is_some() {
            counts[3] += 1;
        }
    }
}

/// docx / docm：表级一块 + 每格一块，两个住处分开数
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/") && one.ends_with(".xml") && !one.ends_with(".rels"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut cell_rows: Vec<Value> = Vec::new();
    let mut parts_scanned = 0usize;
    let mut tables = 0usize;
    let mut with_block = 0usize;
    let mut without_block = 0usize;
    let mut shell = 0usize;
    let mut cells_total = 0usize;
    let mut cells_with = 0usize;
    let mut cells_shell = 0usize;
    let mut dirs = serde_json::Map::new();
    let mut vals = serde_json::Map::new();
    let mut szs = serde_json::Map::new();
    let mut spaces = serde_json::Map::new();
    let mut colors = serde_json::Map::new();
    // counts: 没写 val、没写 sz、color 是 auto、指了主题
    let mut counts = [0usize; 4];
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        let tables_here = root.descendants("tbl");
        if tables_here.is_empty() {
            continue;
        }
        parts_scanned += 1;
        for (index, tbl) in tables_here.iter().enumerate() {
            tables += 1;
            let holder = tbl.descendants("tblBorders").first().copied();
            let lines = match holder {
                None => Vec::new(),
                Some(had) => bor_block(had),
            };
            match holder {
                None => without_block += 1,
                Some(_) => {
                    if lines.is_empty() {
                        shell += 1;
                    } else {
                        with_block += 1;
                    }
                }
            }
            bor_tally(
                &lines,
                &mut dirs,
                &mut vals,
                &mut szs,
                &mut spaces,
                &mut colors,
                &mut counts,
            );
            let wrote = holder.is_some();
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "table": index,
                    "block_present": wrote,
                    "dirs": bor_dirs(&lines),
                    "values": bor_rows(&lines),
                    "shell": wrote && lines.is_empty(),
                }));
            }
            let cells = tbl.descendants("tc");
            cells_total += cells.len();
            for (cell_index, cell) in cells.iter().enumerate() {
                let mine = match cell.descendants("tcBorders").first().copied() {
                    Some(had) => had,
                    None => continue,
                };
                cells_with += 1;
                let clines = bor_block(mine);
                if clines.is_empty() {
                    cells_shell += 1;
                }
                bor_tally(
                    &clines,
                    &mut dirs,
                    &mut vals,
                    &mut szs,
                    &mut spaces,
                    &mut colors,
                    &mut counts,
                );
                if cell_rows.len() < limit {
                    cell_rows.push(json!({
                        "part": name.as_str(),
                        "table": index,
                        "cell": cell_index,
                        "dirs": bor_dirs(&clines),
                        "values": bor_rows(&clines),
                        "shell": clines.is_empty(),
                    }));
                }
            }
        }
    }
    let mut st_tbl = 0usize;
    let mut st_tc = 0usize;
    if let Some(root) = parse_member(bytes, "word/styles.xml") {
        st_tbl = root.descendants("tblBorders").len();
        st_tc = root.descendants("tcBorders").len();
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": parts_scanned,
        "tables": tables,
        "tables_with_block": with_block,
        "tables_without_block": without_block,
        "tables_shell": shell,
        "cells_total": cells_total,
        "cells_with_block": cells_with,
        "cells_shell": cells_shell,
        "dirs": Value::Object(dirs),
        "vals": Value::Object(vals),
        "szs": Value::Object(szs),
        "spaces": Value::Object(spaces),
        "colors": Value::Object(colors),
        "auto_color": counts[2],
        "theme_pointed": counts[3],
        "no_val": counts[0],
        "no_sz": counts[1],
        "styles_part_tbl": st_tbl,
        "styles_part_tc": st_tc,
        "rows": rows,
        "cell_rows": cell_rows,
        "listed": rows.len(),
        "cell_listed": cell_rows.len(),
        "cut": tables > rows.len() || cells_with > cell_rows.len(),
    })
}

/// 一枚 `a:ln*`：四枚属性与孩子们的名字（填法是哪种、虚线预设是什么）
fn ppt_line(holder: &Node) -> Value {
    let mut out = serde_json::Map::new();
    for key in PPT_EDGE_ATTRS.iter() {
        let hit = match attr_of(holder, key) {
            Some(had) => json!(had),
            None => Value::Null,
        };
        out.insert((*key).to_string(), hit);
    }
    let mut fill: Option<String> = None;
    let mut dash: Option<String> = None;
    let mut head: Option<String> = None;
    let mut tail: Option<String> = None;
    let mut round = false;
    for kid in kids(holder) {
        let tag = kid.local().to_string();
        if fill.is_none() && tag.ends_with("Fill") {
            fill = Some(tag.clone());
        } else if tag == "prstDash" {
            dash = attr_of(kid, "val").map(|one| one.to_string());
        } else if tag == "round" {
            round = true;
        } else if tag == "headEnd" {
            head = attr_of(kid, "type").map(|one| one.to_string());
        } else if tag == "tailEnd" {
            tail = attr_of(kid, "type").map(|one| one.to_string());
        }
    }
    let nul = |mine: &Option<String>| match mine {
        Some(had) => json!(had.as_str()),
        None => Value::Null,
    };
    out.insert("fill".to_string(), nul(&fill));
    out.insert("dash".to_string(), nul(&dash));
    out.insert("round".to_string(), json!(round));
    out.insert("head".to_string(), nul(&head));
    out.insert("tail".to_string(), nul(&tail));
    Value::Object(out)
}

fn zero_str(raw: &str) -> bool {
    match raw.parse::<f64>() {
        Ok(had) => had == 0.0,
        Err(_) => false,
    }
}

/// pptx / pptm：线是 `a:tcPr` 的孩子，一枚线是一串而不是一枚属性
pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("ppt/slides/slide") && one.ends_with(".xml"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut parts_scanned = 0usize;
    let mut cells = 0usize;
    let mut with_edges = 0usize;
    let mut all_four = 0usize;
    let mut partial = 0usize;
    let mut silent = 0usize;
    let mut edges = serde_json::Map::new();
    let mut widths = serde_json::Map::new();
    let mut fills = serde_json::Map::new();
    let mut dashes = serde_json::Map::new();
    let mut zero_width = 0usize;
    let mut no_width = 0usize;
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        parts_scanned += 1;
        for (index, holder) in root.descendants("tcPr").iter().enumerate() {
            cells += 1;
            let mut order: Vec<String> = Vec::new();
            let mut lines = serde_json::Map::new();
            for kid in kids(holder) {
                if !PPT_EDGES.contains(&kid.local()) {
                    continue;
                }
                let mine = ppt_line(kid);
                order.push(kid.local().to_string());
                lines.insert(kid.local().to_string(), mine);
            }
            let four = PPT_FOUR
                .iter()
                .filter(|key| order.iter().any(|one| one == *key))
                .count();
            if order.is_empty() {
                silent += 1;
            } else {
                with_edges += 1;
                if four == 4 {
                    all_four += 1;
                } else {
                    partial += 1;
                }
            }
            for key in order.iter() {
                bump(&mut edges, key.as_str());
                let one = &lines[key];
                match one.get("w").and_then(|had| had.as_str()) {
                    None => no_width += 1,
                    Some(had) => {
                        bump(&mut widths, had);
                        if zero_str(had) {
                            zero_width += 1;
                        }
                    }
                }
                if let Some(had) = one.get("fill").and_then(|mine| mine.as_str()) {
                    bump(&mut fills, had);
                }
                if let Some(had) = one.get("dash").and_then(|mine| mine.as_str()) {
                    bump(&mut dashes, had);
                }
            }
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "cell": index,
                    "edges": order,
                    "lines": Value::Object(lines),
                    "four": four == 4,
                    "said": !order.is_empty(),
                }));
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": parts_scanned,
        "cells": cells,
        "cells_with_edges": with_edges,
        "cells_all_four": all_four,
        "cells_partial": partial,
        "cells_silent": silent,
        "edges": Value::Object(edges),
        "widths": Value::Object(widths),
        "fills": Value::Object(fills),
        "dashes": Value::Object(dashes),
        "zero_width": zero_width,
        "no_width": no_width,
        "rows": rows,
        "listed": rows.len(),
        "cut": cells > rows.len(),
    })
}

/// 一份 `family="table-cell"` 样式：边框住在哪枚孩子上、那几个串、第三种线宽写法
pub(crate) struct BorStyle {
    part: String,
    holder: Option<String>,
    borders: Vec<(String, String)>,
    widths: Vec<String>,
}

fn bor_split(raw: &str) -> Option<String> {
    let pieces: Vec<&str> = raw.split_whitespace().collect();
    if pieces.len() >= 2 {
        return Some(pieces[1].to_string());
    }
    if pieces.is_empty() {
        return None;
    }
    Some(pieces[0].to_string())
}

/// 一枚 properties 孩子上的边框属性（短款、长款都收；`border-line-width-*` 是第三种写法）
fn bor_attrs_of(node: &Node) -> (Vec<(String, String)>, Vec<String>) {
    let mut got: Vec<(String, String)> = Vec::new();
    let mut widths: Vec<String> = Vec::new();
    for (key, value) in node.attrs.iter() {
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        if local.starts_with("border-line-width") {
            widths.push(local);
            continue;
        }
        if local == "border" || local.starts_with("border-") {
            got.push((local, value.clone()));
        }
    }
    (got, widths)
}

/// 跨两份部件收 table-cell 样式（`content.xml` 优先，与 `cell_margins` 同一口径）
pub(crate) fn odf_styles(bytes: &[u8]) -> Vec<(String, BorStyle)> {
    let mut out: Vec<(String, BorStyle)> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("style") {
            if one.attr_local("family") != Some("table-cell") {
                continue;
            }
            let name = match one.attr_local("name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            if out.iter().any(|had: &(String, BorStyle)| had.0 == name) {
                continue;
            }
            let mut holder: Option<String> = None;
            let mut borders: Vec<(String, String)> = Vec::new();
            let mut widths: Vec<String> = Vec::new();
            for kid in kids(one) {
                if !ODF_BOR_PROPS.contains(&kid.local()) {
                    continue;
                }
                let (got, mine) = bor_attrs_of(kid);
                if got.is_empty() && mine.is_empty() {
                    continue;
                }
                if !got.is_empty() {
                    holder = Some(kid.local().to_string());
                }
                for (key, value) in got.into_iter() {
                    match borders.iter_mut().find(|one| one.0 == key) {
                        Some(hit) => hit.1 = value,
                        None => borders.push((key, value)),
                    }
                }
                widths.extend(mine.into_iter());
            }
            out.push((
                name,
                BorStyle {
                    part: part.to_string(),
                    holder,
                    borders,
                    widths,
                },
            ));
        }
    }
    out
}

/// `style:.../@border-model` 那一份账：哪枚孩子上写的、写的是哪个值
fn border_models(bytes: &[u8]) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        let mut all: Vec<&Node> = Vec::new();
        every(&root, &mut all);
        for one in all.iter() {
            for (key, value) in one.attrs.iter() {
                let local = key.rsplit(':').next().unwrap_or(key);
                if local == "border-model" {
                    let hit = format!("{}/{}", one.local(), value);
                    bump(&mut out, hit.as_str());
                }
            }
        }
    }
    out
}

/// odt / odp：格点的名 → 那份样式的边框（短款、四长款、第三种线宽）
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let styles = odf_styles(bytes);
    let models = border_models(bytes);
    let root = match parse_member(bytes, "content.xml") {
        Some(one) => one,
        None => {
            return json!({
                "family": "odf", "available": true, "cells": 0, "cells_named": 0,
                "cells_unnamed": 0, "styles_found": 0, "styles_unfound": 0,
                "cells_with_borders": 0, "shorthand": 0, "longhand": 0, "lines": 0,
                "holders": {}, "keys": {}, "kinds": {},
                "styles_defined": styles.len(), "border_models": Value::Object(models),
                "rows": [], "listed": 0, "cut": false,
            })
        }
    };
    let mut rows: Vec<Value> = Vec::new();
    let mut cells = 0usize;
    let mut named = 0usize;
    let mut unnamed = 0usize;
    let mut found = 0usize;
    let mut unfound = 0usize;
    let mut with_borders = 0usize;
    let mut shorthand = 0usize;
    let mut longhand = 0usize;
    let mut lines = 0usize;
    let mut holders = serde_json::Map::new();
    let mut keys = serde_json::Map::new();
    let mut kinds = serde_json::Map::new();
    let mut all: Vec<&Node> = Vec::new();
    every(&root, &mut all);
    let cells_here: Vec<&Node> = all
        .into_iter()
        .filter(|one| one.local() == "table-cell" || one.local() == "covered-table-cell")
        .collect();
    for one in cells_here.iter() {
        cells += 1;
        let want = one.attr_local("style-name");
        match want {
            None => unnamed += 1,
            Some(_) => named += 1,
        }
        let mine = want.and_then(|had| styles.iter().find(|one| one.0 == had).map(|hit| &hit.1));
        if want.is_some() && mine.is_none() {
            unfound += 1;
        }
        if mine.is_some() {
            found += 1;
        }
        let borders: Vec<(String, String)> = match mine {
            Some(had) => had.borders.clone(),
            None => Vec::new(),
        };
        let widths: Vec<String> = match mine {
            Some(had) => had.widths.clone(),
            None => Vec::new(),
        };
        let holder: Option<String> = match mine {
            Some(had) => had.holder.clone(),
            None => None,
        };
        let mut top = Value::Null;
        let mut bottom = Value::Null;
        let mut left = Value::Null;
        let mut right = Value::Null;
        let mut short = Value::Null;
        if !borders.is_empty() {
            with_borders += 1;
            bump(&mut holders, holder.clone().unwrap_or_default().as_str());
            for (key, value) in borders.iter() {
                bump(&mut keys, key.as_str());
                lines += 1;
                let kind = match bor_split(value.as_str()) {
                    Some(had) => had,
                    None => "(空)".to_string(),
                };
                bump(&mut kinds, kind.as_str());
                match key.as_str() {
                    "border" => short = json!(value.as_str()),
                    "border-top" => top = json!(value.as_str()),
                    "border-bottom" => bottom = json!(value.as_str()),
                    "border-left" => left = json!(value.as_str()),
                    "border-right" => right = json!(value.as_str()),
                    _ => {}
                }
            }
            if !short.is_null() {
                shorthand += 1;
            }
            if borders
                .iter()
                .any(|one: &(String, String)| one.0.starts_with("border-"))
            {
                longhand += 1;
            }
        }
        for key in widths.iter() {
            bump(&mut keys, key.as_str());
        }
        if rows.len() < limit {
            rows.push(json!({
                "cell": cells - 1,
                "style_name": want,
                "style_found": mine.is_some(),
                "style_part": mine.map(|had| had.part.as_str()),
                "holder": holder,
                "shorthand": short,
                "top": top,
                "bottom": bottom,
                "left": left,
                "right": right,
                "written": borders.len(),
            }));
        }
    }
    json!({
        "family": "odf",
        "available": true,
        "cells": cells,
        "cells_named": named,
        "cells_unnamed": unnamed,
        "styles_found": found,
        "styles_unfound": unfound,
        "cells_with_borders": with_borders,
        "shorthand": shorthand,
        "longhand": longhand,
        "lines": lines,
        "holders": Value::Object(holders),
        "keys": Value::Object(keys),
        "kinds": Value::Object(kinds),
        "styles_defined": styles.len(),
        "border_models": Value::Object(models),
        "rows": rows,
        "listed": rows.len(),
        "cut": cells > rows.len(),
    })
}
