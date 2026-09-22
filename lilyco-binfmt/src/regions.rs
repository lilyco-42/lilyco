//! `lbin regions` — 把整份文件按区上色：头部 / 表 / 代码 / 数据 / 只读数据 / 复杂区 / 空闲区。
//!
//! 这套词表与 wasm 端那份分析模块完全同源，因为回答的是同一个问题：**文件自己的结构里，
//! 有没有谁指着这些字节**。所以：
//!
//! - `header`：文件头 + 头里那张表（ELF 的程序头与节头、PE 的段表、Mach-O 的 load command 链）；
//! - `tables`：被头指到的其他表（符号表、串表、重定位表、间接符号索引…）；
//! - `code` / `data` / `rodata`：加载器要映射进内存、而且文件自己说了它是什么的字节；
//! - `meta`：只在分析时起作用的区（`.debug*`、`.note*`、`__compact_unwind`、符号/串表…）；
//! - `gap`：没被任何表指到、也不在任何加载段里的字节 —— 这就是「安全区」（绿）。
//!   **绿色只说明「文件结构里没有谁指向这里」，不保证改了没事**：校验和、签名、
//!   以及按偏移自读的文件，这张图看不见；
//! - `overlay`：最后一个区之后剩下的尾巴（PE 的证书区常见）。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::read::{fourcc, read_blob, sniff, u64_at};

/// 一次列出的区数上限；总数照实在 `totals` 里给
const MAX_REGIONS: usize = 256;

#[derive(Clone, Debug)]
struct Span {
    start: u64,
    length: u64,
    kind: &'static str,
    name: String,
    note: String,
}

fn span(start: u64, length: u64, kind: &'static str, name: &str, note: &str) -> Option<Span> {
    if length == 0 {
        return None;
    }
    Some(Span {
        start,
        length,
        kind,
        name: name.to_string(),
        note: note.to_string(),
    })
}

/// 分区图（T0 只读）
#[derive(App)]
#[app(
    name = "regions",
    run = "run_regions",
    about = "Paint the whole file as byte regions and report which bytes nothing points at: returns { path, size, mapped, format, families of kinds, totals: {regions, file, claimed, unreferenced, loaded_unaddressed, listed, cut}, regions: [{ start, length, kind, name, note }] } with kind in {header, tables, code, data, rodata, meta, gap, overlay}. `gap`/green means no table and no loaded segment names those bytes - it is NOT a promise that editing them is safe (checksums, signatures and self-reading programs are invisible here). ELF, PE, Mach-O and PNG are mapped; anything else answers mapped=false with the reason, rather than inventing ranges. Read-only (safety T0): nothing is written, executed or patched."
)]
pub struct Regions {
    /// 要画的文件
    #[arg(about = "File to map", must_exist = true)]
    path: PathBuf,

    /// 最多读多少字节
    #[arg(
        about = "Read at most this many bytes; larger files report truncated=true",
        default = 33554432
    )]
    max_bytes: u64,
}

fn run_regions(app: &Regions, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.bytes.len() as u64),
        message: Some(format!("mapping {}", blob.bytes.len())),
    });
    let guess = sniff(&blob.bytes);
    let (mapped, reason, mut spans, loaded) = match guess.as_ref() {
        Some(one) => match one.family {
            "elf" => (
                true,
                String::new(),
                elf_spans(&blob.bytes),
                elf_loaded(&blob.bytes),
            ),
            "pe" => (true, String::new(), pe_spans(&blob.bytes), Vec::new()),
            "macho" | "macho-fat" => (true, String::new(), macho_spans(&blob.bytes), Vec::new()),
            "png" => (true, String::new(), png_spans(&blob.bytes), Vec::new()),
            other => (
                false,
                format!("族 {other} 的表还没实现，不替它编区间"),
                Vec::new(),
                Vec::new(),
            ),
        },
        None => (
            false,
            "认不出签名，没有表可画".to_string(),
            Vec::new(),
            Vec::new(),
        ),
    };
    let mut regions = Vec::new();
    let mut unreferenced = 0u64;
    let mut loaded_unaddressed = 0u64;
    let mut claimed = 0u64;
    if mapped {
        spans.sort_by(|left, right| {
            left.start
                .cmp(&right.start)
                .then_with(|| {
                    if left.kind == "gap" { 1 } else { 0 }
                        .cmp(&(if right.kind == "gap" { 1 } else { 0 }))
                })
                .then_with(|| left.name.cmp(&right.name))
        });
        let total = blob.bytes.len() as u64;
        let mut cursor = 0u64;
        for one in spans {
            if one.start >= total {
                break;
            }
            let mut at = one.start;
            let mut length = one.length;
            if at < cursor {
                let drop = cursor - at;
                if drop >= length {
                    continue;
                }
                at = cursor;
                length -= drop;
            }
            if at > cursor {
                let gap = at - cursor;
                let inside = loaded
                    .iter()
                    .any(|(from, stop)| *from <= cursor && at <= *stop);
                if inside {
                    loaded_unaddressed += gap;
                } else {
                    unreferenced += gap;
                }
                regions.push(Span {
                    start: cursor,
                    length: gap,
                    kind: "gap",
                    name: if inside {
                        "alignment padding"
                    } else {
                        "unreferenced"
                    }
                    .to_string(),
                    note: if inside {
                        "loaded but unaddressed"
                    } else {
                        "not loaded"
                    }
                    .to_string(),
                });
            }
            let Some(stop) = at.checked_add(length) else {
                break;
            };
            if stop > total {
                break;
            }
            if one.kind != "gap" && one.kind != "overlay" {
                claimed += length;
            }
            regions.push(Span {
                start: at,
                length,
                ..one
            });
            cursor = cursor.max(stop);
        }
        if cursor < total {
            let tail = total - cursor;
            unreferenced += tail;
            regions.push(Span {
                start: cursor,
                length: tail,
                kind: "overlay",
                name: "after the last table".to_string(),
                note: "not loaded".to_string(),
            });
        }
    }
    let listed = regions.len().min(MAX_REGIONS);
    let kinds: Vec<&'static str> = {
        let mut seen: Vec<&'static str> = Vec::new();
        for one in &regions {
            if !seen.contains(&one.kind) {
                seen.push(one.kind);
            }
        }
        seen
    };
    ctx.tick(regions.len() as u64, Some(regions.len() as u64), "");
    let rows: Vec<Value> = regions
        .iter()
        .take(listed)
        .map(|one| {
            json!({
                "start": one.start,
                "length": one.length,
                "kind": one.kind,
                "name": one.name,
                "note": if one.note.is_empty() { Value::Null } else { json!(one.note) },
            })
        })
        .collect();
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "truncated": blob.truncated,
        "mapped": mapped,
        "reason": if mapped { Value::Null } else { json!(reason) },
        "format": guess.as_ref().map(|one| one.format.clone()).unwrap_or_else(|| "unknown".to_string()),
        "kinds": kinds,
        "totals": {
            "regions": regions.len(),
            "listed": listed,
            "cut": regions.len() - listed,
            "file": blob.bytes.len(),
            "claimed": claimed,
            "unreferenced": unreferenced,
            "loaded_unaddressed": loaded_unaddressed,
        },
        "regions": rows,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 把 ELF 的两张头表、节区与元数据节都变成区
fn elf_spans(raw: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    let Some((wide, little, phoff, shoff, phentsize, phnum, shentsize, shnum, shstrndx)) =
        elf_shape(raw)
    else {
        return out;
    };
    // 头部只有 `e_ehsize`：可重定位目标文件没有程序头表，若把「到第一节头表之前」都算成头，
    // 整段节内容会被头部吃掉，剩下的地图就只剩两块并自称毫无空闲。
    let first = if phoff == 0 {
        shoff
    } else if shoff == 0 {
        phoff
    } else {
        phoff.min(shoff)
    };
    let smallest = if wide { 64u64 } else { 52u64 };
    let ehsize = half(raw, if wide { 52 } else { 40 }, little)
        .unwrap_or(smallest)
        .clamp(smallest, first.max(smallest));
    out.extend(span(0, ehsize, "header", "elf header", ""));
    out.extend(span(
        phoff,
        phentsize.saturating_mul(phnum),
        "tables",
        "program headers",
        "",
    ));
    out.extend(span(
        shoff,
        shentsize.saturating_mul(shnum),
        "tables",
        "section headers",
        "",
    ));
    let strtab = shoff
        .checked_add(shstrndx.saturating_mul(shentsize))
        .and_then(|entry| {
            u64_at(
                raw,
                entry as usize + if wide { 24 } else { 16 },
                wide,
                little,
            )
        })
        .unwrap_or(0);
    let mut index = 0u64;
    while index < shnum.min(256) {
        let Some(at) = shoff.checked_add(index.saturating_mul(shentsize)) else {
            break;
        };
        index += 1;
        let Some(name_off) = word(raw, at as usize, little) else {
            break;
        };
        let Some(sh_type) = word(raw, at as usize + 4, little) else {
            break;
        };
        let Some(sh_flags) = u64_at(raw, at as usize + 8, wide, little) else {
            break;
        };
        let fields = (
            u64_at(raw, at as usize + if wide { 16 } else { 12 }, wide, little),
            u64_at(raw, at as usize + if wide { 24 } else { 16 }, wide, little),
            u64_at(raw, at as usize + if wide { 32 } else { 20 }, wide, little),
        );
        let (Some(sh_addr), Some(sh_offset), Some(sh_size)) = fields else {
            break;
        };
        if sh_type == 0 || sh_type == 8 || sh_size == 0 {
            // NULL 没有内容，NOBITS（.bss 那类）在文件里没有字节：两者都画不出区
            continue;
        }
        let name = strtab
            .checked_add(name_off)
            .map(|where_at| crate::read::cstr(raw, where_at as usize, 64).unwrap_or_default())
            .unwrap_or_default();
        let allocated = sh_flags & 2 != 0;
        let meta = !allocated
            || [2u64, 3, 4, 5, 6, 7, 9, 0x6FFF_FF00, 0x6FFF_FFFF].contains(&sh_type)
            || name.starts_with(".debug")
            || name.starts_with(".zdebug")
            || name.starts_with(".comment")
            || name.starts_with(".note")
            || name.starts_with(".rel")
            || name.starts_with(".symtab")
            || name.starts_with(".strtab");
        let kind = if meta {
            "meta"
        } else if sh_flags & 4 != 0 || name.starts_with(".text") || name.starts_with(".plt") {
            "code"
        } else if sh_flags & 1 != 0 {
            "data"
        } else {
            "rodata"
        };
        let note = if allocated {
            format!("loaded at {sh_addr:#x}")
        } else {
            "not loaded".to_string()
        };
        out.extend(span(sh_offset, sh_size, kind, &name, &note));
    }
    out
}

/// 加载器要映射的地址范围（PT_LOAD），用来区分「对齐填充」与「谁都不指的空闲」
fn elf_loaded(raw: &[u8]) -> Vec<(u64, u64)> {
    let Some((wide, little, phoff, _, phentsize, phnum, ..)) = elf_shape(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut index = 0u64;
    while index < phnum.min(64) {
        let Some(at) = phoff.checked_add(index.saturating_mul(phentsize)) else {
            break;
        };
        index += 1;
        if word(raw, at as usize, little) != Some(1) {
            continue;
        }
        let pair = if wide {
            (
                u64_at(raw, at as usize + 8, true, little),
                u64_at(raw, at as usize + 32, true, little),
            )
        } else {
            (
                word(raw, at as usize + 4, little),
                word(raw, at as usize + 16, little),
            )
        };
        if let (Some(from), Some(size)) = pair {
            if let Some(stop) = from.checked_add(size) {
                out.push((from, stop));
            }
        }
    }
    out
}

#[allow(clippy::type_complexity)]
fn elf_shape(raw: &[u8]) -> Option<(bool, bool, u64, u64, u64, u64, u64, u64, u64)> {
    if raw.len() < 64 || !raw.starts_with(b"\x7fELF") {
        return None;
    }
    let wide = raw.get(4) == Some(&2);
    let little = raw.get(5) == Some(&1);
    Some((
        wide,
        little,
        u64_at(raw, if wide { 32 } else { 28 }, wide, little)?,
        u64_at(raw, if wide { 40 } else { 32 }, wide, little)?,
        half(raw, if wide { 54 } else { 42 }, little)?,
        half(raw, if wide { 56 } else { 44 }, little)?,
        half(raw, if wide { 58 } else { 46 }, little)?,
        half(raw, if wide { 60 } else { 48 }, little)?,
        half(raw, if wide { 62 } else { 50 }, little)?,
    ))
}

/// 两个字节：`e_phentsize` 到 `e_shstrndx` 这五个槽都是 u16，用四个字节的读法会把相邻那一个
/// 当成高位乘进来（节头表项尺寸会变成 131136 而不是 64），整张表就跑到文件外了。
fn half(raw: &[u8], at: usize, little: bool) -> Option<u64> {
    let pair = [raw.get(at).copied()?, raw.get(at + 1).copied()?];
    Some(if little {
        u16::from_le_bytes(pair) as u64
    } else {
        u16::from_be_bytes(pair) as u64
    })
}

fn word(raw: &[u8], at: usize, little: bool) -> Option<u64> {
    if little {
        u64_at(raw, at, false, true)
    } else {
        u64_at(raw, at, false, false)
    }
}

fn pe_spans(raw: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    let Some(lfanew) = u64_at(raw, 0x3C, false, true) else {
        return out;
    };
    let Ok(sign) = usize::try_from(lfanew) else {
        return out;
    };
    if fourcc(raw, sign).as_deref() != Some("PE\0\0") {
        return out;
    }
    // 这两槽都是 u16：按四个字节读会把相邻字段乘进高位（SizeOfOptionalHeader 变成 0x22_00f0，
    // 段表因此跑到文件外），整张段表就一节也画不出来。
    let Some(opts_size) = half(raw, sign + 20, true) else {
        return out;
    };
    let opt = sign + 24;
    let Some(magic) = word(raw, opt, true) else {
        return out;
    };
    let wide = magic == 0x20b;
    let step: usize = if wide { 112 } else { 96 };
    let Some(size_headers) = u64_at(raw, opt + 60, false, true) else {
        return out;
    };
    let Some(section_count) = half(raw, sign + 6, true) else {
        return out;
    };
    // 段表项是定长 40 字节：COFF 头里没有「项大小」这个字段
    let section_bytes: u64 = 40;
    // DOS 头 + 签名 + 文件头 + 可选头 + 段表：这些是「头部」，段表本身是「表」
    out.extend(span(0, size_headers, "header", "pe headers", ""));
    let body = sign as u64 + 24 + opts_size;
    let mut index = 0u64;
    let mut cursor = body;
    let mut dirs = Vec::new();
    while index < section_count.min(96) {
        let at = cursor as usize;
        let Some(name_bytes) = raw.get(at..at + 8) else {
            break;
        };
        let name = String::from_utf8_lossy(name_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        let virtual_size = u64_at(raw, at + 8, false, true).unwrap_or(0);
        let raw_size = u64_at(raw, at + 16, false, true).unwrap_or(0);
        let raw_at = u64_at(raw, at + 20, false, true).unwrap_or(0);
        let chars = u64_at(raw, at + 36, false, true).unwrap_or(0);
        if raw_size > 0 {
            let kind = if chars & 0x2000_0000 != 0 {
                "code"
            } else if chars & 0x8000_0000 != 0 {
                "data"
            } else {
                "rodata"
            };
            let note = if virtual_size > raw_size {
                format!("{virtual_size} bytes in memory, {raw_size} on disk")
            } else {
                String::new()
            };
            out.extend(span(raw_at, raw_size, kind, &name, &note));
        }
        index += 1;
        cursor = cursor.saturating_add(section_bytes);
    }
    // 数据目录 4 是证书区，它按文件偏移而非 RVA 计数，所以是一个「叠在镜像上」的区
    if let (Some(at), Some(size)) = (
        u64_at(raw, opt + step.saturating_add(32), false, true),
        u64_at(raw, opt + step.saturating_add(36), false, true),
    ) {
        if at > 0 && size > 0 {
            dirs.push(at);
            out.extend(span(
                at,
                size,
                "meta",
                "certificate",
                "签名或时间戳，按文件偏移计",
            ));
        }
    }
    out
}

fn macho_spans(raw: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    let Some(magic) = raw.first_chunk::<4>().copied() else {
        return out;
    };
    // 头的字节排列决定后面每个字段的端序：CF FA ED FE 是一个小端 64 位镜像。
    let (wide, little) = match magic {
        [0xCE, 0xFA, 0xED, 0xFE] => (false, true),
        [0xCF, 0xFA, 0xED, 0xFE] => (true, true),
        [0xFE, 0xED, 0xFA, 0xCE] => (false, false),
        [0xFE, 0xED, 0xFA, 0xCF] => (true, false),
        [0xCA, 0xFE, 0xBA, 0xBE] => return fat_spans(raw, false),
        [0xBE, 0xBA, 0xFE, 0xCA] => return fat_spans(raw, true),
        _ => return out,
    };
    let word = |at: usize| u64_at(raw, at, false, little);
    let entry: u64 = if wide { 32 } else { 28 };
    let Some(ncmds) = word(16) else {
        return out;
    };
    let Some(cmdbytes) = word(20) else {
        return out;
    };
    out.extend(span(
        0,
        entry.saturating_add(cmdbytes),
        "header",
        "mach-o header",
        "",
    ));
    let mut at = entry;
    let mut index = 0u64;
    while index < ncmds.min(256) {
        let Some(cmd) = word(at as usize) else {
            break;
        };
        let Some(size) = word(at as usize + 4) else {
            break;
        };
        let room = match usize::try_from(size) {
            Ok(value) if value >= 8 => value,
            _ => break,
        };
        if at as usize + room > raw.len() {
            break;
        }
        if cmd == 0x19 || cmd == 1 {
            for one in macho_sections(raw, at as usize, wide, little, room) {
                out.push(one);
            }
        } else if cmd == 2 {
            let symoff = word(at as usize + 8).unwrap_or(0);
            let nsyms = word(at as usize + 12).unwrap_or(0);
            let stroff = word(at as usize + 16).unwrap_or(0);
            let strsize = word(at as usize + 20).unwrap_or(0);
            out.extend(span(
                symoff,
                nsyms.saturating_mul(if wide { 16 } else { 12 }),
                "tables",
                "symbol table",
                "",
            ));
            out.extend(span(stroff, strsize, "tables", "symbol names", ""));
        } else if cmd == 0xb {
            let where_at = word(at as usize + 56).unwrap_or(0);
            let count = word(at as usize + 60).unwrap_or(0);
            if count > 0 {
                out.extend(span(
                    where_at,
                    count.saturating_mul(4),
                    "tables",
                    "indirect symbols",
                    "",
                ));
            }
        }
        at += size;
        index += 1;
    }
    out
}

fn macho_sections(raw: &[u8], at: usize, wide: bool, little: bool, room: usize) -> Vec<Span> {
    let record: usize = if wide { 80 } else { 68 };
    let head: usize = if wide { 72 } else { 56 };
    let Some(count) = u64_at(raw, at + if wide { 64 } else { 48 }, false, little) else {
        return Vec::new();
    };
    let span_bytes = record as u64;
    let need = usize::try_from(count.saturating_mul(span_bytes)).unwrap_or(usize::MAX);
    if head + need > room {
        return Vec::new();
    }
    let tail = if wide { 48 } else { 40 };
    let owner = crate::read::fixed_name(raw, at + 8, 16);
    let mut out = Vec::new();
    for each in 0..count.min(256) as usize {
        let p = at + head + each * record;
        let sect = crate::read::fixed_name(raw, p, 16);
        // 节自带的段名经常是空的（尤其是可执行镜像），就取所属 LC_SEGMENT 的名字
        let own = crate::read::fixed_name(raw, p + 16, 16);
        let seg = if own.is_empty() { owner.clone() } else { own };
        let (Some(size), Some(offset)) = (
            u64_at(raw, p + if wide { 40 } else { 36 }, wide, little),
            u64_at(raw, p + tail, false, little),
        ) else {
            break;
        };
        let reloff = u64_at(raw, p + tail + 8, false, little).unwrap_or(0);
        let nreloc = u64_at(raw, p + tail + 12, false, little).unwrap_or(0);
        let flags = u64_at(raw, p + tail + 16, false, little).unwrap_or(0);
        let addr = u64_at(raw, p + 32, wide, little).unwrap_or(0);
        if nreloc > 0 {
            out.extend(span(
                reloff,
                nreloc.saturating_mul(8),
                "tables",
                &format!("relocations in {sect}"),
                "",
            ));
        }
        // 全零节（__bss / __common 这类）的 offset 是 0：它们在文件里不占字节，
        // 真涂上去就把文件头当成数据了。
        if offset == 0 {
            continue;
        }
        if flags & 0x8000_0000 != 0 {
            out.extend(span(
                offset,
                size,
                "code",
                &format!("{seg},{sect}"),
                &format!("loaded at {addr:#x}"),
            ));
        } else if flags & 0x0200_0000 != 0 {
            out.extend(span(
                offset,
                size,
                "meta",
                &format!("{seg},{sect}"),
                "S_ATTR_DEBUG",
            ));
        } else if seg == "__TEXT" {
            out.extend(span(
                offset,
                size,
                "rodata",
                &format!("{seg},{sect}"),
                &format!("loaded at {addr:#x}"),
            ));
        } else if seg == "__DATA" {
            out.extend(span(
                offset,
                size,
                "data",
                &format!("{seg},{sect}"),
                &format!("loaded at {addr:#x}"),
            ));
        }
    }
    out
}

/// 通用二进制：头 + 架构表，再加上每个切片自报的范围
fn fat_spans(raw: &[u8], little: bool) -> Vec<Span> {
    let mut out = Vec::new();
    let Some(count) = u64_at(raw, 4, false, little) else {
        return out;
    };
    if count == 0 || count > 64 {
        return out;
    }
    out.extend(span(
        0,
        8u64.saturating_add(count.saturating_mul(20)),
        "header",
        "fat header",
        "",
    ));
    let mut index = 0u64;
    while index < count {
        let at = 8 + index.saturating_mul(20) as usize;
        let (Some(offset), Some(size)) = (
            u64_at(raw, at + 8, false, little),
            u64_at(raw, at + 12, false, little),
        ) else {
            break;
        };
        if offset + size <= raw.len() as u64 {
            out.extend(span(
                offset,
                size,
                "code",
                &format!("slice {}", index + 1),
                "文件自报的架构切片",
            ));
        }
        index += 1;
    }
    out
}

fn png_spans(raw: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    out.extend(span(0, 8, "header", "png signature", ""));
    let mut at = 8u64;
    let mut index = 0;
    while at.saturating_add(12) <= raw.len() as u64 && index < 512 {
        let Some(length) = u64_at(raw, at as usize, false, false) else {
            break;
        };
        let name = crate::read::cstr4(raw, at.saturating_add(4) as usize).unwrap_or_default();
        let body = at.saturating_add(8);
        let Some(stop) = body.checked_add(length).and_then(|one| one.checked_add(4)) else {
            break;
        };
        if stop > raw.len() as u64 {
            out.push(Span {
                start: at,
                length: raw.len() as u64 - at,
                kind: "gap",
                name: "truncated chunk".to_string(),
                note: format!("{name} 自报 {length} 字节，文件没那么多"),
            });
            break;
        }
        let crc_ok = crc32(&raw[body.saturating_sub(4) as usize..stop as usize - 4])
            == u64_at(raw, stop as usize - 4, false, false).unwrap_or(u64::MAX);
        let kind = match name.as_str() {
            "IHDR" | "PLTE" | "tRNS" | "gAMA" | "cHRM" | "iCCP" | "sRGB" | "pHYs" | "bKGD" => {
                "meta"
            }
            "IDAT" => "data",
            "IEND" => "meta",
            _ => "meta",
        };
        out.push(Span {
            start: at,
            length: stop - at,
            kind,
            name: name.clone(),
            note: if crc_ok {
                "crc ok".to_string()
            } else {
                "crc 不对".to_string()
            },
        });
        at = stop;
        index += 1;
        if name == "IEND" {
            break;
        }
    }
    out
}

/// PNG 的块 CRC 用的是多项式 0xEDB88320，这里按需算，不查表（块数有限）
fn crc32(bytes: &[u8]) -> u64 {
    let mut crc = 0xFFFF_FFFFu64;
    for byte in bytes {
        crc ^= u64::from(*byte);
        for _ in 0..8 {
            let hit = crc & 1;
            crc >>= 1;
            if hit == 1 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tiny_elf;
    use lilyco_core::Context;
    use std::sync::mpsc;

    fn run(path: PathBuf) -> Result<Value, AppError> {
        let app = Regions {
            path,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        run_regions(&app, &Context::new_test(tx))
    }

    /// 图必须把文件铺满：claimed + unreferenced + loaded-unaddressed == 读进来的字节数
    #[test]
    fn the_map_tiles_the_file_it_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.out");
        std::fs::write(&path, tiny_elf()).expect("写 ELF");
        let out = run(path).expect("regions 应成功");
        assert_eq!(out["mapped"], true);
        let totals = &out["totals"];
        assert_eq!(totals["file"], out["size"], "图的长度就是读进来的文件长度");
        assert_eq!(totals["file"], 203);
        assert_eq!(
            totals["claimed"].as_u64().unwrap()
                + totals["unreferenced"].as_u64().unwrap()
                + totals["loaded_unaddressed"].as_u64().unwrap(),
            203,
            "{out}"
        );
        let kinds: Vec<&str> = out["kinds"]
            .as_array()
            .expect("kinds")
            .iter()
            .map(|one| one.as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"header"), "{kinds:?}");
        assert!(
            kinds.contains(&"tables"),
            "节表本身要当成表画出来：{kinds:?}"
        );
    }

    /// PNG 的块 CRC 是真算的：改一个字节就得从 `crc ok` 变成 `crc 不对`
    #[test]
    fn png_chunk_crcs_are_actually_computed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.png");
        std::fs::write(&path, tiny_png()).expect("写 PNG");
        let out = run(path.clone()).expect("regions 应成功");
        let rows = out["regions"].as_array().expect("regions");
        assert!(rows.iter().any(|one| one["note"] == "crc ok"), "{out}");
        let mut broken = tiny_png();
        let last = broken.len() - 5;
        broken[last] ^= 0xff; // 动 IHDR 名字之外的字节，CRC 就对不上了
        std::fs::write(&path, &broken).expect("再写一次");
        let out = run(path).expect("regions 应成功");
        let rows = out["regions"].as_array().expect("regions");
        assert!(
            rows.iter().any(|one| one["note"] == "crc 不对"),
            "改了字节却说 crc ok：{out}"
        );
    }

    /// 认不出来的文件：mapped=false，并说清为什么，而不是画一张假图
    #[test]
    fn unmapped_bytes_say_so_instead_of_inventing_ranges() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("g.bin");
        std::fs::write(&path, b"random-ish bytes with no signature whatsoever").expect("写文件");
        let out = run(path).expect("regions 应成功");
        assert_eq!(out["mapped"], false);
        assert_eq!(out["regions"].as_array().expect("regions").len(), 0);
        assert!(
            out["reason"].as_str().unwrap_or("").contains("签名"),
            "{out}"
        );
    }

    /// 一个块表完整的最小 PNG：签名 + IHDR + IEND，CRC 全部真算
    fn tiny_png() -> Vec<u8> {
        let mut raw = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&8u32.to_be_bytes());
        ihdr.extend_from_slice(&6u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        raw.extend_from_slice(&chunk(b"IHDR", &ihdr));
        raw.extend_from_slice(&chunk(b"IEND", &[]));
        raw
    }

    fn chunk(name: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(body);
        let mut crc_input = name.to_vec();
        crc_input.extend_from_slice(body);
        out.extend_from_slice(&(crc32(&crc_input) as u32).to_be_bytes());
        out
    }

    fn p32(raw: &mut [u8], at: usize, value: u32, little: bool) {
        if little {
            raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
        } else {
            raw[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
    }

    fn p64(raw: &mut [u8], at: usize, value: u64, little: bool) {
        if little {
            raw[at..at + 8].copy_from_slice(&value.to_le_bytes());
        } else {
            raw[at..at + 8].copy_from_slice(&value.to_be_bytes());
        }
    }

    /// 只有节头表、没有程序头表的目标文件：头部只能到 `e_ehsize`。把「到第一节头表之前」
    /// 都算成头会让整段节内容消失，而图还自称一个空闲字节都没有。
    #[test]
    fn an_object_without_program_headers_keeps_its_section_bodies() {
        let mut raw = vec![0u8; 64 + 16 + 64 * 2];
        raw[..4].copy_from_slice(&[127u8, b'E', b'L', b'F']);
        raw[4] = 2; // ELFCLASS64
        raw[5] = 1; // ELFDATA2LSB
        raw[6] = 1;
        raw[16..18].copy_from_slice(&1u16.to_le_bytes()); // ET_REL
        raw[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        raw[20..24].copy_from_slice(&1u32.to_le_bytes());
        raw[40..48].copy_from_slice(&80u64.to_le_bytes()); // e_shoff，节内容在头与表之间
        raw[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        raw[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        raw[60..62].copy_from_slice(&2u16.to_le_bytes()); // e_shnum
        raw[64..80].copy_from_slice(&[0x90u8; 16]); // .text 的正文
        let shdr = 64 + 16 + 64; // 跳过全零的 NULL 节头
        raw[shdr + 4..shdr + 8].copy_from_slice(&1u32.to_le_bytes()); // SHT_PROGBITS
        raw[shdr + 8..shdr + 16].copy_from_slice(&6u64.to_le_bytes()); // ALLOC|EXECINSTR
        raw[shdr + 16..shdr + 24].copy_from_slice(&0x1000u64.to_le_bytes());
        raw[shdr + 24..shdr + 32].copy_from_slice(&64u64.to_le_bytes()); // sh_offset
        raw[shdr + 32..shdr + 40].copy_from_slice(&16u64.to_le_bytes()); // sh_size
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.o");
        std::fs::write(&path, raw).expect("写目标文件");
        let out = run(path).expect("regions 应成功");
        let rows = out["regions"].as_array().expect("regions");
        let head = rows
            .iter()
            .find(|one| one["name"] == "elf header")
            .expect("头部区");
        assert_eq!(head["length"], 64, "头部只能到 e_ehsize：{out}");
        assert!(
            rows.iter()
                .any(|one| one["start"] == 64 && one["length"] == 16 && one["kind"] == "code"),
            "节内容被头部吃掉了：{out}"
        );
    }

    /// 端序由头的字节排列决定；16 字节的节名占满时没有结尾符；零填充节在文件里没有字节
    fn tiny_macho(little: bool) -> Vec<u8> {
        let mut raw = vec![0u8; 32 + 232 + 8];
        let magic: [u8; 4] = if little {
            [0xCF, 0xFA, 0xED, 0xFE]
        } else {
            [0xFE, 0xED, 0xFA, 0xCF]
        };
        raw[..4].copy_from_slice(&magic);
        p32(&mut raw, 4, 0x0100_0007, little); // CPU_TYPE_X86_64
        p32(&mut raw, 12, 1, little); // MH_OBJECT
        p32(&mut raw, 16, 1, little); // ncmds
        p32(&mut raw, 20, 232, little); // sizeofcmds
        raw[40..44].copy_from_slice(b"__LD");
        p32(&mut raw, 32, 0x19, little); // LC_SEGMENT_64
        p32(&mut raw, 36, 232, little); // cmdsize
        p32(&mut raw, 96, 2, little); // nsects
        p64(&mut raw, 56, 0x1000, little); // vmaddr
        p64(&mut raw, 64, 264, little); // vmsize
        p64(&mut raw, 72, 0, little); // fileoff
        p64(&mut raw, 80, 264, little); // filesize
                                        // section[0]：节名正好 16 字节，没有结尾符
        raw[104..120].copy_from_slice(b"__compact_unwind");
        raw[120..124].copy_from_slice(b"__LD");
        p64(&mut raw, 136, 0x2000, little); // addr
        p64(&mut raw, 144, 8, little); // size
        p32(&mut raw, 152, 264, little); // offset
        p32(&mut raw, 168, 0x0200_0000, little); // S_ATTR_DEBUG
                                                 // section[1]：S_ZEROFILL，offset 为 0，在文件里没有字节
        raw[184..190].copy_from_slice(b"__zero");
        raw[200..206].copy_from_slice(b"__DATA");
        p64(&mut raw, 216, 0x3000, little);
        p64(&mut raw, 224, 8, little);
        p32(&mut raw, 248, 1, little); // S_ZEROFILL
        raw[264..272].copy_from_slice(&[0x11u8; 8]);
        raw
    }

    #[test]
    fn the_macho_walk_follows_the_byte_order_the_header_declares() {
        for little in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(if little { "le.mh" } else { "be.mh" });
            std::fs::write(&path, tiny_macho(little)).expect("写 Mach-O");
            let out = run(path).expect("regions 应成功");
            let rows = out["regions"].as_array().expect("regions");
            let found = rows
                .iter()
                .find(|one| one["name"] == "__LD,__compact_unwind")
                .unwrap_or_else(|| panic!("little={little} 时找不到那节，图是 {out}"));
            assert_eq!(found["kind"], "meta", "{out}");
            assert_eq!(found["start"], 264, "{out}");
            assert_eq!(found["length"], 8, "{out}");
            assert_eq!(found["note"], "S_ATTR_DEBUG", "{out}");
            assert!(
                !rows.iter().any(|one| one["kind"] == "data"),
                "零填充节没有文件字节，不该画出数据区：{out}"
            );
            assert!(
                rows.iter()
                    .all(|one| one["start"] != 0 || one["kind"] == "header"),
                "0 处只能是头部：{out}"
            );
        }
    }
}
