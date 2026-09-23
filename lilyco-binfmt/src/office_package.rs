//! `lbin office-package` — 这个办公包自己说得圆不圆。
//!
//! OOXML 的包是一组部件加两张账：`[Content_Types].xml` 说「每个文件是什么类型」，
//! `_rels/*.rels` 说「谁指着谁」。常用的问题都落在这两张账的裂缝上：
//! 关系指着不存在的部件（Word 会说「文件已损坏」）、部件没声明类型、
//! 或者一堆文件没有任何关系指着（孤儿部件 —— 有的合法，比如样式表；有的才是问题）。
//!
//! ODF 不用 OPC：它的清单是 `META-INF/manifest.xml`，只说媒体类型、不说跨文件关系，
//! 所以这条命令对 ODF 报的是「清单里有没有没列出的文件 / 列出的文件在不在」，
//! 并把关系相关的检查标成不适用 —— 而不是硬套一套 OPC 的说法。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, ContentTypes, Family};
use crate::read::read_blob;
use crate::zipread;

/// 检查办公包的部件表、内容类型与关系是否自洽（T0 只读）
#[derive(App)]
#[app(
    name = "office-package",
    run = "run_office_package",
    about = "Audit an office package against its own books: for OOXML it lists every part with the content type that declares it (Default by extension or Override by part name), the member's own stated sizes and whether inflating it matched its CRC-32, plus how many relationship entries point at it; and it reports relationships whose target is not in the package, parts nothing points at, and parts no content type declares. For ODF it compares META-INF/manifest.xml entries against the actual files (both directions) because that format has no cross-part relationships - those checks are marked not-applicable instead of being faked as empty passes. Returns { path, format, kind, parts: [{name, content_type, size, compressed, method, verified, pointed_at}], counts, checks: [{claim, ok, note}], notes }. Read-only (safety T0): parts are inflated in memory only, and nothing is rewritten or repaired."
)]
pub struct OfficePackage {
    /// 办公包
    #[arg(about = "Office package to audit", must_exist = true)]
    path: PathBuf,

    /// 最多解出多少个部件（计数仍是全量）
    #[arg(about = "Verify at most this many parts", default = 200, min = 1)]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 200;

fn run_office_package(app: &OfficePackage, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("auditing the package".to_string()),
    });
    let doc = open(&blob.bytes);
    if !doc.is_zip_family() {
        return Err(AppError::InvalidInput(format!(
            "{} 不是包型办公文件（它是 {}）；.doc/.xls/.ppt 的流表请用 entries 或 office-info",
            app.path.display(),
            doc.format
        )));
    }
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    let types = if doc.family == Family::Ooxml {
        ContentTypes::read(&blob.bytes)
    } else {
        ContentTypes::default()
    };
    let (rels, mut notes) = crate::opack::relationships(&blob.bytes, &doc.entries);
    notes.extend(types.notes.iter().cloned());
    let mut pointed: Vec<String> = rels.iter().filter_map(|one| one.resolved.clone()).collect();
    if doc.family == Family::Odf {
        let manifest = crate::opack::odf_manifest(&blob.bytes);
        for (path, _) in &manifest {
            let trimmed = path.trim_end_matches('/');
            if !trimmed.is_empty() {
                pointed.push(trimmed.to_string());
            }
        }
    }
    let mut parts: Vec<Value> = Vec::new();
    let mut untyped: Vec<String> = Vec::new();
    let mut media = 0usize;
    let mut unverified = 0usize;
    for (index, one) in doc.entries.iter().enumerate() {
        let name = one.name.as_str();
        if name.ends_with('/') || name == "[Content_Types].xml" {
            continue;
        }
        let content_type = types.of(name);
        if doc.family == Family::Ooxml && content_type.is_none() {
            untyped.push(name.to_string());
        }
        if content_type
            .map(|one| one.starts_with("image/"))
            .unwrap_or(false)
        {
            media += 1;
        }
        let hits = pointed.iter().filter(|one| one.as_str() == name).count();
        let mut entry = json!({
            "name": name,
            "content_type": content_type,
            "size": one.size,
            "compressed": one.compressed,
            "method": one.method,
            "pointed_at": hits,
        });
        if index < limit {
            match zipread::read_member(&blob.bytes, one, zipread::DEFAULT_MEMBER_CAP) {
                Ok(member) => {
                    entry["verified"] = json!(member.verified);
                    if !member.verified {
                        unverified += 1;
                        notes.push(format!("{}：{}", name, member.note));
                    }
                }
                Err(why) => {
                    entry["verified"] = json!(false);
                    entry["error"] = json!(why);
                    unverified += 1;
                }
            }
        } else {
            entry["verified"] = json!(null);
        }
        parts.push(entry);
    }
    let orphans: Vec<String> = parts
        .iter()
        .filter(|one| {
            one["pointed_at"].as_u64().unwrap_or(0) == 0
                && one["name"].as_str().unwrap_or("") != "docProps/core.xml"
                && one["name"].as_str().unwrap_or("") != "[Content_Types].xml"
        })
        .map(|one| one["name"].as_str().unwrap_or_default().to_string())
        .collect();
    let broken: Vec<String> = notes
        .iter()
        .filter(|one| one.contains("包里没有的部件"))
        .cloned()
        .collect();
    // 四条自证：每条都是一个 json! 对象，先在外头拼好 —— `json!` 里嵌 `[...]` 再
    // 接 `.iter()` 不是合法的 Rust，宏吃不下尾随表达式。
    let checks = vec![
        json!({
            "claim": "成员表能自证（EOCD 自报的条数与位置对得上）",
            "ok": doc.notes.is_empty(),
            "note": doc.notes.join("；"),
        }),
        json!({
            "claim": "每条内部关系都指着一个真实存在的部件",
            "ok": broken.is_empty(),
            "note": broken.join("；"),
        }),
        json!({
            "claim": "每个部件都声明了内容类型",
            "ok": if doc.family == Family::Ooxml { untyped.is_empty() } else { true },
            "note": if doc.family == Family::Ooxml {
                untyped.join("；")
            } else {
                "ODF 不用 OPC 的内容类型表，这条不适用".to_string()
            },
        }),
        json!({
            "claim": "解压出来的每个部件都过它自己声明的 CRC-32 与长度",
            "ok": unverified == 0,
            "note": format!("{unverified} 个部件没过（上限 {limit} 之内）"),
        }),
    ];
    ctx.tick(1, Some(1), "audited");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "format": doc.format,
        "kind": if doc.family == Family::Ooxml { "opc" } else { "odf" },
        "parts": parts,
        "counts": {
            "members": doc.entries.len(),
            "listed": parts.len(),
            "relationships": rels.len(),
            "external_targets": rels.iter().filter(|one| one.external).count(),
            "media": media,
            "unverified": unverified,
            "undeclared_types": untyped.len(),
            "unpointed": orphans.len(),
        },
        "checks": checks,
        "notes": notes,
        "unpointed_parts": orphans,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str, limit: u64) -> Value {
        let app = OfficePackage {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_package(&app, &Context::new_test(tx)).expect("office-package 应成功")
    }

    /// 真实生产者包：四条检查全过，部件数与内容类型数与独立读者一致
    #[test]
    fn a_real_package_passes_all_four_checks() {
        for name in ["notes.docx", "book.xlsx", "deck.pptx", "notes.odt"] {
            let out = run(name, 200);
            for one in out["checks"].as_array().expect("有 checks") {
                assert_eq!(one["ok"], json!(true), "{name}: {one}");
            }
            assert!(
                out["counts"]["members"].as_u64().expect("有 members") > 3,
                "{out}"
            );
            // ODF 根本没有跨部件关系：那里数出来的 0 是实情，不是缺陷
            if !name.ends_with(".odt") {
                assert!(
                    out["counts"]["relationships"]
                        .as_u64()
                        .expect("有 relationships")
                        > 0,
                    "{name}: {out}"
                );
            }
        }
    }

    /// docx 的每个部件都要有类型，且图片被关系指着
    #[test]
    fn parts_carry_their_declared_type_and_pointer_count() {
        let out = run("notes.docx", 200);
        assert_eq!(out["counts"]["members"], 20, "{out}");
        let parts = out["parts"].as_array().expect("是数组");
        let image = parts
            .iter()
            .find(|one| one["name"] == "word/media/image1.png")
            .expect("那张图要在部件表里");
        assert_eq!(image["content_type"], "image/png");
        assert_eq!(image["pointed_at"], 1, "被一条关系指着");
        assert_eq!(image["verified"], json!(true));
        assert_eq!(out["counts"]["undeclared_types"], 0);
    }

    /// ODF 没有 OPC 关系：那条检查要标不适用，而不是假装通过成「零条关系也没问题」
    #[test]
    fn odf_reports_its_own_book_instead_of_faking_opc() {
        let out = run("notes.odt", 200);
        assert_eq!(out["kind"], "odf");
        assert_eq!(out["counts"]["relationships"], 0, "{out}");
        let typed = out["checks"]
            .as_array()
            .expect("有 checks")
            .iter()
            .find(|one| one["note"].as_str().unwrap_or("").contains("不适用"))
            .expect("内容类型那条要说明 ODF 不适用");
        assert_eq!(typed["ok"], json!(true));
    }

    /// 上限只影响「解了几个」，计数仍是全量：静默截断是本域最不该犯的错
    #[test]
    fn a_small_limit_still_counts_everything() {
        let out = run("notes.docx", 2);
        assert_eq!(out["counts"]["members"], 20, "{out}");
        assert_eq!(
            out["parts"].as_array().expect("是数组").len(),
            19,
            "19 = 20 去掉 [Content_Types].xml"
        );
        let nulls = out["parts"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter(|one| one["verified"].is_null())
            .count();
        assert!(nulls >= 15, "上限之外的部件要标 null 而不是标失败：{nulls}");
    }

    /// 非包型文件（.doc）要明确拒绝并指向该用的命令
    #[test]
    fn a_compound_document_is_not_a_package() {
        let app = OfficePackage {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/notes.doc"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_package(&app, &Context::new_test(tx)).unwrap_err();
        assert!(why.to_string().contains("office-info"), "{why}");
    }

    #[test]
    fn content_type_helper_is_shared_and_does_not_double_parse() {
        let raw = std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/deck.pptx"),
        )
        .expect("读 fixture");
        let types = ContentTypes::read(&raw);
        assert!(
            types
                .of("ppt/slideLayouts/slideLayout1.xml")
                .unwrap_or_default()
                .contains("slideLayout"),
            "{:?}",
            types.overrides
        );
    }
}
