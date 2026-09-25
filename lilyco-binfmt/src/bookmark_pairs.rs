//! 「这些书签是怎么配对的」——一族的结束记号只写号不写名字（所以要按号配），
//! 一族的跨段记号两头都写名字（所以按名字配），而同一句话在两族里可能是**一枚点**而不是**一对**
//!
//! OOXML：`w:bookmarkStart` 带 `w:id` 与 `w:name`，`w:bookmarkEnd` **只带 `w:id`** ——
//! 所以「这条书签闭没闭」只能按号配，而号是生产者自己排的（实测 LibreOffice 重写时把
//! 1..5 重排成 0..3）。「开始没有结束」与「结束没有开始」是两本账，各数各的。
//! 名字还可以重复（实测两条都叫 `口径`）：重名不合并，另外给 `duplicate_names`。
//! 名字以下划线开头的（`_GoBack`）是 Word 自己塞的光标记号，不是人起的名字，
//! 所以 `hidden_starts` 另数一笔 —— 把它算进「这份文档有几个书签」就替 Word 说了话。
//!
//! ODF：三种记号 —— `text:bookmark`（**一个点**）、`text:bookmark-start` / `-end`（跨段的一对，
//! 两头都写 `text:name`，所以按**名字**配）。最要紧的一条：同段起止的一对在这族被写成
//! **一枚点**（实测 `口径` 与 `_GoBack` 都成了 `text:bookmark`），所以「start 几条」在两族
//! 不是同一个问，两个数都不换算。
//!
//! 实测三份件（`bkmks.docx` 由 python-docx + `OxmlElement` 写八段，各改一个变量）：
//! 1. 原件 5 起 5 止：闭 3 对、1 条开始没有结束（`断了`）、1 条结束没有开始（号 `9`）；
//! 2. LibreOffice 重写同一份 docx：把两个**断的整个删掉**（5 起 5 止 → 4 起 4 止）、
//!    号全部重排、第二条重名的 `口径` 改名成 `口径_副本_1` —— 而站内跳转的 `w:anchor="跨段"` 一字未改；
//! 3. 转成 ODF：`text:bookmark` 三枚（闭在同段的与 Word 那条光标的）加一对跨段的
//!    `bookmark-start`/`-end`（都写 `跨段`），那个改名的副本在这一族写成 `口径 副本 1`（**空格**），
//!    而引用它的 `text:a` 写 `href="#跨段"`。
//!
//! RTF 那一族的 `\bkmkstart` / `\bkmkend` 早就在读（见 `structure.bookmarks` 那一条），
//! 这一支不再交一次；遗留 .doc 的段落在表流里，两个键都不交。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// 号有没有配上：`None` 与**空串**都不算一个号（与第二读者同一条）
fn pairs_with(id: &Option<String>, pool: &[Option<String>]) -> bool {
    match id {
        Some(raw) if !raw.is_empty() => pool
            .iter()
            .any(|had| matches!(had, Some(had) if !had.is_empty() && had == raw)),
        _ => false,
    }
}

/// 记下每个名字出现了几次（按文件里第一次出现的顺序）
fn tally_names(seen: &mut Vec<(String, usize)>, raw: &Option<String>) {
    let Some(name) = raw else { return };
    if let Some(had) = seen.iter_mut().find(|had| &had.0 == name) {
        had.1 += 1;
        return;
    }
    seen.push((name.clone(), 1));
}

/// OOXML 那一份：起与止各一条 list，配对按 `w:id`
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut start_rows: Vec<Value> = Vec::new();
    let mut end_rows: Vec<Value> = Vec::new();
    let mut start_ids: Vec<Option<String>> = Vec::new();
    let mut end_ids: Vec<Option<String>> = Vec::new();
    let mut in_para = 0usize;
    for (index, para) in body.descendants("p").iter().enumerate() {
        for kid in para.descendants("bookmarkStart") {
            let id = kid.attr_local("id").map(String::from);
            let name = kid.attr_local("name").map(String::from);
            start_ids.push(id.clone());
            start_rows.push(json!({
                "paragraph": index,
                "id_written": id,
                "name_written": name.clone(),
                "hidden": name.map(|raw| raw.starts_with('_')).unwrap_or(false),
            }));
        }
        for kid in para.descendants("bookmarkEnd") {
            let id = kid.attr_local("id").map(String::from);
            end_ids.push(id.clone());
            end_rows.push(json!({
                "paragraph": index,
                "id_written": id,
                "name_written": kid.attr_local("name").map(String::from),
            }));
        }
    }
    in_para = start_rows.len() + end_rows.len();
    let loose = (body.descendants("bookmarkStart").len() + body.descendants("bookmarkEnd").len())
        .saturating_sub(in_para);
    let mut closed = 0usize;
    let mut names: Vec<(String, usize)> = Vec::new();
    let mut hidden = 0usize;
    for (row, id) in start_rows.iter_mut().zip(start_ids.iter()) {
        let done = pairs_with(id, &end_ids);
        if done {
            closed += 1;
        }
        if let Some(object) = row.as_object_mut() {
            object.insert("has_end".to_string(), json!(done));
        }
        tally_names(&mut names, &row["name_written"].as_str().map(String::from));
        if row["hidden"] == json!(true) {
            hidden += 1;
        }
    }
    for row in end_rows.iter_mut() {
        let id = row["id_written"].as_str().map(String::from);
        let found = pairs_with(&id, &start_ids);
        if let Some(object) = row.as_object_mut() {
            object.insert("has_start".to_string(), json!(found));
        }
    }
    let orphan_starts = start_rows
        .iter()
        .filter(|row| row["has_end"] == json!(false))
        .count();
    let orphan_ends = end_rows
        .iter()
        .filter(|row| row["has_start"] == json!(false))
        .count();
    let duplicates: Vec<String> = names
        .iter()
        .filter(|had| had.1 > 1)
        .map(|had| had.0.clone())
        .collect();
    json!({
        "family": "ooxml",
        "available": true,
        "starts_total": start_rows.len(),
        "ends_total": end_rows.len(),
        "pairs_closed": closed,
        "starts_without_end": orphan_starts,
        "ends_without_start": orphan_ends,
        "names_total": names.iter().map(|had| had.1).sum::<usize>(),
        "distinct_names": names.iter().map(|had| had.0.clone()).collect::<Vec<String>>(),
        "duplicate_names": duplicates,
        "hidden_starts": hidden,
        "marks_outside_paragraphs": loose,
        "starts": start_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "ends": end_rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：点是一枚 `text:bookmark`，跨段是一对按名字配的 start/end
pub(crate) fn odf(content: &Node, limit: usize) -> Value {
    let mut point_rows: Vec<Value> = Vec::new();
    let mut start_rows: Vec<Value> = Vec::new();
    let mut end_rows: Vec<Value> = Vec::new();
    let mut start_names: Vec<Option<String>> = Vec::new();
    let mut end_names: Vec<Option<String>> = Vec::new();
    for (index, para) in content.descendants("p").iter().enumerate() {
        for kid in para.descendants("bookmark") {
            let name = kid.attr_local("name").map(String::from);
            point_rows.push(json!({"paragraph": index, "name_written": name}));
        }
        for kid in para.descendants("bookmark-start") {
            let name = kid.attr_local("name").map(String::from);
            start_names.push(name.clone());
            start_rows.push(json!({"paragraph": index, "name_written": name}));
        }
        for kid in para.descendants("bookmark-end") {
            let name = kid.attr_local("name").map(String::from);
            end_names.push(name.clone());
            end_rows.push(json!({"paragraph": index, "name_written": name}));
        }
    }
    let in_para = point_rows.len() + start_rows.len() + end_rows.len();
    let loose = (content.descendants("bookmark").len()
        + content.descendants("bookmark-start").len()
        + content.descendants("bookmark-end").len())
    .saturating_sub(in_para);
    let mut closed = 0usize;
    let mut names: Vec<(String, usize)> = Vec::new();
    for row in start_rows.iter_mut() {
        let name = row["name_written"].as_str().map(String::from);
        let done = pairs_with(&name, &end_names);
        if done {
            closed += 1;
        }
        if let Some(object) = row.as_object_mut() {
            object.insert("has_end".to_string(), json!(done));
        }
    }
    for row in end_rows.iter_mut() {
        let name = row["name_written"].as_str().map(String::from);
        let found = pairs_with(&name, &start_names);
        if let Some(object) = row.as_object_mut() {
            object.insert("has_start".to_string(), json!(found));
        }
    }
    for row in point_rows.iter() {
        tally_names(&mut names, &row["name_written"].as_str().map(String::from));
    }
    for row in start_rows.iter() {
        tally_names(&mut names, &row["name_written"].as_str().map(String::from));
    }
    let duplicates: Vec<String> = names
        .iter()
        .filter(|had| had.1 > 1)
        .map(|had| had.0.clone())
        .collect();
    json!({
        "family": "odf",
        "available": true,
        "points_total": point_rows.len(),
        "spans_start": start_rows.len(),
        "spans_end": end_rows.len(),
        "spans_closed": closed,
        "starts_without_end": start_rows.iter().filter(|row| row["has_end"] == json!(false)).count(),
        "ends_without_start": end_rows.iter().filter(|row| row["has_start"] == json!(false)).count(),
        "names_total": names.iter().map(|had| had.1).sum::<usize>(),
        "distinct_names": names.iter().map(|had| had.0.clone()).collect::<Vec<String>>(),
        "duplicate_names": duplicates,
        "marks_outside_paragraphs": loose,
        "points": point_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "span_starts": start_rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "span_ends": end_rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
