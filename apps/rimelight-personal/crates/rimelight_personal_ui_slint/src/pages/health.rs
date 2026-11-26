//! Glue between the Health controller (`rimelight_personal_feature_health`) and the shell's `HealthRoute`.
//!
//! The controller answers on a worker thread and reports each change through `notify`, which posts the view to the
//! event loop. Starting a stretch routine also tells the shell (`on_routine_started`) with the routine's steps in the
//! Focus crate's type, so the shell can pass them to the Focus page's `start_stretches`. This glue does not import the
//! Focus glue; the caller (the shell) passes the hook in.

use std::sync::Arc;

use rimelight_personal_feature_focus::Stretch;
use rimelight_personal_feature_health::{
    Feedback, HealthApi, HealthController, HealthView, SetField, StretchCategoryView,
};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::RimelightPersonalShell;
use crate::generated::{
    CalendarDay, ChartPoint, HealthExerciseDay, HealthExerciseRow, HealthHistoryBar,
    HealthMetricRow, HealthRoutine, HealthSetRow, HealthStageRow, HealthState, HealthStretchRow,
};

pub struct Health {
    controller: HealthController,
    ui: slint::Weak<RimelightPersonalShell>,
}

impl Health {
    /// Connects `ui` to a controller that calls `api`. `on_routine_started` runs when a stretch routine actually starts,
    /// with its steps in order, so the shell can pass them to the Focus page's `start_stretches`. Nothing is fetched
    /// until [`Health::activate`].
    pub fn new(
        ui: &RimelightPersonalShell,
        api: Arc<dyn HealthApi>,
        feedback: Arc<dyn Feedback>,
        on_routine_started: impl Fn(Vec<Stretch>) + 'static,
    ) -> Self {
        let weak = ui.as_weak();
        let notify_weak = weak.clone();
        let controller = HealthController::new(api, feedback, move |view: HealthView| {
            let _ = notify_weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller, on_routine_started);
        Self {
            controller,
            ui: weak,
        }
    }

    /// The shell calls this when a health route opens. `/health/workouts` shows the workouts section,
    /// `/health/measurements` the measurements, anything else (`/health`, `/health/fasting`) the fasting section. The
    /// section is set on the global here, because the feature's view does not carry it.
    pub fn open(&self, route: &str) {
        let section = match route {
            "/health/workouts" => 1,
            "/health/measurements" => 2,
            _ => 0,
        };
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<HealthState>().set_section(section);
        }
    }

    /// The shell calls this when the Health route is shown: every section is loaded.
    pub fn activate(&self) {
        self.controller.load_all();
    }

    /// Advances the running fast and the rest countdown. The shell calls this on a timer.
    pub fn tick(&self) {
        self.controller.tick();
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn text(value: &str) -> SharedString {
    SharedString::from(value)
}

fn show_view(ui: &RimelightPersonalShell, view: &HealthView) {
    let state = ui.global::<HealthState>();
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
    state.set_notice(view.notice.clone().unwrap_or_default().into());
    show_fasting(&state, view);
    show_workouts(&state, view);
    show_measurements(&state, view);
}

fn show_fasting(state: &HealthState<'_>, view: &HealthView) {
    let f = &view.fasting;
    state.set_protocols(model(f.protocols.iter().map(|p| text(p)).collect()));
    state.set_protocol(f.protocol as i32);
    state.set_custom_hours(text(&f.custom_hours));
    state.set_custom_start_on(f.custom_start_on);
    state.set_start_days(model(
        f.start_cells
            .iter()
            .map(|c| CalendarDay {
                day: c.day as i32,
                in_month: c.in_month,
                today: c.today,
                selected: c.selected,
                ..Default::default()
            })
            .collect(),
    ));
    state.set_weekday_labels(model(f.weekday_labels.iter().map(|w| text(w)).collect()));
    state.set_start_title(text(&f.start_title));
    state.set_start_label(text(&f.start_label));
    state.set_start_hour(f.start_hour as i32);
    state.set_start_minute(f.start_minute as i32);
    match &f.active {
        Some(active) => {
            state.set_has_active(true);
            state.set_active_id(text(&active.id));
            state.set_active_started(text(&active.started));
            state.set_active_elapsed(text(&active.elapsed));
            state.set_active_goal(text(&active.goal_label));
            state.set_active_progress(active.progress);
            state.set_active_stage(text(&active.stage_name));
            state.set_active_stage_description(text(&active.stage_description));
        }
        None => {
            state.set_has_active(false);
            state.set_active_id(SharedString::new());
        }
    }
    state.set_stages(model(
        f.stages
            .iter()
            .map(|s| HealthStageRow {
                name: text(&s.name),
                description: text(&s.description),
                reached: s.reached,
                current: s.current,
            })
            .collect(),
    ));
    state.set_history(model(
        f.history
            .iter()
            .map(|h| HealthHistoryBar {
                label: text(&h.label),
                hours: h.hours,
                tone: i32::from(h.tone),
            })
            .collect(),
    ));
}

fn show_workouts(state: &HealthState<'_>, view: &HealthView) {
    let w = &view.workouts;
    state.set_workout_tab(w.tab as i32);
    state.set_exercise_days(model(
        w.exercises
            .iter()
            .map(|day| HealthExerciseDay {
                id: text(&day.id),
                name: text(&day.name),
                exercises: model(
                    day.exercises
                        .iter()
                        .map(|ex| HealthExerciseRow {
                            id: text(&ex.id),
                            name: text(&ex.name),
                            notes: text(&ex.notes),
                            sets: model(
                                ex.sets
                                    .iter()
                                    .map(|set| HealthSetRow {
                                        id: text(&set.id),
                                        kg: text(&set.kg),
                                        reps: text(&set.reps),
                                        rest: text(&set.rest),
                                        done: set.done,
                                    })
                                    .collect(),
                            ),
                        })
                        .collect(),
                ),
            })
            .collect(),
    ));
    state.set_routines(model(
        w.stretches
            .iter()
            .map(|r| HealthRoutine {
                id: text(&r.id),
                name: text(&r.name),
                items: model(
                    r.items
                        .iter()
                        .map(|item| HealthStretchRow {
                            id: text(&item.id),
                            name: text(&item.name),
                            duration: text(&item.duration),
                        })
                        .collect(),
                ),
            })
            .collect(),
    ));
    state.set_rest_remaining(
        w.rest_remaining
            .map(|s| s.to_string())
            .unwrap_or_default()
            .into(),
    );
    match &w.runner {
        Some(run) => {
            state.set_runner_active(true);
            state.set_runner_routine(text(&run.routine));
            state.set_runner_current(text(&run.current));
            state.set_runner_remaining(text(&run.remaining));
            state.set_runner_step(text(&run.step));
            state.set_runner_paused(run.paused);
            state.set_runner_finished(run.finished);
            state.set_runner_progress(run.progress);
        }
        None => state.set_runner_active(false),
    }
}

fn show_measurements(state: &HealthState<'_>, view: &HealthView) {
    let m = &view.measurements;
    state.set_years(model(m.years.iter().map(|y| text(y)).collect()));
    state.set_year(text(&m.year));
    state.set_metrics(model(
        m.metrics
            .iter()
            .map(|row| HealthMetricRow {
                id: text(&row.id),
                name: text(&row.name),
                unit: text(&row.unit),
                target: text(&row.target),
                cells: model(row.cells.iter().map(|c| text(c)).collect()),
            })
            .collect(),
    ));
    state.set_weight_unit(text(&m.goals.weight_unit));
    state.set_weight_gap(text(&m.goals.weight_gap));
    state.set_body_fat_gap(text(&m.goals.body_fat_gap));
    state.set_goal_current_weight(text(&m.goals.current_weight));
    state.set_goal_target_weight(text(&m.goals.target_weight));
    state.set_goal_current_body_fat(text(&m.goals.current_body_fat));
    state.set_goal_target_body_fat(text(&m.goals.target_body_fat));
    state.set_goal_unit(text(&m.goals.weight_unit));
    state.set_chart_metric_name(text(&m.chart_metric_name));
    state.set_chart(model(
        m.chart
            .iter()
            .map(|p| ChartPoint {
                label: text(&p.label),
                value: p.value,
            })
            .collect(),
    ));
}

/// The steps of the routine `category_id` in the Focus timer's type, in order. A duration that does not read as whole
/// seconds counts as zero. An unknown category has no steps.
fn routine_steps(categories: &[StretchCategoryView], category_id: &str) -> Vec<Stretch> {
    let Some(category) = categories.iter().find(|c| c.id == category_id) else {
        return Vec::new();
    };
    category
        .items
        .iter()
        .map(|item| Stretch {
            category: category.name.clone(),
            name: item.name.clone(),
            seconds: item.duration.parse().unwrap_or(0),
        })
        .collect()
}

fn wire(
    ui: &RimelightPersonalShell,
    controller: &HealthController,
    on_routine_started: impl Fn(Vec<Stretch>) + 'static,
) {
    let state = ui.global::<HealthState>();

    let c = controller.clone();
    state.on_dismiss_requested(move || c.dismiss());

    let c = controller.clone();
    state.on_protocol_requested(move |i| c.select_protocol(i.max(0) as usize));

    let c = controller.clone();
    state.on_custom_hours_changed(move |t| c.set_custom_hours(&t));

    let c = controller.clone();
    state.on_custom_start_toggled(move |on| c.set_custom_start_on(on));

    let c = controller.clone();
    state.on_pick_start_day_requested(move |i| {
        c.pick_start_day(i.max(0) as usize);
    });

    let c = controller.clone();
    state.on_step_start_month_requested(move |d| c.step_start_month(d));

    let c = controller.clone();
    state.on_start_time_requested(move |h, m| {
        c.set_start_time(h.max(0) as u32, m.max(0) as u32);
    });

    let c = controller.clone();
    state.on_chart_metric_requested(move |id| {
        c.select_chart_metric(&id);
    });

    let c = controller.clone();
    state.on_start_fast_requested(move || {
        c.start_fast();
    });

    let c = controller.clone();
    state.on_end_fast_requested(move |id| c.end_fast(&id));

    let c = controller.clone();
    state.on_workout_tab_requested(move |i| c.select_tab(i.max(0) as usize));

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_day_requested(move |name| {
        if c.add_exercise_category(&name)
            && let Some(ui) = weak.upgrade()
        {
            ui.global::<HealthState>()
                .set_new_day_name(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_delete_day_requested(move |id| {
        c.delete_exercise_category(&id);
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_exercise_requested(move |cat, name| {
        if c.add_exercise(&cat, &name)
            && let Some(ui) = weak.upgrade()
        {
            ui.global::<HealthState>()
                .set_new_exercise_name(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_delete_exercise_requested(move |cat, ex| {
        c.delete_exercise(&cat, &ex);
    });

    let c = controller.clone();
    state.on_exercise_notes_changed(move |cat, ex, notes| {
        c.set_exercise_notes(&cat, &ex, &notes);
    });

    let c = controller.clone();
    state.on_add_set_requested(move |cat, ex| {
        c.add_set(&cat, &ex);
    });

    let c = controller.clone();
    state.on_delete_set_requested(move |cat, ex, set| {
        c.delete_set(&cat, &ex, &set);
    });

    let c = controller.clone();
    state.on_set_field_changed(move |cat, ex, set, field, value| {
        let field = match field {
            0 => SetField::Kg,
            1 => SetField::Reps,
            _ => SetField::Rest,
        };
        c.set_set_field(&cat, &ex, &set, field, &value);
    });

    let c = controller.clone();
    state.on_toggle_set_requested(move |cat, ex, set| {
        c.toggle_set_done(&cat, &ex, &set);
    });

    // Stretch routines: the category's add/delete and the timer's start and commands.
    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_routine_requested(move |name| {
        if c.add_stretch_category(&name)
            && let Some(ui) = weak.upgrade()
        {
            ui.global::<HealthState>()
                .set_new_routine_name(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_delete_routine_requested(move |id| {
        c.delete_stretch_category(&id);
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_stretch_requested(move |cat, name, duration| {
        if c.add_stretch(&cat, &name, &duration)
            && let Some(ui) = weak.upgrade()
        {
            let state = ui.global::<HealthState>();
            state.set_new_stretch_name(SharedString::new());
            state.set_new_stretch_duration(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_delete_stretch_requested(move |cat, id| {
        c.delete_stretch(&cat, &id);
    });

    let c = controller.clone();
    state.on_start_routine_requested(move |cat| {
        // The steps are read before the start, from the routine as the page shows it.
        let steps = routine_steps(&c.view().workouts.stretches, &cat);
        if c.start_routine(&cat) {
            on_routine_started(steps);
        }
    });

    let c = controller.clone();
    state.on_routine_command_requested(move |command| c.routine_command(&command));

    // Measurements.
    let c = controller.clone();
    state.on_year_requested(move |year| {
        c.select_year(&year);
    });

    let c = controller.clone();
    state.on_cell_changed(move |metric, month, value| {
        c.set_cell(&metric, month.clamp(0, 255) as u8, &value);
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_add_metric_requested(move |name, unit, target| {
        if c.add_metric(&name, &unit, &target)
            && let Some(ui) = weak.upgrade()
        {
            let state = ui.global::<HealthState>();
            state.set_new_metric_name(SharedString::new());
            state.set_new_metric_target(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_remove_metric_requested(move |id| {
        c.remove_metric(&id);
    });

    let c = controller.clone();
    state.on_goals_requested(
        move |unit, current_weight, target_weight, current_fat, target_fat| {
            c.save_goals(
                &unit,
                &current_weight,
                &target_weight,
                &current_fat,
                &target_fat,
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, name: &str, duration: &str) -> rimelight_personal_feature_health::StretchItemView {
        rimelight_personal_feature_health::StretchItemView {
            id: id.to_owned(),
            name: name.to_owned(),
            duration: duration.to_owned(),
        }
    }

    #[test]
    fn routine_steps_keep_order_names_and_seconds() {
        let categories = vec![StretchCategoryView {
            id: "morning".to_owned(),
            name: "Morning".to_owned(),
            items: vec![item("1", "Neck Roll", "30"), item("2", "Cat-Cow", "45")],
        }];
        assert_eq!(
            routine_steps(&categories, "morning"),
            vec![
                Stretch {
                    category: "Morning".to_owned(),
                    name: "Neck Roll".to_owned(),
                    seconds: 30,
                },
                Stretch {
                    category: "Morning".to_owned(),
                    name: "Cat-Cow".to_owned(),
                    seconds: 45,
                },
            ]
        );
    }

    #[test]
    fn unknown_routine_has_no_steps_and_bad_duration_counts_as_zero() {
        let categories = vec![StretchCategoryView {
            id: "a".to_owned(),
            name: "A".to_owned(),
            items: vec![item("1", "Calf", "soon")],
        }];
        assert!(routine_steps(&categories, "missing").is_empty());
        assert_eq!(routine_steps(&categories, "a")[0].seconds, 0);
    }
}
