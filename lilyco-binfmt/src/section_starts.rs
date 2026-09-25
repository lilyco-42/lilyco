//! 「这一节是从哪儿开始的」——另起一页 / 连续 / 奇偶页，OOXML 写在 `w:sectPr/w:type` 上
//!
//! 形状：一节一条 `{section, element_present, type_written, written}`，外加
//! `sections_total` / `with_element` / `distinct_types` / `type_missing`。
//! `element_present` 说那枚 `w:type` 在不在，`written` 交它自己写着的属性表
//! （值在 `@w:val` 上，键名按写的交，不折成 `type`）。
//!
//! 实测两份件（`sstart.docx` 由 python-docx 写三节：第一节设成「另起一页」、第二节「连续」、
//! 第三节「偶数页」；`sstart-lo.docx` 是 LibreOffice 的同格式重写）：
//! 1. **「另起一页」在 python-docx 那份里根本没写** —— 那是 Word 的默认值，于是第一节
//!    `element_present: false`、`type_written: null`；而 LibreOffice 重写同一份时**把它写出来了**
//!    （`<w:type w:val="nextPage"/>`），第一节变成 true + `"nextPage"`。
//!    同一份稿子两副样子，「没说」与「说了默认」是两件事，所以两份各交各的；
//! 2. `continuous` 与 `evenPage` 两个值往返一次一字未变 —— 这一族生产者改的是「说没说」，
//!    不是「说了什么」；
//! 3. 值词表就按文件写的交（`nextPage` / `continuous` / `evenPage` / `oddPage` /
//!    `unsupportedType` 是规范里那五个，本语料出现前三个）—— 不做归一、不折成布尔。
//!
//! ODF 那一族**不交这个键**（缺键 = 这一族没看，不是 0），理由是量过的：LibreOffice 把
//! `sstart.docx` 转成 odt 之后，「连续」那一节变成一枚 `text:section`（`text:name="TextSection"`），
//! 而「另起一页 / 偶数页」那两节既没有 `text:section` 也没有任何写着起始类型的地方，
//! 分页改由段落属性上的 `style:master-page-name="Converted2"` 承担 —— 同一句问话在这族里
//! 被拆成两处、还有一半没落纸，所以不硬凑一个键；页版式与母版页那两处的账另在
//! `page_numbering` 与 `header_footers` 里。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// 一个元素的属性表（局部名；命名空间声明不算属性）
fn attrs_of(node: &Node) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    out
}

/// OOXML 那一份：正文里每一条 `w:sectPr` 一条记录（含文末那条）
pub(crate) fn docx(document: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    for (index, one) in document.descendants("sectPr").iter().enumerate() {
        let holder = match one.descendants("type").into_iter().next() {
            Some(had) => had,
            None => {
                rows.push(json!({
                    "section": index,
                    "element_present": false,
                    "type_written": Value::Null,
                    "written": Value::Object(serde_json::Map::new()),
                }));
                continue;
            }
        };
        let table = attrs_of(holder);
        let value = table
            .get("val")
            .and_then(|had| had.as_str())
            .map(String::from);
        if let Some(text) = &value {
            if !types.iter().any(|one| one == text) {
                types.push(text.clone());
            }
        }
        rows.push(json!({
            "section": index,
            "element_present": true,
            "type_written": match &value {
                Some(text) => json!(text),
                None => Value::Null,
            },
            "written": Value::Object(table),
        }));
    }
    let with_element = rows
        .iter()
        .filter(|one| one["element_present"] == json!(true))
        .count();
    json!({
        "family": "ooxml",
        "available": true,
        "sections_total": rows.len(),
        "with_element": with_element,
        "type_missing": rows.iter().filter(|one| one["type_written"].is_null()).count(),
        "distinct_types": types,
        "sections": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
