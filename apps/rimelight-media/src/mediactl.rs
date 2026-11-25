//! The player page: open, play, seek, volume, audio and subtitle tracks, repeat, frame snapshots and trimming.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::app::{App, go, post, remember, toast, update_title, with_app};
use crate::files;
use crate::ui;
use rimelight_media::engine::player::{self, MediaInfo, Player, SUB_EXTERNAL, SUB_OFF};
use rimelight_media::engine::{export, subs};

#[derive(Clone, PartialEq)]
enum SubChoice {
    Off,
    Stream(usize),
    File(PathBuf),
}

pub struct MediaState {
    pub path: Option<PathBuf>,
    info: Option<MediaInfo>,
    player: Option<Player>,
    audio_track: Option<usize>,
    sub: SubChoice,
    sub_files: Vec<PathBuf>,
    preview: Option<f32>,
    /// When the seek bar was last dragged. A drag that never got its release drops the preview after a while.
    preview_at: Option<Instant>,
    trim: (u64, u64),
    /// Earlier trim ranges to return to (undo), and ranges undone (redo).
    trim_undo: Vec<(u64, u64)>,
    trim_redo: Vec<(u64, u64)>,
    /// When the trim was last edited (edits close together are one undo step).
    trim_edit_at: Option<Instant>,
    muted: bool,
    /// Repeat: 0 off, 1 once more, 2 forever.
    loop_mode: i32,
    /// Repeat-once has played its extra time.
    repeated: bool,
    last_pos_text: String,
    last_size: (u32, u32),
    generation: u64,
    speed: f32,
    sub_delay_ms: i64,
    sub_scale: f32,
    /// The picture subtitle on screen (by address), so it is only converted once.
    shown_sub_picture: usize,
    /// The subtitle choice before V turned them off.
    sub_before_off: Option<SubChoice>,
    /// Markers saved beside the file (the voice recorder's <name>.json), in milliseconds, in the order shown.
    markers: Vec<u64>,
    /// Background threads that open the file (probe, waveform): each with its cancel flag. Cancelled when the player
    /// is taken, so no file stays open.
    tasks: Vec<(Arc<AtomicBool>, JoinHandle<()>)>,
    /// The playlist: the files opened together (mediactl::open_set), or the audio or video files in a single opened
    /// file's folder (files::sibling_media), and the open file's place in it. Cleared on close.
    playlist: Vec<PathBuf>,
    playlist_index: usize,
    /// Whether the playlist panel is shown. Kept across files; off on close.
    playlist_on: bool,
    /// Whether the next item plays when one ends (the Continue button). Kept across files and on close.
    auto_next: bool,
}

/// Keeps a background thread so take_player can cancel and join it; finished ones are forgotten.
fn track(m: &mut MediaState, cancel: Arc<AtomicBool>, handle: JoinHandle<()>) {
    m.tasks.retain(|(_, h)| !h.is_finished());
    m.tasks.push((cancel, handle));
}

/// Playback speeds: the flyout's presets, also the steps of the step keys.
const SPEEDS: [f32; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0];

/// Trim edits within this long of each other (a drag) count as one undo step.
const TRIM_STEP_GAP: Duration = Duration::from_millis(500);

/// How many bars the waveform track draws.
const WAVE_BARS: usize = 300;

/// The speed as the button shows it: always two decimals ("1.00×", "1.25×").
fn speed_label(s: f32) -> String {
    format!("{s:.2}×")
}

impl Default for MediaState {
    fn default() -> Self {
        MediaState {
            path: None,
            info: None,
            player: None,
            audio_track: None,
            sub: SubChoice::Off,
            sub_files: Vec::new(),
            preview: None,
            preview_at: None,
            trim: (0, 0),
            trim_undo: Vec::new(),
            trim_redo: Vec::new(),
            trim_edit_at: None,
            muted: false,
            loop_mode: 0,
            repeated: false,
            last_pos_text: String::new(),
            last_size: (0, 0),
            generation: 0,
            speed: 1.0,
            sub_delay_ms: 0,
            sub_scale: 1.0,
            shown_sub_picture: 0,
            sub_before_off: None,
            markers: Vec::new(),
            tasks: Vec::new(),
            playlist: Vec::new(),
            playlist_index: 0,
            playlist_on: false,
            auto_next: true,
        }
    }
}

impl MediaState {
    /// Closes the player (and its sound device), stops the background threads that hold the file, and waits briefly
    /// for them all to let go of it.
    pub fn take_player(&mut self) {
        if let Some(pl) = self.player.take() {
            pl.release();
        }
        for (cancel, _) in &self.tasks {
            cancel.store(true, Relaxed);
        }
        let handles: Vec<JoinHandle<()>> = self.tasks.drain(..).map(|(_, h)| h).collect();
        player::join_within(handles, Duration::from_millis(500));
    }
}

fn player_ui(app: &Rc<App>) -> ui::Player<'_> {
    app.ui.global::<ui::Player>()
}

pub fn wire(app: &Rc<App>) {
    let p = player_ui(app);
    p.set_volume(app.prefs.borrow().volume);
    p.on_toggle_play(|| with_app(toggle_play));
    p.on_seek(|f| {
        with_app(move |a| {
            a.media.borrow_mut().preview = None;
            // Show the new spot at once, so the handle and the waveform do not jump back while the seek runs.
            a.ui.global::<ui::Player>().set_position(f);
            seek_fraction(a, f);
        })
    });
    p.on_seek_preview(|f| {
        with_app(move |a| {
            {
                let mut m = a.media.borrow_mut();
                m.preview = Some(f);
                m.preview_at = Some(Instant::now());
            }
            let dur = a.media.borrow().info.as_ref().map_or(0, |i| i.duration_ms);
            let at = files::clock((f64::from(f) * dur as f64) as u64);
            player_ui(a).set_time_text(format!("{at} / {}", files::clock(dur)).into());
        })
    });
    p.on_seek_marker(|i| with_app(move |a| seek_marker(a, i as usize)));
    p.on_skip(|secs| with_app(move |a| skip(a, i64::from(secs) * 1000)));
    p.on_set_volume(|v| {
        with_app(move |a| {
            player_ui(a).set_volume(v);
            a.prefs.borrow_mut().volume = v;
            if let Some(pl) = a.media.borrow().player.as_ref() {
                pl.set_volume(v);
            }
        })
    });
    p.on_toggle_mute(|| with_app(toggle_mute));
    p.on_toggle_info(|| with_app(toggle_info));
    p.on_toggle_loop(|| with_app(cycle_loop));
    p.on_pick_audio(|i| with_app(move |a| pick_audio(a, i as usize)));
    p.on_pick_subtitle(|i| with_app(move |a| pick_subtitle(a, i as usize)));
    p.on_load_subtitle_file(|| with_app(load_subtitle_file));
    p.on_snapshot(|| with_app(snapshot));
    p.on_trim_set(|start| with_app(move |a| trim_set(a, start)));
    p.on_trim_save(|| with_app(trim_save));
    p.on_save_in_place(|| with_app(save_in_place));
    p.on_trim_undo(|| with_app(|a| trim_step(a, true)));
    p.on_trim_redo(|| with_app(|a| trim_step(a, false)));
    p.on_trim_discard(|| with_app(trim_discard));
    p.on_share(|| with_app(share));
    p.on_toggle_playlist(|| with_app(toggle_playlist));
    p.on_play_item(|i| with_app(move |a| play_item(a, i.max(0) as usize)));
    p.on_play_next(|| with_app(|a| play_step(a, 1)));
    p.on_play_prev(|| with_app(|a| play_step(a, -1)));
    p.set_auto_next(app.media.borrow().auto_next);
    p.on_toggle_auto_next(|| with_app(toggle_auto_next));
    p.on_open_folder(|| {
        with_app(|a| {
            let path = a.media.borrow().path.clone();
            if let Some(path) = path {
                files::reveal(&path);
            }
        })
    });
    p.on_pick_speed(|i| {
        with_app(move |a| set_speed(a, SPEEDS.get(i as usize).copied().unwrap_or(1.0)))
    });
    p.on_speed_step(|d| with_app(move |a| speed_step(a, d)));
    // The speed flyout (slider and presets): a speed from 0.25x to 3x, rounded to steps of 0.05. No toast.
    p.on_set_speed(|s| {
        with_app(move |a| {
            let speed = ((s.clamp(0.25, 3.0) / 0.05).round() * 0.05).clamp(0.25, 3.0);
            apply_speed(a, speed);
        })
    });
    p.on_trim_drag(|start, frac| with_app(move |a| trim_drag(a, start, frac)));
    p.on_sub_delay(|ms| with_app(move |a| sub_delay(a, i64::from(ms))));
    p.on_sub_size(|d| with_app(move |a| sub_size(a, d)));
    refresh_speeds(app);

    app.tick
        .start(slint::TimerMode::Repeated, Duration::from_millis(8), || {
            with_app(tick)
        });
}

/// Opens one file on its own. The playlist becomes the folder's files of the same kind (files::sibling_media), so
/// Next and Previous walk the folder.
pub fn open(app: &Rc<App>, path: &Path) {
    {
        let mut m = app.media.borrow_mut();
        m.playlist = match files::kind_of(path) {
            Some(kind @ (files::Kind::Audio | files::Kind::Video)) => {
                files::sibling_media(path, kind)
            }
            _ => vec![path.to_path_buf()],
        };
    }
    load(app, path);
}

/// Opens several audio or video files picked or dropped together. The playlist is exactly these files, of the first
/// file's kind (others are ignored), in the order given.
pub fn open_set(app: &Rc<App>, paths: &[PathBuf]) {
    let Some(first) = paths.first() else { return };
    let Some(kind) = files::kind_of(first) else {
        return;
    };
    let mut list: Vec<PathBuf> = Vec::new();
    for p in paths {
        if files::kind_of(p) == Some(kind) && !list.contains(p) {
            list.push(p.clone());
        }
    }
    app.media.borrow_mut().playlist = list;
    load(app, first);
}

/// Starts a file that is already in the playlist: sets its place and loads it. Shared by open, open_set, the panel,
/// Next and Previous, and auto-advance.
fn load(app: &Rc<App>, path: &Path) {
    // Stop what was playing.
    let generation = {
        let mut m = app.media.borrow_mut();
        m.take_player();
        m.generation += 1;
        m.path = Some(path.to_path_buf());
        m.info = None;
        m.playlist_index = m.playlist.iter().position(|p| p == path).unwrap_or(0);
        m.generation
    };
    refresh_playlist(app);
    let p = player_ui(app);
    p.set_buffering(true);
    p.set_error(SharedString::new());
    p.set_subtitle(SharedString::new());
    p.set_meta_text(SharedString::new());
    p.set_has_wave(false);
    p.set_wave_bars(ModelRc::default());
    set_markers(app, &[], 1);
    p.set_playing(false);
    p.set_position(0.0);
    p.set_trim_mode(false);
    p.set_frame(slint::Image::default());
    p.set_media_title(files::file_name(path).into());
    p.set_media_detail(SharedString::new());
    p.set_info_name(files::file_name(path).into());
    p.set_info_detail(SharedString::new());
    p.set_has_video(files::kind_of(path) == Some(files::Kind::Video));
    go(
        app,
        if files::kind_of(path) == Some(files::Kind::Audio) {
            "audio"
        } else {
            "video"
        },
    );
    let path = path.to_path_buf();
    let cancel = Arc::new(AtomicBool::new(false));
    let probe_cancel = Arc::clone(&cancel);
    let handle = std::thread::spawn(move || {
        if probe_cancel.load(Relaxed) {
            return;
        }
        let result = player::probe(&path);
        post(move |a| {
            if a.media.borrow().generation != generation {
                return;
            }
            match result {
                Ok(info) => start(a, &path, info),
                Err(e) => {
                    let p = player_ui(a);
                    p.set_buffering(false);
                    p.set_error(e.into());
                }
            }
        });
    });
    track(&mut app.media.borrow_mut(), cancel, handle);
}

/// Whether the trim range was moved from the whole file (an edit not yet saved or discarded).
fn trim_dirty(m: &MediaState) -> bool {
    m.info
        .as_ref()
        .is_some_and(|i| m.trim != (0, i.duration_ms))
}

/// Pushes the playlist to the Player page: the items, which one is current, the panel's state and whether Previous
/// and Next have a target.
fn refresh_playlist(app: &Rc<App>) {
    let (items, index, on) = {
        let m = app.media.borrow();
        let items: Vec<ui::PlaylistItem> = m
            .playlist
            .iter()
            .enumerate()
            .map(|(i, p)| ui::PlaylistItem {
                name: files::file_name(p).into(),
                current: i == m.playlist_index,
                index: i as i32,
            })
            .collect();
        (items, m.playlist_index, m.playlist_on)
    };
    let len = items.len();
    let p = player_ui(app);
    p.set_playlist(ModelRc::new(VecModel::from(items)));
    p.set_has_prev(index > 0);
    p.set_has_next(index + 1 < len);
    p.set_playlist_on(on);
}

fn toggle_playlist(app: &Rc<App>) {
    {
        let mut m = app.media.borrow_mut();
        m.playlist_on = !m.playlist_on;
    }
    refresh_playlist(app);
}

/// The Continue button: whether the next item plays when one ends.
fn toggle_auto_next(app: &Rc<App>) {
    let on = {
        let mut m = app.media.borrow_mut();
        m.auto_next = !m.auto_next;
        m.auto_next
    };
    player_ui(app).set_auto_next(on);
}

/// Opens the playlist item at `i`, if there is one (the panel's rows, Next and Previous).
fn play_item(app: &Rc<App>, i: usize) {
    let path = app.media.borrow().playlist.get(i).cloned();
    if let Some(path) = path {
        load(app, &path);
        remember(app, &path);
    }
}

/// Next (+1) or Previous (-1). No wrap: at either end nothing happens.
fn play_step(app: &Rc<App>, delta: isize) {
    let target = {
        let m = app.media.borrow();
        let i = m.playlist_index as isize + delta;
        (i >= 0 && (i as usize) < m.playlist.len()).then_some(i as usize)
    };
    if let Some(i) = target {
        play_item(app, i);
    }
}

fn output_size(app: &Rc<App>) -> (u32, u32) {
    let s = app.ui.window().size();
    (s.width.max(320), s.height.max(240))
}

fn start(app: &Rc<App>, path: &Path, info: MediaInfo) {
    let p = player_ui(app);
    p.set_has_video(info.video.is_some());
    // The extension guessed the tab; the file decides (a .mkv with only sound is music).
    let page = app.ui.global::<ui::Shell>().get_page();
    if matches!(page.as_str(), "audio" | "video") {
        go(
            app,
            if info.video.is_some() {
                "video"
            } else {
                "audio"
            },
        );
    }
    p.set_video_w(info.width.max(1) as i32);
    p.set_video_h(info.height.max(1) as i32);
    p.set_has_sub_picture(false);
    p.set_media_title(info.title.clone().into());
    let mut detail = info.detail.clone();
    if info.video.is_some() {
        detail = format!("{} × {}", info.width, info.height);
    }
    p.set_media_detail(detail.into());
    p.set_time_text(format!("{} / {}", files::clock(0), files::clock(info.duration_ms)).into());
    p.set_meta_text(meta_text(path, &info).into());
    p.set_info_detail(info_detail(path, &info).into());
    // Markers saved beside the file: ticks on the seek bar.
    let markers = read_markers(path, info.duration_ms);
    set_markers(app, &markers, info.duration_ms);
    app.media.borrow_mut().markers = markers;
    // The waveform is decoded in the background, from the sound track that plays (video files included).
    if let Some(idx) = info
        .audio
        .iter()
        .find(|t| t.default)
        .or(info.audio.first())
        .map(|t| t.index)
    {
        let wave_path = path.to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        let wave_cancel = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            // Only a track with real sound gets a waveform (an all-silent or undecodable track keeps none).
            if let Ok(peaks) = player::peaks(&wave_path, idx, WAVE_BARS, &wave_cancel) {
                if peaks.iter().any(|v| *v > 0.0) {
                    post(move |a| {
                        if a.media.borrow().path.as_deref() == Some(wave_path.as_path()) {
                            let p = player_ui(a);
                            p.set_wave_bars(ModelRc::new(VecModel::from(peaks)));
                            p.set_has_wave(true);
                        }
                    });
                }
            }
        });
        track(&mut app.media.borrow_mut(), cancel, handle);
    }
    let sub_files = subs::files_beside(path);
    // A default subtitle stream, or a file beside the video, is not turned on by itself (VLC-like: off unless marked
    // as forced/default). Default-marked text streams are turned on.
    let default_sub = info
        .subtitles
        .iter()
        .find(|t| t.default && !t.bitmap)
        .map(|t| t.index);
    let size = output_size(app);
    let volume = app.prefs.borrow().volume;
    match Player::open(path, &info, None, 0, size) {
        Ok(pl) => {
            pl.set_volume(volume);
            {
                let m = app.media.borrow();
                pl.set_muted(m.muted);
                pl.set_looping(m.loop_mode == 2);
            }
            {
                let m = app.media.borrow();
                pl.set_speed(m.speed);
                pl.set_sub_delay(m.sub_delay_ms);
            }
            pl.play();
            let mut m = app.media.borrow_mut();
            m.shown_sub_picture = 0;
            m.repeated = false;
            m.audio_track = info
                .audio
                .iter()
                .find(|t| t.default)
                .or(info.audio.first())
                .map(|t| t.index);
            m.trim = (0, info.duration_ms);
            m.trim_undo.clear();
            m.trim_redo.clear();
            m.trim_edit_at = None;
            m.sub_files = sub_files;
            m.sub = SubChoice::Off;
            m.last_size = size;
            m.player = Some(pl);
            m.info = Some(info);
        }
        Err(e) => {
            p.set_buffering(false);
            p.set_error(e.into());
            return;
        }
    }
    if let Some(idx) = default_sub {
        set_sub(app, SubChoice::Stream(idx));
    }
    refresh_tracks(app);
    update_trim_text(app);
    update_title(app);
}

fn tick(app: &Rc<App>) {
    let mut m = app.media.borrow_mut();
    if m.preview.is_some()
        && m.preview_at
            .is_some_and(|t| t.elapsed() > Duration::from_millis(400))
    {
        m.preview = None;
        m.preview_at = None;
    }
    let size = output_size(app);
    let Some(pl) = m.player.as_ref() else { return };
    if size != m.last_size {
        pl.set_output_size(size.0, size.1);
    }
    // Repeat once: play it one more time after it ends.
    let replay = m.loop_mode == 1 && !m.repeated && pl.ended();
    if replay {
        pl.play();
    }
    // Rebuilds the sound output if the device was lost (cheap when nothing is wrong).
    pl.check_output();
    let frame = pl.take_frame();
    let pos = pl.position_ms();
    let dur = pl.duration_ms().max(1);
    let playing = pl.is_playing();
    let buffering = pl.is_buffering() && playing;
    let ended = pl.ended();
    let (subtitle, sub_picture) = pl.subtitle_now();
    let error = pl.error();
    // Auto-advance: when the item ends, and nothing repeats it or waits on an unsaved trim, the next item plays.
    let advance = if ended && m.auto_next && m.loop_mode == 0 && !trim_dirty(&m) {
        m.playlist.get(m.playlist_index + 1).cloned()
    } else {
        None
    };
    m.last_size = size;
    if replay {
        m.repeated = true;
    }
    let preview = m.preview;
    let text = format!("{} / {}", files::clock(pos), files::clock(dur));
    let text_changed = text != m.last_pos_text;
    if text_changed {
        m.last_pos_text = text.clone();
    }
    drop(m);

    let p = player_ui(app);
    if let Some(f) = frame {
        p.set_frame(f.picture.into_slint_image());
    }
    if preview.is_none() {
        // At the end the clock can stop a few ms short of the length; show the whole track as played.
        let frac = if ended {
            1.0
        } else {
            (pos as f64 / dur as f64) as f32
        };
        if (p.get_position() - frac).abs() > 0.0005 {
            p.set_position(frac);
        }
        if text_changed {
            p.set_time_text(text.into());
        }
    }
    if p.get_playing() != playing {
        p.set_playing(playing);
    }
    if p.get_buffering() != buffering {
        p.set_buffering(buffering);
    }
    if p.get_subtitle().as_str() != subtitle {
        p.set_subtitle(subtitle.into());
    }
    show_sub_picture(app, sub_picture);
    if let Some(e) = error {
        if p.get_error().as_str() != e {
            p.set_error(e.into());
        }
    } else if p.get_error().starts_with("Sound output") {
        // The output recovered: its error goes away (other errors stay until the next file).
        p.set_error(Default::default());
    }
    if let Some(next) = advance {
        load(app, &next);
        remember(app, &next);
    }
}

fn toggle_play(app: &Rc<App>) {
    let m = app.media.borrow();
    if let Some(pl) = m.player.as_ref() {
        if pl.is_playing() {
            pl.pause()
        } else {
            pl.play()
        }
    }
}

pub fn pause(app: &Rc<App>) {
    if let Some(pl) = app.media.borrow().player.as_ref() {
        pl.pause();
    }
}

/// Leaving the player page: video pauses, music keeps playing.
pub fn leaving(app: &Rc<App>) {
    if player_ui(app).get_has_video() {
        pause(app);
    }
    if app.ui.global::<ui::Shell>().get_full_screen() {
        crate::app::toggle_full_screen(app);
    }
}

/// The markers saved beside a recording (`<name>.json`, written by the voice recorder): their times in milliseconds,
/// those within the file's length. Missing or unreadable files give none.
fn read_markers(path: &Path, duration_ms: u64) -> Vec<u64> {
    let Ok(text) = std::fs::read_to_string(path.with_extension("json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v.get("markers")
        .and_then(|m| m.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| m.get("ms").and_then(|ms| ms.as_u64()))
                .filter(|ms| *ms <= duration_ms)
                .collect()
        })
        .unwrap_or_default()
}

/// Pushes the marker ticks (place on the seek bar, and label) to the player page.
fn set_markers(app: &Rc<App>, markers: &[u64], duration_ms: u64) {
    let dur = duration_ms.max(1) as f64;
    let ticks: Vec<f32> = markers.iter().map(|ms| (*ms as f64 / dur) as f32).collect();
    let labels: Vec<SharedString> = markers
        .iter()
        .enumerate()
        .map(|(i, ms)| format!("Marker {} · {}", i + 1, files::clock(*ms)).into())
        .collect();
    let p = player_ui(app);
    p.set_marker_ticks(ModelRc::new(VecModel::from(ticks)));
    p.set_marker_labels(ModelRc::new(VecModel::from(labels)));
}

/// Clicking a marker: seeks to its time.
fn seek_marker(app: &Rc<App>, i: usize) {
    let ms = app.media.borrow().markers.get(i).copied();
    let Some(ms) = ms else { return };
    {
        let mut m = app.media.borrow_mut();
        m.preview = None;
        if let Some(pl) = m.player.as_ref() {
            pl.seek(ms);
        }
    }
}

fn seek_fraction(app: &Rc<App>, f: f32) {
    let m = app.media.borrow();
    if let Some(pl) = m.player.as_ref() {
        pl.seek((f64::from(f.clamp(0.0, 1.0)) * pl.duration_ms() as f64) as u64);
    }
}

fn skip(app: &Rc<App>, delta_ms: i64) {
    let m = app.media.borrow();
    if let Some(pl) = m.player.as_ref() {
        let to = (pl.position_ms() as i64 + delta_ms).clamp(0, pl.duration_ms() as i64);
        pl.seek(to as u64);
    }
}

fn toggle_mute(app: &Rc<App>) {
    let mut m = app.media.borrow_mut();
    m.muted = !m.muted;
    if let Some(pl) = m.player.as_ref() {
        pl.set_muted(m.muted);
    }
    player_ui(app).set_muted(m.muted);
}

fn change_volume(app: &Rc<App>, delta: f32) {
    let v = (player_ui(app).get_volume() + delta).clamp(0.0, 1.0);
    player_ui(app).invoke_set_volume(v);
    toast(app, format!("Volume {}%", (v * 100.0).round()));
}

fn refresh_tracks(app: &Rc<App>) {
    let m = app.media.borrow();
    let Some(info) = m.info.as_ref() else { return };
    let audio: Vec<ui::Choice> = info
        .audio
        .iter()
        .map(|t| ui::Choice {
            label: t.label.clone().into(),
            detail: t.detail.clone().into(),
            selected: m.audio_track == Some(t.index),
        })
        .collect();
    let mut subs_list = vec![ui::Choice {
        label: "Off".into(),
        detail: SharedString::new(),
        selected: m.sub == SubChoice::Off,
    }];
    for t in &info.subtitles {
        subs_list.push(ui::Choice {
            label: t.label.clone().into(),
            detail: t.detail.clone().into(),
            selected: m.sub == SubChoice::Stream(t.index),
        });
    }
    for f in &m.sub_files {
        subs_list.push(ui::Choice {
            label: files::file_name(f).into(),
            detail: "File beside the video".into(),
            selected: m.sub == SubChoice::File(f.clone()),
        });
    }
    if let SubChoice::File(f) = &m.sub
        && !m.sub_files.contains(f)
    {
        subs_list.push(ui::Choice {
            label: files::file_name(f).into(),
            detail: "Subtitle file".into(),
            selected: true,
        });
    }
    subs_list.push(ui::Choice {
        label: "Add a subtitle file…".into(),
        detail: "SRT, VTT, ASS or SSA".into(),
        selected: false,
    });
    let p = player_ui(app);
    p.set_audio_tracks(ModelRc::new(VecModel::from(audio)));
    p.set_subtitle_tracks(ModelRc::new(VecModel::from(subs_list)));
}

fn pick_audio(app: &Rc<App>, i: usize) {
    let (path, info, pos, playing, idx) = {
        let m = app.media.borrow();
        let (Some(path), Some(info), Some(pl)) =
            (m.path.clone(), m.info.clone(), m.player.as_ref())
        else {
            return;
        };
        let Some(t) = info.audio.get(i) else { return };
        (
            path,
            info.clone(),
            pl.position_ms(),
            pl.is_playing(),
            t.index,
        )
    };
    let size = output_size(app);
    // A new player on the other track, from the same spot.
    app.media.borrow_mut().take_player();
    match Player::open(&path, &info, Some(idx), pos, size) {
        Ok(pl) => {
            let mut m = app.media.borrow_mut();
            pl.set_volume(app.prefs.borrow().volume);
            pl.set_muted(m.muted);
            pl.set_looping(m.loop_mode == 2);
            pl.set_speed(m.speed);
            pl.set_sub_delay(m.sub_delay_ms);
            if playing {
                pl.play();
            }
            m.audio_track = Some(idx);
            m.player = Some(pl);
            let sub = m.sub.clone();
            drop(m);
            set_sub(app, sub);
        }
        Err(e) => player_ui(app).set_error(e.into()),
    }
    refresh_tracks(app);
}

fn set_sub(app: &Rc<App>, choice: SubChoice) {
    let mut m = app.media.borrow_mut();
    let Some(pl) = m.player.as_ref() else { return };
    match &choice {
        SubChoice::Off => pl.set_subtitles(SUB_OFF, Vec::new()),
        SubChoice::Stream(idx) => pl.set_subtitles(*idx as i64, Vec::new()),
        SubChoice::File(f) => match subs::load_file(f) {
            Ok(cues) => pl.set_subtitles(SUB_EXTERNAL, cues),
            Err(e) => {
                drop(m);
                toast(app, format!("Could not read the subtitles: {e}"));
                return;
            }
        },
    }
    m.sub = choice;
}

fn pick_subtitle(app: &Rc<App>, i: usize) {
    let choice = {
        let m = app.media.borrow();
        let Some(info) = m.info.as_ref() else { return };
        let n_streams = info.subtitles.len();
        if i == 0 {
            Some(SubChoice::Off)
        } else if i <= n_streams {
            Some(SubChoice::Stream(info.subtitles[i - 1].index))
        } else if i - 1 - n_streams < m.sub_files.len() {
            Some(SubChoice::File(m.sub_files[i - 1 - n_streams].clone()))
        } else if matches!(&m.sub, SubChoice::File(f) if !m.sub_files.contains(f))
            && i == 1 + n_streams + m.sub_files.len()
        {
            None
        } else {
            drop(m);
            load_subtitle_file(app);
            return;
        }
    };
    if let Some(c) = choice {
        set_sub(app, c);
    }
    refresh_tracks(app);
}

fn load_subtitle_file(app: &Rc<App>) {
    let dir = app
        .media
        .borrow()
        .path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut d = rfd::FileDialog::new()
        .set_title("Add subtitles")
        .add_filter("Subtitles", files::SUBTITLE_EXTS);
    if let Some(dir) = dir {
        d = d.set_directory(dir);
    }
    if let Some(f) = d.pick_file() {
        set_sub(app, SubChoice::File(f));
        refresh_tracks(app);
    }
}

fn snapshot(app: &Rc<App>) {
    let (path, pos) = {
        let m = app.media.borrow();
        let (Some(path), Some(pl)) = (m.path.clone(), m.player.as_ref()) else {
            return;
        };
        (path, pl.position_ms())
    };
    let dir = app.prefs.borrow().screenshot_dir.clone();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Video")
        .to_string();
    std::thread::spawn(move || {
        let result = player::frame_at(&path, pos).and_then(|pic| {
            let out = files::free_path(
                &dir,
                &format!("{stem} {}", files::clock(pos).replace(':', "-")),
                "png",
            );
            let pic = rimelight_picture::Picture {
                w: pic.width,
                h: pic.height,
                rgba: pic.bytes().to_vec(),
            };
            rimelight_picture::save(&pic, &out).map(|()| out)
        });
        post(move |a| match result {
            Ok(out) => toast(a, format!("Frame saved: {}", files::file_name(&out))),
            Err(e) => toast(a, format!("Could not save the frame: {e}")),
        });
    });
}

fn update_trim_text(app: &Rc<App>) {
    let m = app.media.borrow();
    let dur = m.info.as_ref().map_or(1, |i| i.duration_ms.max(1));
    let (s, e) = m.trim;
    let p = player_ui(app);
    p.set_trim_start((s as f64 / dur as f64) as f32);
    p.set_trim_end((e as f64 / dur as f64) as f32);
    p.set_trim_text(
        format!(
            "Keep {} to {} ({})",
            files::clock(s),
            files::clock(e),
            files::clock(e.saturating_sub(s))
        )
        .into(),
    );
    p.set_can_undo(!m.trim_undo.is_empty());
    p.set_can_redo(!m.trim_redo.is_empty());
    p.set_trim_dirty(
        m.info
            .as_ref()
            .is_some_and(|i| (s, e) != (0, i.duration_ms)),
    );
}

/// Remembers the trim before an edit, so undo can return to it. Edits close together (a drag) are one step.
/// `force` always records (for discard).
fn record_trim(m: &mut MediaState, force: bool) {
    let now = Instant::now();
    let new_step = m
        .trim_edit_at
        .is_none_or(|t| now.duration_since(t) > TRIM_STEP_GAP);
    if force || new_step {
        m.trim_undo.push(m.trim);
        m.trim_redo.clear();
    }
    m.trim_edit_at = Some(now);
}

fn trim_set(app: &Rc<App>, start: bool) {
    {
        let mut m = app.media.borrow_mut();
        let Some(pos) = m.player.as_ref().map(Player::position_ms) else {
            return;
        };
        let dur = m.info.as_ref().map_or(0, |i| i.duration_ms);
        let (s, e) = m.trim;
        let new = if start {
            (pos.min(e.saturating_sub(100)), e)
        } else {
            (s, pos.max(s + 100).min(dur))
        };
        if new != m.trim {
            record_trim(&mut m, false);
            m.trim = new;
        }
    }
    update_trim_text(app);
}

/// Undo (back) or redo (forward) of the trim range.
fn trim_step(app: &Rc<App>, undo: bool) {
    {
        let mut m = app.media.borrow_mut();
        let popped = if undo {
            m.trim_undo.pop()
        } else {
            m.trim_redo.pop()
        };
        let Some(prev) = popped else { return };
        let current = m.trim;
        if undo {
            m.trim_redo.push(current);
        } else {
            m.trim_undo.push(current);
        }
        m.trim = prev;
        m.trim_edit_at = None;
    }
    update_trim_text(app);
}

/// Discard: the trim goes back to the whole file (undoable).
fn trim_discard(app: &Rc<App>) {
    {
        let mut m = app.media.borrow_mut();
        let dur = m.info.as_ref().map_or(0, |i| i.duration_ms);
        if m.trim != (0, dur) {
            record_trim(&mut m, true);
            m.trim = (0, dur);
        }
    }
    update_trim_text(app);
}

fn share(app: &Rc<App>) {
    let Some(path) = app.media.borrow().path.clone() else {
        return;
    };
    if let Err(e) =
        rimelight_platform::share_file(crate::app::main_hwnd(app), &path, &files::file_name(&path))
    {
        toast(app, format!("Could not share: {e}"));
    }
}

/// Save: replaces the open file with the trimmed range, after a question (the same native dialog as the viewer's).
fn save_in_place(app: &Rc<App>) {
    let (path, (s, e)) = {
        let m = app.media.borrow();
        let Some(path) = m.path.clone() else { return };
        (path, m.trim)
    };
    crate::app::ask(
        app,
        "Replace the original?",
        &files::file_name(&path),
        "The file is replaced by the trimmed version. The cut parts can't be brought back.",
        "Replace",
        "",
        move |a, choice| {
            if choice == 0 {
                replace_with_trim(a, path, s, e);
            }
        },
    );
}

fn replace_with_trim(app: &Rc<App>, path: PathBuf, s: u64, e: u64) {
    let ext = files::ext_of(&path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Clip")
        .to_string();
    let tmp = path.with_file_name(format!("{stem} (trimming).{ext}"));
    // The file is let go first, so it can be replaced.
    {
        let mut m = app.media.borrow_mut();
        m.take_player();
        m.generation += 1;
    }
    player_ui(app).set_exporting(true);
    std::thread::spawn(move || {
        let result = export::trim(&path, &tmp, s, e)
            .and_then(|()| std::fs::rename(&tmp, &path).map_err(|err| err.to_string()));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        post(move |a| {
            player_ui(a).set_exporting(false);
            match result {
                Ok(()) => {
                    toast(a, format!("Saved {}", files::file_name(&path)));
                    open(a, &path);
                }
                Err(err) => {
                    // Nothing was replaced: the player comes back on the same file.
                    toast(a, format!("Could not save: {err}"));
                    open(a, &path);
                }
            }
        });
    });
}

fn trim_save(app: &Rc<App>) {
    let (path, (s, e)) = {
        let m = app.media.borrow();
        let Some(path) = m.path.clone() else { return };
        (path, m.trim)
    };
    let ext = files::ext_of(&path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Clip")
        .to_string();
    let suggested = format!("{stem} (trimmed).{ext}");
    let mut d = rfd::FileDialog::new()
        .set_title("Save trimmed copy")
        .set_file_name(&suggested)
        .add_filter(&ext.to_uppercase(), &[ext.as_str()]);
    if let Some(dir) = path.parent() {
        d = d.set_directory(dir);
    }
    let Some(out) = d.save_file() else { return };
    if out == path {
        toast(
            app,
            "Pick another name: the trimmed copy can't replace the file that is open.",
        );
        return;
    }
    player_ui(app).set_exporting(true);
    std::thread::spawn(move || {
        let result = export::trim(&path, &out, s, e);
        post(move |a| {
            player_ui(a).set_exporting(false);
            match result {
                Ok(()) => {
                    player_ui(a).set_trim_mode(false);
                    toast(a, format!("Saved {}", files::file_name(&out)));
                    remember(a, &out);
                }
                Err(err) => toast(a, format!("Could not trim: {err}")),
            }
        });
    });
}

pub fn on_key(app: &Rc<App>, text: &str, ctrl: bool, shift: bool) -> bool {
    use slint::platform::Key;
    let k = |key: Key| text == SharedString::from(key).as_str();
    if ctrl {
        return false;
    }
    let step = if shift { 30_000 } else { 5_000 };
    if text == " " || text == "k" {
        toggle_play(app);
    } else if k(Key::LeftArrow) {
        skip(app, -step);
    } else if k(Key::RightArrow) {
        skip(app, step);
    } else if k(Key::UpArrow) {
        change_volume(app, 0.05);
    } else if k(Key::DownArrow) {
        change_volume(app, -0.05);
    } else if text == "[" || text == "-" {
        speed_step(app, -1);
    } else if text == "]" || text == "+" {
        speed_step(app, 1);
    } else if text == "=" {
        set_speed(app, 1.0);
    } else if text == "g" {
        sub_delay(app, -100);
    } else if text == "h" {
        sub_delay(app, 100);
    } else if text == "G" {
        sub_size(app, -1);
    } else if text == "H" {
        sub_size(app, 1);
    } else if text == "v" {
        toggle_subs(app);
    } else if text == "m" {
        toggle_mute(app);
    } else if text == "f" {
        crate::app::toggle_full_screen(app);
    } else if text == "i" {
        toggle_info(app);
    } else if text == "l" {
        player_ui(app).invoke_toggle_loop();
    } else if text == "n" || text == "N" {
        play_step(app, 1);
    } else if text == "p" || text == "P" {
        play_step(app, -1);
    } else if k(Key::Home) {
        seek_fraction(app, 0.0);
    } else {
        return false;
    }
    true
}

/// Pushes the speed to the control's label and value. Each is set only when it differs, so the slider in the
/// flyout is not reset by its own change.
fn refresh_speeds(app: &Rc<App>) {
    let current = app.media.borrow().speed;
    let p = player_ui(app);
    let text = speed_label(current);
    if p.get_speed_text().as_str() != text {
        p.set_speed_text(text.into());
    }
    if (p.get_speed_value() - current).abs() > 0.0001 {
        p.set_speed_value(current);
    }
}

/// Sets the speed without a message (the flyout's slider and presets call this). Does nothing when it is unchanged.
fn apply_speed(app: &Rc<App>, speed: f32) {
    {
        let mut m = app.media.borrow_mut();
        if (m.speed - speed).abs() < 0.001 {
            return;
        }
        m.speed = speed;
        if let Some(pl) = m.player.as_ref() {
            pl.set_speed(speed);
        }
    }
    refresh_speeds(app);
}

fn set_speed(app: &Rc<App>, speed: f32) {
    apply_speed(app, speed);
    toast(app, format!("Speed {}", speed_label(speed)));
}

/// Moves the start or the end of the trim range to a fraction of the timeline (from a drag on a handle).
fn trim_drag(app: &Rc<App>, start: bool, frac: f32) {
    {
        let mut m = app.media.borrow_mut();
        let dur = m.info.as_ref().map_or(0, |i| i.duration_ms);
        let ms = (f64::from(frac.clamp(0.0, 1.0)) * dur as f64) as u64;
        let (s, e) = m.trim;
        let new = if start {
            (ms.min(e.saturating_sub(100)), e)
        } else {
            (s, ms.max(s + 100).min(dur))
        };
        if new != m.trim {
            record_trim(&mut m, false);
            m.trim = new;
        }
    }
    update_trim_text(app);
}

/// The speaker layout of a channel count ("stereo", "5.1", or "3 ch").
fn channel_name(n: u16) -> String {
    match n {
        0 => String::new(),
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        n => format!("{n} ch"),
    }
}

/// The Details panel text: one "Label: value" line per fact the file has, then the folder and the date changed.
/// Audio and video files share it; a video file also shows its picture's codec and size.
fn info_detail(path: &Path, info: &MediaInfo) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !info.container.is_empty() {
        lines.push(format!("Format: {}", info.container.to_uppercase()));
    }
    if info.video.is_some() {
        if !info.video_codec.is_empty() {
            lines.push(format!("Video codec: {}", info.video_codec));
        }
        lines.push(format!("Resolution: {} × {}", info.width, info.height));
        if info.fps > 0.0 {
            lines.push(format!("Frame rate: {:.2} fps", info.fps));
        }
    }
    lines.push(format!("Duration: {}", files::clock(info.duration_ms)));
    if info.bitrate > 0 {
        lines.push(format!("Bitrate: {} kbps", info.bitrate / 1000));
    }
    if !info.audio_codec.is_empty() {
        lines.push(format!("Audio codec: {}", info.audio_codec));
        if info.sample_rate > 0 {
            lines.push(format!(
                "Sample rate: {} kHz",
                f64::from(info.sample_rate) / 1000.0
            ));
        }
        if info.channels > 0 {
            lines.push(format!("Channels: {}", channel_name(info.channels)));
        }
    }
    for (label, value) in &info.tags {
        lines.push(format!("{label}: {value}"));
    }
    if let Ok(meta) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", files::human_size(meta.len())));
        if let Ok(modified) = meta.modified()
            && let Ok(age) = modified.elapsed()
        {
            let days = age.as_secs() / 86_400;
            lines.push(match days {
                0 => "Changed today".to_string(),
                1 => "Changed yesterday".to_string(),
                d => format!("Changed {d} days ago"),
            });
        }
    }
    lines.push(String::new());
    lines.push(files::folder_name(path));
    lines.join("\n")
}

/// Shows or hides the Details panel (the info button, or the I key).
fn toggle_info(app: &Rc<App>) {
    let p = player_ui(app);
    p.set_show_info(!p.get_show_info());
}

/// The status bar text: codec, bitrate, sample rate and channels (or resolution and fps), duration and size.
fn meta_text(path: &Path, info: &MediaInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if info.video.is_some() {
        parts.push(info.video_codec.clone());
        parts.push(format!("{} × {}", info.width, info.height));
        if info.fps > 0.0 {
            parts.push(format!("{:.2} fps", info.fps));
        }
    }
    if !info.audio_codec.is_empty() {
        parts.push(info.audio_codec.clone());
        if info.sample_rate > 0 {
            parts.push(format!("{} kHz", f64::from(info.sample_rate) / 1000.0));
        }
        parts.push(channel_name(info.channels));
    }
    if info.bitrate > 0 {
        parts.push(format!("{} kbps", info.bitrate / 1000));
    }
    parts.push(files::clock(info.duration_ms));
    if let Ok(meta) = std::fs::metadata(path) {
        parts.push(files::human_size(meta.len()));
    }
    parts.retain(|s| !s.is_empty());
    parts.join("  |  ")
}

fn speed_step(app: &Rc<App>, delta: i32) {
    let current = app.media.borrow().speed;
    let i = SPEEDS
        .iter()
        .position(|s| (*s - current).abs() < 0.001)
        .unwrap_or(3) as i32;
    let next = (i + delta).clamp(0, SPEEDS.len() as i32 - 1) as usize;
    set_speed(app, SPEEDS[next]);
}

fn sub_delay(app: &Rc<App>, change_ms: i64) {
    let ms = {
        let mut m = app.media.borrow_mut();
        m.sub_delay_ms = if change_ms == 0 {
            0
        } else {
            (m.sub_delay_ms + change_ms).clamp(-600_000, 600_000)
        };
        if let Some(pl) = m.player.as_ref() {
            pl.set_sub_delay(m.sub_delay_ms);
        }
        m.sub_delay_ms
    };
    let text = if ms == 0 {
        "0 ms".to_string()
    } else {
        format!("{ms:+} ms")
    };
    player_ui(app).set_sub_delay_text(text.clone().into());
    toast(app, format!("Subtitle delay {text}"));
}

fn sub_size(app: &Rc<App>, delta: i32) {
    let scale = {
        let mut m = app.media.borrow_mut();
        m.sub_scale = if delta == 0 {
            1.0
        } else {
            (m.sub_scale + 0.1 * delta as f32).clamp(0.5, 3.0)
        };
        m.sub_scale
    };
    player_ui(app).set_sub_scale(scale);
    toast(app, format!("Subtitle size {}%", (scale * 100.0).round()));
}

/// V: subtitles off, and back to the last choice.
fn toggle_subs(app: &Rc<App>) {
    let (current, before) = {
        let m = app.media.borrow();
        (m.sub.clone(), m.sub_before_off.clone())
    };
    if current != SubChoice::Off {
        app.media.borrow_mut().sub_before_off = Some(current);
        set_sub(app, SubChoice::Off);
        toast(app, "Subtitles off");
    } else if let Some(b) = before {
        set_sub(app, b);
        toast(app, "Subtitles on");
    } else {
        // Nothing chosen yet: the first text track, else a file beside the video.
        let first = {
            let m = app.media.borrow();
            let stream = m
                .info
                .as_ref()
                .and_then(|i| i.subtitles.first().map(|t| SubChoice::Stream(t.index)));
            stream.or_else(|| m.sub_files.first().map(|f| SubChoice::File(f.clone())))
        };
        match first {
            Some(c) => {
                set_sub(app, c);
                toast(app, "Subtitles on");
            }
            None => toast(
                app,
                "This video has no subtitles. Add a file from the subtitles menu.",
            ),
        }
    }
    refresh_tracks(app);
}

fn show_sub_picture(
    app: &Rc<App>,
    picture: Option<std::sync::Arc<rimelight_media::engine::bitmap_sub::SubPicture>>,
) {
    let p = player_ui(app);
    let key = picture
        .as_ref()
        .map_or(0, |pic| std::sync::Arc::as_ptr(pic) as usize);
    if app.media.borrow().shown_sub_picture == key {
        return;
    }
    app.media.borrow_mut().shown_sub_picture = key;
    let Some(pic) = picture else {
        p.set_has_sub_picture(false);
        return;
    };
    let (vw, vh) = (p.get_video_w().max(1) as f32, p.get_video_h().max(1) as f32);
    let buf =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&pic.rgba, pic.w, pic.h);
    p.set_sub_picture(slint::Image::from_rgba8(buf));
    p.set_sub_x(pic.x as f32 / vw);
    p.set_sub_y(pic.y as f32 / vh);
    p.set_sub_w(pic.w as f32 / vw);
    p.set_sub_h(pic.h as f32 / vh);
    p.set_has_sub_picture(true);
}
/// Repeat: off, then once more, then forever.
fn cycle_loop(app: &Rc<App>) {
    let mode = {
        let mut m = app.media.borrow_mut();
        m.loop_mode = (m.loop_mode + 1) % 3;
        m.repeated = false;
        if let Some(pl) = m.player.as_ref() {
            pl.set_looping(m.loop_mode == 2);
        }
        m.loop_mode
    };
    player_ui(app).set_loop_mode(mode);
    toast(
        app,
        ["Repeat off", "Repeat once", "Repeat forever"][mode as usize],
    );
}

/// Empties the Audio/Video tab: stops playback and frees the player and its pictures.
pub fn close(app: &Rc<App>) {
    {
        let mut m = app.media.borrow_mut();
        m.take_player();
        m.generation += 1;
        m.path = None;
        m.info = None;
        m.sub_files.clear();
        m.markers.clear();
        m.shown_sub_picture = 0;
        m.repeated = false;
        m.trim_undo.clear();
        m.trim_redo.clear();
        m.trim_edit_at = None;
        m.playlist.clear();
        m.playlist_index = 0;
        m.playlist_on = false;
    }
    refresh_playlist(app);
    let p = player_ui(app);
    p.set_frame(slint::Image::default());
    p.set_media_title(SharedString::new());
    p.set_media_detail(SharedString::new());
    p.set_has_video(false);
    p.set_playing(false);
    p.set_position(0.0);
    p.set_subtitle(SharedString::new());
    p.set_has_sub_picture(false);
    p.set_sub_picture(slint::Image::default());
    p.set_error(SharedString::new());
    p.set_buffering(false);
    p.set_trim_mode(false);
    p.set_can_undo(false);
    p.set_can_redo(false);
    p.set_trim_dirty(false);
    p.set_audio_tracks(ModelRc::default());
    p.set_subtitle_tracks(ModelRc::default());
    p.set_has_wave(false);
    p.set_wave_bars(ModelRc::default());
    set_markers(app, &[], 1);
    p.set_meta_text(SharedString::new());
    p.set_info_name(SharedString::new());
    p.set_info_detail(SharedString::new());
    p.set_show_info(false);
    crate::app::update_title(app);
}
