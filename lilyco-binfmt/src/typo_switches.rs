//! pptx 的排印开关：段、字符、框三种宿主上那三十来枚属性，按写的字符串原样交
//!
//! 与第二读者 `office_reader.slide_typo_switches_pptx` 同一条口径。三族宿主各有各的属性面 ——
//! 段（`a:pPr` / `a:defPPr` / `a:lvlNpPr`）管换行与标点（`eaLnBrk` / `hangingPunct` /
//! `latinLnBrk` / `rtl`）、默认制表位 `defTabSz` 与 `marL` / `marR` / `indent` / `algn` /
//! `lvl`；字符（`a:rPr` / `a:defRPr` / `a:endParaRPr`）管字偶距 `kern`、大字距 `capSpacing`、
//! 字距 `spc`、全角半角那两枚 `balancedDblByte` / `eaConformance` 与拼写检查那三枚
//! `dirty` / `err` / `smtClean`；框（`a:bodyPr`）管折行 `wrap` 与 `rtlCol` / `anchor` /
//! `anchorCtr`。主题那一份归 `theme_ledger`，三枚字体指针归 `font_sets`，`wrap` 在 autofit
//! 那本也露过一次面 —— 分工写进 README，这本管的是**开关的拼法与在场**。
//!
//! 值一律按写的字符串交，不折成布尔：OOXML 的 ST_OnOff 允许 `0`/`1` 与 `false`/`true` 两种
//! 拼法，本机 104 份真件里 `b` 一枚就同时写过 1、0、false、true 四种（2481 / 259 / 108 / 44
//! 条），`rtlCol` 同时写 0 与 false（4597 / 121 条）—— 折成布尔就把「这份稿子用什么拼法写的」
//! 那句话丢了。`onoff` 单独记这一族布尔属性各自的拼法条数，`values` 记全部认得的值词汇。
//!
//! `not_under_carrier` 是这条走法自己的对账，而且它**本来就不是 0**：那些名字在同名不同物的
//! 元素上另有其义，逐枚点名交在 `collisions`（`元素名/属性名` → 条数）—— 量到过 `p:ph` 的
//! `sz`（占位符类型号，不是字号）、`a:tab` 的 `algn`（制表位对齐，不是段落对齐）、`a:tcPr` 的
//! `anchor` / `marL` / `marR`（格子的竖直对齐与边距，归 `vertical_align`、`cell_margins`
//! 那两本）。所以三族宿主的属性面是各自一份认字表，合成一张按名字收会把占位符类型号当字号交。
//!
//! 部件按名字定序再走：`parts_seen` 与 rows 的顺序要跟着文件的名走，不能跟 zip 的存储序走。
//! `--limit` 只截 rows，每一份计数仍说整份件。

use crate::xmlscan::Node;
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const TS_PAR: [&str; 12] = [
    "eaLnBrk",
    "hangingPunct",
    "latinLnBrk",
    "rtl",
    "fontAlign",
    "tcAp",
    "defTabSz",
    "marL",
    "marR",
    "indent",
    "algn",
    "lvl",
];
const TS_CHR: [&str; 18] = [
    "kern",
    "capSpacing",
    "spc",
    "balancedDblByte",
    "eaConformance",
    "noProof",
    "HideSpc",
    "dirty",
    "err",
    "smtClean",
    "lang",
    "altLang",
    "b",
    "i",
    "u",
    "strike",
    "sz",
    "baseline",
];
const TS_FRAME: [&str; 8] = [
    "wrap",
    "keepText",
    "fromColumn",
    "rtlCol",
    "anchor",
    "anchorCtr",
    "numCol",
    "spcCol",
];
const TS_PAR_CARRIERS: [&str; 11] = [
    "pPr", "defPPr", "lvl1pPr", "lvl2pPr", "lvl3pPr", "lvl4pPr", "lvl5pPr", "lvl6pPr", "lvl7pPr",
    "lvl8pPr", "lvl9pPr",
];
const TS_CHR_CARRIERS: [&str; 3] = ["rPr", "defRPr", "endParaRPr"];
const TS_FRAME_CARRIERS: [&str; 1] = ["bodyPr"];
/// 布尔那一族（ST_OnOff）：只有这些属性的值有「0/1 与 false/true 两种拼法」可数
const TS_BOOL: [&str; 21] = [
    "eaLnBrk",
    "hangingPunct",
    "latinLnBrk",
    "rtl",
    "tcAp",
    "fontAlign",
    "balancedDblByte",
    "eaConformance",
    "noProof",
    "HideSpc",
    "dirty",
    "err",
    "smtClean",
    "b",
    "i",
    "u",
    "strike",
    "keepText",
    "fromColumn",
    "rtlCol",
    "anchorCtr",
];
const TS_PREFIXES: [&str; 5] = [
    "ppt/slides/",
    "ppt/slideLayouts/",
    "ppt/slideMasters/",
    "ppt/notesSlides/",
    "ppt/notesMasters/",
];
const TS_ROOT_PART: &str = "ppt/presentation.xml";

/// 这一枚元素是哪一族宿主，以及它那一族的认字表
fn group_of(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    if TS_PAR_CARRIERS.contains(&name) {
        return Some(("paragraph", &TS_PAR));
    }
    if TS_CHR_CARRIERS.contains(&name) {
        return Some(("character", &TS_CHR));
    }
    if TS_FRAME_CARRIERS.contains(&name) {
        return Some(("frame", &TS_FRAME));
    }
    None
}

/// 部件名给住处：母版 / 版式 / 正文页 / 母版页 / 备注页 / 演示稿本体
fn kind_of(part: &str) -> &'static str {
    if part.contains("slideMasters/") {
        "master"
    } else if part.contains("slideLayouts/") {
        "layout"
    } else if part.contains("slides/") {
        "slide"
    } else if part.contains("notesMasters/") {
        "notes_master"
    } else if part.contains("notesSlides/") {
        "notes_slide"
    } else {
        "presentation"
    }
}

fn declared() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for one in TS_PAR.iter().chain(TS_CHR.iter()).chain(TS_FRAME.iter()) {
        let had = (*one).to_string();
        if !out.contains(&had) {
            out.push(had);
        }
    }
    out.sort();
    out
}

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

struct Book {
    carriers: u64,
    anywhere: u64,
    rows: Vec<Value>,
    by_group: BTreeMap<String, u64>,
    by_carrier: BTreeMap<String, u64>,
    by_kind: BTreeMap<String, u64>,
    collisions: BTreeMap<String, u64>,
    values: BTreeMap<String, Vec<String>>,
    onoff: BTreeMap<String, BTreeMap<String, u64>>,
}

impl Book {
    fn new() -> Book {
        Book {
            carriers: 0,
            anywhere: 0,
            rows: Vec::new(),
            by_group: BTreeMap::new(),
            by_carrier: BTreeMap::new(),
            by_kind: BTreeMap::new(),
            collisions: BTreeMap::new(),
            values: BTreeMap::new(),
            onoff: BTreeMap::new(),
        }
    }

    fn walk(&mut self, node: &Node, parent: &str, part: &str, kind: &str, declared: &[String]) {
        let name = node.local().to_string();
        let mut locals: Vec<String> = Vec::new();
        for (key, _) in node.attrs.iter() {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            locals.push(key.rsplit(':').next().unwrap_or(key).to_string());
        }
        let here: Vec<String> = declared
            .iter()
            .filter(|one| locals.iter().any(|had| had == *one))
            .cloned()
            .collect();
        if !here.is_empty() {
            self.anywhere += 1;
        }
        match group_of(&name) {
            None => {
                for one in here.iter() {
                    bump(&mut self.collisions, &(name.clone() + "/" + one.as_str()));
                }
            }
            Some((group, allow)) => {
                self.carriers += 1;
                let mut written: Vec<String> = Vec::new();
                let mut vals = serde_json::Map::new();
                for one in allow.iter().copied() {
                    let key = one.to_string();
                    if !locals.contains(&key) {
                        continue;
                    }
                    let got = node.attr_local(one).unwrap_or_default().to_string();
                    written.push(key.clone());
                    vals.insert(key.clone(), json!(got));
                    let book = self.values.entry(key).or_default();
                    if !book.iter().any(|had| had == &got) {
                        book.push(got.clone());
                    }
                    if TS_BOOL.contains(&one) {
                        *self
                            .onoff
                            .entry(one.to_string())
                            .or_default()
                            .entry(got)
                            .or_insert(0) += 1;
                    }
                }
                if !written.is_empty() {
                    bump(&mut self.by_group, group);
                    bump(&mut self.by_carrier, name.as_str());
                    bump(&mut self.by_kind, kind);
                    self.rows.push(json!({
                        "index": self.rows.len(),
                        "part": part,
                        "kind": kind,
                        "group": group,
                        "carrier": name,
                        "parent": parent,
                        "written": written,
                        "attrs": Value::Object(vals),
                    }));
                }
            }
        }
        for kid in node.children.iter() {
            self.walk(kid, &name, part, kind, declared);
        }
    }
}

fn empty(declared: &[String]) -> Value {
    json!({
        "family": "ooxml",
        "available": false,
        "parts_seen": [],
        "carriers_total": 0,
        "with_switch": 0,
        "by_group": {},
        "by_carrier": {},
        "by_kind": {},
        "elements_anywhere": 0,
        "not_under_carrier": 0,
        "collisions": {},
        "attrs_written": [],
        "attrs_never": declared,
        "values": {},
        "onoff": {},
        "listed": 0,
        "cut": 0,
        "rows": [],
    })
}

pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let want_names = declared();
    let mut wanted: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| {
            one.ends_with(".xml")
                && (one == TS_ROOT_PART || TS_PREFIXES.iter().any(|pre| one.starts_with(pre)))
        })
        .collect();
    if wanted.is_empty() {
        return empty(&want_names);
    }
    wanted.sort();
    let mut book = Book::new();
    let mut seen: Vec<String> = Vec::new();
    for part in wanted {
        let Ok(member) = zipread::member(bytes, &part, zipread::DEFAULT_MEMBER_CAP) else {
            continue;
        };
        seen.push(part.clone());
        let text = member.as_text();
        let parsed = crate::xmlscan::parse_str(&text);
        let kind = kind_of(&part);
        // `parse_str` 交的是 `#doc` 伪根：真正的根是它的第一个孩子，第一层的父亲名是空串
        for root in parsed.children.iter() {
            book.walk(root, "", &part, kind, &want_names);
        }
    }
    for row in book.values.values_mut() {
        row.sort();
    }
    let written_names: Vec<String> = book.values.keys().cloned().collect();
    let never: Vec<String> = want_names
        .iter()
        .filter(|one| !book.values.contains_key(*one))
        .cloned()
        .collect();
    let Book {
        carriers,
        anywhere,
        rows,
        by_group,
        by_carrier,
        by_kind,
        collisions,
        values,
        onoff,
    } = book;
    let written = rows.len();
    json!({
        "family": "ooxml",
        "available": true,
        "parts_seen": seen,
        "carriers_total": carriers,
        "with_switch": written,
        "by_group": by_group,
        "by_carrier": by_carrier,
        "by_kind": by_kind,
        "elements_anywhere": anywhere,
        "not_under_carrier": anywhere.saturating_sub(written as u64),
        "collisions": collisions,
        "attrs_written": written_names,
        "attrs_never": never,
        "values": values,
        "onoff": onoff,
        "listed": written.min(limit),
        "cut": written.saturating_sub(limit),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}
