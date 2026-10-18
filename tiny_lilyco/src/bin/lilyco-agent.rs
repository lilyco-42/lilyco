//! Smoke-test binary packaged into the Windows MSI.
//!
//! It touches every part of the design system, so an install can be verified by
//! running one command: colours, scale, trace model and the emitted CSS.

use tiny_lilyco::agent::Phase;
use tiny_lilyco::ui::{tokens, Color, RowBody, Theme, TraceRow};

fn main() {
    let theme = Theme::default_theme();
    println!("lilyco-agent 0.1.0");
    println!("theme      : {} [{}]", theme.label, theme.id);
    print_palette(&theme);
    print_scale();
    print_trace();
    print_css(&theme);
}

fn print_palette(theme: &Theme) {
    let p = &theme.palette;
    println!("background : {}", rgb(p.background));
    println!("foreground : {}", rgb(p.foreground));
    println!("primary    : {}", rgb(p.primary));
    println!("border     : {}", rgb(p.border));
    println!("selection  : {}", rgb(p.selection));
}

fn rgb(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn print_scale() {
    println!("grid       : {}px", tokens::GRID);
    println!(
        "radius     : sm {} / md {} / lg {} / xl {}",
        tokens::RADIUS_SM,
        tokens::RADIUS_MD,
        tokens::RADIUS_LG,
        tokens::RADIUS_XL
    );
    println!(
        "spacing    : {} {} {} {} {}",
        tokens::SPACE_1,
        tokens::SPACE_2,
        tokens::SPACE_3,
        tokens::SPACE_4,
        tokens::SPACE_6
    );
}

fn print_trace() {
    let row = TraceRow::new(3, Phase::Act, "edit_file", RowBody::Diff { added: 12, removed: 4 });
    println!("trace      : {}", row.summary_buf().as_str());
    println!("status     : {}", row.status().label());
}

fn print_css(theme: &Theme) {
    let mut css = heapless::String::<2048>::new();
    if theme.emit_css(&mut css).is_ok() {
        println!();
        println!("--- theme.css ---");
        print!("{}", css.as_str());
    }
}
