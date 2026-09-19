//! EdgeTTS 语音合成 —— 对齐 lly（github.com/lilyco-42/lly）的成熟方案。
//!
//! lly 曾自研协议层（Sec-MS-GEC / WSS 握手头 / X-Timestamp），后按
//! 「采纳优于自修」信条退役，改用 `msedge-tts` crate（纯 Rust TLS：
//! rustls + rustls-platform-verifier + tungstenite，无 openssl 系统依赖）。
//! 本模块复刻同一做法、保留相同签名 `synthesize(text, voice, rate, pitch) -> mp3 bytes`：
//!
//! - 微软 Edge 在线语音（免费、无需密钥），输出 `audio-24khz-48kbitrate-mono-mp3`
//! - DRM 时钟窗口（Sec-MS-GEC，5 分钟粒度）由 crate 内部处理，调用方无感
//! - **网络依赖**：本模块必须联网才能工作；单元测试一律走参数校验 / schema
//!   断言路径，真实合成测试标 `#[ignore]`（见 `tests/pet_bridge.rs`）
//!
//! 与 pet 现有语音链路的关系：pet 已接 GPT-SoVITS 克隆音色（`PET_TTS_URL`，
//! 还原度高但需 GPU 服务常驻）。pet-say 是**轻量兜底链路**——无 GPU / 离线
//! 场景下用 EdgeTTS 少女音色近似丛雨声线。双链路取舍见 `docs/ECOSYSTEM_PET.md`。

use msedge_tts::tts::client::connect;
use msedge_tts::tts::SpeechConfig;

/// 默认音色：zh-CN-XiaoyiNeural（晓伊，少女声线，EdgeTTS 免费音色里最接近丛雨）。
///
/// 官方克隆音色走 pet 侧 GPT-SoVITS；此处仅为「随手可用」的近似。
/// 注意：`#[app]` 宏的 `default` 属性只接受字面量，`app.rs` 里的默认值
/// 与本常量由测试 `pet_say_schema_voice_default_matches_const` 锁同步。
pub const DEFAULT_VOICE: &str = "zh-CN-XiaoyiNeural";

/// 音频格式：24kHz 采样 / 48kbps 码率 / 单声道 mp3（与 lly 一致）
pub const AUDIO_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";

/// 48kbps ≈ 6KB/s，用于估算音频时长（无需解码 mp3）
pub const BYTES_PER_SECOND: usize = 6000;

/// 合成语音，返回 mp3 二进制
///
/// # 参数
/// - `text`：要朗读的文本（调用方保证非空）
/// - `voice`：音色 id，如 `zh-CN-XiaoyiNeural`
/// - `rate`：语速，如 `+10%` / `-10%`
/// - `pitch`：音调，如 `+0Hz`
pub fn synthesize(text: &str, voice: &str, rate: &str, pitch: &str) -> Result<Vec<u8>, String> {
    let config = SpeechConfig {
        voice_name: voice.to_string(),
        audio_format: AUDIO_FORMAT.to_string(),
        pitch: parse_offset(pitch),
        rate: parse_offset(rate),
        volume: 0,
    };
    let mut tts = connect().map_err(|e| format!("connect: {e}"))?;
    let audio = tts
        .synthesize(text, &config)
        .map_err(|e| format!("synthesize: {e}"))?;
    if audio.audio_bytes.is_empty() {
        return Err("empty audio (voice name 可能无效)".into());
    }
    Ok(audio.audio_bytes)
}

/// 由字节数估算音频时长（秒）：48kbps → 约 6KB/s
pub fn estimate_seconds(bytes: usize) -> u64 {
    (bytes / BYTES_PER_SECOND) as u64
}

/// "+10%" / "-5Hz" / "10" → i32 偏移；容错解析，非法值回退 0（与 lly 相同）
fn parse_offset(s: &str) -> i32 {
    s.trim()
        .trim_start_matches('+')
        .trim_end_matches('%')
        .trim_end_matches("Hz")
        .trim()
        .parse()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_offset_tolerates_signs_and_units() {
        assert_eq!(parse_offset("+10%"), 10);
        assert_eq!(parse_offset("-5Hz"), -5);
        assert_eq!(parse_offset("10"), 10);
        assert_eq!(parse_offset("bogus"), 0);
        assert_eq!(parse_offset(""), 0);
    }

    #[test]
    fn estimate_seconds_follows_48kbps_math() {
        assert_eq!(estimate_seconds(6000), 1);
        assert_eq!(estimate_seconds(0), 0);
        assert_eq!(estimate_seconds(5999), 0);
    }
}
