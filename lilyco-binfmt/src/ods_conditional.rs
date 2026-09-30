//! ODF 那一族的「条件格式」：它不写在表上，也不叫 `style:conditional`
//!
//! 量到的形状（LibreOffice 把带 8 枚 `cfRule` 的 `rules-lo.xlsx` 转成 `.ods`）：条件写在
//! **单元格样式身上**的一条 `<style:map>` 里，`style:condition` 用 `cell-content()` 那个函数，
//! 「满足之后长什么样」不在同一条元素里，而是 `style:apply-style-name` 指到一条**具名 table-cell 样式**
//! （`ConditionalStyle_5f_1`，OOXML 那条 dxf 的 `FF9C0006` 穿过转格式落成它的 `fo:color="#9c0006"`），
//! 再另有一枚 `style:base-cell-address` 说这条规则是给哪个格子写的。整本 `<style:conditional>` 一个都没有。
//!
//! 同一枚元素名有两种语义，所以这本账按**父样式家族**分家，两本各数各的：住在
//! `number:number-style` 里的 `<style:map style:condition="value()&gt;=0">` 是数字格式的正负分支
//! （就是格式码里 `[>=0]` 那一层，本仓 17 份 .ods 每份都有 24 枚），与条件格式无关。
//! 判定两件事一起看：条件串的前缀（`cell-content()` 对 `value()`）与父样式的 `style:family`。

use crate::office_doc::kept_attrs;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 把一份部件里的 table-cell 样式扫一遍，收「身上挂着 cell-content() 条件」的那几条
fn cell_styles(root: &xmlscan::Node, part: &str, named: &Value, into: &mut Vec<Value>) {
    for style in root.descendants("style") {
        if style.attr_local("family").as_deref() != Some("table-cell") {
            continue;
        }
        let maps: Vec<Value> = style
            .descendants("map")
            .into_iter()
            .filter(|one| {
                one.attr_local("condition")
                    .unwrap_or_default()
                    .starts_with("cell-content")
            })
            .map(|one| {
                let want = one.attr_local("apply-style-name");
                let had = want
                    .as_deref()
                    .and_then(|name| named.as_object().and_then(|m| m.get(name)))
                    .cloned()
                    .unwrap_or(Value::Null);
                json!({
                    // 条件串按文件写的交：`&gt;` 解成 `>` 是 XML 自己的事，不改它的算法
                    "condition": one.attr_local("condition"),
                    "apply_style_name": want.clone(),
                    "base_cell_address": one.attr_local("base-cell-address"),
                    "written": kept_attrs(&one),
                    // 「then」那一副长相在别处：指得到就交那条样式自己写了什么，指不到交 null
                    "then": had,
                    "then_found": had != Value::Null,
                })
            })
            .collect();
        if maps.is_empty() {
            continue;
        }
        into.push(json!({
            "part": part,
            "style": style.attr_local("name"),
            "display_name": style.attr_local("display-name"),
            "parent_style_name": style.attr_local("parent-style-name"),
            "data_style_name": style.attr_local("data-style-name"),
            "written": kept_attrs(&style),
            "maps": maps,
        }));
    }
}

/// 具名的 table-cell 样式（两份部件都收）：条件指过去的那一条「then」样式在这里查
fn named_cell_styles(parts: &[(&str, xmlscan::Node)]) -> Value {
    let mut out = serde_json::Map::new();
    for (part, root) in parts {
        for style in root.descendants("style") {
            if style.attr_local("family").as_deref() != Some("table-cell") {
                continue;
            }
            let Some(name) = style.attr_local("name") else {
                continue;
            };
            if out.contains_key(name) {
                continue;
            }
            out.insert(
                name.to_string(),
                json!({
                    "part": part,
                    "display_name": style.attr_local("display-name"),
                    "parent_style_name": style.attr_local("parent-style-name"),
                    "written": kept_attrs(&style),
                }),
            );
        }
    }
    Value::Object(out)
}

/// ODS 的条件格式账：条数、两种 `<style:map>` 的分工、指得过去几条「then」样式
pub(crate) fn ods(bytes: &[u8], limit: usize) -> Value {
    let mut parts: Vec<(&str, xmlscan::Node)> = Vec::new();
    for name in ["content.xml", "styles.xml"] {
        if let Ok(member) = zipread::member(bytes, name, DEFAULT_MEMBER_CAP) {
            parts.push((name, xmlscan::parse_str(&member.as_text())));
        }
    }
    if parts.is_empty() {
        return json!({"available": false});
    }
    let named = named_cell_styles(&parts);
    let mut rows: Vec<Value> = Vec::new();
    let mut conditional_elements = 0usize;
    let mut other_maps = 0usize;
    for (part, root) in &parts {
        cell_styles(root, part, &named, &mut rows);
        conditional_elements += root.descendants("conditional").len();
        other_maps += root
            .descendants("map")
            .into_iter()
            .filter(|one| {
                !one.attr_local("condition")
                    .unwrap_or_default()
                    .starts_with("cell-content")
            })
            .count();
    }
    let mut conditions: Vec<String> = rows
        .iter()
        .flat_map(|row| row["maps"].as_array().cloned().unwrap_or_default())
        .filter_map(|one| one["condition"].as_str().map(String::from))
        .collect();
    conditions.sort();
    conditions.dedup();
    let mut targets: Vec<String> = rows
        .iter()
        .flat_map(|row| row["maps"].as_array().cloned().unwrap_or_default())
        .filter_map(|one| one["apply_style_name"].as_str().map(String::from))
        .collect();
    targets.sort();
    targets.dedup();
    let maps_total: usize = rows
        .iter()
        .map(|row| row["maps"].as_array().map(Vec::len).unwrap_or(0))
        .sum();
    let resolved = rows
        .iter()
        .flat_map(|row| row["maps"].as_array().cloned().unwrap_or_default())
        .filter(|one| one["then_found"].as_bool() == Some(true))
        .count();
    json!({
        "family": "odf",
        "available": true,
        // 条件写在哪一层、整本那一种一个也没有 —— 都按看到的交
        "spelling": "style:map",
        "conditional_elements": conditional_elements,
        "styles_with_conditions": rows.len(),
        "maps_total": maps_total,
        "number_format_maps": other_maps,
        "distinct_conditions": conditions.len(),
        "distinct_apply_styles": targets.len(),
        "then_resolved": resolved,
        "then_unresolved": maps_total - resolved,
        "entries": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
