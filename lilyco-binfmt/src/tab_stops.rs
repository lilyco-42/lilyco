//! 「这一段上有哪几个制表位」——同一问在两家写在两个不同的地方
//!
//! OOXML 把它放在**段上**：`w:pPr/w:tabs/w:tab`，三个属性 `w:pos`（twip）、`w:val`（对齐）、
//! `w:leader`（引导符），按文件写的原样交 —— `w:val` 没写不等于「左对齐」，那是规范的默认值，
//! 不是这份文件说的话。
//!
//! ODF 把它放在**一跳之外**：段只点一个样式名（`text:style-name`），制表位坐在那份样式的
//! `style:paragraph-properties/style:tab-stops/style:tab-stop` 上，属性是 `style:position`
//! （带单位的串）、`style:type`（没写=左）、`style:leader-style` 配 `style:leader-text`
//! （两个属性合起来才是「引导符」），小数点那一样是 `type="char"` 配 `style:char`。
//!
//! 实测三条（`tabs.docx` 与它经 LibreOffice 转出的 `.odt` / `.rtf`，一次只改一个变量）：
//! 1. 同一条 9cm 在三家手里是三个串：docx 写 `5102`（twip），ODF 写 **`8.999cm`**
//!    （转一趟少 0.001cm —— 换算的账不归读者平），RTF 又回到 `5102`。
//! 2. 「按了几下制表键」与「定义了哪几个位置」是两本账：段里的 `w:tab` / `text:tab` 是**字符**，
//!    而 `w:tabs/w:tab` 是**定义** —— 两个同名，全局数一遍就会把定义当成字符。
//! 3. RTF 那一族把这件事写成一条扁平流：`\tx<twips>` 是一个位置，而对齐与引导符是**只管紧跟
//!    的那一个** `\tx` 的前缀（`\tldot\tqr\tx1701\tlul\tx5102` = 第一条点引导右对齐、第二条
//!    只带下划线引导）。同一条稿子在手里三家位置数与字符数都一样（8 / 8），单位则与 OOXML
//!    同为 twip。这一族的账不在本模块 —— 它由 `rtf.rs` 数流、`office-doc` 的 RTF 分支交，
//!    形状是「流上的数与原样」：样式表那一群里还另有四条位置（`\tqc\tx4680` 与 `\tqr\tx9360`
//!    各两条，住在 `header` / `footer` 那两份样式里），而这一族的正文walk 照旧跳过样式表，
//!    所以那四条不进账本；一份段点了哪份样式、样式里又有位置，这一族没有段边界可认，
//!    于是**不冒充归属**，只交流上数得清的。

use crate::xmlscan::Node;
use serde_json::{json, Value};

fn bump(map: &mut serde_json::Map<String, Value>, key: &str) {
    let next = map.get(key).and_then(Value::as_u64).unwrap_or(0) + 1;
    map.insert(key.to_string(), json!(next));
}

/// 一条 `style:tab-stop` 写着的五个属性（两家合起来才是「引导符」，所以两个都交）
fn odf_stop(tab: &Node) -> Value {
    json!({
        "position": tab.attr_local("position"),
        "type": tab.attr_local("type"),
        "char": tab.attr_local("char"),
        "leader_style": tab.attr_local("leader-style"),
        "leader_text": tab.attr_local("leader-text"),
    })
}

/// 一份 `style:paragraph-properties` 里的制表位定义
fn stops_under(holder: &Node) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for group in holder.all("tab-stops") {
        for tab in group.all("tab-stop") {
            out.push(odf_stop(tab));
        }
    }
    out
}

/// `style:default-style`（family=paragraph）没有名字可点，而它写的制表位照样落到每一段上 ——
/// 单独交一份，免得「段点的那份样式里没有」被读成「这段没有制表位」
fn default_style_stops(roots: &[&Node]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for root in roots {
        for one in root.descendants("default-style") {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            if let Some(holder) = one.child("paragraph-properties") {
                out.extend(stops_under(holder));
            }
        }
    }
    out
}

/// 一条 `w:tabs` 里的定义：三个属性都按写的交，没写是 null
fn ooxml_stops(holder: &Node) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for tabs in holder.all("tabs") {
        for tab in tabs.all("tab") {
            out.push(json!({
                "pos_written": tab.attr_local("pos"),
                "val": tab.attr_local("val"),
                "leader": tab.attr_local("leader"),
            }));
        }
    }
    out
}

/// OOXML 那一份：逐段交「段上定义了哪几个位置」与「这一段里有几个制表符」
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut vals = serde_json::Map::new();
    let mut leaders = serde_json::Map::new();
    let mut positions: Vec<String> = Vec::new();
    let mut stops_total = 0usize;
    let mut without_val = 0usize;
    let mut without_leader = 0usize;
    let mut chars_total = 0usize;
    for (index, para) in body.descendants("p").iter().enumerate() {
        let mut stops: Vec<Value> = Vec::new();
        if let Some(holder) = para.child("pPr") {
            stops = ooxml_stops(holder);
        }
        // 制表**字符**只认 run 的孩子：`w:tabs/w:tab` 与它同名，全局数会把定义当成字符
        let mut chars = 0usize;
        for run in para.descendants("r") {
            chars += run.all("tab").len();
        }
        chars_total += chars;
        stops_total += stops.len();
        for one in &stops {
            match one["val"].as_str() {
                Some(raw) => bump(&mut vals, raw),
                None => without_val += 1,
            }
            match one["leader"].as_str() {
                Some(raw) => bump(&mut leaders, raw),
                None => without_leader += 1,
            }
            if let Some(raw) = one["pos_written"].as_str() {
                if !positions.contains(&raw.to_string()) {
                    positions.push(raw.to_string());
                }
            }
        }
        rows.push(json!({
            "index": index,
            "stops": stops,
            "tab_chars": chars,
        }));
    }
    let with_stops = rows
        .iter()
        .filter(|one| one["stops"].as_array().map_or(false, |had| !had.is_empty()))
        .count();
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs_total": rows.len(),
        "with_stops": with_stops,
        "stops_total": stops_total,
        "tab_chars_total": chars_total,
        "stops_without_val": without_val,
        "stops_without_leader": without_leader,
        "vals_written": Value::Object(vals),
        "leaders_written": Value::Object(leaders),
        "distinct_positions": positions,
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份的样式表：两份件都扫，段点的名字先在 content 里找、再在 styles 里找
fn paragraph_styles(roots: &[&Node]) -> Vec<(String, Vec<Value>, Option<String>)> {
    let mut out: Vec<(String, Vec<Value>, Option<String>)> = Vec::new();
    for root in roots {
        for one in root.descendants("style").into_iter() {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            let mut stops: Vec<Value> = Vec::new();
            if let Some(holder) = one.child("paragraph-properties") {
                stops = stops_under(holder);
            }
            out.push((
                name.to_string(),
                stops,
                one.attr_local("parent-style-name").map(String::from),
            ));
        }
    }
    out
}

/// ODF 那一份：段只点一个名，制表位在跳到的那份样式上（没跳到就交「这份件里没有」）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<&Node> = vec![content];
    if let Some(extra) = styles {
        roots.push(extra);
    }
    let table = paragraph_styles(&roots);
    let mut rows: Vec<Value> = Vec::new();
    let mut types = serde_json::Map::new();
    let mut leader_styles = serde_json::Map::new();
    let mut positions: Vec<String> = Vec::new();
    let mut stops_total = 0usize;
    let mut without_type = 0usize;
    let mut chars_total = 0usize;
    let mut pointed: Vec<String> = Vec::new();
    for (index, para) in content.descendants("p").iter().enumerate() {
        let name = para.attr_local("style-name");
        let found = name.and_then(|want| table.iter().find(|had| had.0 == want));
        let stops = found.map(|had| had.1.clone()).unwrap_or_default();
        if let Some(want) = name {
            if !stops.is_empty() && !pointed.iter().any(|had| had == want) {
                pointed.push(want.to_string());
            }
        }
        let chars = para.descendants("tab").len();
        chars_total += chars;
        stops_total += stops.len();
        for one in &stops {
            match one["type"].as_str() {
                Some(raw) => bump(&mut types, raw),
                None => without_type += 1,
            }
            if let Some(raw) = one["leader_style"].as_str() {
                bump(&mut leader_styles, raw);
            }
            if let Some(raw) = one["position"].as_str() {
                if !positions.contains(&raw.to_string()) {
                    positions.push(raw.to_string());
                }
            }
        }
        rows.push(json!({
            "index": index,
            "style_written": name.map(String::from),
            "style_found": found.is_some(),
            "parent_style_written": found.and_then(|had| had.2.clone()),
            "stops": stops,
            "tab_chars": chars,
        }));
    }
    let with_stops = rows
        .iter()
        .filter(|one| !one["stops"].as_array().map_or(true, |had| had.is_empty()))
        .count();
    // 有制表位而**没有任何段点它**的那些样式：与母版页那一条同一个问（写了没人点 ≠ 没有）
    let unpointed: Vec<String> = table
        .iter()
        .filter(|had| !had.1.is_empty() && !pointed.iter().any(|one| *one == had.0))
        .map(|had| had.0.clone())
        .collect();
    json!({
        "family": "odf",
        "available": true,
        "paragraphs_total": rows.len(),
        "with_stops": with_stops,
        "stops_total": stops_total,
        "tab_chars_total": chars_total,
        "styles_total": table.len(),
        "styles_with_stops": table.iter().filter(|had| !had.1.is_empty()).count(),
        "unpointed_styles": unpointed,
        "default_style_stops": default_style_stops(&roots),
        "stops_without_type": without_type,
        "types_written": Value::Object(types),
        "leader_styles_written": Value::Object(leader_styles),
        "distinct_positions": positions,
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
