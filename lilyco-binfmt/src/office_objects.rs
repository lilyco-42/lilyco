//! `lbin office-objects` — 文件里除了正文还装了什么，以及哪些是要留心的。
//!
//! 常见需求其实分两类。一类是「这文档里有什么东西」：图片、嵌入对象、字体、自定义 XML、
//! 缩略图；另一类是「这文档会不会咬我」：宏（`vbaProject.bin`）、外部数据（站外链接、
//! 外部工作簿、远程模板）、OLE 对象、表单控件、数字签名、加密。
//!
//! 两类合在一条命令里，因为它们读的是同一批事实（部件 + 关系 + 内容类型），
//! 分开只会让人在两份输出之间对来对去。
//!
//! 界要说清楚：宏检测回答的是「包里有没有宏部件」，**不解析宏内容**（VBA 工程是另一套
//! 压缩结构，而且解析它不等于执行它 —— 本域一律不执行）。这一点在 fixture 里也标得很明白：
//! `notes.docm` 那份的 `vbaProject.bin` 是合成的（没有真宏），它只证明这条检测成立。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, ContentTypes, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 引用算不算「在包外面」：只看它有没有 scheme（`https:`、`vnd.sun.star.script:`…）。
/// 这是 URI 的通用形状，不是照着某一种格式的名字表猜的 —— 没 scheme 的才可能是包内路径
fn has_scheme(raw: &str) -> bool {
    let text = raw.trim_start();
    let Some((head, _)) = text.split_once(':') else {
        return false;
    };
    let mut chars = head.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

/// 把一个部件里的 `href`（ODF 写作 `xlink:href`，前缀由文件自己声明）逐条收下来
fn collect_hrefs(text: &str, part: &str, out: &mut Vec<Value>, limit: usize, total: &mut usize) {
    let root = xmlscan::parse_str(text);
    walk_hrefs(&root, part, out, limit, total);
}

fn walk_hrefs(
    node: &xmlscan::Node,
    part: &str,
    out: &mut Vec<Value>,
    limit: usize,
    total: &mut usize,
) {
    if let Some(raw) = node.attr_local("href") {
        *total += 1;
        if out.len() < limit {
            out.push(json!({
                "target": raw,
                "part": part,
                "element": node.local(),
                "external": has_scheme(raw),
            }));
        }
    }
    for one in &node.children {
        walk_hrefs(one, part, out, limit, total);
    }
}

/// 列出办公文件里的嵌入物与外部引用，并标出需要留心的部分（T0 只读）
#[derive(App)]
#[app(
    name = "office-objects",
    run = "run_office_objects",
    about = "Report what an office document carries besides its text, and what deserves a second look. Covers media parts (by declared content type, not by guessing at filenames), embedded OLE objects (word/embeddings, xl/embeddings, ppt/embeddings, ODF Objects/), embedded fonts, custom XML items, thumbnails; and the watch-list: macro projects (vbaProject.bin or a declared vbaProject content type), encryption parts (EncryptedPackage / encryptionInfo), digital signature parts, external relationships grouped by kind (hyperlink, oleObject, externalLinkData, attachedTemplate) with their targets, external-link workbook parts, and form or ActiveX parts. For legacy compound files it reports stream names (Macros, _VBA_PROJECT_CUR, Embx.*, Object*) instead of parts. ODF packages have no OPC relationship table, so their references are read off xlink:href on whichever element carries one: every hit is listed under links with its part, the element that held it and an external flag set by the one rule that needs no format-specific name table - the target has a URI scheme. Macro detection means 'a macro part is present' - the project stream is never parsed and never executed; ODF Basic libraries are NOT judged here, because no fixture carries one and inventing the part names would be self-authored evidence. Returns { path, format, app, media, objects, fonts, custom_xml, thumbnails, external, links, watch, risk_signals, notes }."
)]
pub struct OfficeObjects {
    /// 办公文件
    #[arg(about = "Office document to inspect", must_exist = true)]
    path: PathBuf,

    /// 最多列多少个目标（总数照实给）
    #[arg(
        about = "List at most this many external targets",
        default = 100,
        min = 1
    )]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 100;

fn run_office_objects(app: &OfficeObjects, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("listing embedded and external things".to_string()),
    });
    let doc = open(&blob.bytes);
    let mut notes: Vec<String> = Vec::new();
    let mut media: Vec<Value> = Vec::new();
    let mut objects: Vec<Value> = Vec::new();
    let mut fonts: Vec<String> = Vec::new();
    let mut custom_xml: Vec<String> = Vec::new();
    let mut thumbnails: Vec<String> = Vec::new();
    let mut external: Vec<Value> = Vec::new();
    let mut links: Vec<Value> = Vec::new();
    let mut watch: Vec<Value> = Vec::new();

    if doc.is_zip_family() {
        let types = if doc.family == Family::Ooxml {
            ContentTypes::read(&blob.bytes)
        } else {
            ContentTypes::default()
        };
        let (rels, mut rel_notes) = crate::opack::relationships(&blob.bytes, &doc.entries);
        notes.append(&mut rel_notes);
        let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
        for one in &doc.entries {
            let name = one.name.as_str();
            let declared = types.of(name).unwrap_or("");
            if name.starts_with("docProps/thumbnail") {
                // 缩略图也是 image/jpeg，但它不是文档带的图：先认它，
                // 否则「这个文件里有几张图」会被一张预览图顶掉（独立读者就不这么数）。
                thumbnails.push(name.to_string());
            } else if declared.starts_with("image/") || name.starts_with("Pictures/") {
                media.push(json!({"part": name, "content_type": declared, "size": one.size}));
            } else if name.contains("/embeddings/")
                || name.starts_with("Objects/")
                || declared.contains("oleObject")
            {
                objects.push(json!({"part": name, "content_type": declared, "size": one.size}));
            } else if name.starts_with("ppt/fonts/") || declared.contains("officeDocumentFont") {
                fonts.push(name.to_string());
            } else if name.starts_with("customXml/") && !name.contains("_rels") {
                custom_xml.push(name.to_string());
            }
            if name.ends_with("vbaProject.bin")
                || declared.contains("vbaProject")
                || name.ends_with("vbaSignature.xml")
            {
                watch.push(json!({"kind": "macro", "part": name, "content_type": declared}));
            }
            if name == "EncryptedPackage" || name == "encryptionInfo" {
                watch.push(json!({"kind": "encrypted", "part": name}));
            }
            if name.ends_with("/vbaSignature.xml") || name.contains("digitalSignature") {
                watch.push(json!({"kind": "signature", "part": name}));
            }
            // 别用 "form" 当判据：每一个 OOXML 内容类型都写着
            // `application/vnd.openxmlformats-…`，"xformats" 里就含 "form"，
            // 于是全包 22 个部件会被当成 15 个表单/ActiveX 控件。
            if name.starts_with("word/activeX")
                || name.starts_with("word/axO")
                || declared.contains("activex")
                || declared.contains("oleObject")
            {
                watch.push(
                    json!({"kind": "form-or-activex", "part": name, "content_type": declared}),
                );
            }
            if name == "_xmlsignatures/signature_1.bin" || name.contains("signature") {
                watch.push(json!({"kind": "signature", "part": name}));
            }
        }
        for one in rels.iter().filter(|one| one.external) {
            if external.len() < limit {
                external
                    .push(json!({"kind": one.kind, "target": one.target, "source": one.source}));
            }
            if one.kind == "attachedTemplate" {
                watch.push(json!({"kind": "remote-template", "target": one.target}));
            }
            if one.kind == "oleObject" {
                watch.push(json!({"kind": "external-ole", "target": one.target}));
            }
        }
        let external_total = rels.iter().filter(|one| one.external).count();
        if external_total > external.len() {
            notes.push(format!(
                "站外目标共 {external_total} 条，只列了前 {} 条（limit）",
                external.len()
            ));
        }
        // 外部工作簿是以「部件」形式存在的（xl/externalLinks/），不是外部关系
        let linked: Vec<&str> = doc
            .entries
            .iter()
            .map(|one| one.name.as_str())
            .filter(|one| one.starts_with("xl/externalLinks/externalLink"))
            .collect();
        if !linked.is_empty() {
            watch.push(json!({"kind": "external-workbook", "parts": linked}));
        }
        if doc.family == Family::Odf {
            // ODF 没有 OPC 关系表：站外引用坐在 `xlink:href` 上，而前缀是文件自己声明的，
            // 所以按局部名找。判据只有一条 —— 带 scheme 的就不是包内路径。
            let mut total = 0usize;
            for one in &doc.entries {
                let name = one.name.as_str();
                if !name
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .ends_with(".xml")
                {
                    continue;
                }
                // `entries` 只是目录（名字、大小、CRC），字要现解出来；
                // 解不动的部件照实说，不装作那份不存在
                let Ok(member) = zipread::member(&blob.bytes, name, DEFAULT_MEMBER_CAP) else {
                    notes.push(format!("{name} 解不出来：那条引用没看"));
                    continue;
                };
                collect_hrefs(&member.as_text(), name, &mut links, limit, &mut total);
            }
            external.extend(
                links
                    .iter()
                    .filter(|one| one["external"].as_bool() == Some(true))
                    .map(|one| {
                        json!({
                            "kind": "xlink-href",
                            "target": one["target"],
                            "source": one["part"],
                            "element": one["element"],
                        })
                    }),
            );
            if total > links.len() {
                notes.push(format!(
                    "ODF 的引用共 {total} 条，只列了前 {} 条（limit）",
                    links.len()
                ));
            }
            notes.push(
                "ODF 的引用按 xlink:href 扫（关系表在 OPC 里，ODF 没有那份表）；\
                 ODF 里带的 Basic 库这一版**不判**：手上没有含宏的 ODF 样本，\
                 照记忆猜部件名等于自造证据"
                    .to_string(),
            );
        }
    } else if let Some(cfb) = doc.compound.as_ref() {
        let names = cfb.stream_names();
        for one in &names {
            let lower = one.to_lowercase();
            if lower.contains("macros") || lower.contains("vba") || lower.contains("_vba_project") {
                watch.push(json!({"kind": "macro", "stream": one}));
            }
            if lower.starts_with("embx") || lower.contains("ole") && one.starts_with('\u{1}') {
                objects.push(json!({"stream": one}));
            }
            if lower.contains("encrypted") {
                watch.push(json!({"kind": "encrypted", "stream": one}));
            }
        }
        notes.push(format!(
            "复合文档按流看：{} 条流（{}）。图片在 .ppt 里是 Pictures 流、在 .doc 里是 Data 流，本命令不解其中的图形记录",
            names.len(),
            names.join("、")
        ));
    } else {
        notes.push(format!(
            "{} 既不是包也不是复合文档，能报的只有这些",
            doc.format
        ));
    }
    ctx.tick(1, Some(1), "listed");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "format": doc.format,
        "app": doc.app,
        "media": media,
        "objects": objects,
        "fonts": fonts,
        "custom_xml": custom_xml,
        "thumbnails": thumbnails,
        "external": external,
        "links": links,
        "watch": watch,
        "risk_signals": {
            "has_macros": watch.iter().any(|one| one["kind"] == "macro"),
            "encrypted": watch.iter().any(|one| one["kind"] == "encrypted"),
            "external_targets": external.len(),
            "remote_template": watch.iter().any(|one| one["kind"] == "remote-template"),
            "external_workbook": watch.iter().any(|one| one["kind"] == "external-workbook"),
            "activex_or_forms": watch.iter().any(|one| one["kind"] == "form-or-activex"),
        },
        "notes": notes,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeObjects {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 100,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_objects(&app, &Context::new_test(tx)).expect("office-objects 应成功")
    }

    /// 干净的 docx：一张图、一个站外超链接、零宏
    #[test]
    fn a_clean_docx_reports_one_image_and_no_macros() {
        let out = run("notes.docx");
        assert_eq!(out["media"].as_array().expect("是数组").len(), 1, "{out}");
        assert_eq!(out["media"][0]["part"], "word/media/image1.png");
        assert_eq!(out["media"][0]["content_type"], "image/png");
        assert_eq!(out["external"][0]["kind"], "hyperlink");
        assert_eq!(out["external"][0]["target"], "https://example.com/budget");
        assert_eq!(out["risk_signals"]["has_macros"], json!(false));
        assert_eq!(out["risk_signals"]["encrypted"], json!(false));
        assert_eq!(out["risk_signals"]["external_targets"], 1);
        assert_eq!(
            out["thumbnails"].as_array().expect("是数组").len(),
            1,
            "docProps/thumbnail.jpeg"
        );
    }

    /// ODF 没有 OPC 关系表：引用在 `xlink:href` 上。`notes.odt` 里三条 ——
    /// 一个站外超链接、一张包内图、还有 `meta.xml` 里那条写着空串的模板引用
    /// （照文件报，不替它删）；`deck.odp` 两条都在包内
    /// （期望值来自 `office_reader.py` 的 odf_links）
    #[test]
    fn odf_references_are_read_off_xlink_href() {
        let out = run("notes.odt");
        let mut lines: Vec<String> = out["links"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| {
                format!(
                    "{}|{}|{}|{}",
                    one["target"].as_str().unwrap_or_default(),
                    one["part"].as_str().unwrap_or_default(),
                    one["element"].as_str().unwrap_or_default(),
                    one["external"],
                )
            })
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            vec![
                "Pictures/1000000100000008000000088E4DF5D4.png|content.xml|image|false",
                "https://example.com/budget|content.xml|a|true",
                "|meta.xml|template|false",
            ],
            "{lines:?}"
        );
        assert_eq!(
            out["external"].as_array().expect("是数组").len(),
            1,
            "{out}"
        );
        assert_eq!(out["external"][0]["kind"], "xlink-href");
        assert_eq!(out["external"][0]["target"], "https://example.com/budget");
        assert_eq!(out["external"][0]["source"], "content.xml");
        assert_eq!(out["risk_signals"]["external_targets"], 1);
        let deck = run("deck.odp");
        assert_eq!(deck["links"].as_array().expect("是数组").len(), 2, "{deck}");
        assert_eq!(
            deck["external"].as_array().expect("是数组").len(),
            0,
            "两张包内图不算站外"
        );
    }

    /// 「带 scheme 就算在包外」这一条判据的边界
    #[test]
    fn the_scheme_rule_separates_outside_from_inside() {
        assert!(has_scheme("https://example.com/x"));
        assert!(has_scheme("mailto:someone@example.com"));
        assert!(has_scheme("vnd.sun.star.script:Foo.bar?language=Basic"));
        assert!(!has_scheme("Pictures/a.png"));
        assert!(!has_scheme("#anchor"));
        assert!(!has_scheme(""));
        assert!(!has_scheme("1abc:x"), "scheme 必须字母开头");
    }

    /// 宏样本：检测必须为真，并且说清「只看包，不解析宏内容」
    #[test]
    fn the_macro_sample_is_caught() {
        let out = run("notes.docm");
        assert_eq!(out["risk_signals"]["has_macros"], json!(true), "{out}");
        let kinds: Vec<String> = out["watch"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(kinds.iter().any(|one| one == "macro"), "{kinds:?}");
        assert!(
            kinds.iter().any(|one| one == "signature"),
            "签名部件是合成的另一条：{kinds:?}"
        );
        assert_eq!(
            out["risk_signals"]["encrypted"],
            json!(false),
            "有宏不等于加密"
        );
    }

    /// pptx：python-pptx 那页有图，模板自带主题字体但没有嵌入字体
    #[test]
    fn a_deck_reports_its_media() {
        let out = run("deck.pptx");
        assert_eq!(out["media"].as_array().expect("是数组").len(), 1, "{out}");
        assert_eq!(out["media"][0]["part"], "ppt/media/image1.png");
        assert!(out["fonts"].as_array().expect("是数组").is_empty());
    }

    /// 外部工作簿这条是「部件」形式的引用，只在有 links 的包里出现
    #[test]
    fn external_workbook_links_are_detected_as_parts() {
        let out = run("book.xlsx");
        let kinds: Vec<String> = out["watch"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(
            !kinds.iter().any(|one| one == "external-workbook"),
            "{kinds:?}：这份表没有外链工作簿"
        );
        assert_eq!(out["risk_signals"]["external_targets"], 0, "{out}");
    }

    /// 遗留 .xls：按流说话，并把「图片在哪个流里」这类局限写清楚
    #[test]
    fn a_legacy_workbook_answers_in_streams() {
        let out = run("book.xls");
        let note = out["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("条流"), "{note}");
        assert!(note.contains("Workbook"), "{note}");
        assert_eq!(out["risk_signals"]["has_macros"], json!(false));
    }
}
