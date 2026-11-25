//! The Health & Fitness page controller (ported from `crates/app/src/health.rs` and the web components in
//! `src/components/health/`).
//!
//! Plain Rust with no Slint. The controller holds the page state (fasts, workouts, measurements, the stretch
//! routine player) and calls the `/api/health/*` endpoints through [`HealthApi`]. Each state change is reported as
//! a [`HealthView`]. The shell's glue turns views into Slint models.
//!
//! Threading: API calls return at once and answer later on a worker thread. The lock is never held across a call,
//! and `notify` runs after the lock is released.

mod local_time;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use idantity_core::health::{
    ExerciseSet, FastEntry, Goals, Measurement, MeasurementsData, MonthValue, Reading,
    StretchCategory, StretchItem, WorkoutData, WorkoutExercise, WorkoutExerciseCategory,
};
use idantity_core::{ApiClient, ApiError};
use rimelight_sound::Tone;
use web_time::{SystemTime, UNIX_EPOCH};

/// Side effects the page asks the platform for: a tone and the keep-awake lock. [`SystemFeedback`] uses the shared
/// crates; tests record the calls.
pub trait Feedback: Send + Sync {
    fn tone(&self, tone: Tone);
    fn keep_awake(&self, on: bool);
}

/// The real feedback: `rimelight_sound` for tones and `rimelight_platform` for keep-awake.
pub struct SystemFeedback;

impl Feedback for SystemFeedback {
    fn tone(&self, tone: Tone) {
        rimelight_sound::play(tone);
    }

    fn keep_awake(&self, on: bool) {
        rimelight_platform::keep_awake(on);
    }
}

/// Tone for a fasting stage change (the web plays a cue per stage).
pub const STAGE_CUE: Tone = Tone {
    hz: 660.0,
    ms: 250,
    volume: 0.4,
};
/// Tone for each stretch step and for the end of a routine (the web plays one per step).
pub const STEP_CUE: Tone = Tone {
    hz: 880.0,
    ms: 180,
    volume: 0.4,
};

/// The page's Slint markup, for the shell's build script.
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// A boxed completion callback, run on the API's worker thread.
pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait HealthApi: Send + Sync {
    fn fasting_list(&self, on_done: Done<Vec<FastEntry>>);
    fn fasting_start(&self, start_time: &str, duration_hours: u32, on_done: Done<FastEntry>);
    fn fasting_end(&self, id: &str, on_done: Done<()>);
    fn measurements_get(&self, on_done: Done<MeasurementsData>);
    fn measurements_save(&self, data: &MeasurementsData, on_done: Done<()>);
    fn workouts_get(&self, on_done: Done<Option<WorkoutData>>);
    fn workouts_save(&self, data: &WorkoutData, on_done: Done<()>);
}

impl HealthApi for ApiClient {
    fn fasting_list(&self, on_done: Done<Vec<FastEntry>>) {
        ApiClient::fasting_list(self, on_done);
    }

    fn fasting_start(&self, start_time: &str, duration_hours: u32, on_done: Done<FastEntry>) {
        ApiClient::fasting_start(self, start_time, duration_hours, on_done);
    }

    fn fasting_end(&self, id: &str, on_done: Done<()>) {
        ApiClient::fasting_end(self, id, on_done);
    }

    fn measurements_get(&self, on_done: Done<MeasurementsData>) {
        ApiClient::measurements_get(self, on_done);
    }

    fn measurements_save(&self, data: &MeasurementsData, on_done: Done<()>) {
        ApiClient::measurements_save(self, data, on_done);
    }

    fn workouts_get(&self, on_done: Done<Option<WorkoutData>>) {
        ApiClient::workouts_get(self, on_done);
    }

    fn workouts_save(&self, data: &WorkoutData, on_done: Done<()>) {
        ApiClient::workouts_save(self, data, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn HealthApi> {
    Arc::new(ApiClient::new(api_base))
}

// ---------------------------------------------------------------------------
// Constants (values copied from the web components and the egui port)
// ---------------------------------------------------------------------------

/// Protocol buttons in display order. "Custom" follows them and reads the hours field.
pub const PROTOCOLS: [(&str, u32); 5] = [
    ("16:8 (Lean)", 16),
    ("18:6 (Moderate)", 18),
    ("20:4 (Warrior)", 20),
    ("24h (OMAD)", 24),
    ("36h (Monk)", 36),
];
/// Index of the Custom protocol button.
pub const CUSTOM_PROTOCOL: usize = 5;
/// Units offered for a measurement track.
pub const UNITS: [&str; 6] = ["cm", "in", "kg", "lb", "%", "mm"];
/// Number of history bars shown.
const HISTORY_LIMIT: usize = 12;

/// Metabolic stage: the hours at which it starts, its name and its one-line description.
pub struct Stage {
    pub start: f64,
    pub end: f64,
    pub name: &'static str,
    pub description: &'static str,
}

pub const STAGES: [Stage; 5] = [
    Stage {
        start: 0.0,
        end: 4.0,
        name: "Blood Sugar Drop",
        description: "Insulin levels drop, body finishes digesting food",
    },
    Stage {
        start: 4.0,
        end: 12.0,
        name: "Glycogen Depletion",
        description: "Liver glycogen is depleted, fat burning begins to activate",
    },
    Stage {
        start: 12.0,
        end: 18.0,
        name: "Ketosis & Fat Burning",
        description: "Ketone production increases, body uses stored fat for energy",
    },
    Stage {
        start: 18.0,
        end: 24.0,
        name: "Autophagy & Cellular Renewal",
        description: "Cells recycle damaged components and misfolded proteins",
    },
    Stage {
        start: 24.0,
        end: 72.0,
        name: "Deep Autophagy & Growth Hormone",
        description: "Human Growth Hormone (HGH) surges and deep rejuvenation occurs",
    },
];

/// Index of the stage that `hours` into a fast falls in. The last stage holds from 72 hours on.
pub fn stage_index(hours: f64) -> usize {
    STAGES
        .iter()
        .position(|s| hours < s.end)
        .unwrap_or(STAGES.len() - 1)
}

/// Bar tone from the web thresholds: 3 purple (24h and more), 2 emerald (18h and more), 1 primary (16h and more),
/// 0 amber (below).
pub fn tone_for_hours(hours: f64) -> u8 {
    if hours >= 24.0 {
        3
    } else if hours >= 18.0 {
        2
    } else if hours >= 16.0 {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Time helpers (ISO 8601, no time zone database)
// ---------------------------------------------------------------------------

/// Seconds since the Unix epoch, now.
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Splits a time of day into its clock part and its offset from UTC in seconds.
fn split_offset(time: &str) -> (&str, f64) {
    if let Some(clock) = time.strip_suffix('Z') {
        return (clock, 0.0);
    }
    if let Some(pos) = time.rfind(['+', '-']).filter(|&p| p > 0) {
        let (clock, offset) = time.split_at(pos);
        let sign = if offset.starts_with('-') { -1.0 } else { 1.0 };
        let mut parts = offset[1..].split(':');
        let hours: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let minutes: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        return (clock, sign * (hours * 3600.0 + minutes * 60.0));
    }
    (time, 0.0)
}

/// Parses `YYYY-MM-DDTHH:MM[:SS[.sss]][Z|+HH:MM|-HH:MM]` into seconds since the epoch.
pub fn parse_iso_secs(value: &str) -> Option<f64> {
    let (date, time) = value.trim().split_once(['T', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = split_offset(time);
    let mut t = clock.split(':');
    let hours: f64 = t.next()?.parse().ok()?;
    let minutes: f64 = t.next()?.parse().ok()?;
    let seconds: f64 = match t.next() {
        Some(s) => s.parse().ok()?,
        None => 0.0,
    };
    let days = days_from_civil(year, month, day) as f64;
    Some(days * 86_400.0 + hours * 3600.0 + minutes * 60.0 + seconds - offset)
}

/// `YYYY-MM-DDTHH:MM:SS.000Z` for `secs` since the epoch.
pub fn iso_from_secs(secs: f64) -> String {
    let total = secs.floor() as i64;
    let (y, m, d) = civil_from_days(total.div_euclid(86_400));
    let rem = total.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// `HH:MM:SS` for a duration in seconds. Hours are not wrapped.
pub fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// Whole numbers print without a decimal, others to one place.
pub fn trim_num(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// The "to goal" distance the web shows: the absolute difference to one decimal. Empty when either value is zero,
/// because the web shows the tile only when both goals are set.
pub fn gap_text(current: f64, target: f64) -> String {
    if current == 0.0 || target == 0.0 {
        String::new()
    } else {
        format!("{:.1}", (current - target).abs())
    }
}

/// Length in hours of a fast. A completed fast is end minus start. An active one uses its planned end
/// (`originalEndTime`, else `endTime`), at least one hour, as the web does.
pub fn fast_hours(fast: &FastEntry) -> f64 {
    let start = parse_iso_secs(&fast.start_time).unwrap_or(0.0);
    let end_time = fast.end_time.as_deref().and_then(parse_iso_secs);
    if !fast.is_active {
        return end_time
            .map(|end| ((end - start) / 3600.0).max(0.0))
            .unwrap_or(0.0);
    }
    let end = fast
        .original_end_time
        .as_deref()
        .and_then(parse_iso_secs)
        .or(end_time)
        .unwrap_or(start);
    ((end - start) / 3600.0).max(1.0)
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A new id for a local record. Unique within the process and across restarts (time-based).
pub fn new_id() -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", (now_secs() * 1000.0) as u64)
}

// ---------------------------------------------------------------------------
// Default workouts (the web seeds these when the server has no arrays yet)
// ---------------------------------------------------------------------------

/// The seed data the web writes when the server has no workout arrays.
pub fn default_workouts() -> WorkoutData {
    let set = |kg: &str, reps: &str, rest: u32| ExerciseSet {
        id: new_id(),
        kg: kg.to_owned(),
        reps: reps.to_owned(),
        rest_timer: rest,
    };
    let stretch = |name: &str, duration: u32| StretchItem {
        id: new_id(),
        name: name.to_owned(),
        duration,
    };
    WorkoutData {
        stretches: vec![
            StretchCategory {
                id: new_id(),
                name: "Daily Spine & Posture".to_owned(),
                items: vec![
                    stretch("Neck Roll & Lateral Flexion", 30),
                    stretch("Thoracic Cat-Cow", 45),
                    stretch("Seated Spinal Twist", 40),
                ],
            },
            StretchCategory {
                id: new_id(),
                name: "Lower Body & Hips".to_owned(),
                items: vec![
                    stretch("Standing Hamstring Stretch", 45),
                    stretch("Quad & Hip Flexor Stretch", 45),
                ],
            },
        ],
        exercises: vec![WorkoutExerciseCategory {
            id: new_id(),
            name: "Push Day".to_owned(),
            items: vec![
                WorkoutExercise {
                    id: new_id(),
                    name: "Barbell Bench Press".to_owned(),
                    notes: "Warmup: 20kg bar, focus on bar path".to_owned(),
                    sets: vec![set("60", "10", 90), set("70", "8", 90), set("75", "6", 120)],
                },
                WorkoutExercise {
                    id: new_id(),
                    name: "Incline Dumbbell Press".to_owned(),
                    notes: "30 degree angle".to_owned(),
                    sets: vec![set("24", "10", 60), set("26", "8", 60)],
                },
            ],
        }],
    }
}

// ---------------------------------------------------------------------------
// Stretch routine player (pure: the caller passes the clock)
// ---------------------------------------------------------------------------

/// Plays a routine: each stretch runs for its duration. Pure state; `now` is seconds since the epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Runner {
    pub routine: String,
    /// `(name, seconds)` per step.
    pub items: Vec<(String, u32)>,
    pub index: usize,
    pub remaining: f64,
    pub paused: bool,
    pub finished: bool,
    last: f64,
}

impl Runner {
    /// `None` when the routine has no stretches.
    pub fn start(routine: &str, items: Vec<(String, u32)>, now: f64) -> Option<Self> {
        let first = items.first()?.1 as f64;
        Some(Self {
            routine: routine.to_owned(),
            items,
            index: 0,
            remaining: first,
            paused: false,
            finished: false,
            last: now,
        })
    }

    /// Moves the clock forward to `now`, stepping through the stretches it passes.
    pub fn advance(&mut self, now: f64) {
        let mut dt = (now - self.last).max(0.0);
        self.last = now;
        if self.paused || self.finished {
            return;
        }
        while dt > 0.0 && !self.finished {
            if dt < self.remaining {
                self.remaining -= dt;
                dt = 0.0;
            } else {
                dt -= self.remaining;
                self.step_to_next();
            }
        }
    }

    fn step_to_next(&mut self) {
        self.index += 1;
        match self.items.get(self.index) {
            Some((_, secs)) => self.remaining = f64::from(*secs),
            None => {
                self.finished = true;
                self.remaining = 0.0;
            }
        }
    }

    pub fn toggle_pause(&mut self, now: f64) {
        self.advance(now);
        self.paused = !self.paused;
        self.last = now;
    }

    /// Skip Next: moves to the following stretch, or finishes.
    pub fn next(&mut self, now: f64) {
        self.advance(now);
        if !self.finished {
            self.step_to_next();
        }
    }

    /// Previous: the stretch before the current one, or the first one again.
    pub fn previous(&mut self, now: f64) {
        self.advance(now);
        self.index = self.index.saturating_sub(1);
        self.finished = false;
        self.remaining = f64::from(self.items[self.index].1);
    }

    /// Whole seconds left in the current stretch.
    pub fn seconds_left(&self) -> u32 {
        self.remaining.ceil().max(0.0) as u32
    }

    /// Share of the routine done, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        let total = self.items.len().max(1) as f32;
        let done = if self.finished {
            total
        } else {
            self.index as f32
        };
        (done / total).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// The view (plain data, no Slint types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveFast {
    pub id: String,
    /// `22 Nov 2026, 10:00 UTC`.
    pub started: String,
    /// Elapsed as `HH:MM:SS`.
    pub elapsed: String,
    pub elapsed_hours: f64,
    /// `Goal: 16 Hours`.
    pub goal_label: String,
    /// Share of the goal reached, 0.0 to 1.0.
    pub progress: f32,
    pub stage_name: String,
    pub stage_description: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageRow {
    pub name: String,
    pub description: String,
    pub reached: bool,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryBar {
    pub label: String,
    pub hours: f32,
    /// 0 amber, 1 primary, 2 emerald, 3 purple. See [`tone_for_hours`].
    pub tone: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FastingView {
    /// Protocol labels, Custom last.
    pub protocols: Vec<String>,
    pub protocol: usize,
    pub custom_hours: String,
    pub custom_start_on: bool,
    /// Calendar of the retroactive start: 42 cells, Sunday first.
    pub start_cells: Vec<CalendarCell>,
    /// `November 2026`.
    pub start_title: String,
    pub weekday_labels: Vec<String>,
    /// The picked start as `22 Nov 2026, 09:00 UTC`, empty when none.
    pub start_label: String,
    pub start_hour: u32,
    pub start_minute: u32,
    pub active: Option<ActiveFast>,
    pub stages: Vec<StageRow>,
    pub history: Vec<HistoryBar>,
}

/// One cell of the start calendar, shaped like the shared `CalendarDay`.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarCell {
    pub day: u32,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetView {
    pub id: String,
    pub kg: String,
    pub reps: String,
    pub rest: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExerciseView {
    pub id: String,
    pub name: String,
    pub notes: String,
    pub sets: Vec<SetView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExerciseCategoryView {
    pub id: String,
    pub name: String,
    pub exercises: Vec<ExerciseView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StretchItemView {
    pub id: String,
    pub name: String,
    pub duration: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StretchCategoryView {
    pub id: String,
    pub name: String,
    pub items: Vec<StretchItemView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunnerView {
    pub routine: String,
    /// Name of the current stretch, or "Routine complete".
    pub current: String,
    /// Seconds left, empty when finished.
    pub remaining: String,
    /// `2 of 5`.
    pub step: String,
    pub paused: bool,
    pub finished: bool,
    pub progress: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkoutsView {
    /// 0 for "Exercises & Workouts", 1 for "Stretching Routines".
    pub tab: usize,
    pub exercises: Vec<ExerciseCategoryView>,
    pub stretches: Vec<StretchCategoryView>,
    /// Seconds left of the rest countdown started by a completed set.
    pub rest_remaining: Option<u32>,
    pub runner: Option<RunnerView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub target: String,
    /// Twelve cells, January first. Empty text when there is no reading.
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GoalsView {
    pub weight_unit: String,
    pub current_weight: String,
    pub target_weight: String,
    pub current_body_fat: String,
    pub target_body_fat: String,
    /// Target minus current weight, signed, one decimal.
    pub weight_gap: String,
    pub body_fat_gap: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeasurementsView {
    pub years: Vec<String>,
    pub year: String,
    pub metrics: Vec<MetricRow>,
    pub goals: GoalsView,
    /// The chart's track (id and name). Empty when there is no track.
    pub chart_metric_id: String,
    pub chart_metric_name: String,
    /// Twelve bars, January first, for the selected year. Missing months are 0.
    pub chart: Vec<ChartPointView>,
}

/// One bar of the measurements chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPointView {
    pub label: String,
    pub value: f32,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HealthView {
    pub loading: bool,
    pub error: Option<String>,
    /// A message for the user that is not an error (for example "Enter a valid start time").
    pub notice: Option<String>,
    pub fasting: FastingView,
    pub workouts: WorkoutsView,
    pub measurements: MeasurementsView,
}

// ---------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------

/// Which field of a workout set a write changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetField {
    Kg,
    Reps,
    Rest,
}

struct State {
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    fasts: Vec<FastEntry>,
    protocol: usize,
    custom_hours: String,
    custom_start_on: bool,
    /// The retroactive start picked in the calendar: (year, month, day), plus the hour and minute.
    start_pick: Option<(i32, u32, u32)>,
    start_hour: u32,
    start_minute: u32,
    /// The month the start calendar shows.
    start_month: (i32, u32),
    workouts: WorkoutData,
    tab: usize,
    done_sets: HashSet<String>,
    rest_ends: Option<f64>,
    runner: Option<Runner>,
    measurements: MeasurementsData,
    year: i32,
    /// Metric shown in the chart. `None` shows the first track.
    chart_metric: Option<String>,
    /// Last fasting stage index a cue was played for (`None` when no fast runs).
    last_stage: Option<usize>,
    /// Last routine step announced: (index, finished).
    announced: Option<(usize, bool)>,
    /// Whether the keep-awake lock is held.
    awake: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            loading: false,
            error: None,
            notice: None,
            fasts: Vec::new(),
            protocol: 0,
            custom_hours: String::new(),
            custom_start_on: false,
            start_pick: None,
            start_hour: 9,
            start_minute: 0,
            start_month: {
                let today = idantity_core::local_date::today();
                (today.year, today.month)
            },
            workouts: WorkoutData::default(),
            tab: 0,
            done_sets: HashSet::new(),
            rest_ends: None,
            runner: None,
            measurements: MeasurementsData::default(),
            year: idantity_core::local_date::today().year,
            chart_metric: None,
            last_stage: None,
            announced: None,
            awake: false,
        }
    }
}

impl State {
    /// Tones due since the last call: a cue when the fasting stage changes, and one per routine step and at its end.
    /// Advances the routine to `now` first.
    fn cues(&mut self, now: f64) -> Vec<Tone> {
        let mut out = Vec::new();
        let stage = self
            .fasts
            .iter()
            .find(|f| f.is_active)
            .and_then(|f| parse_iso_secs(&f.start_time))
            .map(|start| stage_index(((now - start) / 3600.0).max(0.0)));
        if let (Some(prev), Some(cur)) = (self.last_stage, stage)
            && prev != cur
        {
            out.push(STAGE_CUE);
        }
        self.last_stage = stage;

        if let Some(runner) = self.runner.as_mut() {
            runner.advance(now);
            let here = (runner.index, runner.finished);
            if let Some(prev) = self.announced
                && prev != here
            {
                out.push(STEP_CUE);
            }
            self.announced = Some(here);
        } else {
            self.announced = None;
        }
        out
    }

    /// The screen stays awake during an active fast and while a stretch routine runs.
    fn wants_awake(&self) -> bool {
        self.fasts.iter().any(|f| f.is_active) || self.runner.as_ref().is_some_and(|r| !r.finished)
    }

    fn view(&mut self, now: f64) -> HealthView {
        if let Some(runner) = self.runner.as_mut() {
            runner.advance(now);
        }
        HealthView {
            loading: self.loading,
            error: self.error.clone(),
            notice: self.notice.clone(),
            fasting: self.fasting_view(now),
            workouts: self.workouts_view(now),
            measurements: self.measurements_view(),
        }
    }

    fn fasting_view(&self, now: f64) -> FastingView {
        let active = self.fasts.iter().find(|f| f.is_active).map(|fast| {
            let start = parse_iso_secs(&fast.start_time).unwrap_or(now);
            let elapsed_hours = ((now - start) / 3600.0).max(0.0);
            let goal = fast_hours(fast);
            let stage = &STAGES[stage_index(elapsed_hours)];
            ActiveFast {
                id: fast.id.clone(),
                started: started_label(&fast.start_time),
                elapsed: format_clock(now - start),
                elapsed_hours,
                goal_label: format!("Goal: {:.0} Hours ({})", goal, format_clock(goal * 3600.0)),
                progress: (elapsed_hours / goal.max(1.0)).clamp(0.0, 1.0) as f32,
                stage_name: stage.name.to_owned(),
                stage_description: stage.description.to_owned(),
            }
        });
        let current_hours = active.as_ref().map(|a| a.elapsed_hours);
        let stages = STAGES
            .iter()
            .enumerate()
            .map(|(i, s)| StageRow {
                name: s.name.to_owned(),
                description: s.description.to_owned(),
                reached: current_hours.is_some_and(|h| h >= s.start),
                current: current_hours.is_some_and(|h| stage_index(h) == i),
            })
            .collect();
        let completed: Vec<&FastEntry> = self.fasts.iter().filter(|f| !f.is_active).collect();
        // The web shows the last 14 fasts, oldest first, each at least 0.5 hours and rounded to one decimal.
        let history: Vec<HistoryBar> = completed
            .iter()
            .take(HISTORY_LIMIT)
            .rev()
            .map(|fast| {
                let hours = fast_hours(fast).max(0.5);
                let hours = (hours * 10.0).round() / 10.0;
                HistoryBar {
                    label: month_day_label(&fast.start_time),
                    hours: hours as f32,
                    tone: tone_for_hours(hours),
                }
            })
            .collect();
        let (start_cells, start_title) = start_calendar(self.start_month, self.start_pick);
        let start_label = self
            .start_pick
            .map(|(y, m, d)| {
                // The picked local date and time, shown as the picker gives it.
                format!(
                    "{} {d}, {y} {:02}:{:02}",
                    MONTH_NAMES[(m - 1) as usize],
                    self.start_hour,
                    self.start_minute
                )
            })
            .unwrap_or_default();
        let mut protocols: Vec<String> = PROTOCOLS.iter().map(|(l, _)| (*l).to_owned()).collect();
        protocols.push("Custom".to_owned());
        FastingView {
            protocols,
            protocol: self.protocol,
            custom_hours: self.custom_hours.clone(),
            custom_start_on: self.custom_start_on,
            start_cells,
            start_title,
            weekday_labels: WEEKDAYS.iter().map(|w| (*w).to_owned()).collect(),
            start_label,
            start_hour: self.start_hour,
            start_minute: self.start_minute,
            active,
            stages,
            history,
        }
    }

    fn workouts_view(&self, now: f64) -> WorkoutsView {
        let exercises = self
            .workouts
            .exercises
            .iter()
            .map(|cat| ExerciseCategoryView {
                id: cat.id.clone(),
                name: cat.name.clone(),
                exercises: cat
                    .items
                    .iter()
                    .map(|ex| ExerciseView {
                        id: ex.id.clone(),
                        name: ex.name.clone(),
                        notes: ex.notes.clone(),
                        sets: ex
                            .sets
                            .iter()
                            .map(|s| SetView {
                                id: s.id.clone(),
                                kg: s.kg.clone(),
                                reps: s.reps.clone(),
                                rest: s.rest_timer.to_string(),
                                done: self.done_sets.contains(&s.id),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        let stretches = self
            .workouts
            .stretches
            .iter()
            .map(|cat| StretchCategoryView {
                id: cat.id.clone(),
                name: cat.name.clone(),
                items: cat
                    .items
                    .iter()
                    .map(|item| StretchItemView {
                        id: item.id.clone(),
                        name: item.name.clone(),
                        duration: item.duration.to_string(),
                    })
                    .collect(),
            })
            .collect();
        let rest_remaining = self
            .rest_ends
            .map(|end| (end - now).ceil().max(0.0) as u32)
            .filter(|&secs| secs > 0);
        let runner = self.runner.as_ref().map(|r| {
            let step = r
                .items
                .get(r.index)
                .map(|(n, _)| n.clone())
                .unwrap_or_default();
            RunnerView {
                routine: r.routine.clone(),
                current: if r.finished {
                    "Routine complete".to_owned()
                } else {
                    step
                },
                remaining: if r.finished {
                    String::new()
                } else {
                    r.seconds_left().to_string()
                },
                step: format!("{} of {}", (r.index + 1).min(r.items.len()), r.items.len()),
                paused: r.paused,
                finished: r.finished,
                progress: r.progress(),
            }
        });
        WorkoutsView {
            tab: self.tab,
            exercises,
            stretches,
            rest_remaining,
            runner,
        }
    }

    fn measurements_view(&self) -> MeasurementsView {
        let mut years: Vec<i32> = self
            .measurements
            .measurements
            .iter()
            .flat_map(|m| m.values.keys().filter_map(|y| y.parse::<i32>().ok()))
            .collect();
        years.push(self.year);
        years.sort_unstable();
        years.dedup();
        let g = &self.measurements.goals;
        let year_key = self.year.to_string();
        let chart_track = self
            .measurements
            .measurements
            .iter()
            .find(|m| Some(&m.id) == self.chart_metric.as_ref())
            .or_else(|| self.measurements.measurements.first());
        let chart = (0u8..12)
            .map(|month| {
                let value = chart_track
                    .and_then(|m| m.values.get(&year_key))
                    .and_then(|list| list.iter().find(|v| v.month == month))
                    .and_then(|v| v.value.as_f64())
                    .unwrap_or(0.0);
                ChartPointView {
                    label: MONTH_NAMES[usize::from(month)].to_owned(),
                    value: value as f32,
                }
            })
            .collect();
        MeasurementsView {
            chart_metric_id: chart_track.map(|m| m.id.clone()).unwrap_or_default(),
            chart_metric_name: chart_track.map(|m| m.name.clone()).unwrap_or_default(),
            chart,
            years: years.iter().map(|y| y.to_string()).collect(),
            year: year_key.clone(),
            metrics: self
                .measurements
                .measurements
                .iter()
                .map(|m| {
                    let months = m.values.get(&year_key);
                    MetricRow {
                        id: m.id.clone(),
                        name: m.name.clone(),
                        unit: m.unit.clone(),
                        target: m.target.as_ref().map(Reading::display).unwrap_or_default(),
                        cells: (0u8..12)
                            .map(|month| {
                                months
                                    .and_then(|list| list.iter().find(|v| v.month == month))
                                    .map(|v| v.value.display())
                                    .unwrap_or_default()
                            })
                            .collect(),
                    }
                })
                .collect(),
            goals: GoalsView {
                weight_unit: g.weight_unit.clone(),
                current_weight: trim_num(g.current_weight),
                target_weight: trim_num(g.target_weight),
                current_body_fat: trim_num(g.current_body_fat),
                target_body_fat: trim_num(g.target_body_fat),
                weight_gap: gap_text(g.current_weight, g.target_weight),
                body_fat_gap: gap_text(g.current_body_fat, g.target_body_fat),
            },
        }
    }
}

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// Sunday first, as the calendar grid lays them out.
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// `M/D` for an ISO time, the numeric month and day the web's history bars show (UTC).
fn month_day_label(iso: &str) -> String {
    match idantity_core::local_date::local_day_of(iso) {
        Some(day) => format!("{}/{}", day.month, day.day),
        None => iso.to_owned(),
    }
}

/// The 42 cells of the calendar for `month`, Sunday first, and its title.
fn start_calendar(month: (i32, u32), pick: Option<(i32, u32, u32)>) -> (Vec<CalendarCell>, String) {
    let today = idantity_core::local_date::today();
    let cells = (0..42)
        .map(|i| {
            let (y, m, d) = calendar_date_at(month, i);
            CalendarCell {
                day: d,
                in_month: (y, m) == month,
                today: (y, m, d) == (today.year, today.month, today.day),
                selected: pick == Some((y, m, d)),
            }
        })
        .collect();
    let title = format!("{} {}", MONTH_FULL[(month.1 - 1) as usize], month.0);
    (cells, title)
}

/// The date shown in calendar cell `index` (0 to 41) of `month`. Cells before and after the month belong to the
/// neighbouring months, so the result can fall outside `month`.
pub fn calendar_date_at(month: (i32, u32), index: usize) -> (i32, u32, u32) {
    let first = days_from_civil(i64::from(month.0), i64::from(month.1), 1);
    let lead = (first + 4).rem_euclid(7);
    let (y, m, d) = civil_from_days(first + index as i64 - lead);
    (y as i32, m as u32, d as u32)
}

/// `22 Nov 2026, 10:00 UTC` for an ISO time. The raw text when it does not parse.
fn started_label(iso: &str) -> String {
    match local_time::month_day_time(iso) {
        Some(text) => text,
        None => iso.to_owned(),
    }
}

struct Shared {
    api: Arc<dyn HealthApi>,
    feedback: Arc<dyn Feedback>,
    state: Mutex<State>,
    notify: Arc<dyn Fn(HealthView) + Send + Sync>,
}

/// The Health page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct HealthController {
    shared: Arc<Shared>,
}

impl HealthController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn HealthApi>,
        feedback: Arc<dyn Feedback>,
        notify: impl Fn(HealthView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                feedback,
                state: Mutex::new(State::default()),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view at the current time, without notifying.
    pub fn view(&self) -> HealthView {
        self.lock().view(now_secs())
    }

    /// Re-sends the view. The glue calls this once a second for the timers.
    pub fn tick(&self) {
        self.change(|_| {});
    }

    /// Loads fasts, measurements and workouts.
    pub fn load_all(&self) {
        self.change(|s| s.loading = true);
        self.refresh_fasts();
        self.refresh_measurements();
        self.refresh_workouts();
    }

    pub fn refresh_fasts(&self) {
        let me = self.clone();
        self.shared.api.fasting_list(Box::new(move |result| {
            me.change(|s| {
                s.loading = false;
                match result {
                    Ok(fasts) => {
                        s.fasts = fasts;
                        s.error = None;
                    }
                    Err(e) => s.error = Some(e.to_string()),
                }
            });
        }));
    }

    /// Chooses a protocol button. `index` is 0 to 4 for the presets, 5 for Custom.
    pub fn select_protocol(&self, index: usize) {
        self.change(|s| {
            s.protocol = index.min(CUSTOM_PROTOCOL);
            s.notice = None;
        });
    }

    pub fn set_custom_hours(&self, text: &str) {
        self.change(|s| s.custom_hours = text.to_owned());
    }

    pub fn set_custom_start_on(&self, on: bool) {
        self.change(|s| {
            s.custom_start_on = on;
            s.notice = None;
        });
    }

    /// Picks the retroactive start day: `index` is the calendar cell (0 to 41) the user clicked.
    pub fn pick_start_day(&self, index: usize) -> bool {
        if index >= 42 {
            return false;
        }
        self.change(|s| {
            let (y, m, d) = calendar_date_at(s.start_month, index);
            s.start_pick = Some((y, m, d));
            s.start_month = (y, m);
        });
        true
    }

    /// Moves the start calendar by `delta` months (-1 or 1).
    pub fn step_start_month(&self, delta: i32) {
        self.change(|s| {
            let total = s.start_month.0 * 12 + s.start_month.1 as i32 - 1 + delta;
            s.start_month = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        });
    }

    /// Sets the retroactive start time (UTC). Hours 0 to 23 and minutes 0 to 59 only.
    pub fn set_start_time(&self, hour: u32, minute: u32) -> bool {
        if hour > 23 || minute > 59 {
            return false;
        }
        self.change(|s| {
            s.start_hour = hour;
            s.start_minute = minute;
        });
        true
    }

    /// Hours of the selected protocol. Custom reads the hours field (1 to 168, default 16).
    pub fn protocol_hours(&self) -> u32 {
        let s = self.lock();
        if s.protocol == CUSTOM_PROTOCOL {
            s.custom_hours
                .trim()
                .parse::<u32>()
                .unwrap_or(16)
                .clamp(1, 168)
        } else {
            PROTOCOLS[s.protocol.min(CUSTOM_PROTOCOL - 1)].1
        }
    }

    /// Starts a fast now, or at the retroactive start when that is set. An unreadable start is refused with a
    /// notice. Returns `true` when the call was sent.
    pub fn start_fast(&self) -> bool {
        let hours = self.protocol_hours();
        let start = {
            let s = self.lock();
            if s.custom_start_on {
                // The picked day and time are local, as the web's datetime input gives them; the API gets UTC.
                let iso = s.start_pick.and_then(|(y, m, d)| {
                    local_time::local_to_iso(y, m, d, s.start_hour, s.start_minute)
                });
                let Some(iso) = iso else {
                    drop(s);
                    self.change(|st| st.notice = Some("Enter a valid start time".to_owned()));
                    return false;
                };
                iso
            } else {
                iso_from_secs(now_secs())
            }
        };
        let me = self.clone();
        self.shared.api.fasting_start(
            &start,
            hours,
            Box::new(move |result| match result {
                Ok(_) => {
                    me.change(|s| {
                        s.custom_start_on = false;
                        s.start_pick = None;
                        s.notice = None;
                    });
                    me.refresh_fasts();
                }
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// Ends the active fast `id`, then reloads the fasts.
    pub fn end_fast(&self, id: &str) {
        let me = self.clone();
        self.shared.api.fasting_end(
            id,
            Box::new(move |result| match result {
                Ok(()) => me.refresh_fasts(),
                Err(e) => me.fail(e),
            }),
        );
    }

    pub fn refresh_measurements(&self) {
        let me = self.clone();
        self.shared.api.measurements_get(Box::new(move |result| {
            me.change(|s| match result {
                Ok(data) => {
                    s.measurements = data;
                    s.error = None;
                }
                Err(e) => s.error = Some(e.to_string()),
            });
        }));
    }

    pub fn refresh_workouts(&self) {
        let me = self.clone();
        self.shared.api.workouts_get(Box::new(move |result| {
            let mut seed = false;
            me.change(|s| match result {
                Ok(Some(data)) => {
                    s.workouts = data;
                    s.error = None;
                }
                Ok(None) => {
                    s.workouts = default_workouts();
                    seed = true;
                }
                Err(e) => s.error = Some(e.to_string()),
            });
            if seed {
                me.save_workouts();
            }
        }));
    }

    /// Switches between the exercise tab (0) and the stretch tab (1).
    pub fn select_tab(&self, tab: usize) {
        self.change(|s| s.tab = tab.min(1));
    }

    // ----- Workouts -----

    /// Sends the workout data to the server. Called after every edit.
    fn save_workouts(&self) {
        let data = self.lock().workouts.clone();
        let me = self.clone();
        self.shared.api.workouts_save(
            &data,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.fail(e);
                }
            }),
        );
    }

    /// Applies `edit` to the workouts, and saves when it reports a hit.
    fn edit_workouts(&self, edit: impl FnOnce(&mut WorkoutData) -> bool) -> bool {
        let mut hit = false;
        self.change(|s| hit = edit(&mut s.workouts));
        if hit {
            self.save_workouts();
        }
        hit
    }

    /// Adds an exercise day (for example "Chest & triceps"). Blank names are refused.
    pub fn add_exercise_category(&self, name: &str) -> bool {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return false;
        }
        self.edit_workouts(|w| {
            w.exercises.push(WorkoutExerciseCategory {
                id: new_id(),
                name,
                items: Vec::new(),
            });
            true
        })
    }

    pub fn delete_exercise_category(&self, category_id: &str) -> bool {
        self.edit_workouts(|w| {
            let before = w.exercises.len();
            w.exercises.retain(|c| c.id != category_id);
            w.exercises.len() != before
        })
    }

    /// Adds an exercise with one empty set. Blank names are refused.
    pub fn add_exercise(&self, category_id: &str, name: &str) -> bool {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return false;
        }
        self.edit_workouts(|w| {
            let Some(cat) = w.exercises.iter_mut().find(|c| c.id == category_id) else {
                return false;
            };
            cat.items.push(WorkoutExercise {
                id: new_id(),
                name,
                notes: String::new(),
                sets: vec![ExerciseSet {
                    id: new_id(),
                    kg: String::new(),
                    reps: String::new(),
                    rest_timer: 60,
                }],
            });
            true
        })
    }

    pub fn delete_exercise(&self, category_id: &str, exercise_id: &str) -> bool {
        self.edit_workouts(|w| {
            let Some(cat) = w.exercises.iter_mut().find(|c| c.id == category_id) else {
                return false;
            };
            let before = cat.items.len();
            cat.items.retain(|e| e.id != exercise_id);
            cat.items.len() != before
        })
    }

    pub fn set_exercise_notes(&self, category_id: &str, exercise_id: &str, notes: &str) -> bool {
        self.edit_workouts(|w| {
            let Some(ex) = find_exercise_mut(w, category_id, exercise_id) else {
                return false;
            };
            ex.notes = notes.to_owned();
            true
        })
    }

    /// Adds a set with the web defaults (blank weight and reps, 60 seconds rest).
    pub fn add_set(&self, category_id: &str, exercise_id: &str) -> bool {
        self.edit_workouts(|w| {
            let Some(ex) = find_exercise_mut(w, category_id, exercise_id) else {
                return false;
            };
            ex.sets.push(ExerciseSet {
                id: new_id(),
                kg: String::new(),
                reps: String::new(),
                rest_timer: 60,
            });
            true
        })
    }

    pub fn delete_set(&self, category_id: &str, exercise_id: &str, set_id: &str) -> bool {
        self.edit_workouts(|w| {
            let Some(ex) = find_exercise_mut(w, category_id, exercise_id) else {
                return false;
            };
            let before = ex.sets.len();
            ex.sets.retain(|s| s.id != set_id);
            ex.sets.len() != before
        })
    }

    /// Writes one field of a set. Rest is whole seconds: blank means 0, anything else that is not a number is
    /// refused.
    pub fn set_set_field(
        &self,
        category_id: &str,
        exercise_id: &str,
        set_id: &str,
        field: SetField,
        value: &str,
    ) -> bool {
        let value = value.trim().to_owned();
        if field == SetField::Rest && !value.is_empty() && value.parse::<u32>().is_err() {
            return false;
        }
        self.edit_workouts(|w| {
            let Some(ex) = find_exercise_mut(w, category_id, exercise_id) else {
                return false;
            };
            let Some(set) = ex.sets.iter_mut().find(|s| s.id == set_id) else {
                return false;
            };
            match field {
                SetField::Kg => set.kg = value,
                SetField::Reps => set.reps = value,
                SetField::Rest => set.rest_timer = value.parse().unwrap_or(0),
            }
            true
        })
    }

    /// Toggles a set's completion. Completing a set starts its rest countdown.
    pub fn toggle_set_done(&self, category_id: &str, exercise_id: &str, set_id: &str) -> bool {
        let rest = {
            let s = self.lock();
            find_exercise(&s.workouts, category_id, exercise_id)
                .and_then(|ex| ex.sets.iter().find(|set| set.id == set_id))
                .map(|set| set.rest_timer)
        };
        let Some(rest) = rest else { return false };
        let now = now_secs();
        self.change(|s| {
            if !s.done_sets.remove(set_id) {
                s.done_sets.insert(set_id.to_owned());
                s.rest_ends = Some(now + f64::from(rest));
            }
        });
        true
    }

    pub fn add_stretch_category(&self, name: &str) -> bool {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return false;
        }
        self.edit_workouts(|w| {
            w.stretches.push(StretchCategory {
                id: new_id(),
                name,
                items: Vec::new(),
            });
            true
        })
    }

    pub fn delete_stretch_category(&self, category_id: &str) -> bool {
        self.edit_workouts(|w| {
            let before = w.stretches.len();
            w.stretches.retain(|c| c.id != category_id);
            w.stretches.len() != before
        })
    }

    /// Adds a stretch. Duration is whole seconds; blank or non-numeric durations and blank names are refused.
    pub fn add_stretch(&self, category_id: &str, name: &str, duration: &str) -> bool {
        let name = name.trim().to_owned();
        let Ok(duration) = duration.trim().parse::<u32>() else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        self.edit_workouts(|w| {
            let Some(cat) = w.stretches.iter_mut().find(|c| c.id == category_id) else {
                return false;
            };
            cat.items.push(StretchItem {
                id: new_id(),
                name,
                duration,
            });
            true
        })
    }

    pub fn delete_stretch(&self, category_id: &str, stretch_id: &str) -> bool {
        self.edit_workouts(|w| {
            let Some(cat) = w.stretches.iter_mut().find(|c| c.id == category_id) else {
                return false;
            };
            let before = cat.items.len();
            cat.items.retain(|i| i.id != stretch_id);
            cat.items.len() != before
        })
    }

    /// Start Routine. A routine with no stretches is refused.
    pub fn start_routine(&self, category_id: &str) -> bool {
        let now = now_secs();
        let started = {
            let s = self.lock();
            s.workouts
                .stretches
                .iter()
                .find(|c| c.id == category_id)
                .and_then(|c| {
                    let items = c
                        .items
                        .iter()
                        .map(|i| (i.name.clone(), i.duration))
                        .collect();
                    Runner::start(&c.name, items, now)
                })
        };
        let Some(runner) = started else { return false };
        self.change(|s| s.runner = Some(runner));
        true
    }

    /// A player command for the running routine: `pause` (toggles), `next`, `previous` or `stop`.
    pub fn routine_command(&self, command: &str) {
        let now = now_secs();
        self.change(|s| match command {
            "stop" => s.runner = None,
            "pause" => {
                if let Some(r) = s.runner.as_mut() {
                    r.toggle_pause(now);
                }
            }
            "next" => {
                if let Some(r) = s.runner.as_mut() {
                    r.next(now);
                }
            }
            "previous" => {
                if let Some(r) = s.runner.as_mut() {
                    r.previous(now);
                }
            }
            _ => {}
        });
    }

    // ----- Measurements -----

    fn save_measurements(&self) {
        let data = self.lock().measurements.clone();
        let me = self.clone();
        self.shared.api.measurements_save(
            &data,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.fail(e);
                }
            }),
        );
    }

    /// Shows another year in the matrix. A non-year is refused.
    pub fn select_year(&self, year: &str) -> bool {
        let Ok(year) = year.trim().parse::<i32>() else {
            return false;
        };
        self.change(|s| s.year = year);
        true
    }

    /// Chooses the track shown in the chart. An unknown id is refused.
    pub fn select_chart_metric(&self, metric_id: &str) -> bool {
        let known = self
            .lock()
            .measurements
            .measurements
            .iter()
            .any(|m| m.id == metric_id);
        if !known {
            return false;
        }
        self.change(|s| s.chart_metric = Some(metric_id.to_owned()));
        true
    }

    /// Writes one month of a track in the selected year. Blank clears the month. A number is stored as a number,
    /// other text as text (the web keeps both). Non-finite numbers are refused.
    pub fn set_cell(&self, metric_id: &str, month: u8, text: &str) -> bool {
        let text = text.trim().to_owned();
        if month > 11 {
            return false;
        }
        let value = if text.is_empty() {
            None
        } else {
            match text.parse::<f64>() {
                Ok(n) if n.is_finite() => Some(Reading::Number(n)),
                Ok(_) => return false,
                Err(_) => Some(Reading::Text(text.clone())),
            }
        };
        let mut hit = false;
        self.change(|s| {
            let year = s.year.to_string();
            let mut sync: Option<(bool, f64)> = None;
            if let Some(metric) = s
                .measurements
                .measurements
                .iter_mut()
                .find(|m| m.id == metric_id)
            {
                let name = metric.name.to_lowercase();
                let months = metric.values.entry(year).or_default();
                months.retain(|v| v.month != month);
                if let Some(value) = value {
                    months.push(MonthValue { month, value });
                }
                months.sort_by_key(|v| v.month);
                // The web copies the latest numeric reading of a weight or fat track into the current goal.
                if let Some(Reading::Number(latest)) = months.last().map(|v| v.value.clone()) {
                    if name.contains("weight") {
                        sync = Some((true, latest));
                    } else if name.contains("fat") {
                        sync = Some((false, latest));
                    }
                }
                hit = true;
            }
            match sync {
                Some((true, n)) => s.measurements.goals.current_weight = n,
                Some((false, n)) => s.measurements.goals.current_body_fat = n,
                None => {}
            }
        });
        if hit {
            self.save_measurements();
        }
        hit
    }

    /// Adds a track. Blank names are refused; an unknown unit falls back to cm; a target must be a number when
    /// given.
    pub fn add_metric(&self, name: &str, unit: &str, target: &str) -> bool {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return false;
        }
        let unit = if UNITS.contains(&unit) {
            unit.to_owned()
        } else {
            "cm".to_owned()
        };
        let target = target.trim();
        let target = if target.is_empty() {
            None
        } else {
            match target.parse::<f64>() {
                Ok(n) if n.is_finite() => Some(Reading::Number(n)),
                _ => return false,
            }
        };
        self.change(|s| {
            s.measurements.measurements.push(Measurement {
                id: new_id(),
                name,
                unit,
                target,
                values: Default::default(),
            });
        });
        self.save_measurements();
        true
    }

    pub fn remove_metric(&self, metric_id: &str) -> bool {
        let mut hit = false;
        self.change(|s| {
            let before = s.measurements.measurements.len();
            s.measurements.measurements.retain(|m| m.id != metric_id);
            hit = s.measurements.measurements.len() != before;
        });
        if hit {
            self.save_measurements();
        }
        hit
    }

    /// Saves the goals. Each number must parse; the weight unit must be `kg` or `lb`.
    pub fn save_goals(
        &self,
        weight_unit: &str,
        current_weight: &str,
        target_weight: &str,
        current_body_fat: &str,
        target_body_fat: &str,
    ) -> bool {
        let parse = |t: &str| t.trim().parse::<f64>().ok().filter(|n| n.is_finite());
        let (Some(cw), Some(tw), Some(cb), Some(tb)) = (
            parse(current_weight),
            parse(target_weight),
            parse(current_body_fat),
            parse(target_body_fat),
        ) else {
            return false;
        };
        if weight_unit != "kg" && weight_unit != "lb" {
            return false;
        }
        self.change(|s| {
            s.measurements.goals = Goals {
                current_weight: cw,
                target_weight: tw,
                current_body_fat: cb,
                target_body_fat: tb,
                weight_unit: weight_unit.to_owned(),
            };
        });
        self.save_measurements();
        true
    }

    /// Clears the error message and the notice.
    pub fn dismiss(&self) {
        self.change(|s| {
            s.error = None;
            s.notice = None;
        });
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| s.error = Some(error.to_string()));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, update: impl FnOnce(&mut State)) {
        let now = now_secs();
        let (view, cues, awake_change) = {
            let mut state = self.lock();
            update(&mut state);
            let cues = state.cues(now);
            let awake = state.wants_awake();
            let awake_change = (awake != state.awake).then_some(awake);
            state.awake = awake;
            (state.view(now), cues, awake_change)
        };
        // The platform calls run after the lock is released.
        for tone in cues {
            self.shared.feedback.tone(tone);
        }
        if let Some(on) = awake_change {
            self.shared.feedback.keep_awake(on);
        }
        (self.shared.notify)(view);
    }
}

fn find_exercise<'a>(
    w: &'a WorkoutData,
    category_id: &str,
    exercise_id: &str,
) -> Option<&'a WorkoutExercise> {
    w.exercises
        .iter()
        .find(|c| c.id == category_id)?
        .items
        .iter()
        .find(|e| e.id == exercise_id)
}

fn find_exercise_mut<'a>(
    w: &'a mut WorkoutData,
    category_id: &str,
    exercise_id: &str,
) -> Option<&'a mut WorkoutExercise> {
    w.exercises
        .iter_mut()
        .find(|c| c.id == category_id)?
        .items
        .iter_mut()
        .find(|e| e.id == exercise_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each call and answers at once. `fail` makes the writes return a server error.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        fasts: Mutex<Vec<FastEntry>>,
        measurements: Mutex<MeasurementsData>,
        workouts: Mutex<Option<WorkoutData>>,
        fail: Mutex<bool>,
        /// Tone frequencies played, in order.
        tones: Mutex<Vec<f32>>,
        /// Keep-awake calls, in order.
        awake: Mutex<Vec<bool>>,
    }

    impl Feedback for Arc<Fake> {
        fn tone(&self, tone: Tone) {
            self.tones.lock().unwrap().push(tone.hz);
        }

        fn keep_awake(&self, on: bool) {
            self.awake.lock().unwrap().push(on);
        }
    }

    impl Fake {
        fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_owned());
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn write_result(&self) -> Result<(), ApiError> {
            if *self.fail.lock().unwrap() {
                Err(ApiError::Status {
                    code: 500,
                    body: "boom".to_owned(),
                })
            } else {
                Ok(())
            }
        }
    }

    impl HealthApi for Arc<Fake> {
        fn fasting_list(&self, on_done: Done<Vec<FastEntry>>) {
            self.record("fasts");
            on_done(Ok(self.fasts.lock().unwrap().clone()));
        }

        fn fasting_start(&self, start_time: &str, duration_hours: u32, on_done: Done<FastEntry>) {
            self.record(&format!("start:{duration_hours}:{start_time}"));
            let result = self.write_result().map(|()| {
                let fast = FastEntry {
                    id: "f-new".to_owned(),
                    start_time: start_time.to_owned(),
                    end_time: None,
                    original_end_time: None,
                    is_active: true,
                };
                self.fasts.lock().unwrap().push(fast.clone());
                fast
            });
            on_done(result);
        }

        fn fasting_end(&self, id: &str, on_done: Done<()>) {
            self.record(&format!("end:{id}"));
            for fast in self.fasts.lock().unwrap().iter_mut().filter(|f| f.id == id) {
                fast.is_active = false;
                fast.end_time = Some("2026-11-22T10:00:00.000Z".to_owned());
            }
            on_done(self.write_result());
        }

        fn measurements_get(&self, on_done: Done<MeasurementsData>) {
            self.record("measurements_get");
            on_done(Ok(self.measurements.lock().unwrap().clone()));
        }

        fn measurements_save(&self, data: &MeasurementsData, on_done: Done<()>) {
            self.record("measurements_save");
            *self.measurements.lock().unwrap() = data.clone();
            on_done(self.write_result());
        }

        fn workouts_get(&self, on_done: Done<Option<WorkoutData>>) {
            self.record("workouts_get");
            on_done(Ok(self.workouts.lock().unwrap().clone()));
        }

        fn workouts_save(&self, data: &WorkoutData, on_done: Done<()>) {
            self.record("workouts_save");
            *self.workouts.lock().unwrap() = Some(data.clone());
            on_done(self.write_result());
        }
    }

    fn controller(fake: &Arc<Fake>) -> (HealthController, Arc<Mutex<Vec<HealthView>>>) {
        let seen: Arc<Mutex<Vec<HealthView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn HealthApi> = Arc::new(fake.clone());
        let feedback: Arc<dyn Feedback> = Arc::new(fake.clone());
        let c = HealthController::new(api, feedback, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn loaded() -> (Arc<Fake>, HealthController) {
        let fake: Arc<Fake> = Arc::default();
        let (c, _) = controller(&fake);
        c.load_all();
        (fake, c)
    }

    fn active_fast() -> FastEntry {
        FastEntry {
            id: "f1".to_owned(),
            start_time: "2026-11-22T06:00:00.000Z".to_owned(),
            end_time: None,
            original_end_time: None,
            is_active: true,
        }
    }

    #[test]
    fn iso_round_trips_and_offsets_are_applied() {
        let secs = parse_iso_secs("2026-11-22T10:30:00.000Z").unwrap();
        assert_eq!(iso_from_secs(secs), "2026-11-22T10:30:00.000Z");
        let shifted = parse_iso_secs("2026-11-22T12:30:00+02:00").unwrap();
        assert_eq!(shifted, secs);
        assert!(parse_iso_secs("not a time").is_none());
    }

    #[test]
    fn stages_and_tones_follow_the_web_thresholds() {
        assert_eq!(stage_index(0.0), 0);
        assert_eq!(stage_index(4.0), 1);
        assert_eq!(stage_index(18.0), 3);
        assert_eq!(stage_index(100.0), 4);
        assert_eq!(tone_for_hours(15.9), 0);
        assert_eq!(tone_for_hours(16.0), 1);
        assert_eq!(tone_for_hours(18.0), 2);
        assert_eq!(tone_for_hours(24.0), 3);
    }

    #[test]
    fn runner_steps_through_items_and_skips_paused_time() {
        let items = vec![("a".to_owned(), 10), ("b".to_owned(), 5)];
        let mut r = Runner::start("R", items, 0.0).unwrap();
        r.advance(4.0);
        assert_eq!((r.index, r.seconds_left()), (0, 6));
        r.advance(11.0);
        assert_eq!((r.index, r.seconds_left()), (1, 4));
        r.toggle_pause(11.0);
        r.advance(100.0);
        assert_eq!(r.index, 1, "paused time does not advance the routine");
        r.toggle_pause(100.0);
        r.advance(120.0);
        assert!(r.finished);
        assert_eq!(r.progress(), 1.0);
    }

    #[test]
    fn runner_previous_and_next_move_one_step() {
        let items = vec![
            ("a".to_owned(), 10),
            ("b".to_owned(), 5),
            ("c".to_owned(), 7),
        ];
        let mut r = Runner::start("R", items, 0.0).unwrap();
        r.next(0.0);
        assert_eq!(r.index, 1);
        r.previous(0.0);
        assert_eq!((r.index, r.seconds_left()), (0, 10));
        assert!(Runner::start("empty", Vec::new(), 0.0).is_none());
    }

    #[test]
    fn load_all_seeds_the_web_workouts_when_the_server_has_none() {
        let (fake, c) = loaded();
        assert_eq!(
            fake.calls(),
            ["fasts", "measurements_get", "workouts_get", "workouts_save"]
        );
        let view = c.view();
        assert_eq!(view.workouts.exercises[0].name, "Push Day");
        assert_eq!(view.workouts.stretches.len(), 2);
    }

    #[test]
    fn start_fast_refuses_a_retroactive_start_without_a_day() {
        let (fake, c) = loaded();
        c.set_custom_start_on(true);
        assert!(!c.start_fast());
        assert_eq!(c.view().notice.as_deref(), Some("Enter a valid start time"));
        assert!(!fake.calls().iter().any(|call| call.starts_with("start")));
    }

    #[test]
    fn start_fast_sends_the_picked_retroactive_start_and_protocol_hours() {
        let (fake, c) = loaded();
        c.select_protocol(3);
        c.set_custom_start_on(true);
        // Pick the cell that shows the 20th of the calendar month.
        let cell = c
            .view()
            .fasting
            .start_cells
            .iter()
            .position(|cell| cell.in_month && cell.day == 20)
            .expect("the 20th is in every month");
        assert!(c.pick_start_day(cell));
        assert!(c.set_start_time(8, 30));
        assert!(c.start_fast());
        let sent = fake
            .calls()
            .into_iter()
            .find(|call| call.starts_with("start:24:"))
            .expect("start sent");
        // The API gets UTC; read back in the local zone it is the picked 08:30.
        assert!(sent.ends_with("Z"), "got {sent}");
        assert_eq!(local_time::local_hm(&sent), Some((8, 30)), "got {sent}");
        let view = c.view();
        assert!(view.fasting.active.is_some());
        assert!(
            !view.fasting.custom_start_on,
            "a started fast clears the custom start"
        );
    }

    #[test]
    fn start_time_is_range_checked_and_the_calendar_steps_months() {
        let (_, c) = loaded();
        assert!(!c.set_start_time(24, 0));
        assert!(!c.set_start_time(9, 60));
        assert!(c.set_start_time(23, 59));
        assert!(!c.pick_start_day(42));
        let before = c.view().fasting.start_title;
        c.step_start_month(1);
        assert_ne!(c.view().fasting.start_title, before);
        c.step_start_month(-1);
        assert_eq!(c.view().fasting.start_title, before);
    }

    #[test]
    fn calendar_has_the_right_number_of_days_in_month() {
        // February 2024 is a leap month with 29 days.
        let in_month = (0..42)
            .filter(|&i| calendar_date_at((2024, 2), i).1 == 2)
            .count();
        assert_eq!(in_month, 29);
        // 1 February 2024 was a Thursday, so the grid starts on the Sunday 28 January.
        assert_eq!(calendar_date_at((2024, 2), 0), (2024, 1, 28));
        assert_eq!(calendar_date_at((2024, 2), 4), (2024, 2, 1));
    }

    #[test]
    fn history_is_the_last_fourteen_fasts_with_numeric_month_day() {
        let fake: Arc<Fake> = Arc::default();
        let mut fasts = Vec::new();
        for i in 0..16 {
            fasts.push(FastEntry {
                id: format!("f{i}"),
                start_time: format!("2026-11-{:02}T06:00:00.000Z", i + 1),
                end_time: Some(format!("2026-11-{:02}T12:00:00.000Z", i + 1)),
                original_end_time: None,
                is_active: false,
            });
        }
        // Newest first, as the server lists them.
        fasts.reverse();
        *fake.fasts.lock().unwrap() = fasts;
        let (c, _) = controller(&fake);
        c.refresh_fasts();
        let history = c.view().fasting.history;
        assert_eq!(history.len(), 14);
        // Labels are the viewer's local month and day, so the expected text comes from the same local helper.
        let local = |iso: &str| {
            let day = idantity_core::local_date::local_day_of(iso).expect("valid instant");
            format!("{}/{}", day.month, day.day)
        };
        assert_eq!(
            history[0].label,
            local("2026-11-03T06:00:00.000Z"),
            "oldest of the last fourteen"
        );
        assert_eq!(history[13].label, local("2026-11-16T06:00:00.000Z"));
        assert_eq!(history[0].hours, 6.0);
    }

    #[test]
    fn gap_text_is_the_absolute_difference_or_empty() {
        assert_eq!(gap_text(75.4, 72.0), "3.4");
        assert_eq!(gap_text(70.0, 72.0), "2.0");
        assert_eq!(gap_text(0.0, 72.0), "");
    }

    #[test]
    fn custom_protocol_reads_the_hours_field_and_clamps() {
        let (_, c) = loaded();
        c.select_protocol(CUSTOM_PROTOCOL);
        c.set_custom_hours("999");
        assert_eq!(c.protocol_hours(), 168);
        c.set_custom_hours("abc");
        assert_eq!(c.protocol_hours(), 16);
        c.set_custom_hours("0");
        assert_eq!(c.protocol_hours(), 1);
    }

    #[test]
    fn end_fast_sends_the_id_and_reloads_into_history() {
        let fake: Arc<Fake> = Arc::default();
        fake.fasts.lock().unwrap().push(active_fast());
        let (c, _) = controller(&fake);
        c.refresh_fasts();
        assert!(c.view().fasting.active.is_some());
        c.end_fast("f1");
        assert!(fake.calls().contains(&"end:f1".to_owned()));
        let view = c.view();
        assert!(view.fasting.active.is_none());
        assert_eq!(view.fasting.history.len(), 1);
        assert_eq!(view.fasting.history[0].hours, 4.0);
        assert_eq!(view.fasting.history[0].tone, 0);
    }

    #[test]
    fn exercise_writes_validate_and_save() {
        let (fake, c) = loaded();
        let view = c.view();
        let cat = view.workouts.exercises[0].id.clone();
        let ex = view.workouts.exercises[0].exercises[0].id.clone();
        let set = view.workouts.exercises[0].exercises[0].sets[0].id.clone();

        assert!(
            !c.add_exercise_category("   "),
            "blank day names are refused"
        );
        assert!(c.add_exercise_category("  Legs "));
        assert!(c.add_exercise(&cat, "Squat"));
        assert!(!c.add_exercise(&cat, ""));
        assert!(c.set_exercise_notes(&cat, &ex, "slow eccentric"));
        assert!(c.add_set(&cat, &ex));
        assert!(c.set_set_field(&cat, &ex, &set, SetField::Kg, "62.5"));
        assert!(!c.set_set_field(&cat, &ex, &set, SetField::Rest, "soon"));
        assert!(c.set_set_field(&cat, &ex, &set, SetField::Rest, "90"));
        assert!(c.set_set_field(&cat, &ex, &set, SetField::Reps, "8"));
        assert!(c.delete_set(&cat, &ex, &set));
        assert!(c.delete_exercise(&cat, &ex));
        assert!(c.delete_exercise_category(&cat));

        let saved = fake.workouts.lock().unwrap().clone().unwrap();
        assert!(saved.exercises.iter().all(|c| c.id != cat));
        assert!(
            fake.calls()
                .iter()
                .filter(|c| c.as_str() == "workouts_save")
                .count()
                >= 8
        );
        assert!(!c.delete_stretch_category("missing"));
    }

    #[test]
    fn completing_a_set_starts_its_rest_countdown() {
        let (_, c) = loaded();
        let view = c.view();
        let cat = &view.workouts.exercises[0];
        let (ex, set) = (&cat.exercises[0], &cat.exercises[0].sets[0]);
        assert!(c.toggle_set_done(&cat.id, &ex.id, &set.id));
        let rest = c.view().workouts.rest_remaining.expect("countdown runs");
        assert!((89..=90).contains(&rest));
        assert!(c.toggle_set_done(&cat.id, &ex.id, &set.id));
        assert!(c.view().workouts.rest_remaining.is_none());
        assert!(!c.toggle_set_done("missing", "missing", "missing"));
    }

    #[test]
    fn stretch_routines_add_delete_and_play() {
        let (fake, c) = loaded();
        let view = c.view();
        let cat = view.workouts.stretches[0].id.clone();
        assert!(
            !c.add_stretch(&cat, "Calf", "soon"),
            "non-numeric duration refused"
        );
        assert!(!c.add_stretch(&cat, " ", "30"), "blank name refused");
        assert!(c.add_stretch(&cat, "Calf", "30"));
        let item = c.view().workouts.stretches[0]
            .items
            .last()
            .unwrap()
            .id
            .clone();
        assert!(c.delete_stretch(&cat, &item));
        assert!(!c.start_routine("missing"));
        assert!(c.start_routine(&cat));
        assert!(c.view().workouts.runner.is_some());
        c.routine_command("next");
        c.routine_command("previous");
        c.routine_command("stop");
        assert!(c.view().workouts.runner.is_none());
        assert!(fake.calls().contains(&"workouts_save".to_owned()));
    }

    #[test]
    fn measurement_cells_metrics_and_goals_validate_and_save() {
        let (fake, c) = loaded();
        assert!(!c.add_metric("", "cm", ""));
        assert!(!c.add_metric("Waist", "cm", "tall"));
        assert!(
            c.add_metric("Waist", "furlong", "80"),
            "unknown unit falls back to cm"
        );
        let metric = c.view().measurements.metrics[0].clone();
        assert_eq!(metric.unit, "cm");
        assert_eq!(metric.target, "80");

        assert!(c.set_cell(&metric.id, 0, "82.5"));
        assert_eq!(c.view().measurements.metrics[0].cells[0], "82.5");
        assert!(c.set_cell(&metric.id, 1, "tight"));
        assert_eq!(c.view().measurements.metrics[0].cells[1], "tight");
        assert!(c.set_cell(&metric.id, 0, ""));
        assert_eq!(c.view().measurements.metrics[0].cells[0], "");
        assert!(!c.set_cell(&metric.id, 12, "1"), "month out of range");
        assert!(!c.set_cell("missing", 0, "1"));
        assert!(!c.set_cell(&metric.id, 2, "inf"));

        assert!(c.select_year("2025"));
        assert!(
            c.view().measurements.metrics[0]
                .cells
                .iter()
                .all(String::is_empty)
        );
        assert!(!c.select_year("next year"));
        assert!(c.view().measurements.years.contains(&"2025".to_owned()));

        assert!(!c.save_goals("stone", "75", "72", "13", "11"));
        assert!(!c.save_goals("kg", "heavy", "72", "13", "11"));
        assert!(c.save_goals("kg", "75.4", "72", "13.5", "11"));
        assert_eq!(c.view().measurements.goals.weight_gap, "3.4");

        assert!(c.remove_metric(&metric.id));
        assert!(!c.remove_metric(&metric.id));
        assert!(fake.calls().contains(&"measurements_save".to_owned()));
    }

    #[test]
    fn latest_weight_and_fat_readings_sync_the_current_goals() {
        let (_, c) = loaded();
        assert!(c.add_metric("Body weight", "kg", ""));
        assert!(c.add_metric("Body fat", "%", ""));
        let view = c.view().measurements;
        let weight = view.metrics[0].id.clone();
        let fat = view.metrics[1].id.clone();

        assert!(c.set_cell(&weight, 0, "80"));
        assert!(c.set_cell(&weight, 3, "78.5"), "the latest month wins");
        assert!(c.set_cell(&weight, 1, "81"), "an earlier month does not");
        assert_eq!(c.view().measurements.goals.current_weight, "78.5");

        assert!(c.set_cell(&fat, 0, "12.5"));
        assert_eq!(c.view().measurements.goals.current_body_fat, "12.5");

        // Text readings are kept but do not move the goal.
        assert!(c.set_cell(&fat, 5, "lean"));
        assert_eq!(c.view().measurements.goals.current_body_fat, "12.5");

        // Clearing the latest month falls back to the previous numeric reading.
        assert!(c.set_cell(&weight, 3, ""));
        assert_eq!(c.view().measurements.goals.current_weight, "81");
    }

    #[test]
    fn chart_shows_the_chosen_track_by_month() {
        let (_, c) = loaded();
        assert!(c.add_metric("Waist", "cm", ""));
        let waist = c.view().measurements.metrics[0].id.clone();
        assert!(!c.select_chart_metric("missing"));
        assert!(c.select_chart_metric(&waist));
        assert!(c.set_cell(&waist, 0, "82"));
        let chart = c.view().measurements;
        assert_eq!(chart.chart_metric_name, "Waist");
        assert_eq!(chart.chart.len(), 12);
        assert_eq!(chart.chart[0].value, 82.0);
        assert_eq!(chart.chart[0].label, "Jan");
        assert_eq!(chart.chart[1].value, 0.0);
    }

    #[test]
    fn routine_steps_play_cues_and_keep_the_screen_awake_while_running() {
        let (fake, c) = loaded();
        let cat = c.view().workouts.stretches[0].id.clone();
        assert!(c.start_routine(&cat));
        // The first category has three stretches: three skips reach the end.
        c.routine_command("next");
        c.routine_command("next");
        c.routine_command("next");
        c.routine_command("stop");
        // Start is silent; each step change and the finish play one cue.
        let tones = fake.tones.lock().unwrap().clone();
        assert_eq!(tones.len(), 3, "two steps and the finish: {tones:?}");
        assert!(tones.iter().all(|&hz| hz == STEP_CUE.hz));
        assert_eq!(*fake.awake.lock().unwrap(), vec![true, false]);
    }

    #[test]
    fn failed_write_sets_the_error_and_dismiss_clears_it() {
        let (fake, c) = loaded();
        *fake.fail.lock().unwrap() = true;
        assert!(c.add_exercise_category("Legs"));
        assert_eq!(c.view().error.as_deref(), Some("HTTP 500: boom"));
        c.dismiss();
        assert!(c.view().error.is_none());
    }
}
