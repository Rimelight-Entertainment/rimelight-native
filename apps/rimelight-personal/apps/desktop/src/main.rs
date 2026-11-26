//! The Rimelight Personal desktop app: a thin entry. The window, sign-in and every page are in `rimelight_personal_ui_slint`.
//!
//! Run: `cargo run -p personal_desktop`.
//!
//! Flags (all optional):
//! - `--route <path>`: the page shown at start, a concrete path such as `/finances/bills` (default `/`).
//! - `--theme <light|dark|system>`: the theme (default system).
//!
//! Installed Windows copies (Velopack) check for a newer version before the window opens and update first. The log file is
//! `%LOCALAPPDATA%\rimelight-personal\desktop.log`.

// Release builds are GUI apps: no console window. Debug builds keep the console for `cargo run` logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use rimelight_personal_nav::parse;
use rimelight_ui_kit::theme::Mode;

const TOOL: &str = "personal_desktop";

// Installer hooks, the start-up update check and the forced-update watcher (Windows installed copies only).
#[cfg(windows)]
mod update;

fn main() -> ExitCode {
    // First, before anything else: the Velopack install, update and uninstall hooks run here and exit the process.
    #[cfg(windows)]
    {
        update::init();
        set_log_file();
    }

    let mut route = "/".to_owned();
    let mut mode = Mode::System;
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut args = raw.iter();
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            eprintln!("{TOOL}: {flag} needs a value");
            return ExitCode::from(2);
        };
        match flag.as_str() {
            "--route" => route = value.clone(),
            "--theme" => {
                mode = match value.as_str() {
                    "light" => Mode::Light,
                    "dark" => Mode::Dark,
                    "system" => Mode::System,
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
    if parse(&route).is_none() {
        eprintln!("{TOOL}: --route {route:?} is not a known page");
        return ExitCode::from(2);
    }

    // An installed copy checks the feed first and updates before it opens. A copy that is not installed passes straight
    // through. When the check fails and the user quits, the app does not start.
    #[cfg(windows)]
    {
        if !update::gate() {
            return ExitCode::SUCCESS;
        }
        update::watch();
    }

    match rimelight_personal_ui_slint::shell::run(&route, mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Sends the log lines (`rimelight_platform::log_line`) to `%LOCALAPPDATA%\rimelight-personal\desktop.log`. Without the folder, the
/// lines are dropped.
#[cfg(windows)]
fn set_log_file() {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let folder = std::path::PathBuf::from(local).join("Rimelight Personal");
    if std::fs::create_dir_all(&folder).is_ok() {
        rimelight_platform::set_log_file(folder.join("desktop.log"));
    }
}
