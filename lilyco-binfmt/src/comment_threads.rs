//! 「哪条批注已解决、谁回复谁」——OOXML 把这句话写在**另外两份部件**里，靠段号连过来，
//! ODF 只把「结没结」压在注自己身上（`loext:resolved`），而回复那一问在这一族没有位置
//!
//! OOXML 的形状：`word/comments.xml` 里那条 `w:comment` 只有 `w:id` / `w:author` / `w:date`，
//! 它体内那一段写作 `<w:p w14:paraId="…">` —— 号在这里。第二份 `word/commentsExtended.xml`
//! 的 `w15:commentEx` 用 `@w15:paraId` **指回那个段号**（不是 `w:id`），带 `@w15:done`
//! （结没结）与 `@w15:paraIdParent`（回复了哪一条，也是段号）。第三份 `word/commentsIds.xml`
//! 又给同一个段号配一枚 `@w16cid:durableId`。所以一问四份数据、两跳才连得上。
//!
//! 实测五份件（`crep.docx` 把三种情形写全：已解决 / 回复且未解决 / 号对不上任何批注；
//! `crep-lo.docx` 与 `crep-r.docx` 是 LibreOffice 的两个方向；`crep.odt` 与 `crep-r.odt`
//! 是它的 odt 出口；`comments.docx` 是「有注而这一格一个字都没写」的反面凭据）：
//! 1. **两跳各自都可能断**：`crep.docx` 三条 `commentEx` 只有两条对得上批注（`ext_orphans` 1），
//!    `commentsIds.xml` 三条里也有一条对不上（`ids_orphans` 1）—— 合成一个「几条批注」就把
//!    文件里断着的线读成没断；两个方向都数。
//! 2. 「没写」与「写了 0」是两件事：`crep-r.docx`（LibreOffice 从 odt 导出的 docx，
//!    这一份的 `commentsExtended.xml` **是生产者自己写的**）里 2 条注只有 1 条 `commentEx`，
//!    另一条是 `ex_found: false` 而**不是** `done_written: "0"`；而 `crep.docx` 那条明确写着
//!    `done="0"`。三档各自一数：`done_true` / `done_false` / `ex_without_done`。
//! 3. 生产者会丢整份：LibreOffice 把 `crep.docx` 重写一遍成 `crep-lo.docx`，两份部件**整个不见**，
//!    连批注体内那个 `w14:paraId` 也一并没了（`paras_with_para_id` 从 2 掉到 0）——
//!    于是「回复」与「已解决」两头都读不出来，这是这份件的事实，不替它接。
//! 4. 反向也测了：同一问走 odt 再回 docx，LO 会写 `w15:done="1"`（只对已解决那条），
//!    而 docx→odt 那一路它对 `w15:done` **看都不看** —— `crep.odt` 两条注都写
//!    `loext:resolved="false"`，源件里那条 `done="1"` 没落成 `true`。
//! 5. ODF 这一族只有半句话：`loext:resolved` 有（`crep-r.odt` 一份件里同时有 `true` 与 `false`），
//!    而**回复没有任何对应物** —— 那两格里既没有 parent 也没有 thread，所以这一族不交回复那几格
//!    （不是 0）。另外 `cell-notes.ods` 三条注**一条都没写** `loext:resolved`
//!    （`without_resolved` 3）：同一个生产者的两种文档，一种写满、一种一个字不写。
//!
//! 遗留 .rtf / .doc 不交这个键（缺键 = 这一族没看）：RTF 的注只有 `\author` 那一路控制字，
//! 没有「谁回复谁」与「结没结」的位置。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 按局部名取一枚属性（`w15:paraId`、`w14:paraId`、`@done` 都是这么读的）
fn had(node: &Node, want: &str) -> Option<String> {
    node.attr_local(want).map(String::from)
}

/// 一份部件解析成树；部件不存在就是 None
fn part_root(bytes: &[u8], part: &str) -> Option<Node> {
    match zipread::member(bytes, part, DEFAULT_MEMBER_CAP) {
        Ok(member) => Some(xmlscan::parse_str(&member.as_text())),
        Err(_) => None,
    }
}

fn str_or_null(text: Option<String>) -> Value {
    match text {
        Some(one) => Value::String(one),
        None => Value::Null,
    }
}

/// OOXML 那一份：批注体内段号 → `commentsExtended` / `commentsIds`，两跳各数一次断口
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let croot = part_root(bytes, "word/comments.xml");
    let eroot = part_root(bytes, "word/commentsExtended.xml");
    let iroot = part_root(bytes, "word/commentsIds.xml");
    let comments: Vec<&Node> = match &croot {
        Some(root) => root.descendants("comment"),
        None => Vec::new(),
    };
    let ext: Vec<&Node> = match &eroot {
        Some(root) => root.descendants("commentEx"),
        None => Vec::new(),
    };
    let ids: Vec<&Node> = match &iroot {
        Some(root) => root.descendants("commentId"),
        None => Vec::new(),
    };

    let mut rows: Vec<Value> = Vec::new();
    for (index, one) in comments.iter().enumerate() {
        let paras = one.descendants("p");
        let para_id = if paras.is_empty() {
            None
        } else {
            had(paras[0], "paraId")
        };
        rows.push(json!({
            "index": index,
            "id": str_or_null(had(one, "id")),
            "author": str_or_null(had(one, "author")),
            "para_id": str_or_null(para_id),
            "ex_found": false,
            "done_written": Value::Null,
            "done": Value::Null,
            "parent_para_id": Value::Null,
            "replies_to": Value::Null,
            "durable_id": Value::Null,
        }));
    }
    // 段号 → 本清单里第几条（先到先得，与第二读者同一条规矩）
    let mut owners: Vec<(String, usize)> = Vec::new();
    for one in rows.iter() {
        let pid = match one["para_id"].as_str() {
            Some(text) => text.to_string(),
            None => continue,
        };
        let mine = one["index"].as_u64().unwrap_or(0) as usize;
        if !owners.iter().any(|(name, _)| *name == pid) {
            owners.push((pid, mine));
        }
    }
    let mut matched_ext: Vec<String> = Vec::new();
    let mut matched_ids: Vec<String> = Vec::new();
    for row in rows.iter_mut() {
        let pid = match row["para_id"].as_str() {
            Some(text) => text.to_string(),
            None => continue,
        };
        let holder = ext
            .iter()
            .find(|one| one.attr_local("paraId") == Some(pid.as_str()))
            .copied();
        if let Some(found) = holder {
            if !matched_ext.iter().any(|name| *name == pid) {
                matched_ext.push(pid.clone());
            }
            let done = had(found, "done");
            let parent = had(found, "paraIdParent");
            row["ex_found"] = json!(true);
            row["done_written"] = str_or_null(done.clone());
            row["done"] = match &done {
                Some(text) => json!(text.as_str() == "1"),
                None => Value::Null,
            };
            row["parent_para_id"] = str_or_null(parent);
        }
        let with_id = ids
            .iter()
            .find(|one| one.attr_local("paraId") == Some(pid.as_str()))
            .copied();
        if let Some(found) = with_id {
            if !matched_ids.iter().any(|name| *name == pid) {
                matched_ids.push(pid.clone());
            }
            row["durable_id"] = str_or_null(had(found, "durableId"));
        }
    }
    for row in rows.iter_mut() {
        let parent = match row["parent_para_id"].as_str() {
            Some(text) => text.to_string(),
            None => continue,
        };
        if let Some((_, owner)) = owners.iter().find(|(name, _)| *name == parent) {
            row["replies_to"] = json!(owner);
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "comments_total": rows.len(),
        "paras_with_para_id": rows.iter().filter(|one| !one["para_id"].is_null()).count(),
        "ext_part_written": eroot.is_some(),
        "ext_total": ext.len(),
        "ext_matched": matched_ext.len(),
        "ext_orphans": ext.len() - matched_ext.len(),
        "done_written_total": rows.iter().filter(|one| !one["done_written"].is_null()).count(),
        "done_true": rows.iter().filter(|one| one["done"] == json!(true)).count(),
        "done_false": rows.iter().filter(|one| one["done"] == json!(false)).count(),
        "ex_without_done": rows.iter().filter(|one| {
            one["ex_found"] == json!(true) && one["done_written"].is_null()
        }).count(),
        "replies_total": rows.iter().filter(|one| !one["parent_para_id"].is_null()).count(),
        "replies_dangling": rows.iter().filter(|one| {
            !one["parent_para_id"].is_null() && one["replies_to"].is_null()
        }).count(),
        "ids_part_written": iroot.is_some(),
        "ids_total": ids.len(),
        "ids_matched": matched_ids.len(),
        "ids_orphans": ids.len() - matched_ids.len(),
        "threads": take(rows, limit),
    })
}

/// ODF 那一份：`loext:resolved` 压在注自己身上，两份件都走；回复那一问这一族没有位置
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut parts_seen: Vec<String> = Vec::new();
    let holders = [("content.xml", Some(content)), ("styles.xml", styles)];
    for (part, root) in holders {
        let found: Vec<&Node> = match root {
            Some(one) => one.descendants("annotation"),
            None => Vec::new(),
        };
        if found.is_empty() {
            continue;
        }
        parts_seen.push(part.to_string());
        for one in found {
            let written = had(one, "resolved");
            rows.push(json!({
                "index": rows.len(),
                "part": part,
                "name_written": str_or_null(had(one, "name")),
                "resolved_written": str_or_null(written.clone()),
                "resolved": match &written {
                    Some(text) => json!(text == "true"),
                    None => Value::Null,
                },
            }));
        }
    }
    json!({
        "family": "odf",
        "available": true,
        "annotations_total": rows.len(),
        "parts_seen": parts_seen,
        "with_resolved_written": rows.iter()
            .filter(|one| !one["resolved_written"].is_null()).count(),
        "resolved_true": rows.iter().filter(|one| one["resolved"] == json!(true)).count(),
        "resolved_false": rows.iter().filter(|one| one["resolved"] == json!(false)).count(),
        "without_resolved": rows.iter()
            .filter(|one| one["resolved_written"].is_null()).count(),
        "annotations": take(rows, limit),
    })
}

fn take(rows: Vec<Value>, limit: usize) -> Vec<Value> {
    rows.into_iter().take(limit).collect::<Vec<Value>>()
}
