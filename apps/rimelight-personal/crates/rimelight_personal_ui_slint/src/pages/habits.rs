//! Glue between the Habits controller (`rimelight_personal_feature_habits`) and the HabitsState global of `ui/pages/habits.slint`.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback here posts the view to the Slint event loop, and only the event loop touches the window.

use std::sync::Arc;

use rimelight_personal_feature_habits::{DayView, HabitApi, HabitsController, HabitsView, MONTHS};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::generated::{
    HabitCategoryRow, HabitDay, HabitRow, HabitWeek, HabitYearTab, HabitsState,
    RimelightPersonalShell, SelectOption,
};

/// The grids show this many day cells per line.
const DAYS_PER_LINE: usize = 10;

/// The Habits page of the shell: one controller, wired to the HabitsState global.
pub struct Habits {
    controller: HabitsController,
}

impl Habits {
    /// Connects `ui` to a controller that calls `api`. `today` is `(year, month, day)`, as [`rimelight_personal_feature_habits::today`]
    /// returns it. Nothing is fetched until [`Habits::activate`].
    pub fn new(
        ui: &RimelightPersonalShell,
        api: Arc<dyn HabitApi>,
        today: (i32, u32, u32),
    ) -> Self {
        let month_options: Vec<SelectOption> = MONTHS
            .iter()
            .enumerate()
            .map(|(i, name)| SelectOption {
                value: (i + 1).to_string().into(),
                label: (*name).into(),
                icon: Default::default(),
                disabled: false,
                group: "".into(),
                ..Default::default()
            })
            .collect();
        ui.global::<HabitsState>()
            .set_month_options(ModelRc::new(VecModel::from(month_options)));

        let weak = ui.as_weak();
        let controller = HabitsController::new(api, today, move |view: HabitsView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Habits route opens: the tracker is fetched again.
    pub fn activate(&self) {
        self.controller.refresh();
    }

    /// The controller, for tests.
    #[cfg(test)]
    pub fn controller(&self) -> &HabitsController {
        &self.controller
    }
}

/// Fills the HabitsState properties from a view. Every model is built anew, so each control starts from the server's
/// state.
fn show_view(ui: &RimelightPersonalShell, view: &HabitsView) {
    let state = ui.global::<HabitsState>();
    let years: Vec<HabitYearTab> = view
        .years
        .iter()
        .map(|year| HabitYearTab {
            year: *year,
            label: year.to_string().into(),
            selected: view.selected_year == Some(*year),
        })
        .collect();
    state.set_years(ModelRc::new(VecModel::from(years)));
    state.set_month(view.selected_month.to_string().into());
    state.set_has_year(view.has_year);
    state.set_year_label(
        view.selected_year
            .map(|year| year.to_string())
            .unwrap_or_default()
            .into(),
    );
    state.set_streaks_mode(view.streaks_mode);

    let categories: Vec<HabitCategoryRow> = view
        .categories
        .iter()
        .map(|cat| HabitCategoryRow {
            id: cat.id.as_str().into(),
            name: cat.name.as_str().into(),
            habits: ModelRc::new(VecModel::from(
                cat.habits
                    .iter()
                    .map(|habit| HabitRow {
                        id: habit.id.as_str().into(),
                        name: habit.name.as_str().into(),
                        weeks: weeks(&habit.days),
                        current_streak: habit.current_streak as i32,
                        best_streak: habit.best_streak as i32,
                        month_percent: habit.month_percent as i32,
                        year_completed: habit.year_completed as i32,
                        year_percent: habit.year_percent as i32,
                        year_weeks: weeks(&habit.year_cells),
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    state.set_categories(ModelRc::new(VecModel::from(categories)));
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
    state.set_pending_delete(
        view.pending_delete_category
            .clone()
            .unwrap_or_default()
            .into(),
    );
}

/// The day cells, cut into lines of [`DAYS_PER_LINE`].
fn weeks(days: &[DayView]) -> ModelRc<HabitWeek> {
    let lines: Vec<HabitWeek> = days
        .chunks(DAYS_PER_LINE)
        .map(|line| HabitWeek {
            days: ModelRc::new(VecModel::from(
                line.iter()
                    .map(|day| HabitDay {
                        date: day.date.as_str().into(),
                        label: day.day.to_string().into(),
                        done: day.done,
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    ModelRc::new(VecModel::from(lines))
}

/// Connects the HabitsState callbacks to the controller. The year and category fields are cleared only when the
/// controller accepts their text, as the egui page did.
fn wire(ui: &RimelightPersonalShell, controller: &HabitsController) {
    let state = ui.global::<HabitsState>();

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_year_requested(move |text| {
        if c.add_year(&text)
            && let Some(ui) = weak.upgrade()
        {
            ui.global::<HabitsState>().set_new_year("".into());
        }
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_category_requested(move |name| {
        if c.add_category(&name)
            && let Some(ui) = weak.upgrade()
        {
            ui.global::<HabitsState>().set_new_category("".into());
        }
    });

    let c = controller.clone();
    state.on_year_requested(move |year| c.select_year(year));

    let c = controller.clone();
    state.on_month_requested(move |month| {
        if let Ok(month) = month.parse::<u32>() {
            c.select_month(month);
        }
    });

    let c = controller.clone();
    state.on_streaks_mode_requested(move |on| c.set_streaks_mode(on));

    let c = controller.clone();
    state.on_delete_category_requested(move |cat_id| c.request_delete_category(&cat_id));

    let c = controller.clone();
    state.on_confirm_delete_category_requested(move || c.confirm_delete_category());

    let c = controller.clone();
    state.on_cancel_delete_category_requested(move || c.cancel_delete_category());

    let c = controller.clone();
    state.on_add_habit_requested(move |cat_id, name| {
        c.add_habit(&cat_id, &name);
    });

    let c = controller.clone();
    state.on_delete_habit_requested(move |cat_id, habit_id| c.delete_habit(&cat_id, &habit_id));

    let c = controller.clone();
    state.on_toggle_day_requested(move |cat_id, habit_id, date| {
        c.toggle_day(&cat_id, &habit_id, &date)
    });

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use rimelight_personal_core::habits::HabitData;
    use rimelight_personal_core::{ApiError, HabitAction};
    use slint::Model;

    use super::*;

    /// A server that is down: every call is recorded and answers with an error.
    #[derive(Default)]
    struct Down {
        calls: Mutex<Vec<&'static str>>,
    }

    impl HabitApi for Down {
        fn habits(&self, on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>) {
            self.calls.lock().unwrap().push("habits");
            on_done(Err(ApiError::Transport("offline".to_owned())));
        }

        fn act(
            &self,
            _action: &HabitAction<'_>,
            on_done: Box<dyn FnOnce(Result<HabitData, ApiError>) + Send>,
        ) {
            self.calls.lock().unwrap().push("act");
            on_done(Err(ApiError::Transport("offline".to_owned())));
        }
    }

    /// Run by `pages::tests::page_scenarios`, on the thread that installed the testing backend. The month list is filled
    /// at once, and a failed load shows its error and stops the loading flag.
    pub(crate) fn scenario() {
        let ui = RimelightPersonalShell::new().expect("create the shell");
        let api = Arc::new(Down::default());
        let habits = Habits::new(&ui, api.clone(), (2026, 11, 22));
        let state = ui.global::<HabitsState>();
        assert_eq!(state.get_month_options().row_count(), 12);

        habits.activate();
        assert_eq!(api.calls.lock().unwrap().as_slice(), ["habits"]);
        show_view(&ui, &habits.controller().view());
        assert!(!state.get_loading());
        assert!(!state.get_error().is_empty());
    }
}
