//! Snips, screen recordings and voice recordings, with the snip overlay, the recording bar and the tray icon.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::app::{App, go, open_path, post, remember, show_main, toast, with_app};
use crate::capture::{grab, recorder};
use crate::files;
use crate::picture;
use crate::ui;
use rimelight_picture::Picture;
use rimelight_platform::{ScreenRect, TopWindow};

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Snip,
    Record,
}

struct Snip {
    window: ui::SnipWindow,
    shot: Picture,
    rect: ScreenRect,
    windows: Vec<TopWindow>,
    purpose: Purpose,
    main_was_visible: bool,
    /// Freeform outline so far, as fractions of the screen.
    free: Vec<(f32, f32)>,
}

#[derive(Default)]
pub struct CaptureState {
    snip: Option<Snip>,
    busy: bool,
    screen: Option<recorder::Recording>,
    bar: Option<ui::RecBar>,
    voice: Option<recorder::Recording>,
    /// Markers added during the voice recording (milliseconds on the recording's clock).
    voice_marks: Vec<u64>,
    /// The microphone picked in the voice page: 0 is the default one, then the input devices in order.
    voice_mic: usize,
    /// The file format picked in the voice page (an index into VOICE_FORMATS).
    voice_format: usize,
}

/// The file formats the voice recorder can save.
pub const VOICE_FORMATS: [&str; 4] = ["m4a", "mp3", "wav", "flac"];

pub fn wire(app: &Rc<App>) {
    let shell = app.ui.global::<ui::Shell>();
    shell.on_snip(|mode| with_app(move |a| begin(a, &mode, Purpose::Snip)));
    shell.on_record_screen(|mode| with_app(move |a| begin(a, &mode, Purpose::Record)));
    shell.on_stop_recording(|| with_app(stop_screen));
    let voice = app.ui.global::<ui::Voice>();
    voice.set_folder(app.prefs.borrow().voice_dir.display().to_string().into());
    voice.on_toggle(|| with_app(toggle_voice));
    voice.on_open_take(|i| {
        with_app(move |a| {
            if let Some(p) = takes(a).get(i as usize) {
                open_path(a, p);
            }
        })
    });
    voice.on_open_folder(|| {
        with_app(|a| {
            let dir = a.prefs.borrow().voice_dir.clone();
            let _ = std::fs::create_dir_all(&dir);
            files::reveal(&dir);
        })
    });
    voice.on_pause_toggle(|| with_app(toggle_pause));
    voice.on_add_marker(|| with_app(add_marker));
    voice.on_pick_mic(|i| with_app(move |a| pick_mic(a, i)));
    voice.on_pick_format(|i| with_app(move |a| pick_format(a, i)));
    voice.on_rename(|i, name| with_app(move |a| rename_take(a, i, name.to_string())));
    voice.on_delete(|i| with_app(move |a| delete_take(a, i)));
    voice.on_reveal(|i| {
        with_app(move |a| {
            if let Some(p) = takes(a).get(i as usize) {
                files::reveal(p);
            }
        })
    });
    voice.set_formats(ModelRc::new(VecModel::from(
        VOICE_FORMATS
            .iter()
            .map(|f| SharedString::from(*f))
            .collect::<Vec<_>>(),
    )));
    voice.set_format_index(0);
    voice.set_mic_index(0);
    let gain = app.prefs.borrow().voice_gain_db;
    crate::capture::audio_in::set_mic_gain_db(gain);
    voice.set_gain_db(gain);
    voice.on_set_gain(|db| with_app(move |a| set_gain(a, db, false)));
    voice.on_save_gain(|| {
        with_app(|a| {
            let db = a.ui.global::<ui::Voice>().get_gain_db();
            set_gain(a, db, true);
        })
    });
    app.slow_tick.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        || with_app(slow_tick),
    );
}

/// The hotkey/tray entry points.
pub fn hotkey(app: &Rc<App>, action: crate::hotkeys::Action) {
    use crate::hotkeys::Action;
    match action {
        Action::SnipRegion => begin(app, "region", Purpose::Snip),
        Action::SnipWindow => begin(app, "window", Purpose::Snip),
        Action::SnipScreen => begin(app, "screen", Purpose::Snip),
        Action::ToggleRecording => {
            if app.cap.borrow().screen.is_some() {
                stop_screen(app);
            } else {
                begin(app, "region", Purpose::Record);
            }
        }
    }
}

pub fn snip(app: &Rc<App>, mode: &str) {
    begin(app, mode, Purpose::Snip);
}

pub fn record_toggle(app: &Rc<App>) {
    if app.cap.borrow().screen.is_some() {
        stop_screen(app)
    } else {
        begin(app, "region", Purpose::Record)
    }
}

/// Hides Rimelight Media, grabs every monitor, then shows the overlay (or, for "whole screen" snips, finishes at once).
fn begin(app: &Rc<App>, mode: &str, purpose: Purpose) {
    {
        let mut c = app.cap.borrow_mut();
        if c.busy || c.snip.is_some() {
            return;
        }
        if purpose == Purpose::Record && c.screen.is_some() {
            return;
        }
        c.busy = true;
    }
    // The main window stays where it is: it is part of the screen grab, and window mode can pick it.
    let main_was_visible = app.ui.window().is_visible();
    let keep = hwnd_of(app.ui.window());
    let mode = mode.to_string();
    std::thread::spawn(move || {
        let result = grab::screenshot();
        let windows =
            rimelight_platform::list_windows(rimelight_platform::WindowList::capturable(keep));
        post(move |a| {
            a.cap.borrow_mut().busy = false;
            match result {
                Ok((shot, rect)) => {
                    if mode == "screen" && purpose == Purpose::Snip {
                        finish_snip(a, shot, main_was_visible);
                    } else {
                        show_overlay(a, shot, rect, windows, &mode, purpose, main_was_visible);
                    }
                }
                Err(e) => {
                    if main_was_visible {
                        show_main(a);
                    }
                    toast(a, e);
                }
            }
        });
    });
}

fn show_overlay(
    app: &Rc<App>,
    shot: Picture,
    rect: ScreenRect,
    windows: Vec<TopWindow>,
    mode: &str,
    purpose: Purpose,
    main_was_visible: bool,
) {
    // Made for each snip and dropped after it (release_overlay), so nothing is kept at rest. It never zooms in: animations
    // are off for every window of ours from creation (rimelight_platform::start_no_window_animations).
    let Ok(w) = ui::SnipWindow::new() else { return };
    w.invoke_reset();
    w.set_shot(picture::to_slint(&shot));
    w.set_shot_w(shot.w as i32);
    w.set_shot_h(shot.h as i32);
    w.set_mode(mode.into());
    w.set_purpose(
        if purpose == Purpose::Record {
            "record"
        } else {
            "snip"
        }
        .into(),
    );
    let (fw, fh) = (rect.w.max(1) as f32, rect.h.max(1) as f32);
    let boxes: Vec<ui::WindowBox> = windows
        .iter()
        .map(|t| ui::WindowBox {
            title: t.title.clone().into(),
            x: (t.rect.x - rect.x) as f32 / fw,
            y: (t.rect.y - rect.y) as f32 / fh,
            w: t.rect.w as f32 / fw,
            h: t.rect.h as f32 / fh,
        })
        .collect();
    w.set_windows(ModelRc::new(VecModel::from(boxes.clone())));
    // The mode bar on the pointer's monitor, and the window under the pointer highlighted from the start.
    if let Some(((cx, cy), mon)) = rimelight_platform::cursor_monitor() {
        w.set_mon_x((mon.x - rect.x) as f32 / fw);
        w.set_mon_y((mon.y - rect.y) as f32 / fh);
        w.set_mon_w(mon.w as f32 / fw);
        let (fx, fy) = ((cx - rect.x) as f32 / fw, (cy - rect.y) as f32 / fh);
        let first = boxes
            .iter()
            .position(|b| fx >= b.x && fx <= b.x + b.w && fy >= b.y && fy <= b.y + b.h);
        w.set_hovered(first.map_or(-1, |i| i as i32));
    }
    w.on_hit(move |fx, fy| {
        boxes
            .iter()
            .position(|b| fx >= b.x && fx <= b.x + b.w && fy >= b.y && fy <= b.y + b.h)
            .map_or(-1, |i| i as i32)
    });
    w.on_done(|x, y, ww, hh| with_app(move |a| overlay_done(a, x, y, ww, hh)));
    w.on_cancel(|| with_app(overlay_cancel));
    w.on_free_start(|x, y| with_app(move |a| free_point(a, x, y, true)));
    w.on_free_move(|x, y| with_app(move |a| free_point(a, x, y, false)));
    w.on_free_end(|| with_app(free_end));
    w.on_color_at(|x, y| {
        let mut out = SharedString::new();
        with_app(|a| out = color_at(a, x, y));
        out
    });
    w.on_picked(|x, y| with_app(move |a| color_picked(a, x, y)));
    w.window()
        .set_position(slint::PhysicalPosition::new(rect.x, rect.y));
    w.window()
        .set_size(slint::PhysicalSize::new(rect.w as u32, rect.h as u32));
    let _ = w.show();
    // Again after showing: winit may have moved it to fit a monitor.
    w.window()
        .set_position(slint::PhysicalPosition::new(rect.x, rect.y));
    w.window()
        .set_size(slint::PhysicalSize::new(rect.w as u32, rect.h as u32));
    focus_overlay(w.as_weak(), 0);
    app.cap.borrow_mut().snip = Some(Snip {
        window: w,
        shot,
        rect,
        windows,
        purpose,
        main_was_visible,
        free: Vec::new(),
    });
}

/// Gives the overlay the keyboard once its native window exists (Slint creates it inside the event loop, after show).
/// Retried for up to 250 ms.
fn focus_overlay(w: slint::Weak<ui::SnipWindow>, tries: u32) {
    use slint::winit_030::WinitWindowAccessor as _;
    let Some(win) = w.upgrade() else { return };
    let focused = win
        .window()
        .with_winit_window(|ww| ww.focus_window())
        .is_some();
    if !focused && tries < 50 {
        slint::Timer::single_shot(Duration::from_millis(5), move || {
            focus_overlay(w, tries + 1)
        });
    }
}

fn overlay_cancel(app: &Rc<App>) {
    let snip = app.cap.borrow_mut().snip.take();
    if let Some(s) = snip {
        let _ = s.window.hide();
        release_overlay(&s.window);
        if s.main_was_visible {
            show_main(app);
        }
    }
}

fn overlay_done(app: &Rc<App>, x: f32, y: f32, w: f32, h: f32) {
    let Some(s) = app.cap.borrow_mut().snip.take() else {
        return;
    };
    let _ = s.window.hide();
    release_overlay(&s.window);
    let mode = s.window.get_mode();
    // In window mode, use the window's exact pixels.
    let px = |f: f32, total: i32| (f * total as f32).round() as i32;
    let mut area = ScreenRect {
        x: px(x, s.rect.w),
        y: px(y, s.rect.h),
        w: px(w, s.rect.w),
        h: px(h, s.rect.h),
    };
    if mode == "window"
        && let Some(t) = s.windows.iter().find(|t| {
            (t.rect.x - s.rect.x - area.x).abs() <= 2 && (t.rect.y - s.rect.y - area.y).abs() <= 2
        })
    {
        area = ScreenRect {
            x: t.rect.x - s.rect.x,
            y: t.rect.y - s.rect.y,
            w: t.rect.w,
            h: t.rect.h,
        };
    }
    // Clip to the screenshot.
    let x0 = area.x.clamp(0, s.shot.w as i32 - 1);
    let y0 = area.y.clamp(0, s.shot.h as i32 - 1);
    let x1 = (area.x + area.w).clamp(x0 + 1, s.shot.w as i32);
    let y1 = (area.y + area.h).clamp(y0 + 1, s.shot.h as i32);
    match s.purpose {
        Purpose::Snip if mode == "text" => {
            let pic = rimelight_picture::crop_px(
                &s.shot,
                x0 as u32,
                y0 as u32,
                (x1 - x0) as u32,
                (y1 - y0) as u32,
            );
            copy_text(app, &pic, s.main_was_visible);
        }
        Purpose::Snip => {
            let pic = rimelight_picture::crop_px(
                &s.shot,
                x0 as u32,
                y0 as u32,
                (x1 - x0) as u32,
                (y1 - y0) as u32,
            );
            finish_snip(app, pic, s.main_was_visible);
        }
        Purpose::Record => {
            let rect = ScreenRect {
                x: s.rect.x + x0,
                y: s.rect.y + y0,
                w: x1 - x0,
                h: y1 - y0,
            };
            start_screen(app, rect, s.main_was_visible);
        }
    }
}

fn finish_snip(app: &Rc<App>, pic: Picture, main_was_visible: bool) {
    let (copy, save, open, dir) = {
        let p = app.prefs.borrow();
        (
            p.snip_copy,
            p.snip_save,
            p.snip_open,
            p.screenshot_dir.clone(),
        )
    };
    let mut notes = Vec::new();
    if copy {
        match picture::copy_to_clipboard(&pic) {
            Ok(()) => notes.push("copied".to_string()),
            Err(e) => notes.push(format!("not copied ({e})")),
        }
    }
    let mut saved: Option<PathBuf> = None;
    if save {
        let path = files::free_path(&dir, &files::stamp("Screenshot"), "png");
        match rimelight_picture::save(&pic, &path) {
            Ok(()) => {
                notes.push(format!("saved as {}", files::file_name(&path)));
                saved = Some(path);
            }
            Err(e) => notes.push(format!("not saved ({e})")),
        }
    }
    if open || main_was_visible {
        crate::viewer::open_picture(app, pic, saved);
        show_main(app);
    }
    if !notes.is_empty() {
        let mut text = notes.join(", ");
        if let Some(c) = text.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        toast(app, format!("Snip {text}"));
    }
}

/// The Win32 window handle of one of our windows (0 when it has none yet).
fn hwnd_of(window: &slint::Window) -> isize {
    use slint::winit_030::WinitWindowAccessor as _;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    window
        .with_winit_window(|w| match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        })
        .unwrap_or(0)
}

fn start_screen(app: &Rc<App>, rect: ScreenRect, _main_was_visible: bool) {
    let (system_audio, mic, fps, dir) = {
        let p = app.prefs.borrow();
        (
            p.rec_system_audio,
            p.rec_mic,
            p.rec_fps,
            p.recording_dir.clone(),
        )
    };
    let path = recorder::new_path(&dir, "Recording", "mp4");
    let opts = recorder::Options {
        screen: Some((rect, fps)),
        camera: None,
        system_audio,
        mic,
        path,
        mic_name: None,
    };
    match recorder::start(opts) {
        Ok(rec) => {
            let notes = rec.notes.join("; ");
            app.cap.borrow_mut().screen = Some(rec);
            show_bar(app, rect);
            set_recording_flag(app, true);
            if !notes.is_empty() {
                toast(app, notes);
            }
        }
        Err(e) => {
            show_main(app);
            toast(app, e);
        }
    }
}

fn show_bar(app: &Rc<App>, rect: ScreenRect) {
    let Ok(bar) = ui::RecBar::new() else { return };
    bar.set_what(format!("{} × {}", rect.w, rect.h).into());
    bar.on_stop(|| with_app(stop_screen));
    bar.on_discard(|| with_app(discard_screen));
    bar.on_drag_by(|dx, dy| {
        with_app(move |a| {
            if let Some(b) = a.cap.borrow().bar.as_ref() {
                let pos = b.window().position();
                let sf = b.window().scale_factor();
                b.window().set_position(slint::PhysicalPosition::new(
                    pos.x + (dx * sf) as i32,
                    pos.y + (dy * sf) as i32,
                ));
            }
        })
    });
    let vs = rimelight_platform::virtual_screen();
    let _ = bar.show();
    let sf = bar.window().scale_factor();
    let bw = (300.0 * sf) as i32;
    // Above the recorded area if there's room, else at the top of the screen. It never shows in the recording.
    let y = if rect.y - vs.y > (60.0 * sf) as i32 {
        rect.y - (52.0 * sf) as i32
    } else {
        vs.y + (8.0 * sf) as i32
    };
    bar.window().set_position(slint::PhysicalPosition::new(
        rect.x + rect.w / 2 - bw / 2,
        y,
    ));
    use slint::winit_030::WinitWindowAccessor as _;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    bar.window().with_winit_window(|w| {
        if let Ok(h) = w.window_handle()
            && let RawWindowHandle::Win32(h) = h.as_raw()
        {
            rimelight_platform::exclude_from_capture(h.hwnd.get());
        }
    });
    app.cap.borrow_mut().bar = Some(bar);
}

fn set_recording_flag(app: &Rc<App>, on: bool) {
    app.ui.global::<ui::Shell>().set_recording(on);
    if let Some(t) = app.tray.borrow().as_ref() {
        t.set_recording(on);
    }
}

fn close_bar(app: &Rc<App>) {
    let bar = app.cap.borrow_mut().bar.take();
    if let Some(b) = bar {
        let _ = b.hide();
    }
    set_recording_flag(app, false);
}

pub fn stop_screen(app: &Rc<App>) {
    let Some(rec) = app.cap.borrow_mut().screen.take() else {
        return;
    };
    close_bar(app);
    toast(app, "Finishing the recording…");
    std::thread::spawn(move || {
        let result = rec.finish();
        post(move |a| match result {
            Ok(path) => {
                show_main(a);
                open_path(a, &path);
                toast(a, format!("Saved {}", files::file_name(&path)));
            }
            Err(e) => {
                show_main(a);
                toast(a, format!("The recording failed: {e}"));
            }
        });
    });
}

fn discard_screen(app: &Rc<App>) {
    let Some(rec) = app.cap.borrow_mut().screen.take() else {
        return;
    };
    close_bar(app);
    std::thread::spawn(move || rec.discard());
    show_main(app);
    toast(app, "Recording discarded");
}

/// Quitting: finish anything that is recording, so no file is left broken.
pub fn stop_all_now(app: &Rc<App>) {
    let (screen, voice) = {
        let mut c = app.cap.borrow_mut();
        (c.screen.take(), c.voice.take())
    };
    if let Some(r) = screen {
        let _ = r.finish();
    }
    if let Some(r) = voice {
        let _ = r.finish();
    }
}

fn toggle_voice(app: &Rc<App>) {
    let running = app.cap.borrow_mut().voice.take();
    let v = app.ui.global::<ui::Voice>();
    if let Some(rec) = running {
        v.set_recording(false);
        v.set_paused(false);
        v.set_level(0.0);
        let marks = std::mem::take(&mut app.cap.borrow_mut().voice_marks);
        std::thread::spawn(move || {
            let result = rec.finish();
            post(move |a| {
                match result {
                    Ok(path) => {
                        if !marks.is_empty() {
                            write_marks(&path, &marks);
                        }
                        remember(a, &path);
                        toast(a, format!("Saved {}", files::file_name(&path)));
                    }
                    Err(e) => toast(a, format!("The recording failed: {e}")),
                }
                refresh_takes(a);
            });
        });
        return;
    }
    let dir = app.prefs.borrow().voice_dir.clone();
    let (mic_name, ext) = {
        let c = app.cap.borrow();
        (
            mic_name_at(c.voice_mic),
            VOICE_FORMATS[c.voice_format.min(VOICE_FORMATS.len() - 1)],
        )
    };
    let opts = recorder::Options {
        screen: None,
        camera: None,
        system_audio: false,
        mic: true,
        path: recorder::new_path(&dir, "Recording", ext),
        mic_name,
    };
    match recorder::start(opts) {
        Ok(rec) => {
            {
                let mut c = app.cap.borrow_mut();
                c.voice = Some(rec);
                c.voice_marks.clear();
            }
            set_marker_list(app);
            v.set_recording(true);
        }
        Err(e) => toast(app, e),
    }
}

/// The microphone with this index in the voice page's list (0 is the default one).
fn mic_name_at(index: usize) -> Option<String> {
    if index == 0 {
        None
    } else {
        recorder_mic_names().get(index - 1).cloned()
    }
}

fn recorder_mic_names() -> Vec<String> {
    crate::capture::audio_in::input_names()
}

fn toggle_pause(app: &Rc<App>) {
    let paused = {
        let c = app.cap.borrow();
        match c.voice.as_ref() {
            Some(r) => {
                let on = !r.is_paused();
                r.set_paused(on);
                on
            }
            None => return,
        }
    };
    app.ui.global::<ui::Voice>().set_paused(paused);
}

fn add_marker(app: &Rc<App>) {
    let ms = {
        let c = app.cap.borrow();
        match c.voice.as_ref() {
            Some(r) if !r.is_paused() => r.elapsed_ms(),
            _ => return,
        }
    };
    app.cap.borrow_mut().voice_marks.push(ms);
    set_marker_list(app);
}

fn set_marker_list(app: &Rc<App>) {
    let items: Vec<SharedString> = app
        .cap
        .borrow()
        .voice_marks
        .iter()
        .enumerate()
        .map(|(i, ms)| format!("Marker {} · {}", i + 1, files::clock(*ms)).into())
        .collect();
    app.ui
        .global::<ui::Voice>()
        .set_markers(ModelRc::new(VecModel::from(items)));
}

/// Saves the markers next to the recording, as <name>.json.
fn write_marks(path: &std::path::Path, marks: &[u64]) {
    let mut s = String::from("{\n  \"markers\": [\n");
    for (i, ms) in marks.iter().enumerate() {
        let comma = if i + 1 < marks.len() { "," } else { "" };
        s.push_str(&format!(
            "    {{\"ms\": {ms}, \"time\": \"{}\"}}{comma}\n",
            files::clock(*ms)
        ));
    }
    s.push_str("  ]\n}\n");
    let _ = std::fs::write(path.with_extension("json"), s);
}

fn pick_mic(app: &Rc<App>, index: i32) {
    let index = index.max(0) as usize;
    app.cap.borrow_mut().voice_mic = index;
    app.ui.global::<ui::Voice>().set_mic_index(index as i32);
}

/// The microphone gain in dB (-12 to +24). It applies at once (the slider moves); `save` stores it in the settings.
fn set_gain(app: &Rc<App>, db: f32, save: bool) {
    let db = db.clamp(-12.0, 24.0);
    crate::capture::audio_in::set_mic_gain_db(db);
    app.prefs.borrow_mut().voice_gain_db = db;
    if save {
        app.prefs.borrow().save();
    }
    app.ui.global::<ui::Voice>().set_gain_db(db);
}

fn pick_format(app: &Rc<App>, index: i32) {
    let index = (index.max(0) as usize).min(VOICE_FORMATS.len() - 1);
    app.cap.borrow_mut().voice_format = index;
    app.ui.global::<ui::Voice>().set_format_index(index as i32);
}

fn refresh_mics(app: &Rc<App>) {
    let mut names = vec![SharedString::from("Default microphone")];
    names.extend(recorder_mic_names().into_iter().map(SharedString::from));
    let v = app.ui.global::<ui::Voice>();
    v.set_mics(ModelRc::new(VecModel::from(names)));
    v.set_mic_index(app.cap.borrow().voice_mic as i32);
    v.set_format_index(app.cap.borrow().voice_format as i32);
}

/// Renames a take (and its markers file), keeping its format.
fn rename_take(app: &Rc<App>, index: i32, name: String) {
    let Some(old) = takes(app).get(index.max(0) as usize).cloned() else {
        return;
    };
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        toast(app, "That name can't be used.");
        return;
    }
    let file = match old.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{name}.{ext}"),
        None => name.clone(),
    };
    let new = old.with_file_name(file);
    if new == old {
        return;
    }
    if new.exists() {
        toast(app, "A recording with that name already exists.");
        return;
    }
    if let Err(e) = crate::app::rename_file(&old, &new) {
        toast(app, format!("Could not rename it: {e}"));
        return;
    }
    let old_json = old.with_extension("json");
    if old_json.is_file() {
        let _ = crate::app::rename_file(&old_json, &new.with_extension("json"));
    }
    toast(app, format!("Renamed to {}", files::file_name(&new)));
    refresh_takes(app);
}

/// Deletes a take (and its markers file) to the Recycle Bin.
fn delete_take(app: &Rc<App>, index: i32) {
    let Some(p) = takes(app).get(index.max(0) as usize).cloned() else {
        return;
    };
    let json = p.with_extension("json");
    let mut gone = rimelight_platform::recycle(&p) || std::fs::remove_file(&p).is_ok();
    if json.is_file() {
        let _ = rimelight_platform::recycle(&json) || std::fs::remove_file(&json).is_ok();
    }
    if gone {
        gone = !p.exists();
    }
    toast(
        app,
        if gone {
            format!("{} moved to the Recycle Bin", files::file_name(&p))
        } else {
            "Could not delete it.".to_string()
        },
    );
    refresh_takes(app);
}

fn takes(app: &Rc<App>) -> Vec<PathBuf> {
    let dir = app.prefs.borrow().voice_dir.clone();
    let mut list: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| files::kind_of(p) == Some(files::Kind::Audio))
                .map(|p| {
                    (
                        std::fs::metadata(&p)
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH),
                        p,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter().map(|(_, p)| p).collect()
}

pub fn refresh_takes(app: &Rc<App>) {
    let items: Vec<ui::RecentItem> = takes(app)
        .iter()
        .enumerate()
        .map(|(i, p)| ui::RecentItem {
            name: files::file_name(p).into(),
            detail: std::fs::metadata(p)
                .map(|m| files::human_size(m.len()))
                .unwrap_or_default()
                .into(),
            kind: "audio".into(),
            index: i as i32,
        })
        .collect();
    let v = app.ui.global::<ui::Voice>();
    v.set_takes(ModelRc::new(VecModel::from(items)));
    v.set_folder(app.prefs.borrow().voice_dir.display().to_string().into());
    // The list changed, so the picked take may be a different file now.
    v.set_selected(-1);
    refresh_mics(app);
}

fn slow_tick(app: &Rc<App>) {
    let c = app.cap.borrow();
    if let Some(r) = c.screen.as_ref() {
        let t: SharedString = files::clock(r.elapsed_ms()).into();
        if let Some(b) = c.bar.as_ref() {
            b.set_time_text(t.clone());
        }
        app.ui.global::<ui::Shell>().set_recording_time(t);
    }
    if let Some(r) = c.voice.as_ref() {
        let v = app.ui.global::<ui::Voice>();
        v.set_time_text(files::clock(r.elapsed_ms()).into());
        v.set_level(r.level());
    }
}

/// The tray icon, while "keep running" is on.
pub fn ensure_tray(app: &Rc<App>, on: bool) {
    if !on {
        app.tray.borrow_mut().take();
        return;
    }
    if app.tray.borrow().is_some() {
        return;
    }
    let Ok(tray) = ui::Tray::new() else { return };
    tray.on_open(|| {
        with_app(|a| {
            show_main(a);
        })
    });
    tray.on_snip(|m| with_app(move |a| snip(a, &m)));
    tray.on_record(|| with_app(record_toggle));
    tray.on_voice(|| {
        with_app(|a| {
            show_main(a);
            go(a, "voice");
        })
    });
    tray.on_quit(|| {
        with_app(stop_all_now);
        let _ = slint::quit_event_loop();
    });
    let _ = tray.show();
    *app.tray.borrow_mut() = Some(tray);
}

fn free_point(app: &Rc<App>, x: f32, y: f32, first: bool) {
    let mut c = app.cap.borrow_mut();
    let Some(s) = c.snip.as_mut() else { return };
    if first {
        s.free.clear();
    }
    if let Some(&(lx, ly)) = s.free.last()
        && (lx - x).abs() < 0.0008
        && (ly - y).abs() < 0.0008
    {
        return;
    }
    s.free.push((x, y));
    let (sw, sh) = (s.shot.w as f32, s.shot.h as f32);
    let smooth = smooth_outline(
        &s.free
            .iter()
            .map(|(x, y)| (x * sw, y * sh))
            .collect::<Vec<_>>(),
    );
    let mut path = String::new();
    for (i, (px, py)) in smooth.iter().enumerate() {
        path.push_str(&format!(
            "{} {:.0} {:.0} ",
            if i == 0 { "M" } else { "L" },
            px / sw * 10000.0,
            py / sh * 10000.0
        ));
    }
    if smooth.len() > 2 {
        path.push('Z');
    }
    s.window.set_free_path(path.into());
}

/// The lasso's raw pointer path (pixels) made smooth: points under 2 px apart are dropped, then two Chaikin rounds
/// cut each corner. Used for both the live outline and the cut, so what you see is what gets kept.
fn smooth_outline(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|&(lx, ly): &(f32, f32)| {
            ((lx - p.0).powi(2) + (ly - p.1).powi(2)).sqrt() >= 2.0
        }) {
            pts.push(p);
        }
    }
    if pts.len() < 3 {
        return pts;
    }
    for _ in 0..2 {
        pts = chaikin_closed(&pts);
    }
    pts
}

/// One closed Chaikin round: each edge a-b becomes the points a+(b-a)/4 and a+3(b-a)/4.
fn chaikin_closed(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let n = p.len();
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let (ax, ay) = p[i];
        let (bx, by) = p[(i + 1) % n];
        out.push((ax * 0.75 + bx * 0.25, ay * 0.75 + by * 0.25));
        out.push((ax * 0.25 + bx * 0.75, ay * 0.25 + by * 0.75));
    }
    out
}

/// Freeform: the shape's bounding box, with everything outside the shape transparent.
fn free_end(app: &Rc<App>) {
    let points = app
        .cap
        .borrow()
        .snip
        .as_ref()
        .map(|s| s.free.clone())
        .unwrap_or_default();
    if points.len() < 3 {
        if let Some(s) = app.cap.borrow().snip.as_ref() {
            s.window.set_free_path(SharedString::new());
        }
        return;
    }
    let Some(s) = app.cap.borrow_mut().snip.take() else {
        return;
    };
    let _ = s.window.hide();
    release_overlay(&s.window);
    let (sw, sh) = (s.shot.w as f32, s.shot.h as f32);
    let px = smooth_outline(
        &points
            .iter()
            .map(|(x, y)| (x * sw, y * sh))
            .collect::<Vec<_>>(),
    );
    let x0 = px
        .iter()
        .map(|p| p.0)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let y0 = px
        .iter()
        .map(|p| p.1)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let x1 = (px.iter().map(|p| p.0).fold(0.0, f32::max).ceil() as u32).min(s.shot.w);
    let y1 = (px.iter().map(|p| p.1).fold(0.0, f32::max).ceil() as u32).min(s.shot.h);
    if x1 <= x0 + 1 || y1 <= y0 + 1 {
        if s.main_was_visible {
            show_main(app);
        }
        return;
    }
    let mut pic = rimelight_picture::crop_px(&s.shot, x0, y0, x1 - x0, y1 - y0);
    // The mask is the outline filled with anti-aliasing; its coverage becomes the picture's alpha, so the edge blends.
    if let (Some(path), Some(mut mask)) = (
        outline_path(&px, x0 as f32, y0 as f32),
        resvg::tiny_skia::Pixmap::new(pic.w, pic.h),
    ) {
        let mut paint = resvg::tiny_skia::Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        paint.anti_alias = true;
        mask.fill_path(
            &path,
            &paint,
            resvg::tiny_skia::FillRule::EvenOdd,
            resvg::tiny_skia::Transform::identity(),
            None,
        );
        for (i, p) in pic.rgba.chunks_exact_mut(4).enumerate() {
            let cover = mask.data()[i * 4 + 3] as u32;
            p[3] = ((p[3] as u32 * cover + 127) / 255) as u8;
        }
    }
    finish_snip(app, pic, s.main_was_visible);
}

/// The outline as a closed path in the crop's pixel space (screen pixels minus the crop's origin).
fn outline_path(poly: &[(f32, f32)], ox: f32, oy: f32) -> Option<resvg::tiny_skia::Path> {
    let mut pb = resvg::tiny_skia::PathBuilder::new();
    for (i, &(x, y)) in poly.iter().enumerate() {
        if i == 0 {
            pb.move_to(x - ox, y - oy);
        } else {
            pb.line_to(x - ox, y - oy);
        }
    }
    pb.close();
    pb.finish()
}

fn pixel_at(s: &Snip, x: f32, y: f32) -> [u8; 3] {
    let px = ((x * s.shot.w as f32) as u32).min(s.shot.w - 1);
    let py = ((y * s.shot.h as f32) as u32).min(s.shot.h - 1);
    let i = ((py * s.shot.w + px) * 4) as usize;
    [s.shot.rgba[i], s.shot.rgba[i + 1], s.shot.rgba[i + 2]]
}

/// The colour under the pointer as "#RRGGBB" (and the loupe's swatch).
fn color_at(app: &Rc<App>, x: f32, y: f32) -> SharedString {
    let c = app.cap.borrow();
    let Some(s) = c.snip.as_ref() else {
        return SharedString::new();
    };
    let [r, g, b] = pixel_at(s, x, y);
    s.window.set_swatch(slint::Color::from_rgb_u8(r, g, b));
    format!("#{r:02X}{g:02X}{b:02X}").into()
}

fn color_picked(app: &Rc<App>, x: f32, y: f32) {
    let Some(s) = app.cap.borrow_mut().snip.take() else {
        return;
    };
    let _ = s.window.hide();
    release_overlay(&s.window);
    let [r, g, b] = pixel_at(&s, x, y);
    let hex = format!("#{r:02X}{g:02X}{b:02X}");
    let copied = arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(hex.clone()))
        .is_ok();
    if s.main_was_visible {
        show_main(app);
    }
    toast(
        app,
        if copied {
            format!("Copied {hex} (rgb {r}, {g}, {b})")
        } else {
            format!("{hex}: could not copy it")
        },
    );
}

/// Text extractor: Windows' text recognition on the area, copied to the clipboard.
fn copy_text(app: &Rc<App>, pic: &Picture, main_was_visible: bool) {
    let result = rimelight_platform::recognise_text(&pic.rgba, pic.w, pic.h);
    if main_was_visible {
        show_main(app);
    }
    match result {
        Ok(text) if text.trim().is_empty() => toast(app, "No text found there"),
        Ok(text) => {
            let lines = text.lines().count();
            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
                Ok(()) => toast(
                    app,
                    format!(
                        "Copied {lines} line{} of text",
                        if lines == 1 { "" } else { "s" }
                    ),
                ),
                Err(e) => toast(app, format!("Could not copy the text: {e}")),
            }
        }
        Err(e) => toast(app, format!("Could not read the text: {e}")),
    }
}
/// Lets go of a hidden snip overlay, so its native window and GPU surface are freed. The last handle is dropped on the
/// next loop turn, not here: the callers run inside the overlay's own callbacks.
fn release_overlay(w: &ui::SnipWindow) {
    w.set_shot(slint::Image::default());
    let w = w.clone_strong();
    slint::Timer::single_shot(Duration::ZERO, move || drop(w));
}

/// A screen or voice recording is running (the process must not hand over to another copy now).
pub fn busy(app: &Rc<App>) -> bool {
    let c = app.cap.borrow();
    c.screen.is_some() || c.voice.is_some()
}
