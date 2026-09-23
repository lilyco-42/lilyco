//! 跨模块小工具：只放「无领域含义」的纯函数（有归属的都待在自己的模块里）。

use rand::distributions::Alphanumeric;
use rand::Rng;

pub(crate) fn generate_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// HTML 转义：所有插值进 HTML 模板的动态内容必须先过这里
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escapes_dangerous_chars() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a\"b'c&d"), "a&quot;b&#39;c&amp;d");
        assert_eq!(html_escape("普通中文"), "普通中文");
    }
}
