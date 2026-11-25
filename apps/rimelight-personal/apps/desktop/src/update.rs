//! Self-update for installed Windows copies. The updater, the feed and the start-up window's logic are the shared crate
//! `rimelight_update`; this module names the idantity feed and the window and gives `main` its three entry points.
//!
//! The feed is not set yet: `IDANTITY_UPDATE_URL` names it (a Velopack HTTP feed). Until it is set, the updater is `None`
//! and the gate lets the app start, the same as a copy run from `cargo run`.

use std::sync::Arc;

use rimelight_update::{Config, DEFAULT_WATCH_INTERVAL, Feed, Updater};

/// The environment variable that names the update feed.
const FEED_ENV: &str = "IDANTITY_UPDATE_URL";
/// The Velopack channel this build is packaged for, and the channel of its minimum-version file.
const CHANNEL: &str = "stable";

// The Slint type for the start-up window (ui/update.slint, compiled by build.rs). Only this module may allow the generated
// unsafe code (the crate denies unsafe).
#[allow(unsafe_code)]
mod generated {
    slint::include_modules!();
}

// Defines `start_up_gate(Option<Arc<Updater>>) -> bool` in this module.
rimelight_update::update_window_gate!(generated::UpdateWindow, "idantity");

/// Runs the Velopack install, update and uninstall hooks. Must be the first call in `main`.
pub fn init() {
    rimelight_update::init(|app| app);
}

/// The updater for this copy, or `None` when the feed is not configured or this run is not installed.
fn updater() -> Option<Arc<Updater>> {
    let url = std::env::var(FEED_ENV).ok()?;
    Updater::new(Config {
        feed: Feed::Http { url },
        minimum_version_channel: Some(CHANNEL.to_owned()),
    })
    .map(Arc::new)
}

/// Runs before the app shell opens. `true` when the app should start.
pub fn gate() -> bool {
    start_up_gate(updater())
}

/// While the app runs: re-checks the feed's minimum version every half hour.
pub fn watch() {
    if let Some(updater) = updater() {
        updater.watch(DEFAULT_WATCH_INTERVAL);
    }
}
