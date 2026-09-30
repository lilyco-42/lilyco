//! 「这份文档的页面网格与默认制表位」——OOXML 写在两处，两家的写法不一样
//!
//! 量到的形状（本仓这批 .docx 全都在，两家生产者各写一份）：
//!
//! * 文档级：`word/settings.xml` 里 `<w:defaultTabStop w:val="720"/>` —— 段落自己没列制表位时
//!   Tab 跳多远，按文件写的原样交（720 是 1/20 磅那一个单位，这里不换算成厘米）。
//! * 节级：每条 `w:sectPr` 里的 `<w:docGrid>`。手写那一版只写 `w:linePitch="360"`，
//!   LibreOffice 重写同一份时补齐成 `w:type="default" w:linePitch="360" w:charSpace="0"` ——
//!   `w:type` 没写就是「该族默认」，而 `charSpace="0"` 与「没写 charSpace」是两句话，
//!   所以三条属性各交各的，不合并成一个「网格模式」。
//! * 节上也能再写一条 `w:defaultTabStop` 覆盖文档级那一条：这批件**一条都没写**，
//!   那一格交 null，不拿文档级的值补过去。
//!
//! 这一族之外没有对应的东西：这批 .odt 里 `style:default-tab-stop` 一个都没有，RTF 与 `.doc`
//! 更没有这一层，所以 `grid_tab` 这个键在那三族**整个不在场**，由 probe 那侧当反面凭据核对。

use crate::office_doc::local_attrs;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一条 `w:docGrid`：三条属性按原样交，外加「文件到底写了哪几条」
fn grid_of(section: &xmlscan::Node) -> Value {
    let holder = section.child("docGrid");
    let written = match &holder {
        Some(had) => local_attrs(had),
        None => Value::Null,
    };
    // 属性「写了哪几条」按名字排好序交：两家的部件属性顺序不同，这一格问的是集合不是顺序
    let names = match &written {
        Value::Object(map) => {
            let mut had = map.keys().cloned().collect::<Vec<String>>();
            had.sort();
            had
        }
        _ => Vec::<String>::new(),
    };
    json!({
        "present": holder.is_some(),
        "written": written,
        "kind": written.get("type").cloned().unwrap_or(Value::Null),
        "line_pitch": written.get("linePitch").cloned().unwrap_or(Value::Null),
        "char_space": written.get("charSpace").cloned().unwrap_or(Value::Null),
        "written_names": names,
    })
}

/// OOXML 那一份：`word/settings.xml` 一条 + `word/document.xml` 每条节各一本
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let Ok(document) = zipread::member(bytes, "word/document.xml", DEFAULT_MEMBER_CAP) else {
        return json!({"available": false});
    };
    let document_root = xmlscan::parse_str(&document.as_text());
    let settings_root = zipread::member(bytes, "word/settings.xml", DEFAULT_MEMBER_CAP)
        .ok()
        .map(|one| xmlscan::parse_str(&one.as_text()));
    // 文档级那一条在 settings 的孩子里；settings 部件本身没写的文件也存在，那时交 null
    let book = settings_root
        .as_ref()
        .and_then(|had| had.child("settings"))
        .and_then(|had| had.child("defaultTabStop"))
        .and_then(|one| one.attr_local("val").map(String::from));
    let mut sections: Vec<Value> = Vec::new();
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut pitches: Vec<String> = Vec::new();
    let mut with_grid = 0usize;
    let mut sections_tab = 0usize;
    for (index, one) in document_root.descendants("sectPr").into_iter().enumerate() {
        let grid = grid_of(one);
        if grid["present"].as_bool() == Some(true) {
            with_grid += 1;
        }
        let key = grid["kind"].as_str().unwrap_or("(没写 w:type)").to_string();
        *kinds.entry(key).or_insert(0) += 1;
        if let Some(raw) = grid["line_pitch"].as_str() {
            pitches.push(raw.to_string());
        }
        let own = one
            .child("defaultTabStop")
            .and_then(|had| had.attr_local("val").map(String::from));
        if own.is_some() {
            sections_tab += 1;
        }
        sections.push(json!({
            "index": index,
            "grid": grid,
            "default_tab_stop": own,
            "written": local_attrs(one),
        }));
    }
    let mut distinct: Vec<String> = pitches.clone();
    distinct.sort();
    distinct.dedup();
    json!({
        "family": "ooxml",
        "available": true,
        "settings_written": book.is_some(),
        "default_tab_stop": book,
        "sections_total": sections.len(),
        "with_grid": with_grid,
        "sections_with_own_tab_stop": sections_tab,
        "grid_kinds": kinds,
        "distinct_line_pitches": distinct,
        "sections": sections.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
