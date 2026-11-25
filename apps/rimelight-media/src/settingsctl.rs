//! Settings page: preferences, install and default apps, keys, folders, theme.

use std::rc::Rc;

use slint::ComponentHandle as _;

use crate::app::{App, toast, with_app};
use crate::install;
use crate::ui;

fn settings(app: &Rc<App>) -> ui::Settings<'_> {
    app.ui.global::<ui::Settings>()
}

pub fn wire(app: &Rc<App>) {
    let s = settings(app);
    s.on_changed(|| with_app(save_from_ui));
    s.on_check_updates(|| with_app(|a| crate::app::check_updates(a, true)));
    s.on_open_releases(|| {
        if let Some(url) = crate::releases_page() {
            rimelight_platform::open_uri(&url);
        }
    });
    s.on_open_link(|url| {
        rimelight_platform::open_uri(url.as_str());
    });
    s.on_register_defaults(|| {
        with_app(
            |a| match install::register_defaults(a.prefs.borrow().file_types) {
                Ok(()) => {
                    toast(
                        a,
                        "File types registered. Now pick Rimelight Media in Windows' default apps.",
                    );
                    refresh(a);
                }
                Err(e) => toast(a, format!("Could not register the file types: {e}")),
            },
        )
    });
    s.on_open_default_settings(install::open_default_apps_settings);
    s.on_pick_canvas_folder(|| {
        with_app(|a| {
            let current = a.prefs.borrow().canvas_dir.clone();
            if let Some(dir) = rfd::FileDialog::new().set_directory(current).pick_folder() {
                {
                    let mut p = a.prefs.borrow_mut();
                    p.canvas_dir = dir;
                    p.save();
                }
                refresh(a);
            }
        })
    });
    s.on_pick_folder(|screenshots| {
        with_app(move |a| {
            let current = if screenshots {
                a.prefs.borrow().screenshot_dir.clone()
            } else {
                a.prefs.borrow().recording_dir.clone()
            };
            if let Some(dir) = rfd::FileDialog::new().set_directory(current).pick_folder() {
                {
                    let mut p = a.prefs.borrow_mut();
                    if screenshots {
                        p.screenshot_dir = dir;
                    } else {
                        p.recording_dir = dir;
                    }
                    p.save();
                }
                refresh(a);
            }
        })
    });
    refresh(app);
}

pub fn refresh(app: &Rc<App>) {
    let s = settings(app);
    let p = app.prefs.borrow();
    s.set_theme(p.theme);
    s.set_background(p.background);
    s.set_start_with_windows(start_with_windows_on());
    s.set_printscreen_ours(!install::windows_printscreen_on());
    s.set_snip_copy(p.snip_copy);
    s.set_snip_save(p.snip_save);
    s.set_snip_open(p.snip_open);
    s.set_rec_system_audio(p.rec_system_audio);
    s.set_rec_mic(p.rec_mic);
    s.set_rec_fps(p.rec_fps);
    s.set_screenshot_folder(p.screenshot_dir.display().to_string().into());
    s.set_recording_folder(p.recording_dir.display().to_string().into());
    s.set_canvas_folder(p.canvas_dir.display().to_string().into());
    drop(p);
    let version = rimelight_update::installed_version();
    s.set_installed(version.is_some());
    s.set_install_state(
        match &version {
            Some(v) => format!("Version {v}"),
            None => format!("Not installed: this is a build from source (version {}). Install it with RimelightMedia-win-Setup.exe from the releases page, or tools/pack.ps1 for a local installer.", env!("CARGO_PKG_VERSION")),
        }
        .into(),
    );
    let hk = app
        .hotkeys
        .borrow()
        .as_ref()
        .map(|h| h.status.clone())
        .unwrap_or_else(|| "The keys are off (they could not be registered).".into());
    s.set_hotkeys_status(hk.into());
    // The credits table is in ui/settings_page.slint (About).
    s.set_about(format!("Rimelight Media {}", env!("CARGO_PKG_VERSION")).into());
}

/// Whether the start-with-Windows entry (the HKCU Run value that install.rs writes) exists.
#[cfg(windows)]
fn start_with_windows_on() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .and_then(|k| k.get_value::<String, _>(install::APP_NAME))
        .is_ok()
}

#[cfg(not(windows))]
fn start_with_windows_on() -> bool {
    false
}

fn save_from_ui(app: &Rc<App>) {
    let s = settings(app);
    let tray_changed = {
        let mut p = app.prefs.borrow_mut();
        let before = p.background;
        p.theme = s.get_theme();
        p.background = s.get_background();
        p.snip_copy = s.get_snip_copy();
        p.snip_save = s.get_snip_save();
        p.snip_open = s.get_snip_open();
        p.rec_system_audio = s.get_rec_system_audio();
        p.rec_mic = s.get_rec_mic();
        p.rec_fps = s.get_rec_fps();
        p.save();
        before != p.background
    };
    apply_theme(app);
    let ours = s.get_printscreen_ours();
    if ours == install::windows_printscreen_on() {
        install::set_windows_printscreen(!ours);
        {
            let mut p = app.prefs.borrow_mut();
            p.printscreen_changed = ours;
            p.save();
        }
        crate::app::start_hotkeys(app);
    }
    if tray_changed {
        let on = app.prefs.borrow().background;
        crate::capturectl::ensure_tray(app, on);
        toast(
            app,
            if on {
                "Rimelight Media now stays in the tray when you close the window."
            } else {
                "Rimelight Media now quits when you close the window."
            },
        );
    }
    // Start with Windows is its own switch: it only changes the Run entry, not the tray.
    let start = s.get_start_with_windows();
    if start != start_with_windows_on() {
        install::set_run_at_login(start.then(install::launcher_path).as_deref());
    }
}

pub fn apply_theme(app: &Rc<App>) {
    let mode = match app.prefs.borrow().theme {
        0 => rimelight_ui_kit::theme::Mode::Light,
        1 => rimelight_ui_kit::theme::Mode::Dark,
        _ => rimelight_ui_kit::theme::Mode::System,
    };
    ui::theme::set_mode(&app.ui, mode);
    // The tray menu is a Windows menu: it follows the same choice (1 like Windows, 2 dark, 3 light).
    rimelight_platform::set_menu_theme(match app.prefs.borrow().theme {
        0 => 3,
        1 => 2,
        _ => 1,
    });
}
