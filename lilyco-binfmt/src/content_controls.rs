//! 内容控件那一份账 —— 一枚 `w:sdt` 说「这里是可以填的一块」，是哪一种要看它自己写了什么。
//!
//! Word 把这块写成三截：`w:sdtPr`（这块自己的说明）、`w:sdtEndPr`（可选，收尾的那份属性）、
//! `w:sdtContent`（真正的正文）。`sdtPr` 里那串孩子**属于类型词表的那一枚**才是「它是哪一种
//! 控件」（`w:text` / `w:richText` / `w:date` / `w:dropDownList` / `w:comboBox` / `w:picture` /
//! `w:docPartObj` …），而 `w:alias` 与 `w:tag` 与 `w:id` 是给人看的三个名，`w:lock` 说这块
//! 能不能改，`w:dataBinding` 那一跳指向包里的 XML 数据（四个属性全是串），`w:placeholder`
//! 与 `w:showingPlcHdr` 是「现在还是占位」那两句话。
//!
//! 只住 OOXML 的文字那一族：`office-doc` 的 docx 支交 `structure.content_controls`，
//! odt / rtf / 遗留 .doc 与表与放映那几个出口**没有这一格**（没看过就没有，而不是交零条）。
//!
//! 实测（`sdt.docx` = python-docx 打底 + 按 ECMA 手写十一枚（含一枚套娃）；`sdt-lo.docx` =
//! LibreOffice 重写同一份；`sdt.odt` = LibreOffice 转出去的那一份）：
//! 1. LibreOffice 重写同一份做七件事（每件都是这本账的一条凭据）：正文空着那枚**整枚不见**
//!    （11 → 10 枚）、两枚 `w:sdtEndPr` 全丢（`endpr_present` 2 → 0）、`w:richText` 被降级成
//!    `w:text`（`types` 从 `text` 2 + `richText` 3 变成 `text` 6）、日期那两属性
//!    （`dateFormat="yyyy年MM月"` + `calendarType="chineseLunar"`）被换成它自己算出来的
//!    `fullDate="2026-09-27T00:00:00Z"`、下拉那枚的 `w:alias` 被改写成**空串**并添上
//!    `lastValue="0"`、`dataBinding` 的 `storeSchemaID` 掉了而 `sdtPr` 的孩子**换了序**
//!    （`lock` 与 `showingPlcHdr` 跑到 `dataBinding` 前面）、套娃那一层被摊平（内层变成兄弟、
//!    外层失去类型元素）；另外把住在控件里的那张表**摊回正文**：`tables_total` 1 → 0 而整件那本
//!    账仍是 1 张（两本各答各的问，拿一本当另一本会说出「这份没有表了」那种错话）；
//! 2. 「正文里有几段」在两本里是两个数：手写那本 `paras_direct` 11 而 `paras_total` 14
//!    （表格里与套娃内层各带段），重写那本**只剩 3** —— 因为 LibreOffice 把段摊成直接挂在
//!    `w:sdtContent` 下的 `w:r`（字还在：`chars` 82 → 69 而 `runs` 14 → 23）；两个口径都交；
//! 3. 真件普查（本机 33 份真件 .docx / .docm，排在仓库之外）：**只有 4 份写了这一层、各一枚**，
//!    而这四枚 `sdtPr` 里**只写了 `w:id`**（类型元素一枚都没有、`alias` 与 `tag` 也一枚没有、
//!    `dataBinding` 与 `lock` 零枚），而那四枚**全都带** `w:sdtEndPr` —— 「Word 自己导出的控件长这样」
//!    与「模板里那块目录长这样」是两种形状，所以 `type_none` 与 `endpr_present` 分列，
//!    而不是合成一句「完整吗」；
//! 4. 住址这一条是真件给的：那 4 枚**全住 `word/footer1.xml`，正文里一枚都没有** —— 只顺着正文那一路走
//!    会一枚不剩，所以这一族扫 `word/*.xml` 全部部件并按件名排序。自产件是另一本账（90 份 docx/docm
//!    里 6 份写了、共 25 枚，全在正文：`sdt.docx` 11、`sdt-lo.docx` 10、`toc.docx` / `toc-full.docx` /
//!    `customxml.docx` / `customxml-lo.docx` 各 1）—— 目录那一块本身就是这一族的一个 gallery 值
//!    （`toc.docx`：控件 1 而 `contents.entries` 0），所以那一本账（`contents`）与这一本各数各的，不互推；
//! 5. 转成 .odt：ODF **标准**里没有这一层（`<form:` 零枚），但 LibreOffice 把那 10 枚中的 6 枚写进了
//!    自家扩展命名空间 `loext:content-control`（五个名 `alias`/`id`/`tag`/`lock`/`plain-text`，
//!    一个类型都不写），控件里的字与段落都还在（`text:p` 14）。本仓的 odt 读者不读 `loext:`，
//!    所以 odt / rtf / 遗留 .doc 那几支**不交这一格** —— 那一句是「这个读者没读那个」，
//!    而不是「这一层不存在」，交一本零条的账会冒充「读过且读到了没有」。
//!
//! 不做的事：**不猜类型**（`sdtPr` 的孩子整列按写的序交，`type_seen` 只取词表里第一枚，
//! 另交 `type_count` 说一共写了几枚）、**不解 `dataBinding` 那一跳**（`@w:xpath` 指到包里
//! 哪份 XML 要读 `customXml/itemN.xml`，那是 `custom_xml` 那一本的事）、**不判锁得住不住**
//! （`w:lock/@w:val` 有五种枚举，这里只按写的串交）、**不读 `w:date` 的孩子**（那三格两个生产者各写一种：
//! 手写那枚写成 `w:date` 的属性（`dateFormat` + `calendarType`），LibreOffice 那枚既写属性 `fullDate`
//! 又写成它的孩子（`dateFormat` / `calendar` / `lid` / `storeMappedDataAs`）—— 这里只交属性那一本，
//! 所以后一种写法的那一格看着只剩一个数；真件 33 份里 `w:date` 一枚都没有，这一句只能说两个生产者）、
//! **不交 `@w:displayText`**（`w:listItem` 那两格同名不同事，LibreOffice
//! 重写时改的正是 `displayText`（`甲选项` → `甲`）而 `@w:value` 一字未动，所以 `list_values`
//! 只交值那一本）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// 「这是哪一种控件」的那批类型元素名（`w:sdtPr` 的孩子里撞上几个就交 `type_count` 几）
const SDT_TYPES: [&str; 14] = [
    "text",
    "richText",
    "plainText",
    "date",
    "picture",
    "dropDownList",
    "comboBox",
    "gallery",
    "docPartObj",
    "formula",
    "cite",
    "blockList",
    "smartTag",
    "repeatingSection",
];

/// `sdtPr` 里那五枚「只有一个 `@w:val`」的元素
const SDT_VALS: [&str; 5] = ["alias", "tag", "id", "lock", "placeholder"];

/// 七本按行累加的数（口径写在键名上，`_direct` 只数 `sdtContent` 的直接孩子）
const SUM_KEYS: [&str; 7] = [
    "paras_direct",
    "paras_total",
    "tables_direct",
    "tables_total",
    "cells",
    "runs",
    "chars",
];

fn sdt_kids(node: Option<&Node>) -> Vec<&Node> {
    match node {
        Some(one) => one
            .children
            .iter()
            .filter(|kid| kid.name != "#text")
            .collect(),
        None => Vec::new(),
    }
}

fn sdt_kid<'a>(node: Option<&'a Node>, want: &str) -> Option<&'a Node> {
    sdt_kids(node).into_iter().find(|kid| kid.local() == want)
}

fn sdt_attrs(node: Option<&Node>) -> Value {
    let Some(one) = node else { return Value::Null };
    let mut out = serde_json::Map::new();
    for (key, value) in one.attrs.iter() {
        let local = key.rsplit(':').next().unwrap_or(key.as_str()).to_string();
        out.insert(local, json!(value));
    }
    Value::Object(out)
}

fn sdt_val(node: Option<&Node>) -> Value {
    match node.and_then(|one| one.attr_local("val")) {
        Some(had) => json!(had),
        None => Value::Null,
    }
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_u64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

fn said(row: &Value, key: &str) -> usize {
    usize::from(row[key].as_bool().unwrap_or(false))
}

/// 一枚 `w:sdt` → 一行（`index` 与 `depth` 由走树那一步给，与第二读者同一个序）
fn sdt_row(one: &Node, part: &str, index: usize, depth: usize) -> Value {
    let pr = sdt_kid(Some(one), "sdtPr");
    let written: Vec<String> = sdt_kids(pr)
        .iter()
        .map(|kid| kid.local().to_string())
        .collect();
    let hit: Vec<String> = written
        .iter()
        .filter(|one| SDT_TYPES.contains(&one.as_str()))
        .cloned()
        .collect();
    let mut row = serde_json::Map::new();
    row.insert("part".to_string(), json!(part));
    row.insert("index".to_string(), json!(index));
    row.insert("depth".to_string(), json!(depth));
    row.insert("pr_present".to_string(), json!(pr.is_some()));
    row.insert("pr_children".to_string(), json!(written));
    row.insert(
        "type_seen".to_string(),
        match hit.first() {
            Some(one) => json!(one.as_str()),
            None => Value::Null,
        },
    );
    row.insert("type_count".to_string(), json!(hit.len()));
    for key in SDT_VALS.iter() {
        let had = sdt_kid(pr, key);
        row.insert((*key).to_string(), sdt_val(had));
        row.insert(format!("{}_present", key), json!(had.is_some()));
    }
    row.insert(
        "showing_plc_hdr".to_string(),
        json!(sdt_kid(pr, "showingPlcHdr").is_some()),
    );
    row.insert(
        "data_binding".to_string(),
        sdt_attrs(sdt_kid(pr, "dataBinding")),
    );
    row.insert("date".to_string(), sdt_attrs(sdt_kid(pr, "date")));
    let holder = sdt_kid(pr, "dropDownList").or(sdt_kid(pr, "comboBox"));
    let items: Vec<&Node> = sdt_kids(holder)
        .into_iter()
        .filter(|kid| kid.local() == "listItem")
        .collect();
    row.insert(
        "list_kind".to_string(),
        match holder {
            Some(had) => json!(had.local()),
            None => Value::Null,
        },
    );
    row.insert("list_items".to_string(), json!(items.len()));
    row.insert(
        "list_values".to_string(),
        // `w:listItem` 的那个值是 `@w:value`（与上面那五枚的 `@w:val` 不同名），
        // 旁边还有 `@w:displayText`：LibreOffice 重写时把说明换掉了而 `@w:value` 一字未动
        json!(items
            .iter()
            .map(|kid| match kid.attr_local("value") {
                Some(had) => json!(had),
                None => Value::Null,
            })
            .collect::<Vec<Value>>()),
    );
    let obj = sdt_kid(pr, "docPartObj");
    row.insert("doc_part_obj".to_string(), json!(obj.is_some()));
    row.insert(
        "gallery".to_string(),
        sdt_val(obj.and_then(|had| sdt_kid(Some(had), "docPartGallery"))),
    );
    row.insert(
        "endpr_present".to_string(),
        json!(sdt_kid(Some(one), "sdtEndPr").is_some()),
    );
    let body = sdt_kid(Some(one), "sdtContent");
    let dn: Vec<String> = sdt_kids(body)
        .iter()
        .map(|kid| kid.local().to_string())
        .collect();
    row.insert("content_present".to_string(), json!(body.is_some()));
    row.insert("content_children".to_string(), json!(dn));
    row.insert(
        "paras_direct".to_string(),
        json!(dn.iter().filter(|one| one.as_str() == "p").count()),
    );
    row.insert(
        "tables_direct".to_string(),
        json!(dn.iter().filter(|one| one.as_str() == "tbl").count()),
    );
    match body {
        Some(had) => {
            row.insert("paras_total".to_string(), json!(had.descendants("p").len()));
            row.insert(
                "tables_total".to_string(),
                json!(had.descendants("tbl").len()),
            );
            row.insert("cells".to_string(), json!(had.descendants("tc").len()));
            row.insert("runs".to_string(), json!(had.descendants("r").len()));
            let mut chars = 0usize;
            for kid in had.descendants("t") {
                chars += kid.text().chars().count();
            }
            row.insert("chars".to_string(), json!(chars));
        }
        None => {
            for key in SUM_KEYS.iter().skip(1) {
                row.insert((*key).to_string(), json!(0));
            }
        }
    }
    Value::Object(row)
}

/// 前序走一遍：与第二读者 `root.iter()` 同一个文档序（套娃那枚排在它父亲后面）
fn walk(node: &Node, part: &str, depth: usize, out: &mut Vec<Value>) {
    for one in node.children.iter() {
        if one.name == "#text" {
            continue;
        }
        if one.local() == "sdt" {
            out.push(sdt_row(one, part, out.len(), depth));
            walk(one, part, depth + 1, out);
            continue;
        }
        walk(one, part, depth, out);
    }
}

fn parse(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// docx / docm：Word 的内容控件（这一族只有这一支有）
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with("word/") && one.ends_with(".xml") && !one.ends_with(".rels"))
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut parts_with = 0usize;
    let mut controls = 0usize;
    let mut nested = 0usize;
    let mut pr_missing = 0usize;
    let mut pr_empty = 0usize;
    let mut type_none = 0usize;
    let mut endpr = 0usize;
    let mut contents = 0usize;
    let mut binds = 0usize;
    let mut lists = 0usize;
    let mut sums = [0usize; 7];
    let mut children = serde_json::Map::new();
    let mut types = serde_json::Map::new();
    let mut galleries = serde_json::Map::new();
    let mut locks = serde_json::Map::new();
    for name in names.iter() {
        let root = match parse(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        if root.descendants("sdt").is_empty() {
            continue;
        }
        parts_with += 1;
        let mut mine: Vec<Value> = Vec::new();
        walk(&root, name.as_str(), 0, &mut mine);
        for row in mine.iter() {
            controls += 1;
            nested += usize::from(row["depth"].as_u64().unwrap_or(0) > 0);
            pr_missing += 1 - said(row, "pr_present");
            let kids = row["pr_children"].as_array().cloned().unwrap_or_default();
            pr_empty += said(row, "pr_present") * usize::from(kids.is_empty());
            type_none += usize::from(row["type_seen"].is_null());
            endpr += said(row, "endpr_present");
            contents += said(row, "content_present");
            binds += usize::from(!row["data_binding"].is_null());
            lists += row["list_items"].as_u64().unwrap_or(0) as usize;
            for (at, key) in SUM_KEYS.iter().enumerate() {
                sums[at] += row[*key].as_u64().unwrap_or(0) as usize;
            }
            for kid in kids.iter() {
                bump(&mut children, kid.as_str().unwrap_or(""));
            }
            let kind = match row["type_seen"].as_str() {
                Some(one) => one.to_string(),
                None => "(没有类型元素)".to_string(),
            };
            bump(&mut types, kind.as_str());
            if let Some(gal) = row["gallery"].as_str() {
                if !gal.is_empty() {
                    bump(&mut galleries, gal);
                }
            }
            if said(row, "lock_present") == 1 {
                let key = match row["lock"].as_str() {
                    Some(one) => one.to_string(),
                    None => "(没写 val)".to_string(),
                };
                bump(&mut locks, key.as_str());
            }
            if rows.len() < limit {
                rows.push(row.clone());
            }
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "parts_with_controls": parts_with,
        "controls": controls,
        "nested": nested,
        "pr_missing": pr_missing,
        "pr_empty": pr_empty,
        "type_none": type_none,
        "endpr_present": endpr,
        "content_present": contents,
        "data_binding": binds,
        "list_items": lists,
        "paras_direct": sums[0],
        "paras_total": sums[1],
        "tables_direct": sums[2],
        "tables_total": sums[3],
        "cells": sums[4],
        "runs": sums[5],
        "chars": sums[6],
        "children": Value::Object(children),
        "types": Value::Object(types),
        "galleries": Value::Object(galleries),
        "locks": Value::Object(locks),
        "rows": rows,
        "listed": controls.min(limit),
        "cut": controls > limit,
    })
}
