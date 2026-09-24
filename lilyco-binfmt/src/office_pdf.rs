//! `lbin office-pdf` — 一份 PDF 里能不问自答的那些事：几页、多大页、谁做的、
//! 用了什么字体与图、有没有加密、里面埋了哪些会自己动的东西。
//!
//! 为什么 PDF 要单独一条命令：办公文档这一族里只有它不是容器 —— OPC / ODF 是
//! ZIP，.doc/.xls/.ppt 是复合文档，RTF 是文本协议，而 PDF 是**一张对象表加若干
//! 条流**。硬塞进 [`crate::opack`] 的 `Family` 分派只会让每一条命令都多一个
//! 什么都不做的分支，所以这里自成一路，读法在 [`crate::pdf`]。
//!
//! 界要说清楚三件：
//! 1. **不解密**。加密的 PDF 照样能报结构（对象号、页树、字体名这些不加密），
//!    但字符串与流正文是密文 —— 那种乱码不会交给你当元数据，只说为什么没有；
//! 2. **不追交叉引用表**。页顺序按对象号排（`/Kids` 数组的真实顺序、以及
//!    从 XRef 流读页表，是文本抽取那一批的事）；
//! 3. **不解析内容流**。这一条回答的是「这份 PDF 是什么样的」，不是「它写了什么」。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::take_limit;
use crate::pdf::{self, Pdf};
use crate::read::read_blob;

const LIMIT_DEFAULT: usize = 200;

/// 元数据那八个键：PDF 的 Info 字典就叫这几个名字，生产者爱写哪个写哪个
const INFO_KEYS: [(&str, &[u8]); 8] = [
    ("producer", b"/Producer"),
    ("title", b"/Title"),
    ("author", b"/Author"),
    ("creator", b"/Creator"),
    ("creation_date", b"/CreationDate"),
    ("mod_date", b"/ModDate"),
    ("subject", b"/Subject"),
    ("keywords", b"/Keywords"),
];

/// 报出一份 PDF 的结构、元数据、字体与图片清单、以及需要留心的动作（T0 只读）
#[derive(App)]
#[app(
    name = "office-pdf",
    run = "run_office_pdf",
    about = "Report what a PDF file is made of without decrypting or rendering it. Reads objects by scanning N G obj markers instead of trusting the cross-reference table, then unpacks the second layer that scan alone would miss: objects packed inside object streams (/Type /ObjStm, where the header pairs an object number with an offset relative to /First) and trailer keys that live in a /Type /XRef stream dict in files with no trailer keyword at all - both rules measured against real Word 2013 and qpdf files, not recalled from memory. Gives { path, version, binary_comment, objects, xref, encryption, pages, metadata, tags, fonts, images, features, watch, notes }: page count cross-checked between /Count and the real /Type/Page objects (the one-name match matters: /Pages is a tree node, not a page), per-page MediaBox with the inheritance walk up /Parent (MediaBox and Rotate may be written once on the tree), rotation, contents reference and annotation count, the Info dictionary with PDF string rules (backslash escapes, octal, nested parens, hex strings, FEFF-prefixed UTF-16BE), tagged/PDF-X flags (/Lang, /MarkInfo /Marked, /StructTreeRoot), font inventory (/BaseFont, /Subtype, /Encoding, whether a /ToUnicode map exists, whether the font object was hidden in an object stream), image inventory, and the watch list for what runs by itself: /Encrypt parameters (reported, never decrypted - no password here and none should be), document-level /JavaScript, /Launch and /SubmitForm and /GoToR actions, /AcroForm fields, /EmbeddedFiles attachments, /OpenAction and page /AA triggers. Encrypted files report structure only: strings and streams are ciphertext, so metadata and /Lang come back null with a note rather than as decoded garbage. Object numbers that appear more than once (incremental updates) are counted and the first occurrence wins. Not provided: page reading order from /Kids, content-stream text, form field values, and signature validation - text extraction is the next step in this lane."
)]
pub struct OfficePdf {
    /// PDF 文件
    #[arg(about = "PDF file to inspect", must_exist = true)]
    path: PathBuf,

    /// 每类清单最多列多少条
    #[arg(
        about = "List at most this many entries per section",
        default = 200,
        min = 1
    )]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

fn encryption_json(one: &pdf::Encryption) -> Value {
    json!({
        "object": one.id,
        "filter": one.filter,
        "v": one.v,
        "revision": one.revision,
        "key_bits": one.length_bits,
        "has_owner_hash": one.has_o,
        "has_user_hash": one.has_u,
        "aes_256_entries": one.has_owner_entries,
        "permissions_set": one.restricted,
        "decrypted": false,
    })
}

fn object_stream_json(one: &pdf::ObjectStream) -> Value {
    json!({
        "object": one.id,
        "declared_n": one.declared_n,
        "first": one.first,
        "header_pairs": one.header_pairs,
        "unpacked": one.ok,
        "problem": one.error,
    })
}

fn page_json(one: &pdf::Page) -> Value {
    json!({
        "object": one.id,
        "media_box": one.media_box,
        "inherited_box": one.inherited_box,
        "rotate": one.rotate,
        "contents": one.contents_ref,
        "annotations": one.annots,
        "annotations_object": one.annots_ref,
    })
}

fn font_json(one: &pdf::Font) -> Value {
    json!({
        "object": one.id,
        "base_font": one.base_font,
        "subtype": one.subtype,
        "encoding": one.encoding,
        "to_unicode": one.to_unicode,
        "from_object_stream": one.from_object_stream,
    })
}

fn image_json(one: &pdf::Image) -> Value {
    json!({
        "object": one.id,
        "width": one.width,
        "height": one.height,
        "filter": one.filter,
        "color_space": one.color_space,
        "bits_per_component": one.bits,
    })
}

fn watch_list(doc: &Pdf, features: &pdf::Features) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    if let Some(one) = &doc.encryption {
        out.push(json!({
            "kind": "encrypted",
            "detail": format!("Filter/{} V {:?} R {:?}", one.filter, one.v, one.revision),
        }));
    }
    if features.javascript > 0 {
        out.push(json!({"kind": "javascript", "count": features.javascript}));
    }
    if features.launch > 0 {
        out.push(json!({
            "kind": "launch-action",
            "count": features.launch,
            "why": "打开本地程序：PDF 里最像「宏」的那一种"
        }));
    }
    if features.submit_form > 0 {
        out.push(json!({"kind": "submit-form", "count": features.submit_form}));
    }
    if features.import_data > 0 {
        out.push(json!({"kind": "import-data", "count": features.import_data}));
    }
    if features.goto_remote > 0 {
        out.push(json!({"kind": "goto-remote", "count": features.goto_remote}));
    }
    if features.additional_actions > 0 {
        out.push(json!({"kind": "page-triggered-actions", "count": features.additional_actions}));
    }
    if features.open_action > 0 {
        out.push(json!({"kind": "open-action", "count": features.open_action}));
    }
    if features.acroform > 0 {
        out.push(json!({"kind": "interactive-form", "fields": features.fields}));
    }
    if features.filespec > 0 || features.embedded_files_tree > 0 {
        out.push(json!({
            "kind": "attachments",
            "filespec": features.filespec,
            "name_tree": features.embedded_files_tree,
        }));
    }
    if features.xfa > 0 {
        out.push(json!({"kind": "xfa", "count": features.xfa}));
    }
    out
}

fn run_office_pdf(app: &OfficePdf, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the PDF object table".to_string()),
    });
    let data = &blob.bytes[..];
    if !pdf::is_pdf(data) {
        return Err(AppError::InvalidInput(
            "不是 PDF：文件开头没有 %PDF- 版本号".to_string(),
        ));
    }
    let doc = Pdf::read(data);
    let limit = take_limit(app.limit, LIMIT_DEFAULT);
    let mut notes = doc.notes.clone();
    let features = doc.features();

    // 元数据：加密的文件里字符串是密文，解出来的乱码不能当元数据交出去
    let info = doc.info_dict();
    let mut metadata = serde_json::Map::new();
    if doc.encryption.is_none() {
        if let Some((_id, one)) = info {
            for (name, key) in INFO_KEYS {
                if let Some(value) = pdf::one_string(&one.dict, key) {
                    metadata.insert(name.to_string(), json!(value));
                }
            }
        }
    }
    let info_id = doc.info_ref();

    let catalogs = doc.catalogs();
    let mut lang = Value::Null;
    let mut marked = Value::Null;
    let mut struct_tree = Value::Null;
    for id in &catalogs {
        let Some(one) = doc.objects.get(id) else {
            continue;
        };
        if doc.encryption.is_none() {
            if let Some(value) = pdf::one_string(&one.dict, b"/Lang") {
                lang = json!(value);
            }
        }
        if pdf::marked_true(&one.dict) {
            marked = json!(true);
        }
        if pdf::has_key(&one.dict, b"/StructTreeRoot") {
            struct_tree = json!(true);
        }
    }
    if doc.encryption.is_some() {
        notes.push(
            "这份文件是加密的：/Lang、Info 各项与正文都是密文，这里刻意不给出解开的结果 \
             （那会是乱码），只报结构与加密参数"
                .to_string(),
        );
    }

    let pages = doc.page_facts();
    let counts = doc.declared_counts();
    let declared_total: i64 = counts.iter().sum();
    let agreement = declared_total == pages.len() as i64;
    if counts.is_empty() && !pages.is_empty() {
        notes.push(
            "页树节点里没有 /Count：只能报扫到的 /Type/Page 对象数，\
             这与「这份 PDF 有几页」通常一致，但对不上账时没有第二个数可核"
                .to_string(),
        );
    }
    if !agreement {
        notes.push(format!(
            "页数对不上账：/Count 合计 {declared_total}，扫到的 /Type/Page 对象 {} 个 —— \
             两种读法必有一个没走完（页树可能有分支指向看不见的对象）",
            pages.len()
        ));
    }
    // 去重后排序：两边读者要给出同一个顺序（按页出现的先后排会在多尺寸文件上分叉）
    let mut seen_sizes: Vec<String> = Vec::new();
    for one in &pages {
        if let Some(boxed) = &one.media_box {
            if !seen_sizes.contains(boxed) {
                seen_sizes.push(boxed.clone());
            }
        }
    }
    seen_sizes.sort();
    let inner = doc
        .objects
        .values()
        .filter(|one| one.from_stream.is_some())
        .count();

    let result = json!({
        "path": app.path.display().to_string(),
        "size": blob.size,
        "kind": "office-pdf",
        "format": format!("pdf{}", if doc.version.is_empty() { String::new() } else { format!("-{}", doc.version) }),
        "version": doc.version,
        "header_bytes": data[..std::cmp::min(data.len(), 8)].iter().map(|one| format!("{one:02x}")).collect::<String>(),
        "binary_comment": doc.binary_comment,
        "objects": {
            "plain": doc.plain,
            "in_object_streams": inner,
            "total_seen": doc.objects.len(),
            "duplicated_ids": doc.duplicates,
            "object_streams": doc.object_streams.iter().take(limit).map(object_stream_json).collect::<Vec<_>>(),
        },
        "xref": {
            "trailer_keyword": doc.trailers.len(),
            "xref_streams": doc.xref_dicts.iter().map(|(id, _dict)| *id).collect::<Vec<_>>(),
            "declared_sizes": doc.trailer_sizes(),
            "root": doc.root_id(),
            "info": info_id,
            "info_object": info.map(|(id, _one)| id),
        },
        "encryption": doc.encryption.as_ref().map(encryption_json),
        "pages": {
            "page_objects": pages.len(),
            "tree_nodes": doc.pages_nodes().len(),
            "counts": counts,
            "count_matches_pages": agreement,
            "distinct_boxes": seen_sizes,
            "inherited_boxes": pages.iter().filter(|one| one.inherited_box).count(),
            "missing_boxes": pages.iter().filter(|one| one.media_box.is_none()).count(),
            "rotations": pages.iter().map(|one| one.rotate).collect::<Vec<_>>(),
            "list": pages.iter().take(limit).map(page_json).collect::<Vec<_>>(),
        },
        "metadata": if metadata.is_empty() { Value::Null } else { Value::Object(metadata) },
        "tags": {
            "lang": lang,
            "marked": marked,
            "struct_tree_root": struct_tree,
        },
        "fonts": doc.fonts().iter().take(limit).map(font_json).collect::<Vec<_>>(),
        "images": doc.images().iter().take(limit).map(image_json).collect::<Vec<_>>(),
        "features": {
            "javascript": features.javascript,
            "launch": features.launch,
            "submit_form": features.submit_form,
            "import_data": features.import_data,
            "goto_remote": features.goto_remote,
            "uri_actions": features.uri,
            "attachments": features.filespec,
            "attachment_name_trees": features.embedded_files_tree,
            "acroform": features.acroform,
            "fields": features.fields,
            "open_action": features.open_action,
            "page_triggers": features.additional_actions,
            "xfa": features.xfa,
            "link_annotations": features.links,
            "widget_annotations": features.widgets,
            "other_annotations": features.other_annot,
            "self_running_total": features.dangerous,
        },
        "watch": watch_list(&doc, &features),
        "notes": notes,
        "elapsed_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficePdf {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_pdf(&app, &Context::new_test(tx)).expect("office-pdf 应成功")
    }

    /// LibreOffice（Writer）导出的那份：期望值全部来自 `lyco_pdf.py` 的实测，
    /// 页数、页面尺寸、字体与加密标志另经 pdfinfo / pdffonts 第三方读者核对
    #[test]
    fn a_libreoffice_pdf_reports_pages_boxes_fonts_and_producer() {
        let out = run("notes.pdf");
        assert_eq!(out["version"], json!("1.7"));
        assert_eq!(out["objects"]["plain"], json!(75));
        assert_eq!(out["objects"]["in_object_streams"], json!(0));
        assert_eq!(out["objects"]["total_seen"], json!(75));
        assert_eq!(out["objects"]["duplicated_ids"], json!(0));
        assert_eq!(out["xref"]["trailer_keyword"], json!(1));
        assert_eq!(out["xref"]["root"], json!(74));
        assert_eq!(out["xref"]["info"], json!(75));
        assert_eq!(out["pages"]["page_objects"], json!(2));
        assert_eq!(out["pages"]["tree_nodes"], json!(1));
        assert_eq!(out["pages"]["counts"], json!([2]));
        assert_eq!(out["pages"]["count_matches_pages"], json!(true));
        assert_eq!(out["pages"]["distinct_boxes"], json!(["0 0 612 792"]));
        assert_eq!(out["pages"]["rotations"], json!([0, 0]));
        assert_eq!(out["encryption"], Value::Null);
        assert_eq!(
            out["metadata"]["producer"],
            json!("LibreOffice 26.8.0.3 (X86_64)")
        );
        assert_eq!(out["metadata"]["title"], json!("季度预算说明"));
        assert_eq!(out["metadata"]["author"], json!("liuqi"));
        assert_eq!(out["metadata"]["subject"], json!("季度预算"));
        assert_eq!(out["tags"]["lang"], json!("en-US"));
        assert_eq!(out["tags"]["marked"], json!(true));
        assert_eq!(out["tags"]["struct_tree_root"], json!(true));
        let fonts = out["fonts"].as_array().expect("字体清单是数组");
        assert_eq!(fonts.len(), 5);
        assert!(fonts.iter().all(|one| one["to_unicode"] == json!(true)));
        assert!(fonts.iter().all(|one| one["subtype"] == json!("TrueType")));
        assert!(fonts
            .iter()
            .all(|one| one["encoding"] == json!("WinAnsiEncoding")));
        assert_eq!(out["images"].as_array().map(|one| one.len()), Some(2));
        // 正文里的脚注链接是一个 URI 批注；没有脚本、没有表单、没有附件
        assert_eq!(out["features"]["javascript"], json!(0));
        assert_eq!(out["features"]["uri_actions"], json!(1));
        assert_eq!(out["features"]["link_annotations"], json!(1));
        assert_eq!(out["features"]["attachments"], json!(0));
        assert_eq!(out["watch"], json!([]));
    }

    /// 同一份文档换 Impress 导出：页面尺寸与 /Lang 都不同，字体多一张
    #[test]
    fn a_slide_pdf_reports_its_own_page_size_and_language() {
        let out = run("deck.pdf");
        assert_eq!(out["pages"]["distinct_boxes"], json!(["0 0 720 540"]));
        assert_eq!(out["tags"]["lang"], json!("zh-CN"));
        assert_eq!(out["fonts"].as_array().map(|one| one.len()), Some(6));
        assert_eq!(out["features"]["link_annotations"], json!(0));
        assert_eq!(out["metadata"]["creator"], json!("Impress"));
    }

    /// qpdf 从同一份 notes.pdf 存的 1.5 版：68 个对象里只有 17 个是明写的，
    /// 剩下 51 个挤在一个对象流里，而且整个文件**没有 `trailer` 这个词** ——
    /// 只扫 `obj` 的读者会报出「0 页」，只认 trailer 的读者找不到 /Info
    #[test]
    fn objects_hidden_in_a_stream_and_a_trailer_that_is_not_there() {
        let out = run("objstm.pdf");
        assert_eq!(out["version"], json!("1.5"));
        assert_eq!(out["objects"]["plain"], json!(17));
        assert_eq!(out["objects"]["in_object_streams"], json!(51));
        assert_eq!(out["objects"]["total_seen"], json!(68));
        assert_eq!(out["xref"]["trailer_keyword"], json!(0));
        assert_eq!(out["xref"]["xref_streams"], json!([68]));
        // /Info 只写在 XRef 流的字典里：两处都找才指得到
        assert_eq!(out["xref"]["info"], json!(52));
        assert_eq!(out["pages"]["page_objects"], json!(2));
        assert_eq!(out["pages"]["counts"], json!([2]));
        assert_eq!(out["pages"]["distinct_boxes"], json!(["0 0 612 792"]));
        assert_eq!(
            out["metadata"]["producer"],
            json!("LibreOffice 26.8.0.3 (X86_64)")
        );
        assert_eq!(out["tags"]["lang"], json!("en-US"));
        let streams = out["objects"]["object_streams"]
            .as_array()
            .expect("对象流清单");
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0]["declared_n"], json!(51));
        assert_eq!(streams[0]["unpacked"], json!(51));
        assert_eq!(streams[0]["problem"], Value::Null);
        // 字体整个住在对象流里：第三方读者 pdffonts 报的号与这边一致
        let fonts = out["fonts"].as_array().expect("字体清单是数组");
        assert_eq!(fonts.len(), 5);
        assert!(fonts
            .iter()
            .all(|one| one["from_object_stream"] == json!(true)));
        assert_eq!(fonts[0]["object"], json!(35));
    }

    /// AES-256 真加密的一份（qpdf 加口令）。这一条守的是「不编」的界：
    /// 结构照报，密文不乱解，元数据宁可给 null 加一句为什么
    #[test]
    fn an_encrypted_pdf_reports_structure_but_not_ciphertext_as_metadata() {
        let out = run("locked.pdf");
        assert_eq!(out["encryption"]["filter"], json!("Standard"));
        assert_eq!(out["encryption"]["v"], json!(5));
        assert_eq!(out["encryption"]["revision"], json!(6));
        assert_eq!(out["encryption"]["key_bits"], json!(32));
        assert_eq!(out["encryption"]["aes_256_entries"], json!(true));
        assert_eq!(out["encryption"]["decrypted"], json!(false));
        assert_eq!(out["metadata"], Value::Null);
        assert_eq!(out["tags"]["lang"], Value::Null);
        // 数字与名字不加密：页数、页面尺寸、字体名照样报得出（pdfinfo 不给口令
        // 直接拒绝打开，这边能给的就是这些）
        assert_eq!(out["pages"]["page_objects"], json!(2));
        assert_eq!(out["pages"]["distinct_boxes"], json!(["0 0 612 792"]));
        assert_eq!(out["fonts"].as_array().map(|one| one.len()), Some(5));
        let watch = out["watch"].as_array().expect("留心清单");
        assert_eq!(watch[0]["kind"], json!("encrypted"));
        assert!(out["notes"]
            .as_array()
            .expect("说明")
            .iter()
            .any(|one| one.as_str().unwrap_or_default().contains("密文")));
    }

    /// 手搓的那份风险面（pdfinfo 认它：Form: AcroForm、JavaScript: yes、
    /// 612x792 且 rot 90 —— 后两样是**继承**来的）。这份 fixture 是手搓的，
    /// 所以只用它验规则，不用它代表任何真实生产者
    #[test]
    fn a_pdf_that_runs_things_by_itself_lists_every_one_of_them() {
        let out = run("risk.pdf");
        assert_eq!(out["binary_comment"], json!(true));
        assert_eq!(out["objects"]["plain"], json!(16));
        assert_eq!(out["pages"]["page_objects"], json!(1));
        assert_eq!(out["pages"]["inherited_boxes"], json!(1));
        assert_eq!(out["pages"]["rotations"], json!([90]));
        assert_eq!(out["pages"]["distinct_boxes"], json!(["0 0 612 792"]));
        assert_eq!(out["features"]["javascript"], json!(3));
        assert_eq!(out["features"]["launch"], json!(1));
        assert_eq!(out["features"]["uri_actions"], json!(1));
        assert_eq!(out["features"]["attachments"], json!(1));
        assert_eq!(out["features"]["acroform"], json!(1));
        assert_eq!(out["features"]["fields"], json!(1));
        assert_eq!(out["features"]["open_action"], json!(1));
        assert_eq!(out["features"]["page_triggers"], json!(1));
        assert_eq!(out["features"]["link_annotations"], json!(2));
        assert_eq!(out["features"]["widget_annotations"], json!(1));
        assert_eq!(out["metadata"]["title"], json!("Risk fixture"));
        assert_eq!(out["tags"]["lang"], json!("en-US"));
        assert_eq!(out["tags"]["marked"], Value::Null);
        let kinds: Vec<&str> = out["watch"]
            .as_array()
            .expect("留心清单")
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or_default())
            .collect();
        for want in [
            "javascript",
            "launch-action",
            "attachments",
            "interactive-form",
            "open-action",
            "page-triggered-actions",
        ] {
            assert!(kinds.contains(&want), "少了 {want}：{kinds:?}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_refused_by_name() {
        let app = OfficePdf {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_pdf(&app, &Context::new_test(tx)).unwrap_err();
        let text = why.to_string();
        assert!(text.contains("%PDF-"), "{text}");
    }
}
