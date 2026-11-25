//! The app: one shared state on the UI thread, reached from callbacks and from worker threads (through
//! `slint::invoke_from_event_loop` and `with_app`).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::files::{self, Kind};
use crate::prefs::{self, Prefs};
use crate::ui;
use crate::{cameractl, capturectl, filesctl, mediactl, settingsctl, textctl, viewer};

pub struct App {
    pub ui: ui::AppWindow,
    pub prefs: RefCell<Prefs>,
    pub recents: RefCell<Vec<PathBuf>>,
    pub view: RefCell<viewer::ViewState>,
    pub media: RefCell<mediactl::MediaState>,
    pub text: RefCell<textctl::TextState>,
    pub cap: RefCell<capturectl::CaptureState>,
    pub camera: RefCell<cameractl::CameraState>,
    pub explorer: RefCell<filesctl::ExplorerState>,
    pub tray: RefCell<Option<ui::Tray>>,
    pub hotkeys: RefCell<Option<crate::hotkeys::Hotkeys>>,
    /// A downloaded update waiting to be applied.
    pub update: RefCell<Option<Box<rimelight_update::UpdateInfo>>>,
    /// The setup window of the installer's first run, while it is open.
    pub setup: RefCell<Option<ui::SetupWindow>>,
    /// True when started with --background (no window until asked for).
    pub background_start: Cell<bool>,
    pub toast_timer: slint::Timer,
    pub tick: slint::Timer,
    pub slow_tick: slint::Timer,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs `f` with the app, if it exists (UI thread only).
pub fn with_app(f: impl FnOnce(&Rc<App>)) {
    let app = APP.with(|a| a.borrow().clone());
    if let Some(app) = app {
        f(&app);
    }
}

/// Stops playback and recordings and releases the app while the thread is still alive (sound devices must not be
/// closed during process exit).
pub fn shutdown() {
    let app = APP.with(|a| a.borrow_mut().take());
    if let Some(app) = app {
        app.media.borrow_mut().take_player();
        cameractl::stop_all_now(&app);
        capturectl::stop_all_now(&app);
        app.tray.borrow_mut().take();
        app.hotkeys.borrow_mut().take();
        // Quitting with a downloaded update: it is applied on the way out, so the next start is new.
        if let (Some(info), Some(updater)) = (app.update.borrow_mut().take(), crate::updater()) {
            updater.apply_on_exit(&info);
        }
    }
}

/// Runs `f` with the app on the UI thread, from any thread.
pub fn post(f: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_app(f));
}

pub fn build(background: bool) -> Result<Rc<App>, slint::PlatformError> {
    let ui = ui::AppWindow::new()?;
    let app = Rc::new(App {
        ui,
        prefs: RefCell::new(Prefs::load()),
        recents: RefCell::new(prefs::load_recents()),
        view: RefCell::new(viewer::ViewState::default()),
        media: RefCell::new(mediactl::MediaState::default()),
        text: RefCell::new(textctl::TextState::default()),
        cap: RefCell::new(capturectl::CaptureState::default()),
        camera: RefCell::new(cameractl::CameraState::default()),
        explorer: RefCell::new(filesctl::ExplorerState::default()),
        tray: RefCell::new(None),
        hotkeys: RefCell::new(None),
        update: RefCell::new(None),
        setup: RefCell::new(None),
        background_start: Cell::new(background),
        toast_timer: slint::Timer::default(),
        tick: slint::Timer::default(),
        slow_tick: slint::Timer::default(),
    });
    APP.with(|a| *a.borrow_mut() = Some(Rc::clone(&app)));
    wire_shell(&app);
    viewer::wire(&app);
    mediactl::wire(&app);
    capturectl::wire(&app);
    cameractl::wire(&app);
    settingsctl::wire(&app);
    textctl::wire(&app);
    filesctl::wire(&app);
    refresh_recents(&app);
    settingsctl::apply_theme(&app);
    Ok(app)
}

fn wire_shell(app: &Rc<App>) {
    let shell = app.ui.global::<ui::Shell>();
    shell.on_open_file(|| with_app(open_dialog));
    shell.on_rename(|name| with_app(move |a| rename_current(a, &name)));
    shell.on_open_recent(|i| {
        with_app(|a| {
            if let Some(p) = recent_at(a, i as usize) {
                open_path(a, &p);
            }
        })
    });
    // "" clears every kind; otherwise only the files of that kind ("image", "audio", "video", "text").
    shell.on_clear_recents(|kind: SharedString| {
        with_app(move |a| {
            let kind = kind.as_str();
            {
                let mut list = a.recents.borrow_mut();
                list.retain(|p| {
                    !kind.is_empty() && files::kind_of(p).map(files::kind_name) != Some(kind)
                });
                prefs::save_recents(&list);
            }
            refresh_recents(a);
        })
    });
    shell.on_go(|page| with_app(|a| go(a, &page)));
    shell.on_toggle_full_screen(|| with_app(toggle_full_screen));
    shell.on_apply_update(|| {
        with_app(|a| {
            let info = a.update.borrow().clone();
            if let (Some(info), Some(updater)) = (info, crate::updater()) {
                capturectl::stop_all_now(a);
                if let Err(e) = updater.apply_now(&info) {
                    toast(a, format!("Could not update: {e}"));
                }
            }
        })
    });
    shell.on_key(|text, ctrl, shift, alt| {
        let mut handled = false;
        with_app(|a| handled = on_key(a, &text, ctrl, shift, alt));
        handled
    });

    shell.on_close_tab(|| with_app(close_tab));
    // The cast button opens the cast popup, which searches for devices (src/cast.rs).
    shell.on_cast(|| with_app(crate::cast::refresh));
    let cast = app.ui.global::<ui::Cast>();
    cast.on_refresh(|| with_app(crate::cast::refresh));
    cast.on_pick(|i| with_app(|a| crate::cast::pick(a, i as usize)));
    cast.on_pause(crate::cast::toggle_play);
    cast.on_stop(crate::cast::stop);
    cast.on_system_cast(rimelight_platform::open_cast_panel);

    // Closing the window clears every tab (after asking about unsaved changes): it means "I'm done"; minimise keeps
    // things open. Then stay in the tray when "keep running" is on, otherwise quit.
    app.ui.window().on_close_requested(|| {
        // Each unsaved thing asks in the window (picture, canvas, text) and the close goes on from its answer;
        // finish_close hides the window or quits.
        with_app(|a| {
            viewer::settle_picture(a, |a| {
                viewer::settle_canvas(a, |a| {
                    if textctl::settle(a, textctl::Pending::Quit) {
                        finish_close(a);
                    }
                })
            })
        });
        slint::CloseRequestResponse::KeepWindowShown
    });

    // Files dropped on the window open; hovering highlights the home page drop zone.
    use slint::winit_030::{EventResult, WinitWindowAccessor as _, winit};
    app.ui.window().on_winit_window_event(|_w, event| {
        match event {
            winit::event::WindowEvent::HoveredFile(_) => {
                HOVERED.with(|h| h.set(h.get() + 1));
                with_app(|a| a.ui.global::<ui::Shell>().set_drop_hover(true))
            }
            winit::event::WindowEvent::HoveredFileCancelled => {
                HOVERED.with(|h| h.set(0));
                with_app(|a| a.ui.global::<ui::Shell>().set_drop_hover(false))
            }
            winit::event::WindowEvent::DroppedFile(path) => {
                let path = path.clone();
                with_app(move |a| {
                    a.ui.global::<ui::Shell>().set_drop_hover(false);
                    // An image dropped while marking up (or on a canvas) comes in as a picture mark, or the canvas.
                    let markup =
                        a.ui.global::<ui::Shell>().get_page() == "image" && viewer::in_markup(a);
                    if markup && files::kind_of(&path) == Some(Kind::Image) {
                        viewer::drop_picture(a, &path);
                    } else {
                        queue_drop(a, path.clone());
                    }
                });
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

pub fn go(app: &Rc<App>, page: &str) {
    let shell = app.ui.global::<ui::Shell>();
    let was = shell.get_page();
    if was == "image" && page != "image" {
        viewer::slideshow_stop(app);
    }
    if matches!(was.as_str(), "audio" | "video") && was != page {
        mediactl::leaving(app);
    }
    if was == "camera" && page != "camera" {
        cameractl::leave(app);
    }
    if page == "camera" {
        cameractl::enter(app);
    }
    if page == "voice" {
        capturectl::refresh_takes(app);
    }
    if page == "settings" {
        settingsctl::refresh(app);
    }
    if page == "files" {
        filesctl::enter(app);
    }
    shell.set_page(page.into());
    update_title(app);
}

pub fn update_title(app: &Rc<App>) {
    let shell = app.ui.global::<ui::Shell>();
    // The file's name alone (what the title shows and the rename box edits), or the tab's name when there is no file.
    // The window title (taskbar) adds the app name in Slint.
    let page = shell.get_page();
    let (title, name) = match page.as_str() {
        "image" if app.view.borrow().canvas => ("Canvas".to_string(), String::new()),
        "image" => {
            let name = app
                .view
                .borrow()
                .path
                .as_deref()
                .map(files::file_name)
                .unwrap_or_default();
            (
                if name.is_empty() {
                    "Picture".to_string()
                } else {
                    name.clone()
                },
                name,
            )
        }
        "audio" | "video" => {
            let name = app
                .media
                .borrow()
                .path
                .as_deref()
                .map(files::file_name)
                .unwrap_or_default();
            let tab = if page.as_str() == "video" {
                "Video"
            } else {
                "Audio"
            };
            (
                if name.is_empty() {
                    tab.to_string()
                } else {
                    name.clone()
                },
                name,
            )
        }
        "text" => {
            let t = textctl::title(app);
            (
                if t.is_empty() { "Text".to_string() } else { t },
                String::new(),
            )
        }
        "voice" => ("Voice recorder".to_string(), String::new()),
        "camera" => ("Camera".to_string(), String::new()),
        "settings" => ("Settings".to_string(), String::new()),
        "files" => ("Files".to_string(), String::new()),
        _ => ("Home".to_string(), String::new()),
    };
    shell.set_title_name(name.into());
    shell.set_title(title.into());
}

/// Renames a file that a handle of ours may still hold for a moment. Windows answers ERROR_SHARING_VIOLATION (32) while
/// an open handle without FILE_SHARE_DELETE exists, so this retries it every 50 ms for about 500 ms before giving up.
pub fn rename_file(old: &Path, new: &Path) -> std::io::Result<()> {
    const SHARING_VIOLATION: i32 = 32;
    const STEP: Duration = Duration::from_millis(50);
    let mut tries = 0;
    loop {
        match std::fs::rename(old, new) {
            Err(e) if e.raw_os_error() == Some(SHARING_VIOLATION) && tries < 10 => {
                tries += 1;
                std::thread::sleep(STEP);
            }
            other => return other,
        }
    }
}

/// Renames the file in the Image or Audio/Video tab (Enter in the title). The extension is kept when the name has none.
pub fn rename_current(app: &Rc<App>, typed: &str) {
    let page = app.ui.global::<ui::Shell>().get_page();
    let old = match page.as_str() {
        "image" => app.view.borrow().path.clone(),
        "audio" | "video" => app.media.borrow().path.clone(),
        _ => None,
    };
    let Some(old) = old else {
        toast(app, "Save the picture first, then rename it.");
        return;
    };
    let typed = typed.trim();
    if typed.is_empty() || typed.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        toast(app, "A name can't be empty or contain \\ / : * ? \" < > |");
        return;
    }
    let mut name = typed.to_string();
    if Path::new(&name).extension().is_none()
        && let Some(ext) = old.extension()
    {
        name.push('.');
        name.push_str(&ext.to_string_lossy());
    }
    let Some(dir) = old.parent() else { return };
    let new = dir.join(&name);
    if new == old {
        return;
    }
    if new.exists() {
        toast(app, format!("{name} is already in this folder."));
        return;
    }
    // A playing file is let go first, so Windows lets it be renamed; it is opened again under the new name.
    let playing = page != "image";
    if playing {
        app.media.borrow_mut().take_player();
    }
    if let Err(e) = rename_file(&old, &new) {
        toast(app, format!("Could not rename: {e}"));
        if playing {
            mediactl::open(app, &old);
        }
        return;
    }
    if playing {
        mediactl::open(app, &new);
    } else {
        viewer::renamed(app, &new);
    }
    {
        let mut list = app.recents.borrow_mut();
        list.retain(|p| p != &old);
        prefs::push_recent(&mut list, &new);
        prefs::save_recents(&list);
    }
    refresh_recents(app);
    update_title(app);
    toast(app, format!("Renamed to {name}"));
}

pub fn toast(app: &Rc<App>, text: impl Into<SharedString>) {
    app.ui.global::<ui::Shell>().set_toast(text.into());
    app.toast_timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(3500),
        || with_app(|a| a.ui.global::<ui::Shell>().set_toast(SharedString::new())),
    );
}

/// Shows and raises the main window.
pub fn show_main(app: &Rc<App>) {
    // The tray copy runs on the software renderer: let the caller finish (open the snip, pick the tab), then hand over.
    if app.background_start.get() {
        schedule_handoff(app);
        return;
    }
    let _ = app.ui.show();
    use slint::winit_030::WinitWindowAccessor as _;
    app.ui.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

pub fn toggle_full_screen(app: &Rc<App>) {
    let shell = app.ui.global::<ui::Shell>();
    let on = !shell.get_full_screen();
    app.ui.window().set_fullscreen(on);
    shell.set_full_screen(on);
}

pub fn open_dialog(app: &Rc<App>) {
    let mut all: Vec<&str> = files::IMAGE_EXTS.to_vec();
    all.extend_from_slice(files::VIDEO_EXTS);
    all.extend_from_slice(files::AUDIO_EXTS);
    all.extend_from_slice(files::TEXT_EXTS);
    let picked = rfd::FileDialog::new()
        .set_title("Open")
        .add_filter("Pictures, video, music and text", &all)
        .add_filter("Pictures", files::IMAGE_EXTS)
        .add_filter("Video", files::VIDEO_EXTS)
        .add_filter("Music", files::AUDIO_EXTS)
        .add_filter("Text", files::TEXT_EXTS)
        .add_filter("Everything", &["*"])
        .pick_files();
    if let Some(paths) = picked {
        open_many(app, &paths);
    }
}

/// Opens the files picked together. Several pictures show only those in the filmstrip; one file browses its folder.
pub fn open_many(app: &Rc<App>, paths: &[PathBuf]) {
    let Some(first) = paths.first() else { return };
    if paths.len() > 1 && paths.iter().all(|p| files::kind_of(p) == Some(Kind::Image)) {
        viewer::open_set(app, paths);
        remember(app, first);
    } else if paths.len() > 1 && matches!(files::kind_of(first), Some(Kind::Audio | Kind::Video)) {
        mediactl::open_set(app, paths);
        remember(app, first);
    } else {
        open_path(app, first);
    }
}

pub fn open_path(app: &Rc<App>, path: &Path) {
    match files::kind_of(path) {
        Some(Kind::Image) => viewer::open(app, path),
        Some(Kind::Text) => textctl::open(app, path),
        Some(Kind::Video | Kind::Audio) => mediactl::open(app, path),
        None => {
            // Unknown extension: let FFmpeg try it as media.
            if path.is_file() {
                mediactl::open(app, path);
            } else {
                toast(app, "That file can't be opened.");
                return;
            }
        }
    }
    remember(app, path);
}

pub fn remember(app: &Rc<App>, path: &Path) {
    {
        let mut list = app.recents.borrow_mut();
        prefs::push_recent(&mut list, path);
        prefs::save_recents(&list);
    }
    refresh_recents(app);
}

/// Fills the recent lists: all of them (Home), and one per kind for each tab's empty state. Each item's `index` is its
/// place in the full list, which `Shell.open-recent` takes.
pub fn refresh_recents(app: &Rc<App>) {
    let all: Vec<ui::RecentItem> = app
        .recents
        .borrow()
        .iter()
        .filter(|p| p.exists())
        .enumerate()
        .map(|(i, p)| ui::RecentItem {
            name: files::file_name(p).into(),
            detail: files::folder_name(p).into(),
            kind: files::kind_of(p)
                .map(files::kind_name)
                .unwrap_or("video")
                .into(),
            index: i as i32,
        })
        .collect();
    let of_kind = |kind: &str| -> ModelRc<ui::RecentItem> {
        let items: Vec<ui::RecentItem> = all
            .iter()
            .filter(|r| r.kind.as_str() == kind)
            .cloned()
            .collect();
        ModelRc::new(VecModel::from(items))
    };
    let shell = app.ui.global::<ui::Shell>();
    shell.set_recents_image(of_kind("image"));
    shell.set_recents_audio(of_kind("audio"));
    shell.set_recents_video(of_kind("video"));
    shell.set_recents_text(of_kind("text"));
    shell.set_recents(ModelRc::new(VecModel::from(all)));
}

/// Recent items shown are the existing ones; map a shown index back to the stored list.
pub fn recent_at(app: &Rc<App>, i: usize) -> Option<PathBuf> {
    app.recents
        .borrow()
        .iter()
        .filter(|p| p.exists())
        .nth(i)
        .cloned()
}

fn on_key(app: &Rc<App>, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
    use slint::platform::Key;
    let k = |key: Key| text == SharedString::from(key).as_str();
    let page = app.ui.global::<ui::Shell>().get_page();
    if ctrl && (text == "o" || text == "O") {
        open_dialog(app);
        return true;
    }
    // Ctrl+N: a new 1920 x 1080 white canvas at once. Ctrl+V on the home page: the clipboard picture opens as a canvas
    // (the other pages keep their own paste, in the text editor and the picture page).
    // On the Text tab Ctrl+N is a new text file instead.
    if ctrl && (text == "n" || text == "N") && page == "text" {
        textctl::new_doc(app);
        return true;
    }
    if ctrl && (text == "n" || text == "N") {
        app.ui
            .global::<ui::Shell>()
            .invoke_new_canvas(1, "".into(), "".into(), false);
        return true;
    }
    if ctrl && (text == "v" || text == "V") && page == "home" {
        viewer::paste_new(app);
        return true;
    }
    if k(Key::F11) {
        toggle_full_screen(app);
        return true;
    }
    if k(Key::Escape) {
        if app.ui.global::<ui::Shell>().get_full_screen() {
            toggle_full_screen(app);
            return true;
        }
        if page == "image" && app.ui.global::<ui::Viewer>().get_comparing() {
            viewer::close_compare(app);
            return true;
        }
        if page == "image" && !app.ui.global::<ui::Viewer>().get_mode().is_empty() {
            viewer::set_mode(app, "");
            return true;
        }
        return false;
    }
    // Tab and Shift+Tab step through the top-bar tabs, in order, wrapping at the ends. While a text field has the keyboard
    // the keys belong to it: they do nothing here.
    if k(Key::Tab) || k(Key::Backtab) {
        if app.ui.global::<ui::Shell>().get_field_focus() {
            return true;
        }
        const ORDER: [&str; 6] = ["home", "files", "image", "audio", "video", "text"];
        let back = k(Key::Backtab) || shift;
        let here = ORDER.iter().position(|p| *p == page.as_str());
        let next = match (here, back) {
            (Some(i), true) => (i + ORDER.len() - 1) % ORDER.len(),
            (Some(i), false) => (i + 1) % ORDER.len(),
            // Settings, camera and voice are not in the cycle: Tab goes to the first tab, Shift+Tab to the last.
            (None, true) => ORDER.len() - 1,
            (None, false) => 0,
        };
        go(app, ORDER[next]);
        return true;
    }
    let _ = alt;
    match page.as_str() {
        "image" => viewer::on_key(app, text, ctrl, shift),
        "audio" | "video" => mediactl::on_key(app, text, ctrl, shift),
        "files" => filesctl::on_key(app, text, ctrl, shift),
        _ => false,
    }
}

/// Checks for an update in the background; `manual` reports "up to date" and failures too.
pub fn check_updates(app: &Rc<App>, manual: bool) {
    // One check at a time; a finished download stays until it is applied.
    static IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if app.update.borrow().is_some() {
        return;
    }
    if IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if manual {
        app.ui
            .global::<ui::Settings>()
            .set_update_state("Checking for updates…".into());
    }
    std::thread::spawn(move || {
        // None when updates are off (see crate::updater).
        let outcome = crate::updater()
            .map(|updater| updater.check_and_download(&|_: rimelight_update::Step| {}));
        IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
        post(move |a| {
            use rimelight_update::Outcome;
            let text = match outcome {
                None => "Updates are off: this copy isn't installed, or was built without a GitHub repository.".to_string(),
                Some(Outcome::UpToDate) => "Up to date.".to_string(),
                Some(Outcome::Failed(e)) => e,
                Some(Outcome::Ready(info)) => {
                    let v = info.TargetFullRelease.Version.clone();
                    *a.update.borrow_mut() = Some(info);
                    a.ui.global::<ui::Shell>().set_update_ready(v.clone().into());
                    format!("Version {v} is downloaded. Restart to use it (or it is applied when you quit).")
                }
            };
            a.ui.global::<ui::Settings>()
                .set_update_state(text.clone().into());
            if manual {
                toast(a, text);
            }
        });
    });
}
/// (Re)registers the global Print Screen keys and refreshes their status in Settings.
pub fn start_hotkeys(app: &Rc<App>) {
    app.hotkeys.borrow_mut().take();
    let keys = crate::hotkeys::start(|action| with_app(|a| capturectl::hotkey(a, action)));
    *app.hotkeys.borrow_mut() = keys;
    settingsctl::refresh(app);
}
/// Centres a window on the monitor the pointer is on (Windows opens new windows at its own default spot).
pub fn center_window(window: &slint::Window) {
    let size = window.size();
    if let Some((_, mon)) = rimelight_platform::cursor_monitor() {
        let x = mon.x + (mon.w - size.width as i32).max(0) / 2;
        let y = mon.y + (mon.h - size.height as i32).max(0) / 2;
        window.set_position(slint::PhysicalPosition::new(x, y));
    }
}

/// The main window's handle (owner of the print and share dialogs).
pub fn main_hwnd(app: &Rc<App>) -> isize {
    use slint::winit_030::WinitWindowAccessor as _;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    app.ui
        .window()
        .with_winit_window(|w| match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        })
        .unwrap_or(0)
}
/// The X in the top bar: back to the empty version of this tab (asking first about unsaved changes).
pub fn close_tab(app: &Rc<App>) {
    match app.ui.global::<ui::Shell>().get_page().as_str() {
        "image" => viewer::close_tab(app),
        "audio" | "video" => mediactl::close(app),
        "text" => textctl::close(app),
        _ => {}
    }
    update_title(app);
}

/// The window is closing, with nothing left unsaved: empties the tabs, then quits, or hides the window when the app keeps
/// running in the tray.
pub fn finish_close(app: &Rc<App>) {
    clear_all(app);
    if !app.prefs.borrow().background {
        capturectl::stop_all_now(app);
        let _ = slint::quit_event_loop();
    }
    // Going to the tray: a fresh copy on the software renderer takes over, unless something still runs here.
    // The new copy waits for this process to end (--after) before it takes the start lock.
    let me = std::process::id().to_string();
    if !app.background_start.get()
        && !capturectl::busy(app)
        && spawn_self(&["--background", "--takeover", "--after", me.as_str()])
    {
        rimelight_platform::release_start_lock();
        crate::instance::release();
        rimelight_platform::exit_now(0);
    }
    let _ = app.ui.window().hide();
}

/// Empties every tab and frees what they held (the window was closed).
fn clear_all(app: &Rc<App>) {
    viewer::close(app);
    mediactl::close(app);
    textctl::clear(app);
    if app.ui.global::<ui::Shell>().get_full_screen() {
        toggle_full_screen(app);
    }
    go(app, "home");
}

/// A request from a later start (single instance): a file, a snip, or just the window.
pub fn handle_request(request: crate::instance::Request) {
    use crate::instance::Request;
    with_app(|a| match request {
        Request::Show => {
            show_main(a);
            check_updates(a, false);
        }
        Request::Open(p) => {
            show_main(a);
            queue_open(a, PathBuf::from(p));
            check_updates(a, false);
        }
        Request::Snip(mode) => capturectl::snip(a, &mode),
        Request::Nothing => {}
    });
}
thread_local! {
    static PENDING_OPENS: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    static OPEN_GEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Explorer starts the app once per file when several are opened together; the starts arrive here a few ms apart. They
/// are gathered for a moment and opened as one set, so the filmstrip shows just those files.
pub fn queue_open(app: &Rc<App>, path: PathBuf) {
    // Waits until no file has arrived for 400 ms (the copies Explorer starts hand theirs over one by one).
    PENDING_OPENS.with(|p| p.borrow_mut().push(path));
    let ticket = OPEN_GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    {
        let app = app.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(400), move || {
            if OPEN_GEN.with(|g| g.get()) != ticket {
                return;
            }
            let mut paths = PENDING_OPENS.with(|p| std::mem::take(&mut *p.borrow_mut()));
            paths.dedup();
            open_many(&app, &paths);
        });
    }
}

thread_local! {
    static HANDOFF_PENDING: Cell<bool> = const { Cell::new(false) };
}

fn schedule_handoff(app: &Rc<App>) {
    if HANDOFF_PENDING.with(|h| h.replace(true)) {
        return;
    }
    let app = app.clone();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || handoff(&app));
}

/// The tray copy (software renderer, about 22 MB) is asked for its window: a fresh copy on the GPU renderer starts on the
/// same tab and file, and this one exits. Work that can't move (a recording, unsaved text or canvas) keeps this copy and
/// shows its window on the software renderer instead.
fn handoff(app: &Rc<App>) {
    HANDOFF_PENDING.with(|h| h.set(false));
    let stay = capturectl::busy(app) || textctl::is_dirty(app) || app.view.borrow().canvas;
    let page = app.ui.global::<ui::Shell>().get_page().to_string();
    let file = match page.as_str() {
        "image" => app.view.borrow().path.clone(),
        "audio" | "video" => app.media.borrow().path.clone(),
        "text" => app.text.borrow().path.clone(),
        _ => None,
    };
    // The new copy waits for this process to end (--after) before it takes the start lock.
    let mut args = vec![
        "--takeover".to_string(),
        "--after".to_string(),
        std::process::id().to_string(),
        "--page".to_string(),
        page,
    ];
    if let Some(f) = file {
        args.push(f.display().to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    if !stay && spawn_self(&refs) {
        rimelight_platform::release_start_lock();
        crate::instance::release();
        rimelight_platform::exit_now(0);
    }
    app.background_start.set(false);
    show_main(app);
}

/// Starts this exe again with `args`. True when it started.
fn spawn_self(args: &[&str]) -> bool {
    std::env::current_exe()
        .is_ok_and(|exe| std::process::Command::new(exe).args(args).spawn().is_ok())
}

type AskThen = Box<dyn FnOnce(&Rc<App>, i32)>;

thread_local! {
    static ASK: RefCell<Option<AskThen>> = const { RefCell::new(None) };
}

/// Asks a question in the window's own dialog (never a native message box). `then` gets 0 (primary), 1 (secondary) or 2
/// (Cancel). An empty `secondary` shows only the primary button and Cancel.
pub fn ask(
    app: &Rc<App>,
    title: &str,
    name: &str,
    detail: &str,
    primary: &str,
    secondary: &str,
    then: impl FnOnce(&Rc<App>, i32) + 'static,
) {
    ASK.with(|a| *a.borrow_mut() = Some(Box::new(then)));
    let shell = app.ui.global::<ui::Shell>();
    shell.set_confirm_title(title.into());
    shell.set_confirm_name(name.into());
    shell.set_confirm_detail(detail.into());
    shell.set_confirm_primary(primary.into());
    shell.set_confirm_secondary(secondary.into());
    shell.set_confirm_open(true);
}

/// The answer to an `ask` question, if one is waiting. True when it was handled. The dialog goes back to the default
/// "Save changes?" wording for the next question.
pub fn take_ask_answer(app: &Rc<App>, choice: i32) -> bool {
    let Some(then) = ASK.with(|a| a.borrow_mut().take()) else {
        return false;
    };
    let shell = app.ui.global::<ui::Shell>();
    shell.set_confirm_open(false);
    shell.set_confirm_title("Save changes?".into());
    shell.set_confirm_detail("Your changes will be lost if you don't save them.".into());
    shell.set_confirm_primary("Save".into());
    shell.set_confirm_secondary("Don't save".into());
    then(app, choice);
    true
}

thread_local! {
    static DROPPED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    static HOVERED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A file dropped on the window. Winit sends HoveredFile once per file when the drag enters the window, before the drop,
/// so the number of files in the drop is known: when that many have arrived they open together at once, with no wait.
/// If the count never matches (no hover events), a zero-length timer opens whatever has arrived.
pub fn queue_drop(app: &Rc<App>, path: PathBuf) {
    let expected = HOVERED.with(|h| h.get());
    let ready = DROPPED.with(|d| {
        let mut d = d.borrow_mut();
        d.push(path);
        expected == 0 || d.len() >= expected
    });
    if ready {
        flush_drop(app);
        return;
    }
    let app = app.clone();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || flush_drop(&app));
}

fn flush_drop(app: &Rc<App>) {
    HOVERED.with(|h| h.set(0));
    let paths = DROPPED.with(|d| std::mem::take(&mut *d.borrow_mut()));
    if !paths.is_empty() {
        open_many(app, &paths);
    }
}
