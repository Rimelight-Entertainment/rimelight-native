//! The camera page: the live preview, photos and video (through the recorder), the timer, the grid, the mirror, the
//! device list and the last capture's thumbnail. A 30 fps timer shows the newest frame of the open camera; the camera is
//! closed when the page is left.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle as _, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::app::{App, go, open_path, post, remember, toast, with_app};
use crate::capture::{camera, recorder};
use crate::files::{self, Kind};
use crate::ui;

/// The timer choices in the page, in seconds (index 0 is off).
const TIMER_SECS: [u64; 4] = [0, 3, 5, 10];
const VIDEO_FPS: i32 = 30;

#[derive(Default)]
pub struct CameraState {
    cam: Option<camera::Camera>,
    /// The index of the open camera in the device list.
    device: usize,
    /// True in video mode, false in photo mode.
    video: bool,
    /// Index into TIMER_SECS.
    timer: usize,
    grid: bool,
    mirror: bool,
    rec: Option<recorder::Recording>,
    /// The frame number last shown in the preview.
    last_seq: u64,
    /// Seconds left before a timed photo.
    countdown: u64,
    last_path: Option<PathBuf>,
    preview: slint::Timer,
    count: slint::Timer,
}

pub fn wire(app: &Rc<App>) {
    app.camera.borrow_mut().mirror = true;
    let cam = app.ui.global::<ui::Camera>();
    cam.set_mirror(true);
    cam.on_shutter(|| with_app(shutter));
    cam.on_set_device(|i| with_app(move |a| set_device(a, i.max(0) as usize)));
    cam.on_set_video(|on| with_app(move |a| set_video(a, on)));
    cam.on_set_timer(|i| with_app(move |a| set_timer(a, i.clamp(0, 3) as usize)));
    cam.on_set_grid(|on| {
        with_app(move |a| {
            a.camera.borrow_mut().grid = on;
            a.ui.global::<ui::Camera>().set_grid(on);
        })
    });
    cam.on_set_mirror(|on| {
        with_app(move |a| {
            {
                let mut c = a.camera.borrow_mut();
                c.mirror = on;
                // Show the newest frame again, mirrored or not.
                c.last_seq = 0;
            }
            a.ui.global::<ui::Camera>().set_mirror(on);
        })
    });
    cam.on_open_last(|| with_app(open_last));
    cam.on_check_again(|| with_app(enter));
}

/// Called when the camera page opens: lists the cameras, opens the chosen one and starts the preview.
pub fn enter(app: &Rc<App>) {
    let names = camera::devices();
    let cam = app.ui.global::<ui::Camera>();
    let items: Vec<SharedString> = names
        .iter()
        .map(|n| SharedString::from(n.as_str()))
        .collect();
    cam.set_devices(ModelRc::new(VecModel::from(items)));
    cam.set_save_folder(camera_roll().display().to_string().into());
    let has_open = app.camera.borrow().cam.is_some();
    if !has_open {
        if names.is_empty() {
            cam.set_status("No camera found".into());
        } else {
            let index = app.camera.borrow().device.min(names.len() - 1);
            start(app, index);
        }
    }
    cam.set_device_index(app.camera.borrow().device as i32);
    cam.set_video_mode(app.camera.borrow().video);
    cam.set_timer_index(app.camera.borrow().timer as i32);
    cam.set_grid(app.camera.borrow().grid);
    app.camera.borrow().preview.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(33),
        || with_app(tick),
    );
}

/// Called when another page is opened: the preview stops, any video is saved and the camera is closed.
pub fn leave(app: &Rc<App>) {
    app.camera.borrow().preview.stop();
    app.camera.borrow().count.stop();
    stop_video(app);
    let old = app.camera.borrow_mut().cam.take();
    drop(old);
    let cam = app.ui.global::<ui::Camera>();
    cam.set_running(false);
    cam.set_has_frame(false);
    cam.set_countdown(0);
}

/// Quitting: a video in progress is finished so its file is complete.
pub fn stop_all_now(app: &Rc<App>) {
    let rec = app.camera.borrow_mut().rec.take();
    if let Some(r) = rec {
        let _ = r.finish();
    }
}

/// The preview timer: shows the newest frame when it changed, and the recording's time.
fn tick(app: &Rc<App>) {
    let cam = app.ui.global::<ui::Camera>();
    let mut c = app.camera.borrow_mut();
    cam.set_running(c.cam.is_some());
    let (mirror, last) = (c.mirror, c.last_seq);
    let update = c.cam.as_ref().and_then(|cm| {
        let guard = cm.feed.lock().ok()?;
        let f = guard.as_ref().filter(|f| f.seq != last)?;
        Some((f.seq, preview_image(f, mirror)))
    });
    if let Some((seq, img)) = update {
        c.last_seq = seq;
        cam.set_frame(img);
        cam.set_has_frame(true);
    }
    if let Some(r) = c.rec.as_ref() {
        cam.set_time_text(files::clock(r.elapsed_ms()).into());
    }
}

fn shutter(app: &Rc<App>) {
    let (video, recording, timer) = {
        let c = app.camera.borrow();
        (c.video, c.rec.is_some(), c.timer)
    };
    if video {
        if recording {
            stop_video(app);
        } else {
            start_video(app);
        }
        return;
    }
    if timer == 0 {
        take_photo(app);
        return;
    }
    {
        let mut c = app.camera.borrow_mut();
        c.countdown = TIMER_SECS[timer];
        app.ui
            .global::<ui::Camera>()
            .set_countdown(c.countdown as i32);
        app.ui
            .global::<ui::Camera>()
            .set_countdown_text(c.countdown.to_string().into());
        c.count
            .start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
                with_app(count_down)
            });
    }
}

/// One second of the photo timer; the photo is taken when it reaches zero.
fn count_down(app: &Rc<App>) {
    let done = {
        let mut c = app.camera.borrow_mut();
        c.countdown = c.countdown.saturating_sub(1);
        app.ui
            .global::<ui::Camera>()
            .set_countdown(c.countdown as i32);
        app.ui
            .global::<ui::Camera>()
            .set_countdown_text(c.countdown.to_string().into());
        if c.countdown == 0 {
            c.count.stop();
            true
        } else {
            false
        }
    };
    if done {
        take_photo(app);
    }
}

fn take_photo(app: &Rc<App>) {
    let Some(frame) = grab_latest(app) else {
        toast(app, "No picture from the camera yet");
        return;
    };
    let mirror = app.camera.borrow().mirror;
    let path = stamped("IMG", "jpg");
    let rgb = rgb_bytes(&frame, mirror);
    let saved = image::RgbImage::from_raw(frame.w, frame.h, rgb)
        .ok_or_else(|| "the picture is the wrong size".to_string())
        .and_then(|img| {
            img.save_with_format(&path, image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())
        });
    match saved {
        Ok(()) => {
            set_last(app, &path, &frame, mirror);
            remember(app, &path);
            toast(app, format!("Saved {}", files::file_name(&path)));
        }
        Err(e) => toast(app, format!("Could not save the photo: {e}")),
    }
}

fn start_video(app: &Rc<App>) {
    let source = {
        let c = app.camera.borrow();
        c.cam.as_ref().map(|cm| (Arc::clone(&cm.feed), cm.w, cm.h))
    };
    let Some((feed, w, h)) = source else {
        toast(app, "No camera is open");
        return;
    };
    let path = stamped("VID", "mp4");
    let opts = recorder::Options {
        screen: None,
        camera: Some(recorder::CameraSource {
            feed,
            w,
            h,
            fps: VIDEO_FPS,
        }),
        system_audio: false,
        mic: true,
        path,
        mic_name: None,
    };
    match recorder::start(opts) {
        Ok(rec) => {
            app.camera.borrow_mut().rec = Some(rec);
            app.ui.global::<ui::Camera>().set_recording(true);
        }
        Err(e) => toast(app, e),
    }
}

fn stop_video(app: &Rc<App>) {
    let rec = app.camera.borrow_mut().rec.take();
    let Some(rec) = rec else { return };
    let cam = app.ui.global::<ui::Camera>();
    cam.set_recording(false);
    cam.set_time_text("0:00".into());
    std::thread::spawn(move || {
        let result = rec.finish();
        post(move |a| match result {
            Ok(path) => {
                if let Some(frame) = grab_latest(a) {
                    let mirror = a.camera.borrow().mirror;
                    set_last(a, &path, &frame, mirror);
                } else {
                    a.camera.borrow_mut().last_path = Some(path.clone());
                }
                remember(a, &path);
                toast(a, format!("Saved {}", files::file_name(&path)));
            }
            Err(e) => toast(a, format!("The recording failed: {e}")),
        });
    });
}

fn set_device(app: &Rc<App>, index: usize) {
    if app.camera.borrow().rec.is_some() {
        toast(app, "Stop the video first");
        app.ui
            .global::<ui::Camera>()
            .set_device_index(app.camera.borrow().device as i32);
        return;
    }
    if app.camera.borrow().device == index && app.camera.borrow().cam.is_some() {
        return;
    }
    start(app, index);
}

/// Opens the camera at this index (the old one is closed first).
fn start(app: &Rc<App>, index: usize) {
    let old = app.camera.borrow_mut().cam.take();
    drop(old);
    let cam = app.ui.global::<ui::Camera>();
    match camera::open(index) {
        Ok(opened) => {
            cam.set_resolution(format!("{} x {}", opened.w, opened.h).into());
            {
                let mut c = app.camera.borrow_mut();
                c.device = index;
                c.cam = Some(opened);
                c.last_seq = 0;
            }
            cam.set_device_index(index as i32);
            cam.set_status("".into());
            cam.set_has_frame(false);
        }
        Err(e) => {
            cam.set_resolution("".into());
            cam.set_status("The camera could not be opened".into());
            toast(app, format!("Could not open the camera: {e}"));
        }
    }
}

fn set_video(app: &Rc<App>, on: bool) {
    if app.camera.borrow().rec.is_some() {
        app.ui.global::<ui::Camera>().set_video_mode(!on);
        return;
    }
    app.camera.borrow_mut().video = on;
    app.ui.global::<ui::Camera>().set_video_mode(on);
}

fn set_timer(app: &Rc<App>, index: usize) {
    app.camera.borrow_mut().timer = index;
    app.ui.global::<ui::Camera>().set_timer_index(index as i32);
}

/// The thumbnail's click: the capture opens in its own tab.
fn open_last(app: &Rc<App>) {
    let path = app.camera.borrow().last_path.clone();
    let Some(path) = path else { return };
    let kind = files::kind_of(&path);
    open_path(app, &path);
    let page = if matches!(kind, Some(Kind::Video | Kind::Audio)) {
        "video"
    } else {
        "image"
    };
    go(app, page);
}

fn set_last(app: &Rc<App>, path: &std::path::Path, frame: &camera::Frame, mirror: bool) {
    app.camera.borrow_mut().last_path = Some(path.to_path_buf());
    let cam = app.ui.global::<ui::Camera>();
    cam.set_thumb(preview_image(frame, mirror));
    cam.set_has_thumb(true);
}

/// A copy of the newest frame, if the camera has delivered one.
fn grab_latest(app: &Rc<App>) -> Option<camera::Frame> {
    let c = app.camera.borrow();
    let guard = c.cam.as_ref()?.feed.lock().ok()?;
    guard.as_ref().map(|f| camera::Frame {
        w: f.w,
        h: f.h,
        seq: f.seq,
        bgra: f.bgra.clone(),
    })
}

/// The frame as a picture for the page: BGRA to RGBA, mirrored when the mirror is on.
fn preview_image(frame: &camera::Frame, mirror: bool) -> slint::Image {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(frame.w, frame.h);
    let out = buf.make_mut_bytes();
    for y in 0..h {
        for x in 0..w {
            let sx = if mirror { w - 1 - x } else { x };
            let s = (y * w + sx) * 4;
            let d = (y * w + x) * 4;
            out[d] = frame.bgra[s + 2];
            out[d + 1] = frame.bgra[s + 1];
            out[d + 2] = frame.bgra[s];
            out[d + 3] = 255;
        }
    }
    slint::Image::from_rgba8(buf)
}

/// The frame as RGB bytes for the JPEG, mirrored when the mirror is on.
fn rgb_bytes(frame: &camera::Frame, mirror: bool) -> Vec<u8> {
    let (w, h) = (frame.w as usize, frame.h as usize);
    let mut out = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let sx = if mirror { w - 1 - x } else { x };
            let s = (y * w + sx) * 4;
            let d = (y * w + x) * 3;
            out[d] = frame.bgra[s + 2];
            out[d + 1] = frame.bgra[s + 1];
            out[d + 2] = frame.bgra[s];
        }
    }
    out
}

/// A new file in the Camera Roll (Pictures\Camera Roll), named like Windows' Camera app: IMG_YYYYMMDD_HHMMSS.
fn stamped(prefix: &str, ext: &str) -> PathBuf {
    let [y, mo, d, h, mi, s] = rimelight_platform::local_time();
    files::free_path(
        &camera_roll(),
        &format!("{prefix}_{y:04}{mo:02}{d:02}_{h:02}{mi:02}{s:02}"),
        ext,
    )
}

/// The folder photos and videos are saved to (Pictures\Camera Roll).
fn camera_roll() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Pictures").join("Camera Roll")
}
