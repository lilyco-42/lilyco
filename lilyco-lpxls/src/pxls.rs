//! pxls 二进制解析 —— 规格逆向自 PixelLiner 反编译源码：
//! `PxlCharacter.loadProgress`（分节骨架）/ `PxlPose.readFromBytes` /
//! `PxlSequence` / `PxlFrame` / `PxlLayer` / `PxlsImgAtlas` / `PxlImage`。
//!
//! 字节序结论（ByteReader.readInt/readShort 手工逆序 + BitConverter 小端机）：
//! **磁盘上多字节数值为大端**。字符串 = u16 长度 + utf-8；子流 = u32 长度 + 字节。
//!
//! 文件自封闭（self-delimiting）：header(8B) 之后是若干 `14 字节节名 + u32 长度 + 载荷`，
//! 走到未知节名/剩余不足即结束 —— 所以从 UnityFS 解包数据里按签名定位即可整读。

use std::collections::BTreeMap;

/// file_header1 = 2000791807 = 0x7741A8FF（readUnsignedInt 按大端读 4 字节）
pub const MAGIC1: u32 = 0x7741_A8FF;
pub const MAGIC2: &[u8; 4] = b"PXLS";
/// 在 UnityFS 解包数据里定位 pxls 的完整签名（8 字节）
pub const SIGNATURE: [u8; 8] = [0x77, 0x41, 0xA8, 0xFF, b'P', b'X', b'L', b'S'];

pub const SECTION_IMGS: &str = "%IMGS_SECTION%";
pub const SECTION_PACK: &str = "%PACK_SECTION%";
pub const SECTION_IMGV: &str = "%IMGV_SECTION%";
pub const SECTION_POSE: &str = "%POSE_SECTION%";
pub const SECTION_PTCL: &str = "%PTCL_SECTION%";
const SECTION_LEN: usize = 14;

/// PxlLayer.type 常量（PxlLayer.cs:10-26）
pub const LAYER_TYPE_NAMES: [&str; 9] = [
    "luster", "import", "vector", "vimport", "float", "fimport", "vfloat", "vfimport", "group",
];
pub const TYPE_GROUP: i32 = 8;

// ─────────────────────────────────────────────────────────────
// 大端读取器（对齐 ByteReader 语义）
// ─────────────────────────────────────────────────────────────

pub struct Be<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Be<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Be { d, p: 0 }
    }
    pub fn pos(&self) -> usize {
        self.p
    }
    /// 16 字节对齐（UnityFS header version >= 7 的对齐要求）
    pub fn align16(&mut self) {
        self.p = self.p.div_ceil(16) * 16;
    }
    pub fn remaining(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.remaining() < n {
            return Err(format!("EOF: pos={} want={}", self.p, n));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    /// readByte()：signed char
    pub fn i8v(&mut self) -> Result<i32, String> {
        Ok(self.u8()? as i8 as i32)
    }
    pub fn bool(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn f64v(&mut self) -> Result<f64, String> {
        let b = self.take(8)?;
        Ok(f64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    /// readString()：u16 长度 + utf-8
    pub fn string(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        let b = self.take(n)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
    /// readMultiByte(n)：定长 utf-8
    pub fn multibyte(&mut self, n: usize) -> Result<String, String> {
        let b = self.take(n)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
    /// readExtractBytes() 的长度前缀 + 返回切片
    pub fn extract(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
}

// ─────────────────────────────────────────────────────────────
// 数据模型
// ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct Layer {
    pub index: usize,
    pub name: String,
    /// PxlLayer.type 原始值（0..=8，8=group）
    pub kind: i32,
    pub kind_name: String,
    pub group: bool,
    pub alpha: i32,
    pub x: f64,
    pub y: f64,
    pub zmx: f64,
    pub zmy: f64,
    pub rot_r: f64,
    pub blend_variable: u16,
    /// 关联图 key（PxlImage.getIdString 格式 "EDI{id:x}_{id2}"），图集里查不到时为 unknown
    pub img: String,
    /// 图集命中后的像素尺寸（宽, 高）
    pub img_size: Option<(u32, u32)>,
}

#[derive(serde::Serialize, Clone)]
pub struct Frame {
    pub index: usize,
    pub name: String,
    /// 1/60 秒为单位的帧时长（crf60）
    pub crf60: i16,
    pub layers: Vec<Layer>,
}

#[derive(serde::Serialize, Clone)]
pub struct Seq {
    /// 方向 0..=7（readByte-10；超出部分游戏也不会入表，这里保留原值便于诊断）
    pub dir: i32,
    pub width: u16,
    pub height: u16,
    pub body_x: i16,
    pub body_y: i16,
    pub shift_x: i16,
    pub shift_y: i16,
    pub loop_to: i16,
    pub frames: Vec<Frame>,
}

#[derive(serde::Serialize, Clone)]
pub struct Pose {
    pub title: String,
    pub width: u16,
    pub height: u16,
    pub auto_flip: bool,
    pub tetra_pose: bool,
    pub end_jump_loop_count: u16,
    pub end_jump_title: String,
    pub aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub seqs: Vec<Seq>,
}

impl Pose {
    pub fn frames_total(&self) -> usize {
        self.seqs.iter().map(|s| s.frames.len()).sum()
    }
}

#[derive(serde::Serialize, Default)]
pub struct Pxls {
    pub sections: Vec<String>,
    /// 图节条目数（IMGS/PACK/IMGV 合计）
    pub image_count: u32,
    pub pose_count: u32,
    pub poses: Vec<Pose>,
    /// 解析警告（图节里未消费的字节等诊断信息）
    pub warnings: Vec<String>,
    /// 图集命中表：(id, id2 bits) → (宽, 高)
    #[serde(skip)]
    pub img_sizes: BTreeMap<(u32, u64), (u32, u32)>,
}

impl Pxls {
    #[allow(dead_code)] // 语义参考：游戏 getPoseByName 从后往前找（同名取后者）
    pub fn get_pose(&self, title: &str) -> Option<&Pose> {
        // 游戏语义：getPoseByName 从后往前找（同名取后者）
        self.poses.iter().rev().find(|p| p.title == title)
    }
}

// ─────────────────────────────────────────────────────────────
// 顶层解析
// ─────────────────────────────────────────────────────────────

/// 从原始 pxls 字节（含签名处开始也可以，会自动对齐）整读
pub fn parse(d: &[u8]) -> Result<Pxls, String> {
    let mut r = Be::new(d);
    if r.u32()? != MAGIC1 {
        return Err("bad magic1 (not 0x7741A8FF)".into());
    }
    if r.take(4)? != MAGIC2 {
        return Err("bad magic2 (not PXLS)".into());
    }
    let mut out = Pxls::default();
    loop {
        if r.remaining() < SECTION_LEN {
            break;
        }
        let name = r.multibyte(SECTION_LEN)?;
        let known = matches!(
            name.as_str(),
            SECTION_IMGS | SECTION_PACK | SECTION_IMGV | SECTION_POSE | SECTION_PTCL
        );
        if !known {
            break; // 未知节名 = 结束（loader 的 default 分支）
        }
        let len = r.u32()? as usize;
        if r.remaining() < len {
            return Err(format!("section {name} truncated: want {len}, have {}", r.remaining()));
        }
        let payload = r.take(len)?;
        out.sections.push(name.to_string());
        match name.as_str() {
            SECTION_IMGS => parse_imgs(payload, &mut out)?,
            SECTION_PACK => parse_pack(payload, &mut out)?,
            SECTION_IMGV => {
                // PxlVectorData 不解析（矢量角色表），只记条目数
                let mut v = Be::new(payload);
                if let Ok(n) = v.u32() {
                    out.image_count += n;
                }
                out.warnings.push("IMGV section left unparsed (vector data)".into());
            }
            SECTION_POSE => parse_poses(payload, &mut out)?,
            SECTION_PTCL => {} // PxlPartsInfo 颜色表，姿势提取用不到
            _ => unreachable!(),
        }
    }
    out.pose_count = out.poses.len() as u32;
    Ok(out)
}

/// 图节（嵌入 PNG 的 PxlImage）：type(=raw-22)<0 视为异常停止并记录
fn parse_imgs(d: &[u8], out: &mut Pxls) -> Result<(), String> {
    let mut r = Be::new(d);
    let count = r.u32()?;
    for _ in 0..count {
        let t = r.i8v()?;
        if t < 22 {
            out.warnings
                .push(format!("IMGS entry byte {t} < 22 at pos {}, stop", r.pos() - 1));
            break;
        }
        let _flags = r.u8()?;
        let id = r.u32()?;
        let id2 = r.f64v()?;
        let ty = t - 22;
        if ty == 0 || ty == 8 {
            let l1 = r.u32()? as usize;
            r.skip(l1)?; // 嵌入 I png
            let l2 = r.u32()? as usize;
            r.skip(l2)?; // 嵌入 P png
        }
        // 仅嵌入图有尺寸信息吗？—— 嵌入 png 需解 PNG 头，v1 不做；尺寸以图集为准
        if ty == 0 || ty == 8 {
            let _ = (id, id2);
        }
    }
    out.image_count += count;
    Ok(())
}

/// 图集节（PxlsImgAtlas.readFromBytes）：UV 条目给出每张图的像素尺寸
fn parse_pack(d: &[u8], out: &mut Pxls) -> Result<(), String> {
    let mut r = Be::new(d);
    let count = r.u32()?;
    for _ in 0..count {
        let t = r.i8v()?;
        if t < 22 {
            out.warnings
                .push(format!("PACK entry byte {t} < 22 at pos {}, stop", r.pos() - 1));
            break;
        }
        let flags = r.u8()?;
        let margin = r.u8()? as u32;
        let nuv = r.u32()?;
        for _ in 0..nuv {
            let id = r.u32()?;
            let id2 = r.f64v()?;
            let _x = r.u32()?;
            let _y = r.u32()?;
            let w = r.u32()?;
            let h = r.u32()?;
            if w > margin * 2 && h > margin * 2 {
                out.img_sizes
                    .insert((id, id2.to_bits()), (w - margin * 2, h - margin * 2));
            }
        }
        if flags & 1 == 1 {
            let _ext_w = r.u32()?;
            let _ext_h = r.u32()?; // 外部贴图：只有尺寸，无 png 数据
        } else {
            let l = r.u32()? as usize;
            r.skip(l)?; // 内嵌图集 png
        }
    }
    out.image_count += count;
    Ok(())
}

fn parse_poses(d: &[u8], out: &mut Pxls) -> Result<(), String> {
    let mut r = Be::new(d);
    let count = r.u32()?;
    for _ in 0..count {
        out.poses.push(parse_pose(&mut r)?);
    }
    Ok(())
}

fn parse_pose(r: &mut Be) -> Result<Pose, String> {
    // PxlPose.readFromBytes：先 extract 头子流，再在主流上读 8 方向序列
    let sub = r.extract()?;
    let mut s = Be::new(sub);
    let ver = s.i8v()?;
    let auto_flip = s.bool()?;
    let tetra_pose = s.bool()?;
    let title = s.string()?;
    let width = s.u16()?;
    let height = s.u16()?;
    let end_jump_loop_count = s.u16()?;
    let end_jump_title = s.string()?;
    let nalias = s.u16()?;
    let mut aliases = Vec::with_capacity(nalias as usize);
    for _ in 0..nalias {
        aliases.push(s.string()?);
    }
    let comment = if ver >= 2 { Some(s.string()?) } else { None };

    let mut seqs = Vec::new();
    loop {
        let b = r.i8v()?;
        if b == 0 {
            break;
        }
        let dir = b - 10;
        seqs.push(parse_seq(r, dir)?);
    }

    Ok(Pose {
        title,
        width,
        height,
        auto_flip,
        tetra_pose,
        end_jump_loop_count,
        end_jump_title,
        aliases,
        comment,
        seqs,
    })
}

fn parse_seq(r: &mut Be, dir: i32) -> Result<Seq, String> {
    let sub = r.extract()?;
    let mut s = Be::new(sub);
    let _unknown = s.u8()?; // PxlSequence.readFromBytes 开头的 readByte（版本类字段，反编译未命名）
    let width = s.u16()?;
    let height = s.u16()?;
    let body_x = s.i16()?;
    let body_y = s.i16()?;
    let shift_x = s.i16()?;
    let shift_y = s.i16()?;
    let loop_to = s.i16()?;
    let nsnd = s.u16()?;
    for _ in 0..nsnd {
        let _ = s.string()?; // Aframe_snd（帧音效名）
    }

    // 帧在主流上（不是子流里）：u16 帧数 + N × PxlFrame
    let nframes = r.u16()?;
    let mut frames = Vec::with_capacity(nframes as usize);
    for _ in 0..nframes {
        frames.push(parse_frame(r)?);
    }

    Ok(Seq {
        dir,
        width,
        height,
        body_x,
        body_y,
        shift_x,
        shift_y,
        loop_to,
        frames,
    })
}

fn parse_frame(r: &mut Be) -> Result<Frame, String> {
    let sub = r.extract()?;
    let mut s = Be::new(sub);
    let _ver = s.i8v()?; // 帧版本号（传给 PxlLayer 的 vers，当前层格式未分支使用）
    let crf60 = s.i16()?;
    let name = s.string()?;
    if s.remaining() != 0 {
        let _ = s.u8()?; // 尾部可选 u8
    }

    let nlayers = r.i16()?;
    let mut layers = Vec::with_capacity(nlayers.max(0) as usize);
    for i in 0..nlayers.max(0) {
        layers.push(parse_layer(r, i as usize)?);
    }

    Ok(Frame {
        index: 0,
        name,
        crf60,
        layers,
    })
}

fn parse_layer(r: &mut Be, index: usize) -> Result<Layer, String> {
    // PxlLayer.readFromBytes（直接在主流上，无子流）
    let id = r.u32()?;
    let id2 = r.f64v()?;
    let kind = r.i8v()?;
    let name = r.string()?;
    let alpha_raw = r.i16()? as i32 / 100; // C# 整除语义
    let alpha = alpha_raw.clamp(0, 100);

    let mut lay = Layer {
        index,
        name,
        kind,
        kind_name: LAYER_TYPE_NAMES
            .get(kind as usize)
            .copied()
            .unwrap_or("unknown")
            .to_string(),
        group: false,
        alpha,
        x: 0.0,
        y: 0.0,
        zmx: 0.0,
        zmy: 0.0,
        rot_r: 0.0,
        blend_variable: 0,
        img: format!("EDI{:x}_{id2}", id),
        img_size: None,
    };

    if kind == TYPE_GROUP {
        lay.group = true;
        let _preserve_contain_layers = r.u32()?; // PxlGroupContainer.readFromBytes
        return Ok(lay);
    }

    lay.x = r.i16()? as f64 / 10.0;
    lay.y = r.i16()? as f64 / 10.0;
    lay.zmx = r.f64v()?;
    lay.zmy = r.f64v()?;
    lay.rot_r = r.f64v()?;
    lay.blend_variable = r.u16()?;
    let _unk_u32 = r.u32()?;
    let _unk_b1 = r.u8()?;
    let _unk_b2 = r.u8()?;
    lay.img_size = None; // 由调用方按 img key 回填
    Ok(lay)
}

/// 把图集尺寸回填到所有 layer 的 img_size（按 (id, id2) 查表）
pub fn attach_img_sizes(pxls: &mut Pxls) {
    for pose in &mut pxls.poses {
        for seq in &mut pose.seqs {
            for frame in &mut seq.frames {
                for lay in &mut frame.layers {
                    if let Some(sz) = parse_img_key(&lay.img) {
                        lay.img_size = pxls.img_sizes.get(&sz).copied();
                    }
                }
            }
        }
    }
}

/// "EDI1f_123.0" → (0x1f, 123.0f64.to_bits())
pub fn parse_img_key(s: &str) -> Option<(u32, u64)> {
    let rest = s.strip_prefix("EDI")?;
    let (hex, dec) = rest.split_once('_')?;
    let id = u32::from_str_radix(hex, 16).ok()?;
    let id2: f64 = dec.parse().ok()?;
    Some((id, id2.to_bits()))
}
