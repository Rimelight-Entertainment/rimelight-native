//! The web app: the Rimelight Personal Slint shell in a browser canvas.
//!
//! Build: `trunk build --release` in `apps/web` (needs trunk and the `wasm32-unknown-unknown` target). The instance is read
//! at build time from `RIMELIGHT_API_BASE`, `RIMELIGHT_ACCESS_AUD` and `RIMELIGHT_ACCESS_TEAM_ORIGIN` (see
//! `InstanceConfig::from_env`). Trunk loads the wasm and calls `web_main` on start, which opens the shell on the page's
//! canvas. Native builds print a hint: the desktop app (`apps/desktop`) is the native entry.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Opens the shell at the root route, following the system theme. Runs until the page closes.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_main() {
    let mode = rimelight_ui_kit::theme::Mode::System;
    let instance = rimelight_personal_core::InstanceConfig::from_env()
        .unwrap_or_else(|message| panic!("Rimelight Personal is not configured: {message}"));
    rimelight_personal_ui_slint::shell::run("/", mode, instance, rimelight_personal_ui_slint::shell::About { version: env!("CARGO_PKG_VERSION").to_owned(), check: None }).expect("the Rimelight Personal shell failed to run");
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "rimelight-personal-web is a wasm32 target; build it with `trunk build` from apps/web, or run apps/desktop"
    );
}
