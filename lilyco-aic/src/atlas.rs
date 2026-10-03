//! 图集配对与裁切 —— pxls 的 `%PACK_SECTION%` 只存 UV，真正的像素在
//! **外部贴图**（`<pxls>.pxls.bytes.texture_<i>.dat`，UnityFS 包着 Texture2D，
//! 老版本是裸 `.png`）或**内嵌 PNG**（`flags&1==0`）。
//!
//! 裁切矩形取自 `PxlsImgAtlas.assignPxlImage`：
//! ```text
//! rc.x = uv.x + margin;  rc.y = uv.y + margin;
//! rc.w = uv.w - margin*2; rc.h = uv.h - margin*2;
//! ```
//! 反编译里随后有一步 `rc.y = input_height - (rc.y + rc.height)`，那是把
//! **顶左原点**的 UV 换成 Unity 的**底左原点**（UV v=0 在底）。
//! 而 PNG 的行序就是顶左原点 —— 所以导出 PNG 时**直接用 (x+margin, y+margin)**，
//! 不要再翻 Y（AICPxlsUnpacker 的 PIL 裁切也是这个结论）。

use std::path::{Path, PathBuf};

use crate::serialized::{SfObject, SerializedFile};
use crate::unityfs;

/// 一张已解码的 RGBA8 图（行序 = 顶左原点，与 PNG 一致）
#[derive(Clone)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Img {
    pub fn new(w: u32, h: u32, rgba: Vec<u8>) -> Result<Img, String> {
        if rgba.len() != (w as usize) * (h as usize) * 4 {
            return Err(format!(
                "rgba size {} != {}x{}x4",
                rgba.len(),
                w,
                h
            ));
        }
        Ok(Img { w, h, rgba })
    }

    pub fn blank(w: u32, h: u32) -> Img {
        Img { w, h, rgba: vec![0u8; (w as usize) * (h as usize) * 4] }
    }

    /// 顶左原点裁切（超界部分用透明补齐）
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Img {
        let mut out = Img::blank(w, h);
        for row in 0..h {
            let sy = y as i64 + row as i64;
            if sy < 0 || sy >= self.h as i64 {
                continue;
            }
            for col in 0..w {
                let sx = x as i64 + col as i64;
                if sx < 0 || sx >= self.w as i64 {
                    continue;
                }
                let si = ((sy as usize) * (self.w as usize) + sx as usize) * 4;
                let di = ((row as usize) * (w as usize) + col as usize) * 4;
                out.rgba[di..di + 4].copy_from_slice(&self.rgba[si..si + 4]);
            }
        }
        out
    }
}

/// 按格式解码 Unity 贴图数据 → RGBA8（**顶左原点**，与 PNG 一致）
///
/// ## 格式号是 Unity 的 `TextureFormat` 枚举值
///
/// 权威表取自 UnityPy `enum TextureFormat`（与 Unity 2022.3 一致）。本作 125 个
/// `*.texture_0.dat` / 133 张 Texture2D 实际只出现六种：
///
/// | fmt | 名字 | 张数 | `m_StreamData.size` 与 w×h 的关系 |
/// |---|---|---|---|
/// | 12 | DXT5/BC3 | 71 | = w×h（1 B/px），或含 mip 链时 ×4/3 左右 |
/// | 25 | BC7 | 39 | = w×h（1 B/px） |
/// | 29 | DXT5Crunched | 11 | ≪ w×h（Crunch 压缩后的字节数） |
/// | 7 | RGB565 | 6 | = w×h×2 |
/// | 4 | RGBA32 | 5 | = w×h×4 |
/// | 3 | RGB24 | 1 | = w×h×3 |
///
/// ⚠️ **12 是 DXT5 而不是 DXT1**（DXT1 才是 10），**29 是 DXT5Crunched 而不是 ASTC**。
/// 早期版本把这两条写错，71 张 DXT5 图集全部解成斜纹噪点 ——
/// 这是拿 `m_StreamData.size` 与 `image_byte_size()` / UnityPy 交叉验证才抓出来的。
///
/// ## 行序
///
/// Unity 把贴图**底行在前**存在文件里（OpenGL 习惯），而 pxls 的 PACK UV 表和 PNG
/// 都是顶左原点。这里统一在解码末尾翻成顶左原点，与 UnityPy `Texture2D.image`
/// （内部 `transpose(FLIP_TOP_BOTTOM)`）的输出逐字节一致 —— 否则裁出来的 sprite
/// 会整体上下颠倒。
pub fn decode_texture(
    fmt: u32,
    w: u32,
    h: u32,
    inline: &[u8],
    res_s: Option<&[u8]>,
    stream_off: usize,
    stream_size: usize,
) -> Result<Vec<u8>, String> {
    let src: &[u8] = if stream_size > 0 {
        let rs = res_s.ok_or_else(|| {
            format!(".resS stream referenced but not found in bundle (need {stream_size}B @ {stream_off})")
        })?;
        rs.get(stream_off..stream_off + stream_size)
            .ok_or_else(|| format!("resS range {stream_off}+{stream_size} out of stream {}", rs.len()))?
    } else {
        inline
    };

    let npix = w as usize * h as usize;
    let name = format_name(fmt);
    // 块压缩格式先验长度：输入短了直接报错，别把越界读留给解码器。
    // Crunch（28/29）是压缩容器，体积不可预测，跳过。
    if !matches!(fmt, 28 | 29) {
        if let Some(want) = level_byte_size(fmt, w, h) {
            if src.len() < want {
                return Err(format!("{name} data short: {}/{}", src.len(), want));
            }
        }
    }
    let need = |bpp: usize, what: &str| -> Result<(), String> {
        let want = npix * bpp;
        if src.len() < want {
            Err(format!("{what} data short: {}/{}", src.len(), want))
        } else {
            Ok(())
        }
    };
    let mut out: Vec<u8> = match fmt {
        1 => {
            // Alpha8：1 B/px，只有 alpha
            need(1, "Alpha8")?;
            src[..npix].iter().flat_map(|&a| [255u8, 255, 255, a]).collect()
        }
        2 => {
            // ARGB4444：u16 LE，A(15..12) R(11..8) G(7..4) B(3..0)
            need(2, "ARGB4444")?;
            let mut o = Vec::with_capacity(npix * 4);
            for i in 0..npix {
                let v = u16::from_le_bytes([src[i * 2], src[i * 2 + 1]]);
                o.extend_from_slice(&[
                    exp4((v >> 8) as u8),
                    exp4((v >> 4) as u8),
                    exp4(v as u8),
                    exp4((v >> 12) as u8),
                ]);
            }
            o
        }
        3 => {
            // RGB24：字节序就是 R,G,B
            need(3, "RGB24")?;
            let mut o = Vec::with_capacity(npix * 4);
            for px in src[..npix * 3].as_chunks::<3>().0 {
                o.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            o
        }
        // RGBA32 / ARGB32：内存里都是 R,G,B,A 四字节（Unity 的 ARGB 只是 32 位字的写法）
        4 | 5 => {
            need(4, "RGBA32")?;
            src[..npix * 4].to_vec()
        }
        7 => {
            // RGB565：u16 LE → 5-6-5 展开
            need(2, "RGB565")?;
            let mut o = Vec::with_capacity(npix * 4);
            for i in 0..npix {
                let v = u16::from_le_bytes([src[i * 2], src[i * 2 + 1]]);
                let r = ((v >> 11) & 0x1F) as u32;
                let g = ((v >> 5) & 0x3F) as u32;
                let b = (v & 0x1F) as u32;
                o.extend_from_slice(&[
                    ((r * 527 + 23) >> 6) as u8,
                    ((g * 259 + 33) >> 6) as u8,
                    ((b * 527 + 23) >> 6) as u8,
                    255,
                ]);
            }
            o
        }
        9 => {
            // R16：单通道 16 位
            need(2, "R16")?;
            let mut o = Vec::with_capacity(npix * 4);
            for i in 0..npix {
                let g = (u16::from_le_bytes([src[i * 2], src[i * 2 + 1]]) >> 8) as u8;
                o.extend_from_slice(&[g, g, g, 255]);
            }
            o
        }
        // DXT1 / BC1
        10 => bc_decode(src, w, h, texture2ddecoder::decode_bc1, "bc1")?,
        // DXT5 / BC3 —— 本作主力格式（71/133）
        12 => bc_decode(src, w, h, texture2ddecoder::decode_bc3, "bc3")?,
        13 => {
            // RGBA4444：u16 LE，R(15..12) G(11..8) B(7..4) A(3..0)
            need(2, "RGBA4444")?;
            let mut o = Vec::with_capacity(npix * 4);
            for i in 0..npix {
                let v = u16::from_le_bytes([src[i * 2], src[i * 2 + 1]]);
                o.extend_from_slice(&[
                    exp4((v >> 12) as u8),
                    exp4((v >> 8) as u8),
                    exp4((v >> 4) as u8),
                    exp4(v as u8),
                ]);
            }
            o
        }
        14 => {
            // BGRA32：字节序 B,G,R,A
            need(4, "BGRA32")?;
            let mut o = src[..npix * 4].to_vec();
            for px in o.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            o
        }
        // BC7（本作 39 张，高清事件/立绘）
        25 => bc_decode(src, w, h, texture2ddecoder::decode_bc7, "bc7")?,
        26 => bc_decode(src, w, h, texture2ddecoder::decode_bc4, "bc4")?,
        27 => bc_decode(src, w, h, texture2ddecoder::decode_bc5, "bc5")?,
        // DXT1Crunched / DXT5Crunched（本作 11 张事件图）：Unity 的 Crunch 容器，
        // 解码器自己认内部格式（Dxt1 → bc1、Dxt5 → bc3、Etc1 → etc1 …）
        28 | 29 => bc_decode(src, w, h, texture2ddecoder::decode_unity_crunch, "unity-crunch")?,
        // ASTC（48..=51 = RGB 4x4/5x5/6x6/8x8，52..=55 = RGBA 同款）
        48..=55 => {
            let (bw, bh) = match fmt {
                48 | 52 => (4, 4),
                49 | 53 => (5, 5),
                50 | 54 => (6, 6),
                _ => (8, 8),
            };
            let mut raw = vec![0u32; npix];
            texture2ddecoder::decode_astc(src, w as usize, h as usize, bw, bh, &mut raw)
                .map_err(|e| format!("{name}: {e}"))?;
            bgra_to_rgba(&raw)
        }
        other => {
            return Err(format!(
                "format {other} ({name}) not supported for PNG export"
            ))
        }
    };

    flip_rows(&mut out, w, h);
    Ok(out)
}

/// BCn / Crunch 的统一通道：解码到 u32 缓冲再转 RGBA8
fn bc_decode<F>(src: &[u8], w: u32, h: u32, f: F, what: &str) -> Result<Vec<u8>, String>
where
    F: Fn(&[u8], usize, usize, &mut [u32]) -> Result<(), &'static str>,
{
    let mut raw = vec![0u32; w as usize * h as usize];
    f(src, w as usize, h as usize, &mut raw).map_err(|e| format!("{what}: {e}"))?;
    Ok(bgra_to_rgba(&raw))
}

/// 4 位通道展开到 8 位（v * 17）
const fn exp4(v: u8) -> u8 {
    (v & 0xF) * 17
}

/// 把**底行在前**的 Unity 贴图缓冲翻成顶左原点（原地，逐行交换）
fn flip_rows(buf: &mut [u8], w: u32, h: u32) {
    let stride = w as usize * 4;
    let rows = h as usize;
    if stride == 0 || rows < 2 || buf.len() < stride * rows {
        return;
    }
    for y in 0..rows / 2 {
        let bottom = rows - 1 - y;
        let (lo, hi) = buf.split_at_mut(bottom * stride);
        lo[y * stride..(y + 1) * stride].swap_with_slice(&mut hi[..stride]);
    }
}

/// Unity `TextureFormat` 枚举值 → 名字
///
/// 数值取自 UnityPy `enum TextureFormat`，与 Unity 2022.3 一致。
/// 早期版本这里整张表错位（把 12 当 DXT1、29 当 ASTC-4x4），导致 71 张图集解码成噪点。
pub fn format_name(fmt: u32) -> &'static str {
    match fmt {
        1 => "Alpha8",
        2 => "ARGB4444",
        3 => "RGB24",
        4 => "RGBA32",
        5 => "ARGB32",
        7 => "RGB565",
        9 => "R16",
        10 => "DXT1/BC1",
        12 => "DXT5/BC3",
        13 => "RGBA4444",
        14 => "BGRA32",
        15 => "RHalf",
        16 => "RGHalf",
        17 => "RGBAHalf",
        18 => "RFloat",
        19 => "RGFloat",
        20 => "RGBAFloat",
        21 => "YUY2",
        22 => "RGB9e5Float",
        24 => "BC6H",
        25 => "BC7",
        26 => "BC4",
        27 => "BC5",
        28 => "DXT1Crunched",
        29 => "DXT5Crunched",
        34 => "ETC_RGB4",
        47 => "ETC2_RGB",
        48 => "ASTC_RGB_4x4",
        49 => "ASTC_RGB_5x5",
        50 => "ASTC_RGB_6x6",
        51 => "ASTC_RGB_8x8",
        52 => "ASTC_RGBA_4x4",
        53 => "ASTC_RGBA_5x5",
        54 => "ASTC_RGBA_6x6",
        55 => "ASTC_RGBA_8x8",
        _ => "unknown",
    }
}

/// `texture2ddecoder` 的 `color()` 是 `u32::from_le_bytes([b, g, r, a])`，
/// 所以 u32 落到内存里就是 B,G,R,A 四字节 —— 这里换回 R,G,B,A。
fn bgra_to_rgba(px: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &p in px {
        let b = p.to_le_bytes();
        out.extend_from_slice(&[b[2], b[1], b[0], b[3]]);
    }
    out
}

/// 每层像素 → 字节：`(块宽, 块高, 每块字节)`。非块压缩格式就是 1×1 的「块」。
fn level_layout(fmt: u32) -> Option<(u32, u32, usize)> {
    Some(match fmt {
        1 => (1, 1, 1),             // Alpha8
        2 | 7 | 9 | 13 => (1, 1, 2), // ARGB4444 / RGB565 / R16 / RGBA4444
        3 => (1, 1, 3),             // RGB24
        4 | 5 | 14 => (1, 1, 4),    // RGBA32 / ARGB32 / BGRA32
        10 | 26 => (4, 4, 8),       // DXT1 / BC4
        12 | 25 | 27 => (4, 4, 16), // DXT5 / BC7 / BC5
        48 => (4, 4, 16),           // ASTC 4x4
        49 => (5, 5, 16),           // ASTC 5x5
        50 => (6, 6, 16),           // ASTC 6x6
        51 => (8, 8, 16),           // ASTC 8x8
        _ => return None,           // 含 28/29：Crunch 是压缩容器，体积不可预测
    })
}

fn level_byte_size(fmt: u32, w: u32, h: u32) -> Option<usize> {
    let (bw, bh, bytes) = level_layout(fmt)?;
    Some(w.div_ceil(bw) as usize * h.div_ceil(bh) as usize * bytes)
}

/// 该格式在 `w × h` 下**基础层**应占的原始字节数（`None` = 未知格式 / 压缩流不可预测）。
///
/// 这个函数是「格式认错」的照妖镜：Unity 会把流式贴图的 `m_StreamData.size`
/// 写成「基础层」或「完整 mip 链」的字节数，把 DXT5（1 B/px）当成 DXT1（0.5 B/px）
/// 立刻对不上（[`image_mip_byte_size`] 同理）。
pub fn image_byte_size(fmt: u32, w: u32, h: u32) -> Option<usize> {
    level_byte_size(fmt, w, h)
}

/// 含完整 mip 链的字节数。
///
/// 规则经 133 张真值贴图验证：每层宽高各减半（下限 1），一直算到 1×1，
/// **每层都要按块大小向上取整**（所以 2×2 / 1×2 / 1×1 各占 1 个 16 字节块）。
/// 实测 `_icons_fis` 256×256 DXT5 ⇒ 87408 B、`__ev_f` 1024×2048 DXT5 ⇒ 2796240 B，均精确吻合。
pub fn image_mip_byte_size(fmt: u32, w: u32, h: u32) -> Option<usize> {
    level_layout(fmt)?;
    let mut total = 0usize;
    let (mut w, mut h) = (w.max(1), h.max(1));
    loop {
        total += level_byte_size(fmt, w, h)?;
        if w == 1 && h == 1 {
            break;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    Some(total)
}

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "rgba buffer size mismatch".to_string())?;
    img.save(path).map_err(|e| format!("save {}: {e}", path.display()))
}

/// 把 RGBA 像素编码成 PNG **字节**（不落盘）。重建内嵌图集时要把它塞进 PACK 节。
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "rgba buffer size mismatch".to_string())?;
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("encode png: {e}"))?;
    Ok(buf)
}

/// 解码任意 PNG 字节（内嵌图、旧版外部贴图）
pub fn read_png(bytes: &[u8], what: &str) -> Result<Img, String> {
    let im = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| format!("{what}: not a decodable PNG: {e}"))?;
    let rgba = im.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(Img { w, h, rgba: rgba.into_raw() })
}

/// 在对象表里挑出「图集序号 `index`」对应的 Texture2D：返回 `(对象序号, asset 名)`。
///
/// 优先按 asset 名后缀 `_<index>` 匹配 —— 游戏就是这么取的：`MTRX` 加载
/// `...bytes.texture_0` 作为主图，`MTIOneImage.getPxlMtiParts` 再把名字里的 `_0`
/// 换成 `_1` 取部件图。**不能只按对象表顺序**：
/// `noel_t.pxls.bytes.texture_0.dat` 里 `texture_1` 就排在 `texture_0` 之前，
/// 按顺序取会把主图和部件图对调。名字都对不上时才退回顺序。
pub fn pick_texture(sf: &SerializedFile, data: &[u8], index: usize) -> Option<(usize, String)> {
    let texs: Vec<&SfObject> = sf.objects.iter().filter(|o| o.class_name == "Texture2D").collect();
    let suffix = format!("_{index}");
    if let Some(pos) = texs
        .iter()
        .position(|o| sf.peek_name(o, data).is_some_and(|n| n.ends_with(&suffix)))
    {
        let name = sf.peek_name(texs[pos], data).unwrap_or_default();
        return Some((pos, name));
    }
    if index < texs.len() {
        return Some((index, sf.peek_name(texs[index], data).unwrap_or_default()));
    }
    None
}

/// 解 UnityFS 包里**对象序号**为 `ord` 的 Texture2D → Img。
///
/// 包里装着几张 Texture2D 时，**调用方必须先用 [`pick_texture`] 按 asset 名选好**、
/// 把选定的对象序号传进来 —— 这里不再做第二次选择。`noel_t` 的对象表是倒的
/// （`texture_1` 排在 `texture_0` 前面），若按图集序号重选一次，主图和部件图会被对调。
pub fn read_unity_bundle_ordinal(bytes: &[u8], ord: usize, what: &str) -> Result<Img, String> {
    let (data, nodes) =
        unityfs::inflate_bundle_ex(bytes).map_err(|e| format!("{what}: UnityFS {e}"))?;
    let sf = SerializedFile::parse(&data).map_err(|e| format!("{what}: SerializedFile {e}"))?;
    let o = sf
        .objects
        .iter()
        .filter(|o| o.class_name == "Texture2D")
        .nth(ord)
        .ok_or_else(|| {
            format!(
                "{what}: Texture2D #{ord} missing (bundle has {})",
                bundle_texture_count_of(&sf)
            )
        })?;
    let tex_name = sf.peek_name(o, &data).unwrap_or_default();
    decode_texture_object(&sf, &data, &nodes, o, what, &tex_name)
}

/// 已定位的 Texture2D 对象 → 解码 → Img（选图逻辑之外的公共尾巴）
fn decode_texture_object(
    sf: &SerializedFile,
    data: &[u8],
    nodes: &[unityfs::BundleNode],
    o: &SfObject,
    what: &str,
    tex_name: &str,
) -> Result<Img, String> {
    let res_s: Option<&[u8]> = nodes
        .iter()
        .find(|n| n.path.ends_with(".resS"))
        .and_then(|n| data.get(n.offset..n.offset + n.size));
    let fields = sf.read_object(o, data).map_err(|e| format!("{what}: read Texture2D {e}"))?;
    let w = fields.get("m_Width").and_then(|v| v.as_int()).unwrap_or(0) as u32;
    let h = fields.get("m_Height").and_then(|v| v.as_int()).unwrap_or(0) as u32;
    let fmt = fields.get("m_TextureFormat").and_then(|v| v.as_int()).unwrap_or(0) as u32;
    let (stream_off, stream_size) = match fields.get("m_StreamData").and_then(|v| v.as_obj()) {
        Some(sd) => (
            sd.get("offset").and_then(|v| v.as_int()).unwrap_or(0) as usize,
            sd.get("size").and_then(|v| v.as_int()).unwrap_or(0) as usize,
        ),
        None => (0, 0),
    };
    let inline = fields.get("image data").and_then(|v| v.as_bytes()).unwrap_or(&[]);
    let rgba = decode_texture(fmt, w, h, inline, res_s, stream_off, stream_size).map_err(|e| {
        format!("{what}: texture \"{tex_name}\" {w}x{h} fmt {fmt}: {e}")
    })?;
    Img::new(w, h, rgba)
}

fn bundle_texture_count_of(sf: &SerializedFile) -> usize {
    sf.objects.iter().filter(|o| o.class_name == "Texture2D").count()
}

/// 主包里「图集序号 → (Texture2D 对象序号, asset 名)」的映射。
///
/// 只解压元数据前缀（1 MB 足够装下类型表 + 对象表 + 名字），失败才整包解 ——
/// 这些包里可能塞着 80 MB 的 LZMA 贴图流，读名字不该付那个代价。
fn bundle_texture_ordinals(bytes: &[u8], count: usize) -> Option<Vec<Option<(usize, String)>>> {
    let build = |s: &SerializedFile, d: &[u8]| -> Vec<Option<(usize, String)>> {
        (0..count).map(|i| pick_texture(s, d, i)).collect()
    };
    if let Ok(prefix) = unityfs::inflate_prefix(bytes, 1 << 20) {
        if let Ok(sf) = SerializedFile::parse(&prefix) {
            return Some(build(&sf, &prefix));
        }
    }
    let data = unityfs::inflate_bundle(bytes).ok()?;
    let sf = SerializedFile::parse(&data).ok()?;
    Some(build(&sf, &data))
}

/// 解 UnityFS 包里的第一张 Texture2D（兼容旧调用）
pub fn read_unity_bundle_first_texture(bytes: &[u8], what: &str) -> Result<Img, String> {
    read_unity_bundle_ordinal(bytes, 0, what)
}

/// 某个外部图集的像素来源
///
/// 照抄游戏 `PxlCharacter.loadExternalPngResource` 的解析顺序：
/// 1. `<stem>.pxls.bytes.texture_0.dat` 里**名字以 `_i` 结尾**的那张 Texture2D
///    （本作 `texture_1` 通常就打包在这里，磁盘上没有独立的 `texture_1.dat`）；
/// 2. 独立的 `<stem>.pxls.bytes.texture_<i>.dat`（UnityFS）/ `.png`（旧版裸图）；
/// 3. 都没有 → 无贴图。
#[derive(Debug, Clone)]
pub enum TextureSource {
    /// 打包在 `<path>` 里、asset 名为 `name` 的 Texture2D（对象序号 `index`）
    InBundle { path: PathBuf, index: usize, name: String },
    /// 独立文件
    File { path: PathBuf },
}

impl TextureSource {
    pub fn describe(&self) -> String {
        match self {
            TextureSource::InBundle { path, name, index } => {
                format!("{}#{name}(obj {index})", path.display())
            }
            TextureSource::File { path } => path.display().to_string(),
        }
    }

    /// bundle 内的 asset 名（独立文件为 None）
    pub fn asset_name(&self) -> Option<&str> {
        match self {
            TextureSource::InBundle { name, .. } => Some(name),
            TextureSource::File { .. } => None,
        }
    }

    pub fn read(&self) -> Result<Img, String> {
        match self {
            TextureSource::InBundle { path, index, .. } => {
                let bytes = std::fs::read(path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                // `index` 是 resolve 时按 asset 名选定的**对象序号**，直接读它 ——
                // 不要再按图集序号重选（`noel_t` 的对象表是倒的，重选会把主图/部件图对调）。
                read_unity_bundle_ordinal(&bytes, *index, &path.display().to_string())
            }
            TextureSource::File { path } => read_texture_file(path),
        }
    }
}

/// 一次把一张表的全部外部图集来源解析出来（bundle 只解一次元数据）
pub fn resolve_atlas_textures(pxls_path: &Path, count: usize) -> Vec<Option<TextureSource>> {
    let primary = find_paired_texture(pxls_path, 0);
    // 主包里每个图集序号对应的 (Texture2D 对象序号, asset 名)
    let mut in_bundle: Vec<Option<(usize, String)>> = vec![None; count];
    if let Some(bp) = &primary {
        match std::fs::read(bp) {
            Ok(bytes) if bytes.starts_with(b"UnityFS\0") => {
                if let Some(ords) = bundle_texture_ordinals(&bytes, count) {
                    in_bundle = ords;
                }
            }
            Ok(_) => {
                // 旧版裸 PNG：只有第 0 张
                if count > 0 {
                    in_bundle[0] = Some((0, String::new()));
                }
                return (0..count)
                    .map(|i| in_bundle[i].is_some().then(|| TextureSource::File { path: bp.clone() }))
                    .collect();
            }
            Err(_) => {}
        }
    }

    (0..count)
        .map(|i| {
            if let Some((path, (index, name))) = primary.clone().zip(in_bundle.get(i).cloned().flatten()) {
                return Some(TextureSource::InBundle { path, index, name });
            }
            if i > 0 {
                if let Some(p) = find_paired_texture(pxls_path, i as u32) {
                    return Some(TextureSource::File { path: p });
                }
            }
            None
        })
        .collect()
}

/// 读一个外部贴图文件（UnityFS 或裸 PNG）
pub fn read_texture_file(path: &Path) -> Result<Img, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let what = path.display().to_string();
    if bytes.starts_with(b"UnityFS\0") {
        read_unity_bundle_first_texture(&bytes, &what)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        read_png(&bytes, &what)
    } else {
        Err(format!("{what}: neither UnityFS bundle nor PNG"))
    }
}

/// 自动配对 pxls 的外部贴图（PxlsTexture.PxlsTexturePath：`<pxls>.bytes.texture_<i>`）。
/// 兼容改名/旧版：在 pxls 同目录里按 `<stem>.pxls*texture_<i>*` 找，
/// 优先级 `.texture_<i>.dat`(UnityFS) > `.texture_<i>.png`(裸 PNG) > 其它。
pub fn find_paired_texture(pxls_path: &Path, index: u32) -> Option<PathBuf> {
    let dir = pxls_path.parent()?;
    let name = pxls_path.file_name()?.to_string_lossy().to_string();
    // stem = 第一个 ".pxls" 之前的部分（boss_nusi.pxls.dat → boss_nusi）
    let stem = name.split(".pxls").next().unwrap_or(&name).to_string();
    let suffix = format!(".texture_{index}");
    let mut cands: Vec<(u8, PathBuf)> = Vec::new();
    for ent in std::fs::read_dir(dir).ok()?.flatten() {
        let p = ent.path();
        let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !n.starts_with(&stem) || !n.contains(&suffix) {
            continue;
        }
        if !n.starts_with(&format!("{stem}.pxls")) {
            continue;
        }
        let rank = if n.ends_with(&format!("{suffix}.dat")) {
            0
        } else if n.ends_with(&format!("{suffix}.png")) {
            1
        } else if n.ends_with(&suffix) {
            2
        } else {
            continue; // 形如 .texture_0.dat.manifest 等附加文件不选
        };
        cands.push((rank, p));
    }
    cands.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    cands.into_iter().map(|(_, p)| p).next()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str =
        "D:/gal/aic-winlator/game-clean/AliceInCradle/AliceInCradle_Data/StreamingAssets";

    /// Unity `TextureFormat` 编号表必须与官方枚举一致。
    ///
    /// 曾经把 `12` 当 DXT1、`29` 当 ASTC-4x4 —— 结果全游戏 71 张 DXT5 贴图全解成噪声，
    /// `sprites` / `render` 画出来的东西也是一团乱纹。
    #[test]
    fn texture_format_numbers_follow_the_unity_enum() {
        for (v, n) in [
            (1, "Alpha8"),
            (2, "ARGB4444"),
            (3, "RGB24"),
            (4, "RGBA32"),
            (5, "ARGB32"),
            (7, "RGB565"),
            (9, "R16"),
            (10, "DXT1/BC1"),
            (12, "DXT5/BC3"),
            (13, "RGBA4444"),
            (14, "BGRA32"),
            (25, "BC7"),
            (26, "BC4"),
            (27, "BC5"),
            (28, "DXT1Crunched"),
            (29, "DXT5Crunched"),
            (48, "ASTC_RGB_4x4"),
            (49, "ASTC_RGB_5x5"),
            (50, "ASTC_RGB_6x6"),
            (51, "ASTC_RGB_8x8"),
            (52, "ASTC_RGBA_4x4"),
        ] {
            assert_eq!(format_name(v), n, "TextureFormat {v}");
        }
    }

    /// 一个全白的 DXT5/BC3 块：alpha0=255 / alpha1=0 / alpha 索引全 0（用 alpha0），
    /// color0=0xFFFF（白）/ color1=0x0000 / 颜色索引全 0（用 color0）
    #[test]
    fn bc3_block_decodes_to_opaque_white() {
        let mut block = vec![0u8; 16];
        block[0] = 255; // alpha0
        block[1] = 0; // alpha1
        block[8] = 0xFF; // color0 LE 低字节
        block[9] = 0xFF; // color0 LE 高字节
        let px = decode_texture(12, 4, 4, &block, None, 0, 0).unwrap();
        assert_eq!(px.len(), 4 * 4 * 4);
        for p in px.as_chunks::<4>().0 {
            assert_eq!(p, &[255, 255, 255, 255], "BC3 全白块应解成不透明白");
        }
    }

    /// Unity 的 `Texture2D` 像素是**自底向上**存的，解码结果必须翻成顶左原点
    #[test]
    fn texture_rows_are_flipped_to_top_left_origin() {
        // 1×2 RGB24：输入第 0 行是图像**底部**（红），第 1 行是顶部（蓝）
        let src = [255u8, 0, 0, 0, 0, 255];
        let px = decode_texture(3, 1, 2, &src, None, 0, 0).unwrap();
        assert_eq!(&px[0..4], &[0, 0, 255, 255], "翻转后第 0 行应是原顶部（输入的最后一行）");
        assert_eq!(&px[4..8], &[255, 0, 0, 255]);
    }

    /// 全游戏真值：流式贴图的 `m_StreamData.size` 必须正好等于该格式的图像字节数。
    /// 这条能一眼抓出「格式认错」。
    #[test]
    fn streamed_texture_size_matches_its_format() {
        let root = std::path::PathBuf::from(STREAM);
        let mut checked = 0usize;
        let mut mismatched: Vec<String> = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for ent in rd.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !p.file_name().map(|n| n.to_string_lossy().ends_with(".texture_0.dat")).unwrap_or(false) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&p) else { continue };
                // 只解元数据 + 开头几个小对象（流式 Texture2D 对象本身很小）
                let Ok(prefix) = unityfs::inflate_prefix(&bytes, 512 * 1024) else { continue };
                let Ok(sf) = SerializedFile::parse(&prefix) else { continue };
                for o in sf.objects.iter().filter(|o| o.class_name == "Texture2D") {
                    // 内联大贴图的对象数据不在前缀里 —— 直接跳过
                    let Ok(f) = sf.read_object(o, &prefix) else { continue };
                    let w = f.get("m_Width").and_then(|v| v.as_int()).unwrap_or(0) as u32;
                    let h = f.get("m_Height").and_then(|v| v.as_int()).unwrap_or(0) as u32;
                    let fmt = f.get("m_TextureFormat").and_then(|v| v.as_int()).unwrap_or(0) as u32;
                    let size = f
                        .get("m_StreamData")
                        .and_then(|v| v.as_obj())
                        .and_then(|sd| sd.get("size"))
                        .and_then(|v| v.as_int())
                        .unwrap_or(0) as usize;
                    if size == 0 || w == 0 || h == 0 {
                        continue;
                    }
                    let Some(base) = image_byte_size(fmt, w, h) else { continue };
                    let mips = image_mip_byte_size(fmt, w, h);
                    if size != base && Some(size) != mips {
                        mismatched.push(format!(
                            "{}: {} {}x{} fmt {} 声明 {} 字节；基础层 {}，含 mip 链 {}",
                            p.display(),
                            o.class_name,
                            w,
                            h,
                            fmt,
                            size,
                            base,
                            mips.map_or("?".into(), |m| m.to_string())
                        ));
                    }
                    checked += 1;
                }
            }
        }
        if checked == 0 {
            return;
        }
        assert!(
            mismatched.is_empty(),
            "流式贴图大小与格式表对不上（格式认错了？）：\n{}",
            mismatched.join("\n")
        );
        eprintln!("{checked} 张流式贴图的体积与格式表完全吻合");
    }

    /// 外部图集 ↔ 贴图的解析契约（照抄游戏 `PxlCharacter.loadExternalPngResource`）：
    /// 第 i 个图集取 `<stem>.pxls.bytes.texture_0.dat` 里 **asset 名以 `_i` 结尾**的
    /// Texture2D，没有才退到独立的 `texture_<i>` 文件。
    /// （按名而不是按对象表顺序 —— `noel_t` 的 `texture_1` 就排在 `texture_0` 前面。）
    ///
    /// 实测 128 张表：125 张有配套贴图，其中 8 张**双图集**表（`noel` 系列、
    /// `noel_babydall`、`sub_mob_general`）的主包里各装**两张** Texture2D
    /// （`_0` 正常图 + `_1` 部件图），因此第 1 个图集也能取到像素。
    /// 另外 3 张（`_icons` / `noel_bassrobe` / `house`）这份 rip 整份没有贴图 —— 是数据缺口。
    ///
    /// 只有「多图集」的表才真的进包数 Texture2D：单图集的第 0 张必然来自主包，纯文件系统
    /// 判断即可。这些包是**单块 ~80 MB** 的 LZ4，LZ4 块内不可部分解压，进包一次就是一次全解
    /// —— 对全部 128 张表都做会让这个测试从 1.5 s 涨到 70 s（debug 构建）。
    #[test]
    fn external_atlas_texture_resolution_contract() {
        let root = std::path::PathBuf::from(STREAM);
        let Ok(targets) = crate::util::collect_targets(&root) else { return };
        if targets.is_empty() {
            return;
        }
        let mut single = 0usize;
        let mut multi: Vec<String> = Vec::new();
        let mut unpaired_with_uvs: Vec<String> = Vec::new();
        let mut textureless: Vec<String> = Vec::new();

        for path in &targets {
            let Ok(p) = crate::util::load_pxls(path) else { continue };
            let table = path
                .file_name()
                .map(|n| n.to_string_lossy().split(".pxls").next().unwrap_or("").to_string())
                .unwrap_or_default();
            // 第 0 个图集的来源就是「主包文件存在与否」——纯文件系统判断，不必进包。
            if find_paired_texture(path, 0).is_none() {
                textureless.push(table.clone());
                continue;
            }
            if p.atlas.len() <= 1 {
                // 单图集：第 0 张 Texture2D 必然来自主包，没有歧义，不必进包数。
                single += 1;
                continue;
            }
            // 多图集：第 i(i>0) 个可能打包在主包里，也可能根本没有 —— 只有这种表必须真进去数。
            let sources = resolve_atlas_textures(path, p.atlas.len());
            let got = sources.iter().filter(|s| s.is_some()).count();
            multi.push(format!("{table}({} of {})", got, p.atlas.len()));
            for (i, at) in p.atlas.iter().enumerate() {
                if at.flags & 1 != 1 {
                    continue; // 内嵌图集不走外部文件
                }
                match sources.get(i).and_then(|s| s.clone()) {
                    Some(TextureSource::InBundle { name, .. }) => {
                        // 选图必须按 asset 名后缀 `_<i>` 走（游戏 `getPxlMtiParts` 就是换名字），
                        // 而不是对象表顺序 —— `noel_t` 的 `texture_1` 就排在 `texture_0` 之前。
                        assert!(
                            name.ends_with(&format!("_{i}")),
                            "{table}: 图集 {i} 应选到 asset 名以 `_{i}` 结尾的贴图，实得 `{name}`"
                        );
                    }
                    Some(TextureSource::File { .. }) => {}
                    None => {
                        // 无 UV 的 PARTS 图集本就不产生 sprite（复用前一个图集的 UV 表）。
                        if !p.effective_uvs(i).is_empty() {
                            unpaired_with_uvs.push(format!("{table}#{i}"));
                        }
                    }
                }
            }
        }

        textureless.sort_unstable();
        assert_eq!(
            textureless,
            ["_icons", "house", "noel_bassrobe"],
            "没有配套贴图的表变了 —— 要么是新数据，要么是解析逻辑坏了"
        );
        assert!(
            unpaired_with_uvs.is_empty(),
            "这些带 UV 的外部图集取不到像素：{unpaired_with_uvs:?}"
        );
        multi.sort_unstable();
        // 8 张双图集表的主包里都装了 2 张 Texture2D（`_0` 正常图 + `_1` 部件图），
        // 所以第 1 个图集也能取到像素 —— 这不是 noel 的个例。
        assert_eq!(
            multi,
            [
                "noel(2 of 2)",
                "noel_babydall(2 of 2)",
                "noel_magic(2 of 2)",
                "noel_magic_torned(2 of 2)",
                "noel_r18(2 of 2)",
                "noel_r18_torned(2 of 2)",
                "noel_t(2 of 2)",
                "sub_mob_general(2 of 2)",
            ],
            "多图集的表变了 —— 第 i 张 Texture2D 的分配规则要重新核对"
        );
        eprintln!(
            "{single} 张单图集表 + {} 张多图集表全部有主包；多图集分配：{multi:?}；             {} 张表没有配套贴图（{textureless:?}）",
            multi.len(),
            textureless.len()
        );
        assert!(single > 0, "真值目录没扫到任何单图集表");
    }

    /// `noel` 的形状：atlas 0 = NORMAL（1505 UV），atlas 1 = PARTS（0 UV，继承 atlas 0 的 UV 表），
    /// 两者都从**同一个** `texture_0.dat` 里取图（第 0 / 第 1 张 Texture2D）。
    #[test]
    fn noel_normal_and_parts_atlases_share_uvs_but_not_pixels() {
        let path = std::path::Path::new(STREAM).join("PxlNoel/noel.pxls.dat");
        let Ok(p) = crate::util::load_pxls(&path) else { return };
        assert_eq!(p.atlas.len(), 2, "noel 有 2 个 PACK 条目");
        assert_eq!(crate::pxlslib::atlas_img_type(&p.atlas[0]), crate::pxlslib::IMG_TYPE_NORMAL);
        assert_eq!(crate::pxlslib::atlas_img_type(&p.atlas[1]), crate::pxlslib::IMG_TYPE_PARTS);
        assert!(!p.atlas[0].uvs.is_empty(), "atlas 0 自带 UV");
        assert!(p.atlas[1].uvs.is_empty(), "atlas 1 的 UV 数为 0");
        assert!(p.uv_inherited_from(1).is_some(), "atlas 1 继承 atlas 0 的 UV");
        assert_eq!(p.effective_uvs(0), p.effective_uvs(1), "两者有效 UV 表相同");
        assert_eq!(p.effective_uvs(1).len(), p.atlas[0].uvs.len());

        let sources = resolve_atlas_textures(&path, p.atlas.len());
        match (&sources[0], &sources[1]) {
            (
                Some(TextureSource::InBundle { path: a, index: 0, name: na }),
                Some(TextureSource::InBundle { path: b, index: 1, name: nb }),
            ) => {
                assert_eq!(a, b, "两张贴图打包在同一个 bundle 里");
                assert!(na.ends_with("texture_0"), "atlas 0 取主图，实得 {na}");
                assert!(nb.ends_with("texture_1"), "atlas 1 取部件图，实得 {nb}");
            }
            other => panic!("noel 的两个图集都该从主包取图，实得 {other:?}"),
        }
        assert_atlas_pixels_follow_names(&path, "noel");
    }

    /// `noel_t` 的坑：8 张双图集表里**只有它**的对象表把 `texture_1` 排在 `texture_0` 前面。
    /// 若按对象表顺序取图，主图与部件图就会对调，而画面上「看起来还挺像」——
    /// 所以必须按 asset 名后缀 `_<i>` 选（游戏 `MTIOneImage.getPxlMtiParts` 正是换名字取图）。
    #[test]
    fn noel_t_picks_textures_by_asset_name_not_by_object_order() {
        let path = std::path::Path::new(STREAM).join("PxlNoel/noel_t.pxls.dat");
        let Ok(p) = crate::util::load_pxls(&path) else { return };
        assert_eq!(p.atlas.len(), 2, "noel_t 有 2 个 PACK 条目");

        let sources = resolve_atlas_textures(&path, p.atlas.len());
        for (i, s) in sources.iter().enumerate() {
            let name = s.as_ref().and_then(|s| s.asset_name()).unwrap_or("");
            assert!(
                name.ends_with(&format!("_{i}")),
                "图集 {i} 必须取 `_{i}` 那张，实得 `{name}`"
            );
        }
        // 本用例的全部价值在于「对象表顺序 ≠ 名字编号」这件事确实成立；
        // 顺序一旦变了，这条测试会失效，届时应重新核对游戏语义而不是删掉它。
        let ords: Vec<usize> = sources
            .iter()
            .map(|s| match s {
                Some(TextureSource::InBundle { index, .. }) => *index,
                other => panic!("noel_t 的两个图集都该从主包取图，实得 {other:?}"),
            })
            .collect();
        assert_ne!(
            ords,
            vec![0, 1],
            "noel_t 的对象表顺序本该是反的；真变了就得重新确认这个陷阱还在不在"
        );
        assert_eq!(ords, vec![1, 0], "本例固定：texture_1 在对象表第 0 位");
        assert_atlas_pixels_follow_names(&path, "noel_t");
    }

    /// 图集 `i` 读到的像素必须等于「asset 名以 `_texture_{i}` 结尾的那个对象」解出来的像素。
    ///
    /// 只比名字不比像素的测试拦不住「读图时又按图集序号重选一次」这类 bug ——
    /// `noel_t` 的对象表是倒的，重选一次主图/部件图就对调，而画面上看起来还挺像。
    fn assert_atlas_pixels_follow_names(path: &std::path::Path, table: &str) {
        let p = crate::util::load_pxls(path).unwrap();
        let bp = find_paired_texture(path, 0).expect("双图集表必有主包");
        let bundle = std::fs::read(&bp).unwrap();
        let (data, _) = unityfs::inflate_bundle_ex(&bundle).unwrap();
        let sf = SerializedFile::parse(&data).unwrap();
        let sources = resolve_atlas_textures(path, p.atlas.len());
        for (i, s) in sources.iter().enumerate() {
            let got = s
                .as_ref()
                .unwrap_or_else(|| panic!("{table}: 图集 {i} 没配到贴图"))
                .read()
                .unwrap();
            let (ord, name) =
                pick_texture(&sf, &data, i).unwrap_or_else(|| panic!("{table}: 图集 {i} 选不到图"));
            // 本作 asset 名形如 `noel.pxls.bytes.texture_0` —— 分隔符是 `.` 不是 `_`
            assert!(
                name.ends_with(&format!("texture_{i}")),
                "{table}: 图集 {i} 应选到 `*.texture_{i}`，实得 `{name}`"
            );
            let want = read_unity_bundle_ordinal(&bundle, ord, table).unwrap();
            assert_eq!((got.w, got.h), (want.w, want.h), "{table}: 图集 {i} 尺寸不对");
            assert_eq!(
                got.rgba, want.rgba,
                "{table}: 图集 {i} 读到的像素必须来自 `{name}`，而不是对象表第 {i} 个"
            );
        }
    }
}

