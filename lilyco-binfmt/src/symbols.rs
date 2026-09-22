//! `lbin symbols` — 目标文件的节表、符号表与「地址→名字」索引。
//!
//! 走的是 `object` 那份统一的读取层，所以 ELF / PE / COFF / Mach-O 得到同一张表。
//! 一件必须记住的事：**Mach-O 的 `n_sect` 从 1 开始数**，而它自己的节表按下标 0 起列 ——
//! `object` 在两个方向上都保留这个 1-based 数（用 `section_by_index` 查名字是对的，
//! 但和节表列表并列出来就大 1），所以本命令在打印「在第几个节」时先减一，
//! 别的地方一律照文件的说法。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;
use object::{Object, ObjectSection, ObjectSymbol};

use crate::read::read_blob;

/// `--limit` 的缺省值。CLI 那个 `#[arg(default = 128)]` 与「不带这个参数的端」
/// （Web 表单留空 / MCP 省略）必须落在同一个数上，不然四端会给出长短不一的同一张表。
const DEFAULT_LIMIT: u64 = 128;

/// 节与符号（T0 只读）
#[derive(App)]
#[app(
    name = "symbols",
    run = "run_symbols",
    about = "Read an object file the way an analyser does: format, class (relocatable/executable/dynamic), machine, bitness, endianness, entry point, then its sections (name, address, file offset, size on disk, alignment) and its symbol tables - both .symtab and .dynsym, because a stripped distribution binary keeps only the second one - with each symbol's name, value, size, kind and which section owns it, plus an address-to-name index over all of them. Returns { path, format, bits, endian, kind, machine, entry, sections: [...], sections_total, symbols: [...], symbols_total, dynamic_total, names: [{ address, name }], names_total, cut }. Mach-O section numbers are one-based in the file and are reported here as the index the file's own section list uses. Read-only (safety T0): nothing is loaded, applied or executed."
)]
pub struct Symbols {
    /// 目标文件
    #[arg(about = "Object file to read", must_exist = true)]
    path: PathBuf,

    /// 每个表最多列几条
    #[arg(
        about = "List at most this many sections and symbols per table",
        default = 128,
        min = 1
    )]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

fn run_symbols(app: &Symbols, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    // 0 是 Web / MCP 端不带 `limit` 时的取值（`#[arg(default)]` 只在 CLI 生效）。
    // 当成「只列一条」会交出一份看着完整、其实被砍到一行的表，所以按文档缺省办。
    let limit = usize::try_from(if app.limit == 0 {
        DEFAULT_LIMIT
    } else {
        app.limit
    })
    .unwrap_or(usize::MAX);
    ctx.emit(Progress::Started {
        total: Some(blob.bytes.len() as u64),
        message: Some("parsing sections and symbols".to_string()),
    });
    let file = object::File::parse(&blob.bytes[..]).map_err(|error| {
        // 读截断了就别怪文件：把「只读了多少」说出来，用户才知道要加 --max-bytes
        let hint = if blob.truncated {
            format!(
                "（只读了 {} / {} 字节，加大 --max-bytes 再看）",
                blob.bytes.len(),
                blob.size
            )
        } else {
            String::new()
        };
        AppError::InvalidInput(format!("不是本读取层认识的目标文件：{error}{hint}"))
    })?;
    let one_based = label(&file.format()) == "macho";
    let mut sections = Vec::new();
    let mut sections_total = 0usize;
    for (index, section) in file.sections().enumerate() {
        sections_total += 1;
        if index >= limit {
            continue;
        }
        let range = section.file_range();
        sections.push(json!({
            "index": index,
            "name": clean(section.name().unwrap_or("?")),
            "address": section.address(),
            "offset": range.map_or(Value::Null, |(where_at, _)| json!(where_at)),
            "size": section.size(),
            "on_disk": range.map_or(0, |(_, size)| size),
            "align": section.align(),
            "kind": label(&section.kind()),
        }));
    }
    let mut symbols = Vec::new();
    let mut symbols_total = 0usize;
    let mut names: Vec<(u64, String)> = Vec::new();
    for (index, symbol) in file.symbols().enumerate() {
        symbols_total += 1;
        if let (false, Some(where_)) = (symbol.is_undefined(), symbol.section_index()) {
            if !clean(symbol.name().unwrap_or("")).trim().is_empty() {
                names.push((symbol.address(), clean(symbol.name().unwrap_or("?"))));
            }
            let _ = where_;
        }
        if index >= limit {
            continue;
        }
        symbols.push(symbol_row("symtab", index, &symbol, one_based));
    }
    let mut dynamic = Vec::new();
    let mut dynamic_total = 0usize;
    for (index, symbol) in file.dynamic_symbols().enumerate() {
        dynamic_total += 1;
        if symbol.is_undefined() {
            if let Ok(name) = symbol.name() {
                let text = clean(name);
                if !text.trim().is_empty() {
                    names.push((symbol.address(), text));
                }
            }
        }
        if index >= limit {
            continue;
        }
        dynamic.push(symbol_row("dynsym", index, &symbol, one_based));
    }
    names.sort_by_key(|one| one.0);
    names.dedup_by(|later, earlier| later.0 == earlier.0);
    let names_total = names.len();
    let rows: Vec<Value> = names
        .into_iter()
        .take(limit)
        .map(|(address, name)| json!({ "address": format!("0x{address:x}"), "name": name }))
        .collect();
    ctx.tick(symbols_total as u64, Some(symbols_total as u64), "");
    let result = json!({
        "path": app.path.to_string_lossy(),
        "size": blob.size,
        "format": label(&file.format()),
        "bits": if file.is_64() { 64 } else { 32 },
        "endian": if file.is_little_endian() { "little" } else { "big" },
        "kind": label(&file.kind()),
        "machine": label(&file.architecture()),
        "entry": file.entry(),
        "sections": sections,
        "sections_total": sections_total,
        "symbols": symbols,
        "symbols_total": symbols_total,
        "dynamic": dynamic,
        "dynamic_total": dynamic_total,
        "names": rows,
        "names_total": names_total,
        "limit": limit,
        "cut": symbols_total.saturating_sub(limit.min(symbols_total)),
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn symbol_row(
    prefix: &str,
    index: usize,
    symbol: &object::Symbol<'_, '_>,
    one_based: bool,
) -> Value {
    let home = symbol.section_index().map(|id| {
        if one_based {
            id.0.saturating_sub(1)
        } else {
            id.0
        }
    });
    json!({
        "table": prefix,
        "index": index,
        "name": clean(symbol.name().unwrap_or("?")),
        "address": symbol.address(),
        "size": symbol.size(),
        "kind": label(&symbol.kind()),
        "section": home,
        "undefined": symbol.is_undefined(),
        "global": symbol.is_global(),
    })
}

/// 名字里出现制表符或换行会凭空多出一列，所以一律换成空格
fn clean(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\t' | '\n' | '\r' => ' ',
            other => other,
        })
        .collect()
}

/// 枚举的小写拼写：行里说 `elf`、`relocatable`，而不是某个 Debug 输出的原始大小写
fn label<T: std::fmt::Debug>(value: &T) -> String {
    format!("{value:?}")
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tiny_elf;
    use std::sync::mpsc;

    fn run(path: PathBuf) -> Result<Value, AppError> {
        let app = Symbols {
            path,
            limit: 64,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        run_symbols(&app, &Context::new_test(tx))
    }

    #[test]
    fn an_elf_reports_what_its_header_and_tables_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.out");
        std::fs::write(&path, tiny_elf()).expect("写 ELF");
        let out = run(path).expect("symbols 应成功");
        assert_eq!(out["format"], "elf");
        assert_eq!(out["machine"], "x86_64");
        assert_eq!(out["bits"], 64);
        assert_eq!(out["endian"], "little");
        assert_eq!(
            out["sections_total"], 2,
            "NULL 节 + .shstrtab：{}",
            out["sections"]
        );
        assert_eq!(out["sections"][1]["name"], ".shstrtab");
    }

    /// Web / MCP 端不带 `max_bytes` 时传进来的是 0：那必须读全文件。
    /// 只读一千字节会把节头表腰斩，报出来的错还像是文件的锅。
    #[test]
    fn a_zero_cap_reads_the_whole_file() {
        // 把节头表推到 1024 之后：只读一千字节就正好看不见它
        let mut bytes = tiny_elf();
        bytes.resize(1728, 0);
        bytes[40..48].copy_from_slice(&1600u64.to_le_bytes()); // e_shoff
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.out");
        std::fs::write(&path, &bytes).expect("写 ELF");
        let (tx, _rx) = mpsc::channel();
        let app = Symbols {
            path: path.clone(),
            limit: 64,
            max_bytes: 0,
        };
        let out = run_symbols(&app, &Context::new_test(tx))
            .expect("0 上限要当成「不设上限」，把整个文件读进来");
        assert_eq!(out["sections_total"], 2, "{out}");
        // 反过来：真截断的时候要交代「只读了多少」，不能只怪文件
        let (tx, _rx) = mpsc::channel();
        let app = Symbols {
            path,
            limit: 64,
            max_bytes: 1024,
        };
        let err = run_symbols(&app, &Context::new_test(tx)).expect_err("截断后应报错");
        let text = err.to_string();
        assert!(
            text.contains("只读了 1024") && text.contains("--max-bytes"),
            "错误要说清是读少了：{text}"
        );
    }

    /// 不是目标文件的输入要报错并说清原因，不能交出一份空的「符号表」
    #[test]
    fn a_text_file_is_not_reported_as_an_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, b"prose, not a binary").expect("写文件");
        let error = run(path).expect_err("不该被当成目标文件");
        assert!(error.to_string().contains("目标文件"), "{error}");
    }

    #[test]
    fn a_tab_in_a_name_cannot_invent_a_column() {
        assert_eq!(clean("a\tb\nc"), "a b c");
    }

    /// 省略 `limit` 的端传进来的是 0：不能只列一条，要和 CLI 的缺省给同一张表
    #[test]
    fn a_zero_limit_lists_what_the_cli_default_lists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("c.out");
        std::fs::write(&path, tiny_elf()).expect("写 ELF");
        let mut rows = Vec::new();
        for limit in [0u64, 128] {
            let (tx, _rx) = mpsc::channel();
            let app = Symbols {
                path: path.clone(),
                limit,
                max_bytes: 0,
            };
            rows.push(run_symbols(&app, &Context::new_test(tx)).expect("symbols 应成功"));
        }
        assert_eq!(rows[0]["limit"], 128, "0 要当成缺省：{}", rows[0]["limit"]);
        assert_eq!(
            rows[0]["sections_total"], rows[1]["sections_total"],
            "两边表长不一样：{} vs {}",
            rows[0]["sections"], rows[1]["sections"]
        );
        assert_eq!(
            rows[0]["sections"],
            rows[1]["sections"],
            "{out}",
            out = rows[0]
        );
        assert_eq!(
            rows[0]["symbols"],
            rows[1]["symbols"],
            "{out}",
            out = rows[0]
        );
        assert_eq!(rows[0]["cut"], rows[1]["cut"], "{out}", out = rows[0]);
    }
}
