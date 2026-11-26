//! The Music page controller (ported from `crates/app/src/music.rs`, the piano studio).
//!
//! Plain Rust with no Slint. The controller holds the page state (the music blob, the session choices, the metronome,
//! the text drafts) and calls the music endpoints through [`MusicApi`]. Each state change is reported as a
//! [`MusicView`]. The Slint glue in `apps/desktop/src/pages/music.rs` turns those views into the models of
//! `ui/music.slint`.
//!
//! Threading: the API calls return at once and answer later on another thread (ehttp's worker). The controller locks
//! its state only around reads and writes, never across an API call, and `notify` is called after the lock is released.
//! The notify callback must hand the view to the UI thread itself (the glue uses `Weak::upgrade_in_event_loop`).
//!
//! Rebuilds: a view carries `version`. It changes only when the lists change shape (an add, a remove, a filter, a
//! tab). Typing does not notify at all: the field already shows the text, and rebuilding its row would drop focus. The
//! text is kept in this state, so the next rebuild shows it. Scalars (save status, metronome) change without a version.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use rimelight_sound::{ACCENT_CLICK, CLICK, Tone};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rimelight_personal_core::music::{
    Attachment, DEFAULT_REPERTOIRE_STATES, Exercise, MusicData, RepertoireItem, RoutineCategory,
    RoutineItem,
};
use rimelight_personal_core::{ApiClient, ApiError};

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@rimelight_personal_music`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The filter value that shows every stage.
pub const ALL_STAGES: &str = "all";

const BPM_MIN: f64 = 30.0;
const BPM_MAX: f64 = 260.0;
const TAP_MIN: f64 = 40.0;
const TAP_MAX: f64 = 280.0;
const TAP_RESTART: Duration = Duration::from_secs(2);
const METRONOME_DEFAULT_BPM: f64 = 120.0;
const EXERCISE_DEFAULT_BPM: f64 = 60.0;
/// The click on a subdivision that is not a beat (eighths, sixteenths, triplets): quieter and higher than a beat.
const SUBDIVISION_CLICK: Tone = Tone {
    hz: 1000.0,
    ms: 12,
    volume: 0.2,
};
/// The files the attachment picker offers: audio, PDF and images, as the drop zone accepts them.
const ATTACHMENT_FILES: &[(&str, &[&str])] = &[(
    "Audio, sheet music and images",
    &[
        "mp3", "wav", "m4a", "ogg", "flac", "aac", "pdf", "png", "jpg", "jpeg", "gif", "webp",
    ],
)];
const DIFFICULTY_MAX: i64 = 5;
const DEFAULT_DIFFICULTY: i64 = 3;
const BEATS_MIN: usize = 2;
const BEATS_MAX: usize = 8;

/// The API boundary: the three calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait MusicApi: Send + Sync {
    /// `GET /music`: the whole blob.
    fn load(&self, on_done: Box<dyn FnOnce(Result<MusicData, ApiError>) + Send>);
    /// `POST /music`: replaces the whole blob on the server.
    fn save(&self, data: &MusicData, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>);
    /// Uploads an attachment. The reply is the URL to store on the attachment.
    fn upload(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        on_done: Box<dyn FnOnce(Result<String, ApiError>) + Send>,
    );
    /// The absolute URL of a stored attachment, for opening it.
    fn resolve_url(&self, stored: &str) -> String;
}

impl MusicApi for ApiClient {
    fn resolve_url(&self, stored: &str) -> String {
        ApiClient::resolve_url(self, stored)
    }

    fn load(&self, on_done: Box<dyn FnOnce(Result<MusicData, ApiError>) + Send>) {
        ApiClient::music(self, on_done);
    }

    fn save(&self, data: &MusicData, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>) {
        ApiClient::save_music(self, data, on_done);
    }

    fn upload(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        on_done: Box<dyn FnOnce(Result<String, ApiError>) + Send>,
    ) {
        ApiClient::upload_music_file(self, name, mime, bytes, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn MusicApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The three studios. Each has its own routine and exercises; piano and guitar also have a repertoire, and the DJ studio
/// has mixes instead. The route picks the studio: `/music` and `/music/piano` are piano, `/music/guitar` guitar and
/// `/music/dj` DJ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Studio {
    #[default]
    Piano,
    Guitar,
    Dj,
}

impl Studio {
    /// The studio a route shows.
    pub fn from_route(route: &str) -> Self {
        match route.trim_end_matches('/') {
            "/music/guitar" => Self::Guitar,
            "/music/dj" => Self::Dj,
            _ => Self::Piano,
        }
    }

    /// Whether the studio has a repertoire tab (every studio but DJ).
    pub fn has_repertoire(self) -> bool {
        self != Self::Dj
    }
}

impl Tab {
    /// The index this tab has in the strip the studio shows, or `-1` when the studio does not show it.
    pub fn strip_index(self, studio: Studio) -> i32 {
        match (studio, self) {
            (Studio::Dj, Tab::Exercises) => 0,
            (Studio::Dj, Tab::Routine) => 1,
            (Studio::Dj, Tab::Metronome) => 2,
            (Studio::Dj, Tab::Repertoire) => -1,
            _ => self.index(),
        }
    }

    /// The tab a strip index names in the studio's strip.
    pub fn from_strip_index(index: i32, studio: Studio) -> Option<Tab> {
        match studio {
            Studio::Dj => match index {
                0 => Some(Tab::Exercises),
                1 => Some(Tab::Routine),
                2 => Some(Tab::Metronome),
                _ => None,
            },
            _ => Tab::from_index(index),
        }
    }
}

/// The four tabs of the studio, in the order the page shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Exercises,
    Repertoire,
    Routine,
    Metronome,
}

impl Tab {
    /// The tab at `index` in the tab strip, if any.
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Exercises),
            1 => Some(Self::Repertoire),
            2 => Some(Self::Routine),
            3 => Some(Self::Metronome),
            _ => None,
        }
    }

    /// The position of this tab in the tab strip.
    pub fn index(self) -> i32 {
        match self {
            Self::Exercises => 0,
            Self::Repertoire => 1,
            Self::Routine => 2,
            Self::Metronome => 3,
        }
    }
}

/// A metronome beat: accented, normal, or muted. Click cycles through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Accent,
    Normal,
    Muted,
}

impl Beat {
    fn next(self) -> Self {
        match self {
            Self::Accent => Self::Normal,
            Self::Normal => Self::Muted,
            Self::Muted => Self::Accent,
        }
    }

    /// The label under the beat button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Accent => "ACCENT",
            Self::Normal => "NORMAL",
            Self::Muted => "MUTED",
        }
    }

    /// The style the markup reads: 0 accent, 1 normal, 2 muted.
    pub fn kind(self) -> i32 {
        match self {
            Self::Accent => 0,
            Self::Normal => 1,
            Self::Muted => 2,
        }
    }
}

/// How a beat is split. The labels are in the markup, in the same order as [`Subdivision::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subdivision {
    Quarter,
    Eighth,
    Sixteenth,
    Triplet,
}

impl Subdivision {
    pub const ALL: [Self; 4] = [Self::Quarter, Self::Eighth, Self::Sixteenth, Self::Triplet];

    fn per_beat(self) -> u32 {
        match self {
            Self::Quarter => 1,
            Self::Eighth => 2,
            Self::Sixteenth => 4,
            Self::Triplet => 3,
        }
    }

    /// The subdivision at `index` in [`Subdivision::ALL`], if any.
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// The position of this subdivision in [`Subdivision::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// The metronome's local state. Solid's metronome unmounts with its tab, so leaving the tab resets it.
#[derive(Debug, Clone, PartialEq)]
pub struct Metronome {
    pub bpm: f64,
    pub beats: usize,
    pub subdivision: Subdivision,
    pub pattern: Vec<Beat>,
    started: Option<Instant>,
    taps: Vec<Instant>,
    /// The subdivision that last clicked, so each one clicks once.
    last_step: Option<usize>,
    /// The 5-minute countdown: when on, the click stops once [`COUNTDOWN`] has passed since it started.
    pub countdown: bool,
}

/// The countdown length, five minutes (the web metronome's `durationMinutes`).
pub const COUNTDOWN: Duration = Duration::from_secs(5 * 60);

impl Default for Metronome {
    fn default() -> Self {
        Self {
            bpm: METRONOME_DEFAULT_BPM,
            beats: 4,
            subdivision: Subdivision::Quarter,
            pattern: vec![Beat::Accent, Beat::Normal, Beat::Normal, Beat::Normal],
            started: None,
            taps: Vec::new(),
            last_step: None,
            countdown: false,
        }
    }
}

impl Metronome {
    /// Subdivisions in one bar.
    pub fn total(&self) -> usize {
        self.beats * self.subdivision.per_beat() as usize
    }

    /// The highlighted subdivision index while running. Visual only, as in the egui page: no audio is produced.
    pub fn current(&self, now: Instant) -> Option<usize> {
        let started = self.started?;
        let total = self.total();
        if total == 0 {
            return None;
        }
        let per_sub = 60.0 / self.bpm / f64::from(self.subdivision.per_beat());
        let elapsed = now.saturating_duration_since(started).as_secs_f64();
        Some(((elapsed / per_sub) as usize) % total)
    }

    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    /// Seconds left of the countdown while it runs (rounded up), `0` when the countdown is off, the metronome is
    /// stopped, or the time is up.
    pub fn remaining_secs(&self, now: Instant) -> u32 {
        match self.started {
            Some(started) if self.countdown => {
                let elapsed = now.saturating_duration_since(started);
                COUNTDOWN.saturating_sub(elapsed).as_secs_f64().ceil() as u32
            }
            _ => 0,
        }
    }

    fn set_beats(&mut self, beats: usize) {
        self.beats = beats.clamp(BEATS_MIN, BEATS_MAX);
        self.pattern = (0..self.beats)
            .map(|i| if i == 0 { Beat::Accent } else { Beat::Normal })
            .collect();
    }

    fn toggle(&mut self, now: Instant) {
        self.started = if self.started.is_some() {
            None
        } else {
            Some(now)
        };
        self.last_step = None;
    }

    /// The click due now, or `None` when this subdivision already clicked or its beat is muted. A beat clicks accented
    /// or normal by its pattern; the subdivisions between beats click quietly.
    fn due_click(&mut self, now: Instant) -> Option<Tone> {
        if self.countdown && self.remaining_secs(now) == 0 && self.started.is_some() {
            // The countdown is over: the metronome stops, and the glue sees it stopped on its next tick.
            self.started = None;
            self.last_step = None;
            return None;
        }
        let step = self.current(now)?;
        if self.last_step == Some(step) {
            return None;
        }
        self.last_step = Some(step);
        let per_beat = self.subdivision.per_beat() as usize;
        if step % per_beat != 0 {
            return Some(SUBDIVISION_CLICK);
        }
        match self.pattern.get(step / per_beat) {
            Some(Beat::Muted) => None,
            Some(Beat::Accent) => Some(ACCENT_CLICK),
            _ => Some(CLICK),
        }
    }

    fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm.clamp(BPM_MIN, BPM_MAX);
    }

    /// Averages the last four tap intervals. A pause longer than two seconds starts over.
    fn tap(&mut self, now: Instant) {
        if self
            .taps
            .last()
            .is_some_and(|last| now.saturating_duration_since(*last) > TAP_RESTART)
        {
            self.taps.clear();
        }
        self.taps.push(now);
        if self.taps.len() > 4 {
            self.taps.remove(0);
        }
        if self.taps.len() >= 2 {
            let intervals: Vec<f64> = self
                .taps
                .windows(2)
                .map(|w| w[1].saturating_duration_since(w[0]).as_secs_f64())
                .collect();
            let average = intervals.iter().sum::<f64>() / intervals.len() as f64;
            if average > 0.0 {
                self.bpm = (60.0 / average).round().clamp(TAP_MIN, TAP_MAX);
            }
        }
    }
}

/// Where the last save stands. `Saved` stays until the next save; the egui page cleared it after two seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveStatus {
    #[default]
    Idle,
    Saving,
    Saved,
    Error,
}

/// One exercise row, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExerciseRowView {
    pub id: String,
    pub name: String,
    /// The text of the current-BPM field: the draft the user typed, or the value, or the default 60.
    pub bpm_text: String,
    pub target_text: String,
    pub done: bool,
    pub can_up: bool,
    pub can_down: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentView {
    pub id: String,
    pub name: String,
    pub audio: bool,
}

/// One repertoire card, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceView {
    pub id: String,
    pub title: String,
    pub composer: String,
    pub status: String,
    pub difficulty: i64,
    pub expanded: bool,
    pub uploading: bool,
    pub attachments: Vec<AttachmentView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockItemView {
    pub id: String,
    pub name: String,
    pub done: bool,
}

/// One routine block, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockView {
    pub id: String,
    pub name: String,
    pub items: Vec<BlockItemView>,
}

/// One beat button of the metronome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatView {
    pub label: String,
    pub kind: i32,
}

/// The metronome, as the page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct MetronomeView {
    pub bpm: f64,
    pub beats: usize,
    /// The index into [`Subdivision::ALL`].
    pub subdivision: usize,
    pub running: bool,
    /// The lit beat while running, or `None`.
    pub lit_beat: Option<usize>,
    pub total: usize,
    pub cells: Vec<BeatView>,
    /// The 5-minute countdown is switched on. It stops the click when the time runs out.
    pub countdown: bool,
    /// Seconds left of the countdown while it runs, `0` when it is off or stopped.
    pub remaining_secs: u32,
    /// The time left as `mm:ss` while the countdown runs, empty otherwise.
    pub remaining_text: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicView {
    /// Changes when a list changes shape. The glue rebuilds its row models only when this changes.
    pub version: u64,
    pub tab: Tab,
    pub loaded: bool,
    pub notice: Option<String>,
    pub save: SaveStatus,
    pub exercises: Vec<ExerciseRowView>,
    /// The repertoire after the search and stage filter.
    pub pieces: Vec<PieceView>,
    /// The repertoire before any filter.
    pub piece_count: usize,
    /// The repertoire stages: the saved list, or the defaults when it is empty.
    pub stages: Vec<String>,
    pub search: String,
    /// [`ALL_STAGES`] or one of `stages`.
    pub status_filter: String,
    pub blocks: Vec<BlockView>,
    pub metronome: MetronomeView,
    /// The studio on screen: its tabs and its data follow it.
    pub studio: Studio,
    /// The active tab's index in the strip the studio shows (see [`Tab::strip_index`]).
    pub tab_strip: i32,
}

#[derive(Default)]
struct State {
    data: MusicData,
    /// True after the first load succeeded. Nothing is saved before it, so the seed never overwrites the stored blob.
    can_save: bool,
    load_started: bool,
    loaded: bool,
    route: String,
    notice: Option<String>,
    /// True after a text edit, until the next save. The glue saves on a debounce and [`MusicController::flush`] saves
    /// what is left on close and page change.
    dirty: bool,
    save: SaveStatus,
    tab: Option<Tab>,
    version: u64,
    search: String,
    status_filter: String,
    expanded: Option<String>,
    checked_exercises: HashSet<String>,
    checked_routine: HashSet<String>,
    uploading: HashSet<String>,
    /// Text the user typed into number fields, keyed `bpm:{id}` and `target:{id}`. Kept so a blank field stays blank.
    drafts: HashMap<String, String>,
    metronome: Metronome,
    /// The studio the route shows.
    studio: Studio,
    /// Stands in for a repertoire where the studio has none (DJ). Never shown, so repertoire edits there change nothing.
    dj_scratch: Vec<RepertoireItem>,
}

impl State {
    fn new() -> Self {
        Self {
            data: MusicData::seed(),
            tab: Some(Tab::Routine),
            status_filter: ALL_STAGES.to_owned(),
            metronome: Metronome::default(),
            dirty: false,
            ..Self::default()
        }
    }

    fn tab(&self) -> Tab {
        self.tab.unwrap_or(Tab::Routine)
    }

    /// The routine of the studio on screen.
    fn routine(&self) -> &Vec<RoutineCategory> {
        match self.studio {
            Studio::Piano => &self.data.piano.routine,
            Studio::Guitar => &self.data.guitar.routine,
            Studio::Dj => &self.data.dj.routine,
        }
    }

    fn routine_mut(&mut self) -> &mut Vec<RoutineCategory> {
        match self.studio {
            Studio::Piano => &mut self.data.piano.routine,
            Studio::Guitar => &mut self.data.guitar.routine,
            Studio::Dj => &mut self.data.dj.routine,
        }
    }

    /// The exercises of the studio on screen.
    fn exercises(&self) -> &Vec<Exercise> {
        match self.studio {
            Studio::Piano => &self.data.piano.exercises,
            Studio::Guitar => &self.data.guitar.exercises,
            Studio::Dj => &self.data.dj.exercises,
        }
    }

    fn exercises_mut(&mut self) -> &mut Vec<Exercise> {
        match self.studio {
            Studio::Piano => &mut self.data.piano.exercises,
            Studio::Guitar => &mut self.data.guitar.exercises,
            Studio::Dj => &mut self.data.dj.exercises,
        }
    }

    /// The repertoire of the studio on screen; empty for DJ.
    fn repertoire(&self) -> &Vec<RepertoireItem> {
        match self.studio {
            Studio::Piano => &self.data.piano.repertoire,
            Studio::Guitar => &self.data.guitar.repertoire,
            Studio::Dj => &self.dj_scratch,
        }
    }

    fn repertoire_mut(&mut self) -> &mut Vec<RepertoireItem> {
        match self.studio {
            Studio::Piano => &mut self.data.piano.repertoire,
            Studio::Guitar => &mut self.data.guitar.repertoire,
            Studio::Dj => {
                self.dj_scratch.clear();
                &mut self.dj_scratch
            }
        }
    }

    /// The repertoire stages of the studio on screen.
    fn repertoire_states(&self) -> &Vec<String> {
        match self.studio {
            Studio::Guitar => &self.data.guitar.repertoire_states,
            _ => &self.data.piano.repertoire_states,
        }
    }

    /// Solid's session reset on a route change: the tab, the metronome, the ticks, the search and the filter.
    fn reset_session(&mut self) {
        self.tab = Some(Tab::Routine);
        self.metronome = Metronome::default();
        self.checked_exercises.clear();
        self.checked_routine.clear();
        self.search.clear();
        self.status_filter = ALL_STAGES.to_owned();
        self.expanded = None;
        self.notice = None;
    }

    /// The stages the repertoire offers: the saved list, or the defaults when it is empty.
    fn stages(&self) -> Vec<String> {
        if self.repertoire_states().is_empty() {
            DEFAULT_REPERTOIRE_STATES
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
        } else {
            self.repertoire_states().clone()
        }
    }

    fn draft_or(&self, key: String, value: Option<f64>, fallback: Option<f64>) -> String {
        self.drafts
            .get(&key)
            .cloned()
            .unwrap_or_else(|| value.or(fallback).map(format_bpm).unwrap_or_default())
    }

    fn view(&self, now: Instant) -> MusicView {
        let query = self.search.trim().to_lowercase();
        let source_exercises = self.exercises();
        let count = source_exercises.len();
        let exercises = source_exercises
            .iter()
            .enumerate()
            .map(|(index, ex)| ExerciseRowView {
                id: ex.id.clone(),
                name: ex.name.clone(),
                bpm_text: self.draft_or(
                    format!("bpm:{}", ex.id),
                    ex.bpm,
                    Some(EXERCISE_DEFAULT_BPM),
                ),
                target_text: self.draft_or(format!("target:{}", ex.id), ex.target_bpm, None),
                done: self.checked_exercises.contains(&ex.id),
                can_up: index > 0,
                can_down: index + 1 < count,
            })
            .collect();
        let pieces = self
            .repertoire()
            .iter()
            .filter(|p| piece_matches(p, &self.status_filter, &query))
            .map(|p| PieceView {
                id: p.id.clone(),
                title: p.title.clone(),
                composer: p.composer.clone(),
                status: p.status.clone(),
                difficulty: p.difficulty,
                expanded: self.expanded.as_deref() == Some(p.id.as_str()),
                uploading: self.uploading.contains(&p.id),
                attachments: p
                    .attachments
                    .iter()
                    .map(|a| AttachmentView {
                        id: a.id.clone(),
                        name: a.name.clone(),
                        audio: a.mime.as_deref().is_some_and(|m| m.starts_with("audio/")),
                    })
                    .collect(),
            })
            .collect();
        let blocks = self
            .routine()
            .iter()
            .map(|block| BlockView {
                id: block.id.clone(),
                name: block.name.clone(),
                items: block
                    .items
                    .iter()
                    .map(|item| BlockItemView {
                        id: item.id.clone(),
                        name: item.name.clone(),
                        done: self.checked_routine.contains(&item.id),
                    })
                    .collect(),
            })
            .collect();
        let m = &self.metronome;
        let per_beat = m.subdivision.per_beat() as usize;
        let metronome = MetronomeView {
            bpm: m.bpm,
            beats: m.beats,
            subdivision: m.subdivision.index(),
            running: m.running(),
            lit_beat: m.current(now).map(|sub| sub / per_beat),
            total: m.total(),
            countdown: m.countdown,
            remaining_secs: m.remaining_secs(now),
            remaining_text: match m.remaining_secs(now) {
                0 => String::new(),
                secs => format!("{:02}:{:02}", secs / 60, secs % 60),
            },
            cells: m
                .pattern
                .iter()
                .map(|beat| BeatView {
                    label: beat.label().to_owned(),
                    kind: beat.kind(),
                })
                .collect(),
        };
        MusicView {
            version: self.version,
            tab: self.tab(),
            loaded: self.loaded,
            notice: self.notice.clone(),
            save: self.save,
            exercises,
            pieces,
            piece_count: self.repertoire().len(),
            studio: self.studio,
            tab_strip: self.tab().strip_index(self.studio),
            stages: self.stages(),
            search: self.search.clone(),
            status_filter: self.status_filter.clone(),
            blocks,
            metronome,
        }
    }
}

fn piece_matches(piece: &RepertoireItem, filter: &str, query: &str) -> bool {
    (filter == ALL_STAGES || piece.status == filter)
        && (query.is_empty()
            || piece.title.to_lowercase().contains(query)
            || piece.composer.to_lowercase().contains(query))
}

type Notify = Arc<dyn Fn(MusicView) + Send + Sync>;

struct Shared {
    api: Arc<dyn MusicApi>,
    state: Mutex<State>,
    notify: Notify,
}

/// The Music page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct MusicController {
    shared: Arc<Shared>,
}

impl MusicController {
    /// `notify` receives the new view after a state change, possibly on a worker thread.
    pub fn new(api: Arc<dyn MusicApi>, notify: impl Fn(MusicView) + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::new(State::new()),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> MusicView {
        self.lock().view(Instant::now())
    }

    /// The shell calls this when a music route is shown. A new route resets the session, as Solid remounts the studio.
    /// The first activation loads the blob.
    pub fn activate(&self, route: &str) {
        let (route_changed, first) = {
            let mut s = self.lock();
            let route_changed = s.route != route;
            if route_changed {
                s.reset_session();
                s.route = route.to_owned();
                s.studio = Studio::from_route(route);
            }
            let first = !s.load_started;
            s.load_started = true;
            (route_changed, first)
        };
        if first {
            self.load();
        } else if route_changed {
            self.notify_now();
        }
    }

    fn load(&self) {
        let me = self.clone();
        self.shared.api.load(Box::new(move |result| {
            me.change(true, |s| match result {
                Ok(data) => {
                    s.data = data;
                    s.can_save = true;
                    s.loaded = true;
                    s.drafts.clear();
                }
                Err(e) => {
                    s.notice = Some(format!("Could not load music: {e}"));
                    s.loaded = true;
                }
            });
        }));
    }

    /// Hides the notice banner.
    pub fn dismiss_notice(&self) {
        self.change(false, |s| s.notice = None);
    }

    /// Shows a tab. Leaving the metronome resets it, as Solid does.
    /// Shows the tab at `index` in the strip the studio shows (the strip is shorter for DJ, which has no repertoire).
    pub fn select_tab_strip(&self, index: i32) {
        let tab = {
            let s = self.lock();
            Tab::from_strip_index(index, s.studio)
        };
        if let Some(tab) = tab {
            self.select_tab(tab);
        }
    }

    pub fn select_tab(&self, tab: Tab) {
        self.change(true, |s| {
            // The DJ studio has no repertoire tab (the web hides it for DJ too).
            let allowed = tab != Tab::Repertoire || s.studio.has_repertoire();
            if s.tab() != tab && allowed {
                if s.tab() == Tab::Metronome {
                    s.metronome = Metronome::default();
                }
                s.tab = Some(tab);
            }
        });
    }

    // ----- Exercises -------------------------------------------------------------------------------------------------

    pub fn add_exercise(&self) {
        self.commit(|s| {
            s.exercises_mut().push(Exercise {
                id: new_id("ex"),
                ..Default::default()
            });
        });
    }

    /// Swaps the exercise `id` with its neighbour above (`up`) or below.
    pub fn move_exercise(&self, id: &str, up: bool) {
        self.commit(|s| {
            let exercises = &mut s.exercises_mut();
            if let Some(index) = exercises.iter().position(|e| e.id == id) {
                let target = if up {
                    index.checked_sub(1)
                } else {
                    Some(index + 1)
                };
                if let Some(target) = target.filter(|t| *t < exercises.len()) {
                    exercises.swap(index, target);
                }
            }
        });
    }

    pub fn remove_exercise(&self, id: &str) {
        self.commit(|s| {
            s.exercises_mut().retain(|e| e.id != id);
            s.checked_exercises.remove(id);
            s.drafts.remove(&format!("bpm:{id}"));
            s.drafts.remove(&format!("target:{id}"));
        });
    }

    /// The practice tick box. Ticks are not saved.
    pub fn set_exercise_checked(&self, id: &str, done: bool) {
        self.change(true, |s| set_member(&mut s.checked_exercises, id, done));
    }

    /// Typing in the exercise name. Not a rebuild: the field already shows the text. The glue schedules the save.
    pub fn edit_exercise_name(&self, id: &str, name: &str) {
        self.quiet(|s| {
            if let Some(ex) = s.exercises_mut().iter_mut().find(|e| e.id == id) {
                ex.name = name.to_owned();
            }
        });
    }

    /// Typing in the current-BPM field. A blank or zero entry means "not set".
    pub fn edit_exercise_bpm(&self, id: &str, text: &str) {
        self.quiet(|s| {
            s.drafts.insert(format!("bpm:{id}"), text.to_owned());
            if let Some(ex) = s.exercises_mut().iter_mut().find(|e| e.id == id) {
                ex.bpm = parse_bpm(text);
            }
        });
    }

    /// Typing in the target-BPM field.
    pub fn edit_exercise_target(&self, id: &str, text: &str) {
        self.quiet(|s| {
            s.drafts.insert(format!("target:{id}"), text.to_owned());
            if let Some(ex) = s.exercises_mut().iter_mut().find(|e| e.id == id) {
                ex.target_bpm = parse_bpm(text);
            }
        });
    }

    // ----- Repertoire ------------------------------------------------------------------------------------------------

    pub fn set_search(&self, text: &str) {
        self.change(true, |s| s.search = text.to_owned());
    }

    /// `ALL_STAGES` or one stage name.
    pub fn set_status_filter(&self, filter: &str) {
        self.change(true, |s| s.status_filter = filter.to_owned());
    }

    /// Adds a piece at the top of the list and opens it, as the egui page did.
    pub fn add_piece(&self) {
        let id = new_id("piece");
        let added = id.clone();
        self.commit(move |s| {
            s.repertoire_mut().insert(
                0,
                RepertoireItem {
                    id: added.clone(),
                    title: "New Piece".to_owned(),
                    composer: String::new(),
                    status: "Learning".to_owned(),
                    difficulty: DEFAULT_DIFFICULTY,
                    attachments: Vec::new(),
                },
            );
            s.expanded = Some(added);
        });
    }

    pub fn remove_piece(&self, id: &str) {
        self.commit(|s| {
            s.repertoire_mut().retain(|p| p.id != id);
            if s.expanded.as_deref() == Some(id) {
                s.expanded = None;
            }
            s.uploading.remove(id);
        });
    }

    /// Opens the piece `id`, or closes it when it is open.
    pub fn toggle_expanded(&self, id: &str) {
        self.change(true, |s| {
            s.expanded = if s.expanded.as_deref() == Some(id) {
                None
            } else {
                Some(id.to_owned())
            };
        });
    }

    pub fn edit_piece_title(&self, id: &str, title: &str) {
        self.quiet(|s| {
            if let Some(p) = s.repertoire_mut().iter_mut().find(|p| p.id == id) {
                p.title = title.to_owned();
            }
        });
    }

    pub fn edit_piece_composer(&self, id: &str, composer: &str) {
        self.quiet(|s| {
            if let Some(p) = s.repertoire_mut().iter_mut().find(|p| p.id == id) {
                p.composer = composer.to_owned();
            }
        });
    }

    pub fn set_piece_status(&self, id: &str, status: &str) {
        self.commit(|s| {
            if let Some(p) = s.repertoire_mut().iter_mut().find(|p| p.id == id) {
                p.status = status.to_owned();
            }
        });
    }

    /// Sets the difficulty, 1 to 5.
    pub fn set_difficulty(&self, id: &str, value: i64) {
        let value = value.clamp(1, DIFFICULTY_MAX);
        self.commit(|s| {
            if let Some(p) = s.repertoire_mut().iter_mut().find(|p| p.id == id) {
                p.difficulty = value;
            }
        });
    }

    pub fn remove_attachment(&self, piece: &str, attachment: &str) {
        self.commit(|s| {
            if let Some(p) = s.repertoire_mut().iter_mut().find(|p| p.id == piece) {
                p.attachments.retain(|a| a.id != attachment);
            }
        });
    }

    /// The piece whose attachments are open, if any. A dropped file attaches to this piece.
    pub fn expanded_piece(&self) -> Option<String> {
        self.lock().expanded.clone()
    }

    /// The absolute URL of the attachment `attachment` (of any piece), for the page to open. `None` when it is gone.
    pub fn attachment_url(&self, attachment: &str) -> Option<String> {
        let stored = {
            let s = self.lock();
            s.repertoire()
                .iter()
                .flat_map(|p| p.attachments.iter())
                .find(|a| a.id == attachment)
                .map(|a| a.url.clone())?
        };
        Some(self.shared.api.resolve_url(&stored))
    }

    /// Uploads `bytes` as an attachment of the piece `piece`. The attachment is stored when the upload answers.
    pub fn attach_file(&self, piece: &str, name: &str, bytes: &[u8]) {
        let mime = mime_for(name);
        let size = bytes.len() as u64;
        self.change(true, |s| {
            s.uploading.insert(piece.to_owned());
        });
        let me = self.clone();
        let piece = piece.to_owned();
        let name = name.to_owned();
        let file_name = name.clone();
        let mime_text = mime.to_owned();
        self.shared.api.upload(
            &file_name,
            mime,
            bytes,
            Box::new(move |result| match result {
                Ok(url) => {
                    let mut attached = false;
                    me.change(true, |s| {
                        s.uploading.remove(&piece);
                        if let Some(item) = s.repertoire_mut().iter_mut().find(|p| p.id == piece) {
                            item.attachments.push(Attachment {
                                id: new_id("att"),
                                name,
                                url,
                                size: Some(size),
                                mime: Some(mime_text),
                            });
                            attached = true;
                        }
                    });
                    if attached {
                        me.save_now();
                    }
                }
                Err(_) => me.change(true, |s| {
                    s.uploading.remove(&piece);
                    s.notice = Some("Failed to upload file attachment.".to_owned());
                }),
            }),
        );
    }

    // ----- Daily routine ---------------------------------------------------------------------------------------------

    pub fn add_block(&self) {
        self.commit(|s| {
            s.routine_mut().push(RoutineCategory {
                id: new_id("block"),
                name: "New Practice Block".to_owned(),
                items: vec![RoutineItem {
                    id: new_id("task"),
                    name: "First item".to_owned(),
                }],
            });
        });
    }

    pub fn remove_block(&self, id: &str) {
        self.commit(|s| s.routine_mut().retain(|b| b.id != id));
    }

    pub fn edit_block_name(&self, id: &str, name: &str) {
        self.quiet(|s| {
            if let Some(b) = s.routine_mut().iter_mut().find(|b| b.id == id) {
                b.name = name.to_owned();
            }
        });
    }

    pub fn add_block_item(&self, block: &str) {
        self.commit(|s| {
            if let Some(b) = s.routine_mut().iter_mut().find(|b| b.id == block) {
                b.items.push(RoutineItem {
                    id: new_id("task"),
                    name: String::new(),
                });
            }
        });
    }

    pub fn remove_block_item(&self, block: &str, item: &str) {
        self.commit(|s| {
            if let Some(b) = s.routine_mut().iter_mut().find(|b| b.id == block) {
                b.items.retain(|i| i.id != item);
            }
            s.checked_routine.remove(item);
        });
    }

    pub fn edit_block_item(&self, block: &str, item: &str, name: &str) {
        self.quiet(|s| {
            if let Some(i) = s
                .routine_mut()
                .iter_mut()
                .find(|b| b.id == block)
                .and_then(|b| b.items.iter_mut().find(|i| i.id == item))
            {
                i.name = name.to_owned();
            }
        });
    }

    /// The routine tick box. Ticks are not saved.
    pub fn set_block_item_checked(&self, item: &str, done: bool) {
        self.change(true, |s| set_member(&mut s.checked_routine, item, done));
    }

    // ----- Metronome -------------------------------------------------------------------------------------------------

    /// Starts or stops the metronome. The glue runs its tick timer while `view().metronome.running` is true.
    pub fn metronome_toggle(&self) {
        self.change(false, |s| s.metronome.toggle(Instant::now()));
    }

    /// Switches the 5-minute countdown on or off. It applies from the next start.
    pub fn metronome_set_countdown(&self, on: bool) {
        self.change(false, |s| s.metronome.countdown = on);
    }

    pub fn metronome_tap(&self) {
        self.change(false, |s| s.metronome.tap(Instant::now()));
    }

    /// The slider. Not a rebuild: the readout and the slider follow the value.
    pub fn metronome_set_bpm(&self, bpm: f64) {
        self.change(false, |s| s.metronome.set_bpm(bpm));
    }

    /// The minus and plus buttons: one BPM at a time.
    pub fn metronome_step_bpm(&self, delta: f64) {
        self.change(false, |s| {
            let bpm = s.metronome.bpm + delta;
            s.metronome.set_bpm(bpm);
        });
    }

    pub fn metronome_set_beats(&self, beats: usize) {
        self.change(true, |s| s.metronome.set_beats(beats));
    }

    pub fn metronome_set_subdivision(&self, index: usize) {
        self.change(false, |s| {
            if let Some(sub) = Subdivision::from_index(index) {
                s.metronome.subdivision = sub;
            }
        });
    }

    /// Cycles the beat at `index` through accent, normal and muted.
    pub fn metronome_cycle_beat(&self, index: usize) {
        self.change(true, |s| {
            if let Some(beat) = s.metronome.pattern.get_mut(index) {
                *beat = beat.next();
            }
        });
    }

    /// Re-reads the metronome's position and reports it. The glue calls this from its tick timer while the metronome
    /// runs, and stops the timer when the result is `false`.
    pub fn metronome_tick(&self) -> bool {
        let (view, click) = {
            let mut state = self.lock();
            let now = Instant::now();
            let click = state.metronome.due_click(now);
            (state.view(now), click)
        };
        if let Some(tone) = click {
            rimelight_sound::play(tone);
        }
        let running = view.metronome.running;
        (self.shared.notify)(view);
        running
    }

    /// Opens the system file dialog and attaches the files picked to `piece`, as a drop does. A file that cannot be read
    /// is reported in the notice and the rest still go up. Returns how many files the person picked.
    pub fn pick_attachments(&self, piece: &str) -> usize {
        let paths: Vec<PathBuf> = rimelight_platform::pick_files(ATTACHMENT_FILES, true);
        for path in &paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            match std::fs::read(path) {
                Ok(bytes) => self.attach_file(piece, &name, &bytes),
                Err(error) => self.report(&format!("Could not read {name}: {error}")),
            }
        }
        paths.len()
    }

    /// Saves the text edits that are not saved yet. The glue calls it when the page closes or the route changes, so the
    /// debounced save never drops the last edit.
    pub fn flush(&self) {
        let dirty = self.lock().dirty;
        if dirty {
            self.save_now();
        }
    }

    /// Shows `text` in the notice banner. The glue uses it for failures it finds before any API call (an unreadable file).
    pub fn report(&self, text: &str) {
        self.change(false, |s| s.notice = Some(text.to_owned()));
    }

    // ----- Internals -------------------------------------------------------------------------------------------------

    /// Saves the whole blob. Refused until a load has succeeded, so the seed never overwrites the stored blob.
    pub fn save_now(&self) {
        let data = {
            let mut s = self.lock();
            if !s.can_save {
                return;
            }
            s.save = SaveStatus::Saving;
            s.dirty = false;
            s.data.clone()
        };
        self.notify_now();
        let me = self.clone();
        self.shared.api.save(
            &data,
            Box::new(move |result| {
                me.change(false, |s| {
                    s.save = match result {
                        Ok(()) => SaveStatus::Saved,
                        Err(_) => SaveStatus::Error,
                    };
                });
            }),
        );
    }

    /// A structural edit: applied, shown with a rebuild, then saved at once.
    fn commit(&self, update: impl FnOnce(&mut State)) {
        self.change(true, update);
        self.save_now();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in one callback must not make every later call panic too.
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, rebuild: bool, update: impl FnOnce(&mut State)) {
        let view = {
            let mut state = self.lock();
            update(&mut state);
            if rebuild {
                state.version += 1;
            }
            state.view(Instant::now())
        };
        (self.shared.notify)(view);
    }

    /// Applies `update` under the lock without notifying.
    fn quiet(&self, update: impl FnOnce(&mut State)) {
        let mut state = self.lock();
        update(&mut state);
        state.dirty = true;
    }

    fn notify_now(&self) {
        let view = self.lock().view(Instant::now());
        (self.shared.notify)(view);
    }
}

fn set_member(set: &mut HashSet<String>, id: &str, present: bool) {
    if present {
        set.insert(id.to_owned());
    } else {
        set.remove(id);
    }
}

/// A fresh id for a new item. The time stamp keeps it apart from ids saved in earlier sessions.
fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{prefix}-{stamp}-{n}")
}

/// The content type for an attachment, from its file extension.
pub fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn format_bpm(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

/// Blank, zero and non-numbers all mean "not set".
fn parse_bpm(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v != 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each call and answers at once from scripted replies.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        load_reply: Mutex<Option<Result<MusicData, ApiError>>>,
        saves: Mutex<Vec<MusicData>>,
        save_reply: Mutex<Option<Result<(), ApiError>>>,
        upload_reply: Mutex<Option<Result<String, ApiError>>>,
        uploads: Mutex<Vec<(String, String, usize)>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_owned());
        }
    }

    impl MusicApi for Arc<Fake> {
        fn resolve_url(&self, stored: &str) -> String {
            format!("https://example.test{stored}")
        }

        fn load(&self, on_done: Box<dyn FnOnce(Result<MusicData, ApiError>) + Send>) {
            self.record("load");
            let reply = self
                .load_reply
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Ok(MusicData::seed()));
            on_done(reply);
        }

        fn save(&self, data: &MusicData, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>) {
            self.record("save");
            self.saves.lock().unwrap().push(data.clone());
            on_done(self.save_reply.lock().unwrap().take().unwrap_or(Ok(())));
        }

        fn upload(
            &self,
            name: &str,
            mime: &str,
            bytes: &[u8],
            on_done: Box<dyn FnOnce(Result<String, ApiError>) + Send>,
        ) {
            self.record("upload");
            self.uploads
                .lock()
                .unwrap()
                .push((name.to_owned(), mime.to_owned(), bytes.len()));
            let reply = self
                .upload_reply
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Ok(format!("/api/storage/files?key={name}")));
            on_done(reply);
        }
    }

    /// A controller over `fake` that counts the notifications it sends.
    fn controller(fake: &Arc<Fake>) -> (MusicController, Arc<Mutex<usize>>) {
        let count: Arc<Mutex<usize>> = Arc::default();
        let sink = count.clone();
        let api: Arc<dyn MusicApi> = Arc::new(fake.clone());
        let c = MusicController::new(api, move |_| *sink.lock().unwrap() += 1);
        (c, count)
    }

    /// A controller whose blob is loaded, with the notifications so far cleared.
    fn loaded(fake: &Arc<Fake>) -> (MusicController, Arc<Mutex<usize>>) {
        let (c, count) = controller(fake);
        c.activate("/music");
        *count.lock().unwrap() = 0;
        (c, count)
    }

    #[test]
    fn first_activation_loads_the_seeded_studio() {
        let fake = Arc::default();
        let (c, _) = controller(&fake);
        c.activate("/music");
        let view = c.view();
        assert!(view.loaded);
        assert_eq!(view.exercises.len(), 2);
        assert_eq!(view.tab, Tab::Routine);
        assert_eq!(fake.calls(), ["load"]);
        // Later activations of the same route do not fetch again.
        c.activate("/music");
        assert_eq!(fake.calls(), ["load"]);
    }

    #[test]
    fn a_failed_load_shows_the_notice_and_never_saves() {
        let fake: Arc<Fake> = Arc::default();
        *fake.load_reply.lock().unwrap() = Some(Err(ApiError::Transport("down".to_owned())));
        let (c, _) = controller(&fake);
        c.activate("/music");
        assert_eq!(
            c.view().notice.as_deref(),
            Some("Could not load music: network error: down")
        );
        c.add_exercise();
        assert!(
            !fake.calls().contains(&"save".to_owned()),
            "{:?}",
            fake.calls()
        );
    }

    #[test]
    fn a_structural_edit_saves_the_whole_blob() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.add_exercise();
        let saves = fake.saves.lock().unwrap().clone();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].piano.exercises.len(), 3);
        // The guitar and DJ sections the page never edits are sent back unchanged.
        assert_eq!(saves[0].guitar, MusicData::seed().guitar);
        assert_eq!(saves[0].dj, MusicData::seed().dj);
    }

    #[test]
    fn typing_is_silent_and_saved_only_when_flushed() {
        let fake = Arc::default();
        let (c, count) = loaded(&fake);
        let id = c.view().exercises[0].id.clone();
        c.edit_exercise_name(&id, "Scale draft");
        assert_eq!(
            *count.lock().unwrap(),
            0,
            "typing does not rebuild the view"
        );
        assert!(fake.saves.lock().unwrap().is_empty());
        c.save_now();
        let saves = fake.saves.lock().unwrap().clone();
        assert_eq!(saves[0].piano.exercises[0].name, "Scale draft");
    }

    #[test]
    fn a_blank_bpm_stays_blank_across_rebuilds() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        let id = c.view().exercises[0].id.clone();
        c.edit_exercise_bpm(&id, "");
        assert_eq!(c.view().exercises[0].bpm_text, "");
        // An unrelated rebuild keeps the draft: the field does not jump back to the value.
        c.set_exercise_checked(&id, true);
        let row = &c.view().exercises[0];
        assert_eq!(row.bpm_text, "");
        assert!(row.done);
    }

    #[test]
    fn an_unset_bpm_shows_the_default_and_a_value_is_saved() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.add_exercise();
        let new = c.view().exercises.last().unwrap().clone();
        assert_eq!(
            new.bpm_text, "60",
            "the egui default for an unset current BPM"
        );
        assert_eq!(new.target_text, "");
        c.edit_exercise_target(&new.id, "140");
        c.save_now();
        let saved = fake
            .saves
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .piano
            .exercises
            .last()
            .unwrap()
            .clone();
        assert_eq!(saved.target_bpm, Some(140.0));
        assert_eq!(saved.bpm, None);
    }

    #[test]
    fn moving_an_exercise_swaps_with_its_neighbour_and_stops_at_the_ends() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        let ids: Vec<String> = c.view().exercises.iter().map(|e| e.id.clone()).collect();
        c.move_exercise(&ids[0], false);
        let after: Vec<String> = c.view().exercises.iter().map(|e| e.id.clone()).collect();
        assert_eq!(after, [ids[1].clone(), ids[0].clone()]);
        c.move_exercise(&ids[0], false);
        c.move_exercise(&ids[0], false);
        let last: Vec<String> = c.view().exercises.iter().map(|e| e.id.clone()).collect();
        assert_eq!(last, after, "moving the last row down does nothing");
    }

    #[test]
    fn repertoire_search_and_stage_filter_combine() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        assert_eq!(c.view().pieces.len(), 2);
        c.set_search("satie");
        assert_eq!(
            c.view()
                .pieces
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>(),
            ["Gymnopédie No. 1"]
        );
        c.set_search("");
        c.set_status_filter("Polishing");
        assert_eq!(c.view().pieces.len(), 1);
        assert_eq!(c.view().piece_count, 2);
        c.set_status_filter("Learning");
        assert!(c.view().pieces.is_empty());
    }

    #[test]
    fn adding_a_piece_puts_it_first_and_opens_it() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.add_piece();
        let view = c.view();
        assert_eq!(view.pieces[0].title, "New Piece");
        assert!(view.pieces[0].expanded);
        assert_eq!(view.pieces[0].difficulty, DEFAULT_DIFFICULTY);
    }

    #[test]
    fn attaching_a_file_uploads_it_then_saves_the_attachment() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.attach_file("pr1", "take one.mp3", &[0u8; 10]);
        assert_eq!(
            fake.uploads.lock().unwrap().as_slice(),
            [("take one.mp3".to_owned(), "audio/mpeg".to_owned(), 10)]
        );
        let saves = fake.saves.lock().unwrap().clone();
        let piece = saves
            .last()
            .unwrap()
            .piano
            .repertoire
            .iter()
            .find(|p| p.id == "pr1")
            .unwrap();
        assert_eq!(piece.attachments.len(), 1);
        assert_eq!(piece.attachments[0].mime.as_deref(), Some("audio/mpeg"));
        assert_eq!(piece.attachments[0].size, Some(10));
        let view = c.view();
        assert!(
            !view
                .pieces
                .iter()
                .find(|p| p.id == "pr1")
                .unwrap()
                .uploading
        );
        assert!(
            view.pieces
                .iter()
                .find(|p| p.id == "pr1")
                .unwrap()
                .attachments[0]
                .audio
        );
    }

    #[test]
    fn a_failed_upload_reports_and_attaches_nothing() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _) = loaded(&fake);
        *fake.upload_reply.lock().unwrap() = Some(Err(ApiError::Transport("down".to_owned())));
        c.attach_file("pr1", "score.pdf", &[1, 2]);
        let view = c.view();
        assert_eq!(
            view.notice.as_deref(),
            Some("Failed to upload file attachment.")
        );
        assert!(
            view.pieces
                .iter()
                .find(|p| p.id == "pr1")
                .unwrap()
                .attachments
                .is_empty()
        );
    }

    #[test]
    fn leaving_the_route_resets_the_session() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.select_tab(Tab::Metronome);
        c.set_search("satie");
        c.activate("/music/guitar");
        let view = c.view();
        assert_eq!(view.tab, Tab::Routine);
        assert_eq!(view.search, "");
        assert_eq!(view.status_filter, ALL_STAGES);
    }

    #[test]
    fn tap_tempo_averages_the_intervals() {
        let base = Instant::now();
        let mut m = Metronome::default();
        m.tap(base);
        m.tap(base + Duration::from_millis(500));
        m.tap(base + Duration::from_millis(1000));
        assert_eq!(m.bpm, 120.0);
        // A pause longer than two seconds starts the taps over.
        m.tap(base + Duration::from_secs(10));
        assert_eq!(m.taps.len(), 1);
    }

    #[test]
    fn the_metronome_lights_the_beat_by_elapsed_time() {
        let base = Instant::now();
        let mut m = Metronome::default();
        m.toggle(base);
        // 120 bpm, quarter notes, four beats: a beat every half second.
        assert_eq!(m.current(base + Duration::from_millis(500)), Some(1));
        assert_eq!(
            m.current(base + Duration::from_millis(2000)),
            Some(0),
            "the bar wraps"
        );
        m.toggle(base);
        assert_eq!(m.current(base), None);
    }

    #[test]
    fn beat_cycle_and_bpm_steps_stay_in_range() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        // Beat 2 starts normal: normal, then muted, then accent.
        c.metronome_cycle_beat(1);
        assert_eq!(c.view().metronome.cells[1].label, "MUTED");
        assert_eq!(c.view().metronome.cells[1].kind, 2);
        c.metronome_cycle_beat(1);
        assert_eq!(c.view().metronome.cells[1].kind, 0);
        c.metronome_set_bpm(1000.0);
        c.metronome_step_bpm(-1.0);
        assert_eq!(c.view().metronome.bpm, BPM_MAX - 1.0);
        c.metronome_set_beats(12);
        assert_eq!(c.view().metronome.beats, BEATS_MAX);
    }

    #[test]
    fn leaving_the_metronome_tab_resets_it() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        c.select_tab(Tab::Metronome);
        c.metronome_step_bpm(10.0);
        c.select_tab(Tab::Exercises);
        c.select_tab(Tab::Metronome);
        assert_eq!(c.view().metronome.bpm, METRONOME_DEFAULT_BPM);
    }

    #[test]
    fn the_stage_list_falls_back_to_the_defaults_when_empty() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        assert_eq!(c.view().stages.len(), 5);
        assert_eq!(c.view().stages[0], "Want to Learn");
    }

    #[test]
    fn removing_a_block_item_saves_and_drops_its_tick() {
        let fake = Arc::default();
        let (c, _) = loaded(&fake);
        let block = c.view().blocks[0].id.clone();
        let item = c.view().blocks[0].items[0].id.clone();
        c.set_block_item_checked(&item, true);
        assert!(c.view().blocks[0].items[0].done);
        c.remove_block_item(&block, &item);
        assert!(c.view().blocks[0].items.iter().all(|i| i.id != item));
        assert!(fake.calls().last().is_some_and(|call| call == "save"));
    }

    #[test]
    fn each_step_clicks_once_and_a_muted_beat_is_silent() {
        let mut metronome = Metronome::default();
        let start = Instant::now();
        metronome.toggle(start);
        assert_eq!(
            metronome.due_click(start),
            Some(ACCENT_CLICK),
            "beat 0 is accented"
        );
        assert_eq!(
            metronome.due_click(start),
            None,
            "the same step does not click twice"
        );
        // 120 bpm, quarter notes: beat 1 starts at half a second.
        let half = start + Duration::from_millis(500);
        assert_eq!(metronome.due_click(half), Some(CLICK));
        metronome.pattern[2] = Beat::Muted;
        let one = start + Duration::from_secs(1);
        assert_eq!(metronome.due_click(one), None, "beat 2 is muted");
    }

    #[test]
    fn eighths_click_quietly_between_the_beats() {
        let mut metronome = Metronome {
            subdivision: Subdivision::Eighth,
            ..Metronome::default()
        };
        let start = Instant::now();
        metronome.toggle(start);
        assert_eq!(metronome.due_click(start), Some(ACCENT_CLICK));
        // Half a beat in: the first eighth between beats.
        let eighth = start + Duration::from_millis(250);
        assert_eq!(metronome.due_click(eighth), Some(SUBDIVISION_CLICK));
    }

    #[test]
    fn countdown_stops_the_click_after_five_minutes() {
        let mut metronome = Metronome {
            countdown: true,
            ..Metronome::default()
        };
        let start = Instant::now();
        metronome.toggle(start);
        assert_eq!(metronome.remaining_secs(start), 300);
        assert_eq!(metronome.due_click(start + COUNTDOWN), None);
        assert!(
            !metronome.running(),
            "the time is up, so the metronome stops"
        );
    }

    #[test]
    fn route_picks_the_studio() {
        assert_eq!(Studio::from_route("/music"), Studio::Piano);
        assert_eq!(Studio::from_route("/music/piano"), Studio::Piano);
        assert_eq!(Studio::from_route("/music/guitar"), Studio::Guitar);
        assert_eq!(Studio::from_route("/music/dj"), Studio::Dj);
        assert!(!Studio::Dj.has_repertoire());
        assert!(Studio::Guitar.has_repertoire());
    }

    #[test]
    fn flush_saves_only_when_a_text_edit_is_pending() {
        let fake = Arc::default();
        let (c, _seen) = loaded(&fake);
        let calls_before = fake.calls().len();
        c.flush();
        assert_eq!(
            fake.calls().len(),
            calls_before,
            "nothing pending, nothing saved"
        );
        let id = c.view().exercises.first().map(|row| row.id.clone());
        if let Some(id) = id {
            c.edit_exercise_name(&id, "Scales");
            c.flush();
            assert!(
                fake.calls().last().is_some_and(|call| call == "save"),
                "a pending edit is saved"
            );
        }
    }
}
