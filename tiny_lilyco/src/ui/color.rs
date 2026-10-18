//! Colour primitives that work without `std`, floats or an allocator.

/// An sRGB colour stored as `0xRRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(u32);

impl Color {
    /// Build an opaque colour from `0xRRGGBB`.
    pub const fn rgb(hex: u32) -> Self {
        Color(((hex & 0x00FF_FFFF) << 8) | 0xFF)
    }

    /// Build a colour from explicit channels.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | a as u32)
    }

    /// Red channel.
    pub const fn r(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    /// Green channel.
    pub const fn g(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    /// Blue channel.
    pub const fn b(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Alpha channel.
    pub const fn a(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Same hue, different opacity.
    pub const fn with_alpha(self, a: u8) -> Self {
        Color((self.0 & 0xFFFF_FF00) | a as u32)
    }

    /// Perceptual brightness on a `0..=1000` scale (YIQ, integer maths).
    ///
    /// This is a cheap contrast heuristic, not a WCAG measurement: it is used to
    /// pick readable foregrounds and to let CI assert a theme is not inverted.
    pub const fn luma(self) -> u16 {
        let r = self.r() as u32;
        let g = self.g() as u32;
        let b = self.b() as u32;
        ((299 * r + 587 * g + 114 * b) / 255) as u16
    }

    /// True when the colour reads as light.
    pub const fn is_light(self) -> bool {
        self.luma() > 500
    }

    /// A foreground that stays readable on top of this colour.
    pub const fn readable_fg(self) -> Color {
        if self.is_light() {
            Color::rgb(0x09090B)
        } else {
            Color::rgb(0xFAFAFA)
        }
    }

    /// Linear blend towards `other`; `t` runs from 0 (self) to 255 (other).
    pub const fn blend(self, other: Color, t: u8) -> Color {
        let k = t as u32;
        let inv = 255 - k;
        let r = (self.r() as u32 * inv + other.r() as u32 * k) / 255;
        let g = (self.g() as u32 * inv + other.g() as u32 * k) / 255;
        let b = (self.b() as u32 * inv + other.b() as u32 * k) / 255;
        let a = (self.a() as u32 * inv + other.a() as u32 * k) / 255;
        Color::rgba(r as u8, g as u8, b as u8, a as u8)
    }

    /// Write `#rrggbb` (alpha dropped) into `out`.
    pub fn to_hex<const N: usize>(
        self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        ::core::write!(out, "#{:02x}{:02x}{:02x}", self.r(), self.g(), self.b())
            .map_err(|_| "buffer full")
    }

    /// Write `rgba(r, g, b, a)` into `out`.
    pub fn to_css<const N: usize>(
        self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        ::core::write!(
            out,
            "rgba({}, {}, {}, {})",
            self.r(),
            self.g(),
            self.b(),
            self.a()
        )
        .map_err(|_| "buffer full")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_round_trip() {
        let c = Color::rgb(0x1A1B26);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (0x1A, 0x1B, 0x26, 0xFF));
    }

    #[test]
    fn alpha_is_preserved() {
        let c = Color::rgb(0x7AA2F7).with_alpha(0x66);
        assert_eq!(c.a(), 0x66);
        assert_eq!(c.r(), 0x7A);
    }

    #[test]
    fn luma_separates_light_from_dark() {
        assert!(Color::rgb(0xFAFAFA).is_light());
        assert!(!Color::rgb(0x1A1B26).is_light());
        assert!(Color::rgb(0xFFFFFF).luma() >= 990);
        assert!(Color::rgb(0x000000).luma() <= 10);
    }

    #[test]
    fn readable_fg_flips() {
        assert!(!Color::rgb(0xFFFFFF).readable_fg().is_light());
        assert!(Color::rgb(0x000000).readable_fg().is_light());
    }

    #[test]
    fn blend_hits_both_ends() {
        let a = Color::rgb(0x000000);
        let b = Color::rgb(0xFFFFFF);
        assert_eq!(a.blend(b, 0), a);
        assert_eq!(a.blend(b, 255), b);
        let mid = a.blend(b, 128);
        assert!((120..=135).contains(&mid.r()));
    }

    #[test]
    fn hex_and_css_are_exact() {
        let c = Color::rgb(0x1A1B26);
        let mut buf: heapless::String<48> = heapless::String::new();
        c.to_hex(&mut buf).unwrap();
        assert_eq!(buf.as_str(), "#1a1b26");
        let mut buf2: heapless::String<48> = heapless::String::new();
        c.with_alpha(0x80).to_css(&mut buf2).unwrap();
        assert_eq!(buf2.as_str(), "rgba(26, 27, 38, 128)");
    }
}
