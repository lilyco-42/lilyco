//! 「这一段的行距是多少」——一家把倍数与固定值混在同一个数里（靠另一枚属性区分），一家换成两种单位
//!
//! OOXML 用一枚 `w:pPr/w:spacing`：`w:line` 是那个数，而它的**单位取决于** `w:lineRule` ——
//! `auto` 时是 1/240 倍（`360` 就是 1.5 倍），`exact` / `atLeast` 时是 twip（22 磅 = `440`）。
//! 所以这两枚必须一起交：只交 `w:line` 会把 1.5 倍与 22 磅读成同类的数，而单位是文件自己选的。
//!
//! ODF 一跳在段点的那份样式上（`style:paragraph-properties`）：倍数换成百分数（`150%`）、
//! 固定值换成长度串（`0.776cm`），单位写在串里。读者不换算、不约分、不替它统一。
//!
//! 实测四条（`line.docx` 由 python-docx 的行距 API 写五段，另两份是 LibreOffice 转的）：
//! 1. 同一份稿子里「1.5 倍」与「18 磅（至少）」在 docx 都是 `w:line="360"` —— 分别只有
//!    `lineRule` 的 `auto` 与 `atLeast` 说得清，两个值一模一样（`480` = 2 倍、`440` = 22 磅）；
//! 2. 转成 ODF 之后 `atLeast` 那一段**整个没有行高这一格**了（那一份样式里四个相关属性一个都没写）
//!    —— 固定值那一段则活下来，只是换了单位（22 磅 → `0.776cm`）。反过来，docx 里什么都不写的
//!    段零在 ODF 点的 `Standard` 样式里**写着** `115%` —— 同一份稿子两家两个答案，各交各的；
//! 3. LibreOffice 重写自己那份 docx：四个数连单位一个没改口（`atLeast` 也没丢），但给段零补了一份
//!    **没有 `w:spacing` 的** `w:pPr` —— 「有段属性」与「写了行距」因此是两本账；
//! 4. RTF 那一族的 `\sl` 与 `\slmult` 在样式表里就成批出现（`{\s0…\sl276\slmult1…}` 是默认段样式），
//!    与制表位那条同一个坑：段自己没写时它是从样式继承的，归属判不住，这一支不读。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// OOXML 那一份：`w:line` 与 `w:lineRule` 一起交（前者是什么单位由后者决定）
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut rules = serde_json::Map::new();
    let mut with_line = 0usize;
    let mut with_rule = 0usize;
    let mut both = 0usize;
    for (index, para) in body.descendants("p").iter().enumerate() {
        let holder = para.child("pPr");
        let spacing =
            holder.and_then(|had| had.children.iter().find(|kid| kid.local() == "spacing"));
        let line = spacing
            .and_then(|had| had.attr_local("line"))
            .map(String::from);
        let rule = spacing
            .and_then(|had| had.attr_local("lineRule"))
            .map(String::from);
        if line.is_some() {
            with_line += 1;
        }
        if let Some(raw) = &rule {
            with_rule += 1;
            let next = rules.get(raw).and_then(Value::as_u64).unwrap_or(0) + 1;
            rules.insert(raw.clone(), json!(next));
        }
        if line.is_some() && rule.is_some() {
            both += 1;
        }
        rows.push(json!({
            "index": index,
            "has_pPr": holder.is_some(),
            "has_spacing": spacing.is_some(),
            "line_written": line,
            "rule_written": rule,
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs_total": rows.len(),
        "with_line_written": with_line,
        "with_rule_written": with_rule,
        "with_both": both,
        "rules_written": Value::Object(rules),
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份用到的那几个属性（`style:line-height-*` 这一族一起看才说得清）
const ODF_LINE_PROPS: [&str; 4] = [
    "line-height",
    "line-height-style",
    "line-height-inherit",
    "line-break",
];

/// ODF 那一份：段只点样式名，行高在那份样式的段落属性里（单位写在串上）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut roots: Vec<&Node> = vec![content];
    if let Some(extra) = styles {
        roots.push(extra);
    }
    let mut table: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for root in &roots {
        for one in root.descendants("style").into_iter() {
            if one.attr_local("family") != Some("paragraph") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            let mut written: Vec<(String, String)> = Vec::new();
            if let Some(holder) = one.child("paragraph-properties") {
                for key in ODF_LINE_PROPS {
                    if let Some(raw) = holder.attr_local(key) {
                        written.push((key.to_string(), raw.to_string()));
                    }
                }
            }
            table.push((name.to_string(), written));
        }
    }
    let mut rows: Vec<Value> = Vec::new();
    let mut forms = serde_json::Map::new();
    let mut with_height = 0usize;
    for (index, para) in content.descendants("p").iter().enumerate() {
        let name = para.attr_local("style-name");
        let held = name
            .and_then(|want| table.iter().find(|had| had.0 == *want))
            .map(|had| had.1.clone())
            .unwrap_or_default();
        let height = held
            .iter()
            .find(|had| had.0 == "line-height")
            .map(|had| had.1.clone());
        if let Some(raw) = &height {
            with_height += 1;
            // 单位是写在串里的（百分数 / cm / pt …），这里只按尾巴分个类，不换算
            let tail: String = raw
                .chars()
                .skip_while(|had| had.is_ascii_digit() || *had == '.' || *had == '-')
                .collect();
            let key = if tail.is_empty() {
                "无单位"
            } else {
                tail.as_str()
            };
            let next = forms.get(key).and_then(Value::as_u64).unwrap_or(0) + 1;
            forms.insert(key.to_string(), json!(next));
        }
        let mut entry = json!({
            "index": index,
            "style_written": name.map(String::from),
            "line_height_written": height,
        });
        if let Some(map) = entry.as_object_mut() {
            for key in ODF_LINE_PROPS.iter().skip(1) {
                let field = key.replace('-', "_");
                let value = held
                    .iter()
                    .find(|had| had.0 == **key)
                    .map(|had| had.1.clone());
                map.insert(field, json!(value));
            }
        }
        rows.push(entry);
    }
    json!({
        "family": "odf",
        "available": true,
        "paragraphs_total": rows.len(),
        "with_line_height": with_height,
        "styles_total": table.len(),
        "unit_forms": Value::Object(forms),
        "paragraphs": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
