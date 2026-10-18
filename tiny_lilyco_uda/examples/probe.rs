//! Print what UniDesktop/SDK reports on this machine.
//!
//! Used by the manual CI job so a human can eyeball real values.

use tiny_lilyco_uda::probe;

fn main() {
    let facts = probe();
    println!("theme    : {}", facts.theme.label());
    println!("accent   : {:?}", facts.accent);
    println!("caps     : {} (0b{:b})", facts.capability_count(), facts.capabilities);
    println!("degraded : {:?}", facts.degraded);
    println!("ui theme : {}", facts.preferred_theme().id);
    println!("chip     : {}", facts.chip_label());
}
