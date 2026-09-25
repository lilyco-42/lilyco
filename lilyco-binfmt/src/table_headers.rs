//! 「这张表的哪几行每页重复」——同一问在两家写在两个不同的地方
//!
//! OOXML 把它放在**行上**：`w:trPr/w:tblHeader` 是一个没有值的元素，在场就是重复。
//! ODF 把它放在**表身上**：`table:header-rows`（几行算表头）配
//! `table:header-rows-repeated`（每页重复几行），是两个数。形状不同到不折算，所以各交各的。
//!
//! 实测最要紧的一条（同一份稿子三种存法）：LibreOffice 重写这份 docx 时，把**不是从第一行
//! 起**的那一枚标记整个丢掉（原件 4 行标了、重写只剩 3 行），而它给每一行都补一个**空的**
//! `w:trPr`（原件 4 个、重写 12 个）；转成 odt 时那两个属性**一个都不写** —— 所以 ODF 那边
//! 交 null（这份文件没说），不交 0（那才是「说了不重复」）。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// OOXML 那一份：逐表交「哪几行写了 `w:tblHeader`」
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut tables: Vec<Value> = Vec::new();
    for (index, table) in body.descendants("tbl").iter().enumerate() {
        // 行只数这张表自己的直接孩子（套在格子里的另一张表不算，与那张网同一条口径）
        let rows: Vec<&Node> = table
            .children
            .iter()
            .filter(|kid| kid.local() == "tr")
            .collect();
        let mut marks: Vec<bool> = Vec::new();
        let mut tr_pr_elements = 0usize;
        for row in &rows {
            // `trPr` 的枚数单独交：这一族**允许**同一行写两枚，而读者只认第一枚的内容
            tr_pr_elements += row
                .children
                .iter()
                .filter(|kid| kid.local() == "trPr")
                .count();
            let holder = row.children.iter().find(|kid| kid.local() == "trPr");
            marks.push(holder.map_or(false, |one| {
                one.children.iter().any(|kid| kid.local() == "tblHeader")
            }));
        }
        let leading = marks.first().copied().unwrap_or(false);
        let contiguous = leading && marks.windows(2).all(|pair| pair[0] || !pair[1]);
        let header_count = marks.iter().filter(|one| **one).count();
        tables.push(json!({
            "index": index,
            "rows": rows.len(),
            "header_rows": marks,
            "header_count": header_count,
            "tr_pr_elements": tr_pr_elements,
            "contiguous_from_first": contiguous,
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "tables_total": tables.len(),
        "marked": tables.iter().filter(|one| one["header_count"].as_u64().unwrap_or(0) > 0).count(),
        "header_rows_total": tables.iter().map(|one| one["header_count"].as_u64().unwrap_or(0)).sum::<u64>(),
        "non_leading": tables.iter().filter(|one| {
            one["header_count"].as_u64().unwrap_or(0) > 0
                && one["header_rows"].as_array().and_then(|had| had.first().and_then(Value::as_bool)) != Some(true)
        }).count(),
        "tables": tables.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：表身上那四个属性按写的交（没写是 null，不是 0）
pub(crate) fn odf(root: &Node, limit: usize) -> Value {
    let mut tables: Vec<Value> = Vec::new();
    for (index, one) in root.descendants("table").iter().enumerate() {
        tables.push(json!({
            "index": index,
            "name": one.attr_local("name"),
            "header_rows": one.attr_local("header-rows"),
            "header_rows_repeated": one.attr_local("header-rows-repeated"),
            "header_column": one.attr_local("header-column"),
            "header_columns_repeated": one.attr_local("header-columns-repeated"),
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "tables_total": tables.len(),
        "with_header_rows": tables.iter().filter(|one| !one["header_rows"].is_null()).count(),
        "with_repeated": tables.iter().filter(|one| !one["header_rows_repeated"].is_null()).count(),
        "tables": tables.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
