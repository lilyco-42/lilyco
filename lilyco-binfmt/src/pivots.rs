//! 「这张表上挂着数据透视表吗、哪几枚字段摆在哪儿、缓存是谁的」——两族两份账
//!
//! 量到的形状（LibreOffice 把同一份内容各存一次，`.ods` 与 `.xlsx`）：
//!
//! * OOXML 摊成**四类部件**：表自己的 `xl/pivotTables/pivotTable1.xml`、缓存定义
//!   `xl/pivotCache/pivotCacheDefinition1.xml`、缓存正文 `xl/pivotCache/pivotCacheRecords1.xml`，
//!   再加 `xl/workbook.xml` 里那一条 `<pivotCache cacheId="1" r:id="rId5"/>`。表归属到哪张工作表
//!   **不在表自己身上**，在那张表的关系表里（两条 `pivotTable` 类型的关系）；表跳到缓存靠
//!   `cacheId` 对上工作簿那一条，而不是靠表自己的关系（那一条只指缓存定义部件）。
//! * ODF 全收在 `content.xml` 的一棵 `<table:data-pilot-tables>` 里：表、字段、层各自一层元素，
//!   字段名直接写在 `table:source-field-name` 上，不用回头查缓存 —— 因为**根本没有缓存部件**。
//!   而且 LibreOffice 这一份**没写** `table:source-range-address`：数据源在哪只存在于运行时的
//!   属性里，落进文件的只有落点与按钮。这一条如实交 null，不拿 OOXML 那一份补过来。
//!
//! 两族能对齐的只有「这枚字段摆在哪个区」这一问：OOXML 写 `axis="axisRow"`，ODF 写
//! `table:orientation="row"`，所以 `placement` 那一格统一成 row / column / page / data / hidden
//! 五个词（这是**词汇**的对齐，不是数值的换算）；OOXML 里没写 axis 又没有 `dataField` 的字段算
//! hidden，那是该族自己的表达方式。其余各交各的：`cache_id`、`location`、`axis_field_indexes`
//! 只在 OOXML 出现，`buttons`、`source_range_address`、`data_layout_placement` 只在 ODF 出现，
//! 另一族那个键**整个不在场**，由 probe 那侧当反面凭据核对。

use crate::office_doc::{kept_attrs, local_attrs};
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// 属性表里那个「文件写的名字」：这一族的 `local_attrs` 留着局部名，查值按局部名查
fn kept_string(written: &Value, want: &str) -> Option<String> {
    written
        .get(want)
        .and_then(|had| had.as_str())
        .map(String::from)
}

/// 关系表里的 `Target` 按 OOXML 的规矩落成部件名：`../pivotTables/x.xml` 从**关系表所在
/// 目录的上一层**算起，`/xl/...` 那种以斜杠开头的直接从包根算起
fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(rest) = target.strip_prefix('/') {
        return rest.to_string();
    }
    let mut parts: Vec<&str> = base_dir
        .split('/')
        .filter(|one| !one.is_empty() && *one != ".")
        .collect();
    for bit in target.split('/') {
        match bit {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// 一个部件自己的关系表在哪：`xl/worksheets/sheet1.xml` → `xl/worksheets/_rels/sheet1.xml.rels`
fn rels_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// 关系表里挑出某一类型的目标，按文件里的先后交部件名
fn rel_targets(bytes: &[u8], part: &str, type_tail: &str) -> Vec<String> {
    let holder = match zipread::member(bytes, &rels_of(part), DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return Vec::new(),
    };
    let root = xmlscan::parse_str(&holder.as_text());
    let dir = part.rsplit_once('/').map(|(one, _)| one).unwrap_or("");
    root.descendants("Relationship")
        .into_iter()
        .filter(|one| {
            one.attr("Type").unwrap_or_default().ends_with(type_tail)
                && one.attr("Type").unwrap_or_default().contains("pivot")
        })
        .map(|one| resolve(dir, one.attr("Target").unwrap_or_default()))
        .collect()
}

/// OOXML 那五个 axis 词落到共用的五个区名；没写 axis 又不带 `dataField` 才算 hidden
fn ooxml_placement(field: &xmlscan::Node) -> &'static str {
    match field.attr_local("axis").unwrap_or_default() {
        "axisRow" => "row",
        "axisCol" => "column",
        "axisPage" => "page",
        _ => {
            if field.attr_local("dataField").is_some() {
                "data"
            } else {
                "hidden"
            }
        }
    }
}

/// 摆位计数：哪一区摆了几枚字段，只有写过的区才占位
fn tally(values: &[&str]) -> Value {
    let mut out = Map::new();
    for one in values {
        let mine = out.get(*one).and_then(Value::as_u64).unwrap_or(0);
        out.insert((*one).to_string(), json!(mine + 1));
    }
    Value::Object(out)
}

/// 缓存那一条 `cacheField`：名字与声明值按原样，随身的类别清单只数不译
fn cache_field(one: &xmlscan::Node) -> Value {
    let written = local_attrs(one);
    let holder = one.child("sharedItems");
    let kinds = match holder {
        Some(had) => {
            let names: Vec<&str> = had.children.iter().map(|kid| kid.local()).collect();
            tally(&names)
        }
        None => Value::Null,
    };
    json!({
        "name": kept_string(&written, "name"),
        "written": written,
        "shared_written": holder.map(local_attrs).unwrap_or(Value::Null),
        "shared_item_kinds": kinds,
    })
}

/// 缓存定义那一跳：数据源、字段名单、记录部件，各按文件写的原样
fn cache_ledger(bytes: &[u8], part: &str) -> Value {
    let holder = match zipread::member(bytes, part, DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return json!({"part": part, "present": false}),
    };
    let root = xmlscan::parse_str(&holder.as_text());
    let def = match root.descendants("pivotCacheDefinition").first() {
        Some(one) => *one,
        None => return json!({"part": part, "present": true, "written": Value::Null}),
    };
    let source = def.child("cacheSource");
    let fields: Vec<Value> = match def.child("cacheFields") {
        Some(had) => had.all("cacheField").into_iter().map(cache_field).collect(),
        None => Vec::new(),
    };
    let names: Vec<String> = fields
        .iter()
        .map(|one| one["name"].as_str().unwrap_or_default().to_string())
        .collect();
    // 记录正文那一条部件在这份定义的关系表里，声明的行数与实数的 `<r>` 各交一份
    let records_part = rel_targets(bytes, part, "pivotCacheRecords")
        .into_iter()
        .next();
    let mut records_declared: Option<String> = None;
    let mut records_rows: Option<usize> = None;
    if let Some(path) = &records_part {
        if let Ok(one) = zipread::member(bytes, path, DEFAULT_MEMBER_CAP) {
            let records_root = xmlscan::parse_str(&one.as_text());
            if let Some(found) = records_root.descendants("pivotCacheRecords").first() {
                records_declared = found.attr_local("count").map(String::from);
                records_rows = Some(found.all("r").len());
            }
        }
    }
    json!({
        "part": part,
        "present": true,
        "written": local_attrs(def),
        "source_type": source
            .and_then(|had| had.attr_local("type"))
            .map(String::from),
        "worksheet_source": source
            .and_then(|had| had.child("worksheetSource"))
            .map(local_attrs)
            .unwrap_or(Value::Null),
        "declared_field_total": def
            .child("cacheFields")
            .and_then(|had| had.attr_local("count"))
            .map(String::from),
        "fields": fields,
        "field_names": names,
        "records_part": records_part,
        "records_declared": records_declared,
        "records_rows": records_rows,
    })
}

/// 一条轴上的字段序号按原样交：`<field x="…">` 那种写法（`x="-2"` 是「Data」那枚假字段）
fn axis_xs(holder: Option<&xmlscan::Node>) -> Vec<i64> {
    let mut out = Vec::new();
    if let Some(had) = holder {
        for one in had.all("field") {
            if let Ok(raw) = one.attr_local("x").unwrap_or_default().parse::<i64>() {
                out.push(raw);
            }
        }
    }
    out
}

/// 页轴那一族用的是另一个元素与另一个属性名（`<pageField fld="…">`），所以单独走一遍
fn axis_flds(holder: Option<&xmlscan::Node>) -> Vec<i64> {
    let mut out = Vec::new();
    if let Some(had) = holder {
        for one in had.all("pageField") {
            if let Ok(raw) = one.attr_local("fld").unwrap_or_default().parse::<i64>() {
                out.push(raw);
            }
        }
    }
    out
}

/// 名单去重后计数：同一个名字在两枚表上各出现一次，只算一个
fn dedup(items: &[String]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for one in items {
        if !seen.contains(&one.as_str()) {
            seen.push(one);
        }
    }
    seen.len()
}

/// OOXML 那一份：四类部件拼回「哪张表上挂着哪一枚透视表」
pub(crate) fn xlsx(bytes: &[u8], limit: usize) -> Value {
    let workbook = match zipread::member(bytes, "xl/workbook.xml", DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return json!({"available": false}),
    };
    let root = xmlscan::parse_str(&workbook.as_text());
    // 工作簿的顺序是真相：r:id → 部件路径，靠 workbook.xml.rels 对上
    let mut by_id: Vec<(String, String)> = Vec::new();
    if let Ok(rels) = zipread::member(bytes, "xl/_rels/workbook.xml.rels", DEFAULT_MEMBER_CAP) {
        let rel_root = xmlscan::parse_str(&rels.as_text());
        for one in rel_root.descendants("Relationship") {
            let id = one.attr("Id").unwrap_or_default().to_string();
            let target = one.attr("Target").unwrap_or_default();
            by_id.push((id, resolve("xl", target)));
        }
    }
    let mut sheets: Vec<(String, String)> = Vec::new();
    for (index, one) in root.descendants("sheet").iter().enumerate() {
        let name = one.attr("name").unwrap_or_default().to_string();
        let rid = one.attr_local("id").unwrap_or_default().to_string();
        let part = by_id
            .iter()
            .find(|(one_id, _)| *one_id == rid)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", index + 1));
        sheets.push((name, part));
    }
    // 工作簿那一条 `<pivotCache cacheId="1" r:id="rId5"/>` 是 cacheId → 缓存部件的唯一对应
    let mut caches: Vec<(String, String)> = Vec::new();
    for one in root.descendants("pivotCache") {
        let id = one.attr_local("cacheId").unwrap_or_default().to_string();
        let rid = one.attr_local("id").unwrap_or_default().to_string();
        if let Some((_, path)) = by_id.iter().find(|(one_id, _)| *one_id == rid) {
            caches.push((id, path.clone()));
        }
    }
    // 表归属靠那张表的关系表：`xl/worksheets/_rels/sheet1.xml.rels` 里两条 pivotTable 类型
    let mut owner: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (name, part) in &sheets {
        for target in rel_targets(bytes, part, "pivotTable") {
            owner.insert(target, (name.clone(), part.clone()));
        }
    }
    let mut paths: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| {
            one.starts_with("xl/pivotTables/") && one.ends_with(".xml") && !one.contains("_rels")
        })
        .collect();
    paths.sort();
    let mut entries: Vec<Value> = Vec::new();
    let mut placements: Vec<&str> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut owners: Vec<String> = Vec::new();
    for path in &paths {
        let holder = match zipread::member(bytes, path, DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(_) => continue,
        };
        let table_root = xmlscan::parse_str(&holder.as_text());
        let def = match table_root.descendants("pivotTableDefinition").first() {
            Some(one) => *one,
            None => continue,
        };
        let written = local_attrs(def);
        let name = kept_string(&written, "name");
        if let Some(one) = &name {
            names.push(one.clone());
        }
        let cache_id = def.attr_local("cacheId").map(String::from);
        let cache_part = cache_id
            .as_deref()
            .and_then(|want| caches.iter().find(|(one_id, _)| one_id == want))
            .map(|(_, path)| path.clone());
        let cache = match &cache_part {
            Some(part) => cache_ledger(bytes, part),
            None => Value::Null,
        };
        let field_names: Vec<String> = cache["field_names"]
            .as_array()
            .map(|had| {
                had.iter()
                    .map(|one| one.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut fields: Vec<Value> = Vec::new();
        let mut mine: Vec<&str> = Vec::new();
        if let Some(holder) = def.child("pivotFields") {
            for (index, one) in holder.all("pivotField").into_iter().enumerate() {
                let placement = ooxml_placement(one);
                mine.push(placement);
                placements.push(placement);
                fields.push(json!({
                    "index": index,
                    "name": field_names.get(index).cloned(),
                    "placement": placement,
                    "written": local_attrs(one),
                    "item_total": one
                        .child("items")
                        .and_then(|had| had.attr_local("count"))
                        .map(String::from),
                }));
            }
        }
        let measures: Vec<Value> = match def.child("dataFields") {
            Some(had) => had
                .all("dataField")
                .into_iter()
                .map(|one| {
                    let mine = local_attrs(one);
                    json!({
                        "name": kept_string(&mine, "name"),
                        "written": mine,
                    })
                })
                .collect(),
            None => Vec::new(),
        };
        let (owner_sheet, owner_part) = owner
            .get(path.as_str())
            .cloned()
            .unwrap_or((String::new(), String::new()));
        if !owner_sheet.is_empty() {
            owners.push(owner_sheet.clone());
        }
        entries.push(json!({
            "part": path,
            "sheet": if owner_sheet.is_empty() { Value::Null } else { json!(owner_sheet) },
            "sheet_part": if owner_part.is_empty() { Value::Null } else { json!(owner_part) },
            "name": name,
            "written": written,
            "cache_id": cache_id,
            "location": def.child("location").map(local_attrs).unwrap_or(Value::Null),
            "declared_field_total": def
                .child("pivotFields")
                .and_then(|had| had.attr_local("count"))
                .map(String::from),
            "fields": fields,
            "axes_counts": tally(&mine),
            "axis_field_indexes": json!({
                "row": axis_xs(def.child("rowFields")),
                "column": axis_xs(def.child("colFields")),
                "page": axis_flds(def.child("pageFields")),
            }),
            "measures": measures,
            "cache": cache,
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "total": entries.len(),
        "with_location": entries
            .iter()
            .filter(|one| !one["location"].is_null())
            .count(),
        "distinct_names": dedup(&names),
        "sheets_owning": dedup(&owners),
        "caches_written": caches.len(),
        "axes_counts": tally(&placements),
        "entries": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：全收在 `content.xml` 那一棵 `<table:data-pilot-tables>` 里
pub(crate) fn ods(bytes: &[u8], limit: usize) -> Value {
    let holder = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return json!({"available": false}),
    };
    let root = xmlscan::parse_str(&holder.as_text());
    let mut entries: Vec<Value> = Vec::new();
    let mut placements: Vec<&str> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut owners: Vec<String> = Vec::new();
    let mut with_source = 0usize;
    for one in root.descendants("data-pilot-table") {
        let written = kept_attrs(one);
        let name = one.attr_local("name").map(String::from);
        if let Some(had) = &name {
            names.push(had.clone());
        }
        let target = one.attr_local("target-range-address").map(String::from);
        let source = one.attr_local("source-range-address").map(String::from);
        if source.is_some() {
            with_source += 1;
        }
        // 归属只能从落点那条地址的前半截拿：这一族没有「表 → 透视表」那种指针
        let owner = target
            .as_deref()
            .and_then(|raw| raw.split('.').next())
            .map(|raw| raw.split(':').next().unwrap_or(raw).to_string())
            .unwrap_or_default();
        if !owner.is_empty() {
            owners.push(owner.clone());
        }
        let buttons: Vec<String> = one
            .attr_local("buttons")
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from)
            .collect();
        let mut fields: Vec<Value> = Vec::new();
        let mut mine: Vec<&str> = Vec::new();
        let mut layout_placement: Option<String> = None;
        for (index, field) in one.all("data-pilot-field").into_iter().enumerate() {
            let placement = field.attr_local("orientation").unwrap_or("hidden");
            mine.push(placement);
            placements.push(placement);
            let layout = field.attr_local("is-data-layout-field") == Some("true");
            if layout {
                layout_placement = Some(placement.to_string());
            }
            fields.push(json!({
                "index": index,
                "name": field.attr_local("source-field-name").map(String::from),
                "placement": placement,
                "data_layout_field": layout,
                "written": kept_attrs(field),
            }));
        }
        let measures: Vec<Value> = fields
            .iter()
            .filter(|had| had["placement"].as_str() == Some("data"))
            .map(|had| json!({"name": had["name"].clone(), "written": had["written"].clone()}))
            .collect();
        entries.push(json!({
            "part": "content.xml",
            "sheet": if owner.is_empty() { Value::Null } else { json!(owner) },
            "name": name,
            "written": written,
            "target_range_address": target,
            "source_range_address": source,
            "buttons": buttons,
            "fields": fields,
            "axes_counts": tally(&mine),
            "data_layout_placement": layout_placement,
            "measures": measures,
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "total": entries.len(),
        "with_source_range": with_source,
        "distinct_names": dedup(&names),
        "sheets_owning": dedup(&owners),
        "axes_counts": tally(&placements),
        "entries": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
