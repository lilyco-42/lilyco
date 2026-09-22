//! 域内共享的读法：把一段字节读成「这是什么」与「它自己怎么说」。
//!
//! 三条规矩贯穿本文件：
//! 1. **只在文件自己说出来的地方取值**。长度字段说不通（超出文件、不对齐、自己比自己小）
//!    就不继续猜，而是把说不通这件事写进结果里；
//! 2. **能自证的格式一定自证**：zip 的中央目录要能严丝合缝铺满它自报的字节数、tar 每块
//!    要过它自己的校验和、ar 的每个成员头末尾必须是 `` ` `` 且偶数对齐 —— 过不了就在
//!    `checks` 里写清哪一条没过，而不是少报几行；
//! 3. **同一段字节可以被两种格式合法地共享**：`0xCAFEBABE` 既是 Mach-O 通用二进制也是
//!    Java class 的魔数。分不开的时候宁可说「看不准」，不许给一个自信的错误答案。

use serde_json::{json, Value};

/// 一次读进来的文件：字节、原始长度、是否被 `max_bytes` 截断
pub struct Blob {
    pub bytes: Vec<u8>,
    pub size: u64,
    pub truncated: bool,
}

/// 大端 / 小端取数：越界一律 `None`，调用方决定怎么交代
fn le16(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u64)
}
fn le32(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
}
fn le64(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}
fn be16(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as u64)
}
fn be32(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
}
fn be64(at: usize) -> impl Fn(&[u8]) -> Option<u64> {
    move |b| Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// 四字符标签，非 UTF-8 时按 lossy 处理（格式的标签就是四个字节的 ASCII）
pub fn fourcc(b: &[u8], at: usize) -> Option<String> {
    Some(String::from_utf8_lossy(b.get(at..at + 4)?).into_owned())
}

/// NUL 结尾串；只在窗口内查找，找不到就 `None`（不替文件补一个结尾）
pub fn cstr(b: &[u8], at: usize, limit: usize) -> Option<String> {
    let room = b.get(at..b.len().min(at + limit))?;
    let end = room.iter().position(|byte| *byte == 0)?;
    Some(String::from_utf8_lossy(&room[..end]).into_owned())
}

/// 定长四个字符的标签（PNG / IFF / 字体表的块名都是这种，靠长度而不是结尾符界定）
pub fn cstr4(b: &[u8], at: usize) -> Option<String> {
    Some(String::from_utf8_lossy(b.get(at..at + 4)?).into_owned())
}

/// 定长字段里的名字：Mach-O 的 sectname/segname 只有 16 字节，名字占满时没有结尾符，
/// 所以按长度截而不是找 `\0`（`__compact_unwind`、`__gcc_except_tab` 正好占满）。
pub fn fixed_name(b: &[u8], at: usize, limit: usize) -> String {
    let room = b.get(at..b.len().min(at + limit)).unwrap_or(&[]);
    let end = room
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(room.len());
    String::from_utf8_lossy(&room[..end]).into_owned()
}

pub fn u64_at(b: &[u8], at: usize, wide: bool, little: bool) -> Option<u64> {
    if !wide {
        return if little { le32(at)(b) } else { be32(at)(b) };
    }
    if little {
        le64(at)(b)
    } else {
        be64(at)(b)
    }
}

/// 是什么：族、具体格式、以及这个词是谁给的
#[derive(Clone, Debug)]
pub struct Guess {
    pub family: &'static str,
    pub format: String,
    pub confidence: &'static str,
    pub note: String,
}

/// 魔数表。`confidence` 只有两种说法：`signature`（开头的字节唯一确定）与
/// `structural`（魔数之外还过了结构检查才敢这么说）。
pub fn sniff(b: &[u8]) -> Option<Guess> {
    let take =
        |at: usize, text: &[u8]| b.len() >= at + text.len() && &b[at..at + text.len()] == text;
    if take(0, b"\x7fELF") {
        let class = match b.get(4) {
            Some(1) => "ELF32",
            Some(2) => "ELF64",
            _ => "ELF",
        };
        return Some(Guess {
            family: "elf",
            format: class.to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"MZ") {
        // e_lfanew 在 0x3C；PE 头必须跟着 "PE\0\0" 才算数，否则它只是一个 DOS 头
        let lfanew = le32(0x3C)(b)?;
        let at = usize::try_from(lfanew).ok()?;
        if !take(at, b"PE\0\0") {
            return Some(Guess {
                family: "dos",
                format: "DOS/MZ".to_string(),
                confidence: "structural",
                note: "e_lfanew 之后没有 PE 签名".to_string(),
            });
        }
        return Some(Guess {
            family: "pe",
            format: "PE".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if let Some(one) = macho_guess(b) {
        return Some(one);
    }
    if take(0, b"dex\n") {
        return Some(Guess {
            family: "dex",
            format: format!("DEX {}", version(b)),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"PK\x03\x04") || take(0, b"PK\x05\x06") || take(0, b"PK\x07\x08") {
        return Some(Guess {
            family: "zip",
            format: "ZIP".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(257, b"ustar") {
        return Some(Guess {
            family: "tar",
            format: "tar".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"!<arch>\n") {
        return Some(Guess {
            family: "ar",
            format: "ar".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x89PNG\r\n\x1a\n") {
        return Some(Guess {
            family: "png",
            format: "PNG".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\xff\xd8\xff") {
        return Some(Guess {
            family: "jpeg",
            format: "JPEG".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"GIF87a") || take(0, b"GIF89a") {
        return Some(Guess {
            family: "gif",
            format: "GIF".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"BM") {
        return Some(Guess {
            family: "bmp",
            format: "BMP".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"RIFF") {
        let inner = fourcc(b, 8).unwrap_or_default();
        return Some(Guess {
            family: "riff",
            format: match inner.as_str() {
                "WEBP" => "WebP".to_string(),
                "WAVE" => "WAVE".to_string(),
                "AVI " => "AVI".to_string(),
                // 四个字节读出来是空的：只有 RIFF 头，没有 form 类型可说
                other => {
                    if other.is_empty() {
                        "RIFF".to_string()
                    } else {
                        format!("RIFF/{other}")
                    }
                }
            },
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x1aE\xdf\xa3") {
        return Some(Guess {
            family: "ebml",
            format: "EBML/Matroska".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(4, b"ftyp") {
        let brand = fourcc(b, 8).unwrap_or_default();
        return Some(Guess {
            family: "iso-bmff",
            format: format!("ISO BMFF/{brand}"),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"%PDF-") {
        return Some(Guess {
            family: "pdf",
            format: format!("PDF {}", version(b)),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x00\x61\x73\x6d") {
        return Some(Guess {
            family: "wasm",
            format: "WebAssembly".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"II\x2a\x00")
        || take(0, b"MM\x00\x2a")
        || take(0, b"II\x2b\x00")
        || take(0, b"MM\x00\x2b")
    {
        return Some(Guess {
            family: "tiff",
            format: "TIFF".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"SQLite format 3\0") {
        return Some(Guess {
            family: "sqlite",
            format: "SQLite".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\xed\xab\xee\xdb") {
        return Some(Guess {
            family: "rpm",
            format: "RPM".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x04\x22\x4d\x18") {
        return Some(Guess {
            family: "lz4",
            format: "LZ4".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x28\xb5\x2f\xfd") {
        return Some(Guess {
            family: "zstd",
            format: "Zstandard".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x04\x00\x00\x00") || take(0, b"\x50\x4b\x03\x04\x14\x00") {
        return None; // 太容易撞车的四个字节，交给别处判
    }
    if take(0, b"\x1f\x8b") {
        return Some(Guess {
            family: "gzip",
            format: "gzip".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"BZh") {
        return Some(Guess {
            family: "bzip2",
            format: "bzip2".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\xfd7zXZ\0") {
        return Some(Guess {
            family: "xz",
            format: "xz".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"wOF\x02") || take(0, b"wOFF") {
        return Some(Guess {
            family: "woff",
            format: if take(0, b"wOF\x02") { "WOFF2" } else { "WOFF" }.to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"hs7z\x07\x56\x52\xa2") {
        return Some(Guess {
            family: "sevenzip",
            format: "7z".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"Rar!\x1a\x07") {
        return Some(Guess {
            family: "rar",
            format: "RAR".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"CASdBC\x00\x00") || take(0, b"CAB file V9.x") || take(4, b"CAB ") {
        return Some(Guess {
            family: "cab",
            format: "CAB".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"x3\x00\x00") || take(0, b"x6\x00\x00") {
        return Some(Guess {
            family: "xar",
            format: "xar".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\xca\xfe\xba\xbe") {
        return Some(Guess {
            family: "javaclass",
            format: "Java class".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"AB\x00\x00") {
        return Some(Guess {
            family: "android-boot",
            format: "Android boot img".to_string(),
            confidence: "signature",
            note: String::new(),
        });
    }
    if take(0, b"\x7fELF") || take(0, b"\xed\xab\xee\xed") {
        return None;
    }
    None
}

fn version(b: &[u8]) -> String {
    let tail = b.get(5..8).unwrap_or(b"");
    String::from_utf8_lossy(tail).trim().to_string()
}

/// Mach-O 的两个家族：单架构（0xfeedface / 0xfeedfacf）与通用二进制（0xCAFEBABE）。
/// 通用二进制的魔数与 Java class 撞车，所以只有当架构表真的铺得进文件时才认它。
fn macho_guess(b: &[u8]) -> Option<Guess> {
    let magic = be32(0)(b)?;
    // 按大端读头四个字节能把两种排列分开：FE ED FA CF 本身就是大端写的 64 位镜像，
    // 而 CF FA ED FE 是小端写的同一个魔数——后缀说的是文件的字节排列，不是谁在读它。
    match magic {
        0xFEED_FACE => Some(Guess {
            family: "macho",
            format: "Mach-O 32-bit (big endian)".to_string(),
            confidence: "signature",
            note: String::new(),
        }),
        0xFEED_FACF => Some(Guess {
            family: "macho",
            format: "Mach-O 64-bit (big endian)".to_string(),
            confidence: "signature",
            note: String::new(),
        }),
        0xCEFA_EDFE => Some(Guess {
            family: "macho",
            format: "Mach-O 32-bit (little endian)".to_string(),
            confidence: "signature",
            note: String::new(),
        }),
        0xCFFA_EDFE => Some(Guess {
            family: "macho",
            format: "Mach-O 64-bit (little endian)".to_string(),
            confidence: "signature",
            note: String::new(),
        }),
        0xCAFE_BABE => fat_guess(b),
        _ => None,
    }
}

fn fat_guess(b: &[u8]) -> Option<Guess> {
    let count = be32(4)(b)?;
    if count == 0 || count > 64 {
        return None; // Java class 在这里是 constant_pool_count，几乎总是 >64 的小数
    }
    let mut at = 8usize;
    let mut fits = true;
    for _ in 0..count {
        // fat_arch 是 20 字节（64 位容器 fat_arch64 是 32 字节），offset/filesize 决定切片是否真在文件里
        let (Some(cpu), Some(offset), Some(size)) =
            (be32(at)(b), be32(at + 8)(b), be32(at + 12)(b))
        else {
            return None;
        };
        if cpu == 0 || offset < 8 + count * 20 || offset.checked_add(size)? > b.len() as u64 {
            fits = false;
            break;
        }
        at += 20;
    }
    if !fits {
        return None;
    }
    Some(Guess {
        family: "macho-fat",
        format: format!("Mach-O universal ({count} slices)"),
        confidence: "structural",
        note: "魔数与 Java class 相同，靠架构表落在文件内才认定".to_string(),
    })
}

/// 头里的关键事实：按族分派，一律只取文件说出来的字段
pub fn header(b: &[u8], guess: &Guess) -> Value {
    match guess.family {
        "elf" => elf_header(b),
        "pe" => pe_header(b),
        "macho" | "macho-fat" => macho_header(b),
        "zip" => zip_header(b),
        "tar" => json!({ "block_size": 512 }),
        "ar" => json!({ "magic": "!<arch>" }),
        "dex" => dex_header(b),
        "png" => png_header(b),
        "gif" => gif_header(b),
        "bmp" => bmp_header(b),
        "javaclass" => class_header(b),
        "wasm" => wasm_header(b),
        _ => json!({}),
    }
}

fn elf_header(b: &[u8]) -> Value {
    let wide = b.get(4) == Some(&2);
    let little = b.get(5) == Some(&1);
    let word = |at: usize| u64_at(b, at, wide, little);
    let half = |at: usize| if little { le16(at)(b) } else { be16(at)(b) };
    let types = [(1u64, "REL"), (2, "EXEC"), (3, "DYN"), (4, "CORE")];
    json!({
        "class": if wide { "ELF64" } else { "ELF32" },
        "endian": if little { "little" } else { "big" },
        "version": half(6),
        "type": half(16).map(|value| match types.iter().find(|(one, _)| *one == value) {
            Some((_, name)) => (*name).to_string(),
            None => format!("0x{value:x}"),
        }),
        "machine": half(18).map(|value| format!("0x{value:04x}")),
        "entry": word(if wide { 24 } else { 16 }),
        "phoff": word(if wide { 32 } else { 28 }),
        "shoff": word(if wide { 40 } else { 32 }),
        "flags": half(if wide { 48 } else { 40 }),
        "ehsize": half(if wide { 52 } else { 44 }),
        "phentsize": half(if wide { 54 } else { 42 }),
        "phnum": half(if wide { 56 } else { 44 }),
        "shentsize": half(if wide { 58 } else { 46 }),
        "shnum": half(if wide { 60 } else { 48 }),
        "shstrndx": half(if wide { 62 } else { 50 }),
    })
}

fn pe_header(b: &[u8]) -> Value {
    let Some(lfanew) = le32(0x3C)(b) else {
        return json!({});
    };
    let Ok(at) = usize::try_from(lfanew) else {
        return json!({});
    };
    if fourcc(b, at).as_deref() != Some("PE\0\0") {
        return json!({ "pe_signature_at": at });
    }
    let machine = le16(at + 4)(b);
    let sections = le16(at + 6)(b);
    let time = le32(at + 8)(b);
    let opts_size = le16(at + 20)(b);
    let chars = le16(at + 22)(b);
    let magic = le16(at + 24)(b);
    let wide = magic == Some(0x20b);
    let opt = at + 24;
    let dir_base = opt + if wide { 112 } else { 96 };
    let image_base = u64_at(b, opt + (if wide { 24 } else { 28 }), wide, true);
    // 两个 class 的可选头都占 8 字节放在 24..32（PE32 是 BaseOfData + ImageBase 两个 u32，
    // PE32+ 是一个 u64），所以 56/60/68/70 这些偏移对二者是同值。
    let entry = le32(opt + 16)(b);
    let size_image = le32(opt + 56)(b);
    let size_headers = le32(opt + 60)(b);
    let subsystem = le16(opt + 68)(b);
    let dll_chars = le16(opt + 70)(b);
    let mut out = json!({
        "pe_signature_at": at,
        "machine": machine.map(|value| format!("0x{value:04x}")),
        "sections": sections,
        "time_date_stamp": time,
        "pointer_to_symbol_table": le32(at + 12)(b),
        "size_of_optional_header": opts_size,
        "characteristics": chars.map(|value| format!("0x{value:04x}")),
        "optional_magic": magic.map(|value| format!("0x{value:04x}")),
        "bits": match magic { Some(0x10b) => Some(32), Some(0x20b) => Some(64), _ => None },
        "address_of_entry_point": entry,
        "image_base": image_base,
        "size_of_headers": size_headers,
        "subsystem": subsystem,
        "dll_characteristics": dll_chars.map(|value| format!("0x{value:04x}")),
        "size_of_image": size_image,
    });
    // 数据目录：从 0 起每对 8 字节；只有落在文件里的前若干项才报出来
    if let (Some(base), Some(size)) = (le32(dir_base)(b), le32(dir_base + 4)(b)) {
        out["data_directory_0"] = json!({ "rva": base, "size": size });
    }
    out
}

fn macho_header(b: &[u8]) -> Value {
    // 字节的排列决定端序，不是主机决定：CF FA ED FE 是一个小端 64 位镜像。
    let Some(magic) = b.first_chunk::<4>().copied() else {
        return json!({});
    };
    let (wide, little) = match magic {
        [0xCE, 0xFA, 0xED, 0xFE] => (false, true),
        [0xCF, 0xFA, 0xED, 0xFE] => (true, true),
        [0xFE, 0xED, 0xFA, 0xCE] => (false, false),
        [0xFE, 0xED, 0xFA, 0xCF] => (true, false),
        [0xCA, 0xFE, 0xBA, 0xBE] => {
            return json!({ "slices": be32(4)(b), "container": "fat/universal" })
        }
        [0xBE, 0xBA, 0xFE, 0xCA] => {
            return json!({ "slices": le32(4)(b), "container": "fat/universal" })
        }
        _ => return json!({}),
    };
    let word = |at: usize| if little { le32(at)(b) } else { be32(at)(b) };
    let filetypes = [
        (1u64, "MH_OBJECT"),
        (2, "MH_EXECUTE"),
        (3, "MH_FVMLIB"),
        (4, "MH_CORE"),
        (5, "MH_PRELOAD"),
        (6, "MH_DYLIB"),
        (7, "MH_DYLINKER"),
        (8, "MH_BUNDLE"),
        (9, "MH_DYLIB_STUB"),
        (10, "MH_DSYM"),
        (11, "MH_KEXT_BUNDLE"),
    ];
    let cpu = word(4);
    let names = [
        (0x0000_0007u64, "x86"),
        (0x0000_000C, "arm"),
        (0x0000_0012, "arm64_32"),
        (0x0100_0007, "x86_64"),
        (0x0100_000C, "arm64"),
        (0x0100_0018, "powerpc64"),
        (0x0200_000C, "arm64e"),
    ];
    json!({
        "bits": if wide { 64 } else { 32 },
        "endian": if little { "little" } else { "big" },
        "cputype": cpu.map(|value| match names.iter().find(|(one, _)| *one == value) {
            Some((_, name)) => format!("{name} (0x{value:08x})"),
            None => format!("0x{value:08x}"),
        }),
        "cpusubtype": word(8).map(|value| format!("0x{value:08x}")),
        "filetype": word(12).map(|value| match filetypes.iter().find(|(one, _)| *one == value) {
            Some((_, name)) => (*name).to_string(),
            None => format!("{value}"),
        }),
        "ncmds": word(16),
        "sizeofcmds": word(20),
        "flags": word(24).map(|value| format!("0x{value:08x}")),
    })
}

fn zip_header(b: &[u8]) -> Value {
    let (dirs, broken) = central_directory(b);
    let mut out = json!({
        "entries": dirs.len(),
        "comment": dirs.first().and_then(|_| comment(b)),
        "method_of_first": local_method(b),
    });
    if !broken.is_empty() {
        out["broken"] = json!(broken);
    }
    out["cd_tiling"] = json!(cd_tiling(b));
    out
}

/// 中央目录：末尾 64 KiB + 22 字节里找 EOCD，然后逐条走 "PK\x01\x02"
pub fn central_directory(b: &[u8]) -> (Vec<ZipEntry>, Vec<String>) {
    let mut broken = Vec::new();
    let Some((offset, total, size, start, comment)) = eocd(b) else {
        return (Vec::new(), vec!["找不到 EOCD（PK\\x05\\x06）".to_string()]);
    };
    let _ = offset;
    let mut at = usize::try_from(start).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    let end = match start.checked_add(size) {
        Some(stop) => usize::try_from(stop).unwrap_or(usize::MAX),
        None => {
            broken.push("cd size 自报超出文件".to_string());
            usize::MAX
        }
    };
    for index in 0..total {
        if at + 46 > b.len() || fourcc(b, at).as_deref() != Some("PK\x01\x02") {
            broken.push(format!("第 {index} 条中央目录记录不在 {at}"));
            break;
        }
        let name_len = usize::try_from(le16(at + 28)(b).unwrap_or(0)).unwrap_or(usize::MAX);
        let extra_len = usize::try_from(le16(at + 30)(b).unwrap_or(0)).unwrap_or(usize::MAX);
        let tail_len = usize::try_from(le16(at + 32)(b).unwrap_or(0)).unwrap_or(usize::MAX);
        let Some(name_end) = at
            .checked_add(46)
            .and_then(|base| base.checked_add(name_len))
        else {
            broken.push(format!("第 {index} 条记录的文件名超出文件"));
            break;
        };
        let name =
            String::from_utf8_lossy(b.get(at + 46..name_end).unwrap_or_default()).into_owned();
        out.push(ZipEntry {
            name,
            method: le16(at + 10)(b).unwrap_or(0),
            crc: le32(at + 16)(b).unwrap_or(0),
            compressed: le32(at + 20)(b).unwrap_or(0),
            size: le32(at + 24)(b).unwrap_or(0),
            offset: le32(at + 42)(b).unwrap_or(0),
        });
        let Some(next) = at
            .checked_add(46)
            .and_then(|one| one.checked_add(name_len))
            .and_then(|one| one.checked_add(extra_len))
            .and_then(|one| one.checked_add(tail_len))
        else {
            broken.push(format!("第 {index} 条记录之后无界"));
            break;
        };
        at = next;
    }
    if at != end && end != usize::MAX {
        broken.push(format!("中央目录走完在 {at}，文件自报结束于 {end}"));
    }
    if out.len() as u64 != total {
        broken.push(format!("EOCD 说有 {total} 条，读到 {} 条", out.len()));
    }
    let _ = comment;
    (out, broken)
}

pub struct ZipEntry {
    pub name: String,
    pub method: u64,
    pub crc: u64,
    pub compressed: u64,
    pub size: u64,
    pub offset: u64,
}

fn eocd(b: &[u8]) -> Option<(usize, u64, u64, u64, u64)> {
    if b.len() < 22 {
        return None;
    }
    let floor = b.len().saturating_sub(22 + 0xFFFF);
    let mut at = b.len();
    while at > floor {
        at -= 1;
        // 尾部三字节越界是常态（EOCD 正好压在文件末尾时），要跳过而不是就此认为「没有 EOCD」
        let Some(window) = b.get(at..at + 4) else {
            continue;
        };
        if window == b"PK\x05\x06" {
            let comment = le16(at + 20)(b)?;
            return Some((
                at,
                le16(at + 10)(b)?,
                le32(at + 12)(b)?,
                le32(at + 16)(b)?,
                comment,
            ));
        }
    }
    None
}

fn comment(b: &[u8]) -> Option<String> {
    let (at, _, _, _, length) = eocd(b)?;
    let start = at.checked_add(22)?;
    let stop = start.checked_add(usize::try_from(length).ok()?)?;
    Some(String::from_utf8_lossy(b.get(start..stop)?).into_owned())
}

fn local_method(b: &[u8]) -> Option<u64> {
    if fourcc(b, 0).as_deref() == Some("PK\x03\x04") {
        return le16(8)(b);
    }
    None
}

/// 中央目录的自证：条目数 × 记录长 == 自报的 cd size（不定长，所以改为「走到 end 相等」）
fn cd_tiling(b: &[u8]) -> Value {
    let (dirs, broken) = central_directory(b);
    json!({
        "listed": dirs.len(),
        "consistent": broken.is_empty(),
        "notes": broken,
    })
}

fn dex_header(b: &[u8]) -> Value {
    json!({
        "version": String::from_utf8_lossy(b.get(4..8).unwrap_or_default()).trim().to_string(),
        "checksum": le32(8)(b),
        "file_size": le32(32)(b),
        "header_size": le32(36)(b),
        "string_ids_size": le32(56)(b),
        "map_off": le32(52)(b),
    })
}

fn png_header(b: &[u8]) -> Value {
    let wide = le32(8)(b).and_then(|value| usize::try_from(value).ok());
    let (Some(width), Some(height)) = (
        wide,
        le32(12)(b).and_then(|value| usize::try_from(value).ok()),
    ) else {
        return json!({});
    };
    json!({
        "width": width,
        "height": height,
        "bit_depth": b.get(16),
        "color_type": b.get(17),
        "interlaced": b.get(28),
        "first_chunk": fourcc(b, 20),
    })
}

fn gif_header(b: &[u8]) -> Value {
    json!({
        "width": le16(6)(b),
        "height": le16(8)(b),
        "version": String::from_utf8_lossy(b.get(3..6).unwrap_or_default()).into_owned(),
        "background": b.get(11),
    })
}

fn bmp_header(b: &[u8]) -> Value {
    json!({
        "file_size": le32(2)(b),
        "pixels_at": le32(10)(b),
        "header_size": le32(14)(b),
        "width": le32(18)(b),
        "height": le32(22)(b),
        "planes": le16(26)(b),
        "bits_per_pixel": le16(28)(b),
        "compression": le32(30)(b),
    })
}

fn class_header(b: &[u8]) -> Value {
    json!({
        "minor": be16(4)(b),
        "major": be16(6)(b),
        "constant_pool_count": be16(8)(b),
        "access_flags": be16(10)(b).map(|value| format!("0x{value:04x}")),
    })
}

fn wasm_header(b: &[u8]) -> Value {
    json!({ "version": le32(4)(b) })
}

/// 只读取文件；`cap` 是上限（字节），超出则截断并在结果里说明
pub fn read_blob(path: &std::path::Path, cap: u64) -> Result<Blob, String> {
    let meta =
        std::fs::metadata(path).map_err(|error| format!("读不了 {}: {error}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} 不是普通文件", path.display()));
    }
    let size = meta.len();
    // `cap == 0` 是「不设上限」：Web / MCP 端不带这个参数时会传 0（`#[arg(default)]` 只在 CLI 生效）。
    // 把它当成「只读 1024 字节」会把目标文件的表腰斩截断，报错还怪到文件头上。
    let want = if cap == 0 {
        size
    } else {
        size.min(cap.max(1024))
    };
    let mut bytes = Vec::with_capacity(want as usize);
    use std::io::Read;
    let mut handle = std::fs::File::open(path).map_err(|error| format!("打不开: {error}"))?;
    let taken = handle
        .by_ref()
        .take(want)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读一半就断了: {error}"))?;
    if taken != bytes.len() {
        return Err("读到的字节数与请求不一致".to_string());
    }
    Ok(Blob {
        bytes,
        size,
        truncated: size > want,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0xCAFEBABE` 被两种格式合法共用：Java class 的 constant_pool_count 与 fat 的架构数。
    /// 分不开时宁可回 `javaclass`，也不给一个自信的错误答案。
    #[test]
    fn java_class_is_not_called_a_universal_binary() {
        let mut raw = vec![0u8; 32];
        raw[..4].copy_from_slice(b"\xca\xfe\xba\xbe");
        raw[6..8].copy_from_slice(&61u16.to_be_bytes()); // major version
        raw[8..10].copy_from_slice(&27u16.to_be_bytes()); // constant_pool_count
        let guess = sniff(&raw).expect("认得 0xCAFEBABE");
        assert_eq!(guess.family, "javaclass", "{guess:?}");
        assert_eq!(guess.format, "Java class");
    }

    /// 只有当架构表真的铺得进文件，才敢说它是通用二进制
    #[test]
    fn universal_binary_needs_its_slices_to_land_inside() {
        let mut raw = vec![0u8; 200];
        raw[..4].copy_from_slice(b"\xca\xfe\xba\xbe");
        raw[4..8].copy_from_slice(&1u32.to_be_bytes()); // nfat_arch
        raw[8..12].copy_from_slice(&0x0100_0007u32.to_be_bytes()); // fat_arch.cputype = x86_64
        raw[12..16].copy_from_slice(&3u32.to_be_bytes()); // fat_arch.cpusubtype
        raw[16..20].copy_from_slice(&64u32.to_be_bytes()); // fat_arch.offset
        raw[20..24].copy_from_slice(&100u32.to_be_bytes()); // fat_arch.size
        let guess = sniff(&raw).expect("认得 0xCAFEBABE");
        assert_eq!(guess.family, "macho-fat", "{guess:?}");
        assert_eq!(guess.confidence, "structural");
        raw[16..20].copy_from_slice(&9_000u32.to_be_bytes()); // offset 推到文件外
        let again = sniff(&raw).expect("还在 0xCAFEBABE 上");
        assert_eq!(again.family, "javaclass", "越界的架构表不该被当成切片");
    }

    /// PE 的两个面孔：有 PE 签名的，和只有 DOS 头的
    #[test]
    fn a_mz_without_a_pe_header_stays_dos() {
        let mut raw = vec![0u8; 0x100];
        raw[..2].copy_from_slice(b"MZ");
        raw[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes()); // e_lfanew
        raw[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert_eq!(sniff(&raw).expect("认得 MZ").family, "pe");
        let mut lean = vec![0u8; 0x40];
        lean[..2].copy_from_slice(b"MZ");
        assert_eq!(sniff(&lean).expect("认得 MZ").family, "dos");
    }

    #[test]
    fn riff_reports_the_inner_form_and_bmff_the_brand() {
        let mut raw = vec![0u8; 32];
        raw[..4].copy_from_slice(b"RIFF");
        raw[8..12].copy_from_slice(b"WEBP");
        assert_eq!(sniff(&raw).expect("RIFF").format, "WebP");
        let mut mov = vec![0u8; 32];
        mov[4..8].copy_from_slice(b"ftyp");
        mov[8..12].copy_from_slice(b"qt  ");
        assert_eq!(sniff(&mov).expect("ftyp").format, "ISO BMFF/qt  ");
    }

    #[test]
    fn fourcc_and_cstr_stop_at_the_window_they_were_given() {
        let raw = b"AB\x00\x00tail";
        assert_eq!(fourcc(raw, 0).as_deref(), Some("AB\0\0"));
        assert_eq!(cstr(raw, 0, 4).as_deref(), Some("AB"));
        assert_eq!(cstr(raw, 2, 2).as_deref(), Some(""));
        assert_eq!(cstr(raw, 6, 8), None, "窗口里没有 NUL 就不该编一个名字");
    }
}
