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
    about = "Report what a PDF file is made of without decrypting or rendering it. Reads objects by scanning N G obj markers instead of trusting the cross-reference table, then unpacks the second layer that scan alone would miss: objects packed inside object streams (/Type /ObjStm, where the header pairs an object number with an offset relative to /First) and trailer keys that live in a /Type /XRef stream dict in files with no trailer keyword at all - both rules measured against real Word 2013 and qpdf files, not recalled from memory. Gives { path, version, binary_comment, objects, xref, encryption, pages, metadata, tags, fonts, images, features, watch, outline, links, annotations, permissions, notes }: page count cross-checked between /Count and the real /Type/Page objects (the one-name match matters: /Pages is a tree node, not a page), per-page MediaBox with the inheritance walk up /Parent (MediaBox and Rotate may be written once on the tree), rotation, contents reference and annotation count, the Info dictionary with PDF string rules (backslash escapes, octal, nested parens, hex strings, FEFF-prefixed UTF-16BE), tagged/PDF-X flags (/Lang, /MarkInfo /Marked, /StructTreeRoot), font inventory (/BaseFont, /Subtype, /Encoding, whether a /ToUnicode map exists, whether the font object was hidden in an object stream), image inventory, and the watch list for what runs by itself: /Encrypt parameters (reported, never decrypted - no password here and none should be), document-level /JavaScript, /Launch and /SubmitForm and /GoToR actions, /AcroForm fields, /EmbeddedFiles attachments, /OpenAction and page /AA triggers. Encrypted files report structure only: strings and streams are ciphertext, so metadata and /Lang come back null with a note rather than as decoded garbage. Object numbers that appear more than once (incremental updates) are counted and the first occurrence wins. With --text it also walks the content streams and returns { order_from_page_tree, chars, chars_no_spaces, lines, pages, text }: pages in /Kids order, glyph codes mapped through each font's /ToUnicode CMap (bfchar plus both bfrange forms - a range written as one number means consecutive code points, written as an array means one per code), and lines rebuilt from positions rather than from stream order, because LibreOffice splits a single Chinese heading across several TJ arrays with large negative kerns between them. Three position rules make the difference between reading that heading and reading its characters shuffled: BT resets the text matrix to identity, glyph advance moves x only (never y), and a TJ number moves the pen the OPPOSITE way (positive left, negative right). It also answers where the file points: outline (the /Outlines tree - /First then /Next, children under /First, each entry with its title, depth, target page object and which page that is in /Kids order, the root self-reported /Count, and the open/closed sign of each /Count), links (every /Subtype /Link annotation split into outbound URI, page-to-page with the resolved page number, and actions that go nowhere such as /Launch - the last kind carries its /S value in `action`, because a bare category cannot tell a launch from a form submit), and permissions (the /Encrypt /P bits, read as the signed integer it is: bits 3-6 always, 9-12 only from R3, otherwise null). Encrypted files still report structure, page numbers and permission bits, because numbers and names are not ciphertext, while titles and URIs come back null. `annotations` is the whole `/Annots` ledger, not just the links: every annotation the page names, with the /Subtype it wrote (absent stays a separate count rather than an empty name), its /T author, /Contents text and /M stamp handed over as written - LibreOffice writes `D:00000000000000Z` for a note's modification date, and that string is reported instead of being turned into a date or into null - plus the two pointers between annotations (`/Popup` on the note, `/Parent` on the box), because a note usually arrives with a popup that is itself listed in the same array: total annotations and number of notes are therefore two different counts and neither stands in for the other. `form` answers 'what did the file already have filled in': /AcroForm -> /Fields -> /Kids, each field with the /T it wrote itself plus a qualified name joined from the ancestors (that join is ours, the spec defines the period), the effective /FT and /Ff with a boolean saying whether this dictionary wrote them or inherited them from its parent, /V and /DV decoded with the same three PDF-string rules as metadata, /MaxLen, and kids / parent / depth so a hierarchy is visible without flattening it. value_present is a separate key because 'wrote an empty /V' and 'wrote no /V' are different claims, and value_shape names all four ways a /V can be written: string; array (a multi-select list box - value stays null while the parts go out as value_parts rather than being joined into one string the file never wrote); other, i.e. a NAME, which is how checkboxes and radio groups write it (that name comes back as value_name); and null for no /V key at all. Whether a checkbox is ticked is three separate facts and none of them is folded into a boolean: the field's /V, the widget's /AS (the state it shows now, reported as as_state) and the names keyed in the /AP /N dictionary (ap_states - which states exist at all; empty when the widget carries no /AP, which is 'nothing said', not 'none'). A radio group writes /V on the parent while each kid widget writes its own /AS, and a kid whose /AS disagrees with the parent's /V is reported that way instead of being reconciled. options holds every string /Opt wrote, in written order, and options_shape says how that array was written - /Opt is array-only in the spec and two writings are legal: [(a) (b)] means the display value is also the export value (flat), [[export display] ...] means the two are written apart (pairs); mixed and empty name the other two shapes, and a field with no /Opt key at all is null rather than an empty list. Appearance streams are not computed and signatures are not validated - a /FT Sig or a /Sig key is reported as such and nothing more, the same line this reader holds for encrypted files. Caveat kept honest: the inheritance and hierarchy path is exercised on forms-hier.pdf, and that file is not editor output - no editor measured here writes /FT or /Ff on a parent field, so pikepdf wrote it. The counters therefore say what this file says, not what a real form tool exports; on the editor-made PDFs here the inheritance counters stay 0 because each field writes its own /FT.  Not provided: signature validation, the text of encrypted files (streams are ciphertext there), graphics-stream text, and CID fonts' widths, which live in a /W array this reader does not follow - such a font's characters are recognised but its spacing is not. font_embedding asks a different question than fonts: for each font dictionary it reports the subset prefix the producer cut into /BaseFont (null when the name has no plus sign, so a name is never upgraded into a subset claim), the /FontDescriptor object that THIS dictionary wrote (null when it wrote none) and which of /FontFile, /FontFile2, /FontFile3 that descriptor carries. Measured: the seven LibreOffice files are all TrueType with a prefix, a /FontFile2 and a /ToUnicode, and pdffonts says yes/yes/yes on the same object numbers; risk.pdf holds one Helvetica (Type1) that says nothing at all, which is why descriptor_missing is 1 there rather than some embedding failure. Two boundaries were measured too: objstm.pdf keeps all five font dictionaries inside object streams (only 17 plain objects), and locked.pdf is a case where pdffonts lists nothing while the dictionaries are plaintext, so five fonts with /FontFile2 are still reported - the third party staying silent and us reading are different facts, both kept. Not done: a Type0 (CID) font keeps its descriptor one hop further out, under /DescendantFonts, and none of the eight local PDFs has a Type0 font, so that hop is not written here - for a CID font this ledger says descriptor null, which means this dictionary wrote none, not that the file embeds nothing. "
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

    /// 连正文一起端出来（按页树的顺序逐行拼；加密的文件解不出，也照实说）
    #[arg(about = "Also extract each page's text, in /Kids order")]
    text: bool,

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

/// 「这张字体字典自己说了什么」：子集前缀、`/FontDescriptor` 在不在、descriptor 里有没有 FontFile*
///
/// 与上面那份 `fonts` 的分别是**问句**不同：那一份是「这份 PDF 用了哪些字体」，
/// 这一份是「它说没说自己带了字面数据」。两份各按各的读法交，不互相顶替。
fn font_embedding_report(doc: &pdf::Pdf, limit: usize) -> Value {
    let fonts = doc.fonts();
    let mut kinds: Vec<String> = fonts
        .iter()
        .filter_map(|one| one.font_file.clone())
        .collect();
    kinds.sort();
    kinds.dedup();
    let mut subtypes: Vec<String> = fonts
        .iter()
        .filter(|one| !one.subtype.is_empty())
        .map(|one| one.subtype.clone())
        .collect();
    subtypes.sort();
    subtypes.dedup();
    json!({
        "available": true,
        "fonts_total": fonts.len(),
        "with_descriptor": fonts.iter().filter(|one| one.descriptor.is_some()).count(),
        "descriptor_missing": fonts.iter().filter(|one| one.descriptor.is_none()).count(),
        "with_font_file": fonts.iter().filter(|one| one.font_file.is_some()).count(),
        "subsets": fonts.iter().filter(|one| one.subset_prefix.is_some()).count(),
        "with_to_unicode": fonts.iter().filter(|one| one.to_unicode).count(),
        "file_kinds": kinds,
        "subtypes": subtypes,
        "fonts": fonts.iter().take(limit).map(font_embedding_json).collect::<Vec<Value>>(),
    })
}

fn font_embedding_json(one: &pdf::Font) -> Value {
    json!({
        "object": one.id,
        "base_font": one.base_font,
        "subtype": one.subtype,
        "subset_prefix": match &one.subset_prefix {
            Some(text) => json!(text),
            None => Value::Null,
        },
        "descriptor": match one.descriptor {
            Some(num) => json!(num),
            None => Value::Null,
        },
        "font_file": match &one.font_file {
            Some(text) => json!(text),
            None => Value::Null,
        },
        "to_unicode": one.to_unicode,
        "encoding": one.encoding,
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

/// 书签这份账：`/Outlines` 的树与每条想去第几页。
/// 加密的件里标题是密文，那一律给 null —— 而对象号、层级、页号不是密文，照报得出
fn outline_report(doc: &Pdf, limit: usize) -> Value {
    let (items, declared, present) = doc.outlines();
    json!({
        "present": present,
        "declared_count": declared,
        "total": items.len(),
        "listed": items.len().min(limit),
        "items": items.iter().take(limit).map(|one| json!({
            "depth": one.depth,
            "title": one.title,
            "target_object": one.page,
            "page": one.page_index,
            "form": one.form,
            "via": one.via,
            "children": one.children,
            "closed": one.closed,
        })).collect::<Vec<Value>>(),
    })
}

/// 页内链接：往站外的给地址（加密的件里那是密文，给 null），往页内的是页对象号 +
/// 按 `/Kids` 顺序数出来的第几页
fn link_report(doc: &Pdf, limit: usize) -> Value {
    let links = doc.links();
    json!({
        "total": links.len(),
        "listed": links.len().min(limit),
        "internal": links.iter().filter(|one| one.to.is_some()).count(),
        "external": links.iter().filter(|one| one.form == "uri").count(),
        "other": links.iter().filter(|one| one.form != "uri" && one.to.is_none()).count(),
        "items": links.iter().take(limit).map(|one| json!({
            "page": one.page_index,
            "to_object": one.to,
            "to_page": one.to_index,
            "uri": one.uri,
            "via": one.form,
            "action": one.action,
        })).collect::<Vec<Value>>(),
    })
}

/// 页上的注记：链接、批注与它的弹出框都在同一本 `/Annots` 账里 —— 所以
/// 「几条注记」与「几条批注」是两个数，谁也不替谁圆场（一条 `/Text` 通常另配一条
/// `/Popup`，那条框自己也在数组里）。`by_subtype` 里那个空串是「整个没写 `/Subtype`」，
/// 与写了一个不认识的名字不是一回事，`no_subtype` 单数一本。
fn annotation_report(doc: &Pdf, limit: usize) -> Value {
    let all = doc.annotations();
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for one in &all {
        let key = one.subtype.clone().unwrap_or_default();
        match kinds.iter_mut().find(|hit| hit.0 == key) {
            Some(hit) => hit.1 += 1,
            None => kinds.push((key, 1)),
        }
    }
    kinds.sort();
    let of = |want: &str| {
        all.iter()
            .filter(|one| one.subtype.as_deref() == Some(want))
            .count()
    };
    json!({
        "total": all.len(),
        "listed": all.len().min(limit),
        "notes": of("Text"),
        "popups": of("Popup"),
        "links": of("Link"),
        "no_subtype": all.iter().filter(|one| one.subtype.is_none()).count(),
        "by_subtype": kinds
            .iter()
            .map(|(had, n)| json!({"subtype": had, "count": n}))
            .collect::<Vec<Value>>(),
        "items": all.iter().take(limit).map(|one| json!({
            "page": one.page_index,
            "object": one.object,
            "subtype": one.subtype,
            "author": one.author,
            "contents": one.contents,
            "modified": one.modified,
            "popup": one.popup,
            "parent": one.parent,
        })).collect::<Vec<Value>>(),
    })
}

/// 表单那一份账：`/AcroForm` 那一层与逐条字段。值只交文件写的，不替它算 appearance，
/// 也不校验签名 —— 签名那一条只认「`/FT` 是 Sig」与「`/Sig` 这个键在」两件事
fn form_report(doc: &Pdf, limit: usize) -> Value {
    let (form, fields) = doc.form();
    let named = fields.iter().filter(|one| one.value_present).count();
    json!({
        "present": form.present,
        "object": form.object,
        "roots": form.roots,
        "total": form.total,
        "listed": form.total.min(limit),
        "with_v": named,
        "default_appearance": form.default_appearance,
        "need_appearances": form.need_appearances,
        "sig_flags": form.sig_flags,
        "xfa": form.xfa,
        "by_type": {
            "text": fields.iter().filter(|one| one.field_type.as_deref() == Some("Tx")).count(),
            "button": fields.iter().filter(|one| one.field_type.as_deref() == Some("Btn")).count(),
            "choice": fields.iter().filter(|one| one.field_type.as_deref() == Some("Ch")).count(),
            "signature": fields.iter().filter(|one| one.field_type.as_deref() == Some("Sig")).count(),
            "unknown": fields.iter().filter(|one| one.field_type.is_none()).count(),
        },
        "inherited_type": fields.iter().filter(|one| one.type_inherited).count(),
        "inherited_flags": fields.iter().filter(|one| one.flags_inherited).count(),
        "widgets": fields.iter().filter(|one| one.widget).count(),
        "deepest": fields.iter().map(|one| one.depth).max().unwrap_or(0),
        "items": fields.iter().take(limit).map(|one| json!({
            "object": one.object,
            "depth": one.depth,
            "order": one.order,
            "partial": one.partial,
            "qualified": one.qualified,
            "type": one.field_type,
            "type_inherited": one.type_inherited,
            "value": one.value,
            "value_present": one.value_present,
            "value_shape": one.value_shape,
            "value_parts": one.value_parts,
            "value_name": one.value_name,
            "default": one.default_value,
            "flags": one.flags,
            "flags_inherited": one.flags_inherited,
            "max_len": one.max_len,
            "as_state": one.as_state,
            "ap_states": one.ap_states,
            "options": one.options,
            "options_shape": one.options_shape,
            "kids": one.kids,
            "parent": one.parent,
            "widget": one.widget,
            "subtype": one.subtype,
        })).collect::<Vec<Value>>(),
    })
}

/// `/P` 那一份权限账。规范把开关住在第 3 位起（不是第 0 位），R2 只有 3~6 位有意义，
/// R3 起才多出 9~12 位 —— 那几位在 R2 的文件里交回 null，不假装知道
fn permission_report(doc: &Pdf) -> Value {
    let Some(one) = doc.permission_bits() else {
        return Value::Null;
    };
    json!({
        "object": one.object,
        "revision": one.revision,
        "raw": one.raw,
        "print": one.print,
        "modify": one.modify,
        "copy": one.copy,
        "annotate": one.annotate,
        "forms": one.forms,
        "extract_for_screen": one.extract_for_screen,
        "assemble": one.assemble,
        "print_high_quality": one.print_high_quality,
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

    // 正文：`--text` 才解（内容流要逐条解压，结构性问题一次答完更快）
    let mut text_report = Value::Null;
    if app.text {
        let (pages_text, from_tree) = doc.page_texts(data);
        let mut per_page: Vec<Value> = Vec::new();
        let mut joined: Vec<String> = Vec::new();
        for (id, one) in pages_text {
            let lines: Vec<&str> = one.lines().filter(|line| !line.trim().is_empty()).collect();
            per_page.push(json!({
                "object": id,
                "lines": lines.len(),
                "chars": one.chars().count(),
                "text": one,
            }));
            joined.push(one);
        }
        // 只把有字的页拼起来：两份空页之间那个换行会凭空算成一个字符，
        // 于是「正文一个字没有」的文件自报 chars 1（加密那份就这样）
        joined.retain(|one| !one.is_empty());
        let all = joined.join("\n");
        if !from_tree {
            notes.push(
                "页树走不通（缺 /Root 或 /Kids 指向看不见的对象）：正文的页序退成按对象号排，\
                 与放映顺序可能不同"
                    .to_string(),
            );
        }
        if doc.encryption.is_some() {
            notes.push(
                "加密的文件里内容流是密文：这一份的正文解不出来，解不出来也不当成「这页没有字」"
                    .to_string(),
            );
        }
        text_report = json!({
            "order_from_page_tree": from_tree,
            "chars": all.chars().count(),
            "chars_no_spaces": all.chars().filter(|one| !one.is_whitespace()).count(),
            "lines": all.lines().filter(|one| !one.trim().is_empty()).count(),
            "pages": per_page,
            "text": all,
        });
    }

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
        // 「这张字体字典自己说没写descriptor / FontFile*」：与上面那份是**两个问句**
        "font_embedding": font_embedding_report(&doc, limit),
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
        "outline": outline_report(&doc, limit),
        "links": link_report(&doc, limit),
        "annotations": annotation_report(&doc, limit),
        "permissions": permission_report(&doc),
        "form": form_report(&doc, limit),
        "text": text_report,
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
            text: false,
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
        // 这五张子集字体**没有** `/Encoding` 这个键：`pdffonts` 那一列写 "WinAnsi" 是
        // xpdf 给 TrueType 补的默认值，不是文件里有的字。这边不替文件编一个编码出来
        assert!(
            fonts.iter().all(|one| one["encoding"] == json!("")),
            "{fonts:?}"
        );
        assert_eq!(out["images"].as_array().map(|one| one.len()), Some(2));
        // 正文里的脚注链接是一个 URI 批注；没有脚本、没有表单、没有附件
        assert_eq!(out["features"]["javascript"], json!(0));
        assert_eq!(out["features"]["uri_actions"], json!(1));
        assert_eq!(out["features"]["link_annotations"], json!(1));
        assert_eq!(out["features"]["attachments"], json!(0));
        assert_eq!(out["watch"], json!([]));
    }

    /// 书签这份账：LibreOffice 从标题写出的那棵树，两条都指向第 1 页，根上自报
    /// `/Count` 2（期望值来自 `lyco_pdf_nav.py`，也与 pikepdf 的对象走法对过）
    #[test]
    fn the_outline_tree_reports_which_page_each_entry_jumps_to() {
        let out = run("notes.pdf");
        let ol = &out["outline"];
        assert_eq!(ol["present"], json!(true), "{ol}");
        assert_eq!(ol["declared_count"], json!(2));
        assert_eq!(ol["total"], json!(2));
        let items = ol["items"].as_array().expect("是数组");
        assert_eq!(items[0]["title"], json!("一级标题：预算口径"));
        assert_eq!(items[0]["depth"], json!(0));
        assert_eq!(items[0]["page"], json!(1));
        assert_eq!(items[0]["form"], json!("explicit"));
        assert_eq!(items[0]["via"], json!("XYZ"));
        assert_eq!(items[0]["children"], json!(1));
        assert_eq!(items[1]["title"], json!("二级标题：明细"));
        assert_eq!(items[1]["depth"], json!(1), "第二条挂在第一条下面");
        assert_eq!(items[1]["page"], json!(1));
        // 换 Impress 那份：两条都在第 0 层，各指一页，顺序跟着 /Kids 走
        let deck = run("deck.pdf");
        let items = deck["outline"]["items"].as_array().expect("是数组");
        assert_eq!(items.len(), 2, "{deck}");
        assert_eq!(items[0]["page"], json!(1));
        assert_eq!(items[1]["page"], json!(2));
        assert_eq!(items[1]["depth"], json!(0));
    }

    /// 书签躲在对象流里的那种文件也要走得通（`objstm.pdf` 与 `notes.pdf` 是同一批字）
    #[test]
    fn the_outline_tree_survives_a_file_whose_objects_are_packed() {
        let out = run("objstm.pdf");
        let ol = &out["outline"];
        assert_eq!(ol["present"], json!(true), "{ol}");
        assert_eq!(ol["total"], json!(2));
        assert_eq!(ol["items"][0]["title"], json!("一级标题：预算口径"));
        assert_eq!(
            ol["items"][0]["target_object"],
            json!(2),
            "那份的页对象是 2 号"
        );
        assert_eq!(ol["items"][0]["page"], json!(1));
        assert_eq!(ol["items"][1]["depth"], json!(1));
    }

    /// 页内链接：往站外的给地址；`risk.pdf` 还有一条 `/Launch` —— 它既不去页也不去站外，
    /// 照实记成「别的动作」，不硬归进前两类
    #[test]
    fn link_targets_split_into_outbound_and_page_to_page() {
        let out = run("notes.pdf");
        let links = &out["links"];
        assert_eq!(links["total"], json!(1), "{links}");
        assert_eq!(links["external"], json!(1));
        assert_eq!(links["internal"], json!(0));
        assert_eq!(links["items"][0]["page"], json!(1));
        assert_eq!(
            links["items"][0]["uri"],
            json!("https://example.com/budget")
        );
        assert_eq!(links["items"][0]["via"], json!("uri"));
        assert_eq!(
            links["items"][0]["action"],
            Value::Null,
            "站外那条没有动作名"
        );
        let risk = run("risk.pdf");
        assert_eq!(risk["links"]["total"], json!(2), "{risk}");
        assert_eq!(risk["links"]["external"], json!(1));
        assert_eq!(risk["links"]["other"], json!(1), "/Launch 那一条哪儿也不去");
        // `/Annots[10 0 R 11 0 R]` 里先出现的是 /Launch 那条：站外地址是第二条。
        // `via` 说的是「哪一类去处」，动作本身记在 `action` —— 两条都只写 action
        // 就分不出哪一条会启动外部程序
        assert_eq!(risk["links"]["items"][0]["via"], json!("action"));
        assert_eq!(risk["links"]["items"][0]["action"], json!("Launch"));
        assert_eq!(risk["links"]["items"][0]["uri"], Value::Null);
        assert_eq!(
            risk["links"]["items"][1]["uri"],
            json!("https://example.invalid/doc")
        );
        assert_eq!(risk["outline"]["present"], json!(false), "这份没书签");
    }

    /// 权限位：`/P` 是**带符号**的整数，位号从 3 起。`perms.pdf` 关掉了打印/复制/批注/表单，
    /// 而 pdfinfo 把同一份读成 `print:no copy:no change:yes addNotes:no` —— 逐位一致。
    /// 加密字典本身不是密文，所以连 `locked.pdf`（要口令）也报得出这些位
    #[test]
    fn permission_bits_come_from_the_signed_p_value() {
        let out = run("perms.pdf");
        let p = &out["permissions"];
        assert_eq!(p["raw"], json!(-3384), "{p}");
        assert_eq!(p["revision"], json!(6));
        assert_eq!(p["print"], json!(false));
        assert_eq!(p["copy"], json!(false));
        assert_eq!(p["annotate"], json!(false));
        assert_eq!(p["forms"], json!(false));
        assert_eq!(
            p["modify"],
            json!(true),
            "改文档那位是开的（pdfinfo: change:yes）"
        );
        assert_eq!(p["assemble"], json!(false));
        assert_eq!(p["print_high_quality"], json!(false));
        // 没加密的件：这一项是 null，不假装有值
        assert!(run("notes.pdf")["permissions"].is_null());
        let locked = run("locked.pdf");
        assert_eq!(
            locked["permissions"]["raw"],
            json!(-1028),
            "加密字典不是密文"
        );
        assert_eq!(locked["permissions"]["print"], json!(true));
        assert_eq!(locked["permissions"]["assemble"], json!(false));
        // 但字符串是：书签条数、页号照报，标题与 URI 一律 null
        assert_eq!(locked["outline"]["total"], json!(2));
        assert_eq!(locked["outline"]["items"][0]["title"], Value::Null);
        assert_eq!(locked["outline"]["items"][0]["page"], json!(1));
        assert_eq!(locked["links"]["items"][0]["uri"], Value::Null);
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
            text: false,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_pdf(&app, &Context::new_test(tx)).unwrap_err();
        let text = why.to_string();
        assert!(text.contains("%PDF-"), "{text}");
    }

    /// `--text` 那条分支单独跑一遍。期望值来自 `lyco_pdf.py` 的实测，
    /// 并逐行与 `pdftotext`（xpdf 系，第三套代码）对过
    fn run_text(name: &str) -> Value {
        let app = OfficePdf {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
            text: true,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_pdf(&app, &Context::new_test(tx)).expect("office-pdf 应成功")
    }

    fn page_text(out: &Value, which: usize) -> String {
        out["text"]["pages"][which]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn page_text_comes_back_in_reading_order_not_stream_order() {
        let out = run_text("notes.pdf");
        assert_eq!(
            page_text(&out, 0),
            "一级标题：预算口径\n第三季度服务器预算为十二万四千元\n二级标题：明细\n科目 金额\n服务器 124000\n口径见 预算制度"
        );
        assert_eq!(page_text(&out, 1), "最后一页说明：数字为含税口径");
        assert_eq!(out["text"]["lines"], json!(7));
        assert_eq!(out["text"]["order_from_page_tree"], json!(true));
        // 「服务器」与右对齐的「124000」基线差 4.3 点：行容差跟字号走才合成一行
        assert!(page_text(&out, 0).contains("服务器 124000"), "{out}");
    }

    #[test]
    fn the_same_words_come_out_when_every_font_is_hidden_in_a_stream() {
        // objstm.pdf 就是 notes.pdf 经 qpdf 重存的：68 个对象里 51 个住在对象流里。
        // 正文这一层没接上对象流的话，这里会直接空掉
        let plain = run_text("notes.pdf");
        let packed = run_text("objstm.pdf");
        assert_eq!(page_text(&packed, 0), page_text(&plain, 0));
        assert_eq!(out_chars(&packed), out_chars(&plain));
        assert!(out_chars(&packed) > 40, "{packed}");
    }

    fn out_chars(out: &Value) -> i64 {
        out["text"]["chars"].as_i64().unwrap_or_default()
    }

    #[test]
    fn a_slide_page_keeps_its_bullets_on_their_own_lines() {
        let out = run_text("deck.pdf");
        assert_eq!(
            page_text(&out, 0),
            "预算评审\n• 新增两台 64 核应用服务器\n• 第二条要点"
        );
        assert_eq!(page_text(&out, 1), "第二页：数字\n科目 金额\n服务器 124000");
    }

    #[test]
    fn an_encrypted_pdf_says_the_text_is_not_readable_instead_of_calling_it_blank() {
        let out = run_text("locked.pdf");
        assert_eq!(out["text"]["chars"], json!(0));
        assert_eq!(out["text"]["text"], json!(""));
        // 页树不加密：页数照样报得出，正文解不出来是另一件事
        assert_eq!(out["text"]["pages"].as_array().map(Vec::len), Some(2));
        assert!(out["notes"]
            .as_array()
            .expect("说明")
            .iter()
            .any(|one| one.as_str().unwrap_or_default().contains("密文")));
    }

    #[test]
    fn text_is_absent_until_asked_for() {
        let out = run("notes.pdf");
        assert!(matches!(out.get("text"), Some(one) if one.is_null()));
    }

    /// 页上的注记：`/Annots` 那本账不止链接。一条批注带着自己的弹出框，而那个框
    /// 也在同一个数组里 —— 所以「几条注记」与「几条批注」是两个数。
    /// （期望值来自 `lyco_pdf.py` 与 pypdf 对这些字典的直接读取）
    #[test]
    fn a_note_on_a_page_goes_out_with_the_two_pointers_it_wrote() {
        let annots = run("pdf-comments.pdf")["annotations"].clone();
        assert_eq!(annots["total"], json!(3), "{annots}");
        assert_eq!(annots["notes"], json!(1));
        assert_eq!(annots["popups"], json!(1));
        assert_eq!(annots["links"], json!(1));
        assert_eq!(annots["no_subtype"], json!(0), "三条都自己写了名字");
        assert_eq!(annots["by_subtype"][0]["subtype"], "Link");
        assert_eq!(annots["by_subtype"][1]["subtype"], "Popup");
        assert_eq!(annots["by_subtype"][2]["subtype"], "Text");
        let note = &annots["items"][1];
        assert_eq!(note["page"], json!(2), "注记在第二页");
        assert_eq!(note["object"], json!(11));
        assert_eq!(note["subtype"], "Text");
        assert_eq!(
            note["author"], "liuqi, 09/23/26, ",
            "作者与日期被拼成了一条串"
        );
        assert_eq!(note["contents"], "这里要补上不含税口径");
        assert_eq!(
            note["modified"], "D:00000000000000Z",
            "全零那个日期原样交，不解也不编"
        );
        assert_eq!(note["popup"], json!(12));
        assert_eq!(note["parent"], Value::Null, "这一条自己没写 /Parent");
        let popup = &annots["items"][2];
        assert_eq!(popup["subtype"], "Popup");
        assert_eq!(popup["parent"], json!(11), "反向那一条指着批注");
        assert_eq!(popup["popup"], Value::Null);
        assert_eq!(popup["author"], Value::Null, "弹出框自己不带作者");
        assert_eq!(popup["contents"], Value::Null);

        // 生产者不带那个选项导同一份 docx，批注整个不见：只剩那一条链接
        let quiet = run("notes.pdf");
        assert_eq!(quiet["annotations"]["total"], json!(1), "只剩链接那一条");
        assert_eq!(
            quiet["annotations"]["notes"],
            json!(0),
            "0 是数过了没有，不是没看"
        );
        assert_eq!(quiet["annotations"]["popups"], json!(0));
        // 一条注记都没有的件：交 0，不是缺这个键
        assert_eq!(run("deck.pdf")["annotations"]["total"], json!(0));
    }

    /// 表单那一份账：`/AcroForm` → `/Fields` → `/Kids`，值只交文件写了的
    ///
    /// 期望值全部来自 `lyco_pdf.py` 的 `form_of`（同一份件两边各读一遍）
    #[test]
    fn the_form_ledger_reports_only_what_the_file_wrote() {
        let out = run("risk.pdf");
        let form = &out["form"];
        assert_eq!(form["present"], json!(true), "{form}");
        assert_eq!(form["object"], json!(12), "AcroForm 自己住在 12 号对象");
        assert_eq!(form["roots"], json!(1), "/Fields 数组顶层一条");
        assert_eq!(form["total"], json!(1));
        assert_eq!(form["with_v"], json!(1), "这一条自己写了 /V");
        assert_eq!(form["by_type"]["text"], json!(1));
        assert_eq!(form["by_type"]["button"], json!(0));
        assert_eq!(
            form["inherited_type"],
            json!(0),
            "这一条自己写了 /FT，没走继承"
        );
        assert_eq!(form["default_appearance"], Value::Null, "/DA 没写就是没写");
        assert_eq!(
            form["need_appearances"],
            Value::Null,
            "键不在，不替它填 false"
        );
        assert_eq!(form["sig_flags"], Value::Null);
        assert_eq!(form["xfa"], json!(false));
        let one = &form["items"][0];
        assert_eq!(one["object"], json!(15));
        assert_eq!(one["partial"], "name");
        assert_eq!(one["qualified"], "name", "只有一层时全名就是那一截");
        assert_eq!(one["type"], "Tx");
        assert_eq!(one["value"], "x");
        assert_eq!(one["value_present"], json!(true));
        assert_eq!(one["default"], Value::Null);
        assert_eq!(one["flags"], Value::Null, "这一条没写 /Ff");
        assert_eq!(one["options"], json!([]));
        assert_eq!(one["options_shape"], Value::Null, "整个没写 /Opt");
        assert_eq!(one["widget"], json!(true));
        assert_eq!(one["kids"], json!(0));
        assert_eq!(one["parent"], Value::Null);

        // 没有表单的件：present 是 false、几本账都是 0 —— 不是整个没有这个键
        for name in [
            "notes.pdf",
            "deck.pdf",
            "perms.pdf",
            "locked.pdf",
            "objstm.pdf",
        ] {
            let quiet = run(name);
            assert_eq!(quiet["form"]["present"], json!(false), "{name}");
            assert_eq!(quiet["form"]["total"], json!(0), "{name}");
            assert_eq!(quiet["form"]["with_v"], json!(0), "{name}");
        }
    }

    /// 三层字段、两种 `/Opt` 写法、勾选框那三处、`/V` 的四种写法 —— 继承那几条分支真的走了一遍
    ///
    /// 上一条形成的账只到「一层、自己写 /FT、没写 /Ff」为止，所以 `inherited_type` /
    /// `inherited_flags` 那两条一直停在「数过了，没有」。这份件（pikepdf 挂的，不是编辑器
    /// 导的）把 /FT 与 /Ff 只写在祖父上，于是两级孩子各自往上走一跳、两跳才拿到。
    /// 期望值全部来自 `lyco_pdf.py` 的 `form_of`，另由 pypdf 第三个读者数过一遍。
    #[test]
    fn a_three_level_form_inherits_and_reads_both_option_writings() {
        let deck = run("forms-hier.pdf");
        let form = &deck["form"];
        assert_eq!(form["roots"], json!(7), "/Fields 数组七条顶层");
        assert_eq!(form["total"], json!(12), "连子字段一共十二条");
        assert_eq!(form["listed"], json!(12));
        assert_eq!(
            form["with_v"],
            json!(7),
            "写了 /V 的条数（值可以是名、串或数组）"
        );
        assert_eq!(form["deepest"], json!(2), "/Kids 三层");
        assert_eq!(
            form["inherited_type"],
            json!(4),
            "两级孩子加单选的两个控件：四条都靠往上走"
        );
        assert_eq!(
            form["inherited_flags"],
            json!(4),
            "/Ff 同样只在根上写了一次"
        );
        assert_eq!(form["widgets"], json!(6), "六个控件同时挂在页的 /Annots 上");
        assert_eq!(form["by_type"]["text"], json!(4));
        assert_eq!(form["by_type"]["choice"], json!(2));
        assert_eq!(
            form["by_type"]["button"],
            json!(5),
            "两条勾选框加一个单选三条线"
        );
        assert_eq!(
            form["by_type"]["unknown"],
            json!(1),
            "Ghost 一家 /FT 都没写上"
        );
        assert_eq!(form["need_appearances"], json!(true));
        assert_eq!(form["sig_flags"], json!(1));
        assert_eq!(form["default_appearance"], "/Helv 0 Tf 0 g ");
        let items = form["items"].as_array().expect("是数组");
        assert_eq!(items.len(), 12);
        assert_eq!(items[0]["qualified"], "Person");
        assert_eq!(items[2]["qualified"], "Person.Address");
        assert_eq!(
            items[3]["qualified"], "Person.Address.City",
            "点号是我们拼的，规范定的就是这一种拼法"
        );
        assert_eq!(items[3]["type"], "Tx", "往上走两跳拿到的");
        assert_eq!(items[3]["type_inherited"], json!(true));
        assert_eq!(items[3]["flags"], json!(4), "继承来的开关，原值不改");
        assert_eq!(items[3]["value"], "杭州");
        assert_eq!(
            items[1]["flags"],
            json!(1),
            "First 自己写的 /Ff 盖掉父上那个 4"
        );
        assert_eq!(items[1]["type_inherited"], json!(false));
        assert_eq!(items[1]["max_len"], json!(4));
        assert_eq!(items[1]["default"], "李");
        // /Opt 的两种合法写法：成对（导出值与显示值分开）与摊平（同一串）
        assert_eq!(items[4]["options"], json!(["1", "一", "2", "二"]));
        assert_eq!(items[4]["options_shape"], "pairs");
        assert_eq!(items[5]["options"], json!(["甲", "乙", "丙"]));
        assert_eq!(items[5]["options_shape"], "flat");
        assert_eq!(
            items[0]["options_shape"],
            Value::Null,
            "没写，不是写了空数组"
        );
        // 值这一件：串、数组、名字、整个没写，是四件事
        assert_eq!(items[11]["value_present"], json!(true));
        assert_eq!(items[11]["value"], "", "/V 写了，写的是空串");
        assert_eq!(items[11]["value_shape"], "string");
        assert_eq!(
            items[5]["value_present"],
            json!(true),
            "多选的 /V 是一个数组"
        );
        assert_eq!(
            items[5]["value"],
            Value::Null,
            "几段值不并成一个串：那一份在 value_parts 上"
        );
        assert_eq!(items[5]["value_shape"], "array");
        assert_eq!(items[5]["value_parts"], json!(["甲", "丙"]));
        assert_eq!(items[0]["value_present"], json!(false), "父上整个没写 /V");
        assert_eq!(items[0]["value_shape"], Value::Null);
        // 勾选框：/V 是一个名字（不是字符串），当前显示哪一种在控件的 /AS 上，
        // 可显示的那几种在 /AP 的 /N 字典的键上 —— 三处各交各的，不合成一个布尔
        assert_eq!(items[6]["value_shape"], "other", "/V /Yes 是名字不是串");
        assert_eq!(items[6]["value_name"], "Yes");
        assert_eq!(items[6]["as_state"], "Yes");
        assert_eq!(items[6]["ap_states"], json!(["Off", "Yes"]));
        assert_eq!(items[6]["type"], "Btn");
        assert_eq!(items[7]["value_name"], Value::Null, "第二条整个没写 /V");
        assert_eq!(items[7]["as_state"], "Off", "但控件说了它现在是 Off");
        assert_eq!(items[7]["ap_states"], json!(["Off", "Yes"]));
        // 单选：父上写 /V /One，两个孩子各是自己的控件、各写 /AS —— 有一个对不上，那是文件的话
        assert_eq!(items[8]["qualified"], "Pick");
        assert_eq!(items[8]["value_name"], "One");
        assert_eq!(
            items[8]["as_state"],
            Value::Null,
            "父上没有显示状态，那在控件上"
        );
        assert_eq!(items[9]["qualified"], "Pick.On");
        assert_eq!(items[9]["as_state"], "One");
        assert_eq!(items[9]["type_inherited"], json!(true), "/FT /Btn 写在父上");
        assert_eq!(items[9]["ap_states"], json!(["One", "Two"]));
        assert_eq!(items[10]["as_state"], "Two");
        assert_eq!(items[10]["ap_states"], json!([]), "这一个控件没带 /AP");
    }

    /// 「这张字体字典自己说了什么」：子集前缀、`/FontDescriptor` 在不在、里面有没有 `FontFile*`。
    /// 期望值来自 `lyco_pdf.py:font_embedding_of`，另用 `pdffonts` 的 emb/sub/uni 三列对质过
    /// （LibreOffice 那七份每张都是 yes/yes/yes，`risk.pdf` 那一张 Helvetica 是 no/no/no）。
    #[test]
    fn a_subset_font_says_it_carries_the_outline_and_helvetica_does_not() {
        let deck = run("deck.pdf");
        let emb = &deck["font_embedding"];
        assert_eq!(emb["available"], json!(true));
        assert_eq!(emb["fonts_total"], 6);
        assert_eq!(emb["with_descriptor"], 6);
        assert_eq!(emb["descriptor_missing"], 0);
        assert_eq!(emb["with_font_file"], 6);
        assert_eq!(emb["subsets"], 6);
        assert_eq!(emb["with_to_unicode"], 6);
        assert_eq!(emb["file_kinds"], json!(["FontFile2"]));
        assert_eq!(emb["subtypes"], json!(["TrueType"]));
        let rows = emb["fonts"].as_array().expect("是数组");
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["object"], 40);
        assert_eq!(rows[0]["base_font"], "EAAAAA+Calibri");
        assert_eq!(rows[0]["subset_prefix"], "EAAAAA");
        assert_eq!(rows[0]["descriptor"], 38);
        assert_eq!(rows[0]["font_file"], "FontFile2");
        assert_eq!(rows[0]["to_unicode"], json!(true));
        // 同一份 PDF 里 `fonts` 与 `font_embedding` 是两个问句：一份列名字，一份列「说不说自己带了字面数据」
        assert_eq!(deck["fonts"].as_array().expect("是数组").len(), 6);
        assert!(deck["fonts"][0].get("subset_prefix").is_none());
        assert!(rows[0].get("from_object_stream").is_none());

        // 反面凭据：标准 14 字体那份什么都没说 —— descriptor 与 font_file 都是 null（不是 0）
        let risk = run("risk.pdf");
        let one = &risk["font_embedding"];
        assert_eq!(one["fonts_total"], 1);
        assert_eq!(one["with_descriptor"], 0);
        assert_eq!(one["descriptor_missing"], 1);
        assert_eq!(one["with_font_file"], 0);
        assert_eq!(one["subsets"], 0);
        assert_eq!(one["with_to_unicode"], 0);
        assert_eq!(one["file_kinds"], json!([]));
        assert_eq!(one["subtypes"], json!(["Type1"]));
        let rrows = one["fonts"].as_array().expect("是数组");
        assert_eq!(rrows[0]["base_font"], "Helvetica");
        assert_eq!(rrows[0]["subset_prefix"], Value::Null);
        assert_eq!(rrows[0]["descriptor"], Value::Null);
        assert_eq!(rrows[0]["font_file"], Value::Null);

        // 字体字典住在对象流里的那些，照样数得清（这一族不追 xref 的第二层就在这）
        let packed = run("objstm.pdf");
        assert_eq!(packed["font_embedding"]["fonts_total"], 5);
        assert_eq!(packed["font_embedding"]["with_font_file"], 5);
        assert_eq!(packed["font_embedding"]["subsets"], 5);
        // 加密那份：pdffonts 一个字都不列，而字典是明文可读的 —— 两边各说各的，不互相顶替
        let locked = run("locked.pdf");
        assert_eq!(locked["font_embedding"]["fonts_total"], 5);
        assert_eq!(locked["font_embedding"]["with_font_file"], 5);
    }
}
