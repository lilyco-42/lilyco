//! `laic mpcc` — MPCC（MobPCCContainer 立绘换装容器）解析（T0 只读）
//!
//! 对标 `MobPCCContainer.readFromBytesFromFile`（`XX.mobpxl`，unsafeAssem.dll）——
//! 这是**立绘换装 / 改色**的数据面：每个部件 key 挂一串「调色操作」，
//! 游戏在 GPU 上按它把基础贴图算成换装后的立绘。
//!
//! ```text
//! byte         占位（游戏读后丢弃，写回恒 0）
//! string       name        （u16 长度 + utf-8，与 pxls 同族）
//! string       chr_name
//! byte         占位（游戏读后丢弃，写回恒 0）
//! byte         ARMX —— 部件条目数；0 表示整份调色板为空
//! ARMX × {
//!     pstring  部件 key   （u8 长度 + utf-8）
//!     byte     ACC 操作数；<= 0 表示这条丢弃
//!     ACC × {
//!         byte    操作类型：0=NONE(丢弃) 1=HSV 2=TONECURVE
//!         HSV      → i16 h, u8 s, u8 v, u8 flags
//!                    其中 s/v 读入后 **≥127 再减 256**（游戏自己的怪癖，照抄）
//!         TONECURVE→ byte 通道数 N，N × ( byte 点数 M，M × (u8 x, u8 y) )
//!                    （x/y 是 0..255 的定点，游戏除以 255；写回恒写 4 通道）
//!     }
//! }
//! ```
//!
//! 🔴 **两处「读了就丢」必须照抄**：`type == 0` 的操作和 `ACC 操作数 == 0` 的部件
//! 在游戏里都会被丢掉，字节却照样吃掉。解析时不照抄，后面所有条目都会错位。
//!
//! 🔴 **顺序不能换成 BTreeMap**：游戏写回走 `X.objKeys`（字典的插入序），
//! 用有序表重排会让字节回环对不上。部件表存 `Vec` 保序。
//!
//! HSV 的语义看 `MobPCCHsv.executeToImage`：着色器拿 `h/60`、`s/100`、`v/100`，
//! 也就是 h 是角度、s/v 是百分比；`flags` 的 bit0 会让 bit1 失效（`flags & 1 → flags &= ~2`）。

use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

/// 解析 MPCC 容器（立绘换装 / 改色表）
#[derive(App)]
#[app(
    name = "mpcc",
    run = "run_mpcc",
    about = "Parse `*.mpcc.bytes` MobPCCContainer files (mobpcc/ portrait skin & recolor containers) under `root` (file or directory, scanned recursively). This is the data behind Alice in Cradle's outfit/colour customisation: the container names a character and holds a per-PARTS list of colour operations that the game applies on the GPU. Reported per container: `name`, `chr_name`, and every parts entry with its ops — `hsv` (h raw + degrees, s/v raw + percent, flags) and `tone_curve` (channel count and per-channel point counts; pass `full` to also emit the 0..255 key-point lists). Every file is re-serialised and compared byte-for-byte against the input (`roundtrip_ok`), so `true` proves the parse is complete — including the two places where the game reads-and-discards bytes (op type 0, and parts entries whose op count is 0); `dropped` reports how many were discarded. Read-only (safety T0)."
)]
pub struct Mpcc {
    /// 搜索根目录（或单文件）
    #[arg(
        about = "A .mpcc.bytes file, or a directory scanned recursively for MPCC containers",
        must_exist = true
    )]
    root: PathBuf,

    /// 容器名 glob 过滤
    #[arg(about = "Container name glob filter, e.g. 'NOEL*'", default = "")]
    name: String,

    /// 附带 tone curve 的采样点列表（默认只报通道数与点数）
    #[arg(
        about = "Also emit the tone-curve key-point lists (each channel can hold up to 255 points)",
        default = false
    )]
    full: bool,

    /// 最多处理的文件数（0 = 不限）
    #[arg(
        about = "Cap the number of files processed (0 = unlimited)",
        default = 0
    )]
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
        match parse(&d) {
            Ok(m) => {
                if !app.name.is_empty() && !crate::util::glob_match(&app.name, &m.name) {
                    continue;
                }
                parsed.push(m.to_json(&file_str, &d, app.full));
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

// ─────────────────────────────────────────────────────────────
// 模型
// ─────────────────────────────────────────────────────────────

/// 一次调色操作。`NONE`（类型 0）在游戏里是「读了就丢」，不进模型。
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// 色相 / 饱和度 / 明度偏移。`h` 是角度（着色器再除以 60），`s`/`v` 是百分比（除以 100）。
    Hsv { h: i16, s: i32, v: i32, flags: u8 },
    /// 色调曲线：每通道一串 0..255 的关键点（定点，游戏除以 255）。
    ToneCurve { channels: Vec<Vec<(u8, u8)>> },
}

/// 一个部件：key + 它的操作链（顺序即应用顺序）
#[derive(Clone, Debug, PartialEq)]
pub struct PartsEntry {
    pub key: String,
    pub ops: Vec<Op>,
}

/// 一份 MPCC 容器（CLI 参数结构叫 `Mpcc`，模型这边让位给它）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Container {
    pub name: String,
    pub chr_name: String,
    /// 首个占位字节（游戏读后丢弃、写回恒 0）—— 留档以便字节回环比对
    pub reserved: u8,
    /// 保序：游戏写回走字典插入序，换成有序表会破坏字节回环
    pub parts: Vec<PartsEntry>,
    /// 解析时被游戏语义丢掉的条目数；非 0 时字节回环不可能成立
    pub dropped_ops: usize,
    pub dropped_parts: usize,
}

impl Container {
    /// 序列化回字节（照抄 `MobPCCContainer.writeToBytes` + `SkltPalette.writeToBytes`）
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.push(0);
        write_string(&mut out, &self.name);
        write_string(&mut out, &self.chr_name);
        out.push(0);
        out.push(self.parts.len().min(255) as u8);
        for p in &self.parts {
            write_pstring(&mut out, &p.key);
            out.push(p.ops.len().min(255) as u8);
            for op in &p.ops {
                match op {
                    Op::Hsv { h, s, v, flags } => {
                        out.push(1);
                        out.extend_from_slice(&h.to_be_bytes());
                        out.push((*s & 0xff) as u8);
                        out.push((*v & 0xff) as u8);
                        out.push(*flags);
                    }
                    Op::ToneCurve { channels } => {
                        out.push(2);
                        out.push(channels.len().min(255) as u8);
                        for ch in channels {
                            out.push(ch.len().min(255) as u8);
                            for &(x, y) in ch.iter().take(255) {
                                out.push(x);
                                out.push(y);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn to_json(&self, file: &str, orig: &[u8], full: bool) -> serde_json::Value {
        let mut hsv = 0usize;
        let mut tc = 0usize;
        for p in &self.parts {
            for op in &p.ops {
                match op {
                    Op::Hsv { .. } => hsv += 1,
                    Op::ToneCurve { .. } => tc += 1,
                }
            }
        }
        let parts: Vec<serde_json::Value> = self
            .parts
            .iter()
            .map(|p| {
                let ops: Vec<serde_json::Value> = p
                    .ops
                    .iter()
                    .map(|op| match op {
                        Op::Hsv { h, s, v, flags } => serde_json::json!({
                            "type": "hsv",
                            "h": h,
                            "s": s,
                            "v": v,
                            "flags": flags,
                            // 着色器实际取的量：h/60、s/100、v/100
                            "h_deg": (*h as f64) / 60.0,
                            "s_pct": (*s as f64) / 100.0,
                            "v_pct": (*v as f64) / 100.0,
                            "hidden": flags & 1 != 0,
                        }),
                        Op::ToneCurve { channels } => {
                            let mut v = serde_json::json!({
                                "type": "tone_curve",
                                "channels": channels.len(),
                                "points": channels.iter().map(Vec::len).collect::<Vec<_>>(),
                            });
                            if full {
                                v["points_xy"] = serde_json::json!(channels);
                            }
                            v
                        }
                    })
                    .collect();
                serde_json::json!({ "key": p.key, "ops": ops })
            })
            .collect();

        // 字节回环：解析 → 序列化，必须与原文件**逐字节**相同。
        // 只比长度是不够的 —— 长度相同但内容错位（顺序调换、u8/i8 符号翻转）会静默通过。
        let back = self.serialize();
        let roundtrip_ok = back == orig;

        serde_json::json!({
            "file": file,
            "name": self.name,
            "chr_name": self.chr_name,
            "parts_count": self.parts.len(),
            "ops": { "hsv": hsv, "tone_curve": tc },
            "parts": parts,
            "dropped": { "ops": self.dropped_ops, "parts": self.dropped_parts },
            "roundtrip_ok": roundtrip_ok,
            "file_bytes": orig.len(),
        })
    }
}

// ─────────────────────────────────────────────────────────────
// 解析
// ─────────────────────────────────────────────────────────────

struct Be<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Be<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.p + n > self.d.len() {
            return Err(format!(
                "truncated at {} (need {} of {})",
                self.p,
                n,
                self.d.len()
            ));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }
    /// u16 长度 + utf-8（`ByteArray.readString`）
    fn string(&mut self) -> Result<String, String> {
        let b = self.take(2)?;
        let n = u16::from_be_bytes([b[0], b[1]]) as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    /// u8 长度 + utf-8（`ByteArray.readPascalString`）
    fn pstring(&mut self) -> Result<String, String> {
        let n = self.u8()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    /// 照抄 `MobPCCHsv.readFromBytesInner` 的怪癖：≥127 再减 256
    fn signedish(&mut self) -> Result<i32, String> {
        let b = self.u8()? as i32;
        Ok(if b >= 127 { b - 256 } else { b })
    }
}

/// 整份 MPCC（含两处「读了就丢」的字节消耗）
pub fn parse(d: &[u8]) -> Result<Container, String> {
    let mut r = Be { d, p: 0 };
    let reserved = r.u8()?;
    let name = r.string()?;
    let chr_name = r.string()?;
    r.u8()?; // 第二个占位字节：游戏读后丢弃，写回恒 0
    let armp = r.u8()? as usize;
    let mut parts = Vec::new();
    let mut dropped_parts = 0usize;
    let mut dropped_ops = 0usize;
    for _ in 0..armp {
        let key = r.pstring()?;
        let n = r.u8()? as i32;
        if n <= 0 {
            // `ACC.readFromBytesACC` 返回 null，SkltPalette 直接 continue —— 字节已吃掉
            dropped_parts += 1;
            continue;
        }
        let mut ops = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let t = r.u8()?;
            match t {
                1 => {
                    let h = r.i16()?;
                    let s = r.signedish()?;
                    let v = r.signedish()?;
                    let flags = r.u8()?;
                    ops.push(Op::Hsv { h, s, v, flags });
                }
                2 => {
                    let channels_n = r.u8()? as usize;
                    let mut channels = Vec::with_capacity(channels_n);
                    for _ in 0..channels_n {
                        let points = r.u8()? as usize;
                        let mut ch = Vec::with_capacity(points);
                        for _ in 0..points {
                            let x = r.u8()?;
                            let y = r.u8()?;
                            ch.push((x, y));
                        }
                        channels.push(ch);
                    }
                    ops.push(Op::ToneCurve { channels });
                }
                _ => {
                    // 类型 0（NONE）在 `MobPCC.readFromBytes` 里返回 null 被丢弃
                    dropped_ops += 1;
                }
            }
        }
        parts.push(PartsEntry { key, ops });
    }
    Ok(Container {
        name,
        chr_name,
        reserved,
        parts,
        dropped_ops,
        dropped_parts,
    })
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&b[..n]);
}

fn write_pstring(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(255);
    out.push(n as u8);
    out.extend_from_slice(&b[..n]);
}

// ─────────────────────────────────────────────────────────────
// 文件收集
// ─────────────────────────────────────────────────────────────

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
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if matches!(
                name.as_str(),
                ".git" | "target" | "node_modules" | "__pycache__"
            ) {
                continue;
            }
            walk(&p, depth + 1, out)?;
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name.ends_with(".mpcc.bytes") || name.ends_with(".mpcc") {
                out.push(p);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机 `noel__231022_013830.mpcc.bytes`（50 字节）的结构骨架
    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        d.push(0);
        write_string(&mut d, "231022_013830");
        write_string(&mut d, "NOEL");
        d.push(0);
        d.push(2);
        // 部件「明白」：一次 HSV
        write_pstring(&mut d, "明白");
        d.push(1);
        d.push(1);
        d.extend_from_slice(&(-82i16).to_be_bytes());
        d.push(48);
        d.push(236);
        d.push(5);
        // 部件 hair：一次 HSV
        write_pstring(&mut d, "hair");
        d.push(1);
        d.push(1);
        d.extend_from_slice(&(-93i16).to_be_bytes());
        d.push(0);
        d.push(204);
        d.push(4);
        d
    }

    #[test]
    fn parses_the_real_layout_and_round_trips_byte_for_byte() {
        let d = sample();
        let m = parse(&d).expect("parse");
        assert_eq!(m.name, "231022_013830");
        assert_eq!(m.chr_name, "NOEL");
        assert_eq!(m.reserved, 0);
        assert_eq!(m.parts.len(), 2);
        assert_eq!(m.parts[0].key, "明白");
        assert_eq!(m.parts[1].key, "hair");
        // s/v ≥127 要减 256（游戏怪癖）
        assert_eq!(
            m.parts[0].ops[0],
            Op::Hsv {
                h: -82,
                s: 48,
                v: -20,
                flags: 5
            }
        );
        assert_eq!(
            m.parts[1].ops[0],
            Op::Hsv {
                h: -93,
                s: 0,
                v: -52,
                flags: 4
            }
        );
        assert_eq!(m.serialize(), d, "解析 → 序列化必须逐字节相同");
    }

    /// 类型 0 的操作、以及操作数为 0 的部件，字节照样吃掉但条目丢弃
    #[test]
    fn dropped_entries_still_consume_their_bytes() {
        let mut d = Vec::new();
        d.push(0);
        write_string(&mut d, "n");
        write_string(&mut d, "C");
        d.push(0);
        d.push(2);
        write_pstring(&mut d, "a"); // 操作数 0：丢掉
        d.push(0);
        write_pstring(&mut d, "b");
        d.push(2);
        d.push(0); // 类型 0：丢掉，但只吃掉这 1 字节
        d.push(1); // 正常 HSV
        d.extend_from_slice(&(10i16).to_be_bytes());
        d.push(1);
        d.push(2);
        d.push(3);

        let m = parse(&d).expect("parse");
        assert_eq!(m.parts.len(), 1);
        assert_eq!(m.parts[0].key, "b");
        assert_eq!(m.parts[0].ops.len(), 1);
        assert_eq!(m.dropped_parts, 1);
        assert_eq!(m.dropped_ops, 1);
        // 丢弃项无法重建 → 长度必然短于原文件（调用方据此报 roundtrip_ok=false）
        assert!(m.serialize().len() < d.len());
    }

    #[test]
    fn tone_curve_keeps_channel_and_point_counts() {
        let mut d = Vec::new();
        d.push(0);
        write_string(&mut d, "tc");
        write_string(&mut d, "C");
        d.push(0);
        d.push(1);
        write_pstring(&mut d, "skin");
        d.push(1);
        d.push(2); // TONECURVE
        d.push(4); // 4 通道
        for m in [4usize, 3, 3, 3] {
            d.push(m as u8);
            for j in 0..m {
                d.push(j as u8);
                d.push((j * 2) as u8);
            }
        }
        let m = parse(&d).expect("parse");
        match &m.parts[0].ops[0] {
            Op::ToneCurve { channels } => {
                assert_eq!(channels.len(), 4);
                assert_eq!(
                    channels.iter().map(Vec::len).collect::<Vec<_>>(),
                    vec![4, 3, 3, 3]
                );
                assert_eq!(channels[0][3], (3, 6));
            }
            other => panic!("expected tone curve, got {other:?}"),
        }
        assert_eq!(m.serialize(), d);
    }

    #[test]
    fn an_empty_palette_is_valid() {
        let mut d = Vec::new();
        d.push(0);
        write_string(&mut d, "231022_013830");
        write_string(&mut d, "NOEL");
        d.push(0);
        d.push(0); // 0 个部件 —— 真机 NOEL__231022_013830_2 就是这个
        let m = parse(&d).expect("parse");
        assert!(m.parts.is_empty());
        assert_eq!(m.serialize(), d);
    }

    #[test]
    fn truncation_is_reported_not_guessed() {
        let d = sample();
        assert!(parse(&d[..d.len() - 1]).is_err());
    }

    /// `roundtrip_ok` 必须是**逐字节**比对，不是比长度。
    /// 长度相同但内容错位（这里是部件顺序调换）必须被抓出来 —— 只比长度会静默通过。
    #[test]
    fn roundtrip_flag_catches_a_same_length_but_wrong_body() {
        let m = parse(&sample()).expect("parse");
        let orig = sample();

        let j = m.to_json("x", &orig, false);
        assert_eq!(
            j["roundtrip_ok"],
            serde_json::json!(true),
            "原样回环必须通过"
        );

        // 同长度、内容不同：把第一个 key 的字节反转（"明白" 的 3 字节 UTF-8 反转后
        // 仍是 3 字节，且不可能还是合法同样的串）—— 长度天然不变，不靠手算偏移，
        // 测试也不会因为前面多一个/少一个字节而脆掉。
        let ka = orig
            .windows("明白".len())
            .position(|w| w == "明白".as_bytes())
            .expect("第一个 key");
        let mut swapped = orig.clone();
        swapped[ka..ka + "明白".len()].reverse();
        assert_eq!(
            swapped.len(),
            orig.len(),
            "前提：长度必须相同，否则这个测试没意义"
        );
        assert_ne!(swapped, orig, "前提：内容必须真的变了");

        let j2 = m.to_json("x", &swapped, false);
        assert_eq!(
            j2["roundtrip_ok"],
            serde_json::json!(false),
            "同长度不同内容必须报 false"
        );
    }

    #[test]
    fn hsv_signed_quirk_matches_the_game() {
        // 127 → -129、255 → -1、126 → 126（照抄 if (v >= 127) v -= 256）
        for (raw, want) in [(0u8, 0i32), (126, 126), (127, -129), (200, -56), (255, -1)] {
            let mut d = Vec::new();
            d.push(0);
            write_string(&mut d, "n");
            write_string(&mut d, "C");
            d.push(0);
            d.push(1);
            write_pstring(&mut d, "k");
            d.push(1);
            d.push(1);
            d.extend_from_slice(&(0i16).to_be_bytes());
            d.push(raw);
            d.push(0);
            d.push(0);
            let m = parse(&d).expect("parse");
            match m.parts[0].ops[0] {
                Op::Hsv { s, .. } => assert_eq!(s, want, "raw {raw}"),
                ref o => panic!("{o:?}"),
            }
        }
    }
}
