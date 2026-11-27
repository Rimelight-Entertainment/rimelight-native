//! The Focus page controller (ported from `crates/app/src/focus.rs`, with the web floating tools: the Focus timer, the
//! stretch routine card and the metronome card of `FloatingToolsOverlay.tsx`).
//!
//! Plain Rust with no Slint and no network. [`FocusController`] holds three things: the Focus countdown (the mode, the
//! time left, whether it runs, the sessions count), the stretch routine (steps with a countdown each, started by the
//! health page), and the metronome (its tempo, the beat the dots highlight, whether it clicks).
//!
//! Time comes in through [`FocusController::tick`] as an elapsed duration, so tests drive the clock and the page's timer
//! supplies real time. Sounds go through the [`Sounds`] trait: [`RimelightSound`] plays through `rimelight_sound`, and
//! the tests inject a recorder.
//!
//! Threading: the controller locks its state only around reads and writes. The `notify` callback is called after the
//! lock is released, on whichever thread changed the state. The notify callback must hand the view to the UI thread
//! itself (the glue uses `Weak::upgrade_in_event_loop`).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rimelight_sound::{Metronome, Tone, beat_period};

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@rimelight_personal_focus`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

const SECOND: Duration = Duration::from_secs(1);

/// The tempo range of the metronome card (the web slider's bounds), in beats per minute.
pub const BPM_MIN: u32 = 40;
pub const BPM_MAX: u32 = 240;
/// The tempo a fresh metronome starts at (the web's initial `bpm`).
pub const BPM_DEFAULT: u32 = 120;
/// Beats in the metronome's bar: the card's four dots. Beat 0 is accented.
pub const BEATS: u32 = 4;
/// The cue played when a focus phase or a stretch step ends (the web's alert: 880 Hz for half a second).
pub const CUE: Tone = Tone {
    hz: 880.0,
    ms: 500,
    volume: 0.3,
};

/// The sound output the page needs. [`RimelightSound`] is the real one; tests inject a recorder.
pub trait Sounds: Send + Sync {
    /// Plays a one-off cue. It returns at once.
    fn cue(&self, tone: Tone);
    /// Starts clicking at `bpm`. Dropping the returned guard stops the clicks.
    fn start_metronome(&self, bpm: u32) -> Box<dyn Send>;
}

/// The sound output on the default device, through `rimelight_sound`.
pub struct RimelightSound;

impl Sounds for RimelightSound {
    fn cue(&self, tone: Tone) {
        rimelight_sound::play(tone);
    }

    fn start_metronome(&self, bpm: u32) -> Box<dyn Send> {
        Box::new(Metronome::start(bpm, BEATS))
    }
}

/// One of the three presets. The durations are Solid's `focusPresets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Work,
    ShortBreak,
    LongBreak,
}

impl Mode {
    /// The length of a full phase, in seconds.
    pub fn seconds(self) -> u32 {
        match self {
            Mode::Work => 25 * 60,
            Mode::ShortBreak => 5 * 60,
            Mode::LongBreak => 15 * 60,
        }
    }
}

/// One step of the stretch routine: its category, its name and how long it lasts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stretch {
    pub category: String,
    pub name: String,
    pub seconds: u32,
}

/// The stretch card: the current step and its countdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StretchView {
    pub category: String,
    pub name: String,
    /// Zero-based index of the current step.
    pub index: usize,
    pub total: usize,
    pub seconds_left: u32,
    pub seconds_total: u32,
    pub paused: bool,
}

impl StretchView {
    /// The step's time left as a percentage of its length, for the card's bar (100 at the start, 0 at the end).
    pub fn percent_left(&self) -> f32 {
        if self.seconds_total == 0 {
            return 0.0;
        }
        self.seconds_left as f32 / self.seconds_total as f32 * 100.0
    }
}

/// The metronome card: the tempo, whether it clicks, and the beat the dots highlight (0 to 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetronomeView {
    pub running: bool,
    pub bpm: u32,
    pub beat: u32,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusView {
    pub mode: Mode,
    pub time_left: u32,
    /// `MM:SS`, the big number on the card.
    pub time_text: String,
    pub running: bool,
    /// Work sessions completed since the app started.
    pub sessions: u32,
    pub metronome: MetronomeView,
    /// The stretch card, present while a routine runs.
    pub stretch: Option<StretchView>,
}

struct State {
    mode: Mode,
    time_left: u32,
    running: bool,
    sessions: u32,
    /// Sub-second time carried between ticks, so a tick shorter than a second still counts.
    carry: Duration,

    stretches: Vec<Stretch>,
    stretch_index: usize,
    stretch_left: u32,
    stretch_paused: bool,
    stretch_carry: Duration,

    bpm: u32,
    /// The running click, if any. Dropping it stops the clicks.
    metronome: Option<Box<dyn Send>>,
    beat: u32,
    beat_carry: Duration,

    sounds: Arc<dyn Sounds>,
}

impl State {
    fn new(sounds: Arc<dyn Sounds>) -> Self {
        Self {
            mode: Mode::Work,
            time_left: Mode::Work.seconds(),
            running: false,
            sessions: 0,
            carry: Duration::ZERO,
            stretches: Vec::new(),
            stretch_index: 0,
            stretch_left: 0,
            stretch_paused: false,
            stretch_carry: Duration::ZERO,
            bpm: BPM_DEFAULT,
            metronome: None,
            beat: 0,
            beat_carry: Duration::ZERO,
            sounds,
        }
    }

    fn view(&self) -> FocusView {
        FocusView {
            mode: self.mode,
            time_left: self.time_left,
            time_text: format_time(self.time_left),
            running: self.running,
            sessions: self.sessions,
            metronome: MetronomeView {
                running: self.metronome.is_some(),
                bpm: self.bpm,
                beat: self.beat,
            },
            stretch: self.stretch_view(),
        }
    }

    fn stretch_view(&self) -> Option<StretchView> {
        let step = self.stretches.get(self.stretch_index)?;
        Some(StretchView {
            category: step.category.clone(),
            name: step.name.clone(),
            index: self.stretch_index,
            total: self.stretches.len(),
            seconds_left: self.stretch_left,
            seconds_total: step.seconds,
            paused: self.stretch_paused,
        })
    }

    /// One second of the countdown. At one second left the phase completes instead of reaching zero: a finished work
    /// phase counts a session and moves to a break (a long one every fourth session), and a finished break moves back
    /// to work. The timer stops at the end of each phase, as Solid's interval did, and a cue plays.
    fn step_focus(&mut self) {
        if self.time_left > 1 {
            self.time_left -= 1;
            return;
        }
        self.running = false;
        self.carry = Duration::ZERO;
        if self.mode == Mode::Work {
            self.sessions += 1;
            self.mode = if self.sessions.is_multiple_of(4) {
                Mode::LongBreak
            } else {
                Mode::ShortBreak
            };
        } else {
            self.mode = Mode::Work;
        }
        self.time_left = self.mode.seconds();
        self.sounds.cue(CUE);
    }

    /// One second of the stretch step. At one second left the next step starts; after the last step the routine ends.
    fn step_stretch(&mut self) {
        if self.stretch_left > 1 {
            self.stretch_left -= 1;
            return;
        }
        if self.stretch_index + 1 < self.stretches.len() {
            self.stretch_index += 1;
            self.stretch_left = self.stretches[self.stretch_index].seconds;
            self.sounds.cue(CUE);
        } else {
            self.clear_stretches();
        }
    }

    fn clear_stretches(&mut self) {
        self.stretches.clear();
        self.stretch_index = 0;
        self.stretch_left = 0;
        self.stretch_paused = false;
        self.stretch_carry = Duration::ZERO;
    }
}

type Notify = Arc<dyn Fn(FocusView) + Send + Sync>;

struct Shared {
    state: Mutex<State>,
    notify: Notify,
}

/// The Focus page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct FocusController {
    shared: Arc<Shared>,
}

impl FocusController {
    /// `notify` receives the new view after every change of state, possibly on the thread that made the change. Sounds
    /// play on the default device.
    pub fn new(notify: impl Fn(FocusView) + Send + Sync + 'static) -> Self {
        Self::with_sounds(notify, Arc::new(RimelightSound))
    }

    /// As [`FocusController::new`], with the sound output given (tests pass a recorder).
    pub fn with_sounds(
        notify: impl Fn(FocusView) + Send + Sync + 'static,
        sounds: Arc<dyn Sounds>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::new(sounds)),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The view as it is now.
    pub fn view(&self) -> FocusView {
        self.lock().view()
    }

    /// Switches to `mode` at its full length and stops the timer (the card's Work, Short and Long buttons).
    pub fn select_mode(&self, mode: Mode) {
        self.change(|state| {
            state.running = false;
            state.carry = Duration::ZERO;
            state.mode = mode;
            state.time_left = mode.seconds();
            true
        });
    }

    /// Starts the timer, or pauses it when it runs.
    pub fn toggle(&self) {
        self.change(|state| {
            state.running = !state.running;
            state.carry = Duration::ZERO;
            true
        });
    }

    /// Stops the timer and puts the current mode back to its full length.
    pub fn reset(&self) {
        self.change(|state| {
            state.running = false;
            state.carry = Duration::ZERO;
            state.time_left = state.mode.seconds();
            true
        });
    }

    /// Starts a stretch routine at its first step, not paused. An empty list ends any routine.
    pub fn start_stretches(&self, steps: Vec<Stretch>) {
        self.change(|state| {
            // An empty routine over no routine is no change: nothing to clear and nothing to start.
            if steps.is_empty() && state.stretches.is_empty() {
                return false;
            }
            state.clear_stretches();
            if let Some(seconds) = steps.first().map(|step| step.seconds) {
                state.stretch_left = seconds;
                state.stretches = steps;
            }
            true
        });
    }

    /// Pauses the stretch countdown. Nothing happens without a routine or when it is already paused.
    pub fn pause_stretches(&self) {
        self.change(|state| {
            if state.stretches.is_empty() || state.stretch_paused {
                return false;
            }
            state.stretch_paused = true;
            true
        });
    }

    /// Resumes a paused stretch countdown.
    pub fn resume_stretches(&self) {
        self.change(|state| {
            if state.stretches.is_empty() || !state.stretch_paused {
                return false;
            }
            state.stretch_paused = false;
            true
        });
    }

    /// Ends the stretch routine (the card's close and Stop buttons).
    pub fn stop_stretches(&self) {
        self.change(|state| {
            if state.stretches.is_empty() {
                return false;
            }
            state.clear_stretches();
            true
        });
    }

    /// Sets the tempo, clamped to the card's range. A running click keeps the tempo it started with, as the web
    /// metronome's interval did.
    pub fn set_bpm(&self, bpm: u32) {
        self.change(|state| {
            let bpm = bpm.clamp(BPM_MIN, BPM_MAX);
            if state.bpm == bpm {
                return false;
            }
            state.bpm = bpm;
            true
        });
    }

    /// Starts the click at the current tempo, or stops it when it runs.
    pub fn toggle_metronome(&self) {
        self.change(|state| {
            state.beat = 0;
            state.beat_carry = Duration::ZERO;
            if state.metronome.is_some() {
                state.metronome = None;
            } else {
                state.metronome = Some(state.sounds.start_metronome(state.bpm));
            }
            true
        });
    }

    /// Advances the timer, the stretch routine and the metronome's beat by `elapsed`. Stopped parts ignore it. Notifies
    /// only when the display changed.
    pub fn tick(&self, elapsed: Duration) {
        self.change(|state| {
            let mut changed = false;
            if state.running {
                state.carry += elapsed;
                while state.carry >= SECOND && state.running {
                    state.carry -= SECOND;
                    state.step_focus();
                    changed = true;
                }
            }
            if !state.stretches.is_empty() && !state.stretch_paused {
                state.stretch_carry += elapsed;
                while state.stretch_carry >= SECOND && !state.stretches.is_empty() {
                    state.stretch_carry -= SECOND;
                    state.step_stretch();
                    changed = true;
                }
            }
            if state.metronome.is_some() {
                state.beat_carry += elapsed;
                let period = beat_period(state.bpm);
                while state.beat_carry >= period {
                    state.beat_carry -= period;
                    state.beat = (state.beat + 1) % BEATS;
                    changed = true;
                }
            }
            changed
        });
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves plain data behind, which is still valid to read.
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Applies `change` under the lock. When it reports a change, the new view goes to `notify` after the lock is released.
    fn change(&self, change: impl FnOnce(&mut State) -> bool) {
        let view = {
            let mut state = self.lock();
            if !change(&mut state) {
                return;
            }
            state.view()
        };
        (self.shared.notify)(view);
    }
}

/// `MM:SS`, as the egui card wrote the countdown.
fn format_time(total: u32) -> String {
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Records cues and metronome starts and stops. No audio device.
    #[derive(Default)]
    struct FakeSounds {
        cues: AtomicUsize,
        starts: Mutex<Vec<u32>>,
        stops: Arc<AtomicUsize>,
    }

    /// Stands for a running click: dropping it counts a stop.
    struct FakeClick(Arc<AtomicUsize>);

    impl Drop for FakeClick {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Sounds for FakeSounds {
        fn cue(&self, _tone: Tone) {
            self.cues.fetch_add(1, Ordering::SeqCst);
        }

        fn start_metronome(&self, bpm: u32) -> Box<dyn Send> {
            self.starts.lock().unwrap().push(bpm);
            Box::new(FakeClick(self.stops.clone()))
        }
    }

    type Seen = Arc<Mutex<Vec<FocusView>>>;

    /// A controller whose notifications are recorded, and whose sounds are a fake.
    fn controller() -> (FocusController, Seen, Arc<FakeSounds>) {
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let sounds = Arc::new(FakeSounds::default());
        let c = FocusController::with_sounds(
            move |view| sink.lock().unwrap().push(view),
            sounds.clone(),
        );
        (c, seen, sounds)
    }

    /// Runs the current phase to its end: starts the timer and ticks through the time left.
    fn finish_phase(c: &FocusController) {
        if !c.view().running {
            c.toggle();
        }
        c.tick(Duration::from_secs(u64::from(c.view().time_left)));
    }

    fn step(name: &str, seconds: u32) -> Stretch {
        Stretch {
            category: "Legs".into(),
            name: name.into(),
            seconds,
        }
    }

    #[test]
    fn starts_as_a_stopped_25_minute_work_phase() {
        let (c, seen, _) = controller();
        let view = c.view();
        assert_eq!(view.mode, Mode::Work);
        assert_eq!(view.time_text, "25:00");
        assert!(!view.running);
        assert_eq!(view.sessions, 0);
        assert_eq!(
            view.metronome,
            MetronomeView {
                running: false,
                bpm: BPM_DEFAULT,
                beat: 0
            }
        );
        assert!(view.stretch.is_none());
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_stopped_timer_ignores_ticks() {
        let (c, seen, _) = controller();
        c.tick(Duration::from_secs(90));
        assert_eq!(c.view().time_left, 25 * 60);
        assert!(
            seen.lock().unwrap().is_empty(),
            "an idle tick notifies nothing"
        );
    }

    #[test]
    fn toggle_runs_the_clock_and_pauses_it() {
        let (c, _, _) = controller();
        c.toggle();
        assert!(c.view().running);
        c.tick(Duration::from_secs(3));
        assert_eq!(c.view().time_text, "24:57");
        c.toggle();
        c.tick(Duration::from_secs(10));
        assert_eq!(c.view().time_text, "24:57", "a paused timer does not move");
    }

    #[test]
    fn sub_second_ticks_add_up() {
        let (c, _, _) = controller();
        c.toggle();
        c.tick(Duration::from_millis(600));
        assert_eq!(c.view().time_left, 25 * 60, "no whole second yet");
        c.tick(Duration::from_millis(600));
        assert_eq!(c.view().time_left, 25 * 60 - 1);
    }

    #[test]
    fn the_work_phase_completes_into_a_short_break_with_a_cue_and_stops() {
        let (c, _, sounds) = controller();
        c.toggle();
        c.tick(Duration::from_secs(25 * 60 - 1));
        assert_eq!(c.view().time_left, 1);
        assert!(c.view().running);
        assert_eq!(sounds.cues.load(Ordering::SeqCst), 0);

        // At one second left the phase completes, instead of reaching zero.
        c.tick(Duration::from_secs(1));
        let view = c.view();
        assert_eq!(view.mode, Mode::ShortBreak);
        assert_eq!(view.time_text, "05:00");
        assert_eq!(view.sessions, 1);
        assert!(!view.running);
        assert_eq!(sounds.cues.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn every_fourth_session_gives_a_long_break() {
        let (c, _, _) = controller();
        // Work, short break, work, short break, work, short break: three sessions.
        for _ in 0..3 {
            finish_phase(&c);
            assert_eq!(c.view().mode, Mode::ShortBreak);
            finish_phase(&c);
            assert_eq!(c.view().mode, Mode::Work);
        }
        assert_eq!(c.view().sessions, 3);
        // The fourth work session ends in a long break.
        finish_phase(&c);
        let view = c.view();
        assert_eq!(view.mode, Mode::LongBreak);
        assert_eq!(view.time_text, "15:00");
        assert_eq!(view.sessions, 4);
    }

    #[test]
    fn a_finished_break_returns_to_work_at_full_length() {
        let (c, _, _) = controller();
        c.select_mode(Mode::ShortBreak);
        finish_phase(&c);
        let view = c.view();
        assert_eq!(view.mode, Mode::Work);
        assert_eq!(view.time_text, "25:00");
        assert_eq!(view.sessions, 0, "a break is not a session");
    }

    #[test]
    fn selecting_a_mode_stops_the_timer_and_shows_its_length() {
        let (c, _, _) = controller();
        c.toggle();
        c.tick(Duration::from_secs(60));
        c.select_mode(Mode::LongBreak);
        let view = c.view();
        assert_eq!(view.mode, Mode::LongBreak);
        assert_eq!(view.time_text, "15:00");
        assert!(!view.running);
    }

    #[test]
    fn reset_restores_the_current_mode_and_stops() {
        let (c, _, _) = controller();
        c.select_mode(Mode::ShortBreak);
        c.toggle();
        c.tick(Duration::from_secs(100));
        c.reset();
        let view = c.view();
        assert_eq!(view.mode, Mode::ShortBreak, "reset keeps the mode");
        assert_eq!(view.time_text, "05:00");
        assert!(!view.running);
    }

    #[test]
    fn stretch_routine_counts_each_step_and_ends_after_the_last() {
        let (c, _, sounds) = controller();
        c.start_stretches(vec![step("Hamstring", 2), step("Quad", 2)]);
        let view = c.view().stretch.expect("a routine is running");
        assert_eq!((view.index, view.total, view.seconds_left), (0, 2, 2));
        assert_eq!(view.name, "Hamstring");
        assert!(!view.paused);

        c.tick(Duration::from_secs(1));
        assert_eq!(c.view().stretch.unwrap().seconds_left, 1);
        // One second left: the next step starts.
        c.tick(Duration::from_secs(1));
        let view = c.view().stretch.unwrap();
        assert_eq!((view.index, view.name.as_str()), (1, "Quad"));
        assert_eq!(view.seconds_left, 2);
        assert_eq!(sounds.cues.load(Ordering::SeqCst), 1);

        c.tick(Duration::from_secs(2));
        assert!(
            c.view().stretch.is_none(),
            "the routine ends after its last step"
        );
    }

    #[test]
    fn stretch_pause_resume_and_stop() {
        let (c, _, _) = controller();
        c.start_stretches(vec![step("Calf", 30)]);
        c.pause_stretches();
        c.tick(Duration::from_secs(10));
        let view = c.view().stretch.unwrap();
        assert!(view.paused);
        assert_eq!(view.seconds_left, 30, "a paused routine does not count");
        c.resume_stretches();
        c.tick(Duration::from_secs(10));
        assert_eq!(c.view().stretch.unwrap().seconds_left, 20);
        c.stop_stretches();
        assert!(c.view().stretch.is_none());
    }

    #[test]
    fn an_empty_routine_and_stop_without_a_routine_change_nothing() {
        let (c, seen, _) = controller();
        c.start_stretches(Vec::new());
        c.stop_stretches();
        c.pause_stretches();
        c.resume_stretches();
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn percent_left_runs_from_full_to_empty() {
        let view = StretchView {
            category: String::new(),
            name: String::new(),
            index: 0,
            total: 1,
            seconds_left: 15,
            seconds_total: 30,
            paused: false,
        };
        assert!((view.percent_left() - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn metronome_starts_clicks_at_the_tempo_and_stops_them() {
        let (c, _, sounds) = controller();
        c.set_bpm(90);
        c.toggle_metronome();
        assert_eq!(*sounds.starts.lock().unwrap(), vec![90]);
        assert!(c.view().metronome.running);
        c.toggle_metronome();
        assert!(!c.view().metronome.running);
        assert_eq!(
            sounds.stops.load(Ordering::SeqCst),
            1,
            "stopping drops the click"
        );
    }

    #[test]
    fn metronome_beat_cycles_through_four_dots() {
        let (c, _, _) = controller();
        c.set_bpm(60);
        c.toggle_metronome();
        // 60 bpm: one beat a second.
        c.tick(Duration::from_secs(1));
        assert_eq!(c.view().metronome.beat, 1);
        c.tick(Duration::from_secs(3));
        assert_eq!(
            c.view().metronome.beat,
            0,
            "four beats wrap to the first dot"
        );
    }

    #[test]
    fn bpm_is_clamped_and_a_running_click_keeps_its_tempo() {
        let (c, _, sounds) = controller();
        c.toggle_metronome();
        c.set_bpm(500);
        assert_eq!(c.view().metronome.bpm, BPM_MAX);
        c.set_bpm(10);
        assert_eq!(c.view().metronome.bpm, BPM_MIN);
        assert_eq!(*sounds.starts.lock().unwrap(), vec![BPM_DEFAULT]);
    }

    #[test]
    fn each_change_notifies_the_new_view() {
        let (c, seen, _) = controller();
        c.toggle();
        c.tick(Duration::from_secs(1));
        c.reset();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen.last(), Some(&c.view()));
        assert!(seen[0].running);
        assert_eq!(seen[1].time_left, 25 * 60 - 1);
    }
}
