//! Glue between the Focus timer controller (`rimelight_personal_feature_focus`) and the FocusState global of `ui/pages/focus.slint`.
//!
//! The countdown is driven by a `slint::Timer` here: each tick reports the real time that passed, and the controller
//! advances by that much. The notify callback posts the view to the event loop, as the Todos glue does.

use std::time::{Duration, Instant};

use rimelight_personal_feature_focus::{FocusController, FocusView, Mode};
use slint::{ComponentHandle as _, Timer, TimerMode};

use crate::generated::{FocusMode, FocusState, RimelightPersonalShell};

/// How often the timer wakes to advance the countdown. The elapsed time is measured from the clock, so this only sets
/// how often the card redraws.
const TICK: Duration = Duration::from_millis(200);

/// The Focus card: one controller, its countdown timer, and the FocusState global.
pub struct Focus {
    controller: FocusController,
    /// Dropping the timer stops the countdown. It runs while the window is open, whether or not the card is shown.
    _timer: Timer,
}

impl Focus {
    /// Connects `ui` to a new controller, starts the countdown and shows the starting view.
    pub fn new(ui: &RimelightPersonalShell) -> Self {
        let weak = ui.as_weak();
        let controller = FocusController::new(move |view: FocusView| {
            // Runs on whichever thread changed the state. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        show_view(ui, &controller.view());

        let timer = Timer::default();
        let clock = controller.clone();
        let mut last = Instant::now();
        timer.start(TimerMode::Repeated, TICK, move || {
            let now = Instant::now();
            clock.tick(now.duration_since(last));
            last = now;
        });
        Self {
            controller,
            _timer: timer,
        }
    }

    /// The controller, for the shell and for tests.
    pub fn controller(&self) -> &FocusController {
        &self.controller
    }
}

/// Fills the FocusState properties from a view.
fn show_view(ui: &RimelightPersonalShell, view: &FocusView) {
    let state = ui.global::<FocusState>();
    state.set_mode(focus_mode(view.mode));
    state.set_time_text(view.time_text.as_str().into());
    state.set_running(view.running);
    state.set_sessions(i32::try_from(view.sessions).unwrap_or(i32::MAX));
    state.set_bpm(i32::try_from(view.metronome.bpm).unwrap_or(i32::MAX));
    state.set_metronome_running(view.metronome.running);
    state.set_beat(i32::try_from(view.metronome.beat).unwrap_or(0));
    match &view.stretch {
        Some(stretch) => {
            state.set_stretch_visible(true);
            state.set_stretch_category(stretch.category.as_str().into());
            state.set_stretch_name(stretch.name.as_str().into());
            state.set_stretch_index(i32::try_from(stretch.index).unwrap_or(0));
            state.set_stretch_total(i32::try_from(stretch.total).unwrap_or(0));
            state.set_stretch_seconds_left(i32::try_from(stretch.seconds_left).unwrap_or(0));
            state.set_stretch_progress(stretch.percent_left());
            state.set_stretch_paused(stretch.paused);
        }
        None => state.set_stretch_visible(false),
    }
}

/// Connects the FocusState callbacks to the controller.
fn wire(ui: &RimelightPersonalShell, controller: &FocusController) {
    let state = ui.global::<FocusState>();

    let c = controller.clone();
    state.on_mode_requested(move |mode| c.select_mode(mode_of(mode)));

    let c = controller.clone();
    state.on_toggle_requested(move || c.toggle());

    let c = controller.clone();
    state.on_reset_requested(move || c.reset());

    let c = controller.clone();
    state.on_bpm_changed(move |bpm| c.set_bpm(bpm.max(0) as u32));

    let c = controller.clone();
    state.on_metronome_toggle_requested(move || c.toggle_metronome());

    let c = controller.clone();
    state.on_stretch_pause_requested(move || c.pause_stretches());

    let c = controller.clone();
    state.on_stretch_resume_requested(move || c.resume_stretches());

    let c = controller.clone();
    state.on_stretch_stop_requested(move || c.stop_stretches());
}

fn focus_mode(mode: Mode) -> FocusMode {
    match mode {
        Mode::Work => FocusMode::Work,
        Mode::ShortBreak => FocusMode::Short,
        Mode::LongBreak => FocusMode::Long,
    }
}

fn mode_of(mode: FocusMode) -> Mode {
    match mode {
        FocusMode::Work => Mode::Work,
        FocusMode::Short => Mode::ShortBreak,
        FocusMode::Long => Mode::LongBreak,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The card's controller is reachable from the glue, and a preset switch is what the card then shows.
    /// Run by `pages::tests::page_scenarios`, on the thread that installed the testing backend.
    pub(crate) fn scenario() {
        let ui = RimelightPersonalShell::new().expect("create the shell");
        let focus = Focus::new(&ui);
        focus.controller().select_mode(Mode::ShortBreak);
        show_view(&ui, &focus.controller().view());
        assert_eq!(ui.global::<FocusState>().get_mode(), FocusMode::Short);
        assert_eq!(ui.global::<FocusState>().get_time_text().as_str(), "05:00");
        assert!(!ui.global::<FocusState>().get_stretch_visible());
    }
}
