//! Rimelight Media: one small Windows app for pictures, video, music, screenshots, screen recording and voice recording.
//!
//! Usage:
//! - `rimelight-media [file]`: open a file (how Windows starts it for a default app).
//! - `--background`: start in the tray with the global keys and no window (start-with-Windows entry).
//! - `--snip region|window|screen`: start a snip at once.
//! - `--write-icon <file.ico>`: write the app icon and exit (used by tools/pack.ps1).
//! - `--screenshot <file.png>`: open the window without focus, behind other windows, draw about 120 frames, save the
//!   window's picture and exit (used by automated checks).

// Release builds are GUI apps: no console window (as rimelight-space's desktop app).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod archive;
mod cameractl;
mod capture;
mod capturectl;
mod cast;
mod fileops;
mod files;
mod filesctl;
mod highlight;
mod hotkeys;
mod install;
mod instance;
mod mediactl;
mod picture;
mod prefs;
mod settingsctl;
mod setupctl;
mod textctl;
mod ui;
mod viewer;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rimelight_update::{Config, Feed, Updater};
use slint::ComponentHandle as _;

const TOOL: &str = "rimelight-media";

/// "owner/name" of the GitHub repository whose releases feed updates. The release workflow sets it from
/// GITHUB_REPOSITORY when it builds; a local build without it doesn't check for updates.
const REPO: Option<&str> = option_env!("RIMELIGHT_MEDIA_REPO");

/// The window class of every window of ours (set through winit's attributes), so the no-animation hook can tell them
/// from anyone else's.
const WINDOW_CLASS: &str = "RimelightMediaWindow";

/// True on the run the installer starts right after installing.
static FIRST_RUN: AtomicBool = AtomicBool::new(false);

/// This copy's updater, or None when updates are off: a build without a feed, or a copy that isn't installed. The feed is
/// RIMELIGHT_MEDIA_UPDATE_URL (a folder or web address, for testing a release locally) or else the repository's releases.
fn updater() -> Option<Arc<Updater>> {
    static UPDATER: OnceLock<Option<Arc<Updater>>> = OnceLock::new();
    UPDATER
        .get_or_init(|| {
            let feed = match std::env::var("RIMELIGHT_MEDIA_UPDATE_URL") {
                Ok(location) => Feed::Location { location },
                Err(_) => Feed::GithubReleases {
                    repo: REPO?.to_string(),
                },
            };
            Updater::new(Config {
                feed,
                minimum_version_channel: None,
            })
            .map(Arc::new)
        })
        .clone()
}

/// The releases web page of this build's repository, for the release notes link.
fn releases_page() -> Option<String> {
    Feed::GithubReleases {
        repo: REPO?.to_string(),
    }
    .releases_page()
}

struct Args {
    file: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    background: bool,
    snip: Option<String>,
    write_icon: Option<PathBuf>,
    /// Started by another copy that is handing over (tray <-> window, see app::handoff): wait for it to exit.
    takeover: bool,
    /// The process id of the copy being replaced (with --takeover): this copy waits for it to end first.
    after: Option<u32>,
    /// The tab to open on (with --takeover).
    page: Option<String>,
}

fn parse() -> Result<Args, String> {
    let mut args = Args {
        file: None,
        screenshot: None,
        background: false,
        snip: None,
        write_icon: None,
        takeover: false,
        after: None,
        page: None,
    };
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--screenshot") => {
                args.screenshot = Some(it.next().ok_or("--screenshot needs a file")?.into())
            }
            Some("--background") => args.background = true,
            Some("--takeover") => args.takeover = true,
            Some("--after") => {
                args.after = it
                    .next()
                    .and_then(|s| s.into_string().ok())
                    .and_then(|s| s.parse().ok())
            }
            Some("--page") => args.page = it.next().and_then(|s| s.into_string().ok()),
            Some("--write-icon") => {
                args.write_icon = Some(it.next().ok_or("--write-icon needs a file")?.into())
            }
            Some("--snip") => {
                args.snip = Some(
                    it.next()
                        .and_then(|s| s.into_string().ok())
                        .ok_or("--snip needs region, window or screen")?,
                )
            }
            // Win+Shift+S (and Print Screen without the Snipping Tool) start the ms-screenclip: link.
            Some(uri) if uri.to_ascii_lowercase().starts_with("ms-screenclip:") => {
                args.snip = Some(instance::screenclip_mode(uri))
            }
            Some(flag) if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => args.file = Some(PathBuf::from(a)),
        }
    }
    Ok(args)
}

/// Developer check of the engine without the window (set RIMELIGHT_MEDIA_SELFTEST to a folder holding sample.mkv): trims the
/// sample, grabs the screen and records the screen with the computer's sound for two seconds.
fn selftest(dir: &std::path::Path) -> ExitCode {
    let mut ok = true;
    let trimmed = dir.join("selftest-trim.mkv");
    let _ = std::fs::remove_file(&trimmed);
    match rimelight_media::engine::export::trim(&dir.join("sample.mkv"), &trimmed, 1000, 3000) {
        Ok(()) => match rimelight_media::engine::player::probe(&trimmed) {
            Ok(i) => println!(
                "trim: ok, {} ms, {} audio, {} subtitle tracks",
                i.duration_ms,
                i.audio.len(),
                i.subtitles.len()
            ),
            Err(e) => {
                ok = false;
                println!("trim: written but unreadable: {e}")
            }
        },
        Err(e) => {
            ok = false;
            println!("trim: failed: {e}")
        }
    }
    match rimelight_media::engine::player::probe(&dir.join("sample.mkv")).and_then(|info| {
        let p = rimelight_media::engine::player::Player::open(
            &dir.join("sample.mkv"),
            &info,
            None,
            0,
            (640, 360),
        )?;
        p.set_speed(2.0);
        p.play();
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(1500) {
            let _ = p.take_frame();
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(p.position_ms())
    }) {
        // 1.5 s at 2x is about 3 s of video (minus the start-up).
        Ok(ms) if (2_400..=3_200).contains(&ms) => {
            println!("speed: ok, 1.5 s at 2x reached {ms} ms")
        }
        Ok(ms) => {
            ok = false;
            println!("speed: wrong, 1.5 s at 2x reached {ms} ms")
        }
        Err(e) => {
            ok = false;
            println!("speed: failed: {e}")
        }
    }
    // Every file in codecs/: video gives a frame, sound gives decoded samples.
    let mut codec_files: Vec<_> = std::fs::read_dir(dir.join("codecs"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    codec_files.sort();
    for f in codec_files {
        let name = files::file_name(&f);
        let result = rimelight_media::engine::player::probe(&f).and_then(|info| {
            if info.video.is_some() {
                rimelight_media::engine::player::frame_at(&f, 1000)
                    .map(|p| format!("video {}x{}", p.width, p.height))
            } else {
                decode_some_audio(&f).map(|n| format!("audio, {n} samples decoded"))
            }
        });
        match result {
            Ok(what) => println!("codec {name}: ok, {what}"),
            Err(e) => {
                ok = false;
                println!("codec {name}: FAILED: {e}")
            }
        }
    }
    // Text extractor: write text into a white picture, read it back with Windows' text recognition.
    let blank = rimelight_picture::Picture {
        w: 640,
        h: 120,
        rgba: vec![255; 640 * 120 * 4],
    };
    let mark = rimelight_picture::Mark {
        kind: 4,
        rgba: [0, 0, 0, 255],
        width: 10.0,
        points: vec![(20.0, 30.0)],
        text: "Rimelight Media 2026".into(),
        filled: false,
        image: None,
    };
    match rimelight_picture::burn_marks(&blank, &[mark], false)
        .and_then(|p| rimelight_platform::recognise_text(&p.rgba, p.w, p.h))
    {
        Ok(text) if text.contains("Rimelight") => println!("text extractor: ok, read {text:?}"),
        Ok(text) => {
            ok = false;
            println!("text extractor: read {text:?}, expected the sample text")
        }
        Err(e) => {
            ok = false;
            println!("text extractor: failed: {e}")
        }
    }
    match rimelight_picture::load_animation(&dir.join("sample.gif")) {
        Some(a) => println!(
            "gif: ok, {} frames, first {} ms",
            a.frames.len(),
            a.frames[0].1
        ),
        None => {
            ok = false;
            println!("gif: no frames")
        }
    }
    match capture::grab::screenshot() {
        Ok((pic, rect)) => println!(
            "screenshot: ok, {}x{} at {},{}",
            pic.w, pic.h, rect.x, rect.y
        ),
        Err(e) => {
            ok = false;
            println!("screenshot: failed: {e}")
        }
    }
    let path = dir.join("selftest-recording.mp4");
    let _ = std::fs::remove_file(&path);
    let rect = rimelight_platform::ScreenRect {
        x: 0,
        y: 0,
        w: 640,
        h: 360,
    };
    match capture::recorder::start(capture::recorder::Options {
        screen: Some((rect, 30)),
        camera: None,
        system_audio: true,
        mic: false,
        path,
        mic_name: None,
    }) {
        Ok(rec) => {
            println!("recording: started {:?}", rec.notes);
            std::thread::sleep(Duration::from_secs(2));
            match rec
                .finish()
                .and_then(|p| rimelight_media::engine::player::probe(&p).map(|i| (p, i)))
            {
                Ok((p, i)) => println!(
                    "recording: ok, {} ms, video {:?}, {} audio tracks, {}",
                    i.duration_ms,
                    i.video,
                    i.audio.len(),
                    p.display()
                ),
                Err(e) => {
                    ok = false;
                    println!("recording: failed: {e}")
                }
            }
        }
        Err(e) => {
            ok = false;
            println!("recording: failed to start: {e}")
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Decodes the start of a file's sound track (self-test).
fn decode_some_audio(path: &std::path::Path) -> Result<usize, String> {
    use ffmpeg_next as ff;
    let mut ictx = rimelight_media::engine::player::open_input(path)?;
    let (idx, ctx) = {
        let st = ictx
            .streams()
            .best(ff::media::Type::Audio)
            .ok_or("no sound track")?;
        (
            st.index(),
            ff::codec::context::Context::from_parameters(st.parameters())
                .map_err(|e| e.to_string())?,
        )
    };
    let mut dec = ctx.decoder().audio().map_err(|e| e.to_string())?;
    let mut frame = ff::frame::Audio::empty();
    let mut samples = 0;
    for (st, pkt) in ictx.packets() {
        if st.index() != idx {
            continue;
        }
        dec.send_packet(&pkt).map_err(|e| e.to_string())?;
        while dec.receive_frame(&mut frame).is_ok() {
            samples += frame.samples();
        }
        if samples > 48_000 {
            break;
        }
    }
    if samples == 0 {
        Err("no samples".into())
    } else {
        Ok(samples)
    }
}

fn main() -> ExitCode {
    // First: during install, update and uninstall Velopack runs its hooks here and exits.
    rimelight_update::init(|app| {
        app.on_after_install_fast_callback(|_| install::after_install())
            .on_after_update_fast_callback(|_| install::after_update())
            // The installer starts the app once when it finishes: that run shows the setup window instead of the app.
            .on_first_run(|_| FIRST_RUN.store(true, std::sync::atomic::Ordering::Relaxed))
            .on_before_uninstall_fast_callback(|_| install::unregister_all())
    });
    // The hook log (rimelight_platform::log_line) goes to the data folder from here on.
    rimelight_platform::set_log_file(prefs::data_dir().join("hook-log.txt"));
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(path) = &args.write_icon {
        return if install::write_icon(path) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if let Some(dir) = std::env::var_os("RIMELIGHT_MEDIA_SELFTEST") {
        return selftest(&PathBuf::from(dir));
    }

    let checking = args.screenshot.is_some();

    // One running copy: hand this start's request to it and leave.
    if !checking {
        let request = if let Some(f) = &args.file {
            instance::Request::Open(f.display().to_string())
        } else if let Some(m) = &args.snip {
            instance::Request::Snip(m.clone())
        } else if args.background {
            instance::Request::Nothing
        } else {
            instance::Request::Show
        };
        // A hand-over names the copy it replaces: that copy has to end before this one takes the start lock.
        if let Some(pid) = args.after {
            rimelight_platform::wait_for_exit(pid, 15_000);
        }
        // Explorer starts one copy per file when several are opened together, all at once. Only the copy holding the
        // start lock goes on; the others wait for it to start listening and hand it their file. A copy that still can't
        // get the lock after 10 s says so in the hook log, and keeps trying for 30 s more before it gives up.
        let mut waited = 0;
        loop {
            if !args.takeover && instance::forward(&request) {
                return ExitCode::SUCCESS;
            }
            if rimelight_platform::take_start_lock("Local\\RimelightMedia.Instance") {
                break;
            }
            if waited == 100 {
                rimelight_platform::log_line(
                    "the start lock is still held after 10 s: waiting longer",
                );
            }
            if waited >= 400 {
                rimelight_platform::log_line(
                    "gave up waiting for the start lock after 40 s: this start did nothing",
                );
                // The logger thread writes the line in the background: give it a moment before the exit.
                std::thread::sleep(Duration::from_millis(300));
                return ExitCode::FAILURE;
            }
            waited += 1;
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let mut selector = slint::BackendSelector::new().backend_name("winit".into());
    // In the tray (no window) the software renderer: about 40 MB less than the GPU one, which Windows sets up even for a
    // window that is never shown. Snips draw fine with it. Showing the window hands over to a fresh GPU copy (app::handoff).
    if args.background && !checking {
        selector = selector.renderer_name("software".into());
    }
    selector = selector.with_winit_window_attributes_hook(move |attrs| {
        // Our own window class: the creation hook (rimelight_platform::start_no_window_animations) turns animations off for it.
        #[cfg(windows)]
        let attrs = {
            use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows as _;
            attrs.with_class_name(WINDOW_CLASS)
        };
        if !checking {
            return attrs;
        }
        // A check never takes focus and stays behind the person's windows (a covered window still draws on Windows).
        let attrs = attrs.with_active(false);
        #[cfg(windows)]
        let attrs =
            attrs.with_window_level(slint::winit_030::winit::window::WindowLevel::AlwaysOnBottom);
        attrs
    });
    if let Err(e) = selector.select() {
        eprintln!("{TOOL}: no window system: {e}");
        return ExitCode::FAILURE;
    }
    // No window of ours zooms or fades in: animations are turned off as each one is created (on this, the UI thread).
    rimelight_platform::start_no_window_animations(WINDOW_CLASS);

    let app = match app::build(args.background && !checking) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            return ExitCode::FAILURE;
        }
    };

    if !checking {
        app::start_hotkeys(&app);
        instance::serve(app::handle_request);
        let keep = app.prefs.borrow().background;
        capturectl::ensure_tray(&app, keep);
    }

    if let Some(page) = &args.page {
        app::go(&app, page);
    }
    if let Some(file) = &args.file {
        app::queue_open(&app, file.clone());
    }
    // The installer's first run: the setup window instead of the app.
    let first_run =
        !checking && FIRST_RUN.load(std::sync::atomic::Ordering::Relaxed) && setupctl::show(&app);
    if !(args.background && !checking) && !first_run {
        if let Err(e) = app.ui.show() {
            eprintln!("{TOOL}: {e}");
            return ExitCode::FAILURE;
        }
        if !checking {
            app::center_window(app.ui.window());
        }
    }
    if let Some(mode) = &args.snip {
        capturectl::snip(&app, mode);
    }
    if !checking {
        app::check_updates(&app, false);
    }

    // The check: about 120 frames (~2 s), then the window's picture.
    let _check_timer = args.screenshot.map(|out| {
        let timer = slint::Timer::default();
        let frames = std::cell::Cell::new(0u32);
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                app::with_app(|a| {
                    a.ui.window().request_redraw();
                    frames.set(frames.get() + 1);
                    if frames.get() < 120 {
                        return;
                    }
                    let code = match a.ui.window().take_snapshot() {
                        Ok(buf) => match image::RgbaImage::from_raw(
                            buf.width(),
                            buf.height(),
                            buf.as_bytes().to_vec(),
                        ) {
                            Some(img) => {
                                if let Some(dir) = out.parent() {
                                    let _ = std::fs::create_dir_all(dir);
                                }
                                match img.save_with_format(&out, image::ImageFormat::Png) {
                                    Ok(()) => 0,
                                    Err(e) => {
                                        eprintln!("{TOOL}: could not save {}: {e}", out.display());
                                        1
                                    }
                                }
                            }
                            None => 1,
                        },
                        Err(e) => {
                            eprintln!("{TOOL}: could not take the window's picture: {e}");
                            1
                        }
                    };
                    if code != 0 {
                        std::process::exit(code);
                    }
                    let _ = slint::quit_event_loop();
                });
            },
        );
        timer
    });

    let result = slint::run_event_loop_until_quit();
    drop(_check_timer);
    app::shutdown();
    if !checking {
        instance::release();
    }
    drop(app);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{TOOL}: {e}");
            ExitCode::FAILURE
        }
    }
}
