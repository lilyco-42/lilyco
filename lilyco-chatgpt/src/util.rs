//! 小工具：时间戳格式化。刻意不引 chrono / time —— 整个 workspace 都没直接依赖它们，
//! 只有 epoch 秒和 API 给的 ISO 字符串两种输入，一个 civil-from-days 就够。

/// epoch 秒 → `YYYY-MM-DDTHH:MM:SSZ`（UTC）。chatgpt.com 后端两种时间戳都出现：
/// 会话列表里 `create_time` 是 ISO 字符串，消息节点里 `create_time` 是浮点 epoch 秒。
pub fn iso8601_from_epoch_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 浮点 epoch 秒（消息节点的时间戳）→ ISO
pub fn iso8601_from_epoch_f64(secs: f64) -> String {
    iso8601_from_epoch_secs(secs.trunc() as i64)
}

/// 把一个可能是 ISO 字符串、也可能是 epoch 数字的时间字段统一成 ISO；
/// 认不出来就原样返回（宁可不美化，也不要编一个假时间）。
pub fn iso8601_any(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => iso8601_from_epoch_secs(i),
            None => match n.as_f64() {
                Some(f) => iso8601_from_epoch_f64(f),
                None => String::new(),
            },
        },
        _ => String::new(),
    }
}

/// Howard Hinnant 的 days-from-civil 逆运算（公历，无闰秒）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 按字符（不是字节）截断，避免把中文切坏。返回 (截断后的串, 是否真的截了)
/// 只要截断后的串（错误文案里最常用的那种）
pub fn clip(s: &str, budget: usize) -> String {
    take_chars(s, budget).0
}

pub fn take_chars(s: &str, budget: usize) -> (String, bool) {
    if s.chars().count() <= budget {
        (s.to_string(), false)
    } else {
        (s.chars().take(budget).collect::<String>(), true)
    }
}

/// base64url 解码（JWT 用的就是这种字母表）。只为了读 token 里的 claim，
/// 不做任何签名校验 —— 我们只是想知道「这份 token 能干什么」。
pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 3);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '-' | '+' => 62,
            '_' | '/' => 63,
            '=' => continue,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// 标准 base64（带 `=` 填充）。只用于把图片内联进 JSON / MCP 内容块。
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((chunk[0] as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 按魔数判断图片类型：返回 `(mime, 扩展名)`。
///
/// MCP 客户端只认 png / jpeg / webp / gif 四种 MIME，所以这里也只用这四个；
/// 认不出来就当二进制，宁可不上图也不给客户端一个它渲染不了的 mime。
pub fn sniff_image_mime(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(b"\x89PNG") {
        ("image/png", "png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        ("image/jpeg", "jpg")
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 11 && &bytes[8..12] == b"WEBP" {
        ("image/webp", "webp")
    } else if bytes.starts_with(b"GIF8") {
        ("image/gif", "gif")
    } else {
        ("application/octet-stream", "bin")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn magic_numbers_pick_the_right_image_mime() {
        assert_eq!(sniff_image_mime(b"\x89PNG\r\n\x1a\n").0, "image/png");
        assert_eq!(sniff_image_mime(b"\xff\xd8\xff\xe0").1, "jpg");
        assert_eq!(sniff_image_mime(b"RIFF____WEBP").0, "image/webp");
        assert_eq!(sniff_image_mime(b"GIF89a").0, "image/gif");
        assert_eq!(
            sniff_image_mime(b"not an image").0,
            "application/octet-stream"
        );
    }
    #[test]
    fn epoch_zero_is_the_unix_epoch() {
        assert_eq!(iso8601_from_epoch_secs(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_instants_round_trip() {
        // 1795608075 = 2026-11-25T12:01:15Z（额度重置时刻）
        assert_eq!(
            iso8601_from_epoch_secs(1_795_608_075),
            "2026-11-25T12:01:15Z"
        );
        assert_eq!(
            iso8601_from_epoch_secs(1_795_589_543),
            "2026-11-25T06:52:23Z"
        );
        // 闰日
        assert_eq!(
            iso8601_from_epoch_secs(1_709_164_800),
            "2024-02-29T00:00:00Z"
        );
    }

    #[test]
    fn float_and_string_timestamps_both_normalise() {
        assert_eq!(iso8601_any(&json!(1_795_608_075)), "2026-11-25T12:01:15Z");
        assert_eq!(
            iso8601_any(&json!("2026-11-24T12:57:10.051984Z")),
            "2026-11-24T12:57:10.051984Z"
        );
        assert_eq!(
            iso8601_any(&json!(1_795_589_543.562)),
            "2026-11-25T06:52:23Z"
        );
        assert_eq!(iso8601_any(&json!(null)), "");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let (s, cut) = take_chars("中文中文中文", 4);
        assert!(cut);
        assert_eq!(s, "中文中文");
        let (s, cut) = take_chars("短", 4);
        assert!(!cut);
        assert_eq!(s, "短");
    }

    /// JWT 的三段是 base64url，带 - / _ 且常常没有 padding
    #[test]
    fn base64url_decodes_without_padding() {
        let raw = base64url_decode("eyJhIjoxfQ").unwrap();
        assert_eq!(String::from_utf8(raw).unwrap(), "{\"a\":1}");
        assert!(base64url_decode("??").is_none());
    }

    /// 图片要内联进 JSON，得是标准 base64（带 = 填充），而且空输入不能崩
    #[test]
    fn base64_encodes_with_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // PNG 魔数：下载回来的字节要能这么编码出去
        assert_eq!(base64_encode(b"\x89PNG"), "iVBORw==");
    }

    /// 编码再解码必须回到原字节（两套表在 +/ 与 -_ 上不同，但都要能解）
    #[test]
    fn encode_then_decode_round_trips() {
        let raw: Vec<u8> = (0u8..=255).collect();
        let enc = base64_encode(&raw);
        assert_eq!(base64url_decode(&enc).unwrap(), raw);
    }
}
