//! UnityFS bundle 解包。
//!
//! 两层用法：
//! 1. `extract_pxls`：从 `*.pxls.dat`（UnityFS 包着 TextAsset）里按 pxls 签名定位并交给
//!    pxls::parse —— pxls 格式自封闭，签名定位足够可靠。
//! 2. `inflate_bundle_ex`：完整解包，返回裸数据 + 节点表（offset/size/path），
//!    供 serialized.rs 切出内层 SerializedFile 与 `archive:/*.resS` 资源流。
//!
//! LZMA 压缩块已支持（Unity LZMA1 前 5 字节头 + 块表精确 unc 补全 alone 头）。

use crate::pxlslib::{Be, SIGNATURE};

/// UnityFS 包内的一个文件节点（通常是内层 SerializedFile 或 .resS 资源流）
#[derive(Debug, Clone)]
pub struct BundleNode {
    pub path: String,
    pub offset: usize,
    pub size: usize,
}

const FLAG_COMPRESSION_MASK: u32 = 0x3F;
const FLAG_BLOCKS_INFO_AT_END: u32 = 0x80;

const COMP_NONE: u32 = 0;
const COMP_LZMA: u32 = 1;
const COMP_LZ4: u32 = 2;
const COMP_LZ4HC: u32 = 3;

/// 从文件原始字节提取 pxls 内容。
/// 返回 (内容来源描述, pxls 字节)。原始 pxls 文件直接返回；UnityFS 解包后按签名定位。
pub fn extract_pxls(d: &[u8]) -> Result<(&'static str, Vec<u8>), String> {
    if d.len() >= 8 && d[0..4] == SIGNATURE[0..4] && &d[4..8] == b"PXLS" {
        return Ok(("raw", d.to_vec()));
    }
    if d.starts_with(b"UnityFS\0") {
        let raw = inflate_bundle(d).map_err(|e| format!("UnityFS inflate: {e}"))?;
        for (i, off) in find_signatures(&raw).into_iter().enumerate() {
            match crate::pxlslib::parse(&raw[off..]) {
                Ok(p) if !p.poses.is_empty() || p.image_count > 0 => {
                    return Ok(("UnityFS", raw[off..].to_vec()))
                }
                Ok(_) => continue,       // 空表：视为误命中，试下一个签名
                Err(_) if i < 3 => continue, // 误命中（嵌入 png 里的巧合字节），试下一个
                Err(e) => return Err(format!("pxls parse at sig+{off}: {e}")),
            }
        }
        return Err("UnityFS inflated but no pxls signature found".into());
    }
    Err("not a pxls file (raw) nor a UnityFS bundle".into())
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

/// UnityFS → 解压后的裸数据 + 节点表。
/// 节点表（blocks info 尾部）：offset u64 + size u64 + flags u32 + path(null 结尾)，
/// offset 相对解压后数据区起点 —— 用于切出内层 SerializedFile / .resS 资源流。
pub fn inflate_bundle_ex(d: &[u8]) -> Result<(Vec<u8>, Vec<BundleNode>), String> {
    let mut r = Be::new(d);
    let sig = r.take(8).map_err(|e| format!("header: {e}"))?;
    if sig != b"UnityFS\0" {
        return Err("not UnityFS".into());
    }
    let version = r.u32().map_err(|e| format!("version: {e}"))?;
    let _unity_version = cstring(&mut r)?;
    let _unity_revision = cstring(&mut r)?;
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
        d.len() - ci_size
    } else {
        r.pos()
    };
    if r.remaining() < ci_size {
        return Err("blocks info truncated".into());
    }
    let info_raw_slice: &[u8] = &d[info_pos..info_pos + ci_size];
    if flags & FLAG_BLOCKS_INFO_AT_END == 0 {
        r.take(ci_size).unwrap(); // !at_end 时推进读取位置（data 从 info 之后开始）
    }
    let info = decompress(info_raw_slice, ui_size, flags & FLAG_COMPRESSION_MASK)
        .map_err(|e| format!("blocks info decompress: {e}"))?;

    // 解析 blocks info：16 字节 hash + N × block + N × node
    let mut b = Be::new(&info);
    b.take(16).map_err(|e| format!("hash: {e}"))?;
    let nblocks = b.u32().map_err(|e| format!("nblocks: {e}"))?;
    let mut blocks = Vec::with_capacity(nblocks as usize);
    let mut total_unc = 0usize;
    for _ in 0..nblocks {
        let unc = b.u32().map_err(|e| format!("block.unc: {e}"))? as usize;
        let comp = b.u32().map_err(|e| format!("block.comp: {e}"))? as usize;
        let bflags = b.u16().map_err(|e| format!("block.flags: {e}"))?;
        total_unc += unc;
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
        let _nflags = b.u32().map_err(|e| format!("node.flags: {e}"))?;
        let path = cstring(&mut b).map_err(|e| format!("node.path: {e}"))?;
        nodes.push(BundleNode { path, offset: off as usize, size: sz as usize });
    }

    // 数据块：!at_end 时在 blocks info 之后；at_end 时回到 start。
    // BlockInfoNeedPaddingAtStart(0x200) 再对齐 16。
    let mut data_off = if flags & FLAG_BLOCKS_INFO_AT_END != 0 {
        start
    } else {
        info_pos + ci_size
    };
    if flags & 0x200 != 0 {
        data_off = data_off.div_ceil(16) * 16;
    }
    let mut out = Vec::with_capacity(total_unc);
    for (i, (unc, comp, comp_id)) in blocks.iter().enumerate() {
        if data_off + comp > d.len() {
            return Err(format!("block {i} out of range"));
        }
        let src = &d[data_off..data_off + comp];
        out.extend(decompress(src, *unc, *comp_id).map_err(|e| format!("block {i}: {e}"))?);
        data_off += comp;
    }
    Ok((out, nodes))
}

/// UnityFS → 解压后的裸数据（通常里面是一份 SerializedFile）
pub fn inflate_bundle(d: &[u8]) -> Result<Vec<u8>, String> {
    inflate_bundle_ex(d).map(|(data, _)| data)
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
