//! `laic repack` — pxls 写回（**T1 需确认**）
//!
//! 本域唯一的写操作：把改过的 pxls 模型重新序列化，再**打回原来的容器**。
//! 这是「超集」的另一半 —— pixelliner4j 能存盘，我们也必须能，而且要能存回
//! 游戏真正啃得动的那层（UnityFS + v22 SerializedFile 里的 TextAsset）。
//!
//! ## 两条容器路径
//!
//! - **裸 pxls**（`.pxls` / 未打包的 `.pxls.dat`）：`parse` → 改 → `serialize` → 直接落盘。
//! - **UnityFS**（本作 128 个 `*.pxls.dat` 都是）：解包 → 定位 TextAsset →
//!   就地把新正文写回去，并修 4 处长度/偏移：
//!
//!   | 位置 | 字段 | 改法 |
//!   |---|---|---|
//!   | `sig_off-4` | TypelessData 的 `u32 len`（小端） | = 新正文长度 |
//!   | SerializedFile `@24` | 头 `file_size u64`（**大端**） | `+= object_delta` |
//!   | 对象表 | 目标对象 `byte_size u32`（小端） | **重算** `align4(rel_sig + new_len)`，不是 `+=` |
//!   | 对象表 | 其后对象的 `byte_start u64`（小端） | `+= object_delta` |
//!
//!   🔴 目标对象的 `byte_size` **必须重算对齐**：`byte_size = align4(长度字段 + 正文)`
//!   （`sig_off - byte_start` 已经包含 `m_Name` 前缀和 `u32` 长度字段）。若偷懒写
//!   `+= 正文差`，正文加 10 字节时填充会少长 2 字节，UnityPy 立刻报
//!   `Expected to read N, but only read N+2`。所以对象区整段重建、填充按需重铺。
//!
//!   然后 [`crate::unityfs::repack`] 重压数据块 + 重建 blocks info / 节点表。
//!
//! ## 自检
//!
//! 每个写出的文件都会被重新读一遍：`extract_pxls` → `parse` → `serialize`，结果必须与
//! 刚写进去的正文逐字节相同。自检不过就报错，绝不悄悄落一个坏文件。
//!
//! 默认 **dry-run**（只报计划），加 `--apply` 才写盘。MCP 自动化面默认拒绝 T1。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::pxlslib::{self, Pxls};
use crate::serialized::SerializedFile;
use crate::unityfs::{self, RepackComp};
use crate::util::{collect_targets, glob_match};

/// 把改过的 pxls 写回容器
#[derive(App)]
#[app(
    name = "repack",
    run = "run_repack",
    safety = "t1",
    about = "Write a modified PixelLiner .pxls table back into its container — the save half of a sprite/skin pipeline (read-only tools can only look, this one can commit). Edits are applied to the parsed model and then re-serialized, byte-exactly, back into EITHER a bare .pxls file OR the game's UnityFS bundle (`.pxls.dat`): for the bundle path it locates the TextAsset inside the v22 SerializedFile, splices the new body in place and repairs the TypelessData length prefix, the SerializedFile header `file_size`, the object table (byte_size / trailing byte_start) and the UnityFS blocks info + node table. Edits: `rename` (OLD=NEW, repeatable or `;`-separated, renames pose titles), `set-alpha` (LAYER=0..100, sets a layer's alpha in every matching pose). `pose` is a title glob limiting which poses may be edited. `compress` picks how the rewritten bundle is packed: `none`, `lz4`, or `original` (keep the source algorithm; Unity LZMA has no encoder here and degrades to none). DRY RUN BY DEFAULT — pass `apply: true` to actually write (safety tier T1: the automated/MCP surface denies it, so a human must confirm). Every written file is re-read and re-parsed; the round-trip must be byte-identical or the command fails. Writes `<out>/<file-name>`."
)]
pub struct Repack {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 输出目录
    #[arg(about = "Directory to write the repacked files into", must_exist = false)]
    out: PathBuf,

    /// 只允许编辑标题匹配该 glob 的姿势
    #[arg(about = "Only poses whose title matches this glob may be edited", default = "*")]
    pose: String,

    /// 重命名姿势标题：OLD=NEW（可重复，或用 `;` 分隔多条）
    #[arg(about = "Rename pose titles, `OLD=NEW` (repeatable, or `;`-separated)", default = Vec::<String>::new())]
    rename: Vec<String>,

    /// 设置图层的 alpha：LAYER=0..100（可重复，或用 `;` 分隔多条）
    #[arg(about = "Set a layer's alpha, `LAYER=0..100` (repeatable, or `;`-separated)", default = Vec::<String>::new())]
    set_alpha: Vec<String>,

    /// 重打包时的压缩方式
    #[arg(about = "Compression for the rewritten bundle: none | lz4 | original", default = "original")]
    compress: String,

    /// 真正写盘（默认只预览）
    #[arg(about = "Actually write the files. Without this flag the command is a dry run")]
    apply: bool,

    /// 允许覆盖已存在的目标文件
    #[arg(about = "Allow overwriting an existing output file (default: skip it)")]
    overwrite: bool,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of tables processed (0 = unlimited)", default = 0)]
    limit: u64,
}

/// 一次编辑计划
struct Edits {
    rename: Vec<(String, String)>,
    alpha: Vec<(String, i32)>,
}

/// 写回结果
struct Patch {
    bytes: Vec<u8>,
    container: &'static str,
    /// 承载 pxls 的 TextAsset 的 path_id（裸容器为 None）
    object: Option<i64>,
    /// **对象表**的 `byte_size` 变化量（对齐到 4 之后）。UnityFS 下它 ≥ 正文变化量：
    /// 正文只加 10 字节时填充可能多出一个字，于是对象长大 12 字节。
    delta: i64,
    /// **pxls 正文**的长度变化量（真实内容差，不带填充）。裸容器下两者相同。
    body_delta: i64,
}

fn run_repack(app: &Repack, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let edits = parse_edits(app).map_err(AppError::InvalidArg)?;
    let comp = parse_compress(&app.compress).map_err(AppError::InvalidArg)?;

    let targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    let targets = if app.limit > 0 {
        targets.into_iter().take(app.limit as usize).collect()
    } else {
        targets
    };

    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!(
            "{} {} table(s)",
            if app.apply { "repacking" } else { "planning" },
            targets.len()
        )),
    });

    let mut tables: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut written = 0usize;
    let mut changed = 0usize;

    for (i, path) in targets.iter().enumerate() {
        ctx.tick(i as u64, Some(targets.len() as u64), "");
        let file_str = path.display().to_string();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("read: {e}") }));
                continue;
            }
        };
        let (_src, body) = match unityfs::extract_pxls(&bytes) {
            Ok(v) => v,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        let mut p = match pxlslib::parse(&body) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("parse: {e}") }));
                continue;
            }
        };

        let (renamed, alpha_set) = apply_edits(&mut p, &edits, &app.pose);
        let new_body = p.serialize();
        let body_changed = new_body != body;

        // 写回容器
        let patch = match write_back(&bytes, &new_body, comp) {
            Ok(v) => v,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };

        // 自检：重新读一遍，模型必须与刚写进去的一致
        match self_check(&patch.bytes, &new_body) {
            Ok(()) => {}
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("self-check: {e}") }));
                continue;
            }
        }

        if body_changed {
            changed += 1;
        }

        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let out_path = app.out.join(&name);
        let existed = out_path.exists();
        let mut entry = serde_json::json!({
            "file": file_str,
            "container": patch.container,
            "object_path_id": patch.object,
            "poses": p.poses.len(),
            "renamed": renamed,
            "alpha_set": alpha_set,
            "pxls_bytes_before": body.len(),
            "pxls_bytes_after": new_body.len(),
            "pxls_delta": patch.body_delta,
            // 对象表里的 byte_size 增量（对齐到 4），≥ pxls_delta
            "object_delta": patch.delta,
            "pxls_changed": body_changed,
            "bundle_bytes_before": bytes.len(),
            "bundle_bytes_after": patch.bytes.len(),
            "out": out_path.display().to_string(),
            "verified": true,
        });

        if !app.apply {
            entry["written"] = serde_json::json!(false);
            tables.push(entry);
            continue;
        }
        if existed && !app.overwrite {
            entry["written"] = serde_json::json!(false);
            entry["skipped"] = serde_json::json!("target exists (pass overwrite=true)");
            tables.push(entry);
            continue;
        }
        if let Some(parent) = out_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("mkdir: {e}") }));
                continue;
            }
        }
        match std::fs::write(&out_path, &patch.bytes) {
            Ok(()) => {
                written += 1;
                entry["written"] = serde_json::json!(true);
            }
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": format!("write: {e}") }));
                continue;
            }
        }
        tables.push(entry);
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "out": app.out.display().to_string(),
        "dry_run": !app.apply,
        "compress": app.compress,
        "pose_filter": app.pose,
        "tables_scanned": targets.len(),
        "tables_reported": tables.len(),
        "tables_changed": changed,
        "files_written": written,
        "tables": tables,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

// ─────────────────────────────────────────────────────────────
// 参数解析
// ─────────────────────────────────────────────────────────────

/// `OLD=NEW` / `LAYER=ALPHA`；一条里也可以用 `;` 再分组
fn parse_pairs(raw: &[String], what: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for item in raw {
        for part in item.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| format!("--{what} expects `key=value`, got `{part}`"))?;
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(out)
}

fn parse_edits(app: &Repack) -> Result<Edits, String> {
    let rename = parse_pairs(&app.rename, "rename")?;
    let mut alpha = Vec::new();
    for (layer, v) in parse_pairs(&app.set_alpha, "set-alpha")? {
        let a: i32 = v
            .parse()
            .map_err(|_| format!("--set-alpha `{layer}`: `{v}` is not an integer 0..=100"))?;
        if !(0..=100).contains(&a) {
            return Err(format!("--set-alpha `{layer}`: {a} out of range 0..=100"));
        }
        alpha.push((layer, a));
    }
    Ok(Edits { rename, alpha })
}

fn parse_compress(s: &str) -> Result<RepackComp, String> {
    match s.trim().to_lowercase().as_str() {
        "" | "original" => Ok(RepackComp::Original),
        "none" | "off" => Ok(RepackComp::None),
        "lz4" => Ok(RepackComp::Lz4),
        other => Err(format!("--compress `{other}`: use none | lz4 | original")),
    }
}

// ─────────────────────────────────────────────────────────────
// 编辑
// ─────────────────────────────────────────────────────────────

/// 把一个编辑计划应用到模型上；返回 (改名数, 改 alpha 的图层数)
fn apply_edits(p: &mut Pxls, e: &Edits, pose_glob: &str) -> (usize, usize) {
    let mut renamed = 0usize;
    let mut alpha_set = 0usize;
    for pose in &mut p.poses {
        if !glob_match(pose_glob, &pose.title) {
            continue;
        }
        for (old, new) in &e.rename {
            if &pose.title == old {
                pose.title = new.clone();
                renamed += 1;
            }
        }
        for seq in &mut pose.seqs {
            for frame in &mut seq.frames {
                for lay in &mut frame.layers {
                    for (name, a) in &e.alpha {
                        if &lay.name == name {
                            lay.set_alpha(*a);
                            alpha_set += 1;
                        }
                    }
                }
            }
        }
    }
    (renamed, alpha_set)
}

// ─────────────────────────────────────────────────────────────
// 写回
// ─────────────────────────────────────────────────────────────

/// 把新正文打回原容器。`new_body` 是 `Pxls::serialize()` 的产物（自封闭 pxls 字节）。
fn write_back(orig: &[u8], new_body: &[u8], comp: RepackComp) -> Result<Patch, String> {
    // 裸 pxls：正文即整份文件
    if orig.len() >= 8 && orig[..8] == pxlslib::SIGNATURE[..] {
        return Ok(Patch {
            bytes: new_body.to_vec(),
            container: "raw",
            object: None,
            delta: new_body.len() as i64 - orig.len() as i64,
            body_delta: new_body.len() as i64 - orig.len() as i64,
        });
    }
    if !orig.starts_with(b"UnityFS\0") {
        return Err("not a pxls file nor a UnityFS bundle".into());
    }

    let (data, info) = unityfs::inflate_with_info(orig)?;
    let sig_off = unityfs::find_pxls_offset(&data)
        .ok_or_else(|| "pxls signature not found in the inflated bundle".to_string())?;
    if sig_off < 4 {
        return Err("pxls body has no u32 length prefix".into());
    }
    let old_len = u32::from_le_bytes(data[sig_off - 4..sig_off].try_into().unwrap()) as usize;

    let sf = SerializedFile::parse(&data).map_err(|e| format!("SerializedFile: {e}"))?;
    let tgt = sf
        .objects
        .iter()
        .position(|o| sig_off >= o.byte_start && sig_off < o.byte_start + o.byte_size)
        .ok_or_else(|| format!("no SerializedFile object covers the pxls body at {sig_off}"))?;
    let (obj_start, obj_size, obj_pid) = {
        let o = &sf.objects[tgt];
        (o.byte_start, o.byte_size, o.path_id)
    };
    if sig_off + old_len > obj_start + obj_size {
        return Err(format!(
            "pxls length prefix {old_len} overruns its object ({obj_size} bytes)"
        ));
    }
    let sf_node = info
        .nodes
        .iter()
        .position(|n| sig_off >= n.offset && sig_off < n.offset + n.size)
        .ok_or_else(|| format!("no bundle node covers the pxls body at {sig_off}"))?;

    // 对象布局：[m_Name 等前缀][u32 len][正文][对齐填充 → 4]
    // ⚠️ 对象大小是**对齐后**的长度，不是 `旧大小 + 正文差`：正文改 10 字节时
    // 填充可能从 1 变 3，UnityPy 会立刻报 "Expected to read N, but only read N+2"。
    let rel_sig = sig_off - obj_start;
    let new_obj_size = align4(rel_sig + new_body.len());
    let old_pad = &data[sig_off + old_len..obj_start + obj_size];
    let new_pad_len = new_obj_size - rel_sig - new_body.len();
    // 填充长度没变就沿用原字节（保证「不改内容 ⇒ 逐字节不变」）
    let pad: Vec<u8> = if new_pad_len == old_pad.len() {
        old_pad.to_vec()
    } else {
        vec![0u8; new_pad_len]
    };
    let delta = new_obj_size as i64 - obj_size as i64;
    let body_delta = new_body.len() as i64 - old_len as i64;

    // 1) 整个对象区域重建（`data[obj_start..sig_off]` 里**已经含**旧的 u32 长度字段，
    //    所以前缀只取到长度字段之前，长度字段按新值重写 —— 早期版本在这里多写了一次）
    let mut rebuilt = Vec::with_capacity(new_obj_size);
    rebuilt.extend_from_slice(&data[obj_start..sig_off - 4]);
    rebuilt.extend_from_slice(&(new_body.len() as u32).to_le_bytes());
    rebuilt.extend_from_slice(new_body);
    rebuilt.extend_from_slice(&pad);
    debug_assert_eq!(rebuilt.len(), new_obj_size);

    let mut nd = data.clone();
    nd.splice(obj_start..obj_start + obj_size, rebuilt);

    // 2) SerializedFile 头 file_size（大端 u64）
    let fs_pos = sf.file_size_at;
    let old_fs = u64::from_be_bytes(
        data.get(fs_pos..fs_pos + 8)
            .ok_or("SerializedFile header truncated")?
            .try_into()
            .unwrap(),
    );
    let new_fs = (old_fs as i64 + delta).max(0) as u64;
    nd[fs_pos..fs_pos + 8].copy_from_slice(&new_fs.to_be_bytes());

    // 3) 对象表：目标 size = 对齐后的新大小；其后的对象 start += delta
    let tgt_start = sf.objects[tgt].byte_start;
    for (i, o) in sf.objects.iter().enumerate() {
        let (bs_at, bz_at) = sf.obj_table_at[i];
        if i == tgt {
            nd[bz_at..bz_at + 4].copy_from_slice(&(new_obj_size as u32).to_le_bytes());
        } else if o.byte_start > tgt_start {
            let nb = (o.byte_start as i64 + delta) - sf.data_offset as i64;
            nd[bs_at..bs_at + 8].copy_from_slice(&nb.to_le_bytes());
        }
    }

    // 4) 按新布局切出各节点
    let sf_node_off = info.nodes[sf_node].offset;
    let mut nodes_new: Vec<Vec<u8>> = Vec::with_capacity(info.nodes.len());
    for (i, n) in info.nodes.iter().enumerate() {
        let (no, ns) = if i == sf_node {
            (n.offset, (n.size as i64 + delta).max(0) as usize)
        } else if n.offset > sf_node_off {
            (((n.offset as i64) + delta) as usize, n.size)
        } else {
            (n.offset, n.size)
        };
        nodes_new.push(
            nd.get(no..no + ns)
                .ok_or_else(|| {
                    format!("node {i} range {no}+{ns} out of new data ({})", nd.len())
                })?
                .to_vec(),
        );
    }

    let bytes = unityfs::repack(orig, &nodes_new, comp)?;
    Ok(Patch { bytes, container: "UnityFS", object: Some(obj_pid), delta, body_delta })
}

/// 向上取整到 4 字节（SerializedFile 对象尾部与对象起点都是 4 字节对齐）
fn align4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

/// 自检：重新解包 + 解析 + 序列化，必须与刚写进去的正文逐字节相同
fn self_check(out: &[u8], expected: &[u8]) -> Result<(), String> {
    let (_src, body) = unityfs::extract_pxls(out)?;
    let p = pxlslib::parse(&body).map_err(|e| format!("re-parse: {e}"))?;
    let back = p.serialize();
    if back != expected {
        return Err(format!(
            "re-serialized {} bytes != written {} bytes",
            back.len(),
            expected.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const STREAM: &str =
        "D:/gal/aic-winlator/game-clean/AliceInCradle/AliceInCradle_Data/StreamingAssets";

    fn noel() -> PathBuf {
        Path::new(STREAM).join("PxlNoel/noel.pxls.dat")
    }

    fn edits(rename: &[(&str, &str)], alpha: &[(&str, i32)]) -> Edits {
        Edits {
            rename: rename.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            alpha: alpha.iter().map(|(a, b)| (a.to_string(), *b)).collect(),
        }
    }

    /// 不改任何东西也要能原样写回：正文逐字节相同，且新容器仍可解析
    #[test]
    fn identity_repack_keeps_the_pxls_body_byte_identical() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let patch = write_back(&orig, &body, RepackComp::Original).unwrap();
        assert_eq!(patch.container, "UnityFS");
        assert_eq!(patch.delta, 0);
        self_check(&patch.bytes, &body).unwrap();
        // 未编辑时正文不变 ⇒ 与原文同字节
        let (_src2, body2) = unityfs::extract_pxls(&patch.bytes).unwrap();
        assert_eq!(body2, body);
        // 新容器仍是合法 UnityFS，且长度没炸
        assert!(patch.bytes.starts_with(b"UnityFS\0"));
    }

    /// 改名会改变正文长度：SerializedFile 头 / 对象表 / 节点表必须一起跟着走
    #[test]
    fn rename_resizes_the_whole_chain_and_stays_parsable() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let mut p = pxlslib::parse(&body).unwrap();
        let target = p.poses[0].title.clone();
        let new_title = format!("{target}_RENAMED_BY_TEST");
        let e = edits(&[(target.as_str(), new_title.as_str())], &[]);
        let (renamed, _) = apply_edits(&mut p, &e, "*");
        assert_eq!(renamed, 1);
        let new_body = p.serialize();
        assert_ne!(new_body.len(), body.len());

        let patch = write_back(&orig, &new_body, RepackComp::Original).unwrap();
        assert_eq!(patch.delta, new_body.len() as i64 - body.len() as i64);
        self_check(&patch.bytes, &new_body).unwrap();

        // 重新解析出来必须是新标题，且其余姿势一个不少
        let (_s, body2) = unityfs::extract_pxls(&patch.bytes).unwrap();
        let p2 = pxlslib::parse(&body2).unwrap();
        assert_eq!(p2.poses.len(), p.poses.len());
        assert!(p2.poses.iter().any(|x| x.title == new_title));

        // 不等长时容器里所有东西也得对得上：SerializedFile 头 file_size == 节点大小 == 数据总长
        let (data2, info2) = unityfs::inflate_with_info(&patch.bytes).unwrap();
        let sf2 = SerializedFile::parse(&data2).unwrap();
        let fs = u64::from_be_bytes(data2[sf2.file_size_at..sf2.file_size_at + 8].try_into().unwrap());
        assert_eq!(fs as usize, info2.nodes[0].size);
        assert_eq!(info2.nodes[0].size, data2.len());
    }

    /// 改长度时对象尾部**必须重新对齐到 4**：正文只加 10 字节时填充要从 1 变 3。
    /// 只写 `byte_size += delta` 会让 UnityPy 报 "Expected to read N, but only read N+2"。
    #[test]
    fn text_asset_object_size_is_realigned_after_every_resize() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let base_title = pxlslib::parse(&body).unwrap().poses[0].title.clone();

        let (odata, _oinfo) = unityfs::inflate_with_info(&orig).unwrap();
        let osf = SerializedFile::parse(&odata).unwrap();
        let ota = osf.objects.iter().find(|o| o.class_name == "TextAsset").unwrap();
        let onext = osf.objects.iter().find(|o| o.byte_start > ota.byte_start).unwrap();
        // 本作对象之间本来就有 4 字节间隙（TextAsset 结束 370836 → AssetBundle 起点 370840），
        // 所以不该断言「紧邻」，而要断言「间隙不变、后续对象整段平移 delta」
        let gap = onext.byte_start - (ota.byte_start + ota.byte_size);

        for extra in 1..=8usize {
            let mut p = pxlslib::parse(&body).unwrap();
            let new_title = format!("{base_title}{}", "X".repeat(extra));
            let e = edits(&[(&base_title, new_title.as_str())], &[]);
            let (n, _) = apply_edits(&mut p, &e, "*");
            assert_eq!(n, 1, "改名必须命中");
            let new_body = p.serialize();
            let patch = write_back(&orig, &new_body, RepackComp::Original).unwrap();
            // 两个 delta 语义不同，别混：pxls_delta 是真实正文差，object_delta 是对齐后的对象差。
            // 注意填充**可增可减**（368227→368228 的 1 字节填充，在正文 +9 后变成 0），
            // 所以 object_delta 既可能大于也可能小于 body_delta，差值恒在 ±3 内。
            assert_eq!(
                patch.body_delta,
                new_body.len() as i64 - body.len() as i64,
                "extra={extra}: pxls_delta 必须是正文长度差"
            );
            let skew = patch.delta - patch.body_delta;
            assert!((-3..=3).contains(&skew), "extra={extra}: 填充差必须落在 ±3，实际 {skew}");

            let (data2, _info2) = unityfs::inflate_with_info(&patch.bytes).unwrap();
            let sf2 = SerializedFile::parse(&data2).unwrap();
            let o = sf2
                .objects
                .iter()
                .find(|o| o.class_name == "TextAsset")
                .expect("TextAsset 还在");
            let sig2 = unityfs::find_pxls_offset(&data2).unwrap();
            let len2 = u32::from_le_bytes(data2[sig2 - 4..sig2].try_into().unwrap()) as usize;
            assert_eq!(len2, new_body.len(), "长度前缀必须等于新正文长度");
            assert_eq!(o.byte_start, ota.byte_start, "目标对象起点不动");
            // `sig2 - o.byte_start` 已经包含 m_Name 前缀 + u32 长度字段
            assert_eq!(
                o.byte_size,
                align4(sig2 - o.byte_start + len2),
                "extra={extra}: 对象大小必须是「前缀 + 长度字段 + 正文」对齐到 4"
            );
            assert_eq!(o.byte_size % 4, 0, "extra={extra}: 对象大小必须 4 字节对齐");
            // 后续对象整段平移 delta，间隙不变
            let next = sf2.objects.iter().find(|x| x.byte_start > o.byte_start).unwrap();
            assert_eq!(
                next.byte_start,
                (onext.byte_start as i64 + patch.delta) as usize,
                "extra={extra}: 后续对象应整段平移 delta"
            );
            assert_eq!(next.byte_start - (o.byte_start + o.byte_size), gap, "extra={extra}: 间隙不变");
            self_check(&patch.bytes, &new_body).unwrap();
        }
    }

    /// JSON 里 `pxls_delta` 与 `object_delta` 是两个不同的数，别把任何一个改成另一个。
    /// 正文 +9 时填充从 1 掉到 0 → `object_delta = 8` 而 `body_delta = 9`。
    #[test]
    fn pxls_delta_is_the_body_delta_not_the_aligned_object_delta() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_s, body) = unityfs::extract_pxls(&orig).unwrap();
        let base_title = pxlslib::parse(&body).unwrap().poses[0].title.clone();
        let new_title = format!("{base_title}{}", "X".repeat(9));
        let mut p = pxlslib::parse(&body).unwrap();
        assert_eq!(apply_edits(&mut p, &edits(&[(&base_title, new_title.as_str())], &[]), "*").0, 1);
        let new_body = p.serialize();
        let patch = write_back(&orig, &new_body, RepackComp::Original).unwrap();

        assert_eq!(patch.body_delta, new_body.len() as i64 - body.len() as i64);
        assert_ne!(
            patch.body_delta, patch.delta,
            "本用例下正文增量与对象增量必须不同（否则这层区分就没被测到）"
        );
        assert_eq!(patch.body_delta, 9, "标题加了 9 个字符");
    }


    #[test]
    fn identity_repack_leaves_the_serialized_file_byte_identical() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (data, info) = unityfs::inflate_with_info(&orig).unwrap();
        let (_s, body) = unityfs::extract_pxls(&orig).unwrap();
        let patch = write_back(&orig, &body, RepackComp::Original).unwrap();
        let (data2, info2) = unityfs::inflate_with_info(&patch.bytes).unwrap();
        assert_eq!(data2, data, "未编辑时解包数据必须逐字节相同");
        assert_eq!(info2.nodes[0].size, info.nodes[0].size);
    }

    /// 改 alpha 不长不短，正文长度不变但内容变
    #[test]
    fn set_alpha_changes_content_without_resizing() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let mut p = pxlslib::parse(&body).unwrap();
        let layer = p.poses[0].seqs[0].frames[0].layers[0].name.clone();
        let e = edits(&[], &[(layer.as_str(), 42)]);
        let (_, n) = apply_edits(&mut p, &e, "*");
        assert!(n > 0, "至少要命中一个图层");
        let new_body = p.serialize();
        assert_eq!(new_body.len(), body.len());
        assert_ne!(new_body, body);

        let patch = write_back(&orig, &new_body, RepackComp::None).unwrap();
        assert_eq!(patch.delta, 0);
        self_check(&patch.bytes, &new_body).unwrap();
        let (_s, body2) = unityfs::extract_pxls(&patch.bytes).unwrap();
        let p2 = pxlslib::parse(&body2).unwrap();
        assert_eq!(p2.poses[0].seqs[0].frames[0].layers[0].alpha, 42);
    }

    /// 写回只能动 TextAsset 一处：同文件里后续的 `AssetBundle` 对象不能被复制/挪坏。
    /// （这是曾经的 bug —— 正文收边没收好，会把后面 189 字节整段复制一份。）
    #[test]
    fn write_back_never_duplicates_the_following_objects() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let mut p = pxlslib::parse(&body).unwrap();
        let target = p.poses[0].title.clone();
        let e = edits(&[(&target, "A_LONGER_POSE_TITLE_FOR_SIZE")], &[]);
        apply_edits(&mut p, &e, "*");
        let new_body = p.serialize();
        let patch = write_back(&orig, &new_body, RepackComp::Original).unwrap();
        assert!(patch.delta > 0, "改名变长后 delta 应为正");

        let before = object_map(&orig);
        let after = object_map(&patch.bytes);
        assert_eq!(before.len(), after.len(), "对象数不能变：{before:?} vs {after:?}");
        for (b, a) in before.iter().zip(after.iter()) {
            assert_eq!((&b.0, b.1), (&a.0, a.1), "对象 class / path_id / 顺序必须原样保留");
            if b.0 == "TextAsset" {
                assert_eq!(a.2 as i64 - b.2 as i64, patch.delta, "只有 TextAsset 该长胖");
            } else {
                assert_eq!(b.2, a.2, "非目标对象（{}）的 byte_size 不能动", b.0);
            }
        }
        assert!(after.iter().any(|o| o.0 == "AssetBundle"), "后续对象还在");
    }

    /// (class_name, path_id) → byte_size，用于比对写回前后对象表
    fn object_map(bytes: &[u8]) -> Vec<(String, i64, usize)> {
        let (data, _info) = unityfs::inflate_with_info(bytes).unwrap();
        let sf = SerializedFile::parse(&data).unwrap();
        sf.objects
            .iter()
            .map(|o| (o.class_name.clone(), o.path_id, o.byte_size))
            .collect()
    }

    /// 裸 pxls 容器走直通路径
    #[test]
    fn raw_container_passes_the_body_through() {
        let (_s, body) = {
            let Ok(orig) = std::fs::read(noel()) else { return };
            unityfs::extract_pxls(&orig).unwrap()
        };
        let patch = write_back(&body, &body, RepackComp::None).unwrap();
        assert_eq!(patch.container, "raw");
        assert_eq!(patch.bytes, body);
        assert_eq!(patch.object, None);
    }

    #[test]
    fn arg_parsing_rejects_garbage() {
        assert!(parse_compress("zstd").is_err());
        assert_eq!(parse_compress("lz4").unwrap(), RepackComp::Lz4);
        assert_eq!(parse_compress("").unwrap(), RepackComp::Original);
        assert!(parse_pairs(&["noequals".into()], "rename").is_err());
        let got = parse_pairs(&["a=b;c=d".into(), "e=f".into()], "rename").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[2], ("e".into(), "f".into()));
    }

    #[test]
    fn pose_glob_limits_which_poses_are_edited() {
        let Ok(orig) = std::fs::read(noel()) else { return };
        let (_src, body) = unityfs::extract_pxls(&orig).unwrap();
        let mut p = pxlslib::parse(&body).unwrap();
        let other = p
            .poses
            .iter()
            .map(|x| x.title.clone())
            .find(|t| t != &p.poses[0].title)
            .expect("至少两个姿势");
        let first = p.poses[0].title.clone();
        let e = edits(&[(&other, "SHOULD_NOT_APPLY")], &[]);
        let (n, _) = apply_edits(&mut p, &e, &first);
        assert_eq!(n, 0, "glob 不匹配的姿势不能被改");
    }

    #[test]
    fn missing_root_is_invalid_arg() {
        let app = Repack {
            root: PathBuf::from("nope/zzz/qq"),
            out: PathBuf::from("x"),
            pose: "*".into(),
            rename: Vec::new(),
            set_alpha: Vec::new(),
            compress: "original".into(),
            apply: false,
            overwrite: false,
            limit: 0,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let err = run_repack(&app, &Context::new_test(tx)).unwrap_err();
        assert!(matches!(err, AppError::InvalidArg(_)));
    }
}

