//! 「这一段自己有没有说画个框、铺个底」——一家的边框与底纹是段身上两枚不同的元素，
//! 一家两样都在段点的那份样式上，而且一条 shorthand 就能把四条边说完
//!
//! OOXML 在 `w:pPr` 下面摆两枚元素：`w:pBdr` 是**装边的壳**（里面 `w:top` / `w:left` /
//! `w:bottom` / `w:right`（另有 `w:between`、`w:barrier`）各带自己的 `val` / `sz` / `space` /
//! `color`），`w:shd` 是底纹（`val` / `color` / `fill`，还可能点一个主题色）。壳可以在而里面
//! 一条边都没有 —— 那是文件说过的话，不能读成「没写边框」，所以 `border_element` 与 `edge_count`
//! 分开记。
//!
//! ODF 一跳在段点的那份段落样式上：`fo:border` 一条 shorthand 顶四条边（值里塞着「宽度 样式 颜色」
//! 三段），也可以四条各写一遍（`fo:border-top` …，其中「这一边没有」是**明写着 `none`** 的），
//! 底纹是 `fo:background-color`，而 docx 那个 `w:space`（边离字多远）在这一族搬成了 `fo:padding`。
//! 属性名**留着前缀交**（`fo:` 与 `style:` 是两个东西）。
//!
//! 实测五条（`pborder.docx` 由 python-docx 的 `OxmlElement` 手写五段，另两份是 LibreOffice 转的）：
//! 1. 「四边单线」与「只有上面一条双线」在 docx 是同一个壳里 4 条 / 1 条边；转成 ODF 后一条变
//!    `fo:border="0.74pt solid #ff0000"`（`sz="6"` 是 6/8 磅，那条 shorthand 里成了 0.74pt），
//!    另一条变成 `fo:border-top="6.75pt double #000000"` **配上三条明写的 `none`**，还多出一份
//!    `style:border-line-width-top`（双线每一根各多宽）；
//! 2. LibreOffice 重写自己那份 docx：`w:color="auto"` 被换成 `000000`（把「自动」折成一个具体色），
//!    而那个**空的 `w:pBdr` 整个没了** —— 在场与有内容在这一转里是两件事，两份读者各按写的交；
//! 3. 底纹那一枚最要紧：`w:val="clear"` 那一段（`color="auto"` + `fill="FFFF00"`）转成 ODF 是
//!    `#ffff00`，而 `w:val="solid"` 那一段（只有 `fill="00B050"`、另点了一个主题色）转过去成了
//!    `#ffffff` —— 同一个 `fill` 属性在两种 `val` 下不是同一个角色，读者不猜，两串都按写的交。
//!
//! RTF 那一族的段边框（`\brdrb` 这一族）住在样式表里而非段自己身上，与制表位、行距同一个坑：
//! 归属判不住，这一支不读。

use crate::office_doc::local_attrs;
use crate::xmlscan::Node;
use serde_json::{json, Value};

/// ODF 那一份要收的那几样（按局部名挑，交出去时前缀留着）
const ODF_BOX_LOCALS: [&str; 3] = ["border", "background-color", "padding"];

/// OOXML 那一份：`w:pBdr` 这个壳在不在、里面写了哪几条边，与 `w:shd` 那枚底纹
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut vals = serde_json::Map::new();
    let mut fills: Vec<String> = Vec::new();
    let mut p_pr = 0usize;
    let mut with_border = 0usize;
    let mut empty_border = 0usize;
    let mut edges_total = 0usize;
    let mut with_shading = 0usize;
    for (index, para) in body.descendants("p").iter().enumerate() {
        let holder = para.child("pPr");
        if holder.is_some() {
            p_pr += 1;
        }
        let border = holder.and_then(|had| had.children.iter().find(|kid| kid.local() == "pBdr"));
        let mut edges = serde_json::Map::new();
        if let Some(had) = border {
            for kid in had.children.iter() {
                edges.insert(kid.local().to_string(), local_attrs(kid));
            }
        }
        let shading = holder.and_then(|had| had.children.iter().find(|kid| kid.local() == "shd"));
        if let Some(had) = border {
            with_border += 1;
            if had.children.is_empty() {
                empty_border += 1;
            }
        }
        edges_total += edges.len();
        let edge_count = edges.len();
        if let Some(had) = shading {
            with_shading += 1;
            if let Some(raw) = had.attr_local("val") {
                let next = vals.get(raw).and_then(Value::as_u64).unwrap_or(0) + 1;
                vals.insert(raw.to_string(), json!(next));
            }
            if let Some(raw) = had.attr_local("fill") {
                if !fills.iter().any(|had| had == raw) {
                    fills.push(raw.to_string());
                }
            }
        }
        rows.push(json!({
            "index": index,
            "has_pPr": holder.is_some(),
            "border_element": border.is_some(),
            "edges": Value::Object(edges),
            "edge_count": edge_count,
            "shading_written": shading.is_some(),
            "shading": shading.map(local_attrs).unwrap_or(Value::Null),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs_total": rows.len(),
        "p_pr_elements": p_pr,
        "with_border_element": with_border,
        "border_element_empty": empty_border,
        "edges_total": edges_total,
        "with_shading": with_shading,
        "shading_vals": Value::Object(vals),
        "distinct_fills": fills.into_iter().map(Value::from).collect::<Vec<Value>>(),
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 那一份样式里挑出来的边、底与距离（名字按文件写的原样，前缀留着）
fn box_attrs(node: &Node) -> Vec<(String, String)> {
    let mut written: Vec<(String, String)> = Vec::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        let wanted = ODF_BOX_LOCALS.iter().any(|one| local == *one)
            || local.starts_with("border-")
            || local.starts_with("padding-");
        if wanted {
            written.push((key.clone(), value.clone()));
        }
    }
    written
}

/// ODF 那一份：段只点样式名，框与底都在那份样式的段落属性里（一跳）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut table: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for root in [Some(content), styles].into_iter().flatten() {
        for one in root.descendants("style").into_iter() {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            let held = one
                .child("paragraph-properties")
                .map(box_attrs)
                .unwrap_or_default();
            table.push((name.to_string(), held));
        }
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut resolved = 0usize;
    let mut with_shorthand = 0usize;
    let mut with_side = 0usize;
    let mut sides_written = 0usize;
    let mut sides_none = 0usize;
    let mut with_background = 0usize;
    for (index, para) in content.descendants("p").iter().enumerate() {
        let name = para.attr_local("style-name");
        let hit = name.and_then(|want| table.iter().find(|had| had.0 == *want));
        if hit.is_some() {
            resolved += 1;
        }
        let held: Vec<(String, String)> = hit.map(|had| had.1.clone()).unwrap_or_default();
        let mut shorthand: Option<String> = None;
        let mut background: Option<String> = None;
        let mut padding: Option<String> = None;
        let mut sides = serde_json::Map::new();
        let mut line_widths = serde_json::Map::new();
        for (key, value) in held.iter() {
            let local = key.rsplit(':').next().unwrap_or(key);
            if local == "border" {
                shorthand = Some(value.clone());
            } else if local == "background-color" {
                background = Some(value.clone());
            } else if local == "padding" {
                padding = Some(value.clone());
            } else if local.starts_with("border-line-width") {
                let rest = local
                    .trim_start_matches("border-line-width")
                    .trim_start_matches('-');
                line_widths.insert(rest.to_string(), json!(value));
            } else if local.starts_with("border-") {
                sides.insert(key.clone(), json!(value));
                sides_written += 1;
                if value == "none" {
                    sides_none += 1;
                }
            } else if local.starts_with("padding-") {
                sides.insert(key.clone(), json!(value));
            }
        }
        if shorthand.is_some() {
            with_shorthand += 1;
        }
        if !sides.is_empty() {
            with_side += 1;
        }
        if background.is_some() {
            with_background += 1;
        }
        rows.push(json!({
            "index": index,
            "style_written": name.map(String::from),
            "style_found": hit.is_some(),
            "border_shorthand": shorthand,
            "sides_written": Value::Object(sides),
            "background_written": background,
            "padding_written": padding,
            "line_widths": Value::Object(line_widths),
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "paragraphs_total": rows.len(),
        "styles_total": table.len(),
        "style_found_total": resolved,
        "with_shorthand": with_shorthand,
        "with_side_elements": with_side,
        "sides_written": sides_written,
        "sides_none": sides_none,
        "with_background": with_background,
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
