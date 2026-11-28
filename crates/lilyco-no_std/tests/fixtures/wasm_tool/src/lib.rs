//! Minimal Extism plugin used by the CI smoke test.
//!
//! It exists to prove the host side: if this module runs, any PDK-built tool
//! (ast-grep, a formatter, a language server shim) runs the same way.

use extism_pdk::*;

#[plugin_fn]
pub fn run(input: String) -> FnResult<String> {
    Ok(format!("wasm saw: {input}"))
}
