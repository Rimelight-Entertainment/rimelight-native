//! Self-update for installed copies (REL.2 and REL.3, decision 0047). The updater, the feed, the minimum-version check and the
//! start-up window's logic are the shared crate `rimelight_update` (rimelight-rs). This module keeps what is Space's own: the
//! feed, the channel, the start-up window (`space_update_slint::UpdateWindow`) and the three entry points `main` calls.
//!
//! An installed copy is one that `Setup.exe` (Velopack) put under `%LOCALAPPDATA%`. `gate` runs before the app shell opens:
//! it checks the feed, and when a newer version exists it downloads it, applies it and restarts into it, so nobody
//! uses an old version after a launch. The owner chose that the check must succeed before the app opens: when it fails, the
//! window shows the error with Try again and Quit, and the app does not start. A copy run from `cargo run` is not
//! installed, so the updater is `None` and the gate lets it through.
//!
//! The feed is the website Worker route `/v1/releases/win/` (files in R2 under `space/releases/win/`). Next to Velopack's
//! own files it carries `min-version.win.txt`: one version line. `watch` re-checks it while the app runs, and an
//! installed copy below it updates and restarts at once (a backend change that old clients cannot work with).

use std::sync::Arc;

use rimelight_update::{Config, DEFAULT_WATCH_INTERVAL, Feed, Updater};

/// The update feed. `RIMELIGHT_UPDATE_URL` overrides it (a local folder served over HTTP, or staging).
const DEFAULT_FEED_URL: &str = "https://rimelight.com/v1/releases/win";
/// The Velopack channel this build is packaged for (`vpk pack --channel`), and the channel of its minimum-version file.
const CHANNEL: &str = "win";

// The start-up window: defines `start_up_gate(Option<Arc<Updater>>) -> bool` in this module.
rimelight_update::update_window_gate!(space_update_slint::UpdateWindow, "Rimelight Space");

/// Runs the Velopack install, update and uninstall hooks. Must be the first call in `main`: for a hook run it does its
/// work and exits the process. Velopack makes the shortcuts (see the pack command), so there is no hook work of our own.
pub fn init() {
    rimelight_update::init(|app| app);
}

/// The updater for this copy, or `None` when this run is not installed (a `cargo run` build).
fn updater() -> Option<Arc<Updater>> {
    let url =
        std::env::var("RIMELIGHT_UPDATE_URL").unwrap_or_else(|_| DEFAULT_FEED_URL.to_string());
    Updater::new(Config {
        feed: Feed::Http { url },
        minimum_version_channel: Some(CHANNEL.to_string()),
    })
    .map(Arc::new)
}

/// Runs before the app shell opens. Returns `true` when the app should start: this copy is not installed, or it is up to
/// date. Returns `false` when the user quit the update window after a failed check.
pub fn gate() -> bool {
    start_up_gate(updater())
}

/// While the app runs: every half hour, when this copy is below the feed's minimum version, update and restart at once.
/// An update that is not required waits for the next start, where `gate` applies it.
pub fn watch() {
    if let Some(updater) = updater() {
        updater.watch(DEFAULT_WATCH_INTERVAL);
    }
}
