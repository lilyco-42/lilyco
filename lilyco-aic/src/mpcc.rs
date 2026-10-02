//! `laic mpcc` — MPCC（MobPCCContainer 立绘换装容器）头部解析（T0 只读）
//!
//! 反编译规格（MobPCCContainer.readFromBytesFromFile @ mobpxl/MobPCCContainer.cs:401）：
//! ```text
//! byte        （占位 0x00）
//! string      name      （ByteArray.readString = u16 长度 + utf-8，与 pxls 同族）
//! string      chr_name
//! byte        SkltPalette 存在标志（0 = 无后续调色板；>0 交给 SkltPalette.readFromBytes）
//! ...         深层调色板（ACC parts 换装表 —— 二期工程，本命令只报剩余字节数）
//! ```
//!
//! MPCC 是 mobpcc/*.mpcc.bytes 的换装/配色容器，管立绘自定义（darknoel 等）。

use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

/// 解析 MPCC 头部
#[derive(App)]
#[app(
    name = "mpcc",
    run = "run_mpcc",
    about = "Parse the header of `*.mpcc.bytes` MobPCCContainer files (mobpcc/ skin & recolor containers) under `root` (file or directory, scanned recursively): reports container `name`, character `chr_name`, palette presence flag and remaining payload size. Deep ACC palette parsing (per-parts recolor table) is planned — this command is the inventory step. Read-only (safety T0)."
)]
pub struct Mpcc {
    /// 搜索根目录（或单文件）
    #[arg(about = "A .mpcc.bytes file, or a directory scanned recursively for MPCC containers", must_exist = true)]
    root: PathBuf,

    /// 容器名 glob 过滤
    #[arg(about = "Container name glob filter, e.g. 'NOEL*'", default = "")]
    name: String,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of files processed (0 = unlimited)", default = 0)]
    limit: u64,
}

fn run_mpcc(app: &Mpcc, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let targets = collect_mpcc_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };
    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("scanning {} MPCC files", targets.len())),
    });

    let mut parsed: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for (i, path) in targets.iter().enumerate() {
        if i % 8 == 0 {
            ctx.tick(i as u64, Some(targets.len() as u64), "");
        }
        let file_str = path.display().to_string();
        let d = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("read: {e}") }));
                continue;
            }
        };
        match parse_header(&d) {
            Ok(h) => {
                if !app.name.is_empty() && !crate::util::glob_match(&app.name, &h.name) {
                    continue;
                }
                parsed.push(serde_json::json!({
                    "file": file_str,
                    "name": h.name,
                    "chr_name": h.chr_name,
                    "palette_flag": h.palette_flag,
                    "palette_payload_bytes": h.rest_len,
                    "file_bytes": d.len(),
                }));
            }
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
            }
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "files_found": targets.len(),
        "count": parsed.len(),
        "containers": parsed,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

struct Header {
    name: String,
    chr_name: String,
    palette_flag: u8,
    rest_len: usize,
}

/// 头部逐字节解析（ByteArray = pxls 同族：大端 + u16 前缀字符串）
fn parse_header(d: &[u8]) -> Result<Header, String> {
    let mut p = 0usize;
    let need = |p: usize, n: usize| -> Result<(), String> {
        if p + n > d.len() {
            Err(format!("truncated at {p} (need {n} of {})", d.len()))
        } else {
            Ok(())
        }
    };
    need(p, 1)?;
    let _lead = d[p]; // 占位字节
    p += 1;
    let read_str = |p: &mut usize| -> Result<String, String> {
        need(*p, 2)?;
        let n = u16::from_be_bytes([d[*p], d[*p + 1]]) as usize;
        *p += 2;
        need(*p, n)?;
        let s = String::from_utf8_lossy(&d[*p..*p + n]).into_owned();
        *p += n;
        Ok(s)
    };
    let name = read_str(&mut p)?;
    let chr_name = read_str(&mut p)?;
    need(p, 1)?;
    let palette_flag = d[p];
    p += 1;
    Ok(Header {
        name,
        chr_name,
        palette_flag,
        rest_len: d.len() - p,
    })
}

fn collect_mpcc_targets(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        return Err(format!("not a file or directory: {}", root.display()));
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 12 {
        return Ok(());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(name.as_str(), ".git" | "target" | "node_modules" | "__pycache__") {
                continue;
            }
            walk(&p, depth + 1, out)?;
        } else {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if name.ends_with(".mpcc.bytes") || name.ends_with(".mpcc") {
                out.push(p);
            }
        }
    }
    Ok(())
}
