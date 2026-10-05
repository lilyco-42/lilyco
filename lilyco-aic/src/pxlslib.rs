//! pxls 二进制解析 / 序列化 —— 规格逆向自 PixelLiner 反编译源码：
//! `PxlCharacter.loadProgress`（分节骨架）/ `PxlPose.readFromBytes` /
//! `PxlSequence` / `PxlFrame` / `PxlLayer` / `PxlsImgAtlas` / `PxlImage`。
//!
//! 字节序结论（ByteReader.readInt/readShort 手工逆序 + BitConverter 小端机）：
//! **磁盘上多字节数值为大端**。字符串 = u16 长度 + utf-8；子流 = u32 长度 + 字节。
//!
//! 文件自封闭（self-delimiting）：header(8B) 之后是若干 `14 字节节名 + u32 长度 + 载荷`，
//! 走到未知节名/剩余不足即结束 —— 所以从 UnityFS 解包数据里按签名定位即可整读。
//!
//! 分节（PxlCharacter 的 LP 状态机）：
//! `%IMGS_SECTION%`(内嵌 PNG 图) / `%IMGD_SECTION%`(压缩图，游戏自身会 callError) /
//! `%PACK_SECTION%`(打包图集 UV) / `%IMGV_SECTION%`(矢量) / `%POSE_SECTION%` /
//! `%PTCL_SECTION%`(部件色表)。
//!
//! 写回：`serialize()` 可把模型还原成字节。PACK / IMGS / POSE 走结构化重写（可编辑），
//! IMGV / PTCL / IMGD 与节后尾部字节原样透传，因此**未编辑时 parse→serialize 字节相同**。

use std::collections::BTreeMap;

/// file_header1 = 2000791807 = 0x7741A8FF（readUnsignedInt 按大端读 4 字节）
pub const MAGIC1: u32 = 0x7741_A8FF;
pub const MAGIC2: &[u8; 4] = b"PXLS";
/// 在 UnityFS 解包数据里定位 pxls 的完整签名（8 字节）
pub const SIGNATURE: [u8; 8] = [0x77, 0x41, 0xA8, 0xFF, b'P', b'X', b'L', b'S'];

pub const SECTION_IMGS: &str = "%IMGS_SECTION%";
pub const SECTION_IMGD: &str = "%IMGD_SECTION%";
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

/// `PxlsImgAtlas.img_type`（= 磁盘字节 - 22）
pub const IMG_TYPE_NORMAL: i32 = 0;
pub const IMG_TYPE_PARTS: i32 = 1;
pub const IMG_TYPE_PARTS_SIMPLIFIED: i32 = 2;

/// `PackAtlas.raw_type` → `img_type`
pub fn atlas_img_type(at: &PackAtlas) -> i32 {
    at.raw_type - 22
}

/// `img_type` → 名字（PxlsImgAtlas.IMG_TYPE_*）
pub fn img_type_name(t: i32) -> &'static str {
    match t {
        IMG_TYPE_NORMAL => "normal",
        IMG_TYPE_PARTS => "parts",
        IMG_TYPE_PARTS_SIMPLIFIED => "parts_simplified",
        _ => "unknown",
    }
}

// ─────────────────────────────────────────────────────────────
// 大端读取器（对齐 ByteReader 语义）
// ─────────────────────────────────────────────────────────────

pub struct Be<'a> {
    pub(crate) d: &'a [u8],
    pub(crate) p: usize,
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
    pub fn f32v(&mut self) -> Result<f32, String> {
        let b = self.take(4)?;
        Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
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
    /// readExtractBytes() 的长度前缀 + 返回切片（长度在前）
    pub fn extract(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
}

// ─────────────────────────────────────────────────────────────
// 大端写入器（serialize 用；与 Be 严格对称）
// ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct BeW {
    pub d: Vec<u8>,
}

impl BeW {
    pub fn new() -> Self {
        BeW::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.d.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.d.push(if v { 1 } else { 0 });
    }
    pub fn i16(&mut self, v: i16) {
        self.d.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u16(&mut self, v: u16) {
        self.d.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.d.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.d.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.d.extend_from_slice(&v.to_be_bytes());
    }
    /// readString 的逆：u16 长度 + utf-8
    pub fn string(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.d.extend_from_slice(s.as_bytes());
    }
    /// readExtractBytes 的逆：u32 长度 + 字节
    pub fn extract(&mut self, payload: &[u8]) {
        self.u32(payload.len() as u32);
        self.d.extend_from_slice(payload);
    }
    pub fn raw(&mut self, b: &[u8]) {
        self.d.extend_from_slice(b);
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
    /// 图集/嵌入图命中后的像素尺寸（宽, 高）
    pub img_size: Option<(u32, u32)>,
    /// 组层专用：preserve_contain_layers（它认领前方 N 个层作为子层）
    pub group_n: Option<u32>,
    /// 图 key 原始分量（写回用，避免字符串解析误差）
    #[serde(skip)]
    pub id: u32,
    #[serde(skip)]
    pub id2: f64,
    /// 原始 alpha i16（`alpha = raw/100` 是整除，反推会丢精度，写回必须用这个）
    #[serde(skip)]
    pub raw_alpha: i16,
    /// 未使用但必须原样写回的字段（PxlLayer.readFromBytes 尾部 3 个匿名读）
    #[serde(skip)]
    pub unk_u32: u32,
    #[serde(skip)]
    pub unk_b1: u8,
    #[serde(skip)]
    pub unk_b2: u8,
}

#[derive(serde::Serialize, Clone)]
pub struct Frame {
    pub index: usize,
    pub name: String,
    /// 1/60 秒为单位的帧时长（crf60）
    pub crf60: i16,
    pub layers: Vec<Layer>,
    #[serde(skip)]
    pub ver: i32,
    /// 帧头子流尾部的可选字节（反编译：`if bytesAvailable != 0 readUByte`）
    #[serde(skip)]
    pub trailer: Option<u8>,
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
    /// 序列头子流首个字节（反编译未命名）
    #[serde(skip)]
    pub ver: u8,
    /// 帧音效名表
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frame_snd: Vec<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct VectorData {
    /// 所属图 key
    pub img: String,
    /// PxlVectorData.type：0=LUSTER 2=VECTOR 8=LUSTER_HAS_VECTOR（蒙版）
    pub kind: i32,
    /// 多边形顶点：(x, y, z)，z 的 bit0=1 表示新子路径起点（碰撞体 path 分隔）
    pub points: Vec<(f32, f32, f32)>,
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
    /// 姿势版本号（>=2 才有 comment）
    #[serde(skip)]
    pub ver: i32,
}

impl Pose {
    pub fn frames_total(&self) -> usize {
        self.seqs.iter().map(|s| s.frames.len()).sum()
    }
}

impl Layer {
    /// 按「显示 alpha」（0..=100）写回。
    ///
    /// 解析侧是 `alpha = clamp(raw / 100, 0, 100)`（C# 整除），所以反向必须写
    /// `raw = alpha * 100`，这样再读一遍恰好得到同一个 alpha。
    pub fn set_alpha(&mut self, alpha: i32) {
        let a = alpha.clamp(0, 100);
        self.raw_alpha = (a * 100) as i16;
        self.alpha = a;
    }
}

/// 图集 UV 条目（PxlsImgAtlasUv）：裁切来源
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct AtlasUv {
    pub id: u32,
    pub id2: f64,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// 图 key（"EDI{id:x}_{id2}"）
    pub img: String,
}

/// PACK 节的图集条目（PxlsImgAtlas）
#[derive(serde::Serialize, Clone)]
pub struct PackAtlas {
    /// 原始 type 字节（raw-22；0=普通 1=部件 2=部件简化）
    pub raw_type: i32,
    /// flags：bit0=外部贴图，bit1=部件简化标记
    pub flags: u8,
    pub margin: u8,
    pub uvs: Vec<AtlasUv>,
    /// flags&1==1：外部贴图的声明尺寸 (w, h)
    pub external_wh: Option<(u32, u32)>,
    /// flags&1==0：内嵌图集 PNG 字节
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_png: Option<Vec<u8>>,
}

/// IMGS 节的内嵌图条目（PxlImage）
#[derive(serde::Serialize, Clone)]
pub struct EmbeddedImg {
    pub raw_type: i32,
    pub flags: u8,
    pub id: u32,
    pub id2: f64,
    pub img: String,
    /// 正常图 PNG 字节长度（无则 0）
    pub i_len: usize,
    /// 部件/阴影图 PNG 字节长度（无则 0）
    pub p_len: usize,
    #[serde(skip)]
    pub i_png: Option<Vec<u8>>,
    #[serde(skip)]
    pub p_png: Option<Vec<u8>>,
}

/// PTCL 节的部件色表条目（PxlPartsInfo）
#[derive(serde::Serialize, Clone)]
pub struct PartInfo {
    /// topcolor（已 &0xF0F0F0 | 0xFF000000）
    pub topcolor: u32,
    pub name: String,
    pub memo: String,
    pub locked: bool,
}

#[derive(serde::Serialize, Default)]
pub struct Pxls {
    pub sections: Vec<String>,
    /// 图节条目数（IMGS/PACK/IMGV 合计）
    pub image_count: u32,
    pub pose_count: u32,
    pub poses: Vec<Pose>,
    /// IMGV 矢量数据（多边形/碰撞轮廓）
    pub vectors: Vec<VectorData>,
    /// 解析警告（图节里未消费的字节等诊断信息）
    pub warnings: Vec<String>,
    /// 图集命中表：(id, id2 bits) → (宽, 高)
    #[serde(skip)]
    pub img_sizes: BTreeMap<(u32, u64), (u32, u32)>,
    /// PACK 图集（裁切 / 写回用）
    pub atlas: Vec<PackAtlas>,
    /// IMGS 内嵌图
    pub embedded: Vec<EmbeddedImg>,
    /// PTCL 部件色表（解析失败时为空）
    pub parts: Vec<PartInfo>,
    /// 出现了 `%IMGD_SECTION%`（压缩图节：游戏本体与 pixelliner4j 都会硬失败，本工具跳过并继续）
    pub imgd: bool,
    // ── 写回透传（不参与 JSON）──
    #[serde(skip)]
    pub raw_sections: Vec<(String, Vec<u8>)>,
    #[serde(skip)]
    pub raw_tail: Vec<u8>,
}

impl Pxls {
    #[allow(dead_code)] // 语义参考：游戏 getPoseByName 从后往前找（同名取后者）
    pub fn get_pose(&self, title: &str) -> Option<&Pose> {
        // 游戏语义：getPoseByName 从后往前找（同名取后者）
        self.poses.iter().rev().find(|p| p.title == title)
    }

    /// 第 `index` 个图集的**有效 UV 表**。
    ///
    /// 游戏语义（`PxlsImgAtlas.readFromBytes` + `readFromBytesStack`）：
    /// PARTS 图集（`img_type == 1`）UV 数为 0 时，`Apos` 直接**继承前一个图集**的 UV 数组 ——
    /// 同一套 rect、另一张贴图（撕破 / 部件覆盖版）。递归是因为前一个图集自己也可能是继承来的。
    ///
    /// 注意：`PackAtlas.uvs` 本身**保持为空**，序列化才能和原始字节逐字节一致；
    /// 继承只在这里做「读取时」的解释。
    pub fn effective_uvs(&self, index: usize) -> &[AtlasUv] {
        self.effective_uvs_inner(index, 0)
    }

    fn effective_uvs_inner(&self, index: usize, depth: usize) -> &[AtlasUv] {
        if depth > 16 {
            return &[]; // 理论上不会发生；防环
        }
        let Some(at) = self.atlas.get(index) else { return &[] };
        if !at.uvs.is_empty() {
            return &at.uvs;
        }
        if atlas_img_type(at) == IMG_TYPE_PARTS && index > 0 {
            return self.effective_uvs_inner(index - 1, depth + 1);
        }
        &[]
    }

    /// 第 `index` 个图集的 UV 是否继承自前一个图集（供诊断/命名用）
    pub fn uv_inherited_from(&self, index: usize) -> Option<usize> {
        let at = self.atlas.get(index)?;
        if !at.uvs.is_empty() || atlas_img_type(at) != IMG_TYPE_PARTS || index == 0 {
            return None;
        }
        Some(index - 1)
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
            SECTION_IMGS | SECTION_IMGD | SECTION_PACK | SECTION_IMGV | SECTION_POSE | SECTION_PTCL
        );
        if !known {
            // 未知节名 = 结束（loader 的 default 分支）；把名字退回去当尾部数据
            r.p -= SECTION_LEN;
            break;
        }
        let len = r.u32()? as usize;
        if r.remaining() < len {
            return Err(format!("section {name} truncated: want {len}, have {}", r.remaining()));
        }
        let payload = r.take(len)?;
        out.sections.push(name.to_string());
        out.raw_sections.push((name.to_string(), payload.to_vec()));
        match name.as_str() {
            SECTION_IMGS => parse_imgs(payload, &mut out)?,
            SECTION_IMGD => out.imgd = true, // 压缩图节：跳过载荷继续（游戏自身不支持，这里只标识）
            SECTION_PACK => parse_pack(payload, &mut out)?,
            SECTION_IMGV => parse_imgv(payload, &mut out)?,
            SECTION_POSE => parse_poses(payload, &mut out)?,
            SECTION_PTCL => parse_ptcl(payload, &mut out),
            _ => unreachable!(),
        }
    }
    out.raw_tail = r.d[r.p..].to_vec();
    out.pose_count = out.poses.len() as u32;
    Ok(out)
}

/// 矢量节（PxlVectorData.progressVectorDataReading）：
/// 首 u32 条目数，之后逐条 `u32 id + f64 id2 + type + skip + u32 npts + npts×(f32,f32,f32)`
fn parse_imgv(d: &[u8], out: &mut Pxls) -> Result<(), String> {
    let mut r = Be::new(d);
    let count = r.u32()?;
    while r.remaining() >= 14 {
        let id = r.u32()?;
        let id2 = r.f64v()?;
        let kind = r.i8v()?;
        let _skip = r.u8()?;
        let npts = r.u32()? as usize;
        let mut points = Vec::with_capacity(npts);
        for _ in 0..npts {
            let x = r.f32v()?;
            let y = r.f32v()?;
            let z = r.f32v()?;
            points.push((x, y, z));
        }
        out.vectors.push(VectorData {
            img: img_key(id, id2),
            kind,
            points,
        });
    }
    out.image_count += count;
    Ok(())
}

/// PTCL 部件色表（PxlPartsInfo.readColorData）。
/// 反编译里 `X.IntC(20f/32f)` 疑似被碾平（10 列 × 2 行 × 4B = 80B 色表），
/// 因此按 80B 猜测解析并**校验**：条目数读完必须正好到尾部，否则丢弃结果只当透传。
fn parse_ptcl(d: &[u8], out: &mut Pxls) {
    let mut r = Be::new(d);
    let parsed = (|| -> Result<Vec<PartInfo>, String> {
        let _ = r.u8()?; // 占位
        let n = r.u8()? as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let topcolor = (r.u32()? & 0x00F0_F0F0) | 0xFF00_0000;
            let ver = r.u8()?;
            let cnt = r.i16()?;
            if cnt > 0 {
                r.skip(cnt as usize * 4)?;
            }
            let name = r.string()?;
            let _ = r.u8()?;
            r.skip(80)?; // 色表（20 项 × 4B）
            let mut memo = String::new();
            let mut locked = false;
            if ver >= 1 {
                memo = r.string()?;
                if ver >= 2 {
                    locked = r.u8()? & 1 > 0;
                }
            }
            v.push(PartInfo { topcolor, name, memo, locked });
        }
        Ok(v)
    })();
    if let Ok(v) = parsed {
        if r.remaining() == 0 {
            out.parts = v;
            return;
        }
    }
    out.warnings.push(format!(
        "PTCL section present but color-table layout unconfirmed ({} bytes) — kept verbatim",
        d.len()
    ));
}

/// PNG IHDR：跳过 8B 签名 + 4B 长度 + 4B "IHDR" 后是宽、高（各 u32 大端）
fn png_dims(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((w, h))
}

pub fn img_key(id: u32, id2: f64) -> String {
    format!("EDI{id:x}_{id2}")
}

/// 图节（嵌入 PNG 的 PxlImage）：type(=raw-22)<0 视为异常停止并记录；
/// type 0/8 携带嵌入 I/P 两张 PNG，尺寸直接从 IHDR 读
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
        let flags = r.u8()?;
        let id = r.u32()?;
        let id2 = r.f64v()?;
        let ty = t - 22;
        let mut i_png = None;
        let mut p_png = None;
        if ty == 0 || ty == 8 {
            for slot in 0..2 {
                let l = r.u32()? as usize;
                let data = r.take(l)?;
                if l >= 24 {
                    if let Some(dim) = png_dims(data) {
                        out.img_sizes.insert((id, id2.to_bits()), dim);
                    }
                }
                if slot == 0 {
                    i_png = Some(data.to_vec());
                } else {
                    p_png = Some(data.to_vec());
                }
            }
        }
        out.embedded.push(EmbeddedImg {
            raw_type: t,
            flags,
            id,
            id2,
            img: img_key(id, id2),
            i_len: i_png.as_ref().map(|v| v.len()).unwrap_or(0),
            p_len: p_png.as_ref().map(|v| v.len()).unwrap_or(0),
            i_png,
            p_png,
        });
    }
    out.image_count += count;
    Ok(())
}

/// 图集节（PxlsImgAtlas.readFromBytes）：UV 条目给出每张图的像素尺寸与裁切位置
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
        let margin = r.u8()?;
        let nuv = r.u32()?;
        let m = margin as u32;
        let mut uvs = Vec::with_capacity(nuv as usize);
        for _ in 0..nuv {
            let id = r.u32()?;
            let id2 = r.f64v()?;
            let x = r.u32()?;
            let y = r.u32()?;
            let w = r.u32()?;
            let h = r.u32()?;
            if w > m * 2 && h > m * 2 {
                out.img_sizes.insert((id, id2.to_bits()), (w - m * 2, h - m * 2));
            }
            uvs.push(AtlasUv { id, id2, x, y, w, h, img: img_key(id, id2) });
        }
        let (external_wh, embedded_png) = if flags & 1 == 1 {
            let w = r.u32()?;
            let h = r.u32()?;
            (Some((w, h)), None)
        } else {
            let l = r.u32()? as usize;
            (None, Some(r.take(l)?.to_vec()))
        };
        out.atlas.push(PackAtlas {
            raw_type: t,
            flags,
            margin,
            uvs,
            external_wh,
            embedded_png,
        });
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
        ver,
    })
}

fn parse_seq(r: &mut Be, dir: i32) -> Result<Seq, String> {
    let sub = r.extract()?;
    let mut s = Be::new(sub);
    let ver = s.u8()?; // PxlSequence.readFromBytes 开头的 readByte（版本类字段，反编译未命名）
    let width = s.u16()?;
    let height = s.u16()?;
    let body_x = s.i16()?;
    let body_y = s.i16()?;
    let shift_x = s.i16()?;
    let shift_y = s.i16()?;
    let loop_to = s.i16()?;
    let nsnd = s.u16()?;
    let mut frame_snd = Vec::with_capacity(nsnd as usize);
    for _ in 0..nsnd {
        frame_snd.push(s.string()?); // Aframe_snd（帧音效名）
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
        ver,
        frame_snd,
    })
}

fn parse_frame(r: &mut Be) -> Result<Frame, String> {
    let sub = r.extract()?;
    let mut s = Be::new(sub);
    let ver = s.i8v()?; // 帧版本号（传给 PxlLayer 的 vers，当前层格式未分支使用）
    let crf60 = s.i16()?;
    let name = s.string()?;
    let trailer = if s.remaining() != 0 { Some(s.u8()?) } else { None };

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
        ver,
        trailer,
    })
}

fn parse_layer(r: &mut Be, index: usize) -> Result<Layer, String> {
    // PxlLayer.readFromBytes（直接在主流上，无子流）
    let id = r.u32()?;
    let id2 = r.f64v()?;
    let kind = r.i8v()?;
    let name = r.string()?;
    let raw_alpha = r.i16()?;
    let alpha = (raw_alpha as i32 / 100).clamp(0, 100); // C# 整除语义

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
        img: img_key(id, id2),
        img_size: None,
        group_n: None,
        id,
        id2,
        raw_alpha,
        unk_u32: 0,
        unk_b1: 0,
        unk_b2: 0,
    };

    if kind == TYPE_GROUP {
        lay.group = true;
        lay.group_n = Some(r.u32()?); // PxlGroupContainer.readFromBytes: preserve_contain_layers
        return Ok(lay);
    }

    lay.x = r.i16()? as f64 / 10.0;
    lay.y = r.i16()? as f64 / 10.0;
    lay.zmx = r.f64v()?;
    lay.zmy = r.f64v()?;
    lay.rot_r = r.f64v()?;
    lay.blend_variable = r.u16()?;
    lay.unk_u32 = r.u32()?;
    lay.unk_b1 = r.u8()?;
    lay.unk_b2 = r.u8()?;
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

// ─────────────────────────────────────────────────────────────
// 序列化（parse 的逆）
// ─────────────────────────────────────────────────────────────

impl Pxls {
    /// 还原成 pxls 字节。未编辑时与输入字节相同（PACK/IMGS/POSE 结构化重写，
    /// IMGV/PTCL/IMGD 与尾部字节原样透传）。
    pub fn serialize(&self) -> Vec<u8> {
        let mut w = BeW::new();
        w.u32(MAGIC1);
        w.raw(MAGIC2);
        for (name, raw) in &self.raw_sections {
            let payload: Vec<u8> = match name.as_str() {
                SECTION_PACK => write_pack(&self.atlas),
                SECTION_IMGS => write_imgs(&self.embedded),
                SECTION_POSE => write_poses(&self.poses),
                // IMGV / PTCL / IMGD：无结构化编辑，原样回写
                _ => raw.clone(),
            };
            let mut n = name.clone();
            while n.len() < SECTION_LEN {
                n.push('\0');
            }
            w.raw(n.as_bytes());
            w.u32(payload.len() as u32);
            w.raw(&payload);
        }
        w.raw(&self.raw_tail);
        w.d
    }
}

fn write_poses(poses: &[Pose]) -> Vec<u8> {
    let mut w = BeW::new();
    w.u32(poses.len() as u32);
    for p in poses {
        let mut h = BeW::new();
        h.u8(p.ver as u8);
        h.bool(p.auto_flip);
        h.bool(p.tetra_pose);
        h.string(&p.title);
        h.u16(p.width);
        h.u16(p.height);
        h.u16(p.end_jump_loop_count);
        h.string(&p.end_jump_title);
        h.u16(p.aliases.len() as u16);
        for a in &p.aliases {
            h.string(a);
        }
        if p.ver >= 2 {
            h.string(p.comment.as_deref().unwrap_or(""));
        }
        w.extract(&h.d);
        for s in &p.seqs {
            w.u8((s.dir + 10) as u8);
            w.extract(&write_seq_inner(s));
            // 帧数在帧之前（PxlSequence.readFromBytes: readUShort 后逐帧）
            w.u16(s.frames.len() as u16);
            for f in &s.frames {
                w.extract(&write_frame_inner(f));
                w.i16(f.layers.len() as i16);
                for l in &f.layers {
                    write_layer(&mut w, l);
                }
            }
        }
        w.u8(0); // 方向序列终止
    }
    w.d
}

fn write_seq_inner(s: &Seq) -> Vec<u8> {
    let mut w = BeW::new();
    w.u8(s.ver);
    w.u16(s.width);
    w.u16(s.height);
    w.i16(s.body_x);
    w.i16(s.body_y);
    w.i16(s.shift_x);
    w.i16(s.shift_y);
    w.i16(s.loop_to);
    w.u16(s.frame_snd.len() as u16);
    for n in &s.frame_snd {
        w.string(n);
    }
    w.d
}

fn write_frame_inner(f: &Frame) -> Vec<u8> {
    let mut w = BeW::new();
    w.u8(f.ver as u8);
    w.i16(f.crf60);
    w.string(&f.name);
    if let Some(t) = f.trailer {
        w.u8(t);
    }
    w.d
}

fn write_layer(w: &mut BeW, l: &Layer) {
    w.u32(l.id);
    w.f64(l.id2);
    w.u8(l.kind as u8);
    w.string(&l.name);
    // 注意：alpha 写回用原始 i16（`raw/100` 是整除，`alpha*100` 会丢精度）
    w.i16(l.raw_alpha);
    if l.group {
        w.u32(l.group_n.unwrap_or(0));
        return;
    }
    w.i16((l.x * 10.0).round() as i16);
    w.i16((l.y * 10.0).round() as i16);
    w.f64(l.zmx);
    w.f64(l.zmy);
    w.f64(l.rot_r);
    w.u16(l.blend_variable);
    w.u32(l.unk_u32);
    w.u8(l.unk_b1);
    w.u8(l.unk_b2);
}

fn write_pack(atlas: &[PackAtlas]) -> Vec<u8> {
    let mut w = BeW::new();
    w.u32(atlas.len() as u32);
    for a in atlas {
        w.u8(a.raw_type as u8);
        w.u8(a.flags);
        w.u8(a.margin);
        w.u32(a.uvs.len() as u32);
        for uv in &a.uvs {
            w.u32(uv.id);
            w.f64(uv.id2);
            w.u32(uv.x);
            w.u32(uv.y);
            w.u32(uv.w);
            w.u32(uv.h);
        }
        if a.flags & 1 == 1 {
            let (ew, eh) = a.external_wh.unwrap_or((0, 0));
            w.u32(ew);
            w.u32(eh);
        } else {
            let empty: Vec<u8> = Vec::new();
            let png = a.embedded_png.as_ref().unwrap_or(&empty);
            w.u32(png.len() as u32);
            w.raw(png);
        }
    }
    w.d
}

fn write_imgs(imgs: &[EmbeddedImg]) -> Vec<u8> {
    let mut w = BeW::new();
    w.u32(imgs.len() as u32);
    for e in imgs {
        w.u8(e.raw_type as u8);
        w.u8(e.flags);
        w.u32(e.id);
        w.f64(e.id2);
        if e.raw_type - 22 == 0 || e.raw_type - 22 == 8 {
            for slot in 0..2 {
                let empty: Vec<u8> = Vec::new();
                let png = if slot == 0 {
                    e.i_png.as_ref().unwrap_or(&empty)
                } else {
                    e.p_png.as_ref().unwrap_or(&empty)
                };
                w.u32(png.len() as u32);
                w.raw(png);
            }
        }
    }
    w.d
}

// ─────────────────────────────────────────────────────────────
// 组层树重建（PxlGroupContainer.fineLinks 语义）
// ─────────────────────────────────────────────────────────────

/// 重建一帧的组层树。规则（照 fineLinks 反推）：type==8 的组层带 preserve_contain_layers=N，
/// 认领**文件序里它前方**的 N 个条目作为子层；嵌套组条目消耗其整个子树足迹。
/// 返回嵌套 JSON 数组（文件序）；每个组节点带 `children`。
pub fn layer_tree(frame: &Frame) -> serde_json::Value {
    let mut out = Vec::new();
    let mut j = frame.layers.len();
    while j > 0 {
        let (node, used) = build_subtree(&frame.layers, j);
        out.push(node);
        j -= used;
    }
    out.reverse();
    serde_json::Value::Array(out)
}

/// 从 `end-1`（含）向前构建一个顶层条目；返回 (节点, 消耗的条目数)
fn build_subtree(layers: &[Layer], end: usize) -> (serde_json::Value, usize) {
    let idx = end - 1;
    let lay = &layers[idx];
    if let Some(n) = lay.group_n {
        let mut children = Vec::new();
        let mut j = end - 1;
        let mut consumed = 1usize;
        for _ in 0..n {
            if j == 0 {
                break;
            }
            let (node, used) = build_subtree(layers, j);
            children.push(node);
            j -= used;
            consumed += used;
        }
        children.reverse(); // 文件序呈现（游戏内部子层表是倒序收的）
        let mut obj = serde_json::to_value(lay).unwrap_or(serde_json::json!({"index": idx}));
        obj["children"] = serde_json::Value::Array(children);
        (obj, consumed)
    } else {
        (serde_json::to_value(lay).unwrap_or(serde_json::json!({"index": idx})), 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = "D:/gal/aic-winlator/game-clean/AliceInCradle/AliceInCradle_Data/StreamingAssets";

    fn collect_pxls(root: &str) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                if n.ends_with(".pxls.dat") || n.ends_with(".pxls.bytes") || n.ends_with(".pxls") {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    /// 结构化重写必须逐字节还原（PACK/IMGS/POSE 重写 + IMGV/PTCL/尾部透传）
    #[test]
    fn serialize_is_byte_identical_on_real_tables() {
        let files = collect_pxls(STREAM);
        if files.is_empty() {
            return; // 真值不在（CI）时跳过
        }
        let mut n = 0;
        for f in &files {
            let raw = std::fs::read(f).unwrap();
            let Ok((_src, body)) = crate::unityfs::extract_pxls(&raw) else { continue };
            let p = match parse(&body) {
                Ok(p) => p,
                Err(e) => panic!("{}: parse {e}", f.display()),
            };
            let back = p.serialize();
            assert_eq!(back.len(), body.len(), "{}: length {} != {}", f.display(), back.len(), body.len());
            if let Some(i) = back.iter().zip(body.iter()).position(|(a, b)| a != b) {
                panic!(
                    "{}: byte mismatch at {i} (want {:02x} got {:02x})",
                    f.display(),
                    body[i],
                    back[i]
                );
            }
            n += 1;
        }
        eprintln!("round-tripped {n} pxls tables byte-identically");
        assert!(n > 0);
    }

    /// 组层树：组节点认领前方 N 层，剩余层留在顶层
    #[test]
    fn layer_tree_groups_claim_preceding_layers() {
        let p = parse(&build_fixture_with_group()).unwrap();
        let tree = layer_tree(&p.poses[0].seqs[0].frames[0]);
        let arr = tree.as_array().unwrap();
        assert_eq!(arr.len(), 2, "顶层应是 [solo, grp]");
        assert_eq!(arr[1]["name"], "grp");
        assert_eq!(arr[1]["children"].as_array().unwrap().len(), 1);
        assert_eq!(arr[1]["children"][0]["name"], "body");
    }

    /// 造一个含 1 个方向、1 帧、3 层（普通 solo / 普通 body / 组 grp 认领 1 层）的最小 pxls
    fn build_fixture_with_group() -> Vec<u8> {
        let mut poses = BeW::new();
        poses.u32(1);
        let mut h = BeW::new();
        h.u8(1); // ver
        h.bool(false);
        h.bool(false);
        h.string("p");
        h.u16(64);
        h.u16(64);
        h.u16(0);
        h.string("");
        h.u16(0);
        poses.extract(&h.d);
        poses.u8(10); // dir 0
        let mut sq = BeW::new();
        sq.u8(0);
        sq.u16(64);
        sq.u16(64);
        sq.i16(0);
        sq.i16(0);
        sq.i16(0);
        sq.i16(0);
        sq.i16(0);
        sq.u16(0);
        poses.extract(&sq.d);
        poses.u16(1); // nframes（在帧之前）
        // frame 1
        let mut fr = BeW::new();
        fr.u8(1);
        fr.i16(1);
        fr.string("f0");
        poses.extract(&fr.d);
        poses.i16(3);
        for (id, name) in [(1u32, "solo"), (2, "body")] {
            poses.u32(id);
            poses.f64(id as f64);
            poses.u8(0);
            poses.string(name);
            poses.i16(10000);
            poses.i16(0);
            poses.i16(0);
            poses.f64(1.0);
            poses.f64(1.0);
            poses.f64(0.0);
            poses.u16(0);
            poses.u32(0);
            poses.u8(0);
            poses.u8(0);
        }
        // layer 2：组，认领 1 层
        poses.u32(3);
        poses.f64(3.0);
        poses.u8(8);
        poses.string("grp");
        poses.i16(10000);
        poses.u32(1);
        poses.u8(0); // 方向序列终止
        let mut out = BeW::new();
        out.u32(MAGIC1);
        out.raw(MAGIC2);
        out.raw(SECTION_POSE.as_bytes());
        out.u32(poses.d.len() as u32);
        out.raw(&poses.d);
        out.d
    }
}
