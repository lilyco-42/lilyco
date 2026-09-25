//! 「这一节的页码怎么写」——OOXML 把它放在每一节的 `w:sectPr/w:pgNumType` 上（一枚元素，
//! 三个可以各自缺的属性），ODF 放在页版式的 `style:page-layout-properties` 上（两种词汇、
//! 而且**不是一节一条**）
//!
//! OOXML：`w:pgNumType` 可以有 `w:fmt`（`decimal` / `upperRoman` …）、`w:start`（从几开始）、
//! `w:chpNum`（跟着章节号编）。三者都可以缺，缺就交 null —— 这一族的元素在场而属性为空，
//! 与「这一节根本没有这个元素」是两件事（实测 LibreOffice 的 docx 导出**只写 `fmt`**：
//! `w:pgNumType` 在、`w:start` 没有，即使源件明写了从第 7 页起）。
//!
//! ODF：同一件事写在页版式上而不是节上 —— `style:num-format`（`1` / `i` / `I` / `a` / `A` /
//! `none` 这一族的字母表）与 `style:page-number`（从几开始）。它天然是「一份版式一条」，
//! 与 OOXML 的「一节一条」不是一套计数，所以两边各自交总数与找得到的条数，不做等号。
//!
//! 实测五份件（`restart.docx` 由 python-docx 自己的 XML 层往 `w:sectPr` 挂一枚三个属性全写的
//! `w:pgNumType`；`pnum.odt` 由 zipfile 写：段落属性上 `fo:break-before="page"` +
//! `style:page-number="7"` + `style:use-page-numbering="true"`，页版式上 `num-format="1"` +
//! `page-number="1"`，两份母版页共用那一份版式）：
//! 1. LibreOffice 把 `restart.docx` 同格式重写一份，`w:start="7"` 与 `w:fmt="upperRoman"`
//!    都活着，而 `w:chpNum` 整格没了 —— 三个属性不是同一个待遇；
//! 2. 同一份 docx 转成 odt，页版式上只剩 `style:num-format="I"`（这一族把大写罗马写成一个
//!    字母），而「从 7 开始」在 ODF 侧一个字都没落（`with_page_number` 因此是 0，不是「从 1 开始」）；
//! 3. 反方向：`pnum.odt` 段落上明写的「从第 7 页起」转成 docx 后，`w:pgNumType` 在、只带
//!    `fmt="decimal"`，`w:start` 是 null（不是 0、也不是 7），而源件那个「另起一页」也没换出
//!    第二节（`sections_total` 是 1）—— 跨族走一趟，这一问两头各丢一次；
//! 4. `pnum.odt` 里「几条页版式」与「几份母版页」是两个数（1 与 2），所以两边分开交，不拿一个顶另一个；
//! 5. LibreOffice 转出来的那份 odt 另外写了一个 `style:default-page-layout`，它的
//!    `page-layout-properties` 只带网格设置 —— 这一条账走 `style:page-layout`，那一格既不报
//!    也不编号（与「那张纸」那条同一规矩：只带网格的那一条不是一张纸）。
//!
//! RTF 那一族的页码写在节属性那一格里（`\pgndec` 是十进制、`\pgnstart` 才是从几开始）。
//! 实测这批件里 LibreOffice 只写了 `\pgndec`：一份一次，而两份件的 `notes-hf.rtf` 与
//! `paper-a4.rtf` 各两次 —— 正好一节一次；`\pgnstart` 一个都没有。这一支读者的 sections
//! 本来就是 null（节归属判不住，同「那张纸」只交文档级那一条的先例），所以这一问不交键；
//! 遗留 .doc 也一样不交 —— 它的节属性住在 table stream 里，这一族读者不走那里。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// ODF 那一份要收的属性（按局部名收，交出去时把前缀去掉 —— 与读者那边同一条）
const PAGE_NUMBER_LOCALS: [&str; 5] = [
    "num-format",
    "page-number",
    "use-page-numbering",
    "num-prefix",
    "num-suffix",
];

/// OOXML 那一份：一节一条，`w:pgNumType` 在不在、写了哪几个属性
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut sections = 0usize;
    let mut with_element = 0usize;
    let mut start_written = 0usize;
    let mut fmts: Vec<String> = Vec::new();
    for (index, sect) in body.descendants("sectPr").into_iter().enumerate() {
        sections += 1;
        let holder = sect.children.iter().find(|kid| kid.local() == "pgNumType");
        let mut written = serde_json::Map::new();
        if let Some(had) = holder {
            with_element += 1;
            for (key, value) in had.attrs.iter() {
                let local = key.rsplit(':').next().unwrap_or(key).to_string();
                written.insert(local.clone(), json!(value));
                if local == "start" {
                    start_written += 1;
                }
                if local == "fmt" && !fmts.iter().any(|had| *had == *value) {
                    fmts.push(value.clone());
                }
            }
        }
        rows.push(json!({
            "section": index,
            "element_present": holder.is_some(),
            "start_written": holder.and_then(|had| had.attr_local("start")).map(String::from),
            "fmt_written": holder.and_then(|had| had.attr_local("fmt")).map(String::from),
            "chpnum_written": holder.and_then(|had| had.attr_local("chpNum")).map(String::from),
            "written": Value::Object(written),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "sections_total": sections,
        "with_element": with_element,
        "start_written_total": start_written,
        "distinct_fmts": fmts,
        "sections": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：一页版式一条，两份件都走（版式通常住在 styles.xml，但不赌）
pub(crate) fn odf(content: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut layouts = 0usize;
    let mut with_format = 0usize;
    let mut with_start = 0usize;
    let mut formats: Vec<String> = Vec::new();
    let mut masters = 0usize;
    let mut roots: Vec<(&str, &Node)> = vec![("content.xml", content)];
    if let Some(extra) = styles {
        roots.push(("styles.xml", extra));
    }
    for (part, root) in roots.iter() {
        for one in root.descendants("page-layout") {
            let name = one.attr_local("name").map(String::from);
            let holder = one.descendants("page-layout-properties").into_iter().next();
            let Some(had) = holder else {
                layouts += 1;
                rows.push(json!({
                    "part": part,
                    "layout_name": name.clone(),
                    "element_present": false,
                    "num_format_written": Value::Null,
                    "page_number_written": Value::Null,
                    "written": {},
                }));
                continue;
            };
            layouts += 1;
            let mut written = serde_json::Map::new();
            let mut format: Option<String> = None;
            let mut start: Option<String> = None;
            for (key, value) in had.attrs.iter() {
                if key == "xmlns" || key.starts_with("xmlns:") {
                    continue;
                }
                let local = key.rsplit(':').next().unwrap_or(key);
                if !PAGE_NUMBER_LOCALS.contains(&local) {
                    continue;
                }
                written.insert(local.to_string(), json!(value));
                if local == "num-format" {
                    format = Some(value.clone());
                    with_format += 1;
                    if !formats.iter().any(|had| *had == *value) {
                        formats.push(value.clone());
                    }
                }
                if local == "page-number" {
                    start = Some(value.clone());
                    with_start += 1;
                }
            }
            rows.push(json!({
                "part": part,
                "layout_name": name,
                "element_present": true,
                "num_format_written": format,
                "page_number_written": start,
                "written": Value::Object(written),
            }));
        }
        masters += root.descendants("master-page").len();
    }
    json!({
        "family": "odf",
        "available": true,
        "layouts_total": layouts,
        "masters_total": masters,
        "with_num_format": with_format,
        "with_page_number": with_start,
        "distinct_formats": formats,
        "layouts": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
