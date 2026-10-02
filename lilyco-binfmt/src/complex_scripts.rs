//! 这一段文字给拉丁与给复杂脚本各写了什么：`sz`/`szCs`、`b`/`bCs`、`i`/`iCs`、`u`/`uCs`
//!
//! 与第二读者 `office_reader.script_pairs_docx` 同一条口径。八个名字都住在 `w:rPr` 的
//! **直接孩子**里，按局部名认（`Node::local()`）；住处按父与祖定，因为同一枚 `w:rPr`
//! 可以在五个地方各说各的话：正文 run、段落标记、样式自己的、样式的段落标记、
//! `w:docDefaults/w:rPrDefault`，还有表格样式条件分支 `w:tblStylePr` 里那一份。
//! `w:rStyle` 指向的样式**不跟着跳** —— 那是 `run_formats` 与 `styled_text` 在问的话。
//! `stylesWithEffects.xml` 不读：本仓其它样式类账本一律只读主那份（见事实 166）。
//!
//! 两件事分开交，因为它们是不同的问句：
//! * 写了什么 —— `latin_written` / `complex_written` 是文件里的 `@w:val` 原样，没写值就是
//!   `null`（规范里「元素在场而没写值」读作 on）；
//! * 意思是什么 —— `latin` / `complex` 是 `absent` / `on` / `off` / `value` 四态之一，
//!   `sz` 那一族不参与真假三态（它是数字，不是开关）。
//! 于是「拉丁没写值（on）而复杂脚本写了 `w:val="true"`（也是 on）」这种同义不同写法
//! 有单独一枚布尔，而不是被字面比对误报成不合。

use crate::xmlscan::Node;
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 四对：拉丁那一枚与复杂脚本那一枚
const SP_PAIRS: [(&str, &str); 4] = [("sz", "szCs"), ("b", "bCs"), ("i", "iCs"), ("u", "uCs")];
/// 这八枚都算「写了这一句」，其余孩子不进这本书
const SP_NAMES: [&str; 8] = ["sz", "szCs", "b", "bCs", "i", "iCs", "u", "uCs"];
const PARTS: [&str; 2] = ["word/document.xml", "word/styles.xml"];

#[derive(Clone)]
struct Side {
    meaning: String,
    written: Option<String>,
}

struct Row {
    place: String,
    owner: Value,
    pairs: Vec<(String, Value)>,
    names: Vec<String>,
}

fn tag_of(node: &Node) -> String {
    node.local().to_string()
}

/// 一枚开关/数值按意思算什么：不在场 absent，在场没写值 on，0/false off，1/true/on on，
/// 其余（含 sz 那种数字）原样交回、意思记 value
fn meaning(tag: &str, block: Option<&Node>) -> Side {
    let Some(one) = block else {
        return Side {
            meaning: "absent".to_string(),
            written: None,
        };
    };
    let Some(got) = one.attr_local("val") else {
        return Side {
            meaning: "on".to_string(),
            written: None,
        };
    };
    if tag != "sz" {
        let low = got.trim().to_lowercase();
        if low == "0" || low == "false" {
            return Side {
                meaning: "off".to_string(),
                written: Some(got.to_string()),
            };
        }
        if low == "1" || low == "true" || low == "on" {
            return Side {
                meaning: "on".to_string(),
                written: Some(got.to_string()),
            };
        }
    }
    Side {
        meaning: "value".to_string(),
        written: Some(got.to_string()),
    }
}

fn pair(tag_latin: &str, tag_complex: &str, blocks: &BTreeMap<String, &Node>) -> Value {
    let one = meaning(tag_latin, blocks.get(tag_latin).map(|one| *one));
    let two = meaning(tag_complex, blocks.get(tag_complex).map(|one| *one));
    let written = one.meaning != "absent";
    let other = two.meaning != "absent";
    let state = if written && other {
        "both"
    } else if written {
        "only_latin"
    } else if other {
        "only_complex"
    } else {
        "neither"
    };
    let differs = state == "both" && one.written != two.written;
    json!({
        "latin": one.meaning.clone(),
        "latin_written": one.written.clone(),
        "complex": two.meaning.clone(),
        "complex_written": two.written.clone(),
        "state": state,
        "differ": differs,
        "same_meaning_diff_spelling":
            state == "both" && one.meaning == two.meaning && one.written != two.written,
    })
}

/// 一个 rPr 里那八枚的直接孩子（只认自己这一层，同名留先写的那一枚）
fn blocks_of(node: &Node) -> BTreeMap<String, &Node> {
    let mut out: BTreeMap<String, &Node> = BTreeMap::new();
    for kid in &node.children {
        let name = tag_of(kid);
        if SP_NAMES.contains(&name.as_str()) && !out.contains_key(&name) {
            out.insert(name, kid);
        }
    }
    out
}

/// 挂在谁身上决定住处：`par_owner` 是父亲自己那枚 styleId / type
fn place_of(
    par: &str,
    par_owner: Option<String>,
    gpar: &str,
    gpar_owner: Option<String>,
) -> (String, Value) {
    if par == "rPrDefault" {
        return ("doc_default".to_string(), Value::Null);
    }
    if par == "tblStylePr" {
        return ("table_style_branch".to_string(), json!(par_owner));
    }
    if par == "pPr" {
        if gpar == "style" {
            return ("style_paragraph_mark".to_string(), json!(gpar_owner));
        }
        return ("paragraph_mark".to_string(), Value::Null);
    }
    if par == "style" {
        return ("style".to_string(), json!(par_owner));
    }
    if par == "r" {
        return ("run".to_string(), Value::Null);
    }
    (
        "other".to_string(),
        json!(if par.is_empty() {
            None::<&str>
        } else {
            Some(par)
        }),
    )
}

fn owner_of(node: &Node) -> Option<String> {
    let name = tag_of(node);
    if name == "style" {
        return node.attr_local("styleId").map(|one| one.to_string());
    }
    if name == "tblStylePr" {
        return node.attr_local("type").map(|one| one.to_string());
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: &Node,
    par: &str,
    par_owner: Option<String>,
    gpar: &str,
    gpar_owner: Option<String>,
    seen: &mut u64,
    rows: &mut Vec<Row>,
) {
    let name = tag_of(node);
    if name == "rPr" {
        *seen += 1;
        let blocks = blocks_of(node);
        if !blocks.is_empty() {
            let (place, owner) = place_of(par, par_owner, gpar, gpar_owner);
            let mut pairs: Vec<(String, Value)> = Vec::new();
            for (latin, complex_tag) in SP_PAIRS {
                pairs.push((latin.to_string(), pair(latin, complex_tag, &blocks)));
            }
            rows.push(Row {
                place,
                owner,
                pairs,
                names: node.children.iter().map(tag_of).collect::<Vec<String>>(),
            });
        }
    }
    let mine = owner_of(node);
    for kid in &node.children {
        walk(kid, &name, mine.clone(), par, par_owner.clone(), seen, rows);
    }
}

fn book(
    rows: &[Row],
) -> (
    BTreeMap<String, u64>,
    BTreeMap<String, BTreeMap<String, u64>>,
) {
    let mut places: BTreeMap<String, u64> = BTreeMap::new();
    let mut tally: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for (latin, _complex) in SP_PAIRS {
        tally.insert(
            latin.to_string(),
            BTreeMap::from([
                ("latin_written", 0u64),
                ("complex_written", 0u64),
                ("both", 0),
                ("only_latin", 0),
                ("only_complex", 0),
                ("differ", 0),
                ("same_meaning_diff_spelling", 0),
            ]),
        );
    }
    for one in rows {
        *places.entry(one.place.clone()).or_insert(0) += 1;
        for (latin, value) in &one.pairs {
            let book = tally.get_mut(latin).expect("四对预置过");
            if value["latin"] != json!("absent") {
                *book.get_mut("latin_written").expect("键预置过") += 1;
            }
            if value["complex"] != json!("absent") {
                *book.get_mut("complex_written").expect("键预置过") += 1;
            }
            if let Some(state) = value["state"].as_str() {
                if state != "neither" {
                    *book.get_mut(state).expect("键预置过") += 1;
                }
            }
            if value["differ"] == json!(true) {
                *book.get_mut("differ").expect("键预置过") += 1;
            }
            if value["same_meaning_diff_spelling"] == json!(true) {
                *book
                    .get_mut("same_meaning_diff_spelling")
                    .expect("键预置过") += 1;
            }
        }
    }
    (places, tally)
}

fn empty() -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "parts_seen": [],
        "rpr_seen": 0,
        "rpr_written": 0,
        "by_place": {},
        "by_pair": {},
        "listed": 0,
        "cut": 0,
        "rows": [],
    })
}

pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut picked: Vec<(&str, String)> = Vec::new();
    for want in PARTS {
        if let Ok(member) = zipread::member(bytes, want, zipread::DEFAULT_MEMBER_CAP) {
            picked.push((want, member.as_text()));
        }
    }
    if picked.is_empty() {
        return empty();
    }
    let mut seen = 0u64;
    let mut rows: Vec<Row> = Vec::new();
    for (_part, text) in &picked {
        let parsed = crate::xmlscan::parse_str(text);
        // `parse_str` 交的是 `#doc` 伪根：真正的根（w:document / w:styles）是它的第一个孩子，
        // 递归要从那一层起，父与祖在那一层都是空
        let Some(root) = parsed.children.first() else {
            continue;
        };
        walk(root, "", None, "", None, &mut seen, &mut rows);
    }
    let (places, tally) = book(&rows);
    let written = rows.len();
    let kept: Vec<Value> = rows
        .iter()
        .take(limit)
        .enumerate()
        .map(|(index, one)| {
            json!({
                "index": index,
                "place": one.place,
                "owner": one.owner,
                "pairs": Value::Object(serde_json::Map::from_iter(
                    one.pairs.iter().map(|(key, value)| (key.clone(), value.clone()))
                )),
                "names": one.names,
            })
        })
        .collect();
    let mut by_pair = serde_json::Map::new();
    for (latin, book) in tally {
        if book["latin_written"] == 0 && book["complex_written"] == 0 {
            continue;
        }
        let mut one = serde_json::Map::new();
        for (key, value) in book {
            one.insert(key, json!(value));
        }
        by_pair.insert(latin, Value::Object(one));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_seen": picked.iter().map(|(one, _raw)| *one).collect::<Vec<&str>>(),
        "rpr_seen": seen,
        "rpr_written": written,
        "by_place": places,
        "by_pair": Value::Object(by_pair),
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "rows": kept,
    })
}
