//! `lbin identify` — 这是什么文件，以及它自己报出来的头部事实。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::read::{header, read_blob, sniff};

/// 识别格式并展开头部
#[derive(App)]
#[app(
    name = "identify",
    run = "run_identify",
    about = "Say what a file is from its own bytes and unpack the fields its header states: returns { path, size, truncated, magic_hex, family, format, confidence, note, header } where confidence is 'signature' when the first bytes settle it and 'structural' when a table had to fit the file before the answer was given. Covers ELF (32/64), PE and DOS/MZ, Mach-O thin and universal (the 0xCAFEBABE clash with Java class is resolved by checking each slice offset lands inside the file, so a class file is never called a binary), DEX, ZIP/APK, tar, ar, PNG/JPEG/GIF/BMP/WebP/RIFF, TIFF, PDF, WebAssembly, Java class, SQLite, CAB, 7z, RAR, xar, EBML/Matroska, ISO BMFF by brand, gzip/bzip2/xz/zstd/LZ4, WOFF/WOFF2, RPM. Read-only (safety T0); nothing is executed, decompressed or written."
)]
pub struct Identify {
    /// 要看的文件
    #[arg(about = "File to identify", must_exist = true)]
    path: PathBuf,

    /// 最多读多少字节（签名与头部远小于这个数）
    #[arg(about = "Read at most this many bytes", default = 1048576)]
    max_bytes: u64,
}

fn run_identify(app: &Identify, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some("reading header".to_string()),
    });
    let guess = sniff(&blob.bytes);
    let head = match guess.as_ref() {
        Some(one) => header(&blob.bytes, one),
        None => json!({}),
    };
    let taken = blob.bytes.len().min(16);
    let magic_hex = hex(&blob.bytes[..taken]);
    ctx.tick(1, Some(1), "");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "truncated": blob.truncated,
        "magic_hex": magic_hex,
        "family": guess.as_ref().map(|one| one.family).unwrap_or("unknown"),
        "format": guess.as_ref().map(|one| one.format.clone()).unwrap_or_else(|| "unknown".to_string()),
        "confidence": guess.as_ref().map(|one| one.confidence).unwrap_or("none"),
        "note": guess.as_ref().map(|one| one.note.clone()).unwrap_or_default(),
        "header": head,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|one| format!("{one:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 供本域各命令的测试复用的最小 ELF64：头 + NULL 节 + 一个 `.shstrtab`，共 203 字节。
/// 每个字段都按它自己的宽度写（u16 的槽写 u16），因为 `object` 会拒掉 e_shstrndx 为 0 的文件。
#[cfg(test)]
pub(crate) fn tiny_elf() -> Vec<u8> {
    let mut raw = vec![0u8; 64 + 64 * 2 + 11];
    raw[..4].copy_from_slice(b"\x7fELF");
    raw[4] = 2; // ELFCLASS64
    raw[5] = 1; // ELFDATA2LSB
    raw[6] = 1; // EV_CURRENT
    raw[16..18].copy_from_slice(&3u16.to_le_bytes()); // e_type = ET_DYN
    raw[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // e_machine = EM_X86_64
    raw[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    raw[40..48].copy_from_slice(&64u64.to_le_bytes()); // e_shoff
                                                       // 头里这六个 u16 槽的位置是 52 / 54 / 56 / 58 / 60 / 62 —— 差两字节就让 `object` 说
                                                       // “节头表项尺寸不对”，因为这六个字段之间没有任何填充。
    raw[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    raw[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    raw[56..58].copy_from_slice(&0u16.to_le_bytes()); // e_phnum
    raw[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
    raw[60..62].copy_from_slice(&2u16.to_le_bytes()); // e_shnum
    raw[62..64].copy_from_slice(&1u16.to_le_bytes()); // e_shstrndx
                                                      // 第二个节头是 `.shstrtab`（第一个是全零的 NULL 节）：SHT_STRTAB，指向文件末尾那 11 字节
    let shdr = 64 + 64;
    raw[shdr..shdr + 4].copy_from_slice(&1u32.to_le_bytes()); // sh_name
    raw[shdr + 4..shdr + 8].copy_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
    raw[shdr + 24..shdr + 32].copy_from_slice(&192u64.to_le_bytes()); // sh_offset
    raw[shdr + 32..shdr + 40].copy_from_slice(&11u64.to_le_bytes()); // sh_size
    raw[192..203].copy_from_slice(b"\0.shstrtab\0");
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco_core::Context;
    use std::sync::mpsc;

    fn run(path: std::path::PathBuf) -> Result<Value, AppError> {
        let app = Identify {
            path,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        run_identify(&app, &Context::new_test(tx))
    }

    #[test]
    fn reads_an_elf_header_it_was_given() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.out");
        std::fs::write(&path, tiny_elf()).expect("写 ELF");
        let out = run(path).expect("identify 应成功");
        assert_eq!(out["family"], "elf");
        assert_eq!(out["format"], "ELF64");
        assert_eq!(out["confidence"], "signature");
        assert_eq!(out["header"]["machine"], "0x003e");
        assert_eq!(out["header"]["shnum"], 2);
        assert_eq!(out["header"]["class"], "ELF64");
        assert_eq!(out["header"]["endian"], "little");
    }

    /// 认不出来的东西要被老实说认不出来，不许编一个 family
    #[test]
    fn unknown_bytes_answer_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"just some text, no signature at all").expect("写文件");
        let out = run(path).expect("identify 应成功");
        assert_eq!(out["family"], "unknown");
        assert_eq!(out["confidence"], "none");
    }
}
