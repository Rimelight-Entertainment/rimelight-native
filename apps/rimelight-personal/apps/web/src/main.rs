//! The web app: the idantity Slint shell in a browser canvas.
//!
//! Build: `trunk build --release` in `apps/web` (needs trunk and the `wasm32-unknown-unknown` target). Trunk loads the
//! wasm and calls `web_main` on start, which opens the shell on the page's canvas. Native builds print a hint: the desktop
//! app (`apps/desktop`) is the native entry.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Opens the shell at the root route, following the system theme. Runs until the page closes.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    let mode = rimelight_ui_kit::theme::Mode::System;
    idantity_ui_slint::shell::run("/", mode).expect("the idantity shell failed to run");
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "idantity-web is a wasm32 target; build it with `trunk build` from apps/web, or run apps/desktop"
    );
}
