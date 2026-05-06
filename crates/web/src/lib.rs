//! Leptos frontend for the Cyberpunk RED CRPG. WASM only.
//!
//! The real Leptos shell lands in Phase 8. WP-000 ships only the wasm-pack
//! build target so CI can verify the WASM toolchain end-to-end.

use wasm_bindgen::prelude::*;

/// WP-000 placeholder export. Replaced once WP-800 lands the Leptos shell.
#[wasm_bindgen]
pub fn placeholder() {}
