//! 主题样式表里那三本「填充 / 效果 / 线条」到底写了什么（`fmt_styles`，只在 OOXML 交）
//!
//! `theme` 那一本已经数得清 12 个色槽、两组字体角色与 `fmtScheme` 的三个列表**各几条**，
//! 但列表里面写了什么没说 —— 而这正是日常最常被问的一半：这份主题的渐变停在哪些位置、
//! 阴影是什么参数、第几号渐变走的是 `lin` 还是 `path`。
//!
//! 量到的形状（本仓 175 份带主题部件的 OOXML 件、239 份部件，openpyxl / python-pptx /
//! LibreOffice 三家都写；下列词汇由直接读 zip 的第三方脚本数出来，不是两份读者自证）：
//!
//! * `fillStyleLst` 每主题三条：一条 `solidFill`（里面 `schemeClr`）加两条 `gradFill`。
//!   渐变只有 `a:gsLst` + `a:lin`（304 枚渐变全是 `ang=16200000 scaled=0`，其中 126 枚额外带
//!   `a:tileRect`）—— 本仓**没有一条走 `a:path`**，所以 `path` / `path_shape` 常态是 null，
//!   而不是「没有渐变」。停止点 `a:gs/@pos` 只出现 `0` / `35000` / `80000` / `100000` 四种
//!   （万分之一），停止点下面的颜色本仓全是 `schemeClr`（`sysClr` / `srgbClr` 是同一词汇的
//!   其余两枚，这里没出现）—— 都按文件写的交，不折成「深/浅」。
//! * `effectStyleLst` 每主题三条，`a:effectStyle` 的孩子是 `a:effectLst`（openpyxl / python-pptx
//!   那份还会各带一枚 `a:scene3d` + `a:sp3d`，LibreOffice 重写后两枚都没了）。真正的效果在
//!   effectLst **里面**：本仓只有 `a:outerShdw`（239 份部件里 267 枚），写的是
//!   `blurRad` / `dist` / `dir` / `rotWithShape` 四枚（`algn` 一次都没写），它的孩子是那枚
//!   `srgbClr`。`a:innerShdw` / `a:glow` 一家都没写 —— 所以里层空着是「没写」而不是「读到 0」。
//! * `lnStyleLst` 每主题三条 `a:ln`，四枚属性 `w` / `cap` / `cmpd` / `algn` 每次都写满；
//!   里面的孩子三种组合：`solidFill`+`prstDash`（openpyxl/python-pptx 那份）、
//!   `prstDash`+`miter`（LibreOffice 那份）、只有 `prstDash`。子元素只报名与其自身属性，
//!   里面的填充色不替它猜默认值。
//!
//! ODF 那一族没有主题这个概念（样式表住在自己的 styles.xml 里），`.xls` / `.doc` / `.ppt`
//! 的主题在 CFB 的 `theme` 流里、RTF 是那一段 base64，本机都没有第二个读者能核对 ——
//! 所以这些族**不交这个键**，由 probe 那侧当反面凭据核对。

use crate::office_doc::local_attrs;
use crate::theme_ledger::is_theme_part;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一个元素的直接孩子：元素名 + 它自己写的属性（顺序按文件）
fn children_row(holder: &xmlscan::Node) -> Vec<Value> {
    holder
        .children
        .iter()
        .map(|one| json!({"element": one.local(), "written": local_attrs(one)}))
        .collect()
}

/// 渐变那串停止点：pos 按写的字符串，颜色只报它用的是哪一种指针元素与其属性
fn stops_of(holder: &xmlscan::Node) -> Vec<Value> {
    holder
        .all("gs")
        .into_iter()
        .map(|one| {
            let color = one.children.first();
            json!({
                "pos": one.attr_local("pos"),
                "written": local_attrs(one),
                "color_kind": color.map(|had| had.local().to_string()),
                "color_written": color.map(local_attrs).unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// 一条 `fillStyleLst` 的条目：本体写了什么、渐变停在哪、走 lin 还是 path
///
/// `gsLst` / `lin` / `path` 是**这个样式自己的**孩子（不是彼此的），所以三跳都从 `holder` 走。
fn fill_entry(index: usize, holder: &xmlscan::Node) -> Value {
    let stops = match holder.child("gsLst") {
        Some(list) => stops_of(&list),
        None => Vec::new(),
    };
    json!({
        "index": index,
        "element": holder.local(),
        "written": local_attrs(holder),
        "kind": holder.children.first().map(|one| one.local().to_string()),
        "stops": stops,
        "stop_total": stops.len(),
        "lin": holder.child("lin").map(local_attrs).unwrap_or(Value::Null),
        "path": holder.child("path").map(local_attrs).unwrap_or(Value::Null),
        "path_shape": holder
            .child("path")
            .and_then(|had| had.attr_local("path"))
            .map(String::from),
        "children": children_row(holder),
    })
}

/// 效果那一本：`a:effectStyle` 的孩子只有 `a:effectLst`（外加 scene3d / sp3d 两类 3D 壳），
/// 真正的效果（阴影那枚 `a:outerShdw`）在 effectLst **里面** —— 只数到 effectLst 就等于没答。
/// 所以里层那几枚连自己写的参数与其孩子一起交，3D 那两枚留在 `children` 里按原样报。
fn effect_entry(index: usize, holder: &xmlscan::Node) -> Value {
    let inner: Vec<Value> = match holder.child("effectLst") {
        Some(list) => list
            .children
            .iter()
            .map(|one| {
                json!({
                    "element": one.local(),
                    "written": local_attrs(one),
                    "children": children_row(one),
                })
            })
            .collect(),
        None => Vec::new(),
    };
    json!({
        "index": index,
        "element": holder.local(),
        "written": local_attrs(holder),
        "children": children_row(holder),
        "inner_effects": inner,
        "inner_total": inner.len(),
    })
}

/// 线条那一本：`a:ln` 自己写的属性与里面的元素名（里面的填充只报名，不猜默认值）
fn line_entry(index: usize, holder: &xmlscan::Node) -> Value {
    json!({
        "index": index,
        "element": holder.local(),
        "written": local_attrs(holder),
        "kind": holder.children.first().map(|one| one.local().to_string()),
        "children": children_row(holder),
    })
}

/// 一本样式表（fillStyleLst / effectStyleLst / lnStyleLst）里逐条摊开
fn list_rows(list: Option<&xmlscan::Node>, kind: &str) -> Vec<Value> {
    let Some(holder) = list else {
        return Vec::new();
    };
    holder
        .children
        .iter()
        .enumerate()
        .map(|(index, one)| match kind {
            "fill" => fill_entry(index, one),
            "effect" => effect_entry(index, one),
            _ => line_entry(index, one),
        })
        .collect()
}

/// 跨部件的几条计数：逐份累加，最后一起交（`Map<String, Value>` 上做加法要绕 as_u64，不如结构体）
#[derive(Default)]
struct Sums {
    parts: u64,
    unread: u64,
    fill_styles: u64,
    effect_styles: u64,
    line_styles: u64,
}

/// 一个主题部件的三本账
fn one_part(name: &str, raw: &[u8], sums: &mut Sums) -> Value {
    let parsed = xmlscan::parse(raw);
    let root = match parsed.children.first() {
        Some(had) => had,
        None => {
            sums.unread += 1;
            return json!({"part": name, "unread": true});
        }
    };
    // 三本列表住在 fmtScheme 底下（不在 themeElements 那一层）：这一跳错了就是三本空账
    let fmt = root
        .child("themeElements")
        .and_then(|had| had.child("fmtScheme"));
    let fills = list_rows(fmt.and_then(|had| had.child("fillStyleLst")), "fill");
    let effects = list_rows(fmt.and_then(|had| had.child("effectStyleLst")), "effect");
    let lines = list_rows(fmt.and_then(|had| had.child("lnStyleLst")), "line");
    let mut positions: Vec<String> = Vec::new();
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    for one in &fills {
        if let Some(raw) = one["kind"].as_str() {
            *kinds.entry(raw.to_string()).or_insert(0) += 1;
        }
        for stop in one["stops"].as_array().into_iter().flatten() {
            if let Value::String(raw) = &stop["pos"] {
                positions.push(raw.clone());
            }
        }
    }
    positions.sort();
    positions.dedup();
    let mut effect_child_kinds: BTreeMap<String, u64> = BTreeMap::new();
    for one in &effects {
        for had in one["children"].as_array().into_iter().flatten() {
            if let Value::String(raw) = &had["element"] {
                *effect_child_kinds.entry(raw.clone()).or_insert(0) += 1;
            }
        }
    }
    // 里层那几枚才是「这条样式有没有阴影」的答案
    let mut shadow_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut with_shadow = 0usize;
    for one in &effects {
        if let Some(list) = one["inner_effects"].as_array() {
            with_shadow += usize::from(!list.is_empty());
            for had in list {
                if let Value::String(raw) = &had["element"] {
                    *shadow_kinds.entry(raw.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    let mut path_shapes: BTreeMap<String, u64> = BTreeMap::new();
    for one in &fills {
        if let Value::String(raw) = &one["path_shape"] {
            *path_shapes.entry(raw.clone()).or_insert(0) += 1;
        }
    }
    sums.parts += 1;
    sums.fill_styles += fills.len() as u64;
    sums.effect_styles += effects.len() as u64;
    sums.line_styles += lines.len() as u64;
    json!({
        "part": name,
        "unread": false,
        "fills": fills,
        "effects": effects,
        "lines": lines,
        "stop_positions": positions,
        "fill_kinds": kinds,
        "effect_kinds": effect_child_kinds,
        "shadow_kinds": shadow_kinds,
        "styles_with_shadow": with_shadow,
        "path_shapes": path_shapes,
    })
}

/// 把一份部件某本 tally 并进总账
fn merge_tally(row: &Value, book: &str, into: &mut BTreeMap<String, u64>) {
    if let Some(map) = row[book].as_object() {
        for (key, value) in map {
            *into.entry(key.clone()).or_insert(0) += value.as_u64().unwrap_or(0);
        }
    }
}

/// 全家的账：按部件名排序，逐份交三本样式表
pub(crate) fn parts(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| is_theme_part(one))
        .collect();
    names.sort();
    let mut sums = Sums::default();
    let mut rows: Vec<Value> = Vec::new();
    for name in &names {
        if let Ok(member) = zipread::member(bytes, name, DEFAULT_MEMBER_CAP) {
            rows.push(one_part(name, &member.data, &mut sums));
        }
    }
    let mut positions: Vec<String> = Vec::new();
    let mut fill_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut effect_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut shadow_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut path_shapes: BTreeMap<String, u64> = BTreeMap::new();
    let mut with_shadow = 0u64;
    for one in &rows {
        for raw in one["stop_positions"].as_array().into_iter().flatten() {
            if let Value::String(had) = raw {
                positions.push(had.clone());
            }
        }
        merge_tally(one, "fill_kinds", &mut fill_kinds);
        merge_tally(one, "effect_kinds", &mut effect_kinds);
        merge_tally(one, "shadow_kinds", &mut shadow_kinds);
        merge_tally(one, "path_shapes", &mut path_shapes);
        with_shadow += one["styles_with_shadow"].as_u64().unwrap_or(0);
    }
    positions.sort();
    positions.dedup();
    json!({
        "family": "ooxml",
        "available": !names.is_empty(),
        "parts_total": names.len(),
        "unread": sums.unread,
        "fill_styles": sums.fill_styles,
        "effect_styles": sums.effect_styles,
        "line_styles": sums.line_styles,
        "distinct_stop_positions": positions,
        "fill_kinds": fill_kinds,
        "effect_kinds": effect_kinds,
        "shadow_kinds": shadow_kinds,
        "styles_with_shadow": with_shadow,
        "path_shapes": path_shapes,
        "parts": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
