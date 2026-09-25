//! 「这一框对应版式里哪一条」那一跳：页上的 `p:ph/@idx` 对 `ppt/slideLayouts/*.xml` 里那条 `p:ph`
//!
//! 实测这一跳**会在重写里断**：python-pptx 给正文占位符写 `idx="1"`，LibreOffice 重写同一份
//! 时把它写成空元素 `<p:ph/>` —— 号没了、名也没有，于是那一格只能交「对不上」，不按规范的
//! 默认值（body）替它接回去。同一条规则对两份件给出 6 对 6 与 3 对 6，这个差就是这份账的来处。

use crate::opack::resolve_target;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 一条 `p:ph` 写着的四个属性：都按写的交，没写是 null（不替文件补规范默认值）
fn ph_written(one: &xmlscan::Node) -> Value {
    json!({
        "type": one.attr_local("type"),
        "idx": one.attr_local("idx"),
        "sz": one.attr_local("sz"),
        "orient": one.attr_local("orient"),
    })
}

/// 版式里那几条 `p:ph` 的账本（按文档顺序，包括套在组合形状里的那些）
fn layout_placeholders(node: &xmlscan::Node) -> Vec<Value> {
    node.descendants("ph")
        .iter()
        .map(|one| ph_written(one))
        .collect()
}

/// 一条形状：它自己写没写 `p:ph`、写了什么号、对到了版式里哪一条
fn shape_rows(sp: &xmlscan::Node, mine: &[Value]) -> Value {
    let holder = sp.descendants("ph").first().cloned();
    let name = sp
        .descendants("cNvPr")
        .first()
        .and_then(|one| one.attr_local("name"))
        .map(String::from);
    let wrote = holder.is_some();
    let idx = holder
        .and_then(|one| one.attr_local("idx"))
        .map(String::from);
    let kind = match holder {
        Some(one) => one.attr_local("type").map(String::from),
        None => None,
    };
    // 号写了按号对；号没写就按名对；名与号都没写，就只能对「版式里那一条同样什么都没写」——
    // 三种都是**按写的比**，不比的是规范里那句默认值
    let matched: Option<Value> = match (&idx, &kind) {
        (Some(raw), _) => mine
            .iter()
            .find(|one| one["idx"].as_str() == Some(raw.as_str()))
            .cloned(),
        (None, Some(raw)) => mine
            .iter()
            .find(|one| one["idx"].is_null() && one["type"].as_str() == Some(raw.as_str()))
            .cloned(),
        (None, None) => mine
            .iter()
            .find(|one| one["idx"].is_null() && one["type"].is_null())
            .cloned(),
    };
    json!({
        "name": name,
        "ph_element": wrote,
        "type_written": kind,
        "idx_written": idx,
        "layout_matched": matched,
        "hop": if !wrote {
            "no_ph"
        } else {
            match &idx {
                Some(_) => "by_idx",
                None => "by_type",
            }
        },
    })
}

/// 整份账：逐页交「页点哪份版式、那份版式写了哪几条 `p:ph`、这一页每个形状对上了哪一条」
pub(crate) fn hops(bytes: &[u8], limit: usize) -> Value {
    let names = zipread::member_names(bytes);
    let mut slides: Vec<String> = names
        .iter()
        .filter(|one| one.starts_with("ppt/slides/slide") && one.ends_with(".xml"))
        .cloned()
        .collect();
    slides.sort();
    let parse = |name: &str| -> Option<xmlscan::Node> {
        zipread::member(bytes, name, DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| xmlscan::parse_str(&one.as_text()))
    };
    let mut rows: Vec<Value> = Vec::new();
    let mut shape_total = 0usize;
    let mut with_ph = 0usize;
    let mut found = 0usize;
    let mut missing = 0usize;
    let mut no_idx = 0usize;
    for part in slides {
        let file = part.rsplit('/').next().unwrap_or_default().to_string();
        // 页点哪份版式要走页自己那张关系表：成员名是 `ppt/slides/_rels/slide1.xml.rels`
        // （部件名 + `.rels`，少这一段就永远读不到，而 `zipread::member` 按名字精确匹配）
        let rel_name = format!("ppt/slides/_rels/{}.rels", file);
        let mut layout: Option<String> = None;
        if let Some(root) = parse(&rel_name) {
            for one in root.descendants("Relationship") {
                if one
                    .attr_local("Type")
                    .unwrap_or_default()
                    .ends_with("/slideLayout")
                {
                    layout = Some(resolve_target(
                        "ppt/slides",
                        one.attr_local("Target").unwrap_or_default(),
                    ));
                    break;
                }
            }
        }
        let mine = layout
            .as_deref()
            .and_then(|name| parse(name))
            .map(|root| layout_placeholders(&root))
            .unwrap_or_default();
        let mut shapes: Vec<Value> = Vec::new();
        if let Some(root) = parse(&part) {
            for sp in root.descendants("sp") {
                let one = shape_rows(sp, &mine);
                shape_total += 1;
                if one["ph_element"].as_bool() == Some(true) {
                    with_ph += 1;
                    if one["idx_written"].is_null() {
                        no_idx += 1;
                    }
                    if one["layout_matched"].is_null() {
                        missing += 1;
                    } else {
                        found += 1;
                    }
                }
                shapes.push(one);
            }
        }
        rows.push(json!({
            "part": part,
            "layout_part": layout.clone(),
            "layout_found": layout.as_ref().is_some_and(|name| names.iter().any(|had| had == name)),
            "layout_placeholders": mine,
            "shapes": shapes,
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "slide_total": rows.len(),
        "shape_total": shape_total,
        "with_ph": with_ph,
        "hop_found": found,
        "hop_missing": missing,
        "no_idx_written": no_idx,
        "slides": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
