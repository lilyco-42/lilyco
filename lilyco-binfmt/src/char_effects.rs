//! 字符效果那几枚：OOXML 写在 `w:rPr` 的孩子上，ODF 写在 `style:text-properties` 的属性上
//!
//! 与第二读者 `office_reader.docx_char_effects` / `odf_char_effects` 同一条口径。
//! 认的是「这一串字说了些什么额外的话」：小型大写 / 大写 / 字距调整 / 字符间距 / 强调记号 /
//! 缩放 / 阴影 / 轮廓 / 从右往左 / 不校对，ODF 那边是大小写转换 / 字族变体 / 字间距 /
//! 字距调整 / 描边 / 阴影 / 上下标位置 / 跟随窗口字色。
//!
//! 两半**不进**已经交过的东西：`w:vertAlign`（上标下标）、`w:highlight` 与 `w:position` 早在
//! `run_formats` 那本里，重复一遍只会让两本账各说一次同一个数；ODF 那侧的
//! `fo:text-position` 是 `w:vertAlign` 的另一家写法，所以留在这本里 —— 同一问在两家的
//! 两处各交各的，不并账。
//!
//! OOXML 的开关有三种活法，一格说不完：`<w:smallCaps/>` 在场而没写 `@w:val` 是「开」，
//! `@w:val="0"` / `"false"` 是「关」，其余值是「写了个值」（`w:kern val="28"` 是 28 的
//! 1/20 pt 门槛，`w:spacing val="5"` 是 5 的 twips）—— 所以 `states` 与 `vals` 两格
//! 各交各的，`by_state` 只数这三态，数字一律按写的字符串交，不换算。
//!
//! ODF 的八枚基础属性各有 `-asian` / `-complex` 两种孪生写法（与 `font_scripts` 同一层
//! 分工：孪生全在 `style:` 里，本仓量到过而三种槽位在这族上只写过拉丁那一槽：
//! `by_slot` 拉丁 716、亚洲 0、复杂 0 —— 那是生产者的说法，按写的交）。
//! 走法与 `font_scripts` 一致：先 `content.xml` 再 `styles.xml`，每份先 `style` 后
//! `default-style`，另交 `not_under_holder` 说这条走法漏没漏。
//!
//! `--limit` 只截 `entries`，每一份计数仍说整份件。

use crate::xmlscan::Node;
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CE_ELEM: [&str; 11] = [
    "caps",
    "smallCaps",
    "allCaps",
    "kern",
    "spacing",
    "em",
    "scale",
    "shadow",
    "outlined",
    "rtl",
    "noProof",
];
const CE_ON: [&str; 3] = ["1", "true", "on"];
const CE_OFF: [&str; 4] = ["0", "false", "off", "none"];
const CE_BASE: [&str; 8] = [
    "text-transform",
    "font-variant",
    "letter-spacing",
    "letter-kerning",
    "text-outline",
    "text-shadow",
    "text-position",
    "use-window-font-color",
];
const CE_SCRIPTS: [&str; 3] = ["latin", "asian", "complex"];

fn ce_attrs() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for base in CE_BASE.iter() {
        for suf in ["", "-asian", "-complex"].iter() {
            out.push(format!("{}{}", base, suf));
        }
    }
    out
}

fn slot_of(name: &str) -> &'static str {
    if name.ends_with("-asian") {
        "asian"
    } else if name.ends_with("-complex") {
        "complex"
    } else {
        "latin"
    }
}

fn base_of(name: &str) -> String {
    for suf in ["-asian", "-complex"].iter() {
        if let Some(head) = name.strip_suffix(*suf) {
            return head.to_string();
        }
    }
    name.to_string()
}

/// 一枚开关的读法：在场而没写 `@w:val` 就是「开」，写了按写的读
fn state_of(got: Option<&String>) -> &'static str {
    let Some(had) = got else {
        return "on";
    };
    let low = had.trim().to_lowercase();
    if CE_ON.contains(&low.as_str()) {
        "on"
    } else if CE_OFF.contains(&low.as_str()) {
        "off"
    } else {
        "value"
    }
}

fn tally(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

fn push_pair(map: &mut BTreeMap<String, Vec<String>>, key: String, value: &str) {
    let row = map.entry(key).or_default();
    if !row.iter().any(|had| had == value) {
        row.push(value.to_string());
    }
}

// ── OOXML 那一半：w:rPr 的孩子元素 ────────────────────────────────────────────

fn picked_docx(node: &Node) -> BTreeMap<String, Option<String>> {
    let mut out: BTreeMap<String, Option<String>> = BTreeMap::new();
    for kid in node
        .children
        .iter()
        .filter(|one| CE_ELEM.contains(&one.local()))
    {
        let mut val: Option<String> = None;
        for (key, value) in kid.attrs.iter() {
            let local = key.rsplit(':').next().unwrap_or(key);
            if local == "val" {
                val = Some(value.clone());
            }
        }
        out.insert(kid.local().to_string(), val);
    }
    out
}

fn walk_docx(
    node: &Node,
    par: &str,
    gpar: &str,
    owner: Option<&str>,
    part: &str,
    rows: &mut Vec<Value>,
    seen: &mut usize,
) {
    let name = node.local();
    if name == "rPr" {
        *seen += 1;
        let got = picked_docx(node);
        if !got.is_empty() {
            let mut names: Vec<String> = Vec::new();
            let mut states = serde_json::Map::new();
            let mut vals = serde_json::Map::new();
            for (key, value) in got.iter() {
                names.push(key.clone());
                states.insert(key.clone(), json!(state_of(value.as_ref())));
                vals.insert(key.clone(), json!(value.as_deref()));
            }
            names.sort();
            rows.push(json!({
                "index": rows.len(),
                "part": part,
                "place": format!("{}>{}", par, gpar),
                "owner": owner,
                "written": names,
                "states": Value::Object(states),
                "vals": Value::Object(vals),
            }));
        }
    }
    let here = if name == "style" {
        node.attr_local("styleId")
    } else {
        owner
    };
    for kid in node.children.iter() {
        walk_docx(kid, name, par, here, part, rows, seen);
    }
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut parts: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/") && one.ends_with(".xml") && !one.contains("/_rels/"))
        .collect();
    if parts.is_empty() {
        return empty_docx();
    }
    parts.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut seen = 0usize;
    for part in parts {
        let Ok(member) = zipread::member(bytes, &part, zipread::DEFAULT_MEMBER_CAP) else {
            continue;
        };
        let parsed = crate::xmlscan::parse_str(&member.as_text());
        // `parse_str` 交的是 `#doc` 伪根：真正的根是它的第一个孩子，父亲与祖父都从空开始
        for root in parsed.children.iter() {
            walk_docx(root, "", "", None, &part, &mut rows, &mut seen);
        }
    }
    let mut by_part: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_place: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_element: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut parts_seen: Vec<String> = Vec::new();
    let empty = serde_json::Map::new();
    for one in rows.iter() {
        let part = one["part"].as_str().unwrap_or_default();
        tally(&mut by_part, part);
        if !parts_seen.iter().any(|had| had == part) {
            parts_seen.push(part.to_string());
        }
        tally(&mut by_place, one["place"].as_str().unwrap_or_default());
        for (name, value) in one["vals"].as_object().unwrap_or(&empty) {
            tally(&mut by_element, name);
            // 这一格先占上：一枚开关可以全都「没写值」，那份空清单也是文件说过的话
            values.entry(name.clone()).or_default();
            if let Some(had) = value.as_str() {
                push_pair(&mut values, name.clone(), had);
            }
        }
        for value in one["states"].as_object().unwrap_or(&empty).values() {
            if let Some(had) = value.as_str() {
                tally(&mut by_state, had);
            }
        }
    }
    for row in values.values_mut() {
        row.sort();
    }
    let written = rows.len();
    json!({
        "family": "ooxml",
        "available": true,
        "rpr_seen": seen,
        "with_effects": written,
        "by_part": by_part,
        "by_place": by_place,
        "by_element": by_element,
        "by_state": by_state,
        "values": values,
        "parts_seen": parts_seen,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "entries": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}

fn empty_docx() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "rpr_seen": 0,
        "with_effects": 0,
        "by_part": {},
        "by_place": {},
        "by_element": {},
        "by_state": {},
        "values": {},
        "parts_seen": [],
        "listed": 0,
        "cut": 0,
        "entries": [],
    })
}

// ── ODF 那一半：style:text-properties 的属性 ──────────────────────────────────

fn picked_odf(node: &Node, allow: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if allow.iter().any(|one| one == local) {
            out.insert(local.to_string(), value.clone());
        }
    }
    out
}

fn rows_of(holder: &Node, kind: &str, part: &str, allow: &[String], out: &mut Vec<Value>) {
    let name = holder.attr_local("name").map(String::from);
    let family = holder.attr_local("family").map(String::from);
    for kid in holder
        .children
        .iter()
        .filter(|one| one.local() == "text-properties")
    {
        let got = picked_odf(kid, allow);
        if got.is_empty() {
            continue;
        }
        let mut slots: Vec<&str> = CE_SCRIPTS
            .iter()
            .copied()
            .filter(|slot| got.keys().any(|had| slot_of(had) == *slot))
            .collect();
        slots.sort();
        let mut bases: Vec<String> = Vec::new();
        for key in got.keys() {
            let had = base_of(key);
            if !bases.iter().any(|one| *one == had) {
                bases.push(had);
            }
        }
        bases.sort();
        let mut written: Vec<String> = got.keys().cloned().collect();
        written.sort();
        out.push(json!({
            "index": out.len(),
            "part": part,
            "holder": kind,
            "style_name": name.clone(),
            "family": family.clone(),
            "slots": slots,
            "bases": bases,
            "written": written,
            "vals": got,
        }));
    }
}

pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let allow = ce_attrs();
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut elements_written = 0usize;
    let empty = serde_json::Map::new();
    for (part, root) in roots.iter() {
        for one in root.descendants("text-properties") {
            if !picked_odf(one, &allow).is_empty() {
                elements_written += 1;
            }
        }
        for holder in root.descendants("style") {
            rows_of(holder, "style", part, &allow, &mut rows);
        }
        for holder in root.descendants("default-style") {
            rows_of(holder, "default-style", part, &allow, &mut rows);
        }
    }
    let mut by_part: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_holder: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_family: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_base: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_slot: BTreeMap<String, u64> = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut parts_seen: Vec<String> = Vec::new();
    for one in rows.iter() {
        let part = one["part"].as_str().unwrap_or_default();
        tally(&mut by_part, part);
        if !parts_seen.iter().any(|had| had == part) {
            parts_seen.push(part.to_string());
        }
        tally(&mut by_holder, one["holder"].as_str().unwrap_or_default());
        tally(&mut by_family, one["family"].as_str().unwrap_or("none"));
        for (name, value) in one["vals"].as_object().unwrap_or(&empty) {
            tally(&mut by_base, &base_of(name));
            tally(&mut by_slot, slot_of(name));
            if let Some(had) = value.as_str() {
                let book = values.entry(base_of(name)).or_default();
                let row = book.entry(slot_of(name).to_string()).or_default();
                if !row.iter().any(|had2| had2 == had) {
                    row.push(had.to_string());
                }
            }
        }
    }
    for slot in CE_SCRIPTS.iter() {
        by_slot.entry((*slot).to_string()).or_insert(0);
    }
    for book in values.values_mut() {
        for row in book.values_mut() {
            row.sort();
        }
    }
    let written = rows.len();
    json!({
        "family": "odf",
        "available": true,
        "elements_written": elements_written,
        "with_effects": written,
        "not_under_holder": elements_written.saturating_sub(written),
        "by_part": by_part,
        "by_holder": by_holder,
        "by_family": by_family,
        "by_base": by_base,
        "by_slot": by_slot,
        "values": values,
        "parts_seen": parts_seen,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "entries": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}
