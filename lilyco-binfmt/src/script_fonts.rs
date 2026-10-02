//! 这一族字按三种脚本各点了谁：`style:font-name` / `-asian` / `-complex` 同层那 30 枚
//!
//! 与第二读者 `office_reader.odf_font_scripts` 同一条口径。宿主是字符属性
//! `style:text-properties`，它可以挂在 `style:style`（具名样式，也在 `content.xml` 的
//! 自动样式里）与 `style:default-style` 两种宿主上 —— 走法与 `languages.rs` 的 ODF 那半
//! 一模一样（先 `content.xml` 再 `styles.xml`，每份部件先 `style` 后 `default-style`），
//! 另交一份「整棵树里带这三枚 name 之一的元素」条数与 `not_under_holder`，
//! 这样「按宿主走」有没有漏是文件自己说的。
//!
//! 三条 name 之外还收同层的字号 / 字体族 / generic / pitch / 宽高样式 / 语言 / 国别 /
//! charset 十枚的三种脚本写法：`font-name-asian` 从不单独说话，它旁边总是站着
//! `font-size-asian` 与 `language-asian`，只看 name 会把「这一套」读成一枚孤字。
//!
//! 按**局部名**认（与本仓 odt 那几本同一口径）。命名空间的分工是量出来的，记在 README：
//! 拉丁那一套横跨两个命名空间 —— `fo:` 有 `font-family` / `font-size` / `font-style` /
//! `font-weight` / `language` / `country`，`style:` 有 `font-name` / `font-family-generic` /
//! `font-pitch` / `font-charset`；而 `-asian` 与 `-complex` 那两套**全在 `style:`** 里。
//! 本仓 47 份写过这一族的 odt 里，这 30 枚局部名每枚只落在一个命名空间，所以按局部名收
//! 不会把 `fo:font-family` 与一个假想的 `style:font-family` 并成一格。
//!
//! 三套脚本的在场组合是一份账（`by_combo`），每种脚本点过的名字另是一份（`font_names`）；
//! 「点的名字在不在本包 `style:font-face` 名单里」交 `names_unresolved`，解不到才说话。
//! 值一律原样交：本仓量到过 `Cambria` 与 `Cambria1` 并存（两枚都在同一包的 font-face
//! 名单里，各被不同样式点走）、到过全角的 `ＭＳ 明朝`，也到过一枚只叫 `F` 的复杂脚本
//! 家族。后缀为什么出现那份件没说，所以这里只交写法，不替生产者猜理由。
//!
//! OOXML 那侧同一个问句由 `a:latin` / `a:ea` / `a:cs` 三枚**子元素**回答
//! （`font_sets.rs`，pptx 那一路），`w:rFonts` 的 `@eastAsia` / `@cs` 又是第三种写法；
//! 三本各交各的，不并账。`--limit` 只截 `entries`，每一份计数仍说整份件。

use crate::xmlscan::Node;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 同层收的十枚基础属性（三种脚本写法各一份，共 30 枚）
const FS_BASE: [&str; 10] = [
    "font-name",
    "font-family",
    "font-family-generic",
    "font-pitch",
    "font-size",
    "font-style",
    "font-weight",
    "font-charset",
    "language",
    "country",
];
const FS_SUFFIX: [&str; 3] = ["", "-asian", "-complex"];
/// 三套脚本各自的 name 位，也是「这一族字点了谁」的那三枚
const FS_SCRIPTS: [&str; 3] = ["latin", "asian", "complex"];

fn fs_attrs() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for base in FS_BASE.iter() {
        for suf in FS_SUFFIX.iter() {
            out.push(format!("{}{}", base, suf));
        }
    }
    out
}

fn name_attr(script: &str) -> &'static str {
    match script {
        "asian" => "font-name-asian",
        "complex" => "font-name-complex",
        _ => "font-name",
    }
}

/// 一个元素上这一族的属性（局部名，命名空间声明不算属性）
fn picked(node: &Node, allow: &[String]) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if allow.iter().any(|one| one == local) {
            out.insert(local.to_string(), json!(value));
        }
    }
    out
}

fn written_names(map: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = map.keys().cloned().collect();
    out.sort();
    out
}

fn tally(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

fn distinct_names(rows: &[Value], script: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for one in rows.iter() {
        if let Some(value) = one["names"][script]["value"].as_str() {
            if !out.iter().any(|had: &String| had == value) {
                out.push(value.to_string());
            }
        }
    }
    out.sort();
    out
}

/// 一枚宿主（`style:style` 或 `style:default-style`）的直接孩子里，那些写了三枚 name 之一的
fn push_rows(entries: &mut Vec<Value>, holder: &Node, kind: &str, part: &str, allow: &[String]) {
    let name = holder.attr_local("name").map(String::from);
    let family = holder.attr_local("family").map(String::from);
    for kid in holder
        .children
        .iter()
        .filter(|one| one.local() == "text-properties")
    {
        let had = picked(kid, allow);
        let here: Vec<&str> = FS_SCRIPTS
            .iter()
            .copied()
            .filter(|script| had.contains_key(name_attr(script)))
            .collect();
        if here.is_empty() {
            continue;
        }
        let combo = here.join("+");
        let mut names = serde_json::Map::new();
        for script in FS_SCRIPTS.iter() {
            let want = name_attr(*script);
            names.insert(
                (*script).to_string(),
                json!({"present": had.contains_key(want), "value": had.get(want)}),
            );
        }
        entries.push(json!({
            "index": entries.len(),
            "part": part,
            "holder": kind,
            "style_name": name.clone(),
            "family": family.clone(),
            "scripts": here,
            "names": Value::Object(names),
            "combo": combo,
            "written": written_names(&had),
            "attrs": Value::Object(had),
        }));
    }
}

pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let allow = fs_attrs();
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut elements_written = 0usize;
    let mut names_written = 0usize;
    let mut faces_declared = 0usize;
    let mut face_names: Vec<String> = Vec::new();
    for (part, root) in roots.iter() {
        for one in root.descendants("font-face") {
            faces_declared += 1;
            if let Some(name) = one.attr_local("name") {
                face_names.push(name.to_string());
            }
        }
        for one in root.descendants("text-properties") {
            let had = picked(one, &allow);
            if !had.is_empty() {
                elements_written += 1;
            }
            if FS_SCRIPTS
                .iter()
                .any(|script| had.contains_key(name_attr(*script)))
            {
                names_written += 1;
            }
        }
        for holder in root.descendants("style") {
            push_rows(&mut entries, holder, "style", part, &allow);
        }
        for holder in root.descendants("default-style") {
            push_rows(&mut entries, holder, "default-style", part, &allow);
        }
    }
    let mut by_part: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_holder: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_family: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_combo: BTreeMap<String, u64> = BTreeMap::new();
    let mut name_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut parts_seen: Vec<String> = Vec::new();
    for one in entries.iter() {
        let part = one["part"].as_str().unwrap_or_default();
        tally(&mut by_part, part);
        if !parts_seen.iter().any(|had| had == part) {
            parts_seen.push(part.to_string());
        }
        tally(&mut by_holder, one["holder"].as_str().unwrap_or_default());
        tally(&mut by_family, one["family"].as_str().unwrap_or("none"));
        tally(&mut by_combo, one["combo"].as_str().unwrap_or_default());
        for script in FS_SCRIPTS.iter() {
            if one["names"][*script]["present"].as_bool().unwrap_or(false) {
                tally(&mut name_counts, script);
            }
        }
    }
    parts_seen.sort();
    let font_names: BTreeMap<String, Vec<String>> = FS_SCRIPTS
        .iter()
        .map(|script| ((*script).to_string(), distinct_names(&entries, script)))
        .collect();
    let mut pointed: Vec<String> = Vec::new();
    for script in FS_SCRIPTS.iter() {
        for value in font_names[*script].iter() {
            if !pointed.iter().any(|had| had == value) {
                pointed.push(value.clone());
            }
        }
    }
    let unresolved: Vec<String> = pointed
        .iter()
        .filter(|one| !face_names.iter().any(|had| had == *one))
        .cloned()
        .collect();
    let written = entries.len();
    json!({
        "family": "odf",
        "available": true,
        "elements_written": elements_written,
        "names_written": names_written,
        "with_name": written,
        "not_under_holder": names_written.saturating_sub(written),
        "by_part": by_part,
        "by_holder": by_holder,
        "by_family": by_family,
        "by_combo": by_combo,
        "name_counts": name_counts,
        "font_names": font_names,
        "faces_declared": faces_declared,
        "names_unresolved": unresolved,
        "parts_seen": parts_seen,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "entries": entries.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}
