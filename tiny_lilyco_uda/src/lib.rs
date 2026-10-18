//! Bridge between [`tiny_lilyco`] and UniDesktop/SDK (UDA).
//!
//! Verified against `UniDesktop/SDK` tag `v0.2.1`:
//!
//! * `uda_core::appearance::AppearanceManager` exposes `detect_theme`,
//!   `get_accent_color` and `capabilities`.
//! * `uda_core::capability::{Theme, Capability, RgbaColor}` carry the results.
//! * `uda_platform_windows::appearance::WindowsAppearanceManager`
//!   and `uda_platform_linux::appearance::LinuxAppearanceManager` implement it.
//!
//! Degradation is explicit: a failed probe yields `degraded = Some(reason)`,
//! never a panic, and the UI keeps its own theme instead of guessing.

use tiny_lilyco::ui::Theme;

/// What the OS reports about its own appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
    Auto,
    Unknown,
}

impl SystemTheme {
    /// Stable label for badges and logs.
    pub const fn label(self) -> &'static str {
        match self {
            SystemTheme::Light => "light",
            SystemTheme::Dark => "dark",
            SystemTheme::Auto => "auto",
            SystemTheme::Unknown => "unknown",
        }
    }

    /// `Some(dark?)` when the OS actually answered.
    pub const fn is_dark(self) -> Option<bool> {
        match self {
            SystemTheme::Dark => Some(true),
            SystemTheme::Light => Some(false),
            _ => None,
        }
    }
}

/// Snapshot of desktop capabilities taken at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopFacts {
    /// System appearance preference.
    pub theme: SystemTheme,
    /// Accent colour as `(r, g, b)` when the platform exposes one.
    pub accent: Option<(u8, u8, u8)>,
    /// Raw UDA capability bits.
    pub capabilities: u32,
    /// Why the probe is degraded, when it is.
    pub degraded: Option<&'static str>,
}

impl DesktopFacts {
    /// The honest answer when nothing could be probed.
    pub const fn unavailable(reason: &'static str) -> Self {
        DesktopFacts {
            theme: SystemTheme::Unknown,
            accent: None,
            capabilities: 0,
            degraded: Some(reason),
        }
    }

    /// How many capabilities answered yes.
    pub const fn capability_count(self) -> u32 {
        self.capabilities.count_ones()
    }

    /// Map the OS preference onto a shipped theme.
    ///
    /// Unknown and Auto deliberately fall back to the product default rather
    /// than inventing a preference the desktop never expressed.
    pub fn preferred_theme(self) -> Theme {
        match self.theme {
            SystemTheme::Light => Theme::neutral_light(),
            SystemTheme::Dark => Theme::tokyo_night(),
            _ => Theme::default_theme(),
        }
    }

    /// Short label for the header chip, e.g. `dark · 7 caps`.
    pub fn chip_label(&self) -> String {
        match self.degraded {
            None => format!("{} · {} caps", self.theme.label(), self.capability_count()),
            Some(_) => format!("desktop degraded ({})", self.theme.label()),
        }
    }
}

/// Probe the desktop once, at startup.
#[cfg(any(windows, target_os = "linux"))]
pub fn probe() -> DesktopFacts {
    platform_probe()
}

/// Probe the desktop once, at startup.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn probe() -> DesktopFacts {
    DesktopFacts::unavailable("no UDA backend for this platform")
}

#[cfg(any(windows, target_os = "linux"))]
fn platform_probe() -> DesktopFacts {
    match query() {
        Ok(facts) => facts,
        Err(reason) => DesktopFacts::unavailable(reason),
    }
}

#[cfg(windows)]
fn query() -> Result<DesktopFacts, &'static str> {
    use uda_core::appearance::AppearanceManager;
    let manager = uda_platform_windows::appearance::WindowsAppearanceManager::new();
    collect(&manager)
}

#[cfg(target_os = "linux")]
fn query() -> Result<DesktopFacts, &'static str> {
    use uda_core::appearance::AppearanceManager;
    let manager = uda_platform_linux::appearance::LinuxAppearanceManager::new();
    collect(&manager)
}

#[cfg(any(windows, target_os = "linux"))]
fn collect<M: uda_core::appearance::AppearanceManager>(
    manager: &M,
) -> Result<DesktopFacts, &'static str> {
    let theme = manager
        .detect_theme()
        .map_err(|_| "appearance detection failed")?;
    let caps = manager
        .capabilities()
        .map_err(|_| "capability query failed")?;
    let accent = manager
        .get_accent_color()
        .ok()
        .map(|c| (c.r, c.g, c.b));
    Ok(DesktopFacts {
        theme: map_theme(theme),
        accent,
        capabilities: caps.bits(),
        degraded: None,
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn map_theme(theme: uda_core::capability::Theme) -> SystemTheme {
    match theme {
        uda_core::capability::Theme::Light => SystemTheme::Light,
        uda_core::capability::Theme::Dark => SystemTheme::Dark,
        uda_core::capability::Theme::Auto => SystemTheme::Auto,
        uda_core::capability::Theme::Unknown => SystemTheme::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_falls_back_to_the_default_theme() {
        let facts = DesktopFacts::unavailable("test");
        assert_eq!(facts.preferred_theme().id, "tokyo-night");
        assert_eq!(facts.theme.label(), "unknown");
        assert!(facts.degraded.is_some());
    }

    #[test]
    fn light_prefers_the_light_theme() {
        let mut facts = DesktopFacts::unavailable("test");
        facts.theme = SystemTheme::Light;
        assert_eq!(facts.preferred_theme().id, "neutral-light");
    }

    #[test]
    fn dark_prefers_tokyo_night() {
        let mut facts = DesktopFacts::unavailable("test");
        facts.theme = SystemTheme::Dark;
        assert_eq!(facts.preferred_theme().id, "tokyo-night");
        assert_eq!(facts.theme.is_dark(), Some(true));
    }

    #[test]
    fn capability_count_counts_bits() {
        let mut facts = DesktopFacts::unavailable("test");
        facts.capabilities = 0b1011;
        assert_eq!(facts.capability_count(), 3);
    }

    #[test]
    fn chip_label_reports_degradation() {
        let facts = DesktopFacts::unavailable("nope");
        assert!(facts.chip_label().contains("degraded"));
    }
}
