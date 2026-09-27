//! 格子的字离格边多远 —— 四族四个住处，所以三份账分开交。
//!
//! OOXML 一份文档里这块有**两个形状一样、住处不同**的块：`w:tblPr/w:tblCellMar` 是表级默认，
//! `w:tcPr/w:tcMar` 是这一格自己改的；每一块都是若干枚方向孩子（`top` / `left` / `bottom` /
//! `right`，或双向安全那一对 `start` / `end`），每枚带两个属性：`@w:w`（twips 串）与
//! `@w:type`（`dxa` 才是数、`auto` 是「排版自己定」）。
//!
//! DrawingML 把同一问答在**格子自己**身上：`a:tcPr` 的四个属性 `@marL` / `@marR` / `@marT` /
//! `@marB`（EMU），一个都不写就是这一格没说。
//!
//! ODF 是一跳：格只写 `table:style-name`，数在那份 `family="table-cell"` 样式的 properties 上，
//! 而 odt 住 `style:table-cell-properties`、odp 住 `style:graphic-properties`（那一族的格是图形
//! 对象）；同一条数可以写成四枚长款，也可以写成一枚 `fo:padding` 短款。样式跨 `content.xml`
//! 与 `styles.xml` 两份部件找（与 odt 那几本同一口径）。
//!
//! 实测（`margins.docx` = python-docx 打底 + 按 ECMA 手写四张表；`margins.pptx` = python-pptx
//! 的 `cell.margin_*`；`margins.odt` / `margins.odp` = LibreOffice 转出去的那两份；
//! `margins-lo.docx` / `margins-lo.pptx` = LibreOffice 同格式重写）：
//! 1. **「表上没写」不等于「没有边距」**：`word/styles.xml` 里恒有 100 枚 `w:tblCellMar`
//!    （python-docx 那张打底模板的表格样式各带一份），所以这份账把「正文里写没写」与
//!    「样式表里有多少」分开交；
//! 2. LibreOffice 重写同一份 docx 时把方向**改名**（`left/right` → `start/end`）、把四张表
//!    **并成一张**（`tables` 4 → 1）、给 11 个格各自补上四条（`cells_with_mar` 4 → 11），
//!    还把表级那四条换成第一个格写过的值 —— 块的对调是它的算法，本仓只按文件记下来的数交；
//!    而 `w:type="auto"` 那一条被换成 `dxa`（`types` 里 `auto` 从 1 变 0）；
//! 3. python-pptx 只写被设过的那个属性（有一种只写着 `marL`，也有一种四枚全没有），
//!    LibreOffice 重写时给每格补齐四枚，并把 `36576` 绕一圈换成 `36360`（`zero` 两本都是 5）；
//! 4. ODF 那两个数不是一回事：odt 那一份 12 格里 11 份样式写四枚长款、**全零那一条被 LibreOffice
//!    收成短款** `fo:padding="0cm"`（`shorthand` 1 / `longhand` 11），而 odp 的同一族
//!    住在 `style:graphic-properties` 上且六格里有两格**连样式名都不点**；
//! 5. twips 绕成厘米是有损的：`113` → `0.199cm`、`57` → `0.101cm`、`170` → `0.3cm`，
//!    本仓不换算也不比对，两族各交自己那个串（与 `row_heights` 同一处理）；
//! 6. 真件普查（本机 32 份 .docx + 1 份 .docm、读得动的 268 个 `word/*.xml` 部件；
//!    104 份 pptx、940 个 slide 部件）：
//!    docx 里表级块 3180 枚、格级块 232 枚，两边的**序不一样**（表级 `top,left,bottom,right`
//!    3143 枚、格级 `top,bottom,left,right` 232 枚），另有 28 枚只写左右、9 枚写三条；
//!    13583 个方向条目**全部**带 `@w:w` 与 `@w:type`，`type` 恒为 `dxa`（`auto` 0 条）、
//!    约一半的数是 `0`；pptx 里 893 枚 `a:tcPr` **每一枚都写满四个**、128 个值是 `0` ——
//!    「没写」这一形在真件里没有，只在自产件里有。
//!
//! 不做的事：**不换算单位**（twips / EMU / `0.199cm` 都按写的串交）、**不解样式表那 100 枚**
//! （表上没写时到底是哪一份样式给的，要看 `w:tblStyle` 那一跳，本仓不在这一格里猜）、
//! **不判四边是否相等**（那是渲染结论，不是文件写的一句话）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// 方向孩子的六个可能名字（`start` / `end` 是双向安全的那一对）
const MAR_DIRS: [&str; 6] = ["top", "left", "bottom", "right", "start", "end"];
/// `a:tcPr` 上那四个属性（按这个序数「写了哪几枚」）
const MAR_ATTRS: [&str; 4] = ["marL", "marR", "marT", "marB"];
/// ODF 里 padding 会住在的两枚 properties（odt 与 odp 各一）
const MAR_PROPS: [&str; 2] = ["table-cell-properties", "graphic-properties"];
/// 单位尾巴：判「零」要先剥掉它（`0.3cm` 不是零）
const MAR_UNITS: [&str; 6] = ["cm", "pt", "inch", "mm", "pc", "in"];

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

/// `0` / `0cm` / `0.0pt` 才是零；`0.3cm` 不是（判前缀会把 0.3 当 0）
fn mar_zero(raw: &str) -> bool {
    let mut body = raw.trim();
    for tail in MAR_UNITS.iter() {
        if let Some(head) = body.strip_suffix(*tail) {
            body = head;
            break;
        }
    }
    match body.parse::<f64>() {
        Ok(had) => had == 0.0,
        Err(_) => body == "0",
    }
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_i64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

fn mar_values(values: &[(String, String, String)]) -> Value {
    let mut out = serde_json::Map::new();
    for (name, w, kind) in values.iter() {
        out.insert(
            name.clone(),
            json!({
                "w": if w.is_empty() { Value::Null } else { json!(w) },
                "type": if kind == "(没写 type)" { Value::Null } else { json!(kind) },
            }),
        );
    }
    Value::Object(out)
}

/// 表级与格级同一段代码：交出方向序、每方向的两个属性，并把这些数记进票里
fn mar_block(
    holder: &Node,
    dirs: &mut Vec<String>,
    values: &mut Vec<(String, String, String)>,
    types: &mut serde_json::Map<String, Value>,
    dirs_seen: &mut serde_json::Map<String, Value>,
    zeros: &mut usize,
    nonzeros: &mut usize,
    missing: &mut usize,
) {
    for kid in kids(holder) {
        if !MAR_DIRS.contains(&kid.local()) {
            continue;
        }
        let name = kid.local().to_string();
        let w = attr_of(kid, "w");
        let t = attr_of(kid, "type");
        dirs.push(name.clone());
        values.push((
            name.clone(),
            w.unwrap_or("").to_string(),
            t.unwrap_or("(没写 type)").to_string(),
        ));
        bump(dirs_seen, name.as_str());
        bump(types, t.unwrap_or("(没写 type)"));
        match w {
            None => *missing += 1,
            Some(had) if mar_zero(had) => *zeros += 1,
            Some(_) => *nonzeros += 1,
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
    let mut with_mar = 0usize;
    let mut without_mar = 0usize;
    let mut shell = 0usize;
    let mut cells_total = 0usize;
    let mut cells_with = 0usize;
    let mut cells_shell = 0usize;
    let mut dirs_seen = serde_json::Map::new();
    let mut types = serde_json::Map::new();
    let mut zeros = 0usize;
    let mut nonzeros = 0usize;
    let mut missing = 0usize;
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
            let holder = tbl.descendants("tblCellMar").first().copied();
            let mut dirs: Vec<String> = Vec::new();
            let mut values: Vec<(String, String, String)> = Vec::new();
            match holder {
                None => without_mar += 1,
                Some(had) => {
                    mar_block(
                        had,
                        &mut dirs,
                        &mut values,
                        &mut types,
                        &mut dirs_seen,
                        &mut zeros,
                        &mut nonzeros,
                        &mut missing,
                    );
                }
            }
            let wrote = holder.is_some();
            if wrote && dirs.is_empty() {
                shell += 1;
            } else if wrote {
                with_mar += 1;
            }
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "table": index,
                    "mar_present": wrote,
                    "dirs": dirs,
                    "values": mar_values(&values),
                    "shell": wrote && dirs.is_empty(),
                }));
            }
            let cells = tbl.descendants("tc");
            cells_total += cells.len();
            for (cell_index, cell) in cells.iter().enumerate() {
                let mine = match cell.descendants("tcMar").first().copied() {
                    Some(had) => had,
                    None => continue,
                };
                cells_with += 1;
                let mut cdirs: Vec<String> = Vec::new();
                let mut cvalues: Vec<(String, String, String)> = Vec::new();
                mar_block(
                    mine,
                    &mut cdirs,
                    &mut cvalues,
                    &mut types,
                    &mut dirs_seen,
                    &mut zeros,
                    &mut nonzeros,
                    &mut missing,
                );
                if cdirs.is_empty() {
                    cells_shell += 1;
                }
                if cell_rows.len() < limit {
                    cell_rows.push(json!({
                        "part": name.as_str(),
                        "table": index,
                        "cell": cell_index,
                        "dirs": cdirs,
                        "values": mar_values(&cvalues),
                        "shell": cdirs.is_empty(),
                    }));
                }
            }
        }
    }
    let style_mar = match parse_member(bytes, "word/styles.xml") {
        Some(root) => root.descendants("tblCellMar").len(),
        None => 0usize,
    };
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": parts_scanned,
        "tables": tables,
        "tables_with_mar": with_mar,
        "tables_without_mar": without_mar,
        "tables_shell": shell,
        "cells_total": cells_total,
        "cells_with_mar": cells_with,
        "cells_shell": cells_shell,
        "dirs": Value::Object(dirs_seen),
        "types": Value::Object(types),
        "zero": zeros,
        "nonzero": nonzeros,
        "missing_w": missing,
        "styles_part_mar": style_mar,
        "rows": rows,
        "cell_rows": cell_rows,
        "listed": rows.len(),
        "cell_listed": cell_rows.len(),
        "cut": tables > rows.len() || cells_with > cell_rows.len(),
    })
}

/// pptx / pptm：`a:tcPr` 上那四个属性，按 marL / marR / marT / marB 的固定序数「写了哪几枚」
pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("ppt/slides/slide") && one.ends_with(".xml"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut parts_scanned = 0usize;
    let mut cells = 0usize;
    let mut all_four = 0usize;
    let mut partial = 0usize;
    let mut silent = 0usize;
    let mut seen = serde_json::Map::new();
    let mut zeros = 0usize;
    let mut nonzeros = 0usize;
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        parts_scanned += 1;
        for (index, holder) in root.descendants("tcPr").iter().enumerate() {
            cells += 1;
            let mut written: Vec<String> = Vec::new();
            let mut got: Vec<Value> = Vec::new();
            for key in MAR_ATTRS.iter() {
                let mine = holder.attr_local(key);
                got.push(match mine {
                    Some(had) => json!(had),
                    None => Value::Null,
                });
                if let Some(had) = mine {
                    written.push((*key).to_string());
                    bump(&mut seen, key);
                    if mar_zero(had) {
                        zeros += 1;
                    } else {
                        nonzeros += 1;
                    }
                }
            }
            match written.len() {
                0 => silent += 1,
                4 => all_four += 1,
                _ => partial += 1,
            }
            let all = written.len() == 4;
            let said = !written.is_empty();
            if rows.len() < limit {
                rows.push(json!({
                    "part": name.as_str(),
                    "cell": index,
                    "written": written,
                    "mar_l": got[0],
                    "mar_r": got[1],
                    "mar_t": got[2],
                    "mar_b": got[3],
                    "all_four": all,
                    "said": said,
                }));
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": parts_scanned,
        "cells": cells,
        "cells_all_four": all_four,
        "cells_partial": partial,
        "cells_silent": silent,
        "attrs_seen": Value::Object(seen),
        "zero": zeros,
        "nonzero": nonzeros,
        "rows": rows,
        "listed": rows.len(),
        "cut": cells > rows.len(),
    })
}

/// 一份 `family="table-cell"` 样式：padding 住在哪枚孩子上、那几个数按写的交
pub(crate) struct MarStyle {
    part: String,
    holder: Option<String>,
    pads: Vec<(String, String)>,
}

/// 一枚 properties 孩子上带 padding 的属性（局部名，与第二读者同一口径）
fn pad_attrs(node: &Node) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, value) in node.attrs.iter() {
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        if local.contains("padding") {
            out.push((local, value.clone()));
        }
    }
    out
}

/// 一份样式 → padding 住在哪枚孩子上 + 那几个数（两枚都有就按后写的覆盖，与 dict.update 一样）
fn pads_of(style: &Node) -> (Option<String>, Vec<(String, String)>) {
    let mut holder: Option<String> = None;
    let mut out: Vec<(String, String)> = Vec::new();
    for kid in kids(style) {
        if !MAR_PROPS.contains(&kid.local()) {
            continue;
        }
        let got = pad_attrs(kid);
        if got.is_empty() {
            continue;
        }
        holder = Some(kid.local().to_string());
        for (key, value) in got.into_iter() {
            match out.iter_mut().find(|one| one.0 == key) {
                Some(hit) => hit.1 = value,
                None => out.push((key, value)),
            }
        }
    }
    (holder, out)
}

/// 跨两份部件收 table-cell 样式（`content.xml` 优先，与 odt 那几本同一口径）
pub(crate) fn odf_styles(bytes: &[u8]) -> Vec<(String, MarStyle)> {
    let mut out: Vec<(String, MarStyle)> = Vec::new();
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
            if out.iter().any(|had: &(String, MarStyle)| had.0 == name) {
                continue;
            }
            let (holder, pads) = pads_of(one);
            out.push((
                name,
                MarStyle {
                    part: part.to_string(),
                    holder,
                    pads,
                },
            ));
        }
    }
    out
}

/// odt / odp：格点的名 → 那份样式里的 padding（四长款或一枚短款）
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let styles = odf_styles(bytes);
    let root = match parse_member(bytes, "content.xml") {
        Some(one) => one,
        None => {
            return json!({
                "family": "odf", "available": false, "cells": 0, "cells_named": 0,
                "cells_unnamed": 0, "styles_found": 0, "styles_unfound": 0,
                "cells_with_pads": 0, "shorthand": 0, "longhand": 0, "holders": {},
                "keys": {}, "styles_defined": styles.len(), "zero": 0, "nonzero": 0,
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
    let mut with_pads = 0usize;
    let mut shorthand = 0usize;
    let mut longhand = 0usize;
    let mut holders = serde_json::Map::new();
    let mut keys = serde_json::Map::new();
    let mut zeros = 0usize;
    let mut nonzeros = 0usize;
    for one in root.descendants("table-cell").iter() {
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
        let pads: Vec<(String, String)> = mine.map(|had| had.pads.clone()).unwrap_or_default();
        let mut top = Value::Null;
        let mut bottom = Value::Null;
        let mut left = Value::Null;
        let mut right = Value::Null;
        let mut short = Value::Null;
        if !pads.is_empty() {
            with_pads += 1;
            bump(
                &mut holders,
                mine.and_then(|had| had.holder.as_ref().map(|one| one.as_str()))
                    .unwrap_or(""),
            );
            for (key, value) in pads.iter() {
                bump(&mut keys, key.as_str());
                if mar_zero(value.as_str()) {
                    zeros += 1;
                } else {
                    nonzeros += 1;
                }
                match key.as_str() {
                    "padding" => short = json!(value.as_str()),
                    "padding-top" => top = json!(value.as_str()),
                    "padding-bottom" => bottom = json!(value.as_str()),
                    "padding-left" => left = json!(value.as_str()),
                    "padding-right" => right = json!(value.as_str()),
                    _ => {}
                }
            }
            if !short.is_null() {
                shorthand += 1;
            }
            if pads
                .iter()
                .any(|one: &(String, String)| one.0.starts_with("padding-"))
            {
                longhand += 1;
            }
        }
        if rows.len() < limit {
            rows.push(json!({
                "cell": cells - 1,
                "style_name": want,
                "style_found": mine.is_some(),
                "style_part": mine.map(|had| had.part.as_str()),
                "holder": mine.and_then(|had| had.holder.clone()),
                "shorthand": short,
                "top": top,
                "bottom": bottom,
                "left": left,
                "right": right,
                "written": pads.len(),
            }));
        }
    }
    let total = cells;
    json!({
        "family": "odf",
        "available": true,
        "cells": total,
        "cells_named": named,
        "cells_unnamed": unnamed,
        "styles_found": found,
        "styles_unfound": unfound,
        "cells_with_pads": with_pads,
        "shorthand": shorthand,
        "longhand": longhand,
        "holders": Value::Object(holders),
        "keys": Value::Object(keys),
        "styles_defined": styles.len(),
        "zero": zeros,
        "nonzero": nonzeros,
        "rows": rows,
        "listed": rows.len(),
        "cut": total > rows.len(),
    })
}
