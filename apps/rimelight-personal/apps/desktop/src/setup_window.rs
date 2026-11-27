//! The set-up window for a copy that is not set up yet: the owner signs in to Cloudflare, the backend is set up in that
//! account, and the settings are saved. The same steps as `personal_desktop setup`, with the window showing progress.
//!
//! The sign-in and the set-up block (they wait for the browser and call the API), so they run on a worker thread and
//! write their progress into the window with `upgrade_in_event_loop`.

use std::path::{Path, PathBuf};

use slint::ComponentHandle;
use std::sync::{Arc, Mutex};

use rimelight_personal_core::cf_oauth;
use rimelight_personal_core::SessionStore;
use rimelight_personal_core::cloudflare_api::{BearerCloudflare, only_account, user_email};
use rimelight_personal_core::provision::{Plan, provision};
use rimelight_personal_core::settings::{self, Settings};
use rimelight_personal_core::{InstanceConfig, signin::open_system_browser};

use crate::windows_ui::SetupWindow;

/// The schema the set-up applies to an empty database (the same file the migrations are generated from).
const SCHEMA_SQL: &str = include_str!("../../../worker/drizzle/0000_personal_backend.sql");

/// Where the Worker bundle is, next to the installed app. Set-up reads it from there.
fn bundle_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("worker").join("fetch.js")))
        .unwrap_or_else(|| PathBuf::from("worker/fetch.js"))
}

/// Shows the set-up window and returns once the copy is set up. An error means the owner closed the window first.
pub fn run() -> Result<InstanceConfig, String> {
    let path = settings::settings_path().ok_or_else(|| "no settings folder on this computer".to_owned())?;
    let ui = SetupWindow::new().map_err(|e| e.to_string())?;
    let finished: Arc<Mutex<Option<Settings>>> = Arc::new(Mutex::new(None));

    {
        let weak = ui.as_weak();
        let finished = finished.clone();
        ui.on_start(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_failed(false);
            ui.set_busy(true);
            ui.set_status("Opening the Cloudflare sign-in page. Sign in and approve access in your browser.".into());

            let weak = weak.clone();
            let finished = finished.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                let progress = |message: &str| {
                    let message = message.to_owned();
                    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(message.into()));
                };
                match set_up(&bundle_path(), &progress) {
                    Ok(saved) => match settings::save(&path, &saved) {
                        Ok(()) => {
                            // A sign-in kept from before this set-up belongs to another instance: sign in again.
                            let _ = rimelight_personal_core::auth::KeyringStore.clear();
                            *finished.lock().unwrap_or_else(|p| p.into_inner()) = Some(saved);
                            let _ = weak.upgrade_in_event_loop(|ui| {
                                let _ = ui.hide();
                            });
                        }
                        Err(e) => fail(&weak, e),
                    },
                    Err(e) => fail(&weak, e),
                }
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_quit(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
        });
    }

    show_centered(&ui).map_err(|e| e.to_string())?;
    ui.run().map_err(|e| e.to_string())?;

    let saved = finished
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .ok_or_else(|| "set-up was not finished".to_owned())?;
    saved.to_instance()
}

/// Shows an error in the window and lets the owner try again.
fn fail(weak: &slint::Weak<SetupWindow>, message: String) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_failed(true);
        ui.set_busy(false);
        ui.set_status(message.into());
    });
}

/// Signs in, sets up the backend in the account, and returns the settings to save. `progress` gets a line per step.
fn set_up(bundle: &Path, progress: &dyn Fn(&str)) -> Result<Settings, String> {
    let bundle = std::fs::read(bundle).map_err(|_| {
        format!("The Worker bundle is missing ({}). Reinstall Rimelight Personal.", bundle.display())
    })?;

    let session = cf_oauth::sign_in(open_system_browser).map_err(|e| e.to_string())?;
    progress("Setting up the backend in your Cloudflare account. This takes a minute.");

    let cf = BearerCloudflare::new(session.token);
    let email = user_email(&cf).map_err(|e| e.to_string())?;
    // The Allow page chose the account, so the token covers only that one.
    let account_id = only_account(&cf).map_err(|e| e.to_string())?;
    let plan = Plan {
        account_id,
        owner_email: email,
        bundle,
        schema_sql: SCHEMA_SQL.to_owned(),
    };
    let out = provision(&cf, &plan).map_err(|e| e.to_string())?;
    progress("Saving the connection.");

    Ok(Settings {
        api_base: out.api_base,
        access_aud: out.access_aud,
        access_team_origin: out.team_domain,
    })
}

/// Shows the window in the middle of the screen. Slint places a new window at the top-left, so the position is set from
/// the screen size once the window's own size is known.
#[cfg(windows)]
#[allow(unsafe_code)]
fn show_centered(ui: &SetupWindow) -> Result<(), slint::PlatformError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    ui.show()?;
    // SAFETY: GetSystemMetrics only reads the display size; it takes no pointers.
    let (screen_w, screen_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let size = ui.window().size();
    let x = ((screen_w - size.width as i32) / 2).max(0);
    let y = ((screen_h - size.height as i32) / 2).max(0);
    ui.window().set_position(slint::PhysicalPosition::new(x, y));
    Ok(())
}

/// Elsewhere, the window keeps the placement the system gives it.
#[cfg(not(windows))]
fn show_centered(ui: &SetupWindow) -> Result<(), slint::PlatformError> {
    ui.show()
}
