//! 中文排版的那几枚段开关 —— 同一件事有三个住处，而「在场」「写了值」「值是空串」是三句话。
//!
//! Word 把 `w:kinsoku`、`w:wordWrap`、`w:overflowPunct`、`w:autoSpaceDE`、`w:autoSpaceDN`、
//! `w:adjustRightInd`、`w:snapToGrid`、`w:contextualSpacing`、`w:textAlignment` 九枚写在
//! `w:pPr` 底下；同一枚也可能只写在段点的那份样式里，或写在 `docDefaults` 那一处。这一族把
//! **三处都按原样摊开**，不折成「这份文档开没开中文紧凑」，也不替文件挑一个：每枚交
//! 「在不在」与「`@w:val` 原样是什么」—— 没写值是「裸写」（规范里那算真），写了空串是
//! 「写了但没值」，两件事绝不并成一格。
//!
//! 值不换算：真件里 `w:wordWrap` 写过 `off`（不是 0 也不是 false），而 `w:textAlignment`
//! 根本不是布尔（`auto` / `baseline` / `top` / `center` / `bottom`）。字侧那枚 `w:noProof`
//! 实测**全部**住在 `w:rPr` 里而不是段上，所以另交一本（`run_no_proof`），不混进这九枚。
//!
//! ODF 那一头有四枚近亲：`style:contextual-spacing`、`style:line-break`、
//! `style:punctuation-wrap`、`style:snap-to-layout-grid`，它们**只写在段落样式上**，段自己只点
//! 样式名。跨族只核对名字、不折算语义（`kinsoku` 对 `line-break`、`overflowPunct` 对
//! `punctuation-wrap` 只是近亲）。
//!
//! 实测（`cjk-switches.docx` = python-docx 打底 + 按 ECMA 手写；`-lo.docx` = LibreOffice 重写同一份；
//! `cjk-switches.odt` = 同份转 ODF；`cjk-odf.odt` = 手写 ODF；`cjk-odf-lo.docx` = 手写 ODF 转回 docx；
//! 普查 = 本机真件聚合，仓库自产件另算一本）：
//! 1. **真件写哪几枚**（129 份真件 docx/docm）：`contextualSpacing` 108 份、正文 2868 + 样式 1473
//!    枚而 4341 枚**全是裸写**；`snapToGrid` 12 份 788 枚、值 `0` 占 597；`autoSpaceDE` 19 份
//!    （裸 229 / `0` 43 / `true` 22）；`wordWrap` 里有一枚拼成 `off`；`textAlignment` 只见 `auto`
//!    与 `baseline`；`noProof` 148 枚全在 `w:rPr`、全裸写；**docDefaults 一处都不写**这九枚；
//! 2. **ODF 那一头**（64 份真件 .odt）：`contextual-spacing` 1841 枚（false 1085 / true 756）、
//!    `line-break` 恒 `strict` 123 枚、`punctuation-wrap` `hanging` 69 / `simple` 3，而
//!    `snap-to-layout-grid` 与 auto-space、adjust-right-indent、text-align-last **一条都没有**；
//! 3. **LibreOffice 重写这一层动得很凶**：`kinsoku` / `wordWrap`（含那枚 `off`）/ `autoSpaceDE` /
//!    `autoSpaceDN` / `adjustRightInd` / 字侧 `noProof` **六处整个不再写**，docDefaults 那枚也没了；
//!    `overflowPunct` 两枚都改写成 `false`；`snapToGrid` 的 `裸 / 0 / 1` 换成 `true / false / true`；
//!    `contextualSpacing` 显式关掉的那一枚不见了；`textAlignment` 三个枚举值**一字不动**穿过；
//! 4. **反方向只搬得动四件事**：手写 ODF 那份转回 docx 只出现 `kinsoku=true`、`overflowPunct`
//!    （true 与 false 各一）、`snapToGrid=false`、`contextualSpacing` 裸写，其余五枚**全是零**；
//! 5. docx → ODF 那一转把 `wordWrap="off"` 换成 `fo:wrap-option="no-wrap"`（这一族**不读**那一格，
//!    它是排版兼容那本的话），并把 `contextual-spacing` 逐段补满（15 段里 11 段被写成 false）。
//!
//! 不做的事：**不折算跨族语义**、**不把三处折成一个数**（`own` / `style` / `defaults_written`
//! 都交，谁赢是读者的话）、**不读 ODF 的 `fo:wrap-option`**。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 段上那九枚：键就用文件写的元素名（与第二读者同一口径，不做 snake_case 换名）
const WORDS: [&str; 9] = [
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "adjustRightInd",
    "snapToGrid",
    "contextualSpacing",
    "textAlignment",
];
/// ODF 那一头的四枚近亲（属性的局部名）
const ODF_WORDS: [&str; 4] = [
    "contextual-spacing",
    "line-break",
    "punctuation-wrap",
    "snap-to-layout-grid",
];
/// 「这枚在而没写 `@w:val`」在值表里用的那把名字（与第二读者同一串字）
const BARE: &str = "<没写 val>";
/// 规范说没写值就是真，而这四种拼法都算「写着关掉」
const OFF_WORDS: [&str; 4] = ["0", "false", "off", "none"];

fn sw(present: bool, val: Option<&str>) -> Value {
    let off = val.map(|one| OFF_WORDS.contains(&one)).unwrap_or(false);
    json!({
        "present": present,
        "val": val,
        "on_written": present && !off,
        "off_written": present && off,
    })
}

fn blank_words() -> Value {
    let mut out = serde_json::Map::new();
    for one in WORDS.iter() {
        out.insert((*one).to_string(), sw(false, None));
    }
    Value::Object(out)
}

/// 一枚 `w:pPr`（段上的、样式里的、或 docDefaults 里的）这九枚各写了什么
fn attrs_of(holder: Option<&Node>) -> Value {
    let mut out = serde_json::Map::new();
    for name in WORDS.iter() {
        let found = holder.and_then(|had| had.children.iter().find(|kid| kid.local() == *name));
        let val = found.and_then(|had| had.attr_local("val"));
        out.insert((*name).to_string(), sw(found.is_some(), val));
    }
    Value::Object(out)
}

/// 这一本里「在场」的那些枚名（按 WORDS 的序，两读同一序）
fn touched_of(had: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in WORDS.iter() {
        if let Some(one) = had.get(*name) {
            if one["present"].as_bool().unwrap_or(false) {
                out.push((*name).to_string());
            }
        }
    }
    out
}

fn p_pr_of(node: &Node) -> Option<&Node> {
    node.children.iter().find(|kid| kid.local() == "pPr")
}

fn para_text(node: &Node) -> String {
    let mut out = String::new();
    for one in node.descendants("t") {
        out.push_str(&one.text());
    }
    out
}

fn bump(book: &mut BTreeMap<String, u64>, key: &str) {
    *book.entry(key.to_string()).or_insert(0) += 1;
}

fn dump_book(book: &BTreeMap<String, u64>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in book.iter() {
        out.insert(key.clone(), json!(value));
    }
    Value::Object(out)
}

/// 每枚一格：写了几次、其中裸写几次、写空串几次、值按字面分表
type Cell = (u64, u64, u64, BTreeMap<String, u64>);

fn tally_bump(table: &mut BTreeMap<String, Cell>, had: &Value) {
    for name in touched_of(had) {
        let Some(one) = had.get(&name) else {
            continue;
        };
        let slot = table
            .entry(name.clone())
            .or_insert((0, 0, 0, BTreeMap::new()));
        slot.0 += 1;
        match one["val"].as_str() {
            None => {
                slot.1 += 1;
                *slot.3.entry(BARE.to_string()).or_insert(0) += 1;
            }
            Some(text) => {
                if text.is_empty() {
                    slot.2 += 1;
                }
                *slot.3.entry(text.to_string()).or_insert(0) += 1;
            }
        }
    }
}

fn dump_tally(table: &BTreeMap<String, Cell>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, (written, bare, empty, values)) in table.iter() {
        out.insert(
            key.clone(),
            json!({
                "written": written,
                "bare": bare,
                "empty_val": empty,
                "values": dump_book(values)
            }),
        );
    }
    Value::Object(out)
}

fn member(bytes: &[u8], part: &str) -> Option<Node> {
    let one = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = one.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// docx / docm：段上、样式里、docDefaults 三处都交，值按字面；另交字侧那本 `noProof`
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let Some(root) = member(bytes, "word/document.xml") else {
        return json!({"available": false});
    };
    let mut table: BTreeMap<String, Option<Node>> = BTreeMap::new();
    let mut defaults = blank_words();
    if let Some(styles) = member(bytes, "word/styles.xml") {
        for one in styles.descendants("style") {
            let Some(sid) = one.attr_local("styleId") else {
                continue;
            };
            table.insert(sid.to_string(), p_pr_of(one).cloned());
        }
        // 第一枚 docDefaults 里的那一枚 pPr（写成链式取值，别用「循环一次就 break」）
        let first_default = styles
            .descendants("docDefaults")
            .into_iter()
            .next()
            .and_then(|block| block.descendants("pPr").into_iter().next());
        if let Some(holder) = first_default {
            defaults = attrs_of(Some(holder));
        }
    }
    let mut defaults_written: BTreeMap<String, u64> = BTreeMap::new();
    for one in WORDS.iter() {
        let hit = defaults
            .get(*one)
            .and_then(|had| had["present"].as_bool())
            .unwrap_or(false);
        defaults_written.insert((*one).to_string(), if hit { 1 } else { 0 });
    }
    let defaults_touched = touched_of(&defaults);

    // 九枚全在场：语料级那本按枚名逐格取，缺席的一枚交 0 而不是没有这一格
    let mut words: BTreeMap<String, Cell> = BTreeMap::new();
    let mut style_words: BTreeMap<String, u64> = BTreeMap::new();
    for one in WORDS.iter() {
        words.insert((*one).to_string(), (0, 0, 0, BTreeMap::new()));
        style_words.insert((*one).to_string(), 0);
    }
    let mut proof_runs = 0u64;
    let mut proof_paragraphs = 0u64;
    let mut proof_values: BTreeMap<String, u64> = BTreeMap::new();
    let mut listed_rows: Vec<Value> = Vec::new();
    let mut indexed: Vec<u64> = Vec::new();
    let mut total = 0usize;
    let mut p_pr = 0usize;
    let mut conflicts = 0usize;
    let mut see_defaults = 0usize;

    for para in root.descendants("p") {
        let holder = p_pr_of(para);
        if holder.is_some() {
            p_pr += 1;
        }
        let own = attrs_of(holder);
        let touched = touched_of(&own);
        let style_id = holder.and_then(|had| {
            had.children
                .iter()
                .find(|kid| kid.local() == "pStyle")
                .and_then(|kid| kid.attr_local("val"))
                .map(String::from)
        });
        let slot = style_id.as_ref().and_then(|key| table.get(key));
        let style_holder = match slot {
            Some(had) => had.as_ref(),
            None => None,
        };
        let style = attrs_of(style_holder);
        if slot.is_some() {
            for name in touched_of(&style) {
                bump(&mut style_words, &name);
            }
        }
        // 段上与样式里都写了同一枚而值不一样：只把「哪几枚打架」交出去，不替文件挑一个
        let clash: Vec<String> = touched
            .iter()
            .filter(|name| {
                let mine = own.get(name.as_str());
                let theirs = style.get(name.as_str());
                match (mine, theirs) {
                    (Some(one), Some(other)) => {
                        other["present"].as_bool().unwrap_or(false) && other["val"] != one["val"]
                    }
                    _ => false,
                }
            })
            .cloned()
            .collect();
        if !clash.is_empty() {
            conflicts += 1;
        }
        if !defaults_touched.is_empty() {
            see_defaults += 1;
        }
        tally_bump(&mut words, &own);

        let mut proof = 0usize;
        for kid in para.descendants("rPr") {
            for sub in kid.children.iter().filter(|had| had.local() == "noProof") {
                proof += 1;
                proof_runs += 1;
                match sub.attr_local("val") {
                    None => bump(&mut proof_values, BARE),
                    Some(text) => bump(&mut proof_values, text),
                }
            }
        }
        if proof > 0 {
            proof_paragraphs += 1;
        }
        if !touched.is_empty() {
            indexed.push(total as u64);
        }
        if total < limit {
            listed_rows.push(json!({
                "index": total,
                "has_pPr": holder.is_some(),
                "style_id": style_id,
                "style_found": slot.is_some(),
                "own": own,
                "style": style,
                "written": touched,
                "conflict_with_style": clash,
                "defaults_present": defaults_touched,
                "text": para_text(para),
            }));
        }
        total += 1;
    }
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs_total": total,
        "p_pr_elements": p_pr,
        "paragraphs_with_any": indexed.len(),
        "paragraphs_indexed": indexed,
        "words_written": dump_tally(&words),
        "style_written": dump_book(&style_words),
        "defaults_written": dump_book(&defaults_written),
        "paragraphs_see_defaults": see_defaults,
        "run_no_proof": json!({
            "runs": proof_runs,
            "paragraphs": proof_paragraphs,
            "values": dump_book(&proof_values),
        }),
        "conflicts": conflicts,
        "styles_seen": table.len(),
        "paragraphs": listed_rows,
        "listed": total.min(limit),
        "cut": total > limit,
    })
}

/// ODF：段只点样式名，四枚近亲在跳到的那份段落属性上（按字面交，不与 OOXML 折算）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    // 与 keep_switches 同一写法：段只看 content.xml，样式表两处都建
    let mut seen: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        seen.push(("styles.xml", extra));
    }
    let mut table: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for (part, node) in seen.iter() {
        for one in node.descendants("style") {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            let holder = one
                .children
                .iter()
                .find(|kid| kid.local() == "paragraph-properties");
            let mut had = serde_json::Map::new();
            for key in ODF_WORDS.iter() {
                let val = holder.and_then(|slot| slot.attr_local(key));
                had.insert(
                    (*key).to_string(),
                    json!({"present": val.is_some(), "val": val}),
                );
            }
            table.insert(name.to_string(), ((*part).to_string(), Value::Object(had)));
        }
    }
    // 同 docx 那一头：四枚近亲全在场，缺席交 0
    let mut words: BTreeMap<String, (u64, BTreeMap<String, u64>)> = BTreeMap::new();
    for key in ODF_WORDS.iter() {
        words.insert((*key).to_string(), (0, BTreeMap::new()));
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut indexed: Vec<u64> = Vec::new();
    let part = "content.xml";
    for para in content.descendants("p") {
        let sid = para.attr_local("style-name").map(String::from);
        let found = sid.as_ref().and_then(|key| table.get(key));
        let mut had = serde_json::Map::new();
        for key in ODF_WORDS.iter() {
            let one = match found {
                Some(slot) => slot.1.get(*key).cloned(),
                None => None,
            };
            had.insert(
                (*key).to_string(),
                one.unwrap_or_else(|| json!({"present": false, "val": Value::Null})),
            );
        }
        let had = Value::Object(had);
        let touched: Vec<String> = ODF_WORDS
            .iter()
            .filter(|key| had[*key]["present"].as_bool().unwrap_or(false))
            .map(|key| (*key).to_string())
            .collect();
        for name in touched.iter() {
            let slot = words.entry(name.clone()).or_insert((0, BTreeMap::new()));
            slot.0 += 1;
            match had.get(name.as_str()).and_then(|one| one["val"].as_str()) {
                Some(text) => *slot.1.entry(text.to_string()).or_insert(0) += 1,
                None => *slot.1.entry(BARE.to_string()).or_insert(0) += 1,
            }
        }
        if !touched.is_empty() {
            indexed.push(rows.len() as u64);
        }
        rows.push(json!({
            "part": part,
            "index": rows.len(),
            "style_id": sid,
            "style_found": found.is_some(),
            "written": touched,
            "attrs": had,
            "text": para_text(para),
        }));
    }
    let total = rows.len();
    let mut book = serde_json::Map::new();
    for (key, (written, values)) in words.iter() {
        book.insert(
            key.clone(),
            json!({"written": written, "values": dump_book(values)}),
        );
    }
    json!({
        "family": "odf",
        "available": true,
        "paragraphs_total": total,
        "paragraphs_with_any": indexed.len(),
        "paragraphs_indexed": indexed,
        "words_written": Value::Object(book),
        "styles_seen": table.len(),
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "listed": total.min(limit),
        "cut": total > limit,
    })
}
