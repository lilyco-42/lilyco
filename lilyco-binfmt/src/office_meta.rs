//! `lbin office-meta` — 文件属性对话框里那些字段，从文件自己的账上读。
//!
//! 三种记法，来源完全不同，所以结果里也分开写清楚是哪一份账：
//! - **OOXML**：`docProps/core.xml`（都柏林核心 + 核心扩展）、`docProps/app.xml`
//!   （哪个程序写的、写了多久、多少字/页）、`docProps/custom.xml`（用户自己加的属性）；
//! - **ODF**：`meta.xml` 的 `office:meta`（含 `meta:user-defined` 那串自定义属性）；
//! - **复合文档**（.doc/.xls/.ppt）：`\005SummaryInformation` 与
//!   `\005DocumentSummaryInformation` 两条流里的 OLE 属性集，见 [`crate::props`]。
//!
//! 一条原则：属性值一律照文件里写的给，**不换算单位、不补默认值、不猜缺失**。
//! 时间戳是唯一例外：FILETIME 那串 100ns 数附上一个 UTC 时刻读数，并同时给出原始数，
//! 这样「文件说的数」和「人看的时刻」都能对得上（两者的差别见 `notes`）。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, Family};
use crate::props;
use crate::read::read_blob;
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 读办公文件的元数据（T0 只读）
#[derive(App)]
#[app(
    name = "office-meta",
    run = "run_office_meta",
    about = "Read the document properties an office file keeps about itself, from whichever book of record that format actually uses: docProps/core.xml + app.xml + custom.xml for OOXML (docx/xlsx/pptx), office:meta from meta.xml for ODF (odt/ods/odp incl. user-defined fields), and the SummaryInformation / DocumentSummaryInformation OLE property sets for legacy .doc/.xls/.ppt. Returns { path, format, app, source, core, application, custom, legacy, docprops_parts, thumbnail, notes } where core carries title/subject/creator/last-modified-by/keywords/description/category/content-status/language/revision/identifier/version and the two timestamps, application carries the producing program and its counters (Pages, Words, Paragraphs, TotalTime, Template, Company, Manager, Slides, HiddenSlides), legacy entries are keyed by the property name when the format's PID table is unambiguous and by pid when producers disagree (DocSummaryInformation PID 1 is SectionCount in a Word file and CodePage in a LibreOffice one — naming it either way would be a guess). FILETIME values come with both the raw 100ns number and its UTC reading. Read-only (safety T0)."
)]
pub struct OfficeMeta {
    /// 办公文件
    #[arg(about = "Office document to read properties from", must_exist = true)]
    path: PathBuf,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

fn run_office_meta(app: &OfficeMeta, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the document properties".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let mut core = json!({});
    let mut application = json!({});
    let mut custom = json!({});
    let mut legacy = json!({});
    let mut parts: Vec<String> = Vec::new();
    let mut thumbnail = false;
    let mut notes: Vec<String> = Vec::new();
    let source: &'static str;

    match doc.family {
        Family::Ooxml => {
            source = "docProps";
            for name in doc
                .entries
                .iter()
                .map(|one| one.name.clone())
                .filter(|one| one.starts_with("docProps/"))
            {
                parts.push(name.clone());
                if name.starts_with("docProps/thumbnail") {
                    thumbnail = true;
                }
            }
            if let Some(member) = read(bytes, "docProps/core.xml") {
                let root = xmlscan::parse_str(&member.as_text());
                for (tag, key) in CORE_FIELDS {
                    if let Some(one) = root.descendants(tag).into_iter().next() {
                        let text = leaf_text(&one);
                        if !text.is_empty() {
                            core[key] = json!(text);
                        }
                    }
                }
            } else {
                notes.push("没有 docProps/core.xml（这份包没写核心属性）".to_string());
            }
            if let Some(member) = read(bytes, "docProps/app.xml") {
                let root = xmlscan::parse_str(&member.as_text());
                flatten(&root, &mut application, 0);
            } else {
                notes.push("没有 docProps/app.xml（应用属性是可选的）".to_string());
            }
            if let Some(member) = read(bytes, "docProps/custom.xml") {
                let root = xmlscan::parse_str(&member.as_text());
                for one in root.descendants("property") {
                    let name = one.attr("name").unwrap_or_default().to_string();
                    let mut value = Value::Null;
                    for child in &one.children {
                        let kind = child.local();
                        if kind == "lpwstr" || kind == "lpstr" || kind == "wstr" {
                            value = json!(leaf_text(child));
                        } else if kind == "i4" || kind == "ui4" || kind == "i8" {
                            value = json!(leaf_text(child).parse::<i64>().ok());
                        } else if kind == "r4" || kind == "r8" {
                            value = json!(leaf_text(child).parse::<f64>().ok());
                        } else if kind == "bool" {
                            value = json!(leaf_text(child) == "true" || leaf_text(child) == "1");
                        } else if kind == "filetime" {
                            let raw = leaf_text(child);
                            let ticks = raw
                                .trim_start_matches('{')
                                .trim_end_matches('}')
                                .parse::<u64>();
                            value = match ticks {
                                Ok(one) => {
                                    json!({"filetime": one, "utc": props::filetime_iso(one)})
                                }
                                Err(_) => json!(raw),
                            };
                        } else if kind == "vector" || kind == "empty" {
                            value = json!(leaf_text(child));
                        }
                    }
                    custom[name] = value;
                }
            }
        }
        Family::Odf => {
            source = "meta.xml";
            if let Some(member) = read(bytes, "meta.xml") {
                let root = xmlscan::parse_str(&member.as_text());
                let Some(meta) = root.descendants("meta").into_iter().next() else {
                    notes.push("meta.xml 里没有 office:meta".to_string());
                    return finish(
                        app,
                        &doc,
                        source,
                        core,
                        application,
                        custom,
                        legacy,
                        parts,
                        thumbnail,
                        notes,
                        ctx,
                        start,
                    );
                };
                for one in &meta.children {
                    let key = one.local().to_string();
                    let text = leaf_text(one);
                    if text.is_empty() {
                        continue;
                    }
                    match key.as_str() {
                        "title" | "creator" | "initial-creator" | "language" | "subject"
                        | "description" | "keyword" | "creation-date" | "date"
                        | "editing-cycles" | "editing-duration" | "generator"
                        | "document-statistic" => {
                            core[&key] = json!(text);
                        }
                        _ => {
                            if key == "user-defined" {
                                // 属性写作 `meta:name` —— 按精确名找会一个都找不到，
                                // 于是所有自定义属性都并成同一个 "user-defined" 键
                                let name = one
                                    .attr_local("name")
                                    .or_else(|| one.attr_local("href"))
                                    .unwrap_or("user-defined")
                                    .to_string();
                                custom[name] = json!(text);
                            } else {
                                core[&key] = json!(text);
                            }
                        }
                    }
                }
            } else {
                notes.push("包里读不到 meta.xml".to_string());
            }
            parts.push("meta.xml".to_string());
        }
        Family::Compound => {
            source = "ole-property-sets";
            let Some(cfb) = doc.compound.as_ref() else {
                notes.push("复合文档打不开，属性集也就无从读起".to_string());
                return finish(
                    app,
                    &doc,
                    source,
                    core,
                    application,
                    custom,
                    legacy,
                    parts,
                    thumbnail,
                    notes,
                    ctx,
                    start,
                );
            };
            for (stream, key) in [
                ("\u{5}SummaryInformation", "SummaryInformation"),
                (
                    "\u{5}DocumentSummaryInformation",
                    "DocumentSummaryInformation",
                ),
            ] {
                let Some(raw) = cfb.read(bytes, stream) else {
                    notes.push(format!("容器里没有流 {stream}"));
                    continue;
                };
                parts.push(stream.to_string());
                match props::decode(&raw) {
                    Ok(sets) => {
                        let mut merged = json!({});
                        for set in sets {
                            for prop in set.props {
                                let name = match prop.name {
                                    Some(one) => one.to_string(),
                                    None => format!("pid-{}", prop.pid),
                                };
                                merged[&name] = json!({
                                    "pid": prop.pid,
                                    "vt": prop.vt,
                                    "value": prop.value,
                                });
                            }
                        }
                        legacy[key] = merged;
                    }
                    Err(why) => notes.push(format!("{key} 解不开：{why}")),
                }
            }
        }
        Family::Rtf => {
            source = "rtf-info";
            let info = crate::rtf::parse_info(bytes);
            core = json!(info.fields);
            let mut named = serde_json::Map::new();
            let mut kinds: Vec<String> = Vec::new();
            for one in info.props.iter() {
                named.insert(
                    one.name.clone(),
                    json!({"value": one.value.clone(), "type": one.kind}),
                );
                kinds.push(format!(
                    "{}={}",
                    one.name,
                    one.kind.map_or("?".to_string(), |had| had.to_string())
                ));
            }
            custom = Value::Object(named);
            notes.extend(info.notes.iter().cloned());
            if info.found {
                notes.push(format!(
                    "元数据来自 \\info 群（文件自报字符集 {}）：{} 个字段、{} 条自定义属性（RTF 里的类型编号：{}）",
                    info.codepage,
                    info.fields.len(),
                    info.props.len(),
                    kinds.join("、"),
                ));
            }
        }
        _ => {
            source = "none";
            notes.push(format!("{} 没有属性这一层", doc.format));
        }
    }
    finish(
        app,
        &doc,
        source,
        core,
        application,
        custom,
        legacy,
        parts,
        thumbnail,
        notes,
        ctx,
        start,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    app: &OfficeMeta,
    doc: &crate::opack::Doc,
    source: &'static str,
    core: Value,
    application: Value,
    custom: Value,
    legacy: Value,
    parts: Vec<String>,
    thumbnail: bool,
    notes: Vec<String>,
    ctx: &Context,
    start: std::time::Instant,
) -> Result<Value, AppError> {
    let result = json!({
        "path": app.path.to_string_lossy(),
        "parts": doc.entries.len(),
        "streams": doc.compound.as_ref().map(|one| one.stream_count()),
        "format": doc.format,
        "app": doc.app,
        "source": source,
        "core": core,
        "application": application,
        "custom": custom,
        "legacy": legacy,
        "docprops_parts": parts,
        "thumbnail": thumbnail,
        "notes": notes,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// core.xml 的字段：标签（局部名）→ 报出来用的键名
const CORE_FIELDS: &[(&str, &str)] = &[
    ("title", "title"),
    ("subject", "subject"),
    ("creator", "creator"),
    ("keywords", "keywords"),
    ("description", "description"),
    ("category", "category"),
    ("contentStatus", "content-status"),
    ("language", "language"),
    ("identifier", "identifier"),
    ("version", "version"),
    ("lastModifiedBy", "last-modified-by"),
    ("revision", "revision"),
    ("created", "created"),
    ("modified", "modified"),
];

/// 叶子元素的文本（app.xml 全是这种）
fn leaf_text(node: &Node) -> String {
    node.text().trim().to_string()
}

/// 把一棵子里的叶子元素摊成键值表（app.xml 的 `<Application>…</Application>` 就是这种）。
///
/// 只能往下走**一层**：`docProps/app.xml` 的叶子挂在根 `<Properties>` 里面，
/// 而根元素本身不是一个属性 —— 原来把「有孩子的节点」整个摊成 `application.Properties`
/// 那一份子表，于是 `application.Application` 是空的（CI 上真就这么错）。
/// 反过来，`<HeadingPairs>` 那种嵌套容器里的 `<vt:lpstr>` 也不是属性，
/// 再往下钻就会把结构噪声报成账 —— 所以到第二层就停。
fn flatten(node: &Node, into: &mut Value, depth: usize) {
    for one in &node.children {
        if one.name == "#text" {
            continue;
        }
        let nested: Vec<&Node> = one
            .children
            .iter()
            .filter(|child| child.name != "#text")
            .collect();
        if !nested.is_empty() {
            if depth < 1 {
                flatten(one, into, depth + 1);
            }
            continue;
        }
        let text = leaf_text(one);
        if !text.is_empty() {
            into[one.local()] = json!(text);
        }
    }
}

fn read(bytes: &[u8], want: &str) -> Option<zipread::Member> {
    zipread::member(bytes, want, DEFAULT_MEMBER_CAP).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeMeta {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_meta(&app, &Context::new_test(tx)).expect("office-meta 应成功")
    }

    /// docx 的三份账本：核心属性、应用属性、用户自定义属性，值全部按文件里写的给
    #[test]
    fn reads_the_three_docprops_books_of_a_docx() {
        let out = run("notes.docx");
        assert_eq!(out["source"], "docProps");
        assert_eq!(out["core"]["title"], "季度预算说明");
        assert_eq!(out["core"]["subject"], "季度预算");
        assert_eq!(out["core"]["creator"], "liuqi");
        assert_eq!(out["core"]["keywords"], "budget,quarterly");
        assert_eq!(out["core"]["category"], "预算");
        assert_eq!(out["core"]["revision"], "1");
        assert_eq!(out["core"]["created"], "2013-12-23T23:15:00Z");
        assert_eq!(
            out["application"]["Application"],
            "Microsoft Macintosh Word"
        );
        assert_eq!(out["application"]["Template"], "Normal.dotm");
        assert_eq!(out["application"]["Pages"], "1");
        assert_eq!(out["custom"]["口径"], "含税");
        assert_eq!(out["custom"]["预算额度"], 124000);
        assert_eq!(
            out["thumbnail"],
            json!(true),
            "python-docx 的模板自带缩略图"
        );
        assert_eq!(out["docprops_parts"].as_array().expect("是数组").len(), 4);
    }

    /// xlsx / pptx 用的是同一套 docProps：换个生产者也要读得出来
    #[test]
    fn the_same_books_work_for_workbook_and_deck() {
        let out = run("book.xlsx");
        assert_eq!(out["core"]["title"], "季度预算说明");
        assert_eq!(
            out["application"]["Application"],
            "Microsoft Excel Compatible / Openpyxl 3.1.5"
        );
        let deck = run("deck.pptx");
        assert_eq!(deck["core"]["creator"], "liuqi");
        assert_eq!(deck["core"]["last-modified-by"], "Steve Canny");
        assert_eq!(
            deck["application"]["PresentationFormat"],
            "On-screen Show (4:3)"
        );
    }

    /// ODF 的账在 meta.xml，自定义属性在 user-defined 里
    #[test]
    fn reads_opendocument_metadata() {
        let out = run("notes.odt");
        assert_eq!(out["source"], "meta.xml");
        assert_eq!(out["core"]["title"], "季度预算说明");
        // LibreOffice 只写 meta:initial-creator，没有 dc:creator —— 不能替它补一个
        assert_eq!(out["core"]["creator"], Value::Null, "{out}");
        assert_eq!(out["core"]["initial-creator"], "liuqi");
        let generator = out["core"]["generator"].as_str().expect("有 generator");
        assert!(generator.starts_with("LibreOffice/"), "{generator}");
        assert!(
            out["core"]["language"] == "zh-CN" || out["core"]["language"].is_null(),
            "{out}"
        );
    }

    /// 遗留格式：OLE 属性集。中文字要按文件自己声明的字符集解，不能碎
    #[test]
    fn reads_ole_property_sets_of_a_legacy_doc() {
        let out = run("notes.doc");
        assert_eq!(out["source"], "ole-property-sets");
        let summary = &out["legacy"]["SummaryInformation"];
        assert_eq!(summary["title"]["value"], "季度预算说明");
        assert_eq!(summary["subject"]["value"], "季度预算");
        assert_eq!(summary["author"]["value"], "liuqi");
        assert_eq!(summary["keywords"]["value"], "budget, quarterly");
        assert_eq!(summary["template"]["value"], "Normal.dotm");
        assert_eq!(summary["codepage"]["value"], 65001, "CodePage 按无符号报");
        assert_eq!(summary["created"]["value"]["utc"], "2013-12-23T15:15:00");
        assert_eq!(
            summary["created"]["value"]["filetime"],
            130322853000000000u64
        );
    }

    /// 生产者之间 PID 语义不一致时不猜名字：DocSummaryInformation 的 1 号位
    /// 在 Word 里是段落数、在 LibreOffice 写的那份里是 CodePage —— 只能报 PID
    #[test]
    fn ambiguous_pids_are_not_named() {
        let out = run("notes.doc");
        let set = &out["legacy"]["DocumentSummaryInformation"];
        let first = set["pid-1"].as_object().expect("按 pid 报");
        assert_eq!(first.get("pid").and_then(|one| one.as_u64()), Some(1));
        assert!(!set
            .as_object()
            .expect("是对象")
            .contains_key("section-count"));
    }

    /// 没有的属性不许被编成空字符串：缺就是缺
    #[test]
    fn missing_properties_stay_missing() {
        let out = run("book.xls");
        let summary = &out["legacy"]["SummaryInformation"];
        assert_eq!(summary["title"]["value"], "季度预算说明");
        assert!(
            summary.get("keywords").is_none(),
            "这份 .xls 没写关键字，不该凭空出现：{summary}"
        );
        assert!(out["custom"].as_object().expect("是对象").is_empty());
    }

    /// RTF 的属性来自 `\info` 群（期望值同 `lyco_rtf.py::rtf_info`）
    #[test]
    fn rtf_answers_with_its_info_group() {
        let out = run("notes.rtf");
        assert_eq!(out["source"], "rtf-info");
        assert_eq!(out["core"]["title"], "季度预算说明", "{out}");
        assert_eq!(out["core"]["author"], "liuqi");
        assert_eq!(out["core"]["created"], "2013-12-23T23:15:00");
        assert_eq!(out["core"].as_object().expect("是对象").len(), 7);
        assert_eq!(out["custom"]["口径"]["value"], "含税", "{out}");
        assert_eq!(out["custom"]["预算额度"]["type"], 3);
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("info"), "{note}");
        assert!(note.contains("printed"), "全零的时间要说破：{note}");
    }

    /// 每个真实生产者文件都要给出至少一条属性（这条同时是本命令的全家族回归）
    #[test]
    fn every_fixture_yields_some_property() {
        for name in [
            "notes.docx",
            "notes.docm",
            "book.xlsx",
            "deck.pptx",
            "notes.odt",
            "book.ods",
            "deck.odp",
            "notes.doc",
            "book.xls",
            "deck.ppt",
        ] {
            let out = run(name);
            let filled = [
                out["core"].as_object().expect("core").len(),
                out["application"].as_object().expect("application").len(),
                out["custom"].as_object().expect("custom").len(),
                out["legacy"].as_object().expect("legacy").len(),
            ]
            .into_iter()
            .sum::<usize>();
            assert!(filled > 0, "{name} 一条属性都没读到：{out}");
        }
    }
}
