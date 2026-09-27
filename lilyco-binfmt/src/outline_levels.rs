//! 这一段是第几级 —— 级别可能写在三处，而三处可以互相不一致。
//!
//! Word 不说「这是标题」。它给这一段点一个样式号（`w:pStyle/@w:val`），而「第几级」这件事
//! 有三个可能的住处：段自己的 `w:pPr/w:outlineLvl`、段点名的那份样式的名字（`heading 3`、
//! `标题 #1`）、以及那份样式自己的 `w:pPr/w:outlineLvl`。这一族把三处都摊开，只在算 `level`
//! 那一步按一条写死的优先序取一个（段 > 样式名 > 样式自己的那个数），不一致的那些段由
//! `conflict` 说，而不是替文件挑一个就把另两处藏起来。
//!
//! 只住 OOXML 的文字那一族：`office-doc` 的 docx 支交 `structure.outline_levels`，
//! odt / rtf / 遗留 .doc 不交这一格（那三家各有自己的写法，本仓已有别的账在答）。
//!
//! 实测（`levels.docx` = python-docx 打底 + 按 ECMA 手写十一种形状；`levels-lo.docx` =
//! LibreOffice 重写同一份；普查 = 本机 33 份真件 docx/docm，仓库自产件另算一本）：
//! 1. **真件把号写成数字**：Word 自己导出的是 `<w:pStyle w:val="3"/>`，而 "heading 3" 在
//!    `word/styles.xml` 的 `w:name` 上 —— 现成的 `headings` 那一本只拿 id 比 `heading`/`标题`
//!    前缀，所以 33 份真件的 12525 段里 701 段有级，它只认到 290，**漏 411 段（59%）**；
//!    这一本先查样式表那一跳，把那 411 段的级从名字里取回来（真件的级**全部**来自样式名：701 段
//!    里 701 段是样式名给的，段自己写 `outlineLvl` 的在真件里一段都没有）；
//! 2. **`w:outlineLvl w:val="9"` 不是第 10 级**：ECMA 那一格 0..8 是正文的九级大纲，9 是「正文本身」，
//!    所以那一行交 `level: null` 而 `level_from` 写着「那是正文」—— 本仓唯一一处**不照着数加一**的地方。
//!    自产件两份各测一枚（段上一枚、样式上一枚），LibreOffice 重写时把那两枚**整个丢掉**；
//! 3. **断链是真件给的**：真件里有段点着样式表里根本没有的号（自产的 `notes.docx` 也有一份：
//!    3 段里 1 段如此）—— 所以 `style_found` 与 `style_missing` 各交一笔，而不是默认为「普通段」；
//! 4. LibreOffice 重写同一份动了六处：把号 `1`/`21`/`7`/`31` **换成可读的 id**（`Heading1`、
//!    `CustomText`、`emptyoutline`、`TOCHeading`）、给每一段都点上样式（没点的三段被归到 `Normal`，
//!    于是 `with_style` 9 → 12）、把断链 `999` **修成 `Normal`**（`style_missing` 1 → 0）、
//!    把写着 9 的那两枚 `outlineLvl` **整个丢掉**（`body_written` 2 → 0 —— 它认为正文就是不写）、
//!    把样式里 `@w:val=""` 那枚**替文件补成 `0`**（于是那一段从「写了但没值」变成第 1 级）、
//!    而 `TOC Heading` 那份样式的 `outlineLvl` 也没了（那一段两处都不说）；
//! 5. 「名字像不像标题」只认两种写法：小写后以 `heading` 开头，或以 `标题` 开头，级就是名字里那些
//!    **ASCII 数字**（`heading 3` → 3、`标题 #1` → 1、一个数字都没有 → 1）；空串、混着别的字符
//!    （含全角数字）都不算数，两份读者同一口径。
//!
//! 不做的事：**不跟样式继承**（`w:basedOn` 链上的级不追，与样式那一本同一待遇）、
//! **不把三处折成一个数**（三处都按原样交串，`level` 才是按规则算的那一个）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 「整串都是 ASCII 数字」才算一个数：空串、混着别的字符（含全角数字）都算「写了但没说数」
fn ascii(text: Option<&str>) -> Option<usize> {
    let raw = text?;
    if raw.is_empty() || !raw.bytes().all(|one| one.is_ascii_digit()) {
        return None;
    }
    raw.parse::<usize>().ok()
}

/// 名字里那串 ASCII 数字（一个都没有算 1）；名字不像标题就交 None
fn name_level(name: Option<&str>) -> Option<usize> {
    let had = name?;
    let lower = had.to_lowercase();
    if !(lower.starts_with("heading") || had.starts_with("标题")) {
        return None;
    }
    let digits: String = had.chars().filter(|one| one.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Some(1);
    }
    Some(digits.parse::<usize>().unwrap_or(1).max(1))
}

fn kids(node: Option<&Node>) -> Vec<&Node> {
    match node {
        Some(one) => one
            .children
            .iter()
            .filter(|kid| kid.name != "#text")
            .collect(),
        None => Vec::new(),
    }
}

fn kid<'a>(node: Option<&'a Node>, want: &str) -> Option<&'a Node> {
    kids(node).into_iter().find(|one| one.local() == want)
}

/// 一枚样式的三样：名字、它自己写的 `outlineLvl`（串），以及那枚元素在不在场
type Entry = (Option<String>, Option<String>, bool);

fn style_table(root: &Node) -> BTreeMap<String, Entry> {
    let mut out: BTreeMap<String, Entry> = BTreeMap::new();
    // `parse_str` 交的是伪根 `#doc`，真正的 `<w:styles>` 是它的孩子 —— 只看一层孩子会一枚样式都收不到
    for st in root.descendants("style") {
        let id = match st.attr_local("styleId") {
            Some(one) => (*one).to_string(),
            None => continue,
        };
        let name = kid(Some(st), "name")
            .and_then(|one| one.attr_local("val"))
            .map(String::from);
        let mut written: Option<String> = None;
        let mut present = false;
        if let Some(props) = kid(Some(st), "pPr") {
            if let Some(hit) = kid(Some(props), "outlineLvl") {
                present = true;
                written = hit.attr_local("val").map(String::from);
            }
        }
        out.insert(id, (name, written, present));
    }
    out
}

/// 段里那几串字（与第二读者同一口径：按文档顺序拼，不 trim）
fn para_text(node: &Node) -> String {
    let mut out = String::new();
    for one in node.descendants("t") {
        out.push_str(&one.text());
    }
    out
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_u64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

/// 一段一行：三处都按原样交，`level` 按「段 > 样式名 > 样式自己的数」算，9 那一格不加一
fn level_row(part: &str, index: usize, para: &Node, table: &BTreeMap<String, Entry>) -> Value {
    let props = kid(Some(para), "pPr");
    let style_id = kid(props, "pStyle")
        .and_then(|hit| hit.attr_local("val"))
        .map(String::from);
    let own_node = kid(props, "outlineLvl");
    let own_present = own_node.is_some();
    let own = own_node
        .and_then(|hit| hit.attr_local("val"))
        .map(String::from);
    let entry = match style_id.as_ref() {
        Some(key) => table.get(key),
        None => None,
    };
    let (style_name, style_written, style_present) = match entry {
        Some(one) => (one.0.clone(), one.1.clone(), one.2),
        None => (None, None, false),
    };
    let named = name_level(style_name.as_deref());
    let own_num = ascii(own.as_deref());
    let style_num = ascii(style_written.as_deref());
    let mut level: Option<usize> = None;
    let mut from = String::from("(没说)");
    if let Some(raw) = own_num {
        if raw < 9 {
            level = Some(raw + 1);
            from = String::from("段上");
        } else {
            from = String::from("段上写 9（那是正文）");
        }
    } else if let Some(raw) = named {
        level = Some(raw);
        from = String::from("样式名");
    } else if let Some(raw) = style_num {
        if raw < 9 {
            level = Some(raw + 1);
            from = String::from("样式自己的 outlineLvl");
        } else {
            from = String::from("样式写 9（那是正文）");
        }
    } else if own_present {
        // 写了这枚元素却没写出一个数：它不给级，但也不是「没写」，所以留一句
        from = String::from("段上写了但没值");
    } else if style_present {
        from = String::from("样式写了但没值");
    }
    let mut conflict = false;
    if let Some(named) = named {
        if let Some(raw) = own_num {
            if raw < 9 {
                conflict = raw + 1 != named;
            }
        }
        if let Some(raw) = style_num {
            if raw < 9 {
                conflict = conflict || raw + 1 != named;
            }
        }
    }
    json!({
        "part": part,
        "index": index,
        "style_id": style_id,
        "style_found": entry.is_some(),
        "style_name": style_name,
        "name_level": named,
        "own_written": own,
        "own_present": own_present,
        "style_written": style_written,
        "style_present": style_present,
        "level": level,
        "level_from": from,
        "conflict": conflict,
        "text": para_text(para),
    })
}

fn parse(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// docx / docm：那一段是第几级，三处都交，`level` 按一条写死的优先序算
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/") && one.ends_with(".xml") && !one.ends_with(".rels"))
        .collect();
    names.sort();
    let mut table: BTreeMap<String, Entry> = BTreeMap::new();
    let mut roots: Vec<(String, Node)> = Vec::new();
    for name in names.iter() {
        let root = match parse(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        if name.as_str() == "word/styles.xml" {
            table = style_table(&root);
        }
        if !root.descendants("p").is_empty() {
            roots.push((name.clone(), root));
        }
    }
    let styles_seen = table.len();
    let mut rows: Vec<Value> = Vec::new();
    let mut paragraphs = 0usize;
    let mut with_style = 0usize;
    let mut style_missing = 0usize;
    let mut name_matched = 0usize;
    let mut own_written = 0usize;
    let mut own_no_val = 0usize;
    let mut style_written = 0usize;
    let mut style_no_val = 0usize;
    let mut conflicts = 0usize;
    let mut body_written = 0usize;
    let mut resolved = 0usize;
    let mut levels = serde_json::Map::new();
    let mut froms = serde_json::Map::new();
    for (part, root) in roots.iter() {
        for one in root.descendants("p") {
            let props = kid(Some(one), "pPr");
            let has_style = kid(props, "pStyle").is_some();
            let has_own = kid(props, "outlineLvl").is_some();
            if !has_style && !has_own {
                continue;
            }
            let row = level_row(part.as_str(), paragraphs, one, &table);
            paragraphs += 1;
            with_style += usize::from(!row["style_id"].is_null());
            style_missing += usize::from(
                !row["style_id"].is_null() && !row["style_found"].as_bool().unwrap_or(false),
            );
            name_matched += usize::from(!row["name_level"].is_null());
            own_written += usize::from(row["own_present"].as_bool().unwrap_or(false));
            own_no_val += usize::from(
                row["own_present"].as_bool().unwrap_or(false) && row["own_written"].is_null(),
            );
            let style_present = row["style_present"].as_bool().unwrap_or(false);
            style_written += usize::from(style_present);
            style_no_val += usize::from(style_present && row["style_written"].is_null());
            body_written += usize::from(
                row["own_written"].as_str() == Some("9")
                    || row["style_written"].as_str() == Some("9"),
            );
            conflicts += usize::from(row["conflict"].as_bool().unwrap_or(false));
            resolved += usize::from(!row["level"].is_null());
            bump(&mut froms, row["level_from"].as_str().unwrap_or(""));
            if let Some(raw) = row["level"].as_u64() {
                bump(&mut levels, raw.to_string().as_str());
            }
            if rows.len() < limit {
                rows.push(row);
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs": paragraphs,
        "with_style": with_style,
        "style_missing": style_missing,
        "name_matched": name_matched,
        "own_written": own_written,
        "own_no_val": own_no_val,
        "style_written": style_written,
        "style_no_val": style_no_val,
        "conflicts": conflicts,
        "body_written": body_written,
        "resolved": resolved,
        "levels": Value::Object(levels),
        "froms": Value::Object(froms),
        "styles_seen": styles_seen,
        "rows": rows,
        "listed": paragraphs.min(limit),
        "cut": paragraphs > limit,
    })
}
