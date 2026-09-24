//! ODF 那一族的图是**嵌入对象**：宿主里 `draw:frame` 之内一条 `draw:object`，它的
//! `xlink:href` 指着 `Object N`（一个目录），那个目录自己的 `content.xml` 才写着图。
//!
//! 与 OOXML 那两家不一样的三件事，都在这里：地址写第三样（`数据.B2:数据.B3` ——
//! 点分隔、不带 `$`、不引号），图的类型不写在 `chart:chart` 上而是写在**每条**
//! `chart:series` 上，点数另有一条自报的 `chart:data-point@chart:repeated`
//! （「这一条顶几个点」，与 `table:number-columns-repeated` 同一个惯例）。
//! 一切按文件写的交：`written` 是那一个元素上写着的属性，一个也不解释。

use crate::xmlscan;
use crate::zipread;
use serde_json::{json, Value};

/// 一个元素上写着的属性（局部名 → 原值）。ODF 的属性都带前缀，且同一局部名可能来自
/// 两个命名空间（`calcext:value-type` 那种），所以 documentfoundation 的副本一律不看
pub(crate) fn attrs_of(node: &xmlscan::Node) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in &node.attrs {
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        if key.contains(":") && key.starts_with("calcext") {
            continue;
        }
        out.insert(local, json!(value));
    }
    Value::Object(out)
}

pub(crate) fn attr_of<'a>(node: &'a xmlscan::Node, want: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(key, _)| {
            key.rsplit(':').next().unwrap_or(key) == want && !key.starts_with("calcext")
        })
        .map(|(_, value)| value.as_str())
}

fn text_of(node: &xmlscan::Node) -> Option<String> {
    let joined = node
        .descendants("p")
        .iter()
        .map(|one| {
            one.text()
                .split_whitespace()
                .collect::<Vec<&str>>()
                .join(" ")
        })
        .collect::<Vec<String>>()
        .into_iter()
        .filter(|one| !one.is_empty())
        .collect::<Vec<String>>()
        .join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn repeated(node: &xmlscan::Node, want: &str) -> usize {
    attr_of(node, want)
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

/// `Object N` 那一个目录里的图。读不到那个部件就说「不在」，不猜
fn chart_object(bytes: &[u8], folder: &str) -> Value {
    let part = format!("{folder}/content.xml");
    let Ok(member) = zipread::member(bytes, &part, zipread::DEFAULT_MEMBER_CAP) else {
        return json!({"object": folder, "present": false});
    };
    let root = xmlscan::parse_str(&member.as_text());
    let class = root
        .descendants("chart")
        .first()
        .and_then(|one| attr_of(one, "class"))
        .map(String::from);
    let title = root
        .descendants("title")
        .first()
        .and_then(|one| text_of(one));
    let mut series: Vec<Value> = Vec::new();
    for one in root.descendants("series") {
        let points: Vec<&xmlscan::Node> = one
            .children
            .iter()
            .filter(|had| had.local() == "data-point")
            .collect();
        let stated: usize = points.iter().map(|had| repeated(had, "repeated")).sum();
        series.push(json!({
            "class": attr_of(one, "class"),
            "values": attr_of(one, "values-cell-range-address"),
            "label": attr_of(one, "label-cell-address"),
            "point_elements": points.len(),
            "points_written": if points.is_empty() { Value::Null } else { json!(stated) },
            "written": attrs_of(one),
        }));
    }
    let categories = root.descendants("categories").first().map(|one| {
        json!({
            "address": attr_of(one, "cell-range-address"),
            "point_elements": one
                .children
                .iter()
                .filter(|had| had.local() == "data-point")
                .count(),
            "written": attrs_of(one),
        })
    });
    // 图自带的那张 `local-table`：缓存下来的字与数，与 OOXML 的 numCache 是同一件事
    let mut cached: Vec<Value> = Vec::new();
    for table in root
        .descendants("table")
        .iter()
        .filter(|one| attr_of(one, "name") == Some("local-table"))
    {
        for row in table.descendants("table-row") {
            let mut line: Vec<Value> = Vec::new();
            for cell in row
                .children
                .iter()
                .filter(|had| matches!(had.local(), "table-cell" | "header-cell"))
            {
                // `number-columns-repeated` 不铺开：这一格是「抄下来的表」，铺开会让一个
                // 自报 16384 的文件凭空长出上万格；自报的数原样一起交
                line.push(json!({
                    "repeated": repeated(cell, "number-columns-repeated"),
                    "text": text_of(cell),
                    "value": attr_of(cell, "value"),
                    "written": attrs_of(cell),
                }));
            }
            cached.push(json!({"cells": line}));
        }
    }
    json!({
        "object": folder,
        "present": true,
        "class": class,
        "title": title,
        "series": series.len(),
        "series_list": series,
        "categories": categories,
        "local_table": cached,
    })
}

/// 这个宿主（一张 ODS 表或一页 ODP）里的图：`draw:frame` → `draw:object@xlink:href`
pub(crate) fn charts_in(bytes: &[u8], host: &xmlscan::Node) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for frame in host.descendants("frame") {
        for one in &frame.children {
            if one.local() != "object" {
                continue;
            }
            let Some(raw) = attr_of(one, "href") else {
                continue;
            };
            let folder = raw
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_string();
            let mut report = chart_object(bytes, &folder);
            report["frame"] = json!(attr_of(frame, "name"));
            report["preview"] = json!(frame.children.iter().any(|kid| {
                kid.local() == "image"
                    && attr_of(kid, "href")
                        .map(|had| had.starts_with("./ObjectReplacements/"))
                        .unwrap_or(false)
            }));
            out.push(report);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn blob(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("fixture 应在")
    }

    /// 期望值逐字来自 python 侧的镜像 `odf_chart_object()` / `odf_charts_of()`
    #[test]
    fn ods_charts_are_reached_through_the_embedded_object() {
        let bytes = blob("chart.ods");
        let member = zipread::member(&bytes, "content.xml", zipread::DEFAULT_MEMBER_CAP).unwrap();
        let root = xmlscan::parse_str(&member.as_text());
        let tables = root.descendants("table");
        let names = tables
            .iter()
            .map(|one| attr_of(one, "name").unwrap_or_default())
            .collect::<Vec<&str>>();
        assert_eq!(names, ["数据", "无图"], "{names:?}");
        let charts = charts_in(&bytes, tables[0]);
        assert_eq!(charts.len(), 2, "{charts:?}");
        let bar = &charts[0];
        assert_eq!(bar["object"], "Object 1", "{bar}");
        assert_eq!(bar["frame"], "Chart 1");
        assert_eq!(bar["present"], json!(true));
        assert_eq!(bar["preview"], json!(true), "LO 还写了一份预览图");
        assert_eq!(
            bar["class"],
            Value::Null,
            "ODF 的类型不写在 chart:chart 上，写在每条 series 上"
        );
        assert_eq!(bar["title"], "逐月收支", "{bar}");
        assert_eq!(bar["series"], 2);
        let first = &bar["series_list"][0];
        assert_eq!(first["class"], "chart:bar", "{first}");
        assert_eq!(
            first["values"], "数据.B2:数据.B3",
            "第三种地址写法：{first}"
        );
        assert_eq!(first["label"], "数据.B1:数据.B1");
        assert_eq!(first["point_elements"], 1, "文件只写一条 data-point");
        assert_eq!(first["points_written"], 2, "repeated 说这一条顶两个点");
        assert_eq!(first["written"]["style-name"], "ch8", "属性原样交：{first}");
        let cats = &bar["categories"];
        assert_eq!(cats["address"], "数据.A2:数据.A3", "{cats}");
        let rows = bar["local_table"].as_array().expect("是数组");
        assert_eq!(rows.len(), 3, "{bar}");
        assert_eq!(
            rows[1]["cells"]
                .as_array()
                .expect("是数组")
                .iter()
                .map(|one| one["text"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>(),
            ["一月", "10", "4"]
        );
        assert_eq!(
            rows[2]["cells"]
                .as_array()
                .expect("是数组")
                .iter()
                .map(|one| one["value"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>(),
            ["", "25", "9"]
        );
        let line = &charts[1];
        assert_eq!(line["object"], "Object 2", "{line}");
        assert_eq!(line["series_list"][0]["class"], "chart:line", "{line}");
        assert_eq!(line["title"], "收入折线");
        assert!(
            charts_in(&bytes, tables[1]).is_empty(),
            "没挂图的那张表交空表"
        );
    }
}
