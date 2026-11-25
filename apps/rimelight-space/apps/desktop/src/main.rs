//! The desktop app: opens the Slint app shell in one window titled "Rimelight Space".
//!
//! Run: `cargo run -p rimelight_desktop`. The window and its wiring live in `space_ui` (`slint_ui::app_shell`).
//!
//! The open workspaces are loaded from `<app data>/rimelight-space/workspaces.txt` at start and saved there about a second
//! after each change, and on close (planned feature B3). Without an app-data folder they are kept in memory only.
//!
//! Flags (all optional):
//! - `--route <path>`: a page to open as a new tab (a concrete path such as `/calendar`). Without it the saved workspaces
//!   open as they were, or one Home workspace when nothing is saved.
//! - `--theme <light|dark|system>`: the theme (default system).
//!
//! Installed Windows copies (Velopack, decision 0047) check for a newer version before the window opens and update first.

// Release builds are GUI apps: no console window. Debug builds keep the console for `cargo run` logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod popout;

// Installer hooks, the start-up update check and the forced-update watcher (REL.2 and REL.3, decision 0047).
#[cfg(windows)]
mod update;

use slint::ComponentHandle as _;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use space_client::api::ApiClient;
use space_client::session_store::{KeyringStore, SessionStore};
use space_nav::parse;
use space_ui::slint_ui::SessionPhase;
use space_ui::slint_ui::app_shell;
use space_ui::slint_ui::session_driver;
use space_ui::slint_ui::workspace::{APP_FOLDER, FileStore, MemoryStore, Store};
use space_ui::theme::ThemeMode;

const TOOL: &str = "rimelight_desktop";

/// The Rimelight Space API. `RIMELIGHT_API_URL` overrides it (for a local or staging backend).
const DEFAULT_API_URL: &str = "https://rimelight.com";

fn main() -> ExitCode {
    // First, before anything else: the Velopack install, update and uninstall hooks run here and exit the process.
    #[cfg(windows)]
    update::init();
    let mut route: Option<String> = None;
    let mut mode = ThemeMode::System;
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut args = raw.iter();
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            eprintln!("{TOOL}: {flag} needs a value");
            return ExitCode::from(2);
        };
        match flag.as_str() {
            "--route" => route = Some(value.clone()),
            "--theme" => {
                mode = match value.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    other => {
                        eprintln!("{TOOL}: --theme must be light, dark or system, got {other:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            other => {
                eprintln!("{TOOL}: unknown argument {other:?}");
                return ExitCode::from(2);
            }
        }
    }
    if let Some(route) = &route
        && parse(route).is_none()
    {
        eprintln!("{TOOL}: --route {route:?} is not a known page");
        return ExitCode::from(2);
    }
    // An installed copy checks the feed first and updates before it opens (decision 0047). A copy that is not installed,
    // such as `cargo run`, passes straight through. When the check fails and the user quits, the app does not start.
    #[cfg(windows)]
    {
        if !update::gate() {
            return ExitCode::SUCCESS;
        }
        update::watch();
    }
    let store: Rc<dyn Store> = match FileStore::in_app_data(APP_FOLDER) {
        Some(store) => Rc::new(store),
        None => {
            eprintln!("{TOOL}: no app data folder, the workspaces are not saved");
            Rc::new(MemoryStore::default())
        }
    };
    let app = match app_shell::build_with_store(
        route.as_deref(),
        mode,
        Rc::new(app_shell::NoLocation),
        store,
    ) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The window opens on the loading phase; the session driver checks the stored session and sets the next phase.
    app.ui.set_session_phase(SessionPhase::Loading);
    // Files dragged over or dropped on the window go to the canvas page (CV.7). Winit sends one DroppedFile per file, so
    // each dropped file is added on its own call.
    app.ui.window().on_winit_window_event(|_window, event| {
        match event {
            winit::event::WindowEvent::HoveredFile(_) => space_area_canvas::page::drop_hover(true),
            winit::event::WindowEvent::HoveredFileCancelled => {
                space_area_canvas::page::drop_hover(false)
            }
            winit::event::WindowEvent::DroppedFile(path) => {
                space_area_canvas::page::files_dropped(vec![path.clone()], None)
            }
            _ => {}
        }
        EventResult::Propagate
    });
    // The canvas pop-out window's platform hooks (CV.12): window alpha, z-order, owner, click-through and the hotkey.
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    space_area_canvas::popout_platform::install(Box::new(popout::DesktopPopout));
    // Kept until the loop ends: dropping the manager unregisters the hotkey and dropping the timer stops its poll.
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let _popout_hotkey = popout::start_hotkey();
    let base = std::env::var("RIMELIGHT_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());
    let api = Arc::new(ApiClient::new(base));
    let sessions: Arc<dyn SessionStore> = Arc::new(KeyringStore);
    // Kept until the loop ends: dropping it stops the periodic server check.
    let _session = session_driver::attach(&app, api, sessions);
    match app_shell::run_built(&app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            ExitCode::FAILURE
        }
    }
}
