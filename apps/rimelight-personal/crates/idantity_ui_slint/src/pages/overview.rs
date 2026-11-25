//! Glue between the Overview controller (`idantity_feature_overview`) and the shell's `OverviewRoute`.
//!
//! The controller answers on a worker thread. `notify` posts the view to the Slint event loop, and only the event
//! loop touches the window. Navigation links go to the shell's route switch (`IdantityShell::set_route`).

use std::sync::Arc;

use idantity_feature_overview::{OverviewApi, OverviewController, OverviewView};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::IdantityShell;
use crate::generated::{OverviewNote, OverviewState, OverviewTask};

/// The Overview page of the shell: one controller, wired to `OverviewState`.
pub struct Overview {
    controller: OverviewController,
}

impl Overview {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Overview::activate`].
    pub fn new(ui: &IdantityShell, api: Arc<dyn OverviewApi>) -> Self {
        let weak = ui.as_weak();
        let controller = OverviewController::new(api, move |view: OverviewView| {
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Overview route is shown: the data is fetched again.
    pub fn activate(&self) {
        self.controller.refresh();
    }
}

fn show_view(ui: &IdantityShell, view: &OverviewView) {
    let state = ui.global::<OverviewState>();
    state.set_date(view.date.as_str().into());
    state.set_pending_count(view.pending_count as i32);
    state.set_due_today_count(view.due_today_count as i32);
    state.set_urgent_count(view.urgent_count as i32);
    let tasks: Vec<OverviewTask> = view
        .tasks
        .iter()
        .map(|t| OverviewTask {
            id: t.id.as_str().into(),
            title: t.title.as_str().into(),
            category: t.category.as_str().into(),
            priority: t.priority.as_str().into(),
            tone: t.tone.as_str().into(),
        })
        .collect();
    state.set_tasks(ModelRc::new(VecModel::from(tasks)));
    state.set_todo_total(view.todo_total as i32);
    state.set_net_worth(view.net_worth.as_str().into());
    state.set_account_count(view.account_count as i32);
    state.set_monthly_bills(view.monthly_bills.as_str().into());
    state.set_active_subscriptions(view.active_subscriptions as i32);
    let notes: Vec<OverviewNote> = view
        .notes
        .iter()
        .map(|n| OverviewNote {
            id: n.id.as_str().into(),
            title: n.title.as_str().into(),
            preview: n.preview.as_str().into(),
        })
        .collect();
    state.set_notes(ModelRc::new(VecModel::from(notes)));
    state.set_note_total(view.note_total as i32);
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
}

fn wire(ui: &IdantityShell, controller: &OverviewController) {
    let state = ui.global::<OverviewState>();

    // The shell's route switch: the route and its navbar title. The sidebar highlight is not moved here (see report).
    let weak = ui.as_weak();
    state.on_navigate_requested(move |route| {
        if let Some(ui) = weak.upgrade() {
            ui.set_route(route.clone());
            ui.set_page_title(idantity_nav::title_for(&route).into());
        }
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_quick_task_submitted(move || {
        let Some(ui) = weak.upgrade() else { return };
        // The field is two-way bound to the global, so the controller takes its text here before the add.
        c.set_quick_task(&ui.global::<OverviewState>().get_quick_task());
        // The field clears only when the controller accepts the title.
        if c.add_quick_task() {
            ui.global::<OverviewState>()
                .set_quick_task(SharedString::new());
        }
    });

    let c = controller.clone();
    state.on_toggle_todo_requested(move |id| {
        c.toggle_todo(&id);
    });

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());
}
