//! pet-say / pet-act 两个派生应用 —— 同一 struct 派生 CLI/TUI/Web/MCP 四端
//!
//! - [`PetSay`]：T0 只读级（写临时/指定目录的音频产物），合成 + 遥测 + 路径回报
//! - [`PetAct`]：T0 只读级（只产出指令 JSON，不驱动真实世界），指令 + 遥测
//!
//! 完整蓝图见 `docs/ECOSYSTEM_PET.md`。

use lilyco::prelude::*;

use crate::action::PetAction;
use crate::edge_tts;

/// 丛雨说话：EdgeTTS 文本转语音（T0）
///
/// 输出 mp3 到指定路径（缺省系统临时目录 `lilyco-pet/`），遥测上报字节数
/// 与估算时长，Done 返回音频文件路径——聊天链路里由 pet 前端取路径播放。
#[derive(App)]
#[app(
    name = "pet-say",
    about = "丛雨桌宠语音：EdgeTTS 合成 mp3（微软在线语音，无需密钥），返回音频文件路径与字节数",
    run = "run_say"
)]
pub struct PetSay {
    /// 要说的话（必填，支持中日文）
    text: String,
    /// EdgeTTS 音色 id（默认丛雨系少女音色 zh-CN-XiaoyiNeural；官方克隆音色走 pet 侧 GPT-SoVITS 链路）
    #[arg(default = "zh-CN-XiaoyiNeural")]
    voice: String,
    /// 语速，如 "+0%" / "+10%" / "-10%"
    #[arg(default = "+0%")]
    rate: String,
    /// 输出 mp3 路径（默认系统临时目录下 lilyco-pet/pet_say_<时间戳>.mp3）
    output: Option<String>,
}

/// 音色兜底：MCP 等调用面可能不回填 schema default，空串则回退默认音色
fn effective_voice(voice: &str) -> &str {
    if voice.trim().is_empty() {
        edge_tts::DEFAULT_VOICE
    } else {
        voice
    }
}

/// 语速兜底：空串 → "+0%"（不变速）
fn effective_rate(rate: &str) -> &str {
    if rate.trim().is_empty() {
        "+0%"
    } else {
        rate
    }
}

pub fn run_say(app: &PetSay, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let text = app.text.trim();
    if text.is_empty() {
        // 必填校验放在 run 里（from_args 对缺省 String 只给空串），错误可在无网络下验证门控行为
        return Err(AppError::InvalidArg(
            "text 不能为空：请提供丛雨要说的话".into(),
        ));
    }
    let voice = effective_voice(&app.voice);
    let rate = effective_rate(&app.rate);
    // rustls 0.23 要求进程级默认 CryptoProvider（msedge-tts 经 tungstenite/rustls 连 WSS）；
    // 已装过则报错，忽略即可（幂等）
    let _ = rustls::crypto::ring::default_provider().install_default();

    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some("丛雨开口（EdgeTTS 合成中）...".into()),
    });
    let audio = edge_tts::synthesize(text, voice, rate, "+0Hz")
        .map_err(|e| AppError::Runtime(format!("edge-tts: {e}")))?;

    let out = match &app.output {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            // 跨平台：Windows/Linux 临时目录都由 std::env::temp_dir() 给出
            std::env::temp_dir()
                .join("lilyco-pet")
                .join(format!("pet_say_{ts}.mp3"))
        }
    };
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Runtime(format!("mkdir {}: {e}", parent.display())))?;
        }
    }
    std::fs::write(&out, &audio)
        .map_err(|e| AppError::Runtime(format!("write {}: {e}", out.display())))?;

    // P1 遥测：字节数与估算时长（48kbps → 约 6KB/s），Agent 在 MCP 通道实时可见
    let seconds = edge_tts::estimate_seconds(audio.len());
    ctx.telemetry("pet.say.voice", serde_json::json!(voice));
    ctx.telemetry("pet.say.bytes", serde_json::json!(audio.len()));
    ctx.telemetry("pet.say.seconds", serde_json::json!(seconds));

    let r = serde_json::json!({
        "status": "ok",
        "path": out.to_string_lossy(),
        "bytes": audio.len(),
        "estimate_seconds": seconds,
        "voice": voice,
        "rate": rate,
    });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 丛雨做动作：输出结构化指令 JSON（T0）
///
/// 执行 = 产出动作指令（表情 id / AppEvent 变体名 / 数字键序号），供 pet 前端
/// 进程经 IPC 消费；遥测上报动作名。动作词表对齐 pet 源码事件面，见 [`PetAction`]。
#[derive(App)]
#[app(
    name = "pet-act",
    about = "丛雨桌宠动作：输出结构化动作指令 JSON（表情/服装/差分/快速说话），供 pet 前端进程经 IPC 消费",
    run = "run_act"
)]
pub struct PetAct {
    /// 动作（对齐 pet/src/app.rs 的 AppEvent 事件面 + face_id 表情映射）
    action: PetAction,
    /// 强度 0.0-2.0（默认 1.0；预留动画幅度与后续情感强度门控）
    intensity: Option<f64>,
}

pub fn run_act(app: &PetAct, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let intensity = app.intensity.unwrap_or(1.0).clamp(0.0, 2.0);
    let instruction = app.action.instruction(intensity);

    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("丛雨执行动作: {}", app.action.name())),
    });
    // P1 遥测：动作即状态流，pet 前端 / Agent 面板实时可见
    ctx.telemetry("pet.action", serde_json::json!(app.action.name()));

    let r = serde_json::json!({
        "status": "ok",
        "action": app.action.name(),
        "intensity": intensity,
        "instruction": instruction,
        "note": "pet 前端进程经 IPC（本地 MCP / stdio 行协议）消费 instruction；蓝图见 docs/ECOSYSTEM_PET.md",
    });
    ctx.done(r.clone(), 0);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_voice_falls_back_to_default() {
        assert_eq!(effective_voice(""), edge_tts::DEFAULT_VOICE);
        assert_eq!(effective_voice("  "), edge_tts::DEFAULT_VOICE);
        assert_eq!(
            effective_voice("zh-CN-XiaoxiaoNeural"),
            "zh-CN-XiaoxiaoNeural"
        );
    }

    #[test]
    fn empty_rate_falls_back_to_neutral() {
        assert_eq!(effective_rate(""), "+0%");
        assert_eq!(effective_rate("+10%"), "+10%");
    }
}
