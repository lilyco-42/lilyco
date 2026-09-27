//! 图是怎么摆的 —— 同一问在两族里放的地方不一样，所以两份账分开交。
//!
//! OOXML：`w:drawing` 肚子里那一枚孩子就是答案 —— `wp:inline` 是「随字走」，`wp:anchor` 是
//! 「浮着」，浮着才有环绕那一支（`wrapSquare` / `wrapTight` / `wrapThrough` /
//! `wrapTopAndBottom` / `wrapNone`）。两枚都写「图周围留多少」（`@distT/B/L/R`，EMU），
//! 只有 anchor 写「压不压字」「锁不锁」「能不能叠」「层序号」（`@behindDoc` / `@locked` /
//! `@allowOverlap` / `@relativeHeight`），位置分横竖两条（`positionH` / `positionV`：基准写在
//! `@relativeFrom`，怎么定写在孩子上 —— `align` 或 `positionOffset`，也可以都写）。
//!
//! ODF：框自己只写 `text:anchor-type`（`as-char` / `paragraph` / `page` / …），环绕、
//! 穿透、四个边距全在它点名的那份 `style:family="graphic"` 样式里，所以**一跳是这一族的
//! 默认形状**；那一跳断了交 `style_found: false`，属性全交 null，不替文件补一个「不环绕」。
//!
//! 实测（三份件：`wrap.docx` = python-docx 打底那张 `wp:inline` + 两枚按 ECMA 写法**合成**的
//! `wp:anchor`；`wrap.odt` = 三种锚各一枚；`wrap-lo.docx` = LibreOffice 把那份 odt 导回 docx）：
//! 1. `wp:inline` 结构上没有环绕那一支，所以 `wrap_element` 是 null —— 与「anchor 而文件
//!    一个环绕元素都没写」的 null 是同一格不同来路，两份账里都靠 `kind` 分得开；
//! 2. **同一个包在两家手里条数不一样**：三份框导成 docx 只剩两张图（按页锚的那一张被整张
//!    丢掉，`drawings` 3 → 2），而留着的那枚 anchor 的 `relativeHeight` 从合成的
//!    `251658240` 被换成 `3`（同一个意思的两种写法，谁也没错，所以两边都按原样交）；
//! 3. 环绕方式是**翻译过的**：ODF 那面写 `style:wrap="parallel"`，docx 这面 LibreOffice 自己
//!    挑了 `wp:wrapSquare`；而 ODF 那面「随字」那一枚的样式里**根本没有 `style:wrap` 这一格**
//!    （`wrap_unwritten` 1）——「文件没说」与「说了不环绕」（`wrapNone`）是两句话；
//! 4. 合成那两枚 anchor 是真件稀缺形状：本机真件里 33 份 docx 的 `word/document.xml` 共 161 枚
//!    `w:drawing`，只有 1 枚是 `wp:anchor`（它写的是 `wrapSquare`），另 160 枚全是 `wp:inline`，
//!    而 `python-docx` 本来就只会写 `wp:inline` —— 所以这一族的 anchor 侧只能靠合成件守
//!    （与 `notes.docm` 那枚合成宏同一待遇：只证明认得，不证明生产者会这么写）。
//!    ODF 那一面同理：本机那几个目录里一份 `.odt` 也没有，三种锚全靠自产件与 LibreOffice 出口。
//!
//! 不做的事：**不换算单位**（EMU 与 `0.21cm` 都按写的串交）、**不判图在文字上还是下**
//! （`behindDoc` 是文件写的一个串，不是本仓的结论）、**不比对两族的等价性**（同一份稿子
//! 两条路各自记账，比的是各自与第二读者）。

use crate::office_sheet::written_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// OOXML 里要扫的部件：正文之外，页眉页脚脚注尾注里的图也算数
const DOCX_PARTS: [&str; 5] = [
    "word/document.xml",
    "word/header",
    "word/footer",
    "word/footnotes.xml",
    "word/endnotes.xml",
];

fn first_child<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    node.children.iter().find(|one| one.local() == want)
}

fn wrap_child(node: &Node) -> Option<&Node> {
    node.children
        .iter()
        .find(|one| one.local().starts_with("wrap"))
}

fn child_names(node: &Node) -> Vec<Value> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .map(|one| json!(one.local()))
        .collect()
}

/// 前序走一遍，给出（`w:drawing`，它所在段落的序号）：序号就是「走到它之前数到了几个 `w:p`
/// 再减一」，不在任何段里的那一枚是 -1 —— 两份读者用同一条数法，不去比对象身份
fn collect_drawings<'a>(node: &'a Node, seen: &mut i64, out: &mut Vec<(&'a Node, i64)>) {
    if node.local() == "p" {
        *seen += 1;
    }
    if node.local() == "drawing" {
        out.push((node, *seen - 1));
    }
    for kid in node.children.iter() {
        collect_drawings(kid, seen, out);
    }
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// `wp:positionH` / `positionV` 那一格：没写也交一份形状齐全的行（`present: false`），
/// 这样行与行的键集永远一样
fn position(node: Option<&Node>) -> Value {
    let mine = match node {
        Some(had) => had,
        None => {
            return json!({
                "present": false, "relative_from": Value::Null, "align": Value::Null,
                "offset": Value::Null, "offset_written": false, "children": [],
            })
        }
    };
    let mut align = Value::Null;
    let mut align_seen = false;
    let mut offset = Value::Null;
    let mut offset_written = false;
    // 两枚同名的孩子按第一枚算（第二读者 `next(...)` 就是这个口径）
    for one in mine.children.iter().filter(|kid| kid.name != "#text") {
        if !align_seen && one.local() == "align" {
            align_seen = true;
            align = json!(one.attr_local("align"));
        }
        if !offset_written && one.local() == "positionOffset" {
            offset_written = true;
            offset = json!(one.text().trim().to_string());
        }
    }
    json!({
        "present": true,
        "relative_from": mine.attr_local("relativeFrom"),
        "align": align,
        "offset": offset,
        "offset_written": offset_written,
        "children": child_names(mine),
    })
}

fn blank_row(part: &str, para: i64) -> Value {
    json!({
        "part": part, "para": para, "kind": Value::Null, "attrs": {}, "wrap_element": Value::Null,
        "wrap_attrs": Value::Null, "children": [], "dist": Value::Null,
        "behind_doc": Value::Null, "locked": Value::Null, "allow_overlap": Value::Null,
        "layout_in_cell": Value::Null, "simple_pos": Value::Null,
        "relative_height": Value::Null, "doc_pr": Value::Null, "locks": Value::Null,
        "graphic_uri": Value::Null, "effect_extent": Value::Null,
        "position_h": position(None), "position_v": position(None),
    })
}

/// 一枚 `w:drawing` 肚子里的那位主人：按文档序取第一个 `inline` 或 `anchor`
/// （两家生产都不会两个都写，真写了也以先出现的那枚为准 —— 第二读者就是这个口径）
fn holder_of(drawing: &Node) -> Option<&Node> {
    drawing
        .children
        .iter()
        .find(|one| matches!(one.local(), "inline" | "anchor"))
}

/// 这一族的正文之外，页眉页脚脚注尾注里的图也算数（按前缀认，`word/header2.xml` 这种带号的名字在内）
fn wanted_part(name: &str) -> bool {
    DOCX_PARTS.iter().any(|head| name.starts_with(head))
}

/// OOXML 那一面：一份包一本账
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.ends_with(".xml") && wanted_part(one))
        .collect();
    names.sort_unstable();
    let mut rows: Vec<Value> = Vec::new();
    let mut scanned = 0usize;
    for name in names.iter().take(4000) {
        let root = match parse_member(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        let mut drawings: Vec<(&Node, i64)> = Vec::new();
        let mut seen = 0i64;
        collect_drawings(&root, &mut seen, &mut drawings);
        if drawings.is_empty() {
            continue;
        }
        scanned += 1;
        for (drawing, para) in drawings {
            let holder = match holder_of(drawing) {
                Some(had) => had,
                None => {
                    rows.push(blank_row(name, para));
                    continue;
                }
            };
            let attrs = written_attrs(holder);
            let wrap = wrap_child(holder);
            let docpr = first_child(holder, "docPr");
            let graphic = first_child(holder, "graphic");
            let data = graphic.and_then(|had| first_child(had, "graphicData"));
            let lockholder = first_child(holder, "cNvGraphicFramePr");
            let locks = lockholder.map(|had| {
                let inner = first_child(had, "graphicFrameLocks");
                json!({
                    "element": inner.map(|one| one.local()),
                    "attrs": inner.map(written_attrs).unwrap_or_else(|| json!({})),
                    "written": inner.map(|one| !attr_keys(one).is_empty()).unwrap_or(false),
                })
            });
            let extent = first_child(holder, "effectExtent");
            let dist = {
                let table = attrs.as_object();
                let has = table
                    .map(|had| {
                        ["distT", "distB", "distL", "distR"]
                            .iter()
                            .any(|key| had.contains_key(*key))
                    })
                    .unwrap_or(false);
                if !has {
                    Value::Null
                } else {
                    json!({
                        "distT": pick(&attrs, "distT"),
                        "distB": pick(&attrs, "distB"),
                        "distL": pick(&attrs, "distL"),
                        "distR": pick(&attrs, "distR"),
                    })
                }
            };
            // 从 `attrs` 里取的每一格都在它被交出去之前取完（同一个 `json!` 里先移后借不过不了借用检查）
            let behind_doc = pick(&attrs, "behindDoc");
            let locked = pick(&attrs, "locked");
            let allow_overlap = pick(&attrs, "allowOverlap");
            let layout_in_cell = pick(&attrs, "layoutInCell");
            let simple_pos = pick(&attrs, "simplePos");
            let relative_height = pick(&attrs, "relativeHeight");
            rows.push(json!({
                "part": name,
                "para": para,
                "kind": holder.local(),
                "attrs": attrs,
                "wrap_element": wrap.map(|one| one.local()),
                "wrap_attrs": wrap.map(written_attrs).unwrap_or(Value::Null),
                "children": child_names(holder),
                "dist": dist,
                "behind_doc": behind_doc,
                "locked": locked,
                "allow_overlap": allow_overlap,
                "layout_in_cell": layout_in_cell,
                "simple_pos": simple_pos,
                "relative_height": relative_height,
                "doc_pr": docpr.map(|had| {
                    let mut written = attr_keys(had);
                    written.sort();
                    json!({
                        "id": had.attr_local("id"),
                        "name": had.attr_local("name"),
                        "descr": had.attr_local("descr"),
                        "descr_written": had.attrs.iter().any(|(key, _)| {
                            key.rsplit(':').next().unwrap_or(key) == "descr"
                        }),
                        "written": written,
                    })
                }),
                "locks": locks,
                "graphic_uri": data.and_then(|one| one.attr_local("uri").map(String::from)),
                "effect_extent": extent.map(written_attrs).unwrap_or(Value::Null),
                "position_h": position(first_child(holder, "positionH")),
                "position_v": position(first_child(holder, "positionV")),
            }));
        }
    }
    let drawings = rows.len();
    let inline = rows.iter().filter(|one| one["kind"] == "inline").count();
    let anchor = rows.iter().filter(|one| one["kind"] == "anchor").count();
    let mut wraps: Vec<(String, usize)> = Vec::new();
    for one in rows.iter() {
        if let Some(had) = one["wrap_element"].as_str() {
            match wraps.iter_mut().find(|hit| hit.0 == had) {
                Some(hit) => hit.1 += 1,
                None => wraps.push((had.to_string(), 1)),
            }
        }
    }
    // 「哪几种环绕各写了几回」交一张表（不是数组）：第二读者那本也是字典
    let mut wrap_table = serde_json::Map::new();
    for (key, count) in wraps {
        wrap_table.insert(key, json!(count));
    }
    let listed = rows.len().min(limit);
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": scanned,
        "drawings": drawings,
        "inline": inline,
        "anchor": anchor,
        "other_kind": drawings - inline - anchor,
        "anchor_without_wrap": rows.iter().filter(|one| {
            one["kind"] == "anchor" && one["wrap_element"].is_null()
        }).count(),
        "wrap_elements": Value::Object(wrap_table),
        "listed": listed,
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "cut": drawings > listed,
    })
}

/// 属性表里的那一格：没写交 null，写了按文件自己那个串交
fn pick(attrs: &Value, key: &str) -> Value {
    attrs.get(key).cloned().unwrap_or(Value::Null)
}

/// 一个元素写着的那些属性名（局部名，命名空间声明不算）
fn attr_keys(node: &Node) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, _) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit([':', '}']).next().unwrap_or(key);
        if !out.iter().any(|had: &String| had == local) {
            out.push(local.to_string());
        }
    }
    out
}

struct DrawStyle {
    part: &'static str,
    props: Vec<(String, String)>,
    parent: Option<String>,
}

fn graphic_styles(bytes: &[u8]) -> Vec<(String, DrawStyle)> {
    let mut out: Vec<(String, DrawStyle)> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("style") {
            if one.attr_local("family") != Some("graphic") {
                continue;
            }
            let name = match one.attr_local("name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            if out.iter().any(|had: &(String, DrawStyle)| had.0 == name) {
                continue;
            }
            let props = match first_child(one, "graphic-properties") {
                Some(had) => {
                    let mut mine: Vec<(String, String)> = Vec::new();
                    for (key, value) in had.attrs.iter() {
                        if key == "xmlns" || key.starts_with("xmlns:") {
                            continue;
                        }
                        mine.push((
                            key.rsplit([':', '}']).next().unwrap_or(key).to_string(),
                            value.to_string(),
                        ));
                    }
                    mine.sort_by(|a, b| a.0.cmp(&b.0));
                    mine
                }
                None => Vec::new(),
            };
            out.push((
                name.clone(),
                DrawStyle {
                    part,
                    props,
                    parent: one.attr_local("parent-style-name").map(String::from),
                },
            ));
        }
    }
    out
}

fn prop<'a>(table: &'a [(String, String)], want: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|one| one.0 == want)
        .map(|one| one.1.as_str())
}

/// ODF 那一面：框 + 它点名的那份 graphic 样式
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let styles = graphic_styles(bytes);
    let mut rows: Vec<Value> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("frame") {
            if first_child(one, "image").is_none() {
                continue;
            }
            let name = one.attr_local("style-name");
            let mine =
                name.and_then(|want| styles.iter().find(|had| had.0 == want).map(|hit| &hit.1));
            let props: &[(String, String)] = match mine {
                Some(had) => &had.props,
                None => &[],
            };
            rows.push(json!({
                "part": part,
                "frame_name": one.attr_local("name"),
                "anchor_type": one.attr_local("anchor-type"),
                "style_name": name,
                "style_found": mine.is_some(),
                "style_part": mine.map(|had| had.part),
                "parent_style": mine.and_then(|had| had.parent.clone()),
                "wrap": prop(props, "wrap"),
                "wrap_written": props.iter().any(|one| one.0 == "wrap"),
                "wrap_contour": prop(props, "wrap-contour"),
                "run_through": prop(props, "run-through"),
                "flow_with_text": prop(props, "flow-with-text"),
                "vertical_pos": prop(props, "vertical-pos"),
                "vertical_rel": prop(props, "vertical-rel"),
                "horizontal_pos": prop(props, "horizontal-pos"),
                "horizontal_rel": prop(props, "horizontal-rel"),
                "margins": {
                    "margin-top": prop(props, "margin-top"),
                    "margin-bottom": prop(props, "margin-bottom"),
                    "margin-left": prop(props, "margin-left"),
                    "margin-right": prop(props, "margin-right"),
                },
                "props_written": props.len(),
                "x": one.attr_local("x"),
                "y": one.attr_local("y"),
                "width": one.attr_local("width"),
                "height": one.attr_local("height"),
                "z_index": one.attr_local("z-index"),
                "href": first_child(one, "image").and_then(|had| had.attr_local("href")),
            }));
        }
    }
    let mut kinds: Vec<(String, usize)> = Vec::new();
    let mut wraps: Vec<(String, usize)> = Vec::new();
    for one in rows.iter() {
        if let Some(had) = one["anchor_type"].as_str() {
            match kinds.iter_mut().find(|hit| hit.0 == had) {
                Some(hit) => hit.1 += 1,
                None => kinds.push((had.to_string(), 1)),
            }
        }
        if let Some(had) = one["wrap"].as_str() {
            match wraps.iter_mut().find(|hit| hit.0 == had) {
                Some(hit) => hit.1 += 1,
                None => wraps.push((had.to_string(), 1)),
            }
        }
    }
    let mut kind_table = serde_json::Map::new();
    for (key, count) in kinds {
        kind_table.insert(key, json!(count));
    }
    let mut wrap_table = serde_json::Map::new();
    for (key, count) in wraps {
        wrap_table.insert(key, json!(count));
    }
    let frames = rows.len();
    let listed = frames.min(limit);
    json!({
        "family": "odf",
        "available": true,
        "frames": frames,
        "anchor_types": Value::Object(kind_table),
        "wrap_values": Value::Object(wrap_table),
        "wrap_unwritten": rows.iter().filter(|one| !one["wrap_written"].as_bool().unwrap_or(false)).count(),
        "style_unfound": rows.iter().filter(|one| !one["style_found"].as_bool().unwrap_or(false)).count(),
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "listed": listed,
        "cut": frames > listed,
    })
}
