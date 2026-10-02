//! UnityFS bundle 解包 **与写回**。
//!
//! 三层用法：
//! 1. `extract_pxls`：从 `*.pxls.dat`（UnityFS 包着 TextAsset）里按 pxls 签名定位并交给
//!    pxls::parse —— pxls 格式自封闭，签名定位足够可靠。
//! 2. [`analyze`] / [`inflate_with_info`]：完整解包，返回裸数据 + [`BundleInfo`]
//!    （头字段 + blocks info + 节点表 + 数据区起点），供 serialized.rs 切出内层
//!    SerializedFile 与 `archive:/*.resS` 资源流。
//! 3. [`repack`]：拿改过的节点内容重新生成一个 UnityFS —— 写回能力的底座。
//!
//! ## 容器格式（实测本作 128 个 `*.pxls.dat` 完全一致）
//!
//! ```text
//! "UnityFS\0"(8) + version u32(=8) + unity_version cstring("5.x.x")
//! + unity_revision cstring("2022.3.62f2") + bundle_size u64
//! + ci_size u32 + ui_size u32 + flags u32          ← 头部共 50 字节
//! align16                                          ← version>=7
//! blocks info（ci_size 压缩字节）→ 解压出 ui_size 字节
//! align16（flags & 0x200）→ 数据块
//! ```
//!
//! 实测 flags = `0x243` = `LZ4HC(0x03) | DirectoryInfo(0x40) | NeedPaddingAtStart(0x200)`，
//! 且 **blocks info 紧跟头部**（没有 `0x80` AtEnd 位）。每个 pxls.dat 都只有 **1 个节点**
//! （`CAB-<32hex>` 的 SerializedFile），ci≈65 / ui=91 字节。
//!
//! blocks info 布局：`hash[16] + nblocks u32 + nblocks×(unc u32 + comp u32 + flags u16)`
//! `+ nnodes u32 + nnodes×(offset u64 + size u64 + flags u32 + path\0)`，全部大端。
//!
//! ## 写回策略
//!
//! [`repack`] **不沿用原始压缩块**，而是把节点内容按 0x20000 重新分块后整份重压：
//! - 压缩方式取「原文件的算法」或显式指定；Unity LZMA 编码器本仓库没有，故 LZMA 会退化为不压缩；
//! - 头部 hash 字段写 **16 个 0** —— 与 UnityPy `save_fs` 完全一致，读端不校验；
//! - 新头 `flags = 0x40 | comp_id`（DirectoryInfo + 压缩号），blocks info 紧跟头部、无起始对齐，
//!   是最简单的一种合法布局（UnityPy 的 `packer="none"` 走同一形状）。

use crate::pxlslib::{Be, BeW, SIGNATURE};

/// UnityFS 包内的一个文件节点（通常是内层 SerializedFile 或 .resS 资源流）
#[derive(Debug, Clone)]
pub struct BundleNode {
    pub path: String,
    pub offset: usize,
    pub size: usize,
    pub flags: u32,
}

/// UnityFS 头部 + blocks info 的完整描述（解包与写回共用）
///
/// 这里把容器形状完整摊开：写回要拿头字段与节点表重建，诊断（`--schema` 之外的人工核对）
/// 要对照 ci/ui/blocks。部分字段目前只有测试在断言「容器形状契约」，故允许 dead_code。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub version: u32,
    pub unity_version: String,
    pub unity_revision: String,
    pub flags: u32,
    /// blocks info 在文件里的起点
    pub info_pos: usize,
    pub ci_size: usize,
    pub ui_size: usize,
    /// blocks info 解压后的字节
    pub info_raw: Vec<u8>,
    /// 每块 `(未压缩大小, 压缩大小, 压缩号)`
    pub blocks: Vec<(usize, usize, u32)>,
    pub nodes: Vec<BundleNode>,
    /// 数据区起点（文件内绝对偏移）
    pub data_off: usize,
}

impl BundleInfo {
    /// 本包的压缩号（`flags & 0x3F`）
    pub fn compression(&self) -> u32 {
        self.flags & FLAG_COMPRESSION_MASK
    }

    /// 把解压后的整体数据切成各节点的内容
    #[allow(dead_code)] // 测试用来验证写回闭环
    pub fn node_slices<'a>(&self, data: &'a [u8]) -> Vec<&'a [u8]> {
        self.nodes
            .iter()
            .map(|n| data.get(n.offset..n.offset + n.size).unwrap_or(&[]))
            .collect()
    }
}

/// 写回时用哪种压缩
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepackComp {
    /// 不压缩（数据块与 blocks info 都裸存）
    None,
    /// LZ4 block（lz4_flex）
    Lz4,
    /// 沿用原文件的算法；本仓库只有 LZ4 编码器，LZMA/其它一律退化为不压缩
    Original,
}

const FLAG_COMPRESSION_MASK: u32 = 0x3F;
const FLAG_BLOCKS_INFO_AT_END: u32 = 0x80;
/// DirectoryInfo 标志：节点表存在（UnityFS 恒为真，UnityPy 写回时也强制要求）
const FLAG_DIRECTORY_INFO: u32 = 0x40;
/// 数据区起始需要 16 字节对齐
const FLAG_PADDING_AT_START: u32 = 0x200;

const COMP_NONE: u32 = 0;
const COMP_LZMA: u32 = 1;
const COMP_LZ4: u32 = 2;
const COMP_LZ4HC: u32 = 3;

/// 写回时的分块大小（与 Unity 自身的默认块大小一致）
const WRITE_BLOCK: usize = 0x20000;

/// 从文件原始字节提取 pxls 内容。
/// 返回 (内容来源描述, pxls 字节)。原始 pxls 文件直接返回；UnityFS 解包后按签名定位。
pub fn extract_pxls(d: &[u8]) -> Result<(&'static str, Vec<u8>), String> {
    if d.len() >= 8 && d[0..4] == SIGNATURE[0..4] && &d[4..8] == b"PXLS" {
        return Ok(("raw", d.to_vec()));
    }
    if d.starts_with(b"UnityFS\0") {
        let raw = inflate_bundle(d).map_err(|e| format!("UnityFS inflate: {e}"))?;
        let off = find_pxls_offset(&raw)
            .ok_or_else(|| "UnityFS inflated but no pxls signature found".to_string())?;
        return Ok(("UnityFS", bound_payload(&raw, off)));
    }
    Err("not a pxls file (raw) nor a UnityFS bundle".into())
}

/// 按正文前 4 字节的**长度前缀**（TypelessData 的 `u32 len`，小端）收边。
///
/// TextAsset 后面往往还跟着别的对象 —— 本作每个 `*.pxls.dat` 的 SerializedFile 里
/// TextAsset 之后就是一个 184 字节的 `AssetBundle` 对象（外加 1 字节对齐填充）。
/// 不收边的话正文会多带一截，写回时等于把后续对象复制一份。前缀解析不过时退回「到结尾」。
fn bound_payload(raw: &[u8], off: usize) -> Vec<u8> {
    if off >= 4 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&raw[off - 4..off]);
        let len = u32::from_le_bytes(b) as usize;
        if len > 0 && off + len <= raw.len() && crate::pxlslib::parse(&raw[off..off + len]).is_ok() {
            return raw[off..off + len].to_vec();
        }
    }
    raw[off..].to_vec()
}

/// 在解压后的数据里定位 pxls 正文起点（对候选做一次真实 parse，排除内嵌 PNG 里的巧合字节）
pub fn find_pxls_offset(d: &[u8]) -> Option<usize> {
    for (i, off) in find_signatures(d).into_iter().enumerate() {
        match crate::pxlslib::parse(&d[off..]) {
            Ok(p) if !p.poses.is_empty() || p.image_count > 0 => return Some(off),
            Ok(_) => continue,           // 空表：误命中
            Err(_) if i < 3 => continue, // 误命中（嵌入 png 里的巧合字节），试下一个
            Err(_) => break,
        }
    }
    None
}

fn find_signatures(d: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if d.len() < 8 {
        return out;
    }
    for i in 0..=d.len() - 8 {
        if d[i..i + 8] == SIGNATURE {
            out.push(i);
            if out.len() >= 8 {
                break;
            }
        }
    }
    out
}

/// 解析 UnityFS 头与 blocks info（不解压数据块）
pub fn analyze(d: &[u8]) -> Result<BundleInfo, String> {
    let mut r = Be::new(d);
    let sig = r.take(8).map_err(|e| format!("header: {e}"))?;
    if sig != b"UnityFS\0" {
        return Err("not UnityFS".into());
    }
    let version = r.u32().map_err(|e| format!("version: {e}"))?;
    let unity_version = cstring(&mut r)?;
    let unity_revision = cstring(&mut r)?;
    let _bundle_size = if version >= 7 {
        u64::from_be_bytes(r.take(8).map_err(|e| format!("size: {e}"))?.try_into().unwrap()) as usize
    } else {
        r.u32().map_err(|e| format!("size: {e}"))? as usize
    };
    let ci_size = r.u32().map_err(|e| format!("ci_size: {e}"))? as usize;
    let ui_size = r.u32().map_err(|e| format!("ui_size: {e}"))? as usize;
    let flags = r.u32().map_err(|e| format!("flags: {e}"))?;
    // 关键：header version >= 7（2019.4.15+ 对齐修复）后，读完 flags 要 16 字节对齐，
    // blocks info / data 都从对齐后的位置开始（UnityPy read_fs:129-131 同款逻辑）。
    if version >= 7 {
        r.align16();
    }
    let start = r.pos();

    // blocks info 位置：文件尾 或 header 对齐之后
    let info_pos = if flags & FLAG_BLOCKS_INFO_AT_END != 0 {
        d.len().saturating_sub(ci_size)
    } else {
        r.pos()
    };
    if info_pos + ci_size > d.len() {
        return Err(format!(
            "blocks info range {info_pos}+{ci_size} exceeds file size {}",
            d.len()
        ));
    }
    let info_raw_slice: &[u8] = &d[info_pos..info_pos + ci_size];
    if flags & FLAG_BLOCKS_INFO_AT_END == 0 {
        r.take(ci_size).map_err(|e| format!("skip blocks info: {e}"))?;
    }
    let info = decompress(info_raw_slice, ui_size, flags & FLAG_COMPRESSION_MASK)
        .map_err(|e| format!("blocks info decompress: {e}"))?;

    // 解析 blocks info：16 字节 hash + N × block + N × node
    let mut b = Be::new(&info);
    b.take(16).map_err(|e| format!("hash: {e}"))?;
    let nblocks = b.u32().map_err(|e| format!("nblocks: {e}"))?;
    let mut blocks = Vec::with_capacity(nblocks as usize);
    for _ in 0..nblocks {
        let unc = b.u32().map_err(|e| format!("block.unc: {e}"))? as usize;
        let comp = b.u32().map_err(|e| format!("block.comp: {e}"))? as usize;
        let bflags = b.u16().map_err(|e| format!("block.flags: {e}"))?;
        blocks.push((unc, comp, (bflags as u32) & FLAG_COMPRESSION_MASK));
    }
    let nnodes = b.u32().map_err(|e| format!("nnodes: {e}"))?;
    let mut nodes = Vec::with_capacity(nnodes as usize);
    for _ in 0..nnodes {
        let off = u64::from_be_bytes(
            b.take(8).map_err(|e| format!("node.offset: {e}"))?.try_into().unwrap(),
        );
        let sz = u64::from_be_bytes(
            b.take(8).map_err(|e| format!("node.size: {e}"))?.try_into().unwrap(),
        );
        let nflags = b.u32().map_err(|e| format!("node.flags: {e}"))?;
        let path = cstring(&mut b).map_err(|e| format!("node.path: {e}"))?;
        nodes.push(BundleNode { path, offset: off as usize, size: sz as usize, flags: nflags });
    }

    // 数据块：!at_end 时在 blocks info 之后；at_end 时回到 start。
    // BlockInfoNeedPaddingAtStart(0x200) 再对齐 16。
    let mut data_off = if flags & FLAG_BLOCKS_INFO_AT_END != 0 {
        start
    } else {
        info_pos + ci_size
    };
    if flags & FLAG_PADDING_AT_START != 0 {
        data_off = data_off.div_ceil(16) * 16;
    }

    Ok(BundleInfo {
        version,
        unity_version,
        unity_revision,
        flags,
        info_pos,
        ci_size,
        ui_size,
        info_raw: info,
        blocks,
        nodes,
        data_off,
    })
}

/// UnityFS → 解压后的裸数据 + 头信息
pub fn inflate_with_info(d: &[u8]) -> Result<(Vec<u8>, BundleInfo), String> {
    let info = analyze(d)?;
    let total_unc: usize = info.blocks.iter().map(|(u, _, _)| *u).sum();
    let mut out = Vec::with_capacity(total_unc);
    let mut off = info.data_off;
    for (i, (unc, comp, comp_id)) in info.blocks.iter().enumerate() {
        if off + comp > d.len() {
            return Err(format!("block {i} out of range"));
        }
        let src = &d[off..off + comp];
        out.extend(decompress(src, *unc, *comp_id).map_err(|e| format!("block {i}: {e}"))?);
        off += comp;
    }
    Ok((out, info))
}

/// UnityFS → 解压后的裸数据 + 节点表。
/// 节点表（blocks info 尾部）：offset u64 + size u64 + flags u32 + path(null 结尾)，
/// offset 相对解压后数据区起点 —— 用于切出内层 SerializedFile / .resS 资源流。
pub fn inflate_bundle_ex(d: &[u8]) -> Result<(Vec<u8>, Vec<BundleNode>), String> {
    let (data, info) = inflate_with_info(d)?;
    Ok((data, info.nodes))
}

/// UnityFS → 解压后的裸数据（通常里面是一份 SerializedFile）
pub fn inflate_bundle(d: &[u8]) -> Result<Vec<u8>, String> {
    inflate_bundle_ex(d).map(|(data, _)| data)
}

/// 只解压到前 `need` 字节为止。
///
/// SerializedFile 的元数据（类型表 + 对象表）永远在最前面，而数据区可能是一整条
/// **80 MB** 的 LZMA 贴图流（实测 `noel.pxls.bytes.texture_0.dat` 就只有一个块：
/// `unc=83,891,144`、`comp=2,518,015`、块压缩号 1=LZMA，而 blocks info 自己用的是 LZ4HC）——
/// 只想数一下对象个数时没必要全解。
///
/// LZMA 是流式的，可以解到够了就主动打断；LZ4 不行（要填满目标缓冲），但 LZ4 很快，全解也够便宜。
/// 返回内容可能短于 `need`（块用完就停），调用方按 `Le` 的边界检查自然失败再回退。
pub fn inflate_prefix(d: &[u8], need: usize) -> Result<Vec<u8>, String> {
    let info = analyze(d)?;
    let mut out = Vec::new();
    let mut off = info.data_off;
    for (i, (unc, comp, comp_id)) in info.blocks.iter().enumerate() {
        if out.len() >= need {
            break;
        }
        if off + comp > d.len() {
            return Err(format!("block {i} out of range"));
        }
        let src = &d[off..off + comp];
        let remaining = need - out.len();
        if remaining < *unc {
            // 原块比我们要的多：LZMA 可早停，成则直接收工
            if let Some(part) = decompress_lzma_prefix(src, *unc, *comp_id, remaining + PREFIX_SLACK) {
                out.extend_from_slice(&part);
                break;
            }
        }
        out.extend(decompress(src, *unc, *comp_id).map_err(|e| format!("block {i}: {e}"))?);
        off += comp;
    }
    Ok(out)
}

/// 早停时多要一点，避免正好切在元数据末尾
const PREFIX_SLACK: usize = 4096;

/// 流式接住前 `need` 字节就报错中止（`lzma-rs` 会把写出错原样抛回）
struct Cutoff {
    buf: Vec<u8>,
    need: usize,
}

impl std::io::Write for Cutoff {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let take = (self.need - self.buf.len()).min(b.len());
        self.buf.extend_from_slice(&b[..take]);
        if self.buf.len() >= self.need {
            return Err(std::io::Error::other("prefix satisfied"));
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 只解出 LZMA 块的前 `need` 字节；不足或非 LZMA 时返回 None（调用方回退到整块解）
fn decompress_lzma_prefix(src: &[u8], unc: usize, comp_id: u32, need: usize) -> Option<Vec<u8>> {
    if comp_id != COMP_LZMA || src.len() < 5 || need == 0 || need >= unc {
        return None;
    }
    let mut input = Vec::with_capacity(13 + src.len() - 5);
    input.extend_from_slice(&src[..5]);
    input.extend_from_slice(&(unc as u64).to_le_bytes());
    input.extend_from_slice(&src[5..]);
    let mut sink = Cutoff { buf: Vec::with_capacity(need), need };
    let _ = lzma_rs::lzma_decompress(&mut std::io::Cursor::new(&input), &mut sink);
    if sink.buf.len() >= need {
        Some(sink.buf)
    } else {
        None
    }
}

/// 用**新的节点内容**重新打包一个 UnityFS。
///
/// - `nodes_new` 必须与 `d` 里的节点数、顺序一致（改哪个节点就换哪个元素）；
/// - 节点偏移按顺序重新累加，因此改大一个节点后其后的节点会自动后移；
/// - 头部/块表/节点表全部重建，`bundle_size` 回填真实长度。
pub fn repack(d: &[u8], nodes_new: &[Vec<u8>], comp: RepackComp) -> Result<Vec<u8>, String> {
    let info = analyze(d)?;
    if nodes_new.len() != info.nodes.len() {
        return Err(format!(
            "node count mismatch: got {}, bundle has {}",
            nodes_new.len(),
            info.nodes.len()
        ));
    }
    let cid = match comp {
        RepackComp::None => COMP_NONE,
        RepackComp::Lz4 => COMP_LZ4,
        RepackComp::Original => match info.compression() {
            COMP_LZ4 | COMP_LZ4HC => COMP_LZ4,
            _ => COMP_NONE, // LZMA 等本仓库没有编码器，退化为不压缩（合法布局）
        },
    };

    // 数据区 = 节点顺序拼接
    let mut file_data: Vec<u8> = Vec::new();
    let mut offs: Vec<usize> = Vec::with_capacity(nodes_new.len());
    for n in nodes_new {
        offs.push(file_data.len());
        file_data.extend_from_slice(n);
    }

    // 数据块
    let mut blocks: Vec<(usize, usize, u16)> = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    if file_data.is_empty() {
        blocks.push((0, 0, cid as u16));
    } else {
        for chunk in file_data.chunks(WRITE_BLOCK) {
            let c = compress(chunk, cid)?;
            blocks.push((chunk.len(), c.len(), cid as u16));
            payload.extend_from_slice(&c);
        }
    }

    // blocks info
    let mut b = BeW::new();
    // uncompressedDataHash：UnityPy save_fs 同样写 16 个 0，读端不校验
    b.raw(&[0u8; 16]);
    b.u32(blocks.len() as u32);
    for (unc, c, f) in &blocks {
        b.u32(*unc as u32);
        b.u32(*c as u32);
        b.u16(*f);
    }
    b.u32(nodes_new.len() as u32);
    for (i, n) in info.nodes.iter().enumerate() {
        b.u64(offs[i] as u64);
        b.u64(nodes_new[i].len() as u64);
        b.u32(n.flags);
        b.raw(n.path.as_bytes());
        b.u8(0);
    }
    let ui_size = b.d.len();
    let info_payload = compress(&b.d, cid)?;

    // 头
    let mut w = BeW::new();
    w.raw(b"UnityFS\0");
    w.u32(info.version);
    w.raw(info.unity_version.as_bytes());
    w.u8(0);
    w.raw(info.unity_revision.as_bytes());
    w.u8(0);
    let size_at = w.d.len();
    if info.version >= 7 {
        w.u64(0); // bundle_size 占位，末尾回填
    } else {
        w.u32(0);
    }
    w.u32(info_payload.len() as u32);
    w.u32(ui_size as u32);
    // DirectoryInfo + 压缩号；blocks info 紧跟头部（不设 AtEnd / 不设起始对齐）
    w.u32(FLAG_DIRECTORY_INFO | cid);
    if info.version >= 7 {
        while !w.d.len().is_multiple_of(16) {
            w.u8(0);
        }
    }
    w.raw(&info_payload);
    w.raw(&payload);

    let total = w.d.len() as u64;
    if info.version >= 7 {
        w.d[size_at..size_at + 8].copy_from_slice(&total.to_be_bytes());
    } else {
        w.d[size_at..size_at + 4].copy_from_slice(&(total as u32).to_be_bytes());
    }
    Ok(w.d)
}

fn compress(src: &[u8], comp_id: u32) -> Result<Vec<u8>, String> {
    match comp_id {
        COMP_NONE => Ok(src.to_vec()),
        COMP_LZ4 | COMP_LZ4HC => Ok(lz4_flex::block::compress(src)),
        other => Err(format!("cannot write compression id {other}")),
    }
}

fn decompress(src: &[u8], unc: usize, comp_id: u32) -> Result<Vec<u8>, String> {
    match comp_id {
        COMP_NONE => {
            if src.len() != unc {
                return Err(format!("raw block size mismatch {}/{}", src.len(), unc));
            }
            Ok(src.to_vec())
        }
        COMP_LZ4 | COMP_LZ4HC => {
            let mut dst = vec![0u8; unc];
            let n = lz4_flex::block::decompress_into(src, &mut dst)
                .map_err(|e| format!("lz4: {e}"))?;
            dst.truncate(n);
            Ok(dst)
        }
        COMP_LZMA => lzma_decompress_unity(src, unc),
        other => Err(format!("unknown compression id {other}")),
    }
}

/// Unity 的 LZMA 数据块 = LZMA1 alone 格式的**前 5 字节头**（props + dict_size LE）+
/// 裸 LZMA1 流（缺 8 字节 unpacked-size 字段）。块表里有精确 unc —— 补全标准 13 字节
/// alone 头（size 填真值）后交给 lzma-rs（纯 Rust）。
fn lzma_decompress_unity(src: &[u8], unc: usize) -> Result<Vec<u8>, String> {
    if src.len() < 5 {
        return Err("lzma block too short".into());
    }
    let mut input = Vec::with_capacity(13 + src.len() - 5);
    input.extend_from_slice(&src[..5]); // props u8 + dict_size u32 LE（与 alone 头一致）
    input.extend_from_slice(&(unc as u64).to_le_bytes());
    input.extend_from_slice(&src[5..]);
    let mut out = Vec::with_capacity(unc);
    lzma_rs::lzma_decompress(&mut std::io::Cursor::new(&input), &mut out)
        .map_err(|e| format!("lzma: {e}"))?;
    Ok(out)
}

fn cstring(r: &mut Be) -> Result<String, String> {
    let mut out = Vec::new();
    loop {
        let c = r.u8()?;
        if c == 0 {
            break;
        }
        out.push(c);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str =
        "D:/gal/aic-winlator/game-clean/AliceInCradle/AliceInCradle_Data/StreamingAssets";

    fn pxls_files() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(STREAM)];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p
                    .file_name()
                    .map(|n| n.to_string_lossy().ends_with(".pxls.dat"))
                    .unwrap_or(false)
                {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    /// TextAsset 后面还跟着一个 `AssetBundle` 对象：正文必须按 m_Script 的长度前缀收边。
    /// 不收边的话写回会把后续对象整段复制一份（历史 bug，差点悄悄写坏文件）。
    #[test]
    fn pxls_body_is_bounded_by_its_length_prefix() {
        let path = std::path::Path::new(STREAM).join("PxlNoel/noel.pxls.dat");
        let Ok(orig) = std::fs::read(&path) else { return };
        let (data, _info) = inflate_with_info(&orig).unwrap();
        let sig = find_pxls_offset(&data).expect("pxls signature");
        let len = u32::from_le_bytes(data[sig - 4..sig].try_into().unwrap()) as usize;
        let body = extract_pxls(&orig).unwrap().1;

        assert_eq!(body.len(), len, "正文长度必须等于 m_Script 长度前缀");
        assert!(
            sig + len < data.len(),
            "本作的包在正文之后还有别的对象，正文不应该是「到结尾」"
        );
        // 正文之后确实是另一个对象（AssetBundle）
        let sf = crate::serialized::SerializedFile::parse(&data).unwrap();
        assert!(
            sf.objects.iter().any(|o| o.class_name == "AssetBundle"),
            "后续对象应是 AssetBundle"
        );
        // 收边之后仍能完整解析 + 回环
        let p = crate::pxlslib::parse(&body).unwrap();
        assert_eq!(p.serialize(), body);
    }

    /// 本作 pxls.dat 的容器形状必须稳定：version 8 / flags 0x243 / 单节点 / blocks info 紧跟头
    #[test]
    fn real_pxls_bundles_have_the_documented_shape() {
        let files = pxls_files();
        if files.is_empty() {
            return; // 真值不在（CI）时跳过
        }
        let mut n = 0;
        for f in &files {
            if f.file_name().map(|n| n.to_string_lossy().contains("texture_")).unwrap_or(false) {
                continue;
            }
            let d = std::fs::read(f).unwrap();
            if !d.starts_with(b"UnityFS\0") {
                continue; // 裸 pxls
            }
            let info = analyze(&d).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            assert_eq!(info.version, 8, "{}", f.display());
            assert_eq!(info.compression(), COMP_LZ4HC, "{}", f.display());
            assert_eq!(info.flags & FLAG_BLOCKS_INFO_AT_END, 0, "{}", f.display());
            assert_eq!(info.nodes.len(), 1, "{}", f.display());
            assert!(info.nodes[0].path.starts_with("CAB-"), "{}", f.display());
            n += 1;
        }
        eprintln!("checked {n} real UnityFS pxls bundles");
        assert!(n > 0);
    }

    /// 写回闭环：把节点内容原样重打包 → 再解包，必须逐字节相同
    #[test]
    fn repack_round_trips_every_real_bundle() {
        let files = pxls_files();
        if files.is_empty() {
            return;
        }
        let mut n = 0;
        for f in &files {
            if f.file_name().map(|n| n.to_string_lossy().contains("texture_")).unwrap_or(false) {
                continue;
            }
            let d = std::fs::read(f).unwrap();
            if !d.starts_with(b"UnityFS\0") {
                continue;
            }
            let (data, info) = inflate_with_info(&d).unwrap();
            for comp in [RepackComp::None, RepackComp::Lz4, RepackComp::Original] {
                let slices: Vec<Vec<u8>> = info.node_slices(&data).into_iter().map(|s| s.to_vec()).collect();
                let out = repack(&d, &slices, comp).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
                let (data2, info2) = inflate_with_info(&out)
                    .unwrap_or_else(|e| panic!("{} ({comp:?}): re-inflate {e}", f.display()));
                assert_eq!(data2.len(), data.len(), "{} ({comp:?})", f.display());
                assert_eq!(data2, data, "{} ({comp:?}) data differ", f.display());
                assert_eq!(info2.nodes.len(), info.nodes.len(), "{}", f.display());
            }
            n += 1;
        }
        eprintln!("repacked {n} bundles round-trip clean");
        assert!(n > 0);
    }
}
