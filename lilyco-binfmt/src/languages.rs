//! 「这份文档写了哪种语言」——OOXML 把三路文字写在同一枚元素的三个属性上，
//! ODF 只用一格语言位、还要拆成 `language` + `country` 两个属性
//!
//! OOXML 的 `w:lang` 有三个可以各自缺的属性：`w:val`（拉丁那一路）、`w:eastAsia`（中日韩那一路）、
//! `w:bidi`（复杂脚本从右往左那一路）。所以一条元素可以同时说三路，而**每一路可以是三种不同的
//! 语言**。它可以出现在四层上：`w:styles.xml` 的 `w:docDefaults`、样式定义、段自己的
//! `w:pPr/w:rPr`、以及每串字的 `w:rPr` —— 四层各交一份，不合并。
//!
//! ODF 没有「三路」这个形状：字符属性 `style:text-properties` 上只有 `fo:language` 与
//! `fo:country`（外加 `fo:script`）一格语言位，值还是**拆开的两段**（`en` + `US`，对 OOXML 的
//! `en-US`）。两边各按写的交，不拼也不折。
//!
//! 实测（`lang.docx` 由 python-docx 自己的 XML 层写：段上 `val="es-ES"`，三串字分别只写
//! `val="fr-FR"`、只写 `eastAsia="ja-JP"`、三路全写 `val="de-DE" eastAsia="zh-CN" bidi="ar-SA"`）：
//! 1. 模板的 `docDefaults` 一条写全三个值 `val="en-US" eastAsia="en-US" bidi="ar-SA"` —— 一份
//!    全中文稿子在文件级默认上声明「复杂脚本是阿拉伯语」：那是模板的说法，按写的交、
//!    不读成「这份文档有阿拉伯语」（本仓库不拿规范/模板默认当这份件说过的话）；
//! 2. 全语料 51 份 docx 的 `w:lang` **一条都不在正文里**（`in_document` 全是 0，只有 styles.xml
//!    那一份），段层与 run 层是这份新件第一次让它们有非零凭据；
//! 3. LibreOffice 重写同一份（`lang-lo.docx`）：正文那四条一字未动，另外**给三个样式各补了
//!    一条** `Normal` / `NoSpacing` / `MacroText`（值全是 `en-US` / `en-US` / `ar-SA`）——
//!    元素 5 条变 8 条，`levels_seen` 多出一层；
//! 4. 同一份转成 odt（`lang.odt`）：**只写 `eastAsia="ja-JP"` 那一串字在 ODF 里一个字都没落**
//!    （那一份样式没有语言格可放），三路全写那串只剩 `de` + `DE`，`bidi="ar-SA"` 也没了 ——
//!    交回来的 `distinct_languages` 是 `de / en / es / fr`，没有 `ja`、没有 `zh`、没有 `ar`；
//! 5. `tbox-lo.odt` 的一条写 **`fo:language="none"` 而整个不带 `fo:country`** —— 这一族用字面
//!    `none` 说「没有语言」，`none_written` 就是数它；而 `pnum.odt` / `tbox.odt`（zipfile 写的
//!    最小件）一个字都不写 → `elements_total` 是 0，不是缺键，也不替它补 `en`。
//!
//! 层级判据不用父指针：段与 run 都从各自的宿主元素往下取直接孩子（`w:p/w:pPr/w:rPr/w:lang`、
//! `w:r/w:rPr/w:lang`），ODF 那侧同理（`style` / `default-style` 的直接孩子才是宿主），
//! 另交一份「整棵树里带这三个属性的元素」条数与 `not_under_style`，两边可以互相对账。
//!
//! RTF 那一族写的是 `\langN`（LCID 数字，另有 `\langfe` 那一路），整名比对没做完之前不判归属，
//! 所以这一族不交这个键；遗留 .doc 的语言住在 table stream 的 `grfDdc`/Lcb 里，这一族读者不走那里。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// ODF 那三个属性（按局部名收，前缀丢掉 —— 与读者同一条）
const LANG_ATTRS: [&str; 3] = ["language", "country", "script"];

/// 一个元素的全部属性（局部名），与 `attr_map` 同一条：命名空间声明不算属性
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

fn first_lang(holder: &Node) -> Option<serde_json::Map<String, Value>> {
    holder
        .descendants("lang")
        .into_iter()
        .next()
        .map(|one| attrs_of(one))
}

/// OOXML 那一份：四层各交各的，三个属性各自的 distinct 按文档序（document.xml 先、styles.xml 后）
pub(crate) fn docx(document: &Node, bytes: &[u8], limit: usize) -> Value {
    let styles_node = zipread::member(bytes, "word/styles.xml", DEFAULT_MEMBER_CAP)
        .ok()
        .map(|one| xmlscan::parse_str(&one.as_text()));
    let styles = match styles_node.as_ref() {
        Some(had) => had.child("styles").or(Some(had)),
        None => None,
    };
    let body = document.child("body");
    let doc_langs = document.descendants("lang");
    let style_langs: Vec<&Node> = match styles {
        Some(had) => had.descendants("lang").into_iter().collect(),
        None => Vec::new(),
    };
    let mut defaults: Option<serde_json::Map<String, Value>> = None;
    let mut style_rows: Vec<Value> = Vec::new();
    if let Some(had) = styles {
        defaults = had
            .descendants("docDefaults")
            .into_iter()
            .next()
            .and_then(|one| first_lang(one));
        for one in had.descendants("style") {
            let mut id: Option<String> = None;
            let mut kind: Option<String> = None;
            for (key, value) in one.attrs.iter() {
                match key.rsplit(':').next().unwrap_or(key) {
                    "styleId" => id = Some(value.clone()),
                    "type" => kind = Some(value.clone()),
                    _ => {}
                }
            }
            let holder = one.children.iter().find(|kid| kid.local() == "rPr");
            if let Some(table) = holder.and_then(|had| first_lang(had)) {
                style_rows.push(json!({
                    "style_id": id,
                    "style_type": kind,
                    "attrs": Value::Object(table),
                }));
            }
        }
    }
    let mut para_rows: Vec<Value> = Vec::new();
    let mut run_rows: Vec<Value> = Vec::new();
    if let Some(had) = body {
        for (index, para) in had
            .children
            .iter()
            .filter(|kid| kid.local() == "p")
            .enumerate()
        {
            let ppr = para.children.iter().find(|kid| kid.local() == "pPr");
            let holder = ppr.and_then(|had| had.children.iter().find(|kid| kid.local() == "rPr"));
            if let Some(table) = holder.and_then(|had| first_lang(had)) {
                para_rows.push(json!({"index": index, "attrs": Value::Object(table)}));
            }
        }
        for (index, run) in had.descendants("r").into_iter().enumerate() {
            let holder = run.children.iter().find(|kid| kid.local() == "rPr");
            if let Some(table) = holder.and_then(|had| first_lang(had)) {
                run_rows.push(json!({"run": index, "attrs": Value::Object(table)}));
            }
        }
    }
    let mut tables: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for one in doc_langs.iter().chain(style_langs.iter()) {
        let got = attrs_of(one);
        for (slot, key) in ["val", "eastAsia", "bidi"].iter().enumerate() {
            if let Some(value) = got.get(*key).and_then(|one| one.as_str()) {
                if !tables[slot].iter().any(|had: &String| had == value) {
                    tables[slot].push(value.to_string());
                }
            }
        }
    }
    let mut levels: Vec<&str> = Vec::new();
    for (name, had) in [
        ("doc_defaults", defaults.is_some()),
        ("styles", !style_rows.is_empty()),
        ("paragraphs", !para_rows.is_empty()),
        ("runs", !run_rows.is_empty()),
    ]
    .into_iter()
    {
        if had {
            levels.push(name);
        }
    }
    let styles_with = style_rows.len();
    let paras_with = para_rows.len();
    let runs_with = run_rows.len();
    json!({
        "family": "ooxml",
        "available": true,
        "elements_total": doc_langs.len() + style_langs.len(),
        "in_document": doc_langs.len(),
        "in_styles": style_langs.len(),
        "doc_defaults_written": defaults.is_some(),
        "doc_defaults": defaults.map(Value::Object).unwrap_or(Value::Null),
        "styles": style_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "styles_with_lang": styles_with,
        "paragraphs": para_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "paragraphs_with_lang": paras_with,
        "runs": run_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "runs_with_lang": runs_with,
        "distinct_vals": tables[0],
        "distinct_east_asia": tables[1],
        "distinct_bidi": tables[2],
        "levels_seen": levels,
    })
}

/// ODF 那一份：宿主走法（`style` / `default-style` 的直接孩子），另数一份整棵树的
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut anywhere = 0usize;
    for (part, root) in roots.iter() {
        for one in root.descendants("text-properties") {
            let had = attrs_of(one);
            let keeps = pick_lang(&had);
            if keeps.is_some() {
                anywhere += 1;
            }
        }
        for holder in root.descendants("style") {
            let name = holder.attr_local("name").map(String::from);
            let family = holder.attr_local("family").map(String::from);
            for kid in holder
                .children
                .iter()
                .filter(|one| one.local() == "text-properties")
            {
                let had = attrs_of(kid);
                if let Some(table) = pick_lang(&had) {
                    entries.push(json!({
                        "part": part,
                        "holder": holder.local(),
                        "style_name": name.clone(),
                        "family": family.clone(),
                        "attrs": Value::Object(table),
                    }));
                }
            }
        }
        for holder in root.descendants("default-style") {
            let family = holder.attr_local("family").map(String::from);
            for kid in holder
                .children
                .iter()
                .filter(|one| one.local() == "text-properties")
            {
                let had = attrs_of(kid);
                if let Some(table) = pick_lang(&had) {
                    entries.push(json!({
                        "part": part,
                        "holder": holder.local(),
                        "style_name": Value::Null,
                        "family": family.clone(),
                        "attrs": Value::Object(table),
                    }));
                }
            }
        }
    }
    let langs = distinct(&entries, "language");
    let countries = distinct(&entries, "country");
    let scripts = distinct(&entries, "script");
    let none_written = entries
        .iter()
        .filter(|one| one["attrs"]["language"].as_str() == Some("none"))
        .count();
    let parts = {
        let mut out: Vec<String> = Vec::new();
        for one in entries.iter() {
            if let Some(had) = one["part"].as_str() {
                if !out.iter().any(|k: &String| k == had) {
                    out.push(had.to_string());
                }
            }
        }
        out.sort();
        out
    };
    let under = entries.len();
    json!({
        "family": "odf",
        "available": true,
        "elements_total": anywhere,
        "under_style": under,
        "not_under_style": anywhere.saturating_sub(under),
        "none_written": none_written,
        "distinct_languages": langs,
        "distinct_countries": countries,
        "distinct_scripts": scripts,
        "parts_seen": parts,
        "entries": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 只留那三个语言属性；一个都没有就当「这条没说语言」
fn pick_lang(map: &serde_json::Map<String, Value>) -> Option<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for key in LANG_ATTRS.iter() {
        if let Some(value) = map.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn distinct(rows: &[Value], key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for one in rows.iter() {
        if let Some(value) = one["attrs"][key].as_str() {
            if !out.iter().any(|had: &String| had == value) {
                out.push(value.to_string());
            }
        }
    }
    out.sort();
    out
}
