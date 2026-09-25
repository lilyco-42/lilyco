//! 「脚注与尾注怎么编号」——OOXML 把同一句话写在**两处**（settings 与每一条节），
//! ODF 给每一类注一份 `text:notes-configuration`，而两类注答得不对称
//!
//! OOXML 的 `w:footnotePr` / `w:endnotePr` 自己**不写属性**：值在孩子身上
//! （`<w:numStart w:val="5"/>`、`<w:numRestart w:val="eachPage"/>`、`<w:numFmt w:val="decimal"/>`、
//! `<w:pos w:val="sectEnd"/>`），所以「段上有没有这一格」与「这一格写了什么」要分两层看。
//! 另有两个特殊孩子 `w:footnote` / `w:endnote` 带 `w:id` —— 那是分隔符与延续分隔符的引用
//! （注部件里那两条空正文的占位，注那一条 lane 已经量过）。它可以出现在两处：
//! `word/settings.xml` 里一份，每一条 `w:sectPr` 里又一份，而**两份说的可以不一样**。
//!
//! ODF 是一份 `text:notes-configuration` 管一类注（`text:note-class`）。实测 LibreOffice 把
//! 两份都写在 **styles.xml**：footnote 那份带 `style:num-format` + `text:start-value` +
//! `text:footnotes-position` + `text:start-numbering-at`，endnote 那份**只有前两个** ——
//! 没写的就交 false，不拿另一类的写法替它接。
//!
//! 实测（`nset.docx` = 拿 LibreOffice 自己写的 `notes-end.docx`（真有 1 条脚注 + 1 条尾注）
//! 用 python-docx 打开，往那两枚元素上补 `numStart` / `numRestart`，孩子按 schema 顺序重排）：
//! 1. settings 那份说了 `numFmt=decimal` `numStart=5` `numRestart=eachPage` `pos=sectEnd`，
//!    而 `w:sectPr` 那份只有 `pos` 与 `numFmt` —— 「从几开始、每页重来吗」**只在一处说过话**，
//!    所以 `attrs_only_in_settings` 是 `["numRestart", "numStart"]`，`attrs_in_both` 是
//!    `["numFmt", "pos"]`；两份不合成一份；
//! 2. LibreOffice 把同一份重写一遍（`nset-lo.docx`）：那两格**在两处都没了**（只剩 `pos` 与
//!    `numFmt`），`attrs_only_in_settings` 因此变空 —— 「谁丢了起点」这件事账上看得见；
//! 3. 同一份转成 odt（`nset.odt`）写的是 LibreOffice 自己的默认：`text:start-value="0"`
//!    （不是 5）、`text:start-numbering-at="document"`（不是 eachPage），编号格式换成
//!    `style:num-format="1"` / `"i"` —— 词汇与 OOXML 不是一套，两边各按写的交、不折算；
//! 4. 反面凭据 `notes.docx`（python-docx 的原件，有脚注但两处都没写过这一格）：
//!    `footnote_written` / `endnote_written` 与两条 `sections_with_*_pr` 全是 false / 0 ——
//!    「这份件有注」与「这份件说了注怎么编号」是两件事。
//!
//! 两条造件教训（写在这里免得再试）：元素名是 `w:footnotePr`，写成 `w:footPr` 会被整条丢掉；
//! 而**稿子里一条注都没有时，LibreOffice 两处也都不写** —— 那测出来的是「没有注所以没设置」，
//! 不是「LO 不读设置」。
//!
//! RTF 与遗留 .doc 不交这个键（缺键 = 这一族没看）：RTF 的注编号写在 `\frncn` / `\frn`
//! 那一路控制字上且没有节级对应物，归属判不住；.doc 的注设置住在 table stream，这一族读者不走。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 一个元素的属性表（局部名；命名空间声明不算属性）
fn attrs_of(node: &Node) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    out
}

/// `w:footnotePr` / `w:endnotePr` 一份：值在孩子身上，分隔符引用另交一份
fn read_pr(holder: &Node) -> Value {
    let mut written = serde_json::Map::new();
    let mut refs: Vec<Value> = Vec::new();
    for kid in holder.children.iter() {
        let name = kid.local().to_string();
        let kid_attrs = attrs_of(kid);
        if name == "footnote" || name == "endnote" {
            refs.push(match kid_attrs.get("id") {
                Some(had) => had.clone(),
                None => Value::Null,
            });
            continue;
        }
        match kid_attrs.get("val") {
            Some(had) => {
                written.insert(name, had.clone());
            }
            None => {
                written.insert(name, Value::Object(kid_attrs));
            }
        }
    }
    json!({
        "written": Value::Object(written),
        "note_refs": refs,
        "holder_attrs": Value::Object(attrs_of(holder)),
    })
}

/// 一份件里第一条叫这个名字的元素（`descendants` 已是文档序）
fn first_of<'a>(root: Option<&'a Node>, name: &str) -> Option<&'a Node> {
    match root {
        Some(had) => had.descendants(name).into_iter().next(),
        None => None,
    }
}

/// OOXML 那一份：settings 里一份、每条节一份，两处各按各的交
pub(crate) fn docx(document: &Node, bytes: &[u8], limit: usize) -> Value {
    let settings = zipread::member(bytes, "word/settings.xml", DEFAULT_MEMBER_CAP)
        .ok()
        .map(|one| xmlscan::parse_str(&one.as_text()));
    let settings_root = settings.as_ref().and_then(|had| had.child("settings"));
    let footnote = first_of(settings_root, "footnotePr").map(read_pr);
    let endnote = first_of(settings_root, "endnotePr").map(read_pr);
    let mut seen: Vec<String> = Vec::new();
    for one in footnote.iter().chain(endnote.iter()) {
        if let Some(had) = one["written"].as_object() {
            for key in had.keys() {
                if !seen.iter().any(|k: &String| k == key) {
                    seen.push(key.clone());
                }
            }
        }
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut with_foot = 0usize;
    let mut with_end = 0usize;
    let mut sect_seen: Vec<String> = Vec::new();
    let mut sections = 0usize;
    if let Some(body) = document.child("body") {
        let all: Vec<&Node> = body.descendants("sectPr").into_iter().collect();
        sections = all.len();
        for (index, sect) in all.into_iter().enumerate() {
            let mut row = json!({"section": index});
            for (key, name) in [("footnote", "footnotePr"), ("endnote", "endnotePr")] {
                let holder = sect.children.iter().find(|kid| kid.local() == name);
                match holder {
                    Some(had) => {
                        let had = read_pr(had);
                        if key == "footnote" {
                            with_foot += 1;
                        } else {
                            with_end += 1;
                        }
                        if let Some(map) = had["written"].as_object() {
                            for attr in map.keys() {
                                if !sect_seen.iter().any(|k: &String| k == attr) {
                                    sect_seen.push(attr.clone());
                                }
                            }
                        }
                        row[format!("{}_written", key).as_str()] = json!(true);
                        row[key] = had;
                    }
                    None => {
                        row[format!("{}_written", key).as_str()] = json!(false);
                        row[key] = Value::Null;
                    }
                }
            }
            rows.push(row);
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "settings_part": settings_root.is_some(),
        "footnote_written": footnote.is_some(),
        "endnote_written": endnote.is_some(),
        "footnote": footnote.unwrap_or(Value::Null),
        "endnote": endnote.unwrap_or(Value::Null),
        "sections_total": sections,
        "sections_with_footnote_pr": with_foot,
        "sections_with_endnote_pr": with_end,
        "sections": rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "attrs_only_in_settings": only_in(&seen, &sect_seen),
        "attrs_only_in_sections": only_in(&sect_seen, &seen),
        "attrs_in_both": both(&seen, &sect_seen),
    })
}

fn only_in(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = a
        .iter()
        .filter(|one| !b.iter().any(|had: &String| had == *one))
        .cloned()
        .collect();
    out.sort();
    out
}

fn both(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = a
        .iter()
        .filter(|one| b.iter().any(|had: &String| had == *one))
        .cloned()
        .collect();
    out.sort();
    out
}

/// ODF 那一份：一类注一份 configuration（两份都在 styles.xml，但不赌）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut classes: Vec<String> = Vec::new();
    let mut formats: Vec<String> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut with_pos = 0usize;
    let mut with_start_numbering = 0usize;
    for (part, root) in roots.iter() {
        for one in root.descendants("notes-configuration") {
            let table = attrs_of(one);
            let get = |key: &str| -> Option<String> {
                table
                    .get(key)
                    .and_then(|had| had.as_str())
                    .map(String::from)
            };
            let note_class = get("note-class");
            let num_format = get("num-format");
            if let Some(had) = &note_class {
                if !classes.iter().any(|k: &String| k == had) {
                    classes.push(had.clone());
                }
            }
            if let Some(had) = &num_format {
                if !formats.iter().any(|k: &String| k == had) {
                    formats.push(had.clone());
                }
            }
            if !parts.iter().any(|k: &String| k.as_str() == *part) {
                parts.push((*part).to_string());
            }
            let has_pos = table.contains_key("footnotes-position");
            let has_start = table.contains_key("start-numbering-at");
            if has_pos {
                with_pos += 1;
            }
            if has_start {
                with_start_numbering += 1;
            }
            rows.push(json!({
                "part": part,
                "note_class": note_class,
                "written": Value::Object(table),
                "num_format": get("num-format"),
                "start_value": get("start-value"),
                "position_written": has_pos,
                "start_numbering_written": has_start,
            }));
        }
    }
    parts.sort();
    json!({
        "family": "odf",
        "available": true,
        "configs_total": rows.len(),
        "classes_written": classes,
        "distinct_num_formats": formats,
        "with_position": with_pos,
        "with_start_numbering": with_start_numbering,
        "parts_seen": parts,
        "configs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
