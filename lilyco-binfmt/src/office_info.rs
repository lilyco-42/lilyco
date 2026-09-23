//! `lbin office-info` — 这份办公文件是什么、谁写的、里面有什么、要不要小心。
//!
//! 一条命令回答四个最常问的问题，因为它们共用同一层读取（打开容器 + 读属性部件）：
//! 「这是 docx 还是改了后缀的 docx」「哪个程序写的、谁写的」「有多少部件 / 媒体 / 流」
//! 「有没有宏、有没有加密、有没有外部数据」。
//!
//! 承诺的边界与本域其它命令一样是 **T0 只读**：只在内存里解压与解析，不写回、不执行、
//! 不解密（加密包只会被告知「这是加密包」）。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{family_name, open, risk_signals, Family};
use crate::read::read_blob;
use crate::zipread;

/// 看一份办公文件的身份与包结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-info",
    run = "run_office_info",
    about = "Identify an office document from its bytes and report what its own package says: returns { path, size, family, format, app, container, producer, authoring, parts, media, external_targets, streams, signals, checks, notes }. family is one of ooxml / opendocument / compound / rtf / zip / unknown and is decided by part names or stream names, never by the file extension, so a renamed docx is still reported as docx. signals separates macros (vbaProject.bin), encryption (EncryptedPackage / encryptionInfo), digital signatures and external-link parts; producer/authoring come from docProps or the OLE property sets. checks records how the package's own claims line up (member table consistency, parts without a declared content type, streams that could not be read). Read-only (safety T0): nothing is written, executed or decrypted. Legacy .doc/.xls/.ppt are identified here; their body text is office-text's job."
)]
pub struct OfficeInfo {
    /// 办公文件（docx / xlsx / pptx / doc / xls / ppt / odt / ods / odp / rtf）
    #[arg(about = "Office document to inspect", must_exist = true)]
    path: PathBuf,

    /// 最多读多少字节（包结构与属性都在文件开头那一段）
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

fn run_office_info(app: &OfficeInfo, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the package".to_string()),
    });
    let doc = open(&blob.bytes);
    ctx.tick(1, Some(1), "classified");
    let mut media = 0usize;
    let mut external = 0usize;
    let mut checks: Vec<Value> = Vec::new();
    if doc.is_zip_family() {
        media = doc
            .entries
            .iter()
            .filter(|one| {
                one.name.starts_with("word/media/")
                    || one.name.starts_with("xl/media/")
                    || one.name.starts_with("ppt/media/")
                    || one.name.starts_with("Pictures/")
            })
            .count();
        // 外部目标：关系表里 TargetMode="External" 的条数（要读 .rels 才能数，
        // 每个关系文件都不大，且必须解压才知道内容）
        for one in &doc.entries {
            if !one.name.ends_with(".rels") {
                continue;
            }
            let Ok(member) = zipread::read_member(&blob.bytes, one, zipread::DEFAULT_MEMBER_CAP)
            else {
                continue;
            };
            let text = member.as_text();
            let doc = crate::xmlscan::parse_str(&text);
            external += doc
                .descendants("Relationship")
                .iter()
                .filter(|one| one.attr("TargetMode") == Some("External"))
                .count();
        }
        checks.push(json!({
            "claim": "成员表能自证（走到 EOCD 自报的位置与条数）",
            "ok": doc.notes.is_empty(),
            "note": doc.notes.join("；"),
        }));
        checks.push(json!({
            "claim": "每个部件都有声明过的内容类型",
            "ok": crate::opack::untyped_parts(&doc).is_empty(),
            "note": crate::opack::untyped_parts(&doc).join("；"),
        }));
    }
    if let Some(one) = doc.compound.as_ref() {
        let readable = one
            .entries
            .iter()
            .filter(|item| item.is_stream())
            .filter(|item| one.read(&blob.bytes, &item.name).is_none())
            .count();
        checks.push(json!({
            "claim": "每条流都能按目录项自报的大小读出来",
            "ok": readable == 0,
            "note": if readable == 0 { String::new() } else { format!("{readable} 条流的字节读不齐") },
        }));
        for note in &one.notes {
            checks.push(json!({ "claim": "容器自述", "ok": false, "note": note }));
        }
    }
    let (producer, authoring) = producer_of(&doc, &blob.bytes);
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "truncated": blob.truncated,
        "family": family_name(doc.family),
        "format": doc.format,
        "app": doc.app,
        "container": match doc.family {
            Family::Compound => "MS-CFB compound file",
            Family::Rtf => "RTF text",
            Family::Ooxml => "ZIP (OPC package)",
            Family::Odf => "ZIP (OpenDocument package)",
            Family::Zip => "ZIP",
            Family::Other => "unknown",
        },
        "producer": producer,
        "authoring": authoring,
        "parts": doc.entries.len(),
        "media": media,
        "external_targets": external,
        "streams": doc.compound.as_ref().map(|one| one.stream_names()),
        "signals": risk_signals(&doc),
        "checks": checks,
        "notes": doc.notes,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 「谁写的」：OOXML 看 docProps/app.xml，ODF 看 meta.xml，复合文档看 OLE 属性集
fn producer_of(doc: &crate::opack::Doc, bytes: &[u8]) -> (Value, Value) {
    let mut producer = json!({});
    let mut authoring = json!({});
    if doc.is_zip_family() {
        if let Some(member) = read(bytes, "docProps/app.xml") {
            let node = crate::xmlscan::parse_str(&member.as_text());
            for one in node.descendants("Application") {
                producer["application"] = json!(one.text());
            }
            for one in node.descendants("AppVersion") {
                producer["app-version"] = json!(one.text());
            }
            for one in node.descendants("Template") {
                producer["template"] = json!(one.text());
            }
        }
        if let Some(member) = read(bytes, "docProps/core.xml") {
            let node = crate::xmlscan::parse_str(&member.as_text());
            for (tag, key) in [
                ("creator", "creator"),
                ("lastModifiedBy", "last-modified-by"),
                ("created", "created"),
                ("modified", "modified"),
                ("revision", "revision"),
            ] {
                if let Some(one) = node.descendants(tag).first() {
                    authoring[key] = json!(one.text());
                }
            }
        }
        if let Some(member) = read(bytes, "meta.xml") {
            let node = crate::xmlscan::parse_str(&member.as_text());
            for (tag, key) in [
                ("generator", "generator"),
                ("creator", "creator"),
                ("initial-creator", "initial-creator"),
                ("creation-date", "created"),
                ("date", "modified"),
                ("editing-cycles", "revision"),
            ] {
                if let Some(one) = node.descendants(tag).first() {
                    let value = one.text();
                    if !value.is_empty() {
                        authoring[key] = json!(value.clone());
                        // meta:generator 就是「谁写的」，producer 里也该有一份
                        if key == "generator" {
                            producer["generator"] = json!(value);
                        }
                    }
                }
            }
        }
        return (producer, authoring);
    }
    if let Some(one) = doc.compound.as_ref() {
        for name in ["\u{5}SummaryInformation", "\u{5}DocumentSummaryInformation"] {
            let Some(raw) = one.read(bytes, name) else {
                continue;
            };
            let Ok(sets) = crate::props::decode(&raw) else {
                continue;
            };
            for set in sets {
                for prop in set.props {
                    let Some(key) = prop.name else { continue };
                    if key == "application" || key == "template" {
                        producer[key] = prop.value.clone();
                    } else if matches!(
                        key,
                        "creator"
                            | "author"
                            | "last-author"
                            | "created"
                            | "last-saved"
                            | "revision"
                    ) {
                        authoring[key] = prop.value.clone();
                    }
                }
            }
        }
    }
    (producer, authoring)
}

fn read(bytes: &[u8], want: &str) -> Option<zipread::Member> {
    zipread::member(bytes, want, zipread::DEFAULT_MEMBER_CAP).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/office")
            .join(name);
        let app = OfficeInfo {
            path,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_info(&app, &Context::new_test(tx)).expect("office-info 应成功")
    }

    /// docx：20 个部件、一张媒体、没有宏、生产者写着 Microsoft Word、作者是 liuqi
    #[test]
    fn describes_a_docx_written_by_python_docx() {
        let out = run("notes.docx");
        assert_eq!(out["family"], "ooxml");
        assert_eq!(out["format"], "docx");
        assert_eq!(out["app"], "word");
        assert_eq!(out["parts"], 20, "{out}");
        assert_eq!(out["media"], 1);
        assert_eq!(out["external_targets"], 1, "文档里有一个站外超链接");
        assert_eq!(out["signals"]["has_macros"], json!(false));
        assert_eq!(out["signals"]["is_encrypted"], json!(false));
        assert_eq!(out["producer"]["application"], "Microsoft Macintosh Word");
        assert_eq!(out["authoring"]["creator"], "liuqi");
        assert_eq!(out["authoring"]["created"], "2013-12-23T23:15:00Z");
    }

    /// 同一条命令看四种家族：每条都得给出可用的答案，而不是只会读 docx
    #[test]
    fn one_command_covers_all_four_families() {
        let xlsx = run("book.xlsx");
        assert_eq!(xlsx["family"], "ooxml");
        assert_eq!(xlsx["app"], "excel");
        assert_eq!(xlsx["parts"], 13, "{xlsx}");

        let pptx = run("deck.pptx");
        assert_eq!(pptx["app"], "powerpoint");
        assert_eq!(pptx["parts"], 46, "{pptx}");
        assert_eq!(pptx["media"], 1);

        let odt = run("notes.odt");
        assert_eq!(odt["family"], "opendocument");
        assert_eq!(odt["format"], "odt");
        // 只比到产品名：具体版本号跟着 LibreOffice 的构建走，钉死它等于给自己埋雷
        let generator = odt["producer"]["generator"].as_str().unwrap_or("");
        assert!(
            generator.starts_with("LibreOffice/"),
            "meta.xml 里的 generator 应当说明是谁写的：{odt}"
        );
        assert_eq!(odt["authoring"]["initial-creator"], "liuqi", "{odt}");

        let legacy = run("notes.doc");
        assert_eq!(legacy["family"], "compound");
        assert_eq!(legacy["app"], "word");
        assert_eq!(
            legacy["streams"].as_array().expect("是数组").len(),
            7,
            "{legacy}"
        );

        let rtf = run("notes.rtf");
        assert_eq!(rtf["family"], "rtf");
        assert_eq!(rtf["app"], "word");
    }

    /// 宏样本：docm 必须被 signals 说清楚，同时不能被当成加密
    #[test]
    fn the_macro_fixture_is_reported_as_having_macros() {
        let out = run("notes.docm");
        assert_eq!(out["format"], "docm", "{out}");
        assert_eq!(out["signals"]["has_macros"], json!(true));
        assert_eq!(out["signals"]["macro_parts"][0], "word/vbaProject.bin");
        assert_eq!(out["signals"]["is_encrypted"], json!(false));
    }

    /// 所有检查项都必须「过」——这条测试同时是本命令的自检：
    /// 真实生产者文件不该出现「成员表不自洽」这种话
    #[test]
    fn every_producer_fixture_passes_its_own_checks() {
        for name in [
            "notes.docx",
            "book.xlsx",
            "deck.pptx",
            "notes.odt",
            "book.ods",
            "deck.odp",
            "notes.doc",
            "book.xls",
            "deck.ppt",
            "notes.rtf",
        ] {
            let out = run(name);
            for one in out["checks"]
                .as_array()
                .unwrap_or_else(|| panic!("{name} 没有 checks"))
            {
                assert_eq!(one["ok"], json!(true), "{name}: {one}");
            }
            assert!(out["family"] != "unknown", "{name} 应该认出来：{out}");
        }
    }

    /// 不是办公文件也要有条理地回答，而不是报错
    #[test]
    fn a_non_office_binary_still_answers() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let app = OfficeInfo {
            path,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let out = run_office_info(&app, &Context::new_test(tx)).expect("读文本文件不该失败");
        assert_eq!(out["family"], "unknown");
        assert!(!out["notes"].as_array().expect("有 notes").is_empty());
    }

    /// `max_bytes` 截断时要说出来：属性读不齐比假装读齐好
    #[test]
    fn a_truncated_read_says_so() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/deck.ppt");
        let app = OfficeInfo {
            path,
            max_bytes: 4096,
        };
        let (tx, _rx) = mpsc::channel();
        let out = run_office_info(&app, &Context::new_test(tx)).expect("截断也能给出部分答案");
        assert!(out["truncated"].as_bool().expect("有 truncated"));
        assert_eq!(out["size"], 656896, "原始大小要照报");
    }
}
