//! 正文那些手指的账：写着的颜色名字点到主题那十二格中的哪一格，从哪条路过去的。
//!
//! 主题那一本（`theme_ledger`）读的是「格子里坐着什么」，这一本读的是「谁点了这一格」。
//! 三家各写各的：Word 在 `w:color` 上写 `themeColor="text1"`，DrawingML 写
//! `<a:schemeClr val="tx1"/>`，Excel 写 `theme="4"` 一个序号。名字点到格有三条来路，
//! 都记在 `via` 上，解不出来就交 null，不照着规范替文件补全。
//!
//! 实测（240 份件里 30404 条手指：Word 那一路 25084、DrawingML 5210、Excel 序号 110；
//! 走过 2189 个 `.xml` 部件，543 个部件里手指在场）：
//! * Word 那一路**每一条都另写了一遍六位实色**（`@val`）当影子，所以那一路可以自己跟自己
//!   核对：不带修饰符的 22440 条逐条与本包主题格对上，`mismatched` 0 条 —— 这张表是自检的；
//! * DrawingML 那一路 5210 条都写了 `val`（一条都没少），但它没有伴随实色可核，
//!   所以那一族的 `matches` 全为 null；名字坐实靠的是文件自己写的 `a:clrMap`——
//!   实测 83 份对照分布在 21 个包里，全写在母版那两种部件上（`slideMasterN.xml` 81 份、
//!   `notesMaster1.xml` 2 份，主题那一份件里一个都没有），说的是同一套十二对
//!   （`alias_conflict` 0），于是 `tx1` / `bg1` 这一族 580 条由文件自己解，
//!   而 42 条落在没有对照的包里就交解不出（不是猜）；
//! * 别名表（`via = "wml-alias"`）只收 Word 那一族、且只用「无修饰符而写了影子」量出来的
//!   那四个名字：`text1`→`dk1`、`background1`→`lt1`、`text2`→`dk2`、`dark1`→`dk1`，共 21628 条；
//!   `dark2`（8 条，整批带 shade）与 `phClr`（2079 条，主题占位色，压根不是那十二格之一）
//!   都不在这张表里，也不补；
//! * 序号那一族有**两读**：规范顺序与 Excel 实际用的顺序前四格要对调，所以 `slot` 与
//!   `alt_slot` 都交，只有两边说同一格的 8 条算 `index_agree`，另 102 条是 `index_disagree`；
//! * 本包主题件里那一格有多份件说不同值时不判（一份 pptx 一个母版一个主题件，实测 12 个），
//!   所以 `slots` 那一本交十二行、每行带 `parts` 与 `agree`，判得住与否留给 `matches`。
//!
//! 不做的事与主题那一本同一条：**不替文件算色**。带 `tint` / `shade` / `themeTint` /
//! `themeShade` / `satMod` 的 4540 条一律 `matches = null`（对不上是应该的，对上才是巧合），
//! 字面量按写的交；四种「判不住」在合计里分列（`skip_modified` / `skip_no_slot` /
//! `skip_no_literal` / `skip_multi_value`），不揉成一格。
//!
//! 非 ZIP 的件（`.doc` / `.ppt` / `.xls` / RTF / PDF）交一本零条的账而不是缺键 ——
//! 那两族的主题坐在 CFB 的 `Theme` 流与 RTF 的 `{\*\themedata}` 群里，本机做不出凭据，
//! 所以那一本还没开。ODF 那 67 份没有主题这个概念：`parts_scanned` 非零而 `refs` 零条。

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::theme_ledger::{is_theme_part, CANON};
use crate::xmlscan::{self, Node};
use crate::zipread;

/// Excel 那个序号的第二读：规范把 0 当作 dk1，而 Excel 实际用的那张表前四格是反的，
/// 于是两条都交出去，谁胜出留给 `index_agree`
const EXCEL: [&str; 12] = [
    "lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// Word 那一族的四个别名：只从「无修饰符而写了影子」的那批量出来，所以只给 `wmlColor` 用
/// （`accent1` / `accent2` 那 1600 条不需要表 —— 名字本身就是一格）
const WML_ALIAS: [(&str, &str); 4] = [
    ("background1", "lt1"),
    ("dark1", "dk1"),
    ("text1", "dk1"),
    ("text2", "dk2"),
];

/// 合计里那些一格一格的计数，先声明成 0：缺键与零条是两件事
const COUNTERS: [&str; 23] = [
    "refs",
    "wml_color",
    "scheme_clr",
    "theme_index",
    "parts_scanned",
    "parts_unread",
    "parts_with_refs",
    "theme_parts",
    "clr_map_written",
    "alias_names",
    "alias_conflict",
    "in_slots",
    "off_slots",
    "resolved",
    "unresolved",
    "matched",
    "mismatched",
    "skip_modified",
    "skip_no_literal",
    "skip_no_slot",
    "skip_multi_value",
    "index_agree",
    "index_disagree",
];

/// 合计里那六本映射，先声明成空对象
const BOOKS: [&str; 6] = [
    "by_kind",
    "by_name",
    "by_holder",
    "by_via",
    "slot_refs",
    "mods_seen",
];

/// 一行手指：十四键一次写全再填，谁都不许缺键
struct Row {
    part: String,
    kind: &'static str,
    at: String,
    holder: String,
    name: Option<String>,
    slot: Option<String>,
    via: Option<&'static str>,
    alt_slot: Option<String>,
    literal: Option<String>,
    tint: Option<String>,
    shade: Option<String>,
    mods: Vec<String>,
    in_slots: bool,
    judged: Option<bool>,
}

impl Row {
    fn new(part: &str, kind: &'static str, at: &str, holder: &str) -> Row {
        Row {
            part: part.to_string(),
            kind,
            at: at.to_string(),
            holder: holder.to_string(),
            name: None,
            slot: None,
            via: None,
            alt_slot: None,
            literal: None,
            tint: None,
            shade: None,
            mods: Vec::new(),
            in_slots: false,
            judged: None,
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "part": self.part,
            "kind": self.kind,
            "at": self.at,
            "holder": self.holder,
            "name": self.name,
            "slot": self.slot,
            "via": self.via,
            "alt_slot": self.alt_slot,
            "literal": self.literal,
            "tint": self.tint,
            "shade": self.shade,
            "mods": self.mods,
            "in_slots": self.in_slots,
            "matches": self.judged,
        })
    }
}

/// 往一张计数表里加一笔（按写的值当键，没写的记成 `(没写)`，与主题那本同一口径）
fn bump_ref(table: &mut serde_json::Map<String, Value>, key: Option<&str>, add: usize) {
    let name = key.unwrap_or("(没写)").to_string();
    let next = table.get(&name).and_then(Value::as_u64).unwrap_or(0) + add as u64;
    table.insert(name, json!(next));
}

/// 合计里加一笔（`bump` 与主题那本同一条：0 不加，免得把没走到的路记成一次）
fn bump(sum: &mut serde_json::Map<String, Value>, key: &str, add: usize) {
    if add == 0 {
        return;
    }
    let next = sum.get(key).and_then(Value::as_u64).unwrap_or(0) + add as u64;
    sum.insert(key.to_string(), json!(next));
}

/// 拿一本映射（不存在就开一本空的）
fn book<'a>(
    sum: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let had = sum
        .entry(key.to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("计数表一定是对象");
    had
}

/// 拿 `by_name` / `by_holder` 那一层里某个 kind 的那本
fn sub_book<'a>(
    sum: &'a mut serde_json::Map<String, Value>,
    key: &str,
    kind: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let outer = book(sum, key);
    let inner = outer
        .entry(kind.to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("一层映射里那一本也是对象");
    inner
}

fn new_totals() -> serde_json::Map<String, Value> {
    let mut sum = serde_json::Map::new();
    for key in COUNTERS {
        sum.insert(key.to_string(), json!(0));
    }
    for key in BOOKS {
        sum.insert(key.to_string(), json!({}));
    }
    sum
}

/// 这个名字点到哪一格、从哪条路过去的：名字本身就是一格 → `name`；
/// 本包 `a:clrMap` 说过 → `clrMap`；Word 那一族量出来的四个别名 → `wml-alias`
fn slot_of(
    nm: Option<&str>,
    kind: &str,
    alias: &HashMap<String, String>,
) -> (Option<String>, bool, Option<&'static str>) {
    let Some(one) = nm else {
        return (None, false, None);
    };
    if CANON.contains(&one) {
        return (Some(one.to_string()), true, Some("name"));
    }
    if let Some(got) = alias.get(one) {
        return (Some(got.clone()), false, Some("clrMap"));
    }
    if kind == "wmlColor" {
        if let Some((_, got)) = WML_ALIAS.iter().find(|(from, _)| *from == one) {
            return (Some(got.to_string()), false, Some("wml-alias"));
        }
    }
    (None, false, None)
}

/// Word 那一路：`themeColor` 点名字，`val` 又写了一遍实色当影子（按写的交，不换算）
fn wml_row(part: &str, at: &str, holder: &str, kid: &Node, alias: &HashMap<String, String>) -> Row {
    let nm = kid.attr_local("themeColor");
    let mut one = Row::new(part, "wmlColor", at, holder);
    one.name = nm.map(|value| value.to_string());
    let (slot, in_slots, via) = slot_of(one.name.as_deref(), "wmlColor", alias);
    one.slot = slot;
    one.in_slots = in_slots;
    one.via = via;
    one.literal = kid.attr_local("val").map(|value| value.to_string());
    one.tint = kid.attr_local("themeTint").map(|value| value.to_string());
    one.shade = kid.attr_local("themeShade").map(|value| value.to_string());
    one.mods = ["themeTint", "themeShade"]
        .iter()
        .filter(|key| kid.attr_local(key).is_some())
        .map(|key| (*key).to_string())
        .collect();
    one
}

/// 一个属性名的局部名（`w:themeColor` 读成 `themeColor`，与 `written_attrs` 同一条）
fn attr_local_name(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

/// DrawingML 那一路：`val` 点名字，修饰符是孩子元素；这一路没有伴随实色可核
fn scheme_row(
    part: &str,
    at: &str,
    holder: &str,
    kid: &Node,
    alias: &HashMap<String, String>,
) -> Row {
    let nm = kid.attr_local("val");
    let mut one = Row::new(part, "schemeClr", at, holder);
    one.name = nm.map(|value| value.to_string());
    let (slot, in_slots, via) = slot_of(one.name.as_deref(), "schemeClr", alias);
    one.slot = slot;
    one.in_slots = in_slots;
    one.via = via;
    one.mods = kid.children.iter().map(|c| c.local().to_string()).collect();
    // tint / shade 各按「第一个那样名的孩子」的 `val`，两个键分列不并成一个
    for which in ["tint", "shade"] {
        if let Some(head) = kid.children.iter().find(|c| c.local() == which) {
            let got = head.attr_local("val").map(|value| value.to_string());
            if which == "tint" {
                one.tint = got;
            } else {
                one.shade = got;
            }
        }
    }
    one
}

/// Excel 那一路：点的是序号，所以两种顺序都交；`rgb` 有就交（实测一条都没有）
fn index_row(part: &str, at: &str, holder: &str, kid: &Node) -> Row {
    let raw = kid.attr_local("theme");
    let mut one = Row::new(part, "themeIndex", at, holder);
    one.name = raw.map(|value| value.to_string());
    let number = raw
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(-1);
    if number >= 0 && (number as usize) < CANON.len() {
        let place = number as usize;
        one.slot = Some(CANON[place].to_string());
        one.alt_slot = Some(EXCEL[place].to_string());
        one.via = Some("index");
    }
    one.literal = kid.attr_local("rgb").map(|value| value.to_string());
    one.tint = kid.attr_local("tint").map(|value| value.to_string());
    if one.tint.is_some() {
        one.mods = vec!["tint".to_string()];
    }
    one
}

/// 影子与那一格的取值对不对得上：带修饰符的不判（不算色），点不出格的不判，
/// 本包里那一格有多值的不判 —— 三种不判在合计里分列
fn judged(one: &Row, uniq: &HashMap<String, String>) -> Option<bool> {
    let literal = one.literal.as_deref()?;
    let slot = one.slot.as_deref()?;
    if !one.mods.is_empty() {
        return None;
    }
    let want = uniq.get(slot)?;
    Some(want.eq_ignore_ascii_case(literal))
}

/// 一条手指进合计：三种点法各数各的，四条来路各数各的，判不住的四种各数各的
fn tally(sum: &mut serde_json::Map<String, Value>, one: &Row) {
    bump(sum, "refs", 1);
    bump(
        sum,
        match one.kind {
            "wmlColor" => "wml_color",
            "schemeClr" => "scheme_clr",
            _ => "theme_index",
        },
        1,
    );
    bump_ref(book(sum, "by_kind"), Some(one.kind), 1);
    bump_ref(sub_book(sum, "by_name", one.kind), one.name.as_deref(), 1);
    let seat = format!("{}/{}", one.at, one.holder);
    bump_ref(sub_book(sum, "by_holder", one.kind), Some(&seat), 1);
    bump_ref(book(sum, "by_via"), one.via, 1);
    bump(
        sum,
        if one.in_slots {
            "in_slots"
        } else {
            "off_slots"
        },
        1,
    );
    match &one.slot {
        None => bump(sum, "unresolved", 1),
        Some(got) => {
            bump(sum, "resolved", 1);
            bump_ref(book(sum, "slot_refs"), Some(got), 1);
            if one.kind == "themeIndex" {
                bump(
                    sum,
                    if one.slot == one.alt_slot {
                        "index_agree"
                    } else {
                        "index_disagree"
                    },
                    1,
                );
            }
        }
    }
    match one.judged {
        Some(true) => bump(sum, "matched", 1),
        Some(false) => bump(sum, "mismatched", 1),
        None if !one.mods.is_empty() => bump(sum, "skip_modified", 1),
        None if one.slot.is_none() => bump(sum, "skip_no_slot", 1),
        None if one.literal.is_none() => bump(sum, "skip_no_literal", 1),
        None => bump(sum, "skip_multi_value", 1),
    }
    for name in &one.mods {
        bump_ref(book(sum, "mods_seen"), Some(name), 1);
    }
}

/// 前序走一遍：先办自己再进孩子，孩子办完的顺序就是文件写的顺序（镜像同一条递归）
fn walk(
    node: &Node,
    holder: &str,
    part: &str,
    uniq: &HashMap<String, String>,
    alias: &HashMap<String, String>,
    rows: &mut Vec<Row>,
    sum: &mut serde_json::Map<String, Value>,
) {
    for kid in &node.children {
        let at = kid.local().to_string();
        let mut one = None;
        if at == "color" && kid.attr_local("themeColor").is_some() {
            one = Some(wml_row(part, &at, holder, kid, alias));
        } else if at == "schemeClr" {
            one = Some(scheme_row(part, &at, holder, kid, alias));
        } else if kid.attr_local("theme").is_some() {
            one = Some(index_row(part, &at, holder, kid));
        }
        if let Some(mut got) = one.take() {
            got.judged = judged(&got, uniq);
            tally(sum, &got);
            rows.push(got);
        }
        walk(kid, &at, part, uniq, alias, rows, sum);
    }
}

/// 属性里的命名空间声明不是数据（与 `written_attrs` 同一条口径）
fn is_declared(key: &str) -> bool {
    key == "xmlns" || key.starts_with("xmlns:")
}

/// 一个元素的局部名是不是这个（自己也算，与镜像 `root.iter()` 同一条）
fn count_local(node: &Node, want: &str) -> usize {
    let mut out = usize::from(node.local() == want);
    for kid in &node.children {
        out += count_local(kid, want);
    }
    out
}

/// 第一遍只收两样：十二格都取值是什么，以及文件自己写的名字→格对照（`a:clrMap`）
fn survey(node: &Node, values: &mut Vec<Vec<String>>, alias: &mut HashMap<String, Vec<String>>) {
    match node.local() {
        "clrScheme" => {
            for slot in &node.children {
                let name = slot.local().to_string();
                let Some(place) = CANON.iter().position(|one| *one == name) else {
                    continue;
                };
                let Some(head) = slot.children.first() else {
                    continue;
                };
                // lastClr 只是缓存的猜测，空了就得往后退一格（与镜像的 `or` 同一条）
                let got = head
                    .attr_local("lastClr")
                    .filter(|value| !value.is_empty())
                    .or_else(|| head.attr_local("val"));
                if let Some(value) = got {
                    values[place].push(value.to_string());
                }
            }
        }
        "clrMap" => {
            for (key, value) in &node.attrs {
                if is_declared(key) {
                    continue;
                }
                let name = attr_local_name(key);
                alias
                    .entry(name.to_string())
                    .or_default()
                    .push(value.to_string());
            }
        }
        _ => {}
    }
    for kid in &node.children {
        survey(kid, values, alias);
    }
}

/// 十二格各自的取值清单：值去重并大写，`parts` 是几份主题件写了这一格，
/// `agree` 是「有件写了而且说的都是同一件事」—— 没件写的时候不成立，
/// 别把「没人说话」读成「大家都同意」。同意的那些格同时是判影子用的那张表
fn slot_book(values: &[Vec<String>]) -> (Vec<Value>, HashMap<String, String>) {
    let mut rows: Vec<Value> = Vec::new();
    let mut uniq: HashMap<String, String> = HashMap::new();
    for (place, name) in CANON.iter().enumerate() {
        let mut distinct: Vec<String> =
            values[place].iter().map(|one| one.to_uppercase()).collect();
        distinct.sort();
        distinct.dedup();
        let agree = distinct.len() == 1;
        if agree {
            uniq.insert(name.to_string(), distinct[0].clone());
        }
        rows.push(json!({
            "slot": name,
            "values": distinct,
            "parts": values[place].len(),
            "agree": agree,
        }));
    }
    (rows, uniq)
}

/// 一册包的正文手指账：`.xml` 部件走两遍（第一遍收那两份对照，第二遍收手指）。
/// 非 ZIP（`.doc` / `.ppt` / `.xls` / RTF）交一本零条的账 —— 键一个不少，只是每格都空
pub fn refs(zip: &[u8], limit: usize) -> Value {
    let mut rows: Vec<Row> = Vec::new();
    let mut sum = new_totals();
    let mut values: Vec<Vec<String>> = CANON.iter().map(|_| Vec::new()).collect();
    // 十二行先按「全空」摊好：非 ZIP 也交这一本（Python 那本就是这么起步的），ZIP 分支再拿实测覆盖
    let mut slots: Vec<Value> = slot_book(&values).0;
    let mut alias: HashMap<String, Vec<String>> = HashMap::new();
    let mut uniq: HashMap<String, String> = HashMap::new();
    let mut alias_map: HashMap<String, String> = HashMap::new();
    if zip.starts_with(b"PK") {
        let (dirs, _) = zipread::entries(zip);
        // 第一遍：谁写了对照谁说话（主题件可能排在 styles 后面，所以不能一边走一边判）
        let mut good: Vec<(usize, String)> = Vec::new();
        for (place, one) in dirs.iter().enumerate() {
            if !one.name.ends_with(".xml") {
                continue;
            }
            bump(&mut sum, "parts_scanned", 1);
            let member = match zipread::read_member(zip, one, zipread::DEFAULT_MEMBER_CAP) {
                Ok(member) => member,
                Err(_) => {
                    bump(&mut sum, "parts_unread", 1);
                    continue;
                }
            };
            let parsed = xmlscan::parse(&member.data);
            // `#text` 不算根：容错解析会把「一个尖括号都没有」的一份件摊成一枚文本节点，
            // 而那在 Python 那本里是 `ParseError`（解不开）。两家对同一份件得给同一个判决。
            let Some(root) = parsed.children.first().filter(|one| one.name != "#text") else {
                bump(&mut sum, "parts_unread", 1);
                continue;
            };
            if is_theme_part(&one.name) {
                bump(&mut sum, "theme_parts", 1);
            }
            survey(root, &mut values, &mut alias);
            bump(&mut sum, "clr_map_written", count_local(root, "clrMap"));
            good.push((place, one.name.clone()));
        }
        let (book_rows, seen) = slot_book(&values);
        slots = book_rows;
        uniq = seen;
        // 名字→格那张对照：一个名字被多份件说成两格就不判（实测 83 份说的是同一套）
        for (key, got) in &alias {
            let mut distinct: Vec<&String> = got.iter().collect();
            distinct.sort();
            distinct.dedup();
            if distinct.len() == 1 {
                alias_map.insert(key.clone(), distinct[0].clone());
            } else {
                bump(&mut sum, "alias_conflict", 1);
            }
        }
        sum.insert("alias_names".to_string(), json!(alias_map.len()));
        // 第二遍：谁写手指就收谁
        for (place, name) in &good {
            let Ok(member) = zipread::read_member(zip, &dirs[*place], zipread::DEFAULT_MEMBER_CAP)
            else {
                continue;
            };
            let parsed = xmlscan::parse(&member.data);
            let Some(root) = parsed.children.first() else {
                continue;
            };
            let before = rows.len();
            walk(
                root,
                root.local(),
                name,
                &uniq,
                &alias_map,
                &mut rows,
                &mut sum,
            );
            if rows.len() > before {
                bump(&mut sum, "parts_with_refs", 1);
            }
        }
    }
    let listed: Vec<Value> = rows.iter().take(limit).map(Row::to_value).collect();
    let total = rows.len();
    json!({
        "refs": listed,
        "total": total,
        "listed": listed.len(),
        "cut": total > limit,
        "slots": slots,
        "totals": sum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 二十三个合计按声明顺序摊成一行，测试里一次看全
    fn agg(ledger: &Value) -> Value {
        Value::Array(COUNTERS.iter().map(|key| ledger[*key].clone()).collect())
    }

    fn zeros() -> Value {
        Value::Array(vec![json!(0); COUNTERS.len()])
    }

    /// 自己打一个「存储」（不压缩）的包：真件只有一份本子的形状，
    /// 而这一本要的那些形状（两份对照打架、两格多值、认不出的序号）真件里一个都没有
    fn packed(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        for (name, body) in parts {
            let raw = body.as_bytes();
            let crc = (zipread::crc32(raw) & 0xFFFF_FFFF) as u32;
            starts.push(out.len() as u32);
            out.extend_from_slice(&[b'P', b'K', 3, 4]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(raw);
        }
        let cd = out.len() as u32;
        for (index, (name, body)) in parts.iter().enumerate() {
            let raw = body.as_bytes();
            let crc = (zipread::crc32(raw) & 0xFFFF_FFFF) as u32;
            out.extend_from_slice(&[b'P', b'K', 1, 2]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&starts[index].to_le_bytes());
            out.extend_from_slice(name.as_bytes());
        }
        let size = out.len() as u32 - cd;
        let total = parts.len() as u16;
        out.extend_from_slice(&[b'P', b'K', 5, 6]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&cd.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    /// `refs` 里第一条 kind + name 都对上的行（找不到就 panic：测试要的是「那条在」）
    fn row(had: &Value, kind: &str, name: &str) -> Value {
        had["refs"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|one| one["kind"] == kind && one["name"] == name)
                    .cloned()
            })
            .unwrap_or_else(|| panic!("{} 那一路没有点 `{}` 的行", kind, name))
    }

    fn slot_book_row(had: &Value, which: &str) -> Value {
        had["slots"]
            .as_array()
            .and_then(|rows| rows.iter().find(|one| one["slot"] == which).cloned())
            .expect("那一格在场")
    }

    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

    fn theme_part(scheme: &str) -> String {
        format!(
            "<a:theme xmlns:a=\"{}\"><a:themeElements><a:clrScheme>{}</a:clrScheme></a:themeElements></a:theme>",
            A, scheme
        )
    }

    #[test]
    fn a_row_always_carries_all_fourteen_keys() {
        let one = Row::new("word/styles.xml", "wmlColor", "color", "rPr");
        let had = one.to_value();
        // `serde_json` 的 Map 是 BTreeMap：交出去的键按字母序，声明顺序只活在源里
        let mut got = had
            .as_object()
            .expect("一行是对象")
            .keys()
            .cloned()
            .collect::<Vec<String>>();
        let mut want = vec![
            "part", "kind", "at", "holder", "name", "slot", "via", "alt_slot", "literal", "tint",
            "shade", "mods", "in_slots", "matches",
        ];
        want.sort();
        got.sort();
        assert_eq!(got, want, "十四键一次写全：解不出也要有那一格");
        assert_eq!(had["slot"], Value::Null);
        assert_eq!(had["mods"], json!([]));
        assert_eq!(had["in_slots"], false);
    }

    #[test]
    fn the_three_routes_are_told_apart_and_phClr_stays_unresolved() {
        let body = format!(
            "<w:document xmlns:w=\"{W}\"><w:r><w:rPr><w:color w:val=\"FF0000\" w:themeColor=\"text1\"/>\
             <w:color w:val=\"000000\" w:themeColor=\"dark2\" w:themeShade=\"BF\"/></w:rPr></w:r>\
             <w:drawing><a:solidFill xmlns:a=\"{A}\"><a:schemeClr val=\"phClr\"/></a:solidFill></w:drawing></w:document>"
        );
        let had = refs(&packed(&[("word/document.xml", &body)]), 400);
        assert_eq!(had["total"], 3);
        assert_eq!(
            had["totals"]["by_kind"],
            json!({"wmlColor": 2, "schemeClr": 1})
        );
        assert_eq!(
            had["totals"]["by_via"],
            json!({"wml-alias": 1, "(没写)": 2}),
            "解不出来的那两路也占一格，别让它们并进「没写」以外的那一本"
        );
        assert_eq!(
            row(&had, "wmlColor", "text1"),
            json!({"part": "word/document.xml", "kind": "wmlColor", "at": "color",
                   "holder": "rPr", "name": "text1", "slot": "dk1", "via": "wml-alias",
                   "alt_slot": null, "literal": "FF0000", "tint": null, "shade": null,
                   "mods": [], "in_slots": false, "matches": null}),
            "别名那一跳落格了，而这一包没有主题件，所以影子不判"
        );
        assert_eq!(
            row(&had, "wmlColor", "dark2")["mods"],
            json!(["themeShade"]),
            "带修饰符的行先落 skip_modified，不参与对影子"
        );
        assert_eq!(row(&had, "wmlColor", "dark2")["slot"], Value::Null);
        assert_eq!(row(&had, "schemeClr", "phClr")["slot"], Value::Null);
        assert_eq!(had["totals"]["skip_modified"], 1);
        assert_eq!(had["totals"]["skip_no_slot"], 1);
        assert_eq!(had["totals"]["skip_multi_value"], 1);
        assert_eq!(had["totals"]["skip_no_literal"], 0);
    }

    #[test]
    fn the_shadow_literal_proves_the_word_route_itself() {
        let theme = theme_part(
            "<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
             <a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>\
             <a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1>",
        );
        let body = format!(
            "<w:document xmlns:w=\"{W}\"><w:r><w:rPr>\
             <w:color w:val=\"4F81BD\" w:themeColor=\"accent1\"/>\
             <w:color w:val=\"EE1111\" w:themeColor=\"text1\"/></w:rPr></w:r></w:document>"
        );
        let had = refs(
            &packed(&[
                ("word/theme/theme1.xml", &theme),
                ("word/document.xml", &body),
            ]),
            400,
        );
        assert_eq!(had["totals"]["parts_scanned"], 2);
        assert_eq!(had["totals"]["theme_parts"], 1);
        assert_eq!(had["totals"]["matched"], 1);
        assert_eq!(had["totals"]["mismatched"], 1);
        assert_eq!(row(&had, "wmlColor", "accent1")["matches"], json!(true));
        assert_eq!(
            row(&had, "wmlColor", "text1")["matches"],
            json!(false),
            "影子对不上就说对不上：这一路的自证不靠规范，靠文件自己写的那两遍"
        );
        assert_eq!(
            had["slots"][0],
            json!({"slot": "dk1", "values": ["000000"], "parts": 1, "agree": true}),
            "sysClr 那一格取的是 lastClr 那个缓存值，因为影子写的就是它"
        );
        assert_eq!(had["slots"][4]["values"], json!(["4F81BD"]));
        assert_eq!(
            had["slots"][2],
            json!({"slot": "dk2", "values": [], "parts": 0, "agree": false}),
            "没件写的那一格 agree 不成立 —— 没人说话不是大家都同意"
        );
    }

    #[test]
    fn a_sequence_number_has_two_readings_and_both_are_given() {
        let theme = theme_part(
            "<a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>\
             <a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2>\
             <a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1>",
        );
        let body = "<styleSheet><font><color theme=\"4\" tint=\"0.5\"/></font>\
             <font><color theme=\"1\"/></font><font><color theme=\"x\"/></font></styleSheet>"
            .to_string();
        let had = refs(
            &packed(&[("xl/theme/theme1.xml", &theme), ("xl/styles.xml", &body)]),
            400,
        );
        assert_eq!(had["total"], 3);
        assert_eq!(had["totals"]["index_agree"], 1, "`4` 两读都是 accent1");
        assert_eq!(had["totals"]["index_disagree"], 1, "`1` 一名两读，都交");
        let first = row(&had, "themeIndex", "4");
        assert_eq!(first["slot"], "accent1");
        assert_eq!(first["alt_slot"], "accent1");
        assert_eq!(first["via"], "index");
        assert_eq!(
            first["in_slots"], false,
            "序号那一读不算「名字本身就是一格」"
        );
        assert_eq!(
            first["mods"],
            json!(["tint"]),
            "Excel 的 tint 是属性而不是孩子元素，照样记进修饰符那一本"
        );
        assert_eq!(first["tint"], "0.5");
        let second = row(&had, "themeIndex", "1");
        assert_eq!(second["slot"], "lt1");
        assert_eq!(second["alt_slot"], "dk1");
        assert_eq!(second["matches"], Value::Null, "序号那一族没有影子可对");
        assert_eq!(
            row(&had, "themeIndex", "x")["slot"],
            Value::Null,
            "认不出的序号就说认不出"
        );
        assert_eq!(had["totals"]["unresolved"], 1);
        assert_eq!(had["totals"]["skip_no_literal"], 1);
    }

    #[test]
    fn the_files_own_clr_map_is_what_resolves_tx1() {
        let theme = theme_part(
            "<a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>",
        );
        let mapped = format!(
            "<p:sldMaster xmlns:a=\"{A}\" xmlns:p=\"{P}\">\
             <p:cSld><p:sp><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill></p:sp></p:cSld>\
             <p:clrMap bg1=\"lt1\" tx1=\"dk1\"/></p:sldMaster>"
        );
        let other = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:p=\"{P}\">\
             <p:background><p:bgPr><a:solidFill><a:schemeClr val=\"bg1\"/></a:solidFill></p:bgPr></p:background>\
             </p:presentation>"
        );
        let had = refs(
            &packed(&[
                ("ppt/theme/theme1.xml", &theme),
                ("ppt/slideMasters/slideMaster1.xml", &mapped),
                ("ppt/presentation.xml", &other),
            ]),
            400,
        );
        assert_eq!(had["totals"]["clr_map_written"], 1);
        assert_eq!(had["totals"]["alias_names"], 2);
        assert_eq!(had["totals"]["alias_conflict"], 0);
        assert_eq!(had["totals"]["by_via"], json!({"clrMap": 2}));
        assert_eq!(row(&had, "schemeClr", "tx1")["slot"], "dk1");
        assert_eq!(
            row(&had, "schemeClr", "bg1")["slot"],
            "lt1",
            "对照是本包那份母版自己写的，跳一格也算跳对"
        );
        assert_eq!(had["totals"]["resolved"], 2);
        assert_eq!(had["totals"]["unresolved"], 0);
    }

    #[test]
    fn a_pointer_without_a_map_in_its_package_is_left_unresolved() {
        let theme = theme_part(
            "<a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>",
        );
        let body = format!(
            "<p:presentation xmlns:a=\"{A}\" xmlns:p=\"{P}\">\
             <p:solidFill><a:schemeClr val=\"tx1\"/></p:solidFill></p:presentation>"
        );
        let had = refs(
            &packed(&[
                ("ppt/theme/theme1.xml", &theme),
                ("ppt/presentation.xml", &body),
            ]),
            400,
        );
        assert_eq!(had["totals"]["clr_map_written"], 0);
        assert_eq!(had["totals"]["alias_names"], 0);
        assert_eq!(
            had["refs"][0],
            json!({"part": "ppt/presentation.xml", "kind": "schemeClr", "at": "schemeClr",
                   "holder": "solidFill", "name": "tx1", "slot": null, "via": null,
                   "alt_slot": null, "literal": null, "tint": null, "shade": null,
                   "mods": [], "in_slots": false, "matches": null}),
            "tx1 在规范里当然是 dk1，可这一包没写对照 —— 补上就是替文件说话（实测这类 42 条）"
        );
        assert_eq!(had["totals"]["skip_no_slot"], 1);
    }

    #[test]
    fn two_maps_that_disagree_stop_judging_that_name() {
        let theme = theme_part(
            "<a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>",
        );
        let one = format!("<p:sldMaster xmlns:p=\"{P}\"><p:clrMap tx1=\"dk1\"/></p:sldMaster>");
        let two = format!("<p:sldMaster xmlns:p=\"{P}\"><p:clrMap tx1=\"lt1\"/></p:sldMaster>");
        let body = format!(
            "<a:stuff xmlns:a=\"{A}\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill></a:stuff>"
        );
        let had = refs(
            &packed(&[
                ("ppt/theme/theme1.xml", &theme),
                ("ppt/slideMasters/slideMaster1.xml", &one),
                ("ppt/slideMasters/slideMaster2.xml", &two),
                ("ppt/notes.xml", &body),
            ]),
            400,
        );
        assert_eq!(had["totals"]["clr_map_written"], 2);
        assert_eq!(had["totals"]["alias_conflict"], 1);
        assert_eq!(had["totals"]["alias_names"], 0, "打架的那个名字不进对照表");
        assert_eq!(row(&had, "schemeClr", "tx1")["slot"], Value::Null);
    }

    #[test]
    fn two_theme_parts_that_disagree_stop_judging_not_picking_one() {
        let left = theme_part("<a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1>");
        let right = theme_part("<a:accent1><a:srgbClr val=\"18A303\"/></a:accent1>");
        let body = format!(
            "<w:document xmlns:w=\"{W}\"><w:r><w:rPr><w:color w:val=\"4F81BD\" w:themeColor=\"accent1\"/></w:rPr></w:r></w:document>"
        );
        let had = refs(
            &packed(&[
                ("word/theme/theme1.xml", &left),
                ("word/theme/theme2.xml", &right),
                ("word/document.xml", &body),
            ]),
            400,
        );
        assert_eq!(
            slot_book_row(&had, "accent1"),
            json!({"slot": "accent1", "values": ["18A303", "4F81BD"], "parts": 2, "agree": false}),
            "两份件各说一个值就都列出来，不替文件挑一个"
        );
        assert_eq!(had["totals"]["theme_parts"], 2);
        assert_eq!(row(&had, "wmlColor", "accent1")["slot"], "accent1");
        assert_eq!(
            row(&had, "wmlColor", "accent1")["matches"],
            Value::Null,
            "名字点是准的，但那一格在本包多值，所以影子不判"
        );
        assert_eq!(had["totals"]["skip_multi_value"], 1);
    }

    #[test]
    fn only_xml_parts_are_walked_and_a_part_without_a_root_is_counted_once() {
        let had = refs(
            &packed(&[
                ("word/document.xml", "不是一份 XML，一个尖括号都没有"),
                ("word/empty.xml", ""),
                ("word/media/image.png", "也不是"),
            ]),
            400,
        );
        assert_eq!(had["totals"]["parts_scanned"], 2, "只走 `.xml` 部件");
        assert_eq!(
            had["totals"]["parts_unread"], 2,
            "纯文本在容错解析下只剩一枚 `#text`，那不是部件的根；空件一个节点都没有。两家按「解不开」各记一次"
        );
        assert_eq!(had["total"], 0);
        assert_eq!(had["listed"], 0);
        assert_eq!(had["cut"], false);
    }

    #[test]
    fn a_limit_cuts_the_list_but_not_the_arithmetic() {
        let had = refs(&fixture("bkmks.docx"), 3);
        assert_eq!(had["total"], 543);
        assert_eq!(had["listed"], 3);
        assert_eq!(had["cut"], true);
        assert_eq!(
            had["totals"]["refs"], 543,
            "合计是整份的账，不是清单那几条的账"
        );
        assert_eq!(had["totals"]["matched"], 466);
        assert_eq!(had["slots"][0]["values"], json!(["000000"]));
        let deep = refs(&fixture("bkmks.docx"), 400);
        assert_eq!(deep["listed"], 400);
        assert_eq!(deep["cut"], true);
    }

    #[test]
    fn the_word_route_shadows_itself_and_never_disagrees() {
        let had = refs(&fixture("bkmks.docx"), 400);
        assert_eq!(
            agg(&had["totals"]),
            json!([
                543, 520, 23, 0, 13, 0, 3, 1, 0, 0, 0, 77, 466, 527, 16, 466, 0, 65, 7, 5, 0, 0, 0
            ]),
            "二十三个合计按声明顺序摊平：word 家这一本是最满的那一种"
        );
        assert_eq!(
            had["totals"]["mismatched"], 0,
            "影子逐条对得上，这一路是自证的"
        );
        assert_eq!(
            had["refs"][0],
            json!({"part": "word/styles.xml", "kind": "wmlColor", "at": "color",
                   "holder": "rPr", "name": "accent1", "slot": "accent1", "via": "name",
                   "alt_slot": null, "literal": "365F91", "tint": null, "shade": "BF",
                   "mods": ["themeShade"], "in_slots": true, "matches": null}),
            "带 themeShade 的那一条不判，而且一律不算色"
        );
        assert_eq!(
            had["totals"]["by_holder"],
            json!({"wmlColor": {"color/rPr": 520},
                   "schemeClr": {"schemeClr/solidFill": 5, "schemeClr/gs": 10,
                                "schemeClr/lnRef": 2, "schemeClr/fillRef": 2,
                                "schemeClr/effectRef": 2, "schemeClr/fontRef": 2}}),
            "谁抱着这一指也记一本：word 那一路全在 rPr 上，DrawingML 那一路分散在填充与引用里"
        );
        assert_eq!(
            had["totals"]["slot_refs"],
            json!({"accent1": 48, "dk1": 194, "dk2": 18, "accent2": 12, "accent3": 4,
                   "accent4": 4, "accent5": 4, "accent6": 4, "lt1": 239}),
            "十二格里没人点的那四格不进这本（hlink / folHlink 全语料也没人点）"
        );
        assert_eq!(
            had["totals"]["mods_seen"],
            json!({"themeShade": 44, "themeTint": 10, "tint": 8, "satMod": 11, "shade": 6})
        );
    }

    #[test]
    fn the_sheet_family_reads_the_number_the_way_excel_writes_it() {
        let had = refs(&fixture("cell-notes-lo.xlsx"), 400);
        assert_eq!(had["total"], 7);
        assert_eq!(had["cut"], false);
        assert_eq!(
            had["refs"][0],
            json!({"part": "xl/styles.xml", "kind": "themeIndex", "at": "color",
                   "holder": "font", "name": "1", "slot": "lt1", "via": "index",
                   "alt_slot": "dk1", "literal": null, "tint": null, "shade": null,
                   "mods": [], "in_slots": false, "matches": null}),
            "同一个序号在两张表里是相反的两格，两读都交"
        );
        assert_eq!(had["totals"]["by_name"]["themeIndex"], json!({"1": 1}));
        assert_eq!(had["totals"]["skip_no_slot"], 6);
        assert_eq!(had["totals"]["skip_modified"], 0);
        // 最满的那一本：主题件里那 12 个 phClr 全在色板之外，另 4 条什么都点不到
        let big = refs(&fixture("book.xlsx"), 400);
        assert_eq!(
            agg(&big["totals"]),
            json!([17, 0, 16, 1, 10, 0, 2, 1, 0, 0, 0, 0, 17, 1, 16, 0, 0, 12, 1, 4, 0, 0, 1])
        );
        assert_eq!(big["totals"]["slot_refs"], json!({"lt1": 1}));
        assert_eq!(
            big["totals"]["mods_seen"],
            json!({"tint": 6, "satMod": 12, "shade": 7}),
            "Excel 的修饰符既写属性也写孩子元素，两样都进这一本"
        );
    }

    #[test]
    fn a_deck_resolves_tx1_from_its_own_master_and_a_many_master_deck_judges_less() {
        let had = refs(&fixture("deck-gr.pptx"), 400);
        assert_eq!(had["totals"]["clr_map_written"], 1);
        assert_eq!(had["totals"]["alias_names"], 12);
        assert_eq!(
            had["totals"]["by_via"],
            json!({"name": 7, "clrMap": 51, "(没写)": 15})
        );
        assert_eq!(
            had["totals"]["slot_refs"],
            json!({"dk1": 50, "accent1": 6, "lt1": 2})
        );
        assert_eq!(
            had["refs"][0],
            json!({"part": "ppt/presentation.xml", "kind": "schemeClr", "at": "schemeClr",
                   "holder": "solidFill", "name": "tx1", "slot": "dk1", "via": "clrMap",
                   "alt_slot": null, "literal": null, "tint": null, "shade": null,
                   "mods": [], "in_slots": false, "matches": null}),
            "DrawingML 那一族没有影子可核，`matches` 交 null 不是漏了"
        );
        // 一个母版一个主题件的那一份：十二格有十格被十二份件说成不同值，于是影子全部不判
        let many = refs(&fixture("deck-lo.pptx"), 400);
        assert_eq!(many["total"], 328);
        assert_eq!(many["totals"]["theme_parts"], 12);
        assert_eq!(many["totals"]["clr_map_written"], 12);
        assert_eq!(many["totals"]["parts_scanned"], 42);
        assert_eq!(many["totals"]["skip_no_literal"], 207);
        assert_eq!(many["totals"]["skip_multi_value"], 0);
        assert_eq!(slot_book_row(&many, "dk1")["agree"], json!(true));
        assert_eq!(
            slot_book_row(&many, "dk2"),
            json!({"slot": "dk2", "values": ["000000", "1F497D"], "parts": 12, "agree": false})
        );
    }

    #[test]
    fn odf_counts_parts_while_legacy_formats_answer_an_empty_book() {
        let odt = refs(&fixture("bkmks.odt"), 400);
        assert_eq!(odt["total"], 0);
        assert_eq!(
            odt["totals"]["parts_scanned"], 5,
            "ODF 也走部件，只是那三种点法一条都没有"
        );
        assert_eq!(odt["totals"]["refs"], 0);
        assert_eq!(
            odt["slots"][0],
            json!({"slot": "dk1", "values": [], "parts": 0, "agree": false})
        );
        assert_eq!(odt["slots"].as_array().expect("十二行").len(), 12);
        for name in ["book.xls", "toc.rtf"] {
            let none = refs(&fixture(name), 400);
            assert_eq!(none["total"], 0, "{} 不是 ZIP", name);
            assert_eq!(none["refs"], json!([]));
            assert_eq!(none["cut"], false);
            assert_eq!(
                agg(&none["totals"]),
                zeros(),
                "非 ZIP 也交二十三个键，一个都不缺"
            );
            assert_eq!(none["totals"]["parts_scanned"], 0);
            assert_eq!(none["slots"].as_array().expect("十二行").len(), 12);
        }
        assert_eq!(agg(&refs(b"not a zip at all", 8)["totals"]), zeros());
    }
}
