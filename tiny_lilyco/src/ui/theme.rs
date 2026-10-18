//! Themes: one semantic palette, three presets, one source of truth.
//!
//! The same tokens feed the GPUI shell (`Color::rgb(..)`), the Tauri/webview
//! shell (`emit_css`) and the CLI (`to_hex`), so the three front-ends cannot
//! drift apart.

use crate::ui::color::Color;

/// Semantic colour roles, named after their shadcn/ui counterparts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub card_foreground: Color,
    pub popover: Color,
    pub popover_foreground: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub secondary_foreground: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub accent: Color,
    pub accent_foreground: Color,
    pub destructive: Color,
    pub destructive_foreground: Color,
    pub border: Color,
    pub input: Color,
    pub ring: Color,
    pub sidebar: Color,
    pub sidebar_foreground: Color,
    pub sidebar_accent: Color,
    pub sidebar_border: Color,
    pub success: Color,
    pub warning: Color,
    pub info: Color,
    pub selection: Color,
    pub code_bg: Color,
}

impl Palette {
    /// Every role as a `(css-name, colour)` pair, in declaration order.
    pub const fn entries(&self) -> [(&'static str, Color); 28] {
        [
            ("background", self.background),
            ("foreground", self.foreground),
            ("card", self.card),
            ("card-foreground", self.card_foreground),
            ("popover", self.popover),
            ("popover-foreground", self.popover_foreground),
            ("primary", self.primary),
            ("primary-foreground", self.primary_foreground),
            ("secondary", self.secondary),
            ("secondary-foreground", self.secondary_foreground),
            ("muted", self.muted),
            ("muted-foreground", self.muted_foreground),
            ("accent", self.accent),
            ("accent-foreground", self.accent_foreground),
            ("destructive", self.destructive),
            ("destructive-foreground", self.destructive_foreground),
            ("border", self.border),
            ("input", self.input),
            ("ring", self.ring),
            ("sidebar", self.sidebar),
            ("sidebar-foreground", self.sidebar_foreground),
            ("sidebar-accent", self.sidebar_accent),
            ("sidebar-border", self.sidebar_border),
            ("success", self.success),
            ("warning", self.warning),
            ("info", self.info),
            ("selection", self.selection),
            ("code-bg", self.code_bg),
        ]
    }

    /// Look a role up by its CSS name.
    pub fn get(&self, name: &str) -> Option<Color> {
        let hit = self.entries().into_iter().find(|(n, _)| *n == name);
        hit.map(|(_, c)| c)
    }

    /// Tokyo Night — the default theme.
    pub const fn tokyo_night() -> Self {
        Palette {
            background: Color::rgb(0x1A1B26),
            foreground: Color::rgb(0xC0CAF5),
            card: Color::rgb(0x1F2335),
            card_foreground: Color::rgb(0xC0CAF5),
            popover: Color::rgb(0x292E42),
            popover_foreground: Color::rgb(0xC0CAF5),
            primary: Color::rgb(0x7AA2F7),
            primary_foreground: Color::rgb(0x1A1B26),
            secondary: Color::rgb(0x292E42),
            secondary_foreground: Color::rgb(0xC0CAF5),
            muted: Color::rgb(0x24283B),
            muted_foreground: Color::rgb(0x565F89),
            accent: Color::rgb(0x292E42),
            accent_foreground: Color::rgb(0xC0CAF5),
            destructive: Color::rgb(0xF7768E),
            destructive_foreground: Color::rgb(0x1A1B26),
            border: Color::rgb(0x292E42),
            input: Color::rgb(0x292E42),
            ring: Color::rgb(0x7AA2F7),
            sidebar: Color::rgb(0x16161E),
            sidebar_foreground: Color::rgb(0xA9B1D6),
            sidebar_accent: Color::rgb(0x292E42),
            sidebar_border: Color::rgb(0x1F2335),
            success: Color::rgb(0x9ECE6A),
            warning: Color::rgb(0xE0AF68),
            info: Color::rgb(0x7DCFFF),
            selection: Color::rgb(0x33467C),
            code_bg: Color::rgb(0x16161E),
        }
    }

    /// Neutral light — shadcn `zinc` light, the "premium paper" look.
    pub const fn neutral_light() -> Self {
        Palette {
            background: Color::rgb(0xFFFFFF),
            foreground: Color::rgb(0x09090B),
            card: Color::rgb(0xFFFFFF),
            card_foreground: Color::rgb(0x09090B),
            popover: Color::rgb(0xFFFFFF),
            popover_foreground: Color::rgb(0x09090B),
            primary: Color::rgb(0x18181B),
            primary_foreground: Color::rgb(0xFAFAFA),
            secondary: Color::rgb(0xF4F4F5),
            secondary_foreground: Color::rgb(0x18181B),
            muted: Color::rgb(0xF4F4F5),
            muted_foreground: Color::rgb(0x71717A),
            accent: Color::rgb(0xF4F4F5),
            accent_foreground: Color::rgb(0x18181B),
            destructive: Color::rgb(0xDC2626),
            destructive_foreground: Color::rgb(0xFAFAFA),
            border: Color::rgb(0xE4E4E7),
            input: Color::rgb(0xE4E4E7),
            ring: Color::rgb(0x18181B),
            sidebar: Color::rgb(0xFAFAFA),
            sidebar_foreground: Color::rgb(0x3F3F46),
            sidebar_accent: Color::rgb(0xF4F4F5),
            sidebar_border: Color::rgb(0xE4E4E7),
            success: Color::rgb(0x16A34A),
            warning: Color::rgb(0xD97706),
            info: Color::rgb(0x0284C7),
            selection: Color::rgb(0xD4D4D8),
            code_bg: Color::rgb(0xF4F4F5),
        }
    }

    /// Neutral dark — shadcn `zinc` dark, the "graphite" look.
    pub const fn neutral_dark() -> Self {
        Palette {
            background: Color::rgb(0x09090B),
            foreground: Color::rgb(0xFAFAFA),
            card: Color::rgb(0x18181B),
            card_foreground: Color::rgb(0xFAFAFA),
            popover: Color::rgb(0x18181B),
            popover_foreground: Color::rgb(0xFAFAFA),
            primary: Color::rgb(0xFAFAFA),
            primary_foreground: Color::rgb(0x18181B),
            secondary: Color::rgb(0x27272A),
            secondary_foreground: Color::rgb(0xFAFAFA),
            muted: Color::rgb(0x27272A),
            muted_foreground: Color::rgb(0xA1A1AA),
            accent: Color::rgb(0x27272A),
            accent_foreground: Color::rgb(0xFAFAFA),
            destructive: Color::rgb(0xEF4444),
            destructive_foreground: Color::rgb(0xFAFAFA),
            border: Color::rgb(0x27272A),
            input: Color::rgb(0x27272A),
            ring: Color::rgb(0xD4D4D8),
            sidebar: Color::rgb(0x111113),
            sidebar_foreground: Color::rgb(0xD4D4D8),
            sidebar_accent: Color::rgb(0x27272A),
            sidebar_border: Color::rgb(0x27272A),
            success: Color::rgb(0x22C55E),
            warning: Color::rgb(0xF59E0B),
            info: Color::rgb(0x38BDF8),
            selection: Color::rgb(0x3F3F46),
            code_bg: Color::rgb(0x111113),
        }
    }
}

/// A named, self-contained theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Stable id used by `data-theme` and config files.
    pub id: &'static str,
    /// Human label shown in the theme switcher.
    pub label: &'static str,
    /// True for dark backgrounds.
    pub dark: bool,
    /// Colour roles.
    pub palette: Palette,
}

impl Theme {
    /// Tokyo Night, the product default.
    pub const fn tokyo_night() -> Self {
        Theme {
            id: "tokyo-night",
            label: "Tokyo Night",
            dark: true,
            palette: Palette::tokyo_night(),
        }
    }

    /// Neutral light.
    pub const fn neutral_light() -> Self {
        Theme {
            id: "neutral-light",
            label: "Neutral Light",
            dark: false,
            palette: Palette::neutral_light(),
        }
    }

    /// Neutral dark.
    pub const fn neutral_dark() -> Self {
        Theme {
            id: "neutral-dark",
            label: "Neutral Dark",
            dark: true,
            palette: Palette::neutral_dark(),
        }
    }

    /// The theme the app boots with.
    pub const fn default_theme() -> Self {
        Theme::tokyo_night()
    }

    /// Every shipped theme, for the switcher.
    pub const fn all() -> [Theme; 3] {
        [
            Theme::tokyo_night(),
            Theme::neutral_light(),
            Theme::neutral_dark(),
        ]
    }

    /// Resolve by id, falling back to the default.
    pub fn by_id(id: &str) -> Theme {
        let found = Theme::all().into_iter().find(|t| t.id == id);
        found.unwrap_or_else(Theme::default_theme)
    }

    /// Emit a CSS custom-property block for the webview shell.
    pub fn emit_css<const N: usize>(
        &self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        let kind = if self.dark { "dark" } else { "light" };
        ::core::write!(out, ":root {{\n  color-scheme: {};\n", kind)
            .map_err(|_| "buffer full")?;
        for (name, color) in self.palette.entries() {
            ::core::write!(
                out,
                "  --{}: #{:02x}{:02x}{:02x};\n",
                name,
                color.r(),
                color.g(),
                color.b()
            )
            .map_err(|_| "buffer full")?;
        }
        ::core::write!(out, "  --theme-kind: {};\n}}\n", kind).map_err(|_| "buffer full")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokyo_night_is_dark_with_light_text() {
        let p = Palette::tokyo_night();
        assert!(p.background.luma() < 120);
        assert!(p.foreground.luma() > 700);
    }

    #[test]
    fn every_theme_has_readable_body_text() {
        for theme in Theme::all() {
            let fg = theme.palette.foreground.luma();
            let bg = theme.palette.background.luma();
            let gap = fg.abs_diff(bg);
            assert!(gap > 500, "theme {} has low contrast", theme.id);
        }
    }

    #[test]
    fn roles_are_unique_and_counted() {
        let entries = Palette::tokyo_night().entries();
        assert_eq!(entries.len(), 28);
        for (i, (name, _)) in entries.iter().enumerate() {
            let dup = entries[i + 1..].iter().any(|(other, _)| other == name);
            assert!(!dup, "duplicate role {}", name);
        }
    }

    #[test]
    fn lookup_by_name_works() {
        let p = Palette::tokyo_night();
        assert_eq!(p.get("background"), Some(Color::rgb(0x1A1B26)));
        assert_eq!(p.get("nope"), None);
    }

    #[test]
    fn css_emission_is_complete() {
        let mut buf: heapless::String<2048> = heapless::String::new();
        Theme::tokyo_night().emit_css(&mut buf).unwrap();
        let css = buf.as_str();
        assert!(css.starts_with(":root {\n  color-scheme: dark;\n"));
        assert!(css.contains("--background: #1a1b26;"));
        assert!(css.contains("--card-foreground: #c0caf5;"));
        assert!(css.ends_with("}\n"));
    }

    #[test]
    fn by_id_falls_back_to_default() {
        assert_eq!(Theme::by_id("neutral-light").id, "neutral-light");
        assert_eq!(Theme::by_id("missing").id, "tokyo-night");
    }

    #[test]
    fn default_is_tokyo_night() {
        assert_eq!(Theme::default_theme().id, "tokyo-night");
        assert!(Theme::default_theme().dark);
    }
}
