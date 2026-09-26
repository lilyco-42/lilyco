//! 域那一份账：同一个「这里有个域」在三族里写在三处，缺法也是三种
//!
//! OOXML 的一条域不是一枚元素，而是**一串散落的位置**：简单式 `w:fldSimple/@w:instr`
//! 一枚元素说完（缓存值是体内那些 `w:t`）；复杂式散成三段 —— `w:fldChar @w:fldCharType`
//! 的 `begin` 开一条、中间的 `w:instrText` 是指令（生产者爱切几段切几段，所以交 `pieces`）、
//! `separate` 之后到 `end` 之间那些 `w:t` 是缓存值。哪一段都可以不写，而且「没写」与
//! 「写了空」是两个答案：没有 `separate` 是 `has_separate false`，没有 `end` 是
//! `closed false`，一条 `w:instrText` 都没写是 `instruction null`、写了空串是 `""`。
//! `w:dirty` 挂在 begin 那一枚上（实测 `fields-mix.docx` 只有一枚 dirty），所以交那一条
//! 域碰到的第一枚原值。域能套域，每行带 `depth`；而**没闭合的那一条会把后面的段落
//! 都吃进去**（实测同一份 `fields-mix.docx`：断掉那条 `PAGE` 之后三条的 depth 是 1，
//! 它的 cached 一路累到「站内字样」）—— 断在哪就报在哪，不替文件补一个 `end`。
//!
//! 只数正文是量不到的：`structure.fields` 那一个整数只看 `w:body`，而页脚里那枚 PAGE
//! 是真件就有的形状（实测 `fields.docx` 正文 3 条、`word/footer1.xml` 里还有 1 条）。
//! 所以这一本把 `word/header*.xml` / `word/footer*.xml` / `footnotes.xml` / `endnotes.xml`
//! 一起扫（按**部件名**认，不查 rels），每行带 `part`，`markers` 是裸计数，`loose` 单数
//! 那些没有 begin 就写了 separate / end 的野标记 —— 复杂式的行数一定等于 begin 的枚数，
//! 闭合的行数等于 end 减去 loose，这三本对得上才算读对。
//!
//! ODF 恰好相反：**种类就是元素名**，这一族根本没有指令串（`text:page-number` /
//! `text:page-count` / `text:date` / `text:time` / `text:sequence` / `text:bookmark-ref` /
//! `text:database-display`），格式与选择全在属性上（`style:data-style-name`、
//! `text:select-page`、`text:formula`），所以每行 `instruction` 一律 null、`switches`
//! 一律空表，另交**文件写的那个名字**与整份属性表。跨族对照最值钱的一条：docx 那两枚
//! `REF` 与 `PAGEREF` 到这里塌成同一枚 `text:bookmark-ref`，只有 `text:reference-format`
//! （`number` / `page`）分得出谁是谁；`MERGEFIELD` 变成 `text:database-display`；
//! `HYPERLINK` 变成 `text:a`（它不是域，所以这一本少一行）；`STYLEREF` 与那条空指令的域
//! 干脆变成字面文字。页眉页脚住在 `styles.xml`，所以两份件都走。
//! `text:sequence-decl` 是**声明**不是域（实测 LibreOffice 六个全写：Drawing / Figure /
//! Illustration / Table / Text / 图，用没用到都写），不进 rows，另交份数与名字。
//! 认字表那 41 个名字里只有七个在这一库里量过，所以再交一份 `text_names`（这份件里
//! 出现过的所有 `text:` 元素名）—— 新形状只要落在这一族就一定在这本里露出来。
//!
//! RTF 是流里一群 `{\field{\*\fldinst 指令}{\fldrslt 显示文字}}`：指令里那些开关必须
//! 写成**成对的反斜杠**（单反斜杠会开出一个控制字，`\o` 就不再是指令里的字母 o），
//! 解一遍之后 `TOC \o "1-2" \h` 与 docx 的 `w:instrText` 逐字同一个形状 —— 所以种类
//! 与开关这两把尺子三家共用一只，不各造。这一族群里没有「空指令」那种分别（群里没字
//! 与没群都是 null），而 `\fldrslt` 的显示文字同时就是页面上的字（游标不吞整群），
//! 所以 `cached` 与正文那本是同一串字的两个视角。域套域时内外各算一枚。
//!
//! 两个数分开交：`control_words` 是流里 `\field` 的裸计数，`fields_total` 是这本账的
//! 行数 —— 坐在不认识的星号群里的 `\field` 只在裸计数里露头。
//!
//! 生产者差别（同一份种子 `fields-mix.docx` 与它的 LibreOffice 重写）实测四条：两枚
//! `fldSimple` 全被改写成复杂式（`forms.simple` 2 → 0）；`\* MERGEFORMAT` 整个丢掉；
//! `REF _RefMix1 \r \h` 变成 `\r \r \h`（多写一遍同一个开关）；`STYLEREF "标题 1"` 变成
//! `"标题 1 (user)"`，而它重算出来的缓存值是字面错误串 `错误: 引用源未找到`。DATE 与
//! TIME 的缓存值被重算成导出时刻。断掉那一条所在的段落直接不再写（15 行 → 13 行，
//! begin / separate / end 各 13 而 instrText 只有 12 —— 空指令那条的 `w:instrText`
//! 根本没写）。See fact 129.

use crate::office_doc::kept_attrs;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// 开关的形状：反斜杠 + 一个字母或星号。`\@` 与 `\-` 那类「反斜杠 + 标点」不在这把
/// 尺子里 —— 要扩就三家一起扩，不一家扩一半（第二读者 `FIELD_SWITCH_RE` 同一个形状）
fn field_switches(had: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(text) = had else { return out };
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    while at + 1 < chars.len() {
        if chars[at] == '\\' {
            let nxt = chars[at + 1];
            if nxt.is_ascii_alphabetic() || nxt == '*' {
                out.push(format!("\\{nxt}"));
                at += 2;
                continue;
            }
        }
        at += 1;
    }
    out
}

/// 指令里第一个空格前的那个词 —— OOXML 与 RTF 把「哪种域」写在它身上。
/// 整串先 trim 再切：` PAGE ` 的种类是 PAGE，不是空串（第二读者同一个顺序）
fn field_kind(had: Option<&str>) -> Option<String> {
    let head = had?.trim();
    let head = head.split(' ').next()?.trim();
    (!head.is_empty()).then(|| head.to_string())
}

/// 计数进一本按文件顺序排的表（值只增，所以不判重）
fn bump(book: &mut serde_json::Map<String, Value>, key: String) {
    let next = book.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
    book.insert(key, json!(next));
}

/// 种类表：没有种类的（指令为 null，或指令只有空格）不入表
fn kind_book(rows: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        if let Value::String(had) = &one["kind"] {
            bump(&mut out, had.clone());
        }
    }
    Value::Object(out)
}

fn switch_book(rows: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        for tok in one["switches"].as_array().into_iter().flatten() {
            if let Value::String(had) = tok {
                bump(&mut out, had.clone());
            }
        }
    }
    Value::Object(out)
}

fn parts_book(rows: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        if let Value::String(had) = &one["part"] {
            bump(&mut out, had.clone());
        }
    }
    Value::Object(out)
}

/// 每行补上种类与开关（行本身是解出来那份，两把尺子在这里量一次）
fn with_kind(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|one| {
            let mut held = one.clone();
            let had = one["instruction"].as_str().map(String::from);
            held["kind"] = match field_kind(had.as_deref()) {
                Some(word) => json!(word),
                None => Value::Null,
            };
            held["switches"] = json!(field_switches(had.as_deref()));
            held
        })
        .collect()
}

/// 那一个键是 null 的行数（「没写」与「写了空串」是两行不同的账，各自数）
fn nulls(rows: &[Value], want: &str) -> usize {
    rows.iter().filter(|one| one[want].is_null()).count()
}

/// 那一个键写着空串的行数
fn empties(rows: &[Value], want: &str) -> usize {
    rows.iter().filter(|one| one[want] == json!("")).count()
}

/// 那个整数键大于 0 的行数
fn deeper(rows: &[Value], want: &str) -> usize {
    rows.iter()
        .filter(|one| one[want].as_u64().unwrap_or(0) > 0)
        .count()
}

/// 「这份 docx 里哪些部件还可能写域」：按**文件名**认，不去查 rels（页脚在哪一份，
/// rels 说了才算，而这一本只要多扫几份、不要漏扫）
fn docx_text_part(name: &str) -> bool {
    if !name.starts_with("word/") || !name.ends_with(".xml") {
        return false;
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    base == "document.xml"
        || base.starts_with("header")
        || base.starts_with("footer")
        || base == "footnotes.xml"
        || base == "endnotes.xml"
}

/// 一条域的现场：栈里开着的那些行号，收尾时按行号回写
struct Ledger {
    rows: Vec<Value>,
    stack: Vec<usize>,
    para: i64,
    part: String,
    markers: serde_json::Map<String, Value>,
    loose: serde_json::Map<String, Value>,
}

impl Ledger {
    fn new_row(&mut self, form: &str, instr: Option<&str>) -> usize {
        let simple = form == "simple";
        self.rows.push(json!({
            "part": self.part.clone(),
            "index": self.rows.len(),
            "para": self.para,
            "form": form,
            "instruction": instr.map(String::from),
            "pieces": 0,
            "cached": if simple { json!("") } else { Value::Null },
            "has_separate": if simple { Value::Null } else { json!(false) },
            "dirty_written": Value::Null,
            "closed": if simple { Value::Null } else { json!(false) },
            "depth": self.stack.len(),
        }));
        let at = self.rows.len() - 1;
        self.stack.push(at);
        at
    }

    fn mark(&mut self, key: &str) {
        bump(&mut self.markers, key.to_string());
    }

    fn loose_mark(&mut self, key: &str) {
        bump(&mut self.loose, key.to_string());
    }

    /// 复杂式那条链的当前一条（简单式也在栈里，所以按 form 判）
    fn open(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    fn walk(&mut self, node: &Node) {
        match node.local() {
            "p" => self.para += 1,
            "fldChar" => {
                let held = node.attr_local("fldCharType");
                if held == Some("begin") {
                    self.mark("begin");
                    self.new_row("complex", None);
                } else if held == Some("separate") {
                    self.mark("separate");
                    match self.open() {
                        None => self.loose_mark("separate"),
                        Some(at) => self.rows[at]["has_separate"] = json!(true),
                    }
                } else if held == Some("end") {
                    self.mark("end");
                    match self.open() {
                        Some(at) if self.rows[at]["form"] == "complex" => {
                            self.stack.pop();
                            self.rows[at]["closed"] = json!(true);
                        }
                        _ => self.loose_mark("end"),
                    }
                }
                if let Some(dirtied) = node.attr_local("dirty") {
                    self.mark("dirty");
                    // 只记第一条域碰到的第一枚：后面的 `w:dirty` 覆盖它不是同一句话
                    if let Some(at) = self.open() {
                        if self.rows[at]["dirty_written"].is_null() {
                            self.rows[at]["dirty_written"] = json!(dirtied);
                        }
                    }
                }
            }
            "instrText" => {
                self.mark("instrText");
                match self.open() {
                    None => self.loose_mark("instrText"),
                    Some(at) => {
                        let had = node.text();
                        let pieces = self.rows[at]["pieces"].as_u64().unwrap_or(0) + 1;
                        // 生产者把一条指令切成几段就拼几段（先算新值再写回：同一行的
                        // 可变与不可变借用不能同时挂在借用检查器面前）
                        let next = match &self.rows[at]["instruction"] {
                            Value::Null => json!(had),
                            Value::String(before) => json!(before.clone() + &had),
                            _ => Value::Null,
                        };
                        self.rows[at]["pieces"] = json!(pieces);
                        self.rows[at]["instruction"] = next;
                    }
                }
            }
            "t" => {
                if let Some(at) = self.open() {
                    let takes = self.rows[at]["form"] == "simple"
                        || self.rows[at]["has_separate"] == json!(true);
                    if takes {
                        let had = node.text();
                        let next = match &self.rows[at]["cached"] {
                            Value::Null => json!(had),
                            Value::String(before) => json!(before.clone() + &had),
                            _ => Value::Null,
                        };
                        self.rows[at]["cached"] = next;
                    }
                }
            }
            "fldSimple" => {
                self.mark("simple");
                self.new_row("simple", node.attr_local("instr"));
                // 栈里这一条之上的深度：收掉自己，体内没闭合的那些也一起收
                // （文件断在哪就报断在哪）
                let depth = self.stack.len() - 1;
                for one in &node.children {
                    self.walk(one);
                }
                while self.stack.len() > depth {
                    self.stack.pop();
                }
                return;
            }
            _ => {}
        }
        for one in &node.children {
            self.walk(one);
        }
    }
}

/// 域那一份账（OOXML）
pub(crate) fn docx(bytes: &[u8], document: &Node, limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| docx_text_part(one))
        .collect();
    names.sort();
    if !names.iter().any(|one| one == "word/document.xml") {
        return json!({"available": false});
    }
    // 正文那份调用方已经解过，其余部件各解一份；两份放不进一个容器就分两处再借回来
    let mut owned: Vec<(String, Node)> = Vec::new();
    for name in &names {
        if name == "word/document.xml" {
            continue;
        }
        let Ok(had) = zipread::member(bytes, name, DEFAULT_MEMBER_CAP) else {
            continue;
        };
        owned.push((name.clone(), xmlscan::parse_str(&had.as_text())));
    }
    let mut parts: Vec<(&str, &Node)> = Vec::new();
    for name in &names {
        if name == "word/document.xml" {
            parts.push((name.as_str(), document));
        } else if let Some((_, root)) = owned.iter().find(|(one, _)| one == name) {
            parts.push((name.as_str(), root));
        }
    }

    let mut me = Ledger {
        rows: Vec::new(),
        stack: Vec::new(),
        para: 0,
        part: String::new(),
        markers: serde_json::Map::new(),
        loose: serde_json::Map::new(),
    };
    for key in ["begin", "separate", "end", "simple", "instrText", "dirty"] {
        me.markers.insert(key.to_string(), json!(0));
    }
    for key in ["separate", "end", "instrText"] {
        me.loose.insert(key.to_string(), json!(0));
    }
    for (name, root) in parts {
        me.part = name.to_string();
        me.para = -1;
        me.stack.clear();
        me.walk(root);
    }
    finish("ooxml", with_kind(&me.rows), &me.markers, &me.loose, limit)
}

fn finish(
    family: &str,
    rows: Vec<Value>,
    markers: &serde_json::Map<String, Value>,
    loose: &serde_json::Map<String, Value>,
    limit: usize,
) -> Value {
    let parts = parts_book(&rows);
    let kinds = kind_book(&rows);
    let switches = switch_book(&rows);
    json!({
        "family": family,
        "available": true,
        "fields_total": rows.len(),
        "listed": rows.len().min(limit),
        "cut": rows.len() > limit,
        "forms": {
            "simple": rows.iter().filter(|one| one["form"] == "simple").count(),
            "complex": rows.iter().filter(|one| one["form"] == "complex").count(),
        },
        "kinds": kinds,
        "switch_tokens": switches,
        "parts": parts,
        "markers": Value::Object(markers.clone()),
        "loose": Value::Object(loose.clone()),
        "unclosed": rows.iter().filter(|one| one["closed"] == json!(false)).count(),
        "no_separate": rows.iter().filter(|one| one["has_separate"] == json!(false)).count(),
        "no_instruction": nulls(&rows, "instruction"),
        "empty_instruction": rows
            .iter()
            .filter(|one| matches!(&one["instruction"], Value::String(had) if had.trim().is_empty()))
            .count(),
        "dirty_on": rows.iter().filter(|one| one["dirty_written"] == json!("true")).count(),
        "cached_written": rows.iter().filter(|one| !one["cached"].is_null()).count(),
        "cached_empty": empties(&rows, "cached"),
        "nested": deeper(&rows, "depth"),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}

/// 域那一份账（ODF）：种类就是元素名，没有指令串这一问
fn odf_walk(
    root: &Node,
    part: &str,
    depth: usize,
    rows: &mut Vec<Value>,
    seen: &mut BTreeSet<String>,
    decls: &mut Vec<String>,
) {
    let written = root.name.as_str();
    let (head, local) = match written.rsplit_once(':') {
        Some((one, two)) => (one, two),
        None => ("", written),
    };
    if head != "text" {
        for one in &root.children {
            odf_walk(one, part, depth, rows, seen, decls);
        }
        return;
    }
    seen.insert(local.to_string());
    // 域可以套域（`text:sequence` 里再放 `text:span` 之类），所以进了这一族深度才加一
    let kids = if local == "sequence-decl" {
        decls.push(root.attr_local("name").unwrap_or_default().to_string());
        depth
    } else if ODF_FIELD_NAMES.contains(&local) {
        rows.push(json!({
            "part": part,
            "index": rows.len(),
            "element": written,
            "kind": local,
            "instruction": Value::Null,
            "switches": [],
            "attrs": kept_attrs(root),
            "cached": root.text(),
            "depth": depth,
            "measured": ODF_MEASURED.contains(&local),
        }));
        depth + 1
    } else {
        depth
    };
    for one in &root.children {
        odf_walk(one, part, kids, rows, seen, decls);
    }
}

/// 认字表。**实测过**的只有下面那七个（这一库 41 份 odt/ott 里出现过的全部）；其余是
/// spec 里这一族的名字，认了它们不等于量过它们 —— 所以另交一份 `text_names`
const ODF_FIELD_NAMES: [&str; 42] = [
    "annotation-count",
    "bookmark-ref",
    "char-count",
    "conditional-text",
    "creation-date",
    "database-column-count",
    "database-display",
    "database-row-count",
    "database-select",
    "database-set-count",
    "date",
    "dde-item",
    "editing-duration",
    "execute-macro",
    "expression",
    "formula",
    "hidden-text",
    "line-count",
    "line-number",
    "measure-time",
    "modified-date",
    "non-word-count",
    "page-count",
    "page-number",
    "page-number-format",
    "page-number-orientation",
    "paragraph-count",
    "print-date",
    "print-time",
    "publication-state",
    "sender-company",
    "sender-first-name",
    "sender-full-name",
    "sender-initials",
    "sender-last-name",
    "sender-title",
    "sequence",
    "sheet-count",
    "syllable-count",
    "table-count",
    "time",
    "word-count",
];
/// 上面那张表里这一库真量过的七个（`unmeasured` 就是「认到却没量过」的那几行）
const ODF_MEASURED: [&str; 7] = [
    "bookmark-ref",
    "database-display",
    "date",
    "page-count",
    "page-number",
    "sequence",
    "time",
];

/// 域那一份账（ODF）：种类是元素名，两份件都扫
pub(crate) fn odf(root: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut decls: Vec<String> = Vec::new();
    odf_walk(root, "content.xml", 0, &mut rows, &mut seen, &mut decls);
    if let Some(other) = styles {
        odf_walk(other, "styles.xml", 0, &mut rows, &mut seen, &mut decls);
    }
    let mut refs = serde_json::Map::new();
    for one in rows.iter() {
        if let Some(had) = attr_local_in(one, "reference-format") {
            bump(&mut refs, had);
        }
    }
    let total = rows.len();
    let declared = decls.len();
    decls.sort();
    json!({
        "family": "odf",
        "available": true,
        "fields_total": total,
        "listed": total.min(limit),
        "cut": total > limit,
        "kinds": kind_book(&rows),
        "elements": elements_book(&rows),
        "parts": parts_book(&rows),
        "reference_formats": Value::Object(refs),
        "sequence_declarations": declared,
        "sequence_declared": json!(decls),
        "unmeasured": rows.iter().filter(|one| one["measured"] == json!(false)).count(),
        "cached_empty": empties(&rows, "cached"),
        "nested": deeper(&rows, "depth"),
        "text_names": seen.into_iter().collect::<Vec<String>>(),
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 那一行自己写的属性里，把前缀去掉找那一个（`text:reference-format` 与
/// `reference-format` 都算，跟第二读者 `_local_in` 同一把尺子）
fn attr_local_in(one: &Value, want: &str) -> Option<String> {
    let held = one.get("attrs")?.as_object()?;
    for (key, val) in held {
        let local = key.rsplit_once(':').map_or(key.as_str(), |(_, two)| two);
        if local == want {
            if let Value::String(text) = val {
                return Some(text.clone());
            }
        }
    }
    None
}

/// 文件写的那个名字那一本（`text:page-number` 这种整串，与 kinds 那本一对看得出前缀写法）
fn elements_book(rows: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        if let Value::String(had) = &one["element"] {
            bump(&mut out, had.clone());
        }
    }
    Value::Object(out)
}

/// 域那一份账（RTF）：行是流那边顺手收的，这里只把种类与开关算出来
pub(crate) fn rtf(rows: &[Value], control_words: usize, limit: usize) -> Value {
    let rows: Vec<Value> = rows
        .iter()
        .map(|one| {
            let mut held = one.clone();
            let had = one["instruction"].as_str().map(String::from);
            held["kind"] = match field_kind(had.as_deref()) {
                Some(word) => json!(word),
                None => Value::Null,
            };
            held["switches"] = json!(field_switches(had.as_deref()));
            held
        })
        .collect();
    let total = rows.len();
    json!({
        "family": "rtf",
        "available": true,
        "fields_total": total,
        "listed": total.min(limit),
        "cut": total > limit,
        "control_words": control_words,
        "kinds": kind_book(&rows),
        "switch_tokens": switch_book(&rows),
        "no_instruction": nulls(&rows, "instruction"),
        "no_result": rows.iter().filter(|one| one["has_result"] == json!(false)).count(),
        "cached_empty": empties(&rows, "cached"),
        "toc": rows
            .iter()
            .filter(|one| matches!(&one["instruction"], Value::String(had) if crate::rtf::is_toc_instruction(had)))
            .count(),
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
