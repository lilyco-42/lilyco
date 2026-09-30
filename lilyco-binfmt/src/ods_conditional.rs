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
//!
//! 还有**第三种存法**在这一族，元素名整个是另一个：`table:conditional-format`（本仓 20 份 .ods 里
//! 只有转出来的 `rules.ods` 写了 3 条）。区间写在 `@target-range-address` 一枚属性里，而**一条可以
//! 塞好几段**（空格分隔：`规则.A2:规则.A6 规则.B2:规则.B4`），所以原样串与分词后的段数都交。
//! 里面挂的孩子有三种词汇，各按各的交、不折成「条件」：`table:condition`（`@value` 两种写法实测都出现 ——
//! 比较式 `&gt;100` 与 `formula-is([.$B2]&gt;200)`，都带 `@base-cell-address`）、`table:icon-set`
//! （`@icon-set-type="3Arrows"` 带三枚 `table:formatting-entry`）、`table:color-scale`
//! （三枚 `table:color-scale-entry`）。
//!
//! 名表因此要两本：`@apply-style-name` 在用户规则上点 `style:style`（`ConditionalStyle_5f_1` 解得到），
//! 而在那 24 条老 map 上点的其实是 `number:*-style` 的名字（`N116P0` 那一类，24/24 都在这本里）——
//! 只建样式名表就会把 24 条全报成「点不到」，那是读法错而不是文件缺。反过来，`rules.ods` 这 3 条新写法的
//! `ConditionalStyle_1` / `ConditionalStyle_2` **两本都解不到**（同一件里老 map 用的是
//! `ConditionalStyle_5f_1` 那个名字），按写的交 null，不替它猜一个「应该是指那条」。

use crate::office_doc::kept_attrs;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

/// 第二本名表：`number:*-style` 的名字。那 24 条老 map 的 `apply-style-name` 点的其实是这一本
/// （`N116P0` 那一类 = 数字格式的正负分支），只建样式名表就会把它们全报成「点不到」。
fn number_style_names(parts: &[(&str, xmlscan::Node)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in [
        "number-style",
        "date-style",
        "time-style",
        "currency-style",
        "text-style",
    ] {
        for (_, root) in parts {
            for one in root.descendants(tag) {
                if let Some(raw) = one.attr_local("name") {
                    out.push(raw.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 一条规则点的名字落在哪本名表里（两本都可能，解不到交 null 而不是替它补一个）
fn resolves_as(name: Option<&str>, styles: &Value, numbers: &[String]) -> Value {
    let Some(want) = name else {
        return Value::Null;
    };
    if styles
        .as_object()
        .map(|m| m.contains_key(want))
        .unwrap_or(false)
    {
        json!("style")
    } else if numbers.iter().any(|one| one == want) {
        json!("number-style")
    } else {
        Value::Null
    }
}

/// 新写法：`table:conditional-format`（元素名整个是另一个，本仓只有转出来的 `rules.ods` 写了）
///
/// 区间写在 `@target-range-address` 一枚属性里，**一条可以塞好几段**（空格分隔），
/// 所以原样串与分词后的段数都交；里面挂的孩子有三种词汇，各按各的交，不折成「条件」。
fn conditional_formats(
    parts: &[(&str, xmlscan::Node)],
    styles: &Value,
    numbers: &[String],
    kinds: &mut BTreeMap<String, u64>,
) -> (Vec<Value>, usize, usize, usize) {
    let mut rows: Vec<Value> = Vec::new();
    let mut multi = 0usize;
    let mut ranges_total = 0usize;
    let mut entries_resolved = 0usize;
    for (part, root) in parts {
        for one in root.descendants("conditional-format") {
            let written = kept_attrs(&one);
            let raw = one.attr_local("target-range-address");
            let ranges: Vec<&str> = raw
                .unwrap_or_default()
                .split(' ')
                .filter(|had| !had.is_empty())
                .collect();
            if ranges.len() > 1 {
                multi += 1;
            }
            ranges_total += ranges.len();
            let entries: Vec<Value> = one
                .all("condition")
                .into_iter()
                .map(|had| {
                    let mine = resolves_as(had.attr_local("apply-style-name"), styles, numbers);
                    if mine != Value::Null {
                        entries_resolved += 1;
                    }
                    json!({
                        "apply_style_name": had.attr_local("apply-style-name"),
                        "resolved_as": mine,
                        "value": had.attr_local("value"),
                        "base_cell_address": had.attr_local("base-cell-address"),
                        "written": kept_attrs(&had),
                    })
                })
                .collect();
            let inner: Vec<Value> = one
                .children
                .iter()
                .map(|had| {
                    kinds.entry(had.local().to_string()).or_insert(0) += 1;
                    json!({
                        "element": had.local(),
                        "written": kept_attrs(had),
                        "children": had
                            .children
                            .iter()
                            .map(|kid| json!({
                                "element": kid.local(),
                                "written": kept_attrs(kid),
                            }))
                            .collect::<Vec<Value>>(),
                    })
                })
                .collect();
            rows.push(json!({
                "part": part,
                "written": written,
                "target_range_address": raw,
                "ranges": ranges,
                "ranges_total": ranges.len(),
                "entries": entries,
                "inner": inner,
            }));
        }
    }
    (rows, multi, ranges_total, entries_resolved)
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
    let numbers = number_style_names(&parts);
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let (formats, multi, ranges_total, entries_resolved) =
        conditional_formats(&parts, &named, &numbers, &mut kinds);
    let mut rows: Vec<Value> = Vec::new();
    let mut conditional_elements = 0usize;
    let mut other_maps = 0usize;
    let mut number_maps_resolved = 0usize;
    for (part, root) in &parts {
        cell_styles(root, part, &named, &mut rows);
        conditional_elements += root.descendants("conditional").len();
        let mine: Vec<&xmlscan::Node> = root
            .descendants("map")
            .into_iter()
            .filter(|one| {
                !one.attr_local("condition")
                    .unwrap_or_default()
                    .starts_with("cell-content")
            })
            .collect();
        other_maps += mine.len();
        number_maps_resolved += mine
            .iter()
            .filter(|one| {
                resolves_as(one.attr_local("apply-style-name"), &named, &numbers) != Value::Null
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
        // 新写法那一本：区间是一枚属性里的多段，孩子有三种词汇，点的名可能两本都解不到
        "formats_total": formats.len(),
        "formats_with_multiple_ranges": multi,
        "ranges_total": ranges_total,
        "format_kinds": kinds,
        "format_entries": formats
            .iter()
            .map(|one| one["entries"].as_array().map(Vec::len).unwrap_or(0))
            .sum::<usize>(),
        "format_entries_resolved": entries_resolved,
        "number_style_names": numbers.len(),
        "number_maps_resolved": number_maps_resolved,
        "formats": formats.into_iter().take(limit).collect::<Vec<Value>>(),
        "entries": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
