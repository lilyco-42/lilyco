//! `word/settings.xml` 上那层「视图与偏好」的账：谁写了哪一枚键、值写成了什么
//!
//! 与第二读者 `office_reader.doc_settings_docx` 同一条口径，只管**直接孩子**（一层，
//! 不下钻）：本仓的 `compat` / `documentProtection` / `footnotePr` / `endnotePr` /
//! `defaultTabStop` 五枚各有自己的账本（`layout_compat` / `protect` / `note_settings` /
//! `grid_tab`），这里只报「这一层有没有它」，不并账。
//!
//! 两件从真件里量出来的形状：
//! * 同一枚**局部名**可以在一份件里写两遍，各挂一个命名空间 —— 本机 33 份带
//!   `word/settings.xml` 的真件里有 12 份把 `docId` 写了两遍，一枚挂 word/2010（w14）、
//!   一枚挂 word/2012（w15）。两个读者都按**局部名**认（`Node::local()`），所以这类
//!   事实在 `children_total` 对 `distinct_children` 的差与 `repeated_children` 里现形，
//!   而不是被并成一枚后看不见；本仓这批 94 份里 `repeated_children` 全是空集，
//!   也就是「两种命名空间并存」至今是生产者习惯，仓里还没有 fixture 量到。
//! * 同一枚 `w:zoom` 有两种属性写法：`@w:val`（取值 `bestFit` 这类视图名）与
//!   `@w:percent`（取值 40 / 80 / 90 / 100 / 130 这类整数）。python-docx 那一路的模板
//!   只写前者（本仓 94 份里 44 份），LibreOffice 重写时只写后者（50 份）；真件里
//!   18 份只写 percent、15 份只写 val，**没有一份两样都写** —— 所以 zoom 那一格交两枚
//!   字段、各按自己写没写，另加一枚 `both_spellings` 而不是折成一个值。

use crate::office_doc::local_attrs;
use crate::xmlscan::Node;
use crate::zipread;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// 这一本解释过、因而不再算「不认识」的键名（局部名）
const KNOWN: [&str; 22] = [
    "zoom",
    "proofState",
    "themeFontLang",
    "clrSchemeMapping",
    "decimalSymbol",
    "listSeparator",
    "docVars",
    "docId",
    "defaultImageDpi",
    "shapeDefaults",
    "hdrShapeDefaults",
    "updateFields",
    "trackRevisions",
    "evenAndOddHeaders",
    "hideSpellingErrors",
    "embedSystemFonts",
    "savePreviewPicture",
    "doNotAutoCompressPictures",
    "autoHyphenation",
    "hyphenationZone",
    "characterSpacingControl",
    "mathPr",
];

/// 别的账本管的键：这里只报在场，不报内容
const ELSEWHERE: [&str; 9] = [
    "compat",
    "documentProtection",
    "footnotePr",
    "endnotePr",
    "defaultTabStop",
    "rsids",
    "rsid",
    "rsidRoot",
    "writingMode",
];

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

fn attr_count(one: Option<&Node>) -> usize {
    match one {
        None => 0,
        Some(node) => match local_attrs(node).as_object() {
            Some(book) => book.len(),
            None => 0,
        },
    }
}

fn find<'a>(kids: &'a [Node], want: &str) -> Option<&'a Node> {
    kids.iter().find(|one| one.local() == want)
}

fn has(kids: &[Node], want: &str) -> bool {
    find(kids, want).is_some()
}

fn val_of(one: Option<&Node>) -> Value {
    match one {
        None => Value::Null,
        Some(node) => json!(node.attr_local("val")),
    }
}

fn flag(one: Option<&Node>) -> Value {
    match one {
        None => json!({"present": false, "val_written": null}),
        Some(node) => json!({"present": true, "val_written": node.attr_local("val")}),
    }
}

fn written_of(one: Option<&Node>) -> Value {
    match one {
        None => json!({}),
        Some(node) => local_attrs(node),
    }
}

fn present_val(one: Option<&Node>) -> Value {
    match one {
        None => json!({"present": false, "val": null}),
        Some(node) => json!({"present": true, "val": node.attr_local("val")}),
    }
}

fn kids_of(one: Option<&Node>) -> &[Node] {
    match one {
        None => &[],
        Some(node) => node.children.as_slice(),
    }
}

fn names_of(kids: &[Node]) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    for one in kids {
        bump(&mut out, one.local());
    }
    out
}

fn child_count(one: Option<&Node>) -> usize {
    match one {
        None => 0,
        Some(node) => node.children.len(),
    }
}

fn empty() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "part": false,
        "part_bytes": 0,
        "root_written": {},
        "children_total": 0,
        "distinct_children": 0,
        "child_names": {},
        "repeated_children": {},
        "order": [],
        "switches": {},
        "zoom": {"present": false, "written": {}, "percent_written": null,
                 "val_written": null, "both_spellings": false},
        "proof_state": {"present": false, "spelling": null, "grammar": null, "written": {}},
        "theme_font_lang": {"present": false, "written": {}, "keys_written": 0},
        "clr_scheme_mapping": {"present": false, "written": {}, "slots_written": 0},
        "decimal_symbol": {"present": false, "val": null},
        "list_separator": {"present": false, "val": null},
        "doc_vars": {"total": 0, "names": [], "written": {}, "empty_values": 0},
        "doc_id": {"present": false, "val_written": null},
        "default_image_dpi": {"present": false, "val_written": null},
        "shape_defaults": {"present": false, "children_total": 0, "child_names": {}},
        "hdr_shape_defaults": {"present": false, "children_total": 0},
        "update_fields": {"present": false, "val_written": null},
        "track_revisions": {"present": false, "val_written": null},
        "even_and_odd_headers": {"present": false},
        "hide_spelling_errors": {"present": false},
        "embed_system_fonts": {"present": false},
        "save_preview_picture": {"present": false},
        "do_not_auto_compress_pictures": {"present": false},
        "auto_hyphenation": {"present": false, "val_written": null},
        "hyphenation_zone": {"present": false, "val_written": null},
        "character_spacing": {"present": false, "val_written": null},
        "math_pr": {"present": false, "children_total": 0, "child_names": {}},
        "covered_elsewhere": {"compat": false, "compat_children": 0,
                              "default_tab_stop": null, "document_protection": false,
                              "footnote_pr": false, "endnote_pr": false, "rsids": 0},
        "unknown_children": [],
        "listed": 0,
        "cut": 0,
        "entries": [],
    })
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let member = match zipread::member(bytes, "word/settings.xml", zipread::DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_why) => return empty(),
    };
    let part_bytes = member.data.len() as u64;
    let text = member.as_text();
    let parsed = crate::xmlscan::parse_str(&text);
    // `parse_str` 交的是 `#doc` 伪根：`w:settings` 是它的第一个孩子，根上的属性也在那一层
    let root = match parsed.children.first() {
        Some(one) => one,
        None => return empty(),
    };
    let kids = root.children.as_slice();
    let mut names: Vec<String> = Vec::new();
    for one in kids {
        names.push(one.local().to_string());
    }
    let counts = names_of(kids);
    let mut repeated: BTreeMap<String, u64> = BTreeMap::new();
    for (name, num) in &counts {
        if *num > 1 {
            repeated.insert(name.clone(), *num);
        }
    }
    let distinct: BTreeSet<String> = names.iter().cloned().collect();
    let mut switches: BTreeMap<String, String> = BTreeMap::new();
    for one in kids {
        if !one.children.is_empty() {
            continue;
        }
        let count = attr_count(Some(one));
        if count == 0 {
            switches.insert(one.local().to_string(), "on".to_string());
            continue;
        }
        if count == 1 {
            if let Some(got) = one.attr_local("val") {
                switches.insert(one.local().to_string(), got.to_string());
            }
        }
    }
    let zoom = find(kids, "zoom");
    let zoom_percent = match zoom {
        None => Value::Null,
        Some(one) => json!(one.attr_local("percent")),
    };
    let zoom_val = match zoom {
        None => Value::Null,
        Some(one) => json!(one.attr_local("val")),
    };
    let proof = find(kids, "proofState");
    let tfl = find(kids, "themeFontLang");
    let clr = find(kids, "clrSchemeMapping");
    let dv_rows = kids_of(find(kids, "docVars"));
    let mut var_names: Vec<Value> = Vec::new();
    let mut var_written: BTreeMap<String, Value> = BTreeMap::new();
    let mut var_empty = 0u64;
    for one in dv_rows {
        let name = one.attr_local("name");
        let value = one.attr_local("val");
        var_names.push(json!(name));
        if let Some(key) = name {
            var_written.insert(key.to_string(), json!(value));
            if value == Some("") {
                var_empty += 1;
            }
        }
    }
    let shape = find(kids, "shapeDefaults");
    let math = find(kids, "mathPr");
    let mut unknown: Vec<String> = Vec::new();
    for one in &names {
        if KNOWN.contains(&one.as_str()) || ELSEWHERE.contains(&one.as_str()) {
            continue;
        }
        if !unknown.contains(one) {
            unknown.push(one.clone());
        }
    }
    unknown.sort();
    let entries: Vec<Value> = kids
        .iter()
        .enumerate()
        .take(limit)
        .map(|(index, one)| {
            json!({
                "index": index,
                "name": one.local(),
                "written": local_attrs(one),
                "children_total": one.children.len(),
            })
        })
        .collect();
    json!({
        "family": "ooxml",
        "available": true,
        "part": true,
        "part_bytes": part_bytes,
        "root_written": local_attrs(root),
        "children_total": kids.len(),
        "distinct_children": distinct.len(),
        "child_names": counts,
        "repeated_children": repeated,
        "order": names.iter().take(limit).cloned().collect::<Vec<String>>(),
        "switches": switches,
        "zoom": {
            "present": zoom.is_some(),
            "written": written_of(zoom),
            "percent_written": zoom_percent,
            "val_written": zoom_val,
            "both_spellings": zoom.is_some()
                && zoom.and_then(|one| one.attr_local("percent")).is_some()
                && zoom.and_then(|one| one.attr_local("val")).is_some(),
        },
        "proof_state": {
            "present": proof.is_some(),
            "spelling": match proof {
                None => Value::Null,
                Some(one) => json!(one.attr_local("spelling")),
            },
            "grammar": match proof {
                None => Value::Null,
                Some(one) => json!(one.attr_local("grammar")),
            },
            "written": written_of(proof),
        },
        "theme_font_lang": {
            "present": tfl.is_some(),
            "written": written_of(tfl),
            "keys_written": attr_count(tfl),
        },
        "clr_scheme_mapping": {
            "present": clr.is_some(),
            "written": written_of(clr),
            "slots_written": attr_count(clr),
        },
        "decimal_symbol": present_val(find(kids, "decimalSymbol")),
        "list_separator": present_val(find(kids, "listSeparator")),
        "doc_vars": {
            "total": dv_rows.len(),
            "names": var_names,
            "written": var_written,
            "empty_values": var_empty,
        },
        "doc_id": flag(find(kids, "docId")),
        "default_image_dpi": flag(find(kids, "defaultImageDpi")),
        "shape_defaults": {
            "present": shape.is_some(),
            "children_total": child_count(shape),
            "child_names": names_of(kids_of(shape)),
        },
        "hdr_shape_defaults": {
            "present": has(kids, "hdrShapeDefaults"),
            "children_total": child_count(find(kids, "hdrShapeDefaults")),
        },
        "update_fields": flag(find(kids, "updateFields")),
        "track_revisions": flag(find(kids, "trackRevisions")),
        "even_and_odd_headers": {"present": has(kids, "evenAndOddHeaders")},
        "hide_spelling_errors": {"present": has(kids, "hideSpellingErrors")},
        "embed_system_fonts": {"present": has(kids, "embedSystemFonts")},
        "save_preview_picture": {"present": has(kids, "savePreviewPicture")},
        "do_not_auto_compress_pictures": {
            "present": has(kids, "doNotAutoCompressPictures")
        },
        "auto_hyphenation": flag(find(kids, "autoHyphenation")),
        "hyphenation_zone": flag(find(kids, "hyphenationZone")),
        "character_spacing": flag(find(kids, "characterSpacingControl")),
        "math_pr": {
            "present": math.is_some(),
            "children_total": child_count(math),
            "child_names": names_of(kids_of(math)),
        },
        "covered_elsewhere": {
            "compat": find(kids, "compat").is_some(),
            "compat_children": child_count(find(kids, "compat")),
            "default_tab_stop": val_of(find(kids, "defaultTabStop")),
            "document_protection": has(kids, "documentProtection"),
            "footnote_pr": has(kids, "footnotePr"),
            "endnote_pr": has(kids, "endnotePr"),
            "rsids": child_count(find(kids, "rsids")),
        },
        "unknown_children": unknown,
        "listed": kids.len().min(limit),
        "cut": kids.len().saturating_sub(limit),
        "entries": entries,
    })
}
