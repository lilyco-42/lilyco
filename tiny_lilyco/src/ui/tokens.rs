//! Non-colour design tokens: grid, radii, type scale, motion.
//!
//! All values are integers so they survive `no_std` and map 1:1 onto GPUI's
//! `px(..)` and CSS pixels.

/// 4px base grid used for every gap and padding.
pub const GRID: u16 = 4;

/// Spacing scale (px), on the 4px grid.
pub const SPACE_1: u16 = 4;
pub const SPACE_2: u16 = 8;
pub const SPACE_3: u16 = 12;
pub const SPACE_4: u16 = 16;
pub const SPACE_6: u16 = 24;
pub const SPACE_8: u16 = 32;
pub const SPACE_12: u16 = 48;

/// Radii (px), aligned with the existing `apps/*` shadcn themes:
/// `--radius-md: .5rem` (8px), `--radius-lg: .75rem` (12px), `--radius-xl: 1rem`.
pub const RADIUS_SM: u16 = 4;
pub const RADIUS_MD: u16 = 8;
pub const RADIUS_LG: u16 = 12;
pub const RADIUS_XL: u16 = 16;
pub const RADIUS_FULL: u16 = 9999;

/// Hairline border, always 1 physical pixel.
pub const BORDER_WIDTH: u16 = 1;

/// Focus ring: 2px ring offset by 2px, per shadcn's focus-visible treatment.
pub const FOCUS_RING_WIDTH: u16 = 2;
pub const FOCUS_RING_OFFSET: u16 = 2;

/// Font sizes (px).
pub const TEXT_CAPTION: u16 = 11;
pub const TEXT_SMALL: u16 = 12;
pub const TEXT_BODY: u16 = 13;
pub const TEXT_EMPHASIS: u16 = 14;
pub const TEXT_TITLE: u16 = 16;
pub const TEXT_HEADING: u16 = 20;
pub const TEXT_DISPLAY: u16 = 24;

/// Line heights (px) paired with the sizes above.
pub const LEADING_CAPTION: u16 = 16;
pub const LEADING_SMALL: u16 = 18;
pub const LEADING_BODY: u16 = 20;
pub const LEADING_EMPHASIS: u16 = 22;
pub const LEADING_TITLE: u16 = 24;
pub const LEADING_HEADING: u16 = 28;
pub const LEADING_DISPLAY: u16 = 32;

/// Font weights.
pub const WEIGHT_REGULAR: u16 = 400;
pub const WEIGHT_MEDIUM: u16 = 500;
pub const WEIGHT_SEMIBOLD: u16 = 600;

/// Motion durations (ms). Fast for hover, base for state, slow for panels.
pub const MOTION_FAST: u16 = 120;
pub const MOTION_BASE: u16 = 180;
pub const MOTION_SLOW: u16 = 240;

/// Emit the non-colour half of the design system as CSS custom properties.
pub fn emit_css<const N: usize>(out: &mut heapless::String<N>) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    let vars: [(&str, u16); 16] = [
        ("space-1", SPACE_1),
        ("space-2", SPACE_2),
        ("space-3", SPACE_3),
        ("space-4", SPACE_4),
        ("space-6", SPACE_6),
        ("space-8", SPACE_8),
        ("radius-sm", RADIUS_SM),
        ("radius-md", RADIUS_MD),
        ("radius-lg", RADIUS_LG),
        ("radius-xl", RADIUS_XL),
        ("text-body", TEXT_BODY),
        ("text-title", TEXT_TITLE),
        ("leading-body", LEADING_BODY),
        ("motion-fast", MOTION_FAST),
        ("motion-base", MOTION_BASE),
        ("motion-slow", MOTION_SLOW),
    ];
    for (name, value) in vars {
        ::core::write!(out, "  --{}: {}px;\n", name, value).map_err(|_| "buffer full")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_is_on_the_grid() {
        for step in [SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_6, SPACE_8, SPACE_12] {
            assert_eq!(step % GRID, 0, "{} is off the {}px grid", step, GRID);
        }
    }

    #[test]
    fn spacing_scale_is_monotonic() {
        let scale = [SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_6, SPACE_8, SPACE_12];
        assert!(scale.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn radii_are_monotonic_and_ordered() {
        assert!(RADIUS_SM < RADIUS_MD);
        assert!(RADIUS_MD < RADIUS_LG);
        assert!(RADIUS_LG < RADIUS_XL);
        assert!(RADIUS_XL < RADIUS_FULL);
    }

    #[test]
    fn type_scale_is_monotonic() {
        let sizes = [
            TEXT_CAPTION,
            TEXT_SMALL,
            TEXT_BODY,
            TEXT_EMPHASIS,
            TEXT_TITLE,
            TEXT_HEADING,
            TEXT_DISPLAY,
        ];
        assert!(sizes.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn leading_always_exceeds_size() {
        let pairs = [
            (TEXT_CAPTION, LEADING_CAPTION),
            (TEXT_SMALL, LEADING_SMALL),
            (TEXT_BODY, LEADING_BODY),
            (TEXT_EMPHASIS, LEADING_EMPHASIS),
            (TEXT_TITLE, LEADING_TITLE),
            (TEXT_HEADING, LEADING_HEADING),
            (TEXT_DISPLAY, LEADING_DISPLAY),
        ];
        assert!(pairs.iter().all(|(size, lead)| lead > size));
    }

    #[test]
    fn css_emits_the_scale() {
        let mut buf: heapless::String<512> = heapless::String::new();
        emit_css(&mut buf).unwrap();
        assert!(buf.as_str().contains("--radius-lg: 12px;"));
        assert!(buf.as_str().contains("--space-4: 16px;"));
    }
}
