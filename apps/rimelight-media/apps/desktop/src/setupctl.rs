//! The setup window the installer's first run shows: shortcuts, start with Windows, Print Screen, default apps, and
//! whether to open the app now (off by default).

use std::rc::Rc;

use slint::ComponentHandle as _;

use crate::app::{App, show_main, with_app};
use crate::install;
use crate::ui;

pub fn show(app: &Rc<App>) -> bool {
    let Ok(w) = ui::SetupWindow::new() else {
        return false;
    };
    {
        let p = app.prefs.borrow();
        w.set_desktop(p.desktop_shortcut);
        w.set_startup(true);
        w.set_pictures(p.file_types[0]);
        w.set_video(p.file_types[1]);
        w.set_music(p.file_types[2]);
        w.set_text(p.file_types[3]);
    }
    w.set_windows_printscreen_on(install::windows_printscreen_on());
    w.set_take_printscreen(true);
    w.set_open_now(false);
    w.on_finish(|| with_app(finish));
    w.window().on_close_requested(|| {
        with_app(finish);
        slint::CloseRequestResponse::KeepWindowShown
    });
    if w.show().is_err() {
        return false;
    }
    crate::app::center_window(w.window());
    *app.setup.borrow_mut() = Some(w);
    true
}

fn finish(app: &Rc<App>) {
    let Some(w) = app.setup.borrow_mut().take() else {
        return;
    };
    let kinds = [w.get_pictures(), w.get_video(), w.get_music(), w.get_text()];
    let startup = w.get_startup();
    let desktop = w.get_desktop();
    let take_ps = w.get_windows_printscreen_on() && w.get_take_printscreen();
    let open_now = w.get_open_now();
    let _ = w.hide();

    {
        let mut p = app.prefs.borrow_mut();
        p.desktop_shortcut = desktop;
        p.file_types = kinds;
        p.background = startup;
        if take_ps {
            p.printscreen_changed = true;
        }
        p.save();
    }
    if take_ps {
        install::set_windows_printscreen(false);
        crate::app::start_hotkeys(app);
    }
    install::set_shortcuts(desktop);
    let _ = install::register_file_types(kinds);
    install::set_run_at_login(startup.then(install::launcher_path).as_deref());
    crate::capturectl::ensure_tray(app, startup);
    crate::settingsctl::refresh(app);
    if kinds.iter().any(|k| *k) {
        install::open_default_apps_settings();
    }
    if open_now {
        show_main(app);
        crate::app::center_window(app.ui.window());
    } else if !startup {
        let _ = slint::quit_event_loop();
    }
}
