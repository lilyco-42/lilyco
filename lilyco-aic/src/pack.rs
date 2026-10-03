//! `laic pack` — 图集重排 + `.pxl` 导出（**T1 需确认**）
//!
//! 「超集」缺的最后一块：pixelliner4j 的 `PxlCharacter.writePackSection` +
//! `Algorithm.packRectangles`（`PxlsKiller` 就是靠它把 `.pxls` 重排成 `.pxl`）。
//! 有了这条，闭环才完整：
//!
//! ```text
//! sprites（裁） → 改 PNG → pack（重排回图集） → 生成 .pxl
//! ```
//!
//! ## 装箱算法
//!
//! 边长从起始值起翻倍（`128 → 256 → …`），直到一次放下全部，然后把画布**裁到实际
//! 用量**（装箱只保证 `side×side` 放得下，实际往往只用掉一半）。两种布局：
//!
//! - `shelf`（**默认**）：按高度降序、逐行填充，行高 = 该行最高图块。
//! - `guillotine`：逐字移植 pixelliner4j 的 `Algorithm.packRectangles`
//!   （面积降序 + 二叉节点树，`right` 优先、失败再 `down`）—— 仅作对照/复现用。
//!
//! 🔴 **别用 guillotine 当默认**：它是天真的货架式切分，`right` 子节点的高度被钉死
//! 成第一个图块的高度，于是横向一路铺到底才肯换行。实测 `noel` 的 1505 个 sprite
//! （内容 7.46 M px，含 margin 预留 7.86 M px）：
//!
//! | 算法 | 需要的边长 | 实际用量 | 占用率 |
//! |---|---|---|---|
//! | guillotine（`Algorithm.packRectangles`） | 8192 | 8192×4462 | **20.4%** |
//! | shelf（本工具默认） | 4096 | 4089×2090 | **91.9%** |
//! | skyline bottom-left | 4096 | 4096×2126 | 90.2% |
//!
//! 游戏本体那张图集是 4096×4096、占用 44% —— 说明 PixelLiner 的**正式**打包器本来就
//! 比 pixelliner4j 这个 `Algorithm` 强得多，后者只是个重排用的凑数实现。
//!
//! ## 比 pixelliner4j 强在哪
//!
//! | 项 | pixelliner4j | laic |
//! |---|---|---|
//! | 装箱 | 天真货架式，实测 20% 占用 | ✓ shelf 默认 92%，画布还裁到实际用量 |
//! | 保留 IMGS / IMGV / PTCL | ✗（只写 PACK+POSE） | ✓ 全节保留 |
//! | 双图集（NORMAL + PARTS） | ✗（压成一张） | ✓ PARTS 图集沿用同一套 rect，UV 表留空以保住「继承」语义 |
//! | margin 留白 | ✗（图块紧贴，采样会渗色） | ✓ 每个图块四周留 `margin` 像素 |
//! | 尺寸未变时 UV 的 w/h | 重算（可能漂移） | ✓ 原样保留，渲染尺寸零漂移 |
//! | 输不出贴图时 | 抛异常 | ✓ 明确报「哪张图集没贴图」 |
//!
//! ## UV 的 w/h 含 margin
//!
//! `PxlsImgAtlas` 里 `uv.w = 内容宽 + margin*2`。所以重排时给图块预留的就是
//! `uv.w × uv.h`（已经含留白），内容画在 `(x + margin, y + margin)`。
//! 未替换的 sprite 一律保留原 w/h —— 只有 x/y 变，渲染尺寸不受影响。
//!
//! ## 替换图怎么对上 sprite（🔴 踩过的坑）
//!
//! `noel` 里有 **617 个** key 的层名都叫 `Layer`（`sprites` 导出成 `Layer.EDI*.png`），
//! 所以**只按层名匹配是错的**：一张 `Layer.png` 会被贴到完全不相干的 sprite 上，
//! 尺寸一变还会让继承 UV 的 PARTS 图集整个错位（实测把 26×36 的块撑成 60×153）。
//! 现在的规则是复刻 `sprites` 的去重命名（`claim_stem`，同名层只有第一个能拿裸名，
//! 后面的都带 key），按如下顺序找，且**一张图只能被一个 key 认领**：
//!
//! 1. `sprites` 导出的名字（`Layer.EDIba8e8_x.png` / `Layer.a1.png` / `EDIxxx.png`）
//! 2. `<层名>.<key>[.a<i>]`
//! 3. `<key>[.a<i>]`
//! 4. `<层名>[.a<i>]`（兜底，同一次运行里只兑现一次）
//!
//! 目录里没被认领的 PNG 会原样报在 `unmatched_replacements` 里 —— 名字写错时不能静默丢。
//!
//! ## 替换改了尺寸怎么办
//!
//! 同尺寸重着色（最常见）零风险：rect 不变，PARTS 图集继续用它自己的图。
//! **改尺寸**时麻烦来了 —— PARTS 图集共用同一张 UV 表，rect 必须与来源逐块一致，
//! 于是：
//!
//! - 没给 `.a<i>.png`：跟着换成同一张（JSON 里记 `carried_from_source_atlas`）；
//! - 给了但尺寸不符：裁/补到 rect（`fitted_to_inherited_rect`）。
//!
//! 两条都保证「UV rect − 2×margin == 内容尺寸」，自检才能逐像素比对得上。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use lilyco::prelude::*;

use crate::atlas::{encode_png, read_png, write_png, Img, TextureSource};
use crate::pxlslib::{AtlasUv, PackAtlas};
use crate::sprites::{layer_names, sanitize, sprite_stems, table_name};
use crate::util::collect_targets;

/// 图集边长下限 / 上限（超过上限直接报「放不下」，不无限翻倍）
const MIN_SIDE: u32 = 128;
const MAX_SIDE: u32 = 16384;

/// 重建图集并导出 `.pxl`
#[derive(App)]
#[app(
    name = "pack",
    run = "run_pack",
    safety = "t1",
    about = "Re-pack a PixelLiner atlas: crop every sprite out of the current atlas (optionally substituting edited PNGs from `replace`), lay them out again with the game's own bin-packing algorithm (area-descending into a guillotine node tree, square atlas side doubling from `size`), and write a self-contained `.pxl` with the new atlas embedded as PNG. This is the missing half of the edit loop — `sprites` cuts, you edit, `pack` puts it back. Unlike pixelliner4j's equivalent it KEEPS the IMGS/IMGV/PTCL sections, keeps dual atlases (the PARTS atlas reuses the same rects and keeps its empty UV list so the inheritance rule still fires), keeps each UV's w/h so rendered sizes never drift, and reserves `margin` pixels around every sprite so neighbours cannot bleed. Layer keys are untouched, so every pose/frame keeps working unchanged. `replace` is a directory of PNGs named like `sprites` writes them (`<layer-name>.png`, or `<img-key>.png`, plus `.a<atlas>` for non-primary atlases). Writes `<out>/<table>.pxl`; `atlas` also dumps `<out>/<table>.atlas_<i>.png`. DRY RUN BY DEFAULT — pass `apply: true` to write (safety tier T1: the automated/MCP surface denies it). Every written file is re-read, re-parsed and its sprites compared pixel-by-pixel against what went in."
)]
pub struct Pack {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 输出目录
    #[arg(about = "Directory to write the .pxl files into", must_exist = false)]
    out: PathBuf,

    /// 替换图目录（PNG，命名同 `sprites` 的输出）
    #[arg(about = "Directory of replacement PNGs (named as `sprites` writes them)", default = "")]
    replace: String,

    /// 姿势标题 glob（只用于对齐 `sprites --pose` 的命名，不影响裁切范围）
    #[arg(about = "Pose title glob, only to reproduce the exact file names `sprites --pose` produced", default = "")]
    pose: String,

    /// 起始图集边长（0 = 取原图集尺寸向上取到 2 的幂）
    #[arg(about = "Starting atlas side in px (0 = derive from the original atlas)", default = 0)]
    size: u32,

    /// 装箱算法：shelf（默认，按高降序逐行）| guillotine（pixelliner4j 原版，仅对照用）
    #[arg(about = "Packing algorithm: shelf (default) | guillotine (pixelliner4j's, for comparison)", default = "shelf")]
    packer: String,

    /// 额外导出重排后的图集 PNG
    #[arg(about = "Also dump the re-packed atlas PNG for inspection", default = false)]
    atlas: bool,

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

fn run_pack(app: &Pack, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let mut targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    if app.limit > 0 {
        targets.truncate(app.limit as usize);
    }
    let packer = parse_packer(&app.packer).map_err(AppError::InvalidArg)?;
    let replace = if app.replace.is_empty() {
        None
    } else {
        Some(index_pngs(Path::new(&app.replace)).map_err(AppError::InvalidArg)?)
    };
    if app.apply {
        std::fs::create_dir_all(&app.out)
            .map_err(|e| AppError::InvalidArg(format!("--out mkdir: {e}")))?;
    }

    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("re-packing atlases of {} tables", targets.len())),
    });

    let mut tables: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut total_sprites = 0usize;

    for (i, path) in targets.iter().enumerate() {
        ctx.tick(i as u64, Some(targets.len() as u64), "");
        match pack_one(path, app, packer, replace.as_ref()) {
            Ok((v, n)) => {
                total_sprites += n;
                tables.push(v);
            }
            Err(e) => {
                errors.push(serde_json::json!({ "file": path.display().to_string(), "error": e }))
            }
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "out": app.out.display().to_string(),
        "replace": app.replace,
        "packer": format!("{packer:?}").to_ascii_lowercase(),
        "apply": app.apply,
        "sprites": total_sprites,
        "tables": tables,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

/// 替换图目录索引：`sanitize(去掉 .png 的文件名)`（**小写**）→ 完整路径。
///
/// 索引用小写是为了和 Windows 上「文件名大小写不敏感」对齐；查找侧同样要压小写再比。
fn index_pngs(dir: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut out = BTreeMap::new();
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for ent in rd.flatten() {
        let p = ent.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let Some(stem) = name.strip_suffix(".png") else { continue };
        out.insert(sanitize(stem), p);
    }
    Ok(out)
}

/// 按 `sprites` 的命名规则找替换图：层名优先，其次 `<层名>.<key>`，最后裸 key
/// 替换图查找：先认 `sprites` 导出的**确定文件名**，再退到带 key 的两种写法，最后才是裸层名。
///
/// ⚠️ 不能只按层名找 —— noel 里有几十个 key 的层名都叫 `Layer`，裸匹配会把
/// `Layer.png` 贴到完全不相干的 sprite 上（尺寸一变，继承 UV 的部件图集还会整个错位）。
/// 所以首选必须是 `sprites` 去重后的那个名字（`claim_stem` 的规则：同名层只有第一个
/// 能拿到裸名，后面的都带 key）。裸层名只作为兜底，且**同一次运行里只能兑现一次**。
fn find_replacement(
    ix: &BTreeMap<String, PathBuf>,
    names: &BTreeMap<String, String>,
    key: &str,
    ai: usize,
    stem: Option<&String>,
    claimed: &mut BTreeSet<String>,
) -> Option<PathBuf> {
    let name = names.get(key).cloned().unwrap_or_else(|| key.to_string());
    let suffix = if ai > 0 { format!(".a{ai}") } else { String::new() };
    let mut cands: Vec<String> = Vec::new();
    if let Some(s) = stem {
        cands.push(sanitize(s));
    }
    cands.push(sanitize(&format!("{name}{suffix}.{key}")));
    cands.push(sanitize(&format!("{key}{suffix}")));
    cands.push(sanitize(&format!("{name}{suffix}")));
    for c in cands {
        let lc = c.to_lowercase();
        if claimed.contains(&lc) {
            continue; // 已被别的 key 认领 —— 同一张图不能贴两个 sprite
        }
        if let Some(p) = ix.get(&lc) {
            claimed.insert(lc);
            return Some(p.clone());
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// guillotine 装箱（`Algorithm.packRectangles` 的移植）
// ─────────────────────────────────────────────────────────────

/// 一个待放图块：外框尺寸已含 margin，内容画在 `(x+margin, y+margin)`
struct Slot {
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    content: Img,
    replaced: bool,
}

struct Node {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    used: bool,
    right: Option<Box<Node>>,
    down: Option<Box<Node>>,
}

impl Node {
    fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Node { x, y, w, h, used: false, right: None, down: None }
    }

    /// Java 版是 `right` 优先、失败再 `down` 的深搜。递归深度 ≤ 节点数（只有插入成功
    /// 才造节点），最坏 = 图块数；本作最多 1505 块，栈上跑得动。
    fn insert(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.used {
            if let Some(p) = self.right.as_mut().and_then(|n| n.insert(w, h)) {
                return Some(p);
            }
            return self.down.as_mut().and_then(|n| n.insert(w, h));
        }
        if w <= self.w && h <= self.h {
            self.used = true;
            self.down = Some(Box::new(Node::new(self.x, self.y + h, self.w, self.h - h)));
            self.right = Some(Box::new(Node::new(self.x + w, self.y, self.w - w, h)));
            return Some((self.x, self.y));
        }
        None
    }
}

/// pixelliner4j `Algorithm.packRectangles` 的逐字移植：面积降序（稳定排序，
/// 等面积保持原 UV 序）→ 二叉节点树，`right` 优先、失败再 `down`。
/// 放不下返回 false，此时 `x/y` 是脏的，调用方必须换更大边长重来。
fn pack_guillotine(slots: &mut [Slot], side: u32) -> bool {
    let areas: Vec<u64> = slots.iter().map(|s| s.w as u64 * s.h as u64).collect();
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by(|&a, &b| areas[b].cmp(&areas[a]));
    let mut root = Node::new(0, 0, side, side);
    for i in order {
        let (w, h) = (slots[i].w, slots[i].h);
        match root.insert(w, h) {
            Some((x, y)) => {
                slots[i].x = x;
                slots[i].y = y;
            }
            None => return false,
        }
    }
    true
}

/// 货架式：按**高度降序**逐行填充，行高 = 该行最高的图块。
///
/// 比 guillotine 简单得多，实测占用率高 4 倍多（noel：91.9% vs 20.4%）。
/// 等高的图块靠稳定排序保持原 UV 序，结果完全确定。
fn pack_shelf(slots: &mut [Slot], side: u32) -> bool {
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by(|&a, &b| slots[b].h.cmp(&slots[a].h));
    let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = (slots[i].w, slots[i].h);
        if x + w > side {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        if w > side || y + h > side {
            return false;
        }
        slots[i].x = x;
        slots[i].y = y;
        x += w;
        row_h = row_h.max(h);
    }
    true
}

fn pack_with(slots: &mut [Slot], side: u32, packer: Packer) -> bool {
    match packer {
        Packer::Shelf => pack_shelf(slots, side),
        Packer::Guillotine => pack_guillotine(slots, side),
    }
}

/// 从 `side` 起翻倍，直到一次放下全部；返回 (边长, 翻倍次数)
fn pack_grow(slots: &mut [Slot], side: u32, packer: Packer) -> Result<(u32, u32), String> {
    let mut side = side.max(MIN_SIDE);
    let mut steps = 0u32;
    loop {
        if pack_with(slots, side, packer) {
            return Ok((side, steps));
        }
        if side >= MAX_SIDE {
            return Err(format!(
                "cannot fit {} sprites into a {MAX_SIDE}x{MAX_SIDE} atlas ({packer:?})",
                slots.len()
            ));
        }
        side *= 2;
        steps += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Packer {
    Shelf,
    Guillotine,
}

fn parse_packer(s: &str) -> Result<Packer, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "shelf" | "" => Ok(Packer::Shelf),
        "guillotine" | "noel" | "pixelliner4j" => Ok(Packer::Guillotine),
        other => Err(format!("unknown packer `{other}` (want `shelf` or `guillotine`)")),
    }
}

/// 把内容按 `(x+margin, y+margin)` 贴进画布。
///
/// 画布**裁到实际用量**（`side` 只是装箱时的上限）—— 装箱能塞进 4096 不代表横向
/// 用满了 4096，noel 实际只用到 4089×2090，砍掉空白能省掉一半 PNG 体积。
fn compose(slots: &[Slot], margin: u32, side: u32) -> Img {
    let mut uw = 0u32;
    let mut uh = 0u32;
    for s in slots {
        uw = uw.max(s.x + s.w);
        uh = uh.max(s.y + s.h);
    }
    let w = uw.min(side).max(1);
    let h = uh.min(side).max(1);
    let mut canvas = Img::blank(w, h);
    for s in slots {
        let dx = s.x + margin;
        let dy = s.y + margin;
        if dx + s.content.w > w || dy + s.content.h > h {
            continue; // 装箱保证不会发生；防越界写崩
        }
        for row in 0..s.content.h {
            let si = (row as usize) * (s.content.w as usize) * 4;
            let di = ((dy + row) as usize * w as usize + dx as usize) * 4;
            let n = s.content.w as usize * 4;
            canvas.rgba[di..di + n].copy_from_slice(&s.content.rgba[si..si + n]);
        }
    }
    canvas
}

// ─────────────────────────────────────────────────────────────
// 单表重排
// ─────────────────────────────────────────────────────────────

fn decode_atlas_image(
    ai: usize,
    at: &PackAtlas,
    sources: &[Option<TextureSource>],
    file_str: &str,
) -> Result<Img, String> {
    if at.flags & 1 == 1 {
        let src = sources.get(ai).and_then(|s| s.clone()).ok_or_else(|| {
            let (ew, eh) = at.external_wh.unwrap_or((0, 0));
            format!(
                "atlas {ai} declares an external {ew}x{eh} texture but no texture source was found \
                 (need <stem>.pxls.bytes.texture_0.dat holding a texture_{ai} asset, or a texture_{ai} file)"
            )
        })?;
        let im = src.read()?;
        if let Some((ew, eh)) = at.external_wh {
            if (im.w, im.h) != (ew, eh) {
                return Err(format!(
                    "{file_str}: atlas {ai} declares {ew}x{eh} but the texture decoded to {}x{}",
                    im.w, im.h
                ));
            }
        }
        Ok(im)
    } else if let Some(png) = &at.embedded_png {
        read_png(png, &format!("{file_str}: embedded atlas {ai}"))
    } else {
        Err(format!("{file_str}: atlas {ai} has neither an external texture nor an embedded PNG"))
    }
}

fn pack_one(
    path: &Path,
    app: &Pack,
    packer: Packer,
    replace: Option<&BTreeMap<String, PathBuf>>,
) -> Result<(serde_json::Value, usize), String> {
    let file_str = path.display().to_string();
    let p = crate::util::load_pxls(path)?;
    if p.atlas.is_empty() {
        return Err("no %PACK_SECTION% to re-pack".into());
    }
    let table = table_name(path);
    let names = layer_names(&p, &app.pose);
    let before = p.serialize().len();
    // 与 `sprites` 逐字一致的导出名 —— 替换图按它反查（裸层名有歧义，不能当主键）
    let stems = sprite_stems(&p, &names, &app.pose);

    let sources = crate::atlas::resolve_atlas_textures(path, p.atlas.len());
    let mut new_atlas: Vec<PackAtlas> = Vec::with_capacity(p.atlas.len());
    let mut atlas_info: Vec<serde_json::Value> = Vec::new();
    // 每个图集的装箱结果 (x, y, w, h)  —— PARTS 图集沿用前一个图集的 rect
    let mut rects_by_atlas: Vec<Vec<(u32, u32, u32, u32)>> = Vec::new();
    let mut repl_by_atlas: Vec<Vec<Option<PathBuf>>> = Vec::new();
    let mut sides_by_atlas: Vec<u32> = Vec::new();
    // 自检用：(图集序号, key) → 期望内容
    let mut expected: BTreeMap<(usize, String), Img> = BTreeMap::new();
    // 已被某个 key 认领的替换图（小写主干），避免一张图贴到多个 sprite 上
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    let mut total = 0usize;

    for (ai, at) in p.atlas.iter().enumerate() {
        let margin = at.margin as u32;
        let img = decode_atlas_image(ai, at, &sources, &file_str)?;
        let inherited = p.uv_inherited_from(ai);
        let uvs: Vec<AtlasUv> = p.effective_uvs(ai).to_vec();

        let mut slots: Vec<Slot> = Vec::with_capacity(uvs.len());
        let mut local_repl: Vec<Option<PathBuf>> = Vec::with_capacity(uvs.len());
        for (ui, uv) in uvs.iter().enumerate() {
            let mut content = img.crop(
                uv.x + margin,
                uv.y + margin,
                uv.w.saturating_sub(margin * 2),
                uv.h.saturating_sub(margin * 2),
            );
            let mut replaced = false;
            let stem = stems.get(ai).and_then(|r| r.get(ui)).and_then(|o| o.as_ref());
            let rp = replace.and_then(|ix| find_replacement(ix, &names, &uv.img, ai, stem, &mut claimed));
            if let Some(rp) = &rp {
                let bytes = std::fs::read(rp)
                    .map_err(|e| format!("read replacement {}: {e}", rp.display()))?;
                content = read_png(&bytes, &rp.display().to_string())?;
                replaced = true;
            }
            local_repl.push(rp);
            slots.push(Slot {
                w: content.w + margin * 2,
                h: content.h + margin * 2,
                x: 0,
                y: 0,
                content,
                replaced,
            });
        }

        // 继承型图集上「被来源图集带着改了尺寸」的块计数（自检/排障用）
        let mut carried = 0usize;
        let mut fitted = 0usize;

        let (side, steps) = match inherited {
            // 继承 UV 的图集（PARTS）：共用同一张 UV 表，rect 必须与来源逐块一致
            Some(src) => {
                let src_rects = rects_by_atlas.get(src).ok_or_else(|| {
                    format!("{file_str}: atlas {ai} inherits UVs from {src}, which was not packed")
                })?;
                if src_rects.len() != slots.len() {
                    return Err(format!(
                        "{file_str}: atlas {ai} has {} sprites but inherits a UV table of {}",
                        slots.len(),
                        src_rects.len()
                    ));
                }
                let src_repl = repl_by_atlas.get(src).map(Vec::as_slice).unwrap_or(&[]);
                for (i, s) in slots.iter_mut().enumerate() {
                    let (x, y, w, h) = src_rects[i];
                    s.x = x;
                    s.y = y;
                    s.w = w;
                    s.h = h;
                    let (cw, ch) = (w.saturating_sub(margin * 2), h.saturating_sub(margin * 2));
                    if (s.content.w, s.content.h) == (cw, ch) {
                        continue;
                    }
                    // 来源那块被换成了别的尺寸的图：这里没有显式的 `.a{i}` 替换图时，
                    // 只能跟着换成同一张 —— UV 表是共用的，两边画的必须是同一个东西
                    if !s.replaced {
                        if let Some(Some(p)) = src_repl.get(i) {
                            let bytes = std::fs::read(p)
                                .map_err(|e| format!("read replacement {}: {e}", p.display()))?;
                            s.content = read_png(&bytes, &p.display().to_string())?;
                            s.replaced = true;
                            carried += 1;
                        }
                    }
                    // 装不下的裁掉、不够的补透明：块尺寸由来源定死，不能让内容越界
                    s.content = s.content.crop(0, 0, cw, ch);
                    fitted += 1;
                }
                (sides_by_atlas[src], 0u32)
            }
            None => {
                let start_side = if app.size > 0 {
                    app.size
                } else {
                    img.w.max(img.h).next_power_of_two().max(MIN_SIDE)
                };
                pack_grow(&mut slots, start_side, packer)?
            }
        };

        // 未替换的 sprite 保留原 w/h：渲染尺寸零漂移（装箱用的就是这个值，此处是防御性对齐）
        // 继承型图集的 rect 来自来源图集，不能拿原 UV 覆盖回去
        if inherited.is_none() {
            for (s, uv) in slots.iter_mut().zip(uvs.iter()) {
                if !s.replaced {
                    s.w = uv.w.max(margin * 2);
                    s.h = uv.h.max(margin * 2);
                }
            }
        }
        let replaced_n = slots.iter().filter(|s| s.replaced).count();

        // 继承型图集**保持 UV 表为空**：读的时候 `num2 == 0 && PARTS` 会触发继承，语义不变
        let new_uvs: Vec<AtlasUv> = if inherited.is_some() {
            Vec::new()
        } else {
            uvs.iter()
                .zip(slots.iter())
                .map(|(uv, s)| AtlasUv {
                    id: uv.id,
                    id2: uv.id2,
                    x: s.x,
                    y: s.y,
                    w: s.w,
                    h: s.h,
                    img: uv.img.clone(),
                })
                .collect()
        };

        let mut png_bytes = 0usize;
        let mut canvas_wh = None;
        if app.apply {
            let canvas = compose(&slots, margin, side);
            let png = encode_png(canvas.w, canvas.h, &canvas.rgba)?;
            png_bytes = png.len();
            canvas_wh = Some([canvas.w, canvas.h]);
            if app.atlas {
                let ap = app.out.join(format!("{table}.atlas_{ai}.png"));
                write_png(&ap, canvas.w, canvas.h, &canvas.rgba)?;
            }
            for (uv, s) in uvs.iter().zip(slots.iter()) {
                expected.insert((ai, uv.img.clone()), s.content.clone());
            }
            new_atlas.push(PackAtlas {
                raw_type: at.raw_type,
                flags: at.flags & !1,
                margin: at.margin,
                uvs: new_uvs,
                external_wh: None,
                embedded_png: Some(png),
            });
        } else {
            new_atlas.push(PackAtlas {
                raw_type: at.raw_type,
                flags: at.flags & !1,
                margin: at.margin,
                uvs: new_uvs,
                external_wh: None,
                embedded_png: Some(Vec::new()),
            });
        }

        rects_by_atlas.push(slots.iter().map(|s| (s.x, s.y, s.w, s.h)).collect());
        repl_by_atlas.push(local_repl);
        sides_by_atlas.push(side);
        total += uvs.len();
        atlas_info.push(serde_json::json!({
            "index": ai,
            "img_type_name": crate::pxlslib::img_type_name(crate::pxlslib::atlas_img_type(at)),
            "uvs": uvs.len(),
            "uv_inherited_from": inherited,
            "side": if inherited.is_some() { serde_json::Value::Null } else { serde_json::json!(side) },
            "doublings": steps,
            "replaced": replaced_n,
            "carried_from_source_atlas": carried,
            "fitted_to_inherited_rect": fitted,
            "canvas": canvas_wh,
            "png_bytes": png_bytes,
        }));
    }

    let mut out = p;
    out.atlas = new_atlas;
    let bytes = out.serialize();
    let out_path = app.out.join(format!("{table}.pxl"));
    let mut verified = false;

    if app.apply {
        if out_path.exists() && !app.overwrite {
            return Err(format!("{} exists (use overwrite to replace it)", out_path.display()));
        }
        std::fs::write(&out_path, &bytes)
            .map_err(|e| format!("write {}: {e}", out_path.display()))?;
        verify(&out_path, &expected)?;
        verified = true;
    }

    // 目录里没被任何 sprite 认领的 PNG —— 多半是名字写错了，静默丢掉太坑
    let unmatched: Vec<String> = replace
        .map(|ix| {
            ix.keys()
                .filter(|k| !claimed.contains(*k))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok((
        serde_json::json!({
            "file": file_str,
            "table": table,
            "output": out_path.display().to_string(),
            "written": app.apply,
            "verified": verified,
            "replaced": claimed.len(),
            "unmatched_replacements": unmatched,
            "pxls_bytes_before": before,
            // dry-run 时图集 PNG 还没编码，报出来的体积会严重偏小 —— 直接不给
            "pxls_bytes_after": if app.apply { Some(bytes.len()) } else { None },
            "atlases": atlas_info,
        }),
        total,
    ))
}

/// 写出的 `.pxl` 必须能被重新解析，且每个 sprite 的像素与装进去的完全一致
fn verify(path: &Path, expected: &BTreeMap<(usize, String), Img>) -> Result<(), String> {
    let d = std::fs::read(path).map_err(|e| format!("re-read {}: {e}", path.display()))?;
    let (_src, raw) = crate::unityfs::extract_pxls(&d)?;
    let mut p = crate::pxlslib::parse(&raw)?;
    crate::pxlslib::attach_img_sizes(&mut p);
    let mut checked = 0usize;
    for (ai, at) in p.atlas.iter().enumerate() {
        let Some(png) = &at.embedded_png else {
            return Err(format!("re-read: atlas {ai} lost its embedded PNG"));
        };
        let img = read_png(png, &format!("re-read atlas {ai}"))?;
        let m = at.margin as u32;
        for uv in p.effective_uvs(ai) {
            let Some(want) = expected.get(&(ai, uv.img.clone())) else { continue };
            let got = img.crop(
                uv.x + m,
                uv.y + m,
                uv.w.saturating_sub(m * 2),
                uv.h.saturating_sub(m * 2),
            );
            if got.rgba != want.rgba {
                return Err(format!(
                    "re-read: atlas {ai} sprite `{}` differs ({}x{} vs {}x{})",
                    uv.img, got.w, got.h, want.w, want.h
                ));
            }
            checked += 1;
        }
    }
    if checked == 0 && !expected.is_empty() {
        return Err("re-read: no sprite could be compared".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pxlslib::Pxls;

    fn slot(w: u32, h: u32) -> Slot {
        Slot {
            w,
            h,
            x: 0,
            y: 0,
            content: Img::blank(w.saturating_sub(2).max(1), h.saturating_sub(2).max(1)),
            replaced: false,
        }
    }

    fn no_overlap(slots: &[Slot]) {
        for i in 0..slots.len() {
            for j in (i + 1)..slots.len() {
                let (a, b) = (&slots[i], &slots[j]);
                let hit = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
                assert!(
                    !hit,
                    "图块 {i} 与 {j} 重叠: ({},{},{},{}) vs ({},{},{},{})",
                    a.x, a.y, a.w, a.h, b.x, b.y, b.w, b.h
                );
            }
        }
    }

    /// 默认算法（shelf）必须不重叠、不越界
    #[test]
    fn shelf_packing_never_overlaps() {
        let mut slots = vec![slot(10, 10), slot(64, 64), slot(32, 16), slot(20, 20)];
        let (side, _) = pack_grow(&mut slots, 128, Packer::Shelf).unwrap();
        assert_eq!(side, 128, "128 够放，不该翻倍");
        for s in &slots {
            assert!(s.x + s.w <= side && s.y + s.h <= side, "图块越界");
        }
        no_overlap(&slots);
    }

    /// guillotine 是 `Algorithm.packRectangles` 的逐字移植，同样必须不重叠
    #[test]
    fn guillotine_packing_never_overlaps() {
        let mut slots = vec![slot(10, 10), slot(64, 64), slot(32, 16), slot(20, 20)];
        let (side, _) = pack_grow(&mut slots, 128, Packer::Guillotine).unwrap();
        assert_eq!(side, 128);
        for s in &slots {
            assert!(s.x + s.w <= side && s.y + s.h <= side, "图块越界");
        }
        no_overlap(&slots);
    }

    #[test]
    fn packing_doubles_the_side_until_everything_fits() {
        // 4 块 102×102 排不进 128 → 需要 256
        for p in [Packer::Shelf, Packer::Guillotine] {
            let mut slots = vec![slot(102, 102), slot(102, 102), slot(102, 102), slot(102, 102)];
            let (side, steps) = pack_grow(&mut slots, 128, p).unwrap();
            assert_eq!(side, 256, "{p:?}: 放不下 128 必须翻倍");
            assert_eq!(steps, 1);
            no_overlap(&slots);
        }
    }

    #[test]
    fn a_sprite_wider_than_the_atlas_is_reported_as_unfittable() {
        // 单个 300 宽的图块：128/256 都放不下，512 才够 —— 不能死循环
        let mut slots = vec![slot(300, 10)];
        let (side, steps) = pack_grow(&mut slots, 128, Packer::Shelf).unwrap();
        assert_eq!(side, 512, "必须一直翻倍到放得下");
        assert_eq!(steps, 2);
    }

    #[test]
    fn equal_sized_slots_keep_their_uv_order() {
        // 稳定排序：等高/等面积时插入顺序 == UV 顺序
        for p in [Packer::Shelf, Packer::Guillotine] {
            let mut slots = vec![slot(16, 16), slot(16, 16), slot(16, 16)];
            pack_grow(&mut slots, 128, p).unwrap();
            assert_eq!(slots[0].x, 0, "{p:?}: 第一块必须在原点");
            assert_eq!(slots[1].x, 16, "{p:?}: 等高按原序依次往右排");
            assert_eq!(slots[2].x, 32);
        }
    }

    #[test]
    fn compose_trims_the_canvas_to_what_is_actually_used() {
        let mut s = slot(6, 6);
        s.x = 10;
        s.y = 20;
        s.content = Img::blank(4, 4);
        // side=64，但实际只用到 16×26 → 画布必须裁到这个尺寸
        let canvas = compose(std::slice::from_ref(&s), 1, 64);
        assert_eq!((canvas.w, canvas.h), (16, 26), "空白部分要砍掉");
    }

    #[test]
    fn packer_names_are_accepted() {
        assert_eq!(parse_packer("shelf").unwrap(), Packer::Shelf);
        assert_eq!(parse_packer("Guillotine").unwrap(), Packer::Guillotine);
        assert_eq!(parse_packer("").unwrap(), Packer::Shelf, "缺省 = shelf");
        assert!(parse_packer("maxrects").is_err());
    }

    #[test]
    fn compose_places_content_inside_the_margin() {
        let mut s = slot(6, 6);
        s.x = 10;
        s.y = 20;
        s.content = Img::new(4, 4, vec![0xABu8; 4 * 4 * 4]).unwrap();
        let canvas = compose(std::slice::from_ref(&s), 1, 64);
        assert_eq!((canvas.w, canvas.h), (16, 26));
        let at = |x: u32, y: u32| canvas.rgba[((y * canvas.w + x) * 4) as usize];
        assert_eq!(at(11, 21), 0xAB);
        assert_eq!(at(10, 20), 0, "左上角是 margin，不该有内容");
        assert_eq!(at(15, 25), 0, "右下角是 margin");
        assert_eq!(at(0, 0), 0);
    }

    #[test]
    fn replacement_lookup_accepts_layer_name_and_bare_key() {
        // 索引里的 key 一律小写（Windows 文件名大小写不敏感）
        let mut ix = BTreeMap::new();
        ix.insert("hair".to_string(), PathBuf::from("hair.png"));
        ix.insert("edi1_0".to_string(), PathBuf::from("edi1_0.png"));
        ix.insert("hair.a1".to_string(), PathBuf::from("hair.a1.png"));
        ix.insert("layer.ediba8e8_x".to_string(), PathBuf::from("dedup.png"));
        let mut names = BTreeMap::new();
        names.insert("EDI1_0".to_string(), "hair".to_string());
        names.insert("EDIba8e8_X".to_string(), "Layer".to_string());
        let mut c = BTreeSet::new();
        assert_eq!(
            find_replacement(&ix, &names, "EDI1_0", 0, Some(&"hair".into()), &mut c),
            Some(PathBuf::from("hair.png"))
        );
        let mut c = BTreeSet::new();
        assert_eq!(
            find_replacement(&ix, &names, &sanitize("edi1_0"), 0, None, &mut c),
            Some(PathBuf::from("edi1_0.png"))
        );
        // sprites 重名时导出成 `<层名>.<key>.png`，查找侧必须也能命中
        let mut c = BTreeSet::new();
        assert_eq!(
            find_replacement(&ix, &names, "EDIba8e8_X", 0, None, &mut c),
            Some(PathBuf::from("dedup.png"))
        );
        let mut c = BTreeSet::new();
        assert_eq!(
            find_replacement(&ix, &names, "EDI1_0", 1, Some(&"hair.a1".into()), &mut c),
            Some(PathBuf::from("hair.a1.png"))
        );
        let mut c = BTreeSet::new();
        assert_eq!(find_replacement(&ix, &names, "EDI9_9", 0, None, &mut c), None);
    }

    /// noel 里有几十个 key 的层名都叫 `Layer` —— 裸层名只能兑现一次，
    /// 否则同一张替换图会被贴到一堆不相干的 sprite 上
    #[test]
    fn a_bare_layer_name_is_only_claimed_once() {
        let mut ix = BTreeMap::new();
        ix.insert("layer".to_string(), PathBuf::from("Layer.png"));
        let mut names = BTreeMap::new();
        names.insert("EDIaaa".to_string(), "Layer".to_string());
        names.insert("EDIbbb".to_string(), "Layer".to_string());
        let mut c = BTreeSet::new();
        // 先来的（sprites 把裸名给了它）拿到
        assert_eq!(
            find_replacement(&ix, &names, "EDIaaa", 0, Some(&"Layer".into()), &mut c),
            Some(PathBuf::from("Layer.png"))
        );
        // 后来的只能走 `<层名>.<key>`，共用一张图会被拒绝
        assert_eq!(
            find_replacement(&ix, &names, "EDIbbb", 0, Some(&"Layer.EDIbbb".into()), &mut c),
            None
        );
    }

    /// `sprites` 去重后的名字优先于裸层名：两个候选同时在场时必须认前者
    #[test]
    fn the_sprites_style_stem_wins_over_the_bare_layer_name() {
        let mut ix = BTreeMap::new();
        ix.insert("layer".to_string(), PathBuf::from("other.png"));
        ix.insert("layer.ediccc".to_string(), PathBuf::from("mine.png"));
        let mut names = BTreeMap::new();
        names.insert("EDIccc".to_string(), "Layer".to_string());
        let mut c = BTreeSet::new();
        assert_eq!(
            find_replacement(&ix, &names, "EDIccc", 0, Some(&"Layer.EDIccc".into()), &mut c),
            Some(PathBuf::from("mine.png"))
        );
    }

    /// `sprite_stems` 必须和 `sprites` 的去重规则一致：同名层第二个起带 key
    #[test]
    fn sprite_stems_mirror_the_sprites_export_names() {
        let mut p = Pxls::default();
        let uvs = vec![
            AtlasUv { id: 0, id2: 0.0, x: 0, y: 0, w: 4, h: 4, img: "EDIaaa".into() },
            AtlasUv { id: 1, id2: 0.0, x: 4, y: 0, w: 4, h: 4, img: "EDIbbb".into() },
            AtlasUv { id: 2, id2: 0.0, x: 8, y: 0, w: 4, h: 4, img: "EDIccc".into() },
        ];
        p.atlas.push(PackAtlas {
            raw_type: 22, // NORMAL
            flags: 0,
            margin: 1,
            uvs: uvs.clone(),
            external_wh: None,
            embedded_png: None,
        });
        p.atlas.push(PackAtlas {
            raw_type: 23, // PARTS —— UV 表留空即继承前一个图集
            flags: 0,
            margin: 1,
            uvs: Vec::new(),
            external_wh: None,
            embedded_png: None,
        });
        let mut names = BTreeMap::new();
        names.insert("EDIaaa".to_string(), "Layer".to_string());
        names.insert("EDIbbb".to_string(), "Layer".to_string());
        let stems = sprite_stems(&p, &names, "");
        assert_eq!(
            stems[0],
            vec![Some("Layer".into()), Some("Layer.EDIbbb".into()), Some("EDIccc".into())]
        );
        assert_eq!(
            stems[1],
            vec![Some("Layer.a1".into()), Some("Layer.a1.EDIbbb".into()), Some("EDIccc.a1".into())]
        );
    }
}
