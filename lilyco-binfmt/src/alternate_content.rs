//! `mc:AlternateContent` —— 同一件事写两遍：一枚 `mc:Choice`（它点名要哪个扩展命名空间才
//! 看得懂）加一枚可选的 `mc:Fallback`（看不懂的那位走这条路）。这一族的存在意义就是
//! 「同一个文件对不同读者说不同的话」，所以它必须被数出来：只数正文里的字，会看不见
//! 那些**只写在某一条分支里**的话。
//!
//! 一块一本账，交的是「几块、两遍各写了吗、Choice 点的是哪几个前缀、两条分支各写了
//! 什么元素」，三处族（docx / xlsx / pptx）走同一个读者：
//!
//! * `blocks` 数的是 `mc:AlternateContent` 那枚元素，`choices` / `fallbacks` 数的是它们
//!   肚子里的分支 —— 三个数互不相减：**「写了两遍」不是这条规矩的常态**（本机真件 3 块里
//!   只有 1 块配了 Fallback，另两块只有 Choice）；
//! * 一块里可以有几条 Choice（ECMA 允许按 `Requires` 挑第一条能认的），所以 `choices`
//!   比 `blocks` 大不是矛盾，而 `orphans`（这一块里 Choice 有、Fallback 没有）单独数；
//! * `requires` 原样交文件写的前缀名（不解命名空间 URI —— 前缀是在那枚根元素的
//!   `xmlns:` 上定义的，本仓不在这里再走一跳），`requires_prefixes` 按「出现次数从多到少、
//!   同数按名字」排 —— 两家生产者的词汇几乎不重叠：docx 画布那一条点 `wps`，
//!   pptx 整册每页都点 `p14`，公式那一族同时点 `a14` 与 `p14`，xlsx 点 `v2`。
//!
//! 实测（`alternate.docx` 由 python-docx 打底 + 按 ECMA 的写法插三块，`alternate-lo.docx`
//! 是 LibreOffice 重写同一份）：
//! 1. LibreOffice 重写时把三块 **连字一起丢掉**（`blocks` 3 → 0，五句只写在分支里的字
//!    一句都不剩，只剩打底那一段），所以这一族不是「换种写法」而是「换个读者就读不到」——
//!    这也是本仓要把它数出来的理由；
//! 2. 真件那一份分布：13 份自产 fixture 里 31 块、31 枚 Choice、31 枚 Fallback、
//!    `orphans` 0（两个生产者都写全两遍），而本机真件 3 块 3 枚 Choice 只有 1 枚 Fallback；
//! 3. 「件名之外」的那一格也要交：`parts_scanned` 是**有块的件数**（不是整包件数），
//!    它与 `entries` 的行数一起说清「哪几件里躺着这些分支」。
//!
//! 不做的事：**不判哪条分支会被用**（那是读者自己的 `Requires` 解析结果，不是文件里
//! 写着的事实）、**不比较两条分支的字**（两遍各写各的，比出个「差几个字」没有意义）。
//! ODF 那一族不交这个键：`mc:AlternateContent` 是 OPC 的东西，ODF 没有这一层。

use crate::xmlscan;
use crate::zipread;
use serde_json::{json, Value};

/// 一块里两条分支的局部名（`mc:` 前缀各家长短不一，只比局部名）
const CHOICE: &str = "Choice";
const FALLBACK: &str = "Fallback";

fn children_named<'a>(node: &'a xmlscan::Node, want: &str) -> Vec<&'a xmlscan::Node> {
    node.children
        .iter()
        .filter(|one| one.local() == want)
        .collect()
}

fn scan_part(root: &xmlscan::Node, prefixes: &mut Vec<(String, usize)>) -> Value {
    let mut blocks = 0usize;
    let mut choices = 0usize;
    let mut fallbacks = 0usize;
    let mut orphans = 0usize;
    let mut requires: Vec<Value> = Vec::new();
    let mut choice_elems: Vec<Value> = Vec::new();
    let mut fallback_elems: Vec<Value> = Vec::new();
    for block in root.descendants("AlternateContent") {
        blocks += 1;
        let mine_choice = children_named(block, CHOICE);
        let mine_fallback = children_named(block, FALLBACK);
        for one in mine_choice.iter() {
            choices += 1;
            for kid in one.children.iter() {
                choice_elems.push(json!(kid.local()));
            }
            let written = one.attr_local("Requires").unwrap_or_default();
            for hit in written.split_whitespace() {
                requires.push(json!(hit));
                match prefixes.iter_mut().find(|had| had.0 == hit) {
                    Some(had) => had.1 += 1,
                    None => prefixes.push((hit.to_string(), 1)),
                }
            }
        }
        for one in mine_fallback.iter() {
            fallbacks += 1;
            for kid in one.children.iter() {
                fallback_elems.push(json!(kid.local()));
            }
        }
        if !mine_choice.is_empty() && mine_fallback.is_empty() {
            orphans += 1;
        }
    }
    json!({
        "blocks": blocks,
        "choices": choices,
        "fallbacks": fallbacks,
        "orphans": orphans,
        "requires": requires,
        "choice_elements": choice_elems,
        "fallback_elements": fallback_elems,
    })
}

/// 一份包一本账：哪几件里躺着这些分支、各写了几个数
pub(crate) fn ledger(bytes: &[u8], limit: usize) -> Value {
    let names = zipread::member_names(bytes);
    let mut prefixes: Vec<(String, usize)> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    let mut scanned = 0usize;
    let (mut blocks, mut choices, mut fallbacks, mut orphans) = (0usize, 0usize, 0usize, 0usize);
    let mut ordered: Vec<&String> = names.iter().collect();
    ordered.sort_unstable();
    for name in ordered {
        if !(name.ends_with(".xml") && !name.ends_with(".rels")) {
            continue;
        }
        let text = match zipread::member(bytes, name.as_str(), zipread::DEFAULT_MEMBER_CAP) {
            Ok(one) => one.as_text(),
            Err(_) => continue,
        };
        if !text.contains("AlternateContent") {
            continue;
        }
        scanned += 1;
        let root = xmlscan::parse_str(&text);
        let mut mine = match scan_part(&root, &mut prefixes) {
            Value::Object(had) => had,
            _ => serde_json::Map::new(),
        };
        mine.insert("part".to_string(), json!(name));
        for (key, hit) in [
            ("blocks", &mut blocks),
            ("choices", &mut choices),
            ("fallbacks", &mut fallbacks),
            ("orphans", &mut orphans),
        ] {
            if let Some(raw) = mine.get(key).and_then(Value::as_u64) {
                *hit += raw as usize;
            }
        }
        if entries.len() < limit {
            entries.push(Value::Object(mine));
        }
    }
    // 「出现次数从多到少、同数按名字」—— 两份读者要给出同一个序，所以两边都自己排
    prefixes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut table = serde_json::Map::new();
    for (name, count) in prefixes.iter() {
        table.insert(name.clone(), json!(count));
    }
    let order: Vec<Value> = prefixes.iter().map(|(name, _)| json!(name)).collect();
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": scanned,
        "blocks": blocks,
        "choices": choices,
        "fallbacks": fallbacks,
        "orphans": orphans,
        "requires_prefixes": order,
        "requires_counts": Value::Object(table),
        "entries": entries,
        "cut": scanned > entries.len(),
    })
}
