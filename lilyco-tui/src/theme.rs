//! 主题配置：默认命名色与 `theme.toml` 覆盖。

use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

/// 颜色字符串（命名色或 `#RRGGBB`）
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ColorSpec(pub String);

/// TUI 主题令牌
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub accent: ColorSpec,
    pub border: ColorSpec,
    pub danger: ColorSpec,
    pub success: ColorSpec,
    pub warn: ColorSpec,
    pub muted: ColorSpec,
    pub text: ColorSpec,
}

impl Default for Theme {
    fn default() -> Self {
        let spec = |name: &str| ColorSpec(name.to_string());
        Self {
            accent: spec("cyan"),
            border: spec("dark_gray"),
            danger: spec("red"),
            success: spec("green"),
            warn: spec("yellow"),
            muted: spec("dark_gray"),
            text: spec("reset"),
        }
    }
}

impl Theme {
    /// 按 XDG / HOME 顺序加载 theme.toml；读取或解析失败都回退默认。
    pub fn load_default() -> Theme {
        let mut paths = Vec::new();
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                paths.push(PathBuf::from(xdg).join("lilyco-tui/theme.toml"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".config/lilyco-tui/theme.toml"));
        }
        for path in paths {
            if path.is_file() {
                return Self::from_path(&path);
            }
        }
        Self::default()
    }

    pub fn accent_style(&self) -> Style {
        Style::default().fg(color(&self.accent))
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(color(&self.border))
    }

    pub fn danger_style(&self) -> Style {
        Style::default().fg(color(&self.danger))
    }

    pub fn success_style(&self) -> Style {
        Style::default().fg(color(&self.success))
    }

    pub fn warn_style(&self) -> Style {
        Style::default().fg(color(&self.warn))
    }

    pub fn muted_style(&self) -> Style {
        Style::default()
            .fg(color(&self.muted))
            .add_modifier(Modifier::DIM)
    }

    pub fn text_style(&self) -> Style {
        Style::default().fg(color(&self.text))
    }

    fn from_path(path: &Path) -> Theme {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<Theme>(&text).ok())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn load_from(path: &Path) -> Theme {
        Self::from_path(path)
    }
}

/// 解析命名色或 `#RRGGBB`；无法解析回退 `Color::Reset`
pub fn color(spec: &ColorSpec) -> Color {
    let raw = spec.0.trim();
    if let Some(hex) = raw.strip_prefix('#') {
        if hex.len() == 6 && hex.is_ascii() {
            let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16);
            if let (Ok(r), Ok(g), Ok(b)) = (parse(0..2), parse(2..4), parse(4..6)) {
                return Color::Rgb(r, g, b);
            }
        }
        return Color::Reset;
    }
    match raw.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "dark_gray" | "darkgray" => Color::DarkGray,
        "light_red" | "lightred" => Color::LightRed,
        "light_green" | "lightgreen" => Color::LightGreen,
        "light_yellow" | "lightyellow" => Color::LightYellow,
        "light_blue" | "lightblue" => Color::LightBlue,
        "light_magenta" | "lightmagenta" => Color::LightMagenta,
        "light_cyan" | "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        "reset" | "default" => Color::Reset,
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_default_parses_named_colors() {
        let theme = Theme::default();
        assert_eq!(color(&theme.accent), Color::Cyan);
        assert_eq!(color(&theme.border), Color::DarkGray);
        assert_eq!(color(&theme.danger), Color::Red);
        assert_eq!(color(&theme.text), Color::Reset);
    }

    #[test]
    fn theme_toml_overrides_accent() {
        let mut file = tempfile::NamedTempFile::new().expect("临时文件");
        std::io::Write::write_all(file.as_file_mut(), b"accent = \"magenta\"\n").expect("写入主题");

        let theme = Theme::load_from(file.path());
        assert_eq!(color(&theme.accent), Color::Magenta);
        assert_eq!(color(&theme.border), Color::DarkGray);
        drop(file);
    }

    #[test]
    fn theme_hex_color_parses() {
        assert_eq!(
            color(&ColorSpec("#5f87af".into())),
            Color::Rgb(95, 135, 175)
        );
        assert_eq!(color(&ColorSpec("#xyzxyz".into())), Color::Reset);
    }
}
