//! The Rimelight Personal desktop app: a thin entry. The window, sign-in and every page are in `rimelight_personal_ui_slint`.
//!
//! Run: `cargo run -p personal_desktop`.
//!
//! Flags (all optional):
//! - `--route <path>`: the page shown at start, a concrete path such as `/finances/bills` (default `/`).
//! - `--theme <light|dark|system>`: the theme (default system).
//!
//! The instance comes from the environment: `RIMELIGHT_API_BASE`, `RIMELIGHT_ACCESS_AUD`, `RIMELIGHT_ACCESS_TEAM_ORIGIN`.
//!
//! Installed Windows copies (Velopack) check for a newer version before the window opens and update first. The log file is
//! `%LOCALAPPDATA%\rimelight-personal\desktop.log`.

// Release builds are GUI apps: no console window. Debug builds keep the console for `cargo run` logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use rimelight_personal_core::InstanceConfig;
use rimelight_personal_nav::parse;
use rimelight_ui_kit::theme::Mode;

// The Slint windows (ui/app.slint, compiled by build.rs): the set-up window and, on Windows, the start-up update window.
#[allow(unsafe_code)]
mod windows_ui {
    slint::include_modules!();
}

const TOOL: &str = "personal_desktop";

// Installer hooks, the start-up update check and the forced-update watcher (Windows installed copies only).
mod setup_window;
/// Only the Windows installed copy has an update check.
#[cfg(windows)]
mod update;

fn main() -> ExitCode {
    // First, before anything else: the Velopack install, update and uninstall hooks run here and exit the process.
    #[cfg(windows)]
    {
        update::init();
        set_log_file();
    }

    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("setup") {
        return run_setup(&raw[1..]);
    }

    let mut route = "/".to_owned();
    let mut mode = Mode::System;
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

    // The instance this app connects to: the RIMELIGHT_* variables when they are all set (development), otherwise the
    // settings that `personal_desktop setup` saved. Nothing is defaulted.
    let instance = match instance_from_env_or_settings() {
        Ok(instance) => instance,
        Err(message) => {
            eprintln!("{TOOL}: {message}");
            return ExitCode::from(2);
        }
    };
    match rimelight_personal_ui_slint::shell::run(&route, mode, instance) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The three RIMELIGHT_* variables when they are all set; otherwise the settings saved by setup.
fn instance_from_env_or_settings() -> Result<InstanceConfig, String> {
    let vars = ["RIMELIGHT_API_BASE", "RIMELIGHT_ACCESS_AUD", "RIMELIGHT_ACCESS_TEAM_ORIGIN"];
    if vars.iter().all(|name| std::env::var_os(name).is_some()) {
        return InstanceConfig::from_env();
    }
    let path = rimelight_personal_core::settings::settings_path()
        .ok_or_else(|| "no settings folder on this computer".to_owned())?;
    match rimelight_personal_core::settings::load(&path)? {
        Some(saved) => saved.to_instance(),
        None => crate::setup_window::run(),
    }
}

/// `personal_desktop setup --owner-email <email> --bundle <worker.js>`: signs in to Cloudflare in the browser, sets up
/// the backend in that account (reusing what exists), and saves where this copy connects.
fn run_setup(args: &[String]) -> ExitCode {
    use rimelight_personal_core::cf_oauth;
    use rimelight_personal_core::cloudflare_api::BearerCloudflare;
    use rimelight_personal_core::provision::{Plan, provision};
    use rimelight_personal_core::settings::{self, Settings};

    let mut owner = None;
    let mut bundle = None;
    let mut account = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next().cloned();
        match (flag.as_str(), value) {
            ("--owner-email", Some(v)) => owner = Some(v),
            ("--bundle", Some(v)) => bundle = Some(v),
            ("--account", Some(v)) => account = Some(v),
            _ => {
                eprintln!("{TOOL} setup: usage: setup --owner-email <email> --bundle <worker.js> --account <account-id>");
                return ExitCode::from(2);
            }
        }
    }
    let Some(owner) = owner else {
        eprintln!("{TOOL} setup: needs --owner-email");
        return ExitCode::from(2);
    };
    // The worker bundle ships next to the app (the installer copies it), unless a path is given.
    let bundle = bundle.map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("worker").join("fetch.js")))
            .unwrap_or_else(|| std::path::PathBuf::from("worker/fetch.js"))
    });
    let bundle = match std::fs::read(&bundle) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{TOOL} setup: could not read the bundle: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("Opening the Cloudflare sign-in page...");
    let session = match cf_oauth::sign_in(rimelight_personal_core::signin::open_system_browser) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("{TOOL} setup: {e}");
            return ExitCode::FAILURE;
        }
    };

    let cf = BearerCloudflare::new(session.token);
    // The account: the one given, or the only one this sign-in can see.
    let account = match account {
        Some(id) => id,
        None => match rimelight_personal_core::cloudflare_api::only_account(&cf) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("{TOOL} setup: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let plan = Plan {
        account_id: account,
        owner_email: owner,
        bundle,
        schema_sql: include_str!("../../../worker/drizzle/0000_personal_backend.sql").to_owned(),
    };
    let out = match provision(&cf, &plan) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("{TOOL} setup: {e}");
            return ExitCode::FAILURE;
        }
    };

    let Some(path) = settings::settings_path() else {
        eprintln!("{TOOL} setup: no settings folder on this computer");
        return ExitCode::FAILURE;
    };
    let saved = Settings {
        api_base: out.api_base,
        access_aud: out.access_aud,
        access_team_origin: out.team_domain,
    };
    if let Err(e) = settings::save(&path, &saved) {
        eprintln!("{TOOL} setup: {e}");
        return ExitCode::FAILURE;
    }
    println!("Set up. This copy now connects to {}", saved.api_base);
    ExitCode::SUCCESS
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
