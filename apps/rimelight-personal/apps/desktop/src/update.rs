//! Self-update for installed Windows copies. The updater, the feed and the start-up window's logic are the shared crate
//! `rimelight_update`; this module names the Rimelight Personal feed and the window and gives `main` its three entry points.
//!
//! The feed is not set yet: `RIMELIGHT_PERSONAL_UPDATE_URL` names it (a Velopack HTTP feed). Until it is set, the updater is `None`
//! and the gate lets the app start, the same as a copy run from `cargo run`.

use std::sync::Arc;

use rimelight_update::{Config, DEFAULT_WATCH_INTERVAL, Feed, Updater};

/// The environment variable that names the update feed.
const FEED_ENV: &str = "RIMELIGHT_PERSONAL_UPDATE_URL";
/// The Velopack channel this build is packaged for, and the channel of its minimum-version file.
const CHANNEL: &str = "stable";

// Defines `start_up_gate(Option<Arc<Updater>>) -> bool` in this module. The window is `crate::windows_ui::UpdateWindow`.
rimelight_update::update_window_gate!(crate::windows_ui::UpdateWindow, "Rimelight Personal");

/// Runs the Velopack install, update and uninstall hooks. Must be the first call in `main`. Velopack makes the shortcuts
/// (see the pack command), so there is no hook work of our own.
pub fn init() {
    rimelight_update::init(|app| app);
}

/// The Personal update feed: its own folder in the rimelight.com bucket (see release-personal.yml). Not the GitHub
/// releases, which carry other apps' packages. `RIMELIGHT_PERSONAL_UPDATE_URL` overrides it for testing.
const FEED_URL: &str = "https://rimelight.com/v1/apps/personal/releases/win";

fn updater() -> Option<Arc<Updater>> {
    let url = std::env::var(FEED_ENV).unwrap_or_else(|_| FEED_URL.to_owned());
    let feed = Feed::Http { url };
    Updater::new(Config {
        feed,
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

/// The About overlay's update check for an installed copy: checks now, and installs a downloaded update when the app
/// closes. `None` for a copy run from the build, which has nothing to update.
pub fn about_check() -> Option<std::sync::Arc<dyn Fn() -> String + Send + Sync>> {
    updater()?;
    Some(std::sync::Arc::new(|| {
        let Some(updater) = updater() else {
            return "Updates are checked in the installed app.".to_owned();
        };
        match updater.check_and_download(&|_| {}) {
            rimelight_update::Outcome::UpToDate => "You have the latest version.".to_owned(),
            rimelight_update::Outcome::Ready(info) => {
                updater.apply_on_exit(&info);
                "An update is downloaded. It installs when you close the app.".to_owned()
            }
            rimelight_update::Outcome::Failed(message) => message,
        }
    }))
}
