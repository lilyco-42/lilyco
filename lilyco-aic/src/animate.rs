//! `laic animate` — 可步进的动画状态机（T0 只读）
//!
//! 逐字移植 pixelliner4j 的 `FrameAnimator`：拿到一段（姿势 × 方向）序列后，按
//! **1/60 秒的 tick** 推进，回答「这段动画一个循环多少帧 / 多少 tick / 从哪帧回卷」。
//! `render` 是「把每一帧都画出来」，这个是「把播放头推到某处」——mod 对动画时序时
//! 要的就是后者的数字。
//!
//! ## 状态机语义（照抄 `FrameAnimator`）
//!
//! ```text
//! step():      stepped += 1
//!              if stepped >= frame.crf60 { stepped = 0; position += 1
//!                  if position >= frame_count { position = loop_to; looped_count += 1 } }
//! stepFrame(): stepped = 0; position += 1; if position >= frame_count { position = loop_to; looped_count += 1 }
//! changeFrame(name): stepped = 0; position = 首个 name 相等的帧（不区分大小写）
//! reset():     position = stepped = looped_count = 0
//! ```
//!
//! `crf60` 是以 1/60 秒为单位的**帧时长**（`PxlFrame` 的字段就是这么叫的）：
//! 10 表示这一帧停 10 个 tick ≈ 1/6 秒。所以一循环的总时长是 `loop_to..end` 各帧
//! crf60 之和，而不是「帧数 × 某个常数」。
//!
//! ⚠️ 原版 `loopTo` 越界时 `sequence.getFrame(position)` 会直接越界崩；这里**钳到 0**
//! 并在输出里标 `loop_to_clamped`，免得一张坏表把工具带崩。

use std::path::PathBuf;
use std::time::Instant;

use lilyco::prelude::*;

use crate::pxlslib::{Frame, Seq};
use crate::render::{build_atlas_set, render_frame};
use crate::sprites::{sanitize, table_name};
use crate::util::{collect_targets, glob_match};

/// 逐帧推进播放头
#[derive(App)]
#[app(
    name = "animate",
    run = "run_animate",
    about = "Drive a PixelLiner pose sequence as a steppable animation state machine — the runtime half that `render` does not expose. Mirrors pixelliner4j's FrameAnimator exactly: each frame lasts `crf60` ticks of 1/60 s, `step` accumulates ticks and advances when the budget is spent, `stepFrame` jumps a whole frame, `changeFrame(name)` seeks by frame name (case-insensitive), and running past the end wraps to `loop_to` and increments `looped_count`. Answers the modding question \"how many ticks is one loop of this animation, and which frame does it return to\". Select the sequence with `pose` (title glob) + `dir` (0..=7); seek with `frame` (name) or `index`; then advance with `ticks` (tick-accurate) and/or `frames` (whole frames). `out` renders the frame the playhead landed on. Read-only (safety T0)."
)]
pub struct Animate {
    /// pxls 文件或目录
    #[arg(about = "A .pxls file, or a directory scanned recursively for pxls tables", must_exist = true)]
    root: PathBuf,

    /// 姿势标题 glob（不区分大小写）
    #[arg(about = "Pose title glob filter (case-insensitive)", default = "*")]
    pose: String,

    /// 只取指定方向（0..=7）
    #[arg(about = "Only direction N (0..=7; omit for all)", default = 99)]
    dir: u32,

    /// 起始帧序号（每个方向内 0 起）
    #[arg(about = "Seek to frame index N before advancing (0-based)", default = 99)]
    index: u32,

    /// 按帧名定位（不区分大小写；优先于 index）
    #[arg(about = "Seek to the frame named N (case-insensitive; takes precedence over index)", default = "")]
    frame: String,

    /// 推进 N 个 tick（1/60 秒；按 crf60 累加）
    #[arg(about = "Advance N ticks of 1/60 s (accumulates against each frame's crf60)", default = 0)]
    ticks: u64,

    /// 额外推进 N 个整帧（忽略 crf60）
    #[arg(about = "Additionally jump N whole frames (ignores crf60)", default = 0)]
    frames: u64,

    /// 渲染播放头所在的那一帧到该目录（空 = 不渲染）
    #[arg(about = "Directory to render the frame the playhead landed on into (empty = no render)", default = "")]
    out: String,

    /// 渲染时的整数倍放大
    #[arg(about = "Integer upscale factor when rendering", default = 1)]
    scale: u32,

    /// 显式指定 atlas 0 的贴图文件
    #[arg(about = "Explicit texture file for atlas 0 (overrides auto pairing)", default = "")]
    texture: String,

    /// 最多处理的文件数（0 = 不限）
    #[arg(about = "Cap the number of tables processed (0 = unlimited)", default = 0)]
    limit: u64,
}

/// `FrameAnimator` 的移植
struct Animator<'a> {
    seq: &'a Seq,
    position: usize,
    stepped: i64,
    looped: i64,
    /// `loop_to` 越界被钳过（原版会直接越界崩）
    clamped: bool,
}

impl<'a> Animator<'a> {
    fn new(seq: &'a Seq) -> Self {
        Animator { seq, position: 0, stepped: 0, looped: 0, clamped: false }
    }

    fn frame_count(&self) -> usize {
        self.seq.frames.len()
    }

    /// 切到另一段序列（对应 Java 的 `setSequence`）：状态全部归零
    fn set_sequence(&mut self, seq: &'a Seq) {
        self.seq = seq;
        self.position = 0;
        self.stepped = 0;
        self.looped = 0;
        self.clamped = false;
    }

    /// 走到序列末尾时的回卷：`position = loop_to`（越界则钳到 0）
    fn wrap(&mut self) {
        let n = self.frame_count();
        let raw = self.seq.loop_to as i64;
        if raw < 0 || raw >= n as i64 {
            self.position = 0;
            self.clamped = true;
        } else {
            self.position = raw as usize;
        }
        self.looped += 1;
    }

    fn advance(&mut self) {
        self.position += 1;
        if self.position >= self.frame_count() {
            self.wrap();
        }
    }

    fn step(&mut self) {
        if self.frame_count() == 0 {
            return;
        }
        self.stepped += 1;
        if self.stepped >= self.seq.frames[self.position].crf60 as i64 {
            self.stepped = 0;
            self.advance();
        }
    }

    fn step_frame(&mut self) {
        if self.frame_count() == 0 {
            return;
        }
        self.stepped = 0;
        self.advance();
    }

    fn change_frame(&mut self, name: &str) -> Result<(), String> {
        let pos = self
            .seq
            .frames
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no frame named `{name}` in this sequence"))?;
        self.stepped = 0;
        self.position = pos;
        Ok(())
    }

    fn current(&self) -> Option<&'a Frame> {
        self.seq.frames.get(self.position)
    }
}

/// 一循环的总 tick 数：`loop_to..end` 各帧 crf60 之和
fn ticks_per_loop(seq: &Seq) -> i64 {
    let n = seq.frames.len();
    let from = if seq.loop_to < 0 || seq.loop_to as usize >= n { 0 } else { seq.loop_to as usize };
    seq.frames[from..].iter().map(|f| f.crf60 as i64).sum()
}

fn run_animate(app: &Animate, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let start = Instant::now();
    let mut targets = collect_targets(&app.root).map_err(AppError::InvalidArg)?;
    if app.limit > 0 {
        targets.truncate(app.limit as usize);
    }
    let render_dir: Option<PathBuf> = if app.out.is_empty() { None } else { Some(PathBuf::from(&app.out)) };
    if let Some(d) = &render_dir {
        std::fs::create_dir_all(d).map_err(|e| AppError::InvalidArg(format!("--out mkdir: {e}")))?;
    }

    ctx.emit(Progress::Started {
        total: Some(targets.len() as u64),
        message: Some(format!("stepping animations in {} tables", targets.len())),
    });

    let mut tables: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut sequences = 0usize;
    let mut rendered = 0usize;

    for (ti, path) in targets.iter().enumerate() {
        ctx.tick(ti as u64, Some(targets.len() as u64), "");
        let file_str = path.display().to_string();
        let p = match crate::util::load_pxls(path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({ "file": file_str, "error": e }));
                continue;
            }
        };
        let mut seqs: Vec<serde_json::Value> = Vec::new();
        let mut set = None;
        // 一个播放头跑完整张表：换个序列就 set_sequence（状态归零），与 Java 一致
        let mut anim: Option<Animator> = None;

        for pose in &p.poses {
            if !glob_match(&app.pose, &pose.title) {
                continue;
            }
            for seq in &pose.seqs {
                if app.dir != 99 && seq.dir as u32 != app.dir {
                    continue;
                }
                if let Some(a) = anim.as_mut() {
                    a.set_sequence(seq);
                } else {
                    anim = Some(Animator::new(seq));
                }
                let anim = anim.as_mut().expect("just ensured");
                if !app.frame.is_empty() {
                    if let Err(e) = anim.change_frame(&app.frame) {
                        errors.push(serde_json::json!({
                            "file": file_str, "pose": pose.title, "dir": seq.dir, "error": e,
                        }));
                        continue;
                    }
                } else if app.index != 99 && (app.index as usize) < anim.frame_count() {
                    anim.position = app.index as usize;
                }
                for _ in 0..app.ticks {
                    anim.step();
                }
                for _ in 0..app.frames {
                    anim.step_frame();
                }
                sequences += 1;

                let mut entry = serde_json::json!({
                    "pose": pose.title,
                    "dir": seq.dir,
                    "frame_count": anim.frame_count(),
                    "loop_to": seq.loop_to,
                    "loop_to_clamped": anim.clamped,
                    "frames_per_loop": anim.frame_count().saturating_sub(
                        if seq.loop_to < 0 { 0 } else { seq.loop_to as usize }.min(anim.frame_count())),
                    "ticks_per_loop": ticks_per_loop(seq),
                    "position": anim.position,
                    "stepped": anim.stepped,
                    "looped_count": anim.looped,
                });
                if let Some(f) = anim.current() {
                    entry["frame"] = serde_json::json!({
                        "index": anim.position,
                        "name": f.name,
                        "crf60": f.crf60,
                        "layers": f.layers.len(),
                        "groups": f.layers.iter().filter(|l| l.group).count(),
                    });
                }

                if let Some(dir) = &render_dir {
                    if let Some(f) = anim.current() {
                        if set.is_none() {
                            match build_atlas_set(&p, path, &app.texture, "*") {
                                Ok(s) => set = Some(s),
                                Err(e) => errors.push(serde_json::json!({
                                    "file": file_str, "error": e,
                                })),
                            }
                        }
                        if let Some(s) = set.as_mut() {
                            let sc = app.scale.max(1) as f64;
                            let cw = (pose.width as f64 * sc).round().max(1.0) as u32;
                            let ch = (pose.height as f64 * sc).round().max(1.0) as u32;
                            // 画布上限与 render 一致，超限就跳过而不是吃掉几个 G 内存
                            if cw > 0 && ch > 0 && cw <= 4096 && ch <= 4096 {
                                let name = format!(
                                    "{}.{}.d{}.f{}.png",
                                    table_name(path),
                                    sanitize(&pose.title),
                                    seq.dir,
                                    anim.position
                                );
                                let fp = dir.join(&name);
                                match render_frame(s, f, cw, ch, sc, pose.width, pose.height) {
                                    Ok(canvas) => match crate::atlas::write_png(&fp, cw, ch, &canvas.rgba) {
                                        Ok(()) => {
                                            rendered += 1;
                                            entry["rendered"] = serde_json::json!(fp.display().to_string());
                                        }
                                        Err(e) => errors.push(serde_json::json!({
                                            "file": file_str, "error": e,
                                        })),
                                    },
                                    Err(e) => errors.push(serde_json::json!({
                                        "file": file_str, "pose": pose.title, "error": e,
                                    })),
                                }
                            }
                        }
                    }
                }
                seqs.push(entry);
            }
        }

        if !seqs.is_empty() {
            tables.push(serde_json::json!({
                "file": file_str,
                "table": table_name(path),
                "sequences": seqs,
            }));
        }
    }

    let result = serde_json::json!({
        "root": app.root.display().to_string(),
        "pose_filter": app.pose,
        "dir": if app.dir == 99 { None } else { Some(app.dir) },
        "frame": app.frame,
        "ticks": app.ticks,
        "frames": app.frames,
        "tables_scanned": targets.len(),
        "sequences": sequences,
        "rendered": rendered,
        "tables": tables,
        "errors": errors,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pxlslib::{Frame, Layer, Seq};

    fn frame(name: &str, crf60: i16) -> Frame {
        Frame {
            index: 0,
            name: name.to_string(),
            crf60,
            layers: Vec::new(),
            ver: 0,
            trailer: None,
        }
    }

    fn seq(frames: Vec<Frame>, loop_to: i16) -> Seq {
        Seq {
            dir: 0,
            width: 0,
            height: 0,
            body_x: 0,
            body_y: 0,
            shift_x: 0,
            shift_y: 0,
            loop_to,
            frames,
            ver: 0,
            frame_snd: Vec::new(),
        }
    }

    /// 帧层字段只是占位，测试里用不到
    #[allow(dead_code)]
    fn dummy_layer() -> Layer {
        Layer {
            index: 0,
            name: String::new(),
            kind: 0,
            kind_name: String::new(),
            group: false,
            alpha: 100,
            x: 0.0,
            y: 0.0,
            zmx: 1.0,
            zmy: 1.0,
            rot_r: 0.0,
            blend_variable: 0,
            img: String::new(),
            img_size: None,
            group_n: None,
            id: 0,
            id2: 0.0,
            raw_alpha: 10000,
            unk_u32: 0,
            unk_b1: 0,
            unk_b2: 0,
        }
    }

    #[test]
    fn step_accumulates_crf60_ticks_before_advancing() {
        let s = seq(vec![frame("a", 2), frame("b", 3)], 0);
        let mut a = Animator::new(&s);
        assert_eq!((a.position, a.stepped), (0, 0));
        a.step();
        assert_eq!((a.position, a.stepped), (0, 1), "crf60=2 的帧要走满 2 tick 才换帧");
        a.step();
        assert_eq!((a.position, a.stepped), (1, 0), "第 2 个 tick 用满，换帧且计数归零");
        a.step();
        assert_eq!((a.position, a.stepped), (1, 1), "crf60=3 的帧要走满 3 tick");
        a.step();
        assert_eq!((a.position, a.stepped), (1, 2));
        a.step();
        assert_eq!((a.position, a.stepped), (0, 0), "走满 3 tick 后回卷到 loop_to=0，计数也归零");
        assert_eq!(a.looped, 1);
    }

    #[test]
    fn running_past_the_end_wraps_to_loop_to() {
        let s = seq(vec![frame("a", 1), frame("b", 1), frame("c", 1)], 1);
        let mut a = Animator::new(&s);
        a.step(); // → b
        a.step(); // → c
        assert_eq!(a.position, 2);
        a.step(); // → 回卷
        assert_eq!(a.position, 1, "必须回到 loop_to=1，而不是 0");
        assert_eq!(a.looped, 1);
    }

    #[test]
    fn step_frame_ignores_the_tick_budget() {
        let s = seq(vec![frame("a", 100), frame("b", 100), frame("c", 100)], 0);
        let mut a = Animator::new(&s);
        a.step();
        assert_eq!(a.position, 0, "crf60=100 时 step 还没换帧");
        a.step_frame();
        assert_eq!(a.position, 1, "stepFrame 整帧跳，不受 crf60 影响");
        assert_eq!(a.stepped, 0, "stepFrame 会清掉 tick 计数");
    }

    #[test]
    fn change_frame_is_case_insensitive_and_rejects_unknown_names() {
        let s = seq(vec![frame("Stand", 1), frame("Walk", 1)], 0);
        let mut a = Animator::new(&s);
        a.change_frame("walk").unwrap();
        assert_eq!(a.position, 1);
        assert_eq!(a.current().unwrap().name, "Walk");
        assert!(a.change_frame("nope").is_err());
    }

    #[test]
    fn loop_to_out_of_range_is_clamped_instead_of_panicking() {
        // 原版 FrameAnimator 在这里会越界崩，我们必须钳住
        let s = seq(vec![frame("a", 1), frame("b", 1)], 9);
        let mut a = Animator::new(&s);
        a.step();
        a.step();
        assert_eq!(a.position, 0, "loop_to=9 越界 → 钳到 0");
        assert!(a.clamped);
        assert_eq!(a.looped, 1);
    }

    #[test]
    fn ticks_per_loop_counts_from_loop_to_not_from_zero() {
        let s = seq(vec![frame("a", 10), frame("b", 20), frame("c", 30)], 1);
        assert_eq!(ticks_per_loop(&s), 50, "一个循环 = b + c 的 crf60");
        let s2 = seq(vec![frame("a", 10), frame("b", 20)], 99);
        assert_eq!(ticks_per_loop(&s2), 30, "loop_to 越界时从 0 起算");
    }

    #[test]
    fn stepping_an_empty_sequence_is_a_noop() {
        let s = seq(vec![], 0);
        let mut a = Animator::new(&s);
        a.step();
        a.step_frame();
        assert_eq!((a.position, a.stepped, a.looped), (0, 0, 0));
        assert!(a.current().is_none());
    }
}
