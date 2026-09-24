//! 办公文件的「这是什么」层：把字节分进四类容器，并把取部件 / 取流的入口收在一处。
//!
//! 四类容器就四件事要说清楚：
//! - **OOXML**（.docx / .xlsx / .pptx / .docm / …）：ZIP + OPC，正文在固定名字的部件里；
//!   分 Word / Excel / PowerPoint 靠的是**部件名**而不是扩展名 —— 改了后缀的文件照样能认出来；
//! - **ODF**（.odt / .ods / .odp）：也是 ZIP，但没有 OPC 的关系表，包清单在
//!   `META-INF/manifest.xml`，类型靠 `mimetype` 这个第一个成员（它是 stored 的）；
//! - **MS-CFB**（.doc / .xls / .ppt）：复合文档，正文与属性都是「流」，见 [`crate::cfb`]；
//! - **RTF**：不是容器，是文本协议，见 [`crate::rtf`]。
//!
//! 还有一类必须单独报出来而不是混进「Word 文件」：**加密**。OOXML 的加密包只有一层
//! `EncryptedPackage` 流（正文是解不开的），CFB 的旧式加密看 `EncryptionInfo`；
//! 本域不解密（没有口令，也不该有），但一定要说出来 —— 把打不开说成「没有正文」是假答案。

use serde_json::{json, Value};

use crate::cfb::{self, Cfb};
use crate::read::{central_directory, ZipEntry};

/// 哪一大家族：决定了取正文走哪条路
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// OOXML 包（OPC：有 [Content_Types].xml）
    Ooxml,
    /// OpenDocument 包（靠 mimetype 与 META-INF/manifest.xml）
    Odf,
    /// 普通 ZIP（有成员表但不是办公包）
    Zip,
    /// 复合文档（.doc / .xls / .ppt）
    Compound,
    /// RTF
    Rtf,
    /// 认不出来
    Other,
}

/// 一次打开的结果
#[derive(Debug)]
pub struct Doc {
    pub family: Family,
    /// 具体格式名：docx / xlsx / pptx / odt / doc / rtf / …
    pub format: String,
    /// 面向哪个应用：word / excel / powerpoint / opendocument / unknown
    pub app: &'static str,
    /// OPC / ODF 的成员表（zip 家族才有）
    pub entries: Vec<ZipEntry>,
    /// 复合文档（CFB 家族才有）
    pub compound: Option<Cfb>,
    /// `[Content_Types].xml` 没覆盖到的部件（OPC 才有意义）
    pub untyped: Vec<String>,
    /// 这个包/容器自己说的不自洽之处
    pub notes: Vec<String>,
}

impl Doc {
    pub fn is_zip_family(&self) -> bool {
        matches!(self.family, Family::Ooxml | Family::Odf | Family::Zip)
    }

    pub fn has_part(&self, want: &str) -> bool {
        crate::zipread::find_in(&self.entries, want).is_some()
    }

    /// 按内容类型看某个部件在不在（`Override` 与 `Default` 都算声明过）
    pub fn parts_under(&self, prefix: &str) -> Vec<&ZipEntry> {
        self.entries
            .iter()
            .filter(|one| one.name.starts_with(prefix))
            .collect()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "family": family_name(self.family),
            "format": self.format,
            "app": self.app,
            "parts": self.entries.len(),
            "streams": self.compound.as_ref().map(|one| one.stream_count()),
            "notes": self.notes,
        })
    }
}

/// derive 的取值口径：Web / MCP / JSON 端省略某个数时拿到的是 **0**，不是 schema 里的
/// `default`（那个只给 CLI 解析与 --help 用）。所以 0 必须回退成缺省值 —— 否则「省略 limit」
/// 就变成「只要 0 条」，交出一份被悄悄砍短的答案。
pub fn take_limit(raw: u64, fallback: usize) -> usize {
    if raw == 0 {
        fallback
    } else {
        usize::try_from(raw).unwrap_or(fallback)
    }
}

pub fn family_name(family: Family) -> &'static str {
    match family {
        Family::Ooxml => "ooxml",
        Family::Odf => "opendocument",
        Family::Zip => "zip",
        Family::Compound => "compound",
        Family::Rtf => "rtf",
        Family::Other => "unknown",
    }
}

pub fn is_rtf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"{\\rtf")
}

/// 打开：只读头与成员表 / 目录，不解任何部件（那是各命令自己的事）
pub fn open(bytes: &[u8]) -> Doc {
    if is_rtf(bytes) {
        return Doc {
            family: Family::Rtf,
            format: "rtf".to_string(),
            app: "word",
            entries: Vec::new(),
            compound: None,
            untyped: Vec::new(),
            notes: Vec::new(),
        };
    }
    if cfb::is_cfb(bytes) {
        return match cfb::open(bytes) {
            Ok(one) => {
                let names: Vec<String> = one.stream_names();
                let app = if names.iter().any(|one| one == "WordDocument") {
                    "word"
                } else if names.iter().any(|one| one == "Workbook" || one == "Book") {
                    "excel"
                } else if names.iter().any(|one| one == "PowerPoint Document") {
                    "powerpoint"
                } else {
                    "unknown"
                };
                let format = match app {
                    "word" => "doc".to_string(),
                    "excel" => "xls".to_string(),
                    "powerpoint" => "ppt".to_string(),
                    _ => "compound".to_string(),
                };
                Doc {
                    family: Family::Compound,
                    format,
                    app,
                    entries: Vec::new(),
                    compound: Some(one),
                    untyped: Vec::new(),
                    notes: Vec::new(),
                }
            }
            Err(why) => Doc {
                family: Family::Compound,
                format: "compound".to_string(),
                app: "unknown",
                entries: Vec::new(),
                compound: None,
                untyped: Vec::new(),
                notes: vec![why],
            },
        };
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        let (entries, broken) = central_directory(bytes);
        let names: Vec<&str> = entries.iter().map(|one| one.name.as_str()).collect();
        let ooxml = names.iter().any(|one| {
            [
                "word/document.xml",
                "xl/workbook.xml",
                "ppt/presentation.xml",
            ]
            .contains(one)
        });
        let has_content_types = names.iter().any(|one| *one == "[Content_Types].xml");
        let odf = names.iter().any(|one| *one == "mimetype")
            && names.iter().any(|one| *one == "content.xml");
        let family = if ooxml || has_content_types {
            Family::Ooxml
        } else if odf {
            Family::Odf
        } else {
            Family::Zip
        };
        let (app, format) = classify_zip(family, &names, bytes);
        let untyped = if family == Family::Ooxml {
            content_type_gaps(bytes, &entries)
        } else {
            Vec::new()
        };
        return Doc {
            family,
            format,
            app,
            entries,
            compound: None,
            untyped,
            notes: broken,
        };
    }
    if crate::pdf::is_pdf(bytes) {
        // PDF 认得出来，但它不在这四类容器里：没有部件表、没有流目录，
        // 是一张对象表加若干条流。所以家族仍是 Other（各 office 命令的分支不会被它误触发），
        // 但格式要说清是 pdf，并把该走的门指给人 —— `lbin office-pdf`。
        return Doc {
            family: Family::Other,
            format: "pdf".to_string(),
            app: "pdf",
            entries: Vec::new(),
            compound: None,
            untyped: Vec::new(),
            notes: vec![
                "PDF：不是包也不是复合文档，结构走 lbin office-pdf（对象表、页树、字体与动作）"
                    .to_string(),
            ],
        };
    }
    Doc {
        family: Family::Other,
        format: "unknown".to_string(),
        app: "unknown",
        entries: Vec::new(),
        compound: None,
        untyped: Vec::new(),
        notes: vec!["既不是 zip 也不是复合文档，也不是 RTF".to_string()],
    }
}

/// zip 家族的具体格式：先按部件名分应用，再按「有没有宏部件 / 加密部件」分后缀；
/// ODF 靠第一个成员 `mimetype` 里的媒体类型（那是规范规定要 stored 放在最前面的东西）
fn classify_zip(family: Family, names: &[&str], bytes: &[u8]) -> (&'static str, String) {
    let has = |want: &str| names.iter().any(|one| *one == want);
    match family {
        Family::Ooxml => {
            let encrypted = has("EncryptedPackage") || has("encryptionInfo");
            let macro_hint = has("word/vbaProject.bin") || has("word/vbaProject");
            if encrypted {
                return ("unknown", "ooxml(encrypted)".to_string());
            }
            if has("word/document.xml") {
                let template = has("word/_rels/document.xml.rels")
                    && names.iter().any(|one| *one == "word/styles.xml");
                let _ = template;
                let format = if macro_hint || has("word/vbaSignature.xml") {
                    "docm"
                } else {
                    "docx"
                };
                return ("word", format.to_string());
            }
            if has("xl/workbook.xml") {
                let format = if macro_hint || has("xl/vbaProject.bin") {
                    "xlsm"
                } else {
                    "xlsx"
                };
                return ("excel", format.to_string());
            }
            if has("ppt/presentation.xml") {
                let format = if macro_hint || has("ppt/vbaProject.bin") {
                    "pptm"
                } else {
                    "pptx"
                };
                return ("powerpoint", format.to_string());
            }
            ("unknown", "ooxml".to_string())
        }
        Family::Odf => {
            let media = odf_mimetype(bytes);
            let app = if media.contains("text") {
                "word"
            } else if media.contains("spreadsheet") {
                "excel"
            } else if media.contains("presentation") || media.contains("drawing") {
                "powerpoint"
            } else {
                "opendocument"
            };
            let ext = if media.ends_with("text") {
                "odt"
            } else if media.ends_with("spreadsheet") {
                "ods"
            } else if media.ends_with("presentation") {
                "odp"
            } else {
                "odf"
            };
            (app, ext.to_string())
        }
        _ => ("unknown", "zip".to_string()),
    }
}

/// `mimetype` 的正文（ODF 规定它是第一个成员且不压缩，所以直接扫本地头就能拿到）
fn odf_mimetype(bytes: &[u8]) -> String {
    let Ok(member) = crate::zipread::member(bytes, "mimetype", 1024) else {
        return String::new();
    };
    member.as_text()
}

/// 这个包里没有任何内容类型声明的部件（OPC 的自证之一）
pub fn untyped_parts(doc: &Doc) -> Vec<String> {
    doc.untyped.clone()
}

/// `[Content_Types].xml` 的两种声明都要认：`Default`（按扩展名）与 `Override`（按部件名）。
/// 只认一种就会把整包部件报成「没声明」，那种假阳性的害处比漏报更大。
fn content_type_gaps(bytes: &[u8], entries: &[ZipEntry]) -> Vec<String> {
    let Ok(member) = crate::zipread::member(
        bytes,
        "[Content_Types].xml",
        crate::zipread::DEFAULT_MEMBER_CAP,
    ) else {
        return Vec::new();
    };
    let node = crate::xmlscan::parse_str(&member.as_text());
    let mut defaults: Vec<String> = Vec::new();
    for one in node.descendants("Default") {
        if let Some(ext) = one.attr("Extension") {
            defaults.push(ext.to_lowercase());
        }
    }
    let mut overrides: Vec<String> = Vec::new();
    for one in node.descendants("Override") {
        if let Some(part) = one.attr("PartName") {
            overrides.push(part.to_string());
        }
    }
    let mut gaps: Vec<String> = Vec::new();
    for one in entries {
        if one.name == "[Content_Types].xml" || one.name.ends_with('/') {
            continue;
        }
        if overrides
            .iter()
            .any(|want| *want == format!("/{}", one.name))
        {
            continue;
        }
        let last = one.name.rsplit('/').next().unwrap_or("").to_string();
        let ext = match last.rfind('.') {
            Some(at) => last[at + 1..].to_lowercase(),
            None => String::new(),
        };
        if !ext.is_empty() && defaults.contains(&ext) {
            continue;
        }
        gaps.push(one.name.clone());
    }
    gaps
}

/// 这个包里跟「宏 / 加密 / 签名 / 外部数据」有关的东西，供各命令共用一份说法
pub fn risk_signals(doc: &Doc) -> Value {
    let mut macros: Vec<String> = Vec::new();
    let mut encrypted: Vec<String> = Vec::new();
    let mut signed: Vec<String> = Vec::new();
    let mut external: usize = 0;
    if doc.is_zip_family() {
        for one in &doc.entries {
            let name = one.name.as_str();
            if name.ends_with("vbaProject.bin") || name.ends_with("VBA/project.bin") {
                macros.push(name.to_string());
            }
            if name == "EncryptedPackage" || name == "encryptionInfo" {
                encrypted.push(name.to_string());
            }
            // 签名与加密是两件事：vbaSignature.xml / 数字签名关系说「谁签的」，
            // 不说「内容被加密」——混在一起就会把一份能读的宏文件报成读不了
            if name.ends_with("/vbaSignature.xml")
                || name.contains("digitalSignature")
                || name.contains("_signature")
                || (name.ends_with("_rels/.rels") && name.contains("package"))
            {
                signed.push(name.to_string());
            }
        }
        external = doc
            .entries
            .iter()
            .filter(|one| one.name.starts_with("xl/externalLinks/"))
            .count();
    }
    if let Some(one) = doc.compound.as_ref() {
        for name in one.stream_names() {
            if name.eq_ignore_ascii_case("Macros")
                || name.contains("VBA")
                || name.contains("_VBA_PROJECT")
            {
                macros.push(name.clone());
            }
            if name.contains("Encrypted") {
                encrypted.push(name);
            }
        }
    }
    json!({
        "macro_parts": macros,
        "has_macros": !macros.is_empty(),
        "encrypted_parts": encrypted,
        "is_encrypted": !encrypted.is_empty(),
        "signature_parts": signed,
        "external_link_parts": external,
    })
}

// ── 包级别的关系表与内容类型 --------------------------------------------------
//
// OPC 的关系表（`_rels/*.rels`）是「谁指着谁」的唯一来源，而它有两个必须自己处理的点：
// - 目标要**按源部件解析**：`word/_rels/document.xml.rels` 里的 `media/image1.png`
//   指的是 `word/media/image1.png`，不是包根的 `media/image1.png`；写成 `/word/...`
//   的又是从根算起。解错了，「关系指向不存在的部件」这条检查就只会误报。
// - `TargetMode="External"` 的目标**不在包里**，不能拿去查存在性 —— 那正是「文档里有站外链接」
//   这一类信息本身。
//
// 内容类型有两种声明（`Default` 按扩展名、`Override` 按部件名），只认一种就会把整包
// 报成「没声明类型」。ODF 不用 OPC：它的包清单是 `META-INF/manifest.xml`，
// 那里只说「哪个文件是什么媒体类型」，没有跨部件关系。

use std::collections::BTreeMap;

use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 一条关系，target 已经解析成包内路径（外部关系保留原文）
#[derive(Debug, Clone)]
pub struct Rel {
    /// 关系表所属的部件，比如 `word/document.xml`
    pub source: String,
    pub id: String,
    /// 类型的最后一段：`hyperlink` / `image` / `oleObject` / `attachedTemplate` / …
    pub kind: String,
    pub target: String,
    pub external: bool,
    /// 解析到包内路径（外部关系为 None）
    pub resolved: Option<String>,
}

/// 读全部 `.rels`；同时把「关系表自己指着不存在的源部件」这类问题留在 notes 里
pub fn relationships(bytes: &[u8], entries: &[ZipEntry]) -> (Vec<Rel>, Vec<String>) {
    let mut out: Vec<Rel> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let names: Vec<&str> = entries.iter().map(|one| one.name.as_str()).collect();
    for one in entries {
        if !one.name.ends_with(".rels") {
            continue;
        }
        let Ok(member) = zipread::read_member(bytes, one, DEFAULT_MEMBER_CAP) else {
            notes.push(format!("关系表 {} 解不出来", one.name));
            continue;
        };
        let root = xmlscan::parse_str(&member.as_text());
        let base = rel_base(&one.name);
        for rel in root.descendants("Relationship") {
            let Some(target) = rel.attr("Target") else {
                notes.push(format!("{} 里有一条关系没有 Target", one.name));
                continue;
            };
            let external = rel.attr("TargetMode") == Some("External");
            let resolved = if external {
                None
            } else {
                let path = resolve_target(&base, target);
                if names.contains(&path.as_str()) {
                    Some(path)
                } else {
                    notes.push(format!(
                        "{} 的关系 {} 指着包里没有的部件 `{}`",
                        one.name,
                        rel.attr("Id").unwrap_or("?"),
                        path
                    ));
                    None
                }
            };
            out.push(Rel {
                source: rel_source(&one.name),
                id: rel.attr("Id").unwrap_or_default().to_string(),
                kind: rel
                    .attr("Type")
                    .unwrap_or_default()
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string(),
                target: target.to_string(),
                external,
                resolved,
            });
        }
    }
    (out, notes)
}

// `word/_rels/document.xml.rels` 的宿主部件所在目录 → `word`；包根的 `_rels/.rels` → ""
fn rel_base(rels_name: &str) -> String {
    match rels_name.rsplit_once("/_rels/") {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

// 关系表属于哪个部件：`word/_rels/document.xml.rels` 的宿主是 `word/document.xml`，
// `_rels/.rels` 属于包根（记成空串，检查时不参与「源部件在不在」的判断）
fn rel_source(rels_name: &str) -> String {
    let without = rels_name.trim_end_matches(".rels");
    match without.rsplit_once("/_rels/") {
        Some((dir, file)) if !file.is_empty() => format!("{dir}/{file}"),
        _ => String::new(),
    }
}

/// 按 OPC 规则解析目标：以 `/` 开头从包根算，否则相对源部件所在目录
pub fn resolve_target(base: &str, target: &str) -> String {
    let cleaned = target.trim_start_matches('/');
    if target.starts_with('/') || base.is_empty() {
        return normalize(cleaned);
    }
    normalize(&format!("{base}/{cleaned}"))
}

/// 消掉 `./` 与 `a/../b`：打包器写法不统一，但包内路径只有一个真相
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for one in path.split('/') {
        match one {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(one),
        }
    }
    parts.join("/")
}

/// `[Content_Types].xml` 的两张表
#[derive(Debug, Default)]
pub struct ContentTypes {
    pub defaults: BTreeMap<String, String>,
    pub overrides: BTreeMap<String, String>,
    pub notes: Vec<String>,
}

impl ContentTypes {
    pub fn read(bytes: &[u8]) -> ContentTypes {
        let mut out = ContentTypes::default();
        let Ok(member) = zipread::member(bytes, "[Content_Types].xml", DEFAULT_MEMBER_CAP) else {
            out.notes.push("包里没有 [Content_Types].xml".to_string());
            return out;
        };
        let root = xmlscan::parse_str(&member.as_text());
        for one in root.descendants("Default") {
            if let (Some(ext), Some(ty)) = (one.attr("Extension"), one.attr("ContentType")) {
                out.defaults.insert(ext.to_lowercase(), ty.to_string());
            }
        }
        for one in root.descendants("Override") {
            if let (Some(part), Some(ty)) = (one.attr("PartName"), one.attr("ContentType")) {
                out.overrides
                    .insert(part.trim_start_matches('/').to_string(), ty.to_string());
            }
        }
        out
    }

    /// 某个部件声明的类型；没声明就 None（调用方决定这是不是错误）
    pub fn of(&self, part: &str) -> Option<&str> {
        if let Some(one) = self.overrides.get(part.trim_start_matches('/')) {
            return Some(one.as_str());
        }
        let last = part.rsplit('/').next().unwrap_or(part);
        let ext = match last.rfind('.') {
            Some(at) => last[at + 1..].to_lowercase(),
            None => return None,
        };
        self.defaults.get(&ext).map(|one| one.as_str())
    }
}

/// ODF 的包清单：`META-INF/manifest.xml` 的 `manifest:file-entry`
pub fn odf_manifest(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(member) = zipread::member(bytes, "META-INF/manifest.xml", DEFAULT_MEMBER_CAP) else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&member.as_text());
    root.descendants("file-entry")
        .iter()
        .filter_map(|one| {
            // 清单里的属性写作 `manifest:full-path` / `manifest:media-type`
            let path = one.attr_local("full-path")?;
            let media = one.attr_local("media-type").unwrap_or_default();
            Some((path.to_string(), media.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod rel_tests {
    use super::*;

    #[test]
    fn rel_paths_resolve_against_their_source_part() {
        assert_eq!(rel_base("word/_rels/document.xml.rels"), "word");
        assert_eq!(rel_base("_rels/.rels"), "");
        assert_eq!(rel_base("xl/_rels/workbook.xml.rels"), "xl");
        assert_eq!(
            resolve_target("word", "media/image1.png"),
            "word/media/image1.png"
        );
        assert_eq!(
            resolve_target("word", "/docProps/core.xml"),
            "docProps/core.xml"
        );
        assert_eq!(
            resolve_target("ppt/slides", "../slideLayouts/slideLayout1.xml"),
            "ppt/slideLayouts/slideLayout1.xml"
        );
        assert_eq!(
            rel_source("word/_rels/document.xml.rels"),
            "word/document.xml"
        );
        assert_eq!(rel_source("_rels/.rels"), "");
    }

    /// 真包真表：notes.docx 的每条内部关系都要指到一个真实存在的部件
    #[test]
    fn every_internal_relationship_in_a_real_package_resolves() {
        let raw = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
        )
        .expect("读 fixture");
        let (dirs, _) = crate::read::central_directory(&raw);
        let (rels, notes) = relationships(&raw, &dirs);
        assert!(!rels.is_empty(), "关系表要读得出条目");
        assert!(notes.is_empty(), "{notes:?}");
        let external: Vec<&Rel> = rels.iter().filter(|one| one.external).collect();
        assert_eq!(external.len(), 1, "文档里就一个站外超链接：{external:?}");
        assert_eq!(external[0].kind, "hyperlink");
        assert_eq!(external[0].target, "https://example.com/budget");
        assert!(rels
            .iter()
            .any(|one| one.kind == "image"
                && one.resolved.as_deref() == Some("word/media/image1.png")));
        // custom.xml 那组关系是真包里的另一类，也要能解析
        assert!(rels.iter().any(|one| one.kind == "customXml"));
    }

    /// 断头关系必须抓到：只验真包等于没验这条检查
    #[test]
    fn a_relationship_pointing_at_nothing_is_reported() {
        let raw = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
        )
        .expect("读 fixture");
        let (mut dirs, _) = crate::read::central_directory(&raw);
        dirs.retain(|one| one.name != "word/media/image1.png");
        let (rels, notes) = relationships(&raw, &dirs);
        assert!(
            notes
                .iter()
                .any(|one| one.contains("word/media/image1.png")),
            "删掉一个被指着的文件，检查应当抓到：{notes:?}；关系={rels:?}"
        );
        assert!(
            rels.iter()
                .any(|one| one.kind == "image" && one.resolved.is_none()),
            "解析不出的那条要留下 resolved=None"
        );
    }

    /// 内容类型：Defaults 与 Overrides 都要认
    #[test]
    fn content_types_come_from_both_tables() {
        let raw = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
        )
        .expect("读 fixture");
        let types = ContentTypes::read(&raw);
        assert_eq!(
            types.of("word/document.xml"),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            ),
            "{:?}",
            types.overrides
        );
        assert_eq!(
            types.of("word/media/image1.png"),
            Some("image/png"),
            "png 是 Default 那条"
        );
        assert_eq!(types.of("word/nothing.xyz"), None);
    }

    /// ODF 的清单是另一套：没有 .rels，只有 manifest 的 file-entry
    #[test]
    fn odf_manifest_lists_its_own_parts() {
        let raw = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.odt"),
        )
        .expect("读 fixture");
        let entries = odf_manifest(&raw);
        assert!(
            entries
                .iter()
                .any(|(path, media)| path == "/"
                    && media == "application/vnd.oasis.opendocument.text"),
            "{entries:?}"
        );
        assert!(entries.iter().any(|(path, _)| path == "content.xml"));
        let (dirs, _) = crate::read::central_directory(&raw);
        let (rels, _) = relationships(&raw, &dirs);
        assert!(rels.is_empty(), "ODF 不该被当成 OPC 读出关系：{rels:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn bytes_of(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    /// 十一种真实生产者文件都要落到对的家族与格式上 —— 这条表就是本模块的全部承诺
    #[test]
    fn classifies_every_producer_fixture() {
        let want = [
            ("notes.docx", Family::Ooxml, "word", "docx"),
            ("notes.docm", Family::Ooxml, "word", "docm"),
            ("book.xlsx", Family::Ooxml, "excel", "xlsx"),
            ("deck.pptx", Family::Ooxml, "powerpoint", "pptx"),
            ("notes.odt", Family::Odf, "word", "odt"),
            ("book.ods", Family::Odf, "excel", "ods"),
            ("deck.odp", Family::Odf, "powerpoint", "odp"),
            ("notes.doc", Family::Compound, "word", "doc"),
            ("book.xls", Family::Compound, "excel", "xls"),
            ("deck.ppt", Family::Compound, "powerpoint", "ppt"),
            ("notes.rtf", Family::Rtf, "word", "rtf"),
        ];
        for (name, family, app, format) in want {
            let doc = open(&bytes_of(name));
            assert_eq!(
                doc.family,
                family,
                "{name} 家族判错：{:?} {}",
                doc.family,
                doc.notes.join("；")
            );
            assert_eq!(doc.app, app, "{name} 应用判错");
            assert_eq!(doc.format, format, "{name} 格式判错");
        }
    }

    /// 改了后缀不许改结论：识别看的是部件名
    #[test]
    fn renaming_the_extension_does_not_change_the_answer() {
        let doc = open(&bytes_of("notes.docx"));
        assert_eq!(doc.format, "docx");
        // 同一批字节按 .zip 的名字读进来还是 docx：判据里没有文件名
        let raw = bytes_of("notes.docx");
        assert_eq!(open(&raw).app, "word");
    }

    /// 0 是「各端省略了这个数」的表示法，不是「要零条」
    #[test]
    fn zero_means_unset_for_limits_and_byte_caps() {
        assert_eq!(take_limit(0, 200), 200);
        assert_eq!(take_limit(7, 200), 7);
        // max_bytes 不归这里管：read_blob 已经定了 0 = 不设上限（见 read.rs 与 docs/binfmt.md）
    }

    /// OPC 的自证：真实生产者的包，每个部件都在 `[Content_Types].xml` 里说过；
    /// 而塞一个没声明的部件进去，检查必须抓到 —— 只验正例等于没验
    #[test]
    fn the_content_type_check_fires_on_an_undeclared_part() {
        let raw = bytes_of("notes.docx");
        let doc = open(&raw);
        assert!(doc.untyped.is_empty(), "{:?}", doc.untyped);
        let mut entries = doc.entries.clone();
        entries.push(ZipEntry {
            name: "word/mystery.xyz".to_string(),
            method: 8,
            crc: 0,
            compressed: 0,
            size: 0,
            offset: 0,
        });
        assert_eq!(
            content_type_gaps(&raw, &entries),
            vec!["word/mystery.xyz".to_string()],
            "检查没有真的在查"
        );
    }

    /// 宏部件要报出来（docm 就是靠它认的），并且与「加密」分开
    #[test]
    fn macro_and_encryption_signals_are_separate() {
        let clean = risk_signals(&open(&bytes_of("notes.docx")));
        assert_eq!(clean["has_macros"], json!(false), "{clean}");
        assert_eq!(clean["is_encrypted"], json!(false));
        let macro_doc = risk_signals(&open(&bytes_of("notes.docm")));
        assert_eq!(macro_doc["has_macros"], json!(true), "{macro_doc}");
        assert_eq!(
            macro_doc["macro_parts"].as_array().expect("是数组").len(),
            1,
            "{macro_doc}"
        );
        assert_eq!(macro_doc["is_encrypted"], json!(false), "有宏不等于加密");
    }

    /// openpyxl 那份表格里有一个外部 nothing，但确实有命名区域与表格：
    /// external_link_parts 只数 `xl/externalLinks/`，别把关系表里的高链算进来
    #[test]
    fn external_link_parts_count_only_the_parts() {
        let signals = risk_signals(&open(&bytes_of("book.xlsx")));
        assert_eq!(signals["external_link_parts"], json!(0), "{signals}");
    }

    /// 认不出来的东西要留一句人话，而不是空着让人以为读到了
    #[test]
    fn unknown_bytes_say_why() {
        let doc = open(b"just some text, no container at all");
        assert_eq!(doc.family, Family::Other);
        assert!(!doc.notes.is_empty());
    }
}
