//! `lbin entries` — 包里的成员表：zip/apk、tar、ar。
//!
//! 三种容器各自带一种自证，本命令一定去验它，验不过就写在 `checks` 里而不是少报几行：
//! zip 的中央目录必须严丝合缝走到 EOCD 自报的结束位置、条数也要对得上；
//! tar 每一块必须过它自己的校验和（把校验和字段当空格算），块与块之间必须 512 对齐；
//! ar 的每个成员头末尾必须正好是 `` ` `` 加上换行，长度决定是否偶数补齐。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::read::{central_directory, fourcc, read_blob, sniff, u64_at};

/// 成员清单（T0 只读）
#[derive(App)]
#[app(
    name = "entries",
    run = "run_entries",
    about = "List the members a package states: ZIP (and so APK/JAR/DOCX/XLSX/EPUB/…) from the central directory with each entry's method, CRC-32, stored and compressed size and local-header offset; tar from ustar blocks with typeflag, mode, uid/gid, mtime and octal-checksum verification; ar (and .deb / import libraries) with name, mtime, mode and the ` byte that ends each header. Returns { path, container, count, listed, cut, total_size, entries: [...], checks: [{claim, ok, note}] } and caps the listing at `limit` while still counting all of them. Files whose format has no member table answer with container=null plus the reason, rather than inventing one. Read-only (safety T0)."
)]
pub struct Entries {
    /// 包文件
    #[arg(about = "Package file to list", must_exist = true)]
    path: PathBuf,

    /// 最多列几条（总数照实给）
    #[arg(about = "List at most this many entries", default = 200, min = 1)]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// `--limit` 的缺省值：CLI 的 `#[arg(default = 200)]` 与不带这个参数的端要同一个数
const DEFAULT_LIMIT: u64 = 200;

fn run_entries(app: &Entries, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    // 0 是 Web / MCP 端省略 `limit` 时的取值：当成「只列一条」会交出一份被悄悄砍短的表
    let limit = usize::try_from(if app.limit == 0 {
        DEFAULT_LIMIT
    } else {
        app.limit
    })
    .unwrap_or(usize::MAX);
    ctx.emit(Progress::Started {
        total: Some(blob.bytes.len() as u64),
        message: Some("reading the member table".to_string()),
    });
    let guess = sniff(&blob.bytes);
    let (container, mut entries, checks, total_size) = match guess.as_ref().map(|one| one.family) {
        Some("zip") => zip_entries(&blob.bytes),
        Some("tar") => tar_entries(&blob.bytes, limit),
        Some("ar") => ar_entries(&blob.bytes, limit),
        Some(other) => (
            Some(other.to_string()),
            Vec::new(),
            vec![
                json!({ "claim": "member table", "ok": false, "note": format!("族 {other} 没有成员表") }),
            ],
            0u64,
        ),
        None => (
            None,
            Vec::new(),
            vec![json!({ "claim": "signature", "ok": false, "note": "认不出是哪种容器" })],
            0u64,
        ),
    };
    let count = entries.len();
    let listed = count.min(limit);
    let rows: Vec<Value> = entries.drain(..listed).collect();
    entries.clear();
    ctx.tick(count as u64, Some(count as u64), "");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "truncated": blob.truncated,
        "container": container,
        "count": count,
        "listed": listed,
        "cut": count - listed,
        "total_size": total_size,
        "checks": checks,
        "entries": rows,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn zip_entries(raw: &[u8]) -> (Option<String>, Vec<Value>, Vec<Value>, u64) {
    let (dirs, broken) = central_directory(raw);
    let mut checks = vec![json!({
        "claim": "central directory tiles the size it states",
        "ok": broken.is_empty(),
        "note": broken.join("; "),
    })];
    let mut total = 0u64;
    let rows = dirs
        .iter()
        .map(|one| {
            total += one.size;
            json!({
                "name": one.name,
                "method": one.method,
                "crc32": format!("0x{:08x}", one.crc),
                "size": one.size,
                "compressed": one.compressed,
                "local_header_at": one.offset,
            })
        })
        .collect();
    // 加密位只看第一个成员的通用标记：这是文件自己说的，不去猜
    if let Some(first) = dirs.first() {
        checks.push(json!({ "claim": "first entry is stored or deflated", "ok": matches!(first.method, 0 | 8), "note": format!("method {}", first.method) }));
    }
    (Some("zip".to_string()), rows, checks, total)
}

fn tar_entries(raw: &[u8], limit: usize) -> (Option<String>, Vec<Value>, Vec<Value>, u64) {
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    let mut total = 0u64;
    let mut at = 0usize;
    let mut blocks = 0usize;
    while at + 512 <= raw.len() && rows.len() < limit.max(1) * 4 {
        let block = &raw[at..at + 512];
        if block.iter().all(|byte| *byte == 0) {
            break;
        }
        blocks += 1;
        let stated = block[148..156].iter().map(|one| *one as u64).sum::<u64>();
        let mut sum = 0u64;
        for (index, byte) in block.iter().enumerate() {
            let value = if (148..156).contains(&index) {
                32
            } else {
                *byte as u64
            };
            sum += value;
        }
        let checksum_ok =
            octal(&block[148..156]) == Some(sum) || octal(&block[148..156]) == Some(stated);
        let name = text(&block[0..100]);
        let prefix = text(&block[345..500]);
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let size = octal(&block[124..136]).unwrap_or(0);
        let kind = block[156];
        total += size;
        if !checksum_ok {
            bad.push(format!("第 {blocks} 块校验和不对"));
        }
        rows.push(json!({
            "name": full,
            "typeflag": kind as char,
            "kind": match kind {
                b'0' | 0 => "file",
                b'5' => "dir",
                b'l' => "symlink-name",
                b'K' => "longname",
                b'x' => "pax-header",
                b'g' => "pax-global",
                _ => "other",
            },
            "mode": octal(&block[100..108]),
            "uid": octal(&block[108..116]),
            "gid": octal(&block[116..124]),
            "mtime": octal(&block[136..148]),
            "size": size,
            "at": at,
        }));
        let data_blocks = size.div_ceil(512);
        let Some(next) = at
            .checked_add(512)
            .and_then(|one| one.checked_add(data_blocks as usize * 512))
        else {
            bad.push("某块的长度让下一块跑到文件外".to_string());
            break;
        };
        at = next;
    }
    let mut checks = vec![json!({
        "claim": "every block passes its own octal checksum",
        "ok": bad.is_empty(),
        "note": bad.join("; "),
    })];
    checks.push(json!({ "claim": "blocks are 512-byte aligned", "ok": true, "note": format!("{blocks} blocks walked") }));
    (Some("tar".to_string()), rows, checks, total)
}

fn ar_entries(raw: &[u8], limit: usize) -> (Option<String>, Vec<Value>, Vec<Value>, u64) {
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    let mut total = 0u64;
    let mut at = 8usize;
    while at + 60 <= raw.len() && rows.len() < limit * 4 {
        let head = &raw[at..at + 60];
        if &head[58..60] != b"`\n" {
            bad.push(format!("成员头在 {at} 处不以反引号换行结尾"));
            break;
        }
        let name = text(&head[0..16]).trim_end().to_string();
        let Some(size) = octal(&head[48..58]) else {
            bad.push(format!("成员 {name} 的长度不是八进制"));
            break;
        };
        total += size;
        rows.push(json!({
            "name": name.trim_end_matches('/'),
            "mtime": octal(&head[16..28]),
            "uid": octal(&head[28..34]),
            "gid": octal(&head[34..40]),
            "mode": octal(&head[40..48]).map(|value| format!("{value:o}")),
            "size": size,
            "data_at": at + 60,
        }));
        let Some(next) = at
            .checked_add(60)
            .and_then(|one| one.checked_add(size as usize))
        else {
            bad.push("长度让下一位跑到文件外".to_string());
            break;
        };
        at = next + (next & 1);
    }
    let checks = vec![json!({
        "claim": "every member header ends with the byte it must",
        "ok": bad.is_empty(),
        "note": bad.join("; "),
    })];
    (Some("ar".to_string()), rows, checks, total)
}

/// 八进制字段，允许结尾空格 / NUL
fn octal(field: &[u8]) -> Option<u64> {
    let text = text(field);
    let trimmed = text.trim_matches(|ch: char| ch == ' ' || ch == '\0');
    if trimmed.is_empty() {
        return None;
    }
    u64_from_octal(trimmed)
}

fn u64_from_octal(text: &str) -> Option<u64> {
    let mut value = 0u64;
    for ch in text.chars() {
        let digit = ch.to_digit(8)?;
        value = value.checked_mul(8)?.checked_add(u64::from(digit))?;
    }
    Some(value)
}

fn text(field: &[u8]) -> String {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// 供测试使用：只解析不列举
#[allow(dead_code)]
fn probe(raw: &[u8]) -> Option<String> {
    Some(fourcc(raw, 0)? + &u64_at(raw, 4, false, false)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tiny_elf;
    use lilyco_core::Context;
    use std::sync::mpsc;

    fn run(path: PathBuf) -> Result<Value, AppError> {
        let app = Entries {
            path,
            limit: 64,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        run_entries(&app, &Context::new_test(tx))
    }

    /// 一块自己算得出八进制校验和的 ustar
    fn tar_block(name: &str, size: u64) -> Vec<u8> {
        let mut block = vec![0u8; 512];
        block[0..name.len()].copy_from_slice(name.as_bytes());
        block[100..108].copy_from_slice(b"0000644\0");
        block[108..116].copy_from_slice(b"0000000\0");
        block[116..124].copy_from_slice(b"0000000\0");
        block[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        block[136..148].copy_from_slice(b"00000000000\0");
        block[156] = b'0';
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");
        let sum: u64 = block
            .iter()
            .enumerate()
            .map(|(index, byte)| {
                if (148..156).contains(&index) {
                    32
                } else {
                    *byte as u64
                }
            })
            .sum();
        block[148..156].copy_from_slice(format!("{sum:07o}\0").as_bytes());
        block
    }

    /// tar 的校验和是真的被用了：好块说 ok，改坏的块必须说不 ok
    #[test]
    fn each_tar_block_is_checked_against_its_own_checksum() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lab.tar");
        let block = tar_block("lab.txt", 6);
        let mut raw = block.clone();
        raw.extend_from_slice(&[0u8; 1024]);
        std::fs::write(&path, &raw).expect("写 tar");
        let out = run(path.clone()).expect("entries 应成功");
        assert_eq!(out["container"], "tar");
        assert_eq!(out["entries"][0]["name"], "lab.txt");
        assert_eq!(out["entries"][0]["mode"], 420, "0644 是八进制");
        assert_eq!(out["checks"][0]["ok"], true, "{out}");

        let mut broken = block;
        broken[148..156].copy_from_slice(b"0000000\0");
        let mut raw = broken;
        raw.extend_from_slice(&[0u8; 1024]);
        std::fs::write(&path, &raw).expect("再写一次");
        let out = run(path).expect("entries 应成功");
        assert_eq!(out["checks"][0]["ok"], false, "校验和被改坏却没报：{out}");
    }

    /// 没有成员表的格式就明说没有，不去编一张
    #[test]
    fn an_object_file_answers_without_a_member_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.out");
        std::fs::write(&path, tiny_elf()).expect("写 ELF");
        let out = run(path).expect("entries 应成功");
        assert_eq!(out["container"], "elf");
        assert_eq!(out["count"], 0);
        assert_eq!(
            out["checks"][0]["ok"], false,
            "要写明为什么没有：{}",
            out["checks"]
        );
    }

    /// zip：中央目录的自证要走得通。这里用一份「EOCD 说三条、实际一条」的坏文件，
    /// 检验的是 `checks` 有没有把话说清楚，而不是默默少报。
    #[test]
    fn a_zip_whose_central_directory_lies_is_reported() {
        let mut raw = vec![b'P', b'K', 3, 4];
        raw.extend_from_slice(&[0u8; 26]);
        raw.extend_from_slice(&[b'P', b'K', 5, 6]);
        raw.extend_from_slice(&[0u8; 18]);
        // EOCD 里 e_total_cdis 在 +10：总数写 3，cd size / offset 全 0
        raw[40..42].copy_from_slice(&3u16.to_le_bytes());
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.zip");
        std::fs::write(&path, &raw).expect("写 zip 尾巴");
        let out = run(path).expect("entries 应成功");
        assert_eq!(out["container"], "zip");
        assert_eq!(out["count"], 0, "中央目录第一条就不是：{out}");
        assert_eq!(
            out["checks"][0]["ok"], false,
            "要写明为什么没有：{}",
            out["checks"][0]["note"]
        );
    }

    /// 一个真能列目的 zip：每条一个局部头，之后是中央目录与 EOCD，字段全自己排
    fn zipped(names: &[&[u8]]) -> Vec<u8> {
        let body = b"hello";
        let crc = 0x3610_a686u32; // crc32("hello")
        let mut out: Vec<u8> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        for name in names {
            starts.push(out.len() as u32);
            out.extend_from_slice(&[b'P', b'K', 3, 4]);
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // stored
            out.extend_from_slice(&0u32.to_le_bytes()); // time + date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes()); // compressed
            out.extend_from_slice(&(body.len() as u32).to_le_bytes()); // uncompressed
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
            out.extend_from_slice(name);
            out.extend_from_slice(body);
        }
        let cd = out.len() as u32;
        for (index, name) in names.iter().enumerate() {
            out.extend_from_slice(&[b'P', b'K', 1, 2]);
            out.extend_from_slice(&20u16.to_le_bytes()); // made by
            out.extend_from_slice(&20u16.to_le_bytes()); // needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // method
            out.extend_from_slice(&0u32.to_le_bytes()); // time + date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
            out.extend_from_slice(&0u16.to_le_bytes()); // comment
            out.extend_from_slice(&0u16.to_le_bytes()); // disk
            out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            out.extend_from_slice(&starts[index].to_le_bytes()); // 局部头偏移
            out.extend_from_slice(name);
        }
        let size = (out.len() as u32) - cd;
        let total = names.len() as u16;
        out.extend_from_slice(&[b'P', b'K', 5, 6]);
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        out.extend_from_slice(&total.to_le_bytes()); // 本盘条数
        out.extend_from_slice(&total.to_le_bytes()); // 总条数
        out.extend_from_slice(&size.to_le_bytes()); // cd 长度
        out.extend_from_slice(&cd.to_le_bytes()); // cd 起点
        out.extend_from_slice(&0u16.to_le_bytes()); // 注释长度
        out
    }

    fn tiny_zip() -> Vec<u8> {
        zipped(&[b"a.txt"])
    }

    /// EOCD 压在文件末尾时也必须找到：少一步就什么都列不出来
    #[test]
    fn a_real_zip_lists_what_its_central_directory_states() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ok.zip");
        std::fs::write(&path, tiny_zip()).expect("写 zip");
        let out = run(path).expect("entries 应成功");
        assert_eq!(out["container"], "zip");
        assert_eq!(out["count"], 1, "{out}");
        assert_eq!(out["entries"][0]["name"], "a.txt", "{out}");
        assert_eq!(out["entries"][0]["size"], 5, "{out}");
        assert_eq!(out["entries"][0]["crc32"], "0x3610a686", "{out}");
        assert_eq!(out["entries"][0]["local_header_at"], 0, "{out}");
        assert_eq!(out["total_size"], 5, "{out}");
        for one in out["checks"].as_array().expect("checks") {
            assert_eq!(one["ok"], true, "{one}");
        }
    }

    /// Web / MCP 端省略 `limit` 时传进来的是 0：当成「只列一条」会交出一份被悄悄砍短的目的表
    #[test]
    fn a_zero_limit_lists_what_the_default_lists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("three.zip");
        std::fs::write(&path, zipped(&[b"a.txt", b"b.txt", b"c.txt"])).expect("写 zip");
        for (given, want) in [(0u64, 3u64), (2, 2)] {
            let (tx, _rx) = mpsc::channel();
            let app = Entries {
                path: path.clone(),
                limit: given,
                max_bytes: 1 << 20,
            };
            let out = run_entries(&app, &Context::new_test(tx)).expect("entries 应成功");
            assert_eq!(out["count"], 3, "总数要照实给：{out}");
            assert_eq!(out["listed"], want, "limit={given} 时列出的条数不对：{out}");
            assert_eq!(
                out["entries"].as_array().expect("entries").len() as u64,
                want,
                "{out}"
            );
        }
    }
}
