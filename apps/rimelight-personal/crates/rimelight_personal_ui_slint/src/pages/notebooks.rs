//! Glue between the Notebooks controller (`rimelight_personal_feature_notebooks`) and the shell's `NotebooksState` global.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback here posts the view to the Slint event loop, and only the event loop touches the window.
//!
//! The list's rows are in display order: the pinned notebooks first, then the others. A row index from the page maps to
//! a notebook through the controller's current view in that same order.

use std::sync::Arc;

use rimelight_personal_feature_notebooks::{
    CardView, EntryView, NotebookApi, NotebookTab, NotebooksController, NotebooksView, ThreadView,
};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::generated::{
    AppWindow, NotebookEntryRow, NotebooksState, PageLoadState, TableColumn, TableRow,
};

/// The Notebooks page of the shell: one controller, wired to the shell's `NotebooksState` and its callbacks.
pub struct Notebooks {
    controller: NotebooksController,
}

impl Notebooks {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Notebooks::activate`].
    pub fn new(ui: &AppWindow, api: Arc<dyn NotebookApi>) -> Self {
        let weak = ui.as_weak();
        let controller = NotebooksController::new(api, move |view: NotebooksView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        ui.global::<NotebooksState>().set_columns(columns());
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Notebooks route is shown: the list is fetched again.
    pub fn activate(&self) {
        self.refresh();
    }

    /// Fetches the notebook list again (also the page's retry and first-open hooks).
    pub fn refresh(&self) {
        self.controller.refresh();
    }

    /// Opens the New Notebook form, as the quick-action menu's "New Notebook" does.
    pub fn open_create(&self) {
        self.controller.open_create();
    }

    /// The controller, for tests and for callers that need to act on the page directly.
    #[cfg(test)]
    pub fn controller(&self) -> &NotebooksController {
        &self.controller
    }
}

/// The table's columns: title, entry count, last update, and status.
fn columns() -> ModelRc<TableColumn> {
    let cols = vec![
        TableColumn {
            title: "Notebook".into(),
            width: 360.0,
            sortable: false,
        },
        TableColumn {
            title: "Entries".into(),
            width: 120.0,
            sortable: false,
        },
        TableColumn {
            title: "Updated".into(),
            width: 140.0,
            sortable: false,
        },
        TableColumn {
            title: "Status".into(),
            width: 120.0,
            sortable: false,
        },
    ];
    ModelRc::new(VecModel::from(cols))
}

fn strings(items: &[String]) -> ModelRc<SharedString> {
    let rows: Vec<SharedString> = items
        .iter()
        .map(|s| SharedString::from(s.as_str()))
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn status(card: &CardView) -> &'static str {
    if card.trashed {
        "Trashed"
    } else if card.archived {
        "Archived"
    } else if card.pinned {
        "Pinned"
    } else {
        ""
    }
}

/// One table row per notebook, pinned first. Each row has one cell per column.
fn rows(view: &NotebooksView) -> ModelRc<TableRow> {
    let items: Vec<TableRow> = view
        .pinned
        .iter()
        .chain(view.others.iter())
        .map(|card| TableRow {
            cells: strings(&[
                card.title.clone(),
                card.count.clone(),
                card.date.clone(),
                status(card).to_owned(),
            ]),
            selected: false,
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

fn entries(items: &[EntryView]) -> ModelRc<NotebookEntryRow> {
    let rows: Vec<NotebookEntryRow> = items
        .iter()
        .map(|entry| NotebookEntryRow {
            id: entry.id.as_str().into(),
            date: entry.date.as_str().into(),
            content: entry.content.as_str().into(),
            editing: entry.editing,
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn page_state(view: &NotebooksView) -> PageLoadState {
    if view.loading {
        PageLoadState::Loading
    } else if view.empty {
        PageLoadState::Empty
    } else {
        PageLoadState::Loaded
    }
}

/// Fills the shell's notebooks state from a view. A text field is set only when it differs, so the caret stays put while
/// the user types. Each dialog's fields are set before its open flag, so it opens with them.
fn show_view(ui: &AppWindow, view: &NotebooksView) {
    let g = ui.global::<NotebooksState>();
    g.set_tab_labels(strings(&view.tabs));
    g.set_tab(view.tab as i32);
    if g.get_search_text().as_str() != view.search {
        g.set_search_text(view.search.as_str().into());
    }
    g.set_state(page_state(view));
    g.set_error(view.error.clone().unwrap_or_default().into());
    g.set_rows(rows(view));

    match &view.thread {
        Some(thread) => show_thread(ui, thread),
        None => g.set_thread_open(false),
    }

    match &view.create {
        Some(create) => {
            if g.get_create_title().as_str() != create.title {
                g.set_create_title(create.title.as_str().into());
            }
            if g.get_create_content().as_str() != create.content {
                g.set_create_content(create.content.as_str().into());
            }
            g.set_creating(create.creating);
            g.set_create_open(true);
        }
        None => g.set_create_open(false),
    }

    match &view.confirm {
        Some(confirm) => {
            g.set_confirm_title(confirm.title.as_str().into());
            g.set_confirm_message(confirm.message.as_str().into());
            g.set_confirm_action(confirm.action.as_str().into());
        }
        // An empty title closes the confirmation.
        None => g.set_confirm_title("".into()),
    }
}

fn show_thread(ui: &AppWindow, thread: &ThreadView) {
    let g = ui.global::<NotebooksState>();
    g.set_thread_id(thread.id.as_str().into());
    if g.get_thread_title().as_str() != thread.title {
        g.set_thread_title(thread.title.as_str().into());
    }
    g.set_thread_count(thread.count.as_str().into());
    g.set_entries(entries(&thread.entries));
    g.set_entries_loading(thread.entries_loading);
    if g.get_thread_draft().as_str() != thread.draft {
        g.set_thread_draft(thread.draft.as_str().into());
    }
    g.set_thread_adding(thread.adding);
    if g.get_edit_text().as_str() != thread.edit_text {
        g.set_edit_text(thread.edit_text.as_str().into());
    }
    g.set_thread_pinned(thread.pinned);
    g.set_thread_archived(thread.archived);
    g.set_thread_trashed(thread.trashed);
    g.set_thread_open(true);
}

/// The id of the notebook a list row index names, in display order (pinned first).
fn id_at(controller: &NotebooksController, index: i32) -> Option<String> {
    let index = usize::try_from(index).ok()?;
    let view = controller.view();
    view.pinned
        .iter()
        .chain(view.others.iter())
        .nth(index)
        .map(|card: &CardView| card.id.clone())
}

/// The id of the open thread, as the shell's state holds it.
fn thread_id(ui: &AppWindow) -> String {
    ui.global::<NotebooksState>().get_thread_id().to_string()
}

/// Connects the shell's notebooks callbacks to the controller.
fn wire(ui: &AppWindow, controller: &NotebooksController) {
    let g = ui.global::<NotebooksState>();

    let c = controller.clone();
    g.on_opened(move || c.refresh());
    let c = controller.clone();
    g.on_retry_requested(move || c.refresh());

    let c = controller.clone();
    g.on_tab_requested(move |index| {
        if let Some(tab) = usize::try_from(index)
            .ok()
            .and_then(NotebookTab::from_index)
        {
            c.set_tab(tab);
        }
    });

    let c = controller.clone();
    g.on_search_requested(move |text| c.set_search(&text));

    let c = controller.clone();
    g.on_new_requested(move || c.open_create());

    let c = controller.clone();
    g.on_row_picked(move |index| {
        if let Some(id) = id_at(&c, index) {
            c.open(&id);
        }
    });

    let c = controller.clone();
    g.on_row_action_requested(move |index, action| {
        let Some(id) = id_at(&c, index) else { return };
        match action.as_str() {
            "pin" => c.toggle_pin(&id),
            "archive" | "unarchive" => c.toggle_archive(&id),
            "trash" => c.trash(&id),
            "restore" => c.restore(&id),
            "delete" => c.ask_delete(&id),
            _ => {}
        }
    });

    let c = controller.clone();
    g.on_dismiss_error_requested(move || c.dismiss_error());

    // The create form's fields are read from the shell's state, since the page has no edit callbacks for them.
    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_create_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        let g = ui.global::<NotebooksState>();
        c.create_title(g.get_create_title().as_str());
        c.create_content(g.get_create_content().as_str());
        c.create();
    });

    let c = controller.clone();
    g.on_create_cancel_requested(move || c.close_create());

    let c = controller.clone();
    g.on_thread_closed(move || c.close_thread());

    let c = controller.clone();
    g.on_thread_rename_requested(move |text| c.rename_thread(&text));

    // The add field's text arrives with the submit. The controller's draft takes it first, so the add sends it.
    let c = controller.clone();
    g.on_thread_draft_add_requested(move |text| {
        c.thread_draft(&text);
        c.add_entry();
    });

    let c = controller.clone();
    g.on_start_edit_requested(move |id| c.start_edit(&id));

    let c = controller.clone();
    g.on_edit_cancel_requested(move || c.cancel_edit());

    let c = controller.clone();
    g.on_edit_save_requested(move |text| {
        c.edit_text(&text);
        c.save_edit();
    });

    let c = controller.clone();
    g.on_delete_entry_requested(move |id| c.delete_entry(&id));

    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_thread_pin_requested(move || {
        if let Some(ui) = weak.upgrade() {
            c.toggle_pin(&thread_id(&ui));
        }
    });

    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_thread_archive_requested(move || {
        if let Some(ui) = weak.upgrade() {
            c.toggle_archive(&thread_id(&ui));
        }
    });

    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_thread_trash_requested(move || {
        if let Some(ui) = weak.upgrade() {
            c.ask_trash(&thread_id(&ui));
        }
    });

    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_thread_restore_requested(move || {
        if let Some(ui) = weak.upgrade() {
            c.restore(&thread_id(&ui));
        }
    });

    let (c, weak) = (controller.clone(), ui.as_weak());
    g.on_thread_delete_requested(move || {
        if let Some(ui) = weak.upgrade() {
            c.ask_delete(&thread_id(&ui));
        }
    });

    let c = controller.clone();
    g.on_confirm_accept_requested(move || c.confirm_accept());

    let c = controller.clone();
    g.on_confirm_cancel_requested(move || c.confirm_cancel());
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use rimelight_personal_core::ApiError;
    use rimelight_personal_core::notebooks::{Notebook, NotebookEntry};
    use rimelight_personal_feature_notebooks::{Done, NotebookApi, NotebookChange};
    use slint::Model as _;

    use super::*;

    /// Records the calls the page makes and answers each one at once.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    impl Recorder {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
    }

    fn notebook(id: &str, title: &str, pinned: bool) -> Notebook {
        Notebook {
            id: id.to_owned(),
            title: title.to_owned(),
            color: "neutral".to_owned(),
            is_pinned: pinned,
            is_archived: false,
            is_trashed: false,
            created_at: None,
            updated_at: Some("2026-05-02T09:30:00Z".to_owned()),
            entry_count: 1,
        }
    }

    impl NotebookApi for Recorder {
        fn list(&self, on_done: Done<Vec<Notebook>>) {
            self.record("list".to_owned());
            on_done(Ok(vec![
                notebook("id-a", "Homelab", true),
                notebook("id-b", "Recipes", false),
            ]));
        }

        fn create(&self, title: &str, content: &str, on_done: Done<Notebook>) {
            self.record(format!("create:{title}:{content}"));
            on_done(Err(ApiError::Transport("no network in tests".to_owned())));
        }

        fn update(&self, change: &NotebookChange, on_done: Done<()>) {
            self.record(format!("update:{}:pinned={:?}", change.id, change.pinned));
            on_done(Ok(()));
        }

        fn delete(&self, id: &str, on_done: Done<()>) {
            self.record(format!("delete:{id}"));
            on_done(Ok(()));
        }

        fn entries(&self, notebook_id: &str, on_done: Done<Vec<NotebookEntry>>) {
            self.record(format!("entries:{notebook_id}"));
            on_done(Ok(vec![NotebookEntry {
                id: "e1".to_owned(),
                notebook_id: notebook_id.to_owned(),
                content: "first".to_owned(),
                created_at: Some("2026-05-03T12:00:00Z".to_owned()),
                updated_at: None,
            }]));
        }

        fn add_entry(&self, notebook_id: &str, content: &str, on_done: Done<NotebookEntry>) {
            self.record(format!("add:{notebook_id}:{content}"));
            on_done(Err(ApiError::Transport("no network in tests".to_owned())));
        }

        fn update_entry(
            &self,
            notebook_id: &str,
            entry_id: &str,
            content: &str,
            on_done: Done<()>,
        ) {
            self.record(format!("update_entry:{notebook_id}:{entry_id}:{content}"));
            on_done(Ok(()));
        }

        fn delete_entry(&self, notebook_id: &str, entry_id: &str, on_done: Done<()>) {
            self.record(format!("delete_entry:{notebook_id}:{entry_id}"));
            on_done(Ok(()));
        }
    }

    /// The page's callbacks drive the controller, and the controller's view is what the shell then shows. Row indexes
    /// are display order (pinned first). Run by `pages::tests::page_scenarios`, on the thread with the testing backend.
    pub(crate) fn scenario() {
        let ui = AppWindow::new().expect("create the shell");
        let api = Arc::new(Recorder::default());
        let notebooks = Notebooks::new(&ui, api.clone());
        ui.set_route("/notebooks".into());

        // Opening the route fetches the list. The headless backend runs no event loop, so the posted view is applied here.
        notebooks.activate();
        show_view(&ui, &notebooks.controller().view());
        assert_eq!(api.calls(), ["list"]);
        assert_eq!(ui.global::<NotebooksState>().get_rows().row_count(), 2);

        // Row 1 is "Recipes" (pinned Homelab comes first). Pin toggles its flag.
        ui.global::<NotebooksState>()
            .invoke_row_action_requested(1, "pin".into());
        assert!(
            api.calls()
                .contains(&"update:id-b:pinned=Some(true)".to_owned()),
            "{:?}",
            api.calls()
        );

        // Picking row 0 opens Homelab and loads its entries.
        ui.global::<NotebooksState>().invoke_row_picked(0);
        assert!(api.calls().contains(&"entries:id-a".to_owned()));
        show_view(&ui, &notebooks.controller().view());
        assert!(ui.global::<NotebooksState>().get_thread_open());
        assert_eq!(
            ui.global::<NotebooksState>().get_thread_id().as_str(),
            "id-a"
        );

        // The add field submits: the add is sent, and the failed add keeps the draft.
        ui.global::<NotebooksState>()
            .invoke_thread_draft_add_requested("hi".into());
        assert!(
            api.calls().contains(&"add:id-a:hi".to_owned()),
            "{:?}",
            api.calls()
        );
        show_view(&ui, &notebooks.controller().view());
        assert_eq!(
            notebooks.controller().view().thread.unwrap().draft,
            "hi",
            "a failed add keeps the draft"
        );

        // An entry's edit: saving sends the update. Cancelling an edit sends nothing.
        ui.global::<NotebooksState>()
            .invoke_start_edit_requested("e1".into());
        show_view(&ui, &notebooks.controller().view());
        assert!(notebooks.controller().view().thread.unwrap().entries[0].editing);
        ui.global::<NotebooksState>().invoke_edit_cancel_requested();
        assert!(
            !notebooks.controller().view().thread.unwrap().entries[0].editing,
            "cancel ends the edit"
        );
        assert!(
            !api.calls().iter().any(|c| c.starts_with("update_entry")),
            "a cancelled edit saves nothing"
        );

        // New Notebook: the form opens; submitting with no fields is refused, so nothing is sent.
        notebooks.controller().close_thread();
        ui.global::<NotebooksState>().invoke_new_requested();
        show_view(&ui, &notebooks.controller().view());
        assert!(ui.global::<NotebooksState>().get_create_open());
        ui.global::<NotebooksState>().invoke_create_requested();
        assert!(
            !api.calls().iter().any(|call| call.starts_with("create:")),
            "{:?}",
            api.calls()
        );
    }
}
