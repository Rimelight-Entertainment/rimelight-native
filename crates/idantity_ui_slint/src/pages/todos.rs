//! Glue between the Todos controller (`idantity_feature_todos`) and the TodosState global of `ui/pages/todos.slint`.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback posts the view to the Slint event loop, and only the event loop touches the window.

use std::sync::Arc;

use idantity_feature_todos::{TodoApi, TodosController, TodosView};
use slint::{ComponentHandle as _, ModelRc, VecModel};

use crate::generated::{IdantityShell, TodoRow, TodoTab, TodosState};

/// The Todos page of the shell: one controller, wired to the TodosState global.
pub struct Todos {
    controller: TodosController,
}

impl Todos {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Todos::activate`].
    pub fn new(ui: &IdantityShell, api: Arc<dyn TodoApi>) -> Self {
        let weak = ui.as_weak();
        let controller = TodosController::new(api, move |view: TodosView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Todos route opens: the list is fetched again.
    pub fn activate(&self) {
        self.controller.refresh();
    }

    /// The controller, for tests.
    #[cfg(test)]
    pub fn controller(&self) -> &TodosController {
        &self.controller
    }
}

/// Fills the TodosState properties from a view. Models are rebuilt each time, so every row starts again from the
/// server's flags (a checkbox keeps its own state after the user clicks it).
fn show_view(ui: &IdantityShell, view: &TodosView) {
    let state = ui.global::<TodosState>();
    let rows: Vec<TodoRow> = view
        .rows
        .iter()
        .map(|row| TodoRow {
            id: row.id.as_str().into(),
            title: row.title.as_str().into(),
            category: row.category.as_str().into(),
            priority: row.priority.as_str().into(),
            completed: row.completed,
            selected: row.selected,
            recurring: row.recurring,
            recurring_label: row.recurring_label.as_str().into(),
            due_label: row.due_label.as_str().into(),
            due_today: row.due_today,
        })
        .collect();
    let tabs: Vec<TodoTab> = view
        .tabs
        .iter()
        .map(|tab| TodoTab {
            label: tab.label.as_str().into(),
            count: tab.count as i32,
        })
        .collect();
    state.set_rows(ModelRc::new(VecModel::from(rows)));
    state.set_tabs(ModelRc::new(VecModel::from(tabs)));
    state.set_filter(view.filter as i32);
    state.set_selected_count(view.selected_count as i32);
    state.set_confirm_open(view.confirm_delete_open);
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
}

/// Connects the TodosState callbacks to the controller.
fn wire(ui: &IdantityShell, controller: &TodosController) {
    let state = ui.global::<TodosState>();

    let c = controller.clone();
    state.on_add_requested(move |title, category, priority, due, recurring, interval| {
        // The page clears its own fields after it sends a non-blank title (TodosPage.submit-add).
        c.add(&title, &category, &priority, &due, recurring, &interval);
    });

    let c = controller.clone();
    state.on_search_changed(move |text| c.set_search(&text));

    let c = controller.clone();
    state.on_filter_requested(move |index| c.set_filter(index.max(0) as usize));

    let c = controller.clone();
    state.on_refresh_requested(move || c.refresh());

    let c = controller.clone();
    state.on_toggle_requested(move |id, done| c.toggle(&id, done));

    let c = controller.clone();
    state.on_delete_requested(move |id| c.delete(&id));

    let c = controller.clone();
    state.on_select_requested(move |id| c.toggle_select(&id));

    let c = controller.clone();
    state.on_batch_complete_requested(move || c.batch_complete());

    let c = controller.clone();
    state.on_batch_delete_requested(move || c.request_batch_delete());

    let c = controller.clone();
    state.on_batch_cancel_requested(move || c.clear_selection());

    let c = controller.clone();
    state.on_batch_delete_confirmed(move || c.confirm_batch_delete());

    let c = controller.clone();
    state.on_batch_delete_cancelled(move || c.cancel_batch_delete());

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use idantity_core::ApiError;
    use idantity_feature_todos::{NewTodo, TodoItem};

    use super::*;

    /// A server that is down: every call is recorded and answers with an error.
    #[derive(Default)]
    struct Down {
        calls: Mutex<Vec<&'static str>>,
    }

    impl Down {
        fn record(&self, call: &'static str) {
            self.calls.lock().unwrap().push(call);
        }

        fn error() -> ApiError {
            ApiError::Status {
                code: 503,
                body: "down".to_owned(),
            }
        }
    }

    impl TodoApi for Down {
        fn list(&self, on_done: Box<dyn FnOnce(Result<Vec<TodoItem>, ApiError>) + Send>) {
            self.record("list");
            on_done(Err(Self::error()));
        }

        fn create(&self, _todo: &NewTodo, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>) {
            self.record("create");
            on_done(Err(Self::error()));
        }

        fn set_completed(
            &self,
            _id: &str,
            _is_completed: bool,
            on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>,
        ) {
            self.record("set_completed");
            on_done(Err(Self::error()));
        }

        fn delete(&self, _id: &str, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>) {
            self.record("delete");
            on_done(Err(Self::error()));
        }
    }

    /// Run by `pages::tests::page_scenarios`, on the thread that installed the testing backend. A failed load shows its
    /// error on the page, and the error can be dismissed.
    pub(crate) fn scenario() {
        let ui = IdantityShell::new().expect("create the shell");
        let api = Arc::new(Down::default());
        let todos = Todos::new(&ui, api.clone());
        todos.activate();
        assert_eq!(api.calls.lock().unwrap().as_slice(), ["list"]);

        show_view(&ui, &todos.controller().view());
        let state = ui.global::<TodosState>();
        assert!(!state.get_loading());
        assert!(!state.get_error().is_empty());

        state.invoke_dismiss_error_requested();
        show_view(&ui, &todos.controller().view());
        assert!(state.get_error().is_empty());
    }
}
