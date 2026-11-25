//! Glue between the Notes controller (`idantity_feature_notes`) and the shell's notes properties.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback here posts the view to the Slint event loop, and only the event loop touches the window.
//!
//! The editor's autosave is a debounce owned here: each edit re-arms a 600 ms timer, and when it fires the controller
//! saves the draft. A colour pick saves at once, as in the egui page.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use idantity_feature_notes::{
    CardView, EditorView, NoteApi, NoteBatchAction, NoteTab, NotesController, NotesView,
};
use slint::{ComponentHandle as _, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::generated::{IdantityShell, NoteCard, NotesState};

/// Autosave debounce. Matches the Solid editor's 600 ms timer.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(600);

/// The Notes page of the shell: one controller, wired to the shell's notes properties and callbacks.
pub struct Notes {
    controller: NotesController,
}

impl Notes {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Notes::activate`].
    pub fn new(ui: &IdantityShell, api: Arc<dyn NoteApi>) -> Self {
        let weak = ui.as_weak();
        let controller = NotesController::new(api, move |view: NotesView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        let autosave = Rc::new(RefCell::new(None));
        wire(ui, &controller, &autosave);
        Self { controller }
    }

    /// The shell calls this when the Notes route is shown: the list is fetched again.
    pub fn activate(&self) {
        self.controller.refresh();
    }

    /// Opens the quick-add form, as the quick-action menu's "New Note" does.
    pub fn open_quick(&self) {
        self.controller.open_quick();
    }

    /// The controller, for tests and for callers that need to act on the page directly.
    #[cfg(test)]
    pub fn controller(&self) -> &NotesController {
        &self.controller
    }
}

/// Starts the autosave debounce: the controller saves once no edit has come for [`AUTOSAVE_DELAY`]. Dropping the
/// previous timer cancels it.
fn arm_autosave(slot: &RefCell<Option<Timer>>, controller: &NotesController) {
    let timer = Timer::default();
    let c = controller.clone();
    timer.start(TimerMode::SingleShot, AUTOSAVE_DELAY, move || c.autosave());
    *slot.borrow_mut() = Some(timer);
}

fn strings(items: &[String]) -> ModelRc<SharedString> {
    let rows: Vec<SharedString> = items
        .iter()
        .map(|s| SharedString::from(s.as_str()))
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn cards(items: &[CardView]) -> ModelRc<NoteCard> {
    let rows: Vec<NoteCard> = items
        .iter()
        .map(|card| NoteCard {
            id: card.id.as_str().into(),
            title: card.title.as_str().into(),
            preview: card.preview.as_str().into(),
            empty: card.empty,
            tags: strings(&card.tags),
            date: card.date.as_str().into(),
            color: card.color.as_str().into(),
            pinned: card.pinned,
            archived: card.archived,
            trashed: card.trashed,
            selected: card.selected,
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Fills the shell's notes properties from a view. The card lists are new models each time, so a card's checkbox
/// starts again from the controller's selection.
///
/// Order matters for the dialogs: the fields are set before the flag that opens the dialog, so it opens with them.
fn show_view(ui: &IdantityShell, view: &NotesView) {
    ui.global::<NotesState>()
        .set_tab_labels(strings(&view.tabs));
    ui.global::<NotesState>().set_tab(view.tab.index() as i32);
    if ui.global::<NotesState>().get_search().as_str() != view.search {
        ui.global::<NotesState>()
            .set_search(view.search.as_str().into());
    }
    ui.global::<NotesState>().set_tags(strings(&view.tags));
    ui.global::<NotesState>()
        .set_tag_filter(view.tag_filter.clone().unwrap_or_default().into());
    ui.global::<NotesState>()
        .set_selected_count(view.selected as i32);
    ui.global::<NotesState>().set_loading(view.loading);
    ui.global::<NotesState>().set_no_notes(view.no_notes);
    ui.global::<NotesState>()
        .set_error(view.error.clone().unwrap_or_default().into());
    ui.global::<NotesState>().set_pinned(cards(&view.pinned));
    ui.global::<NotesState>().set_others(cards(&view.others));

    match &view.editor {
        Some(editor) => show_editor(ui, editor),
        None => ui.global::<NotesState>().set_editor_visible(false),
    }

    if view.quick_open {
        ui.global::<NotesState>()
            .set_quick_creating(view.quick_creating);
        ui.global::<NotesState>().set_quick_open(true);
    } else {
        // The form is closed: its fields start empty the next time it opens.
        ui.global::<NotesState>().set_quick_open(false);
        ui.global::<NotesState>().set_quick_creating(false);
        ui.global::<NotesState>().set_quick_title("".into());
        ui.global::<NotesState>().set_quick_content("".into());
        ui.global::<NotesState>().set_quick_tags("".into());
    }

    match &view.confirm {
        Some(confirm) => {
            ui.global::<NotesState>()
                .set_confirm_title(confirm.title.as_str().into());
            ui.global::<NotesState>()
                .set_confirm_message(confirm.message.as_str().into());
            ui.global::<NotesState>()
                .set_confirm_action(confirm.action.as_str().into());
            ui.global::<NotesState>().set_confirm_open(true);
        }
        None => ui.global::<NotesState>().set_confirm_open(false),
    }
}

fn show_editor(ui: &IdantityShell, editor: &EditorView) {
    // A text field is set only when it differs, so the caret stays put while the user types.
    if ui.global::<NotesState>().get_editor_title().as_str() != editor.title {
        ui.global::<NotesState>()
            .set_editor_title(editor.title.as_str().into());
    }
    if ui.global::<NotesState>().get_editor_content().as_str() != editor.content {
        ui.global::<NotesState>()
            .set_editor_content(editor.content.as_str().into());
    }
    if ui.global::<NotesState>().get_editor_tag_input().as_str() != editor.tag_input {
        ui.global::<NotesState>()
            .set_editor_tag_input(editor.tag_input.as_str().into());
    }
    ui.global::<NotesState>()
        .set_editor_existing(editor.existing);
    ui.global::<NotesState>().set_editor_trashed(editor.trashed);
    ui.global::<NotesState>()
        .set_editor_tags(strings(&editor.tags));
    ui.global::<NotesState>()
        .set_editor_color(editor.color.as_str().into());
    ui.global::<NotesState>()
        .set_editor_color_open(editor.color_open);
    ui.global::<NotesState>().set_editor_pinned(editor.pinned);
    ui.global::<NotesState>()
        .set_editor_archived(editor.archived);
    ui.global::<NotesState>()
        .set_editor_status(editor.status.as_str().into());
    ui.global::<NotesState>().set_editor_visible(true);
}

/// Connects the shell's notes callbacks to the controller. Editor edits also re-arm the autosave debounce.
fn wire(ui: &IdantityShell, controller: &NotesController, autosave: &Rc<RefCell<Option<Timer>>>) {
    let c = controller.clone();
    ui.global::<NotesState>().on_tab_requested(move |index| {
        if let Some(tab) = usize::try_from(index).ok().and_then(NoteTab::from_index) {
            c.set_tab(tab);
        }
    });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_search_edited(move |text| c.set_search(&text));

    let c = controller.clone();
    ui.global::<NotesState>().on_tag_requested(move |tag| {
        // An empty tag is "All Tags".
        c.set_tag_filter(if tag.is_empty() {
            None
        } else {
            Some(tag.to_string())
        });
    });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_new_note_requested(move || c.new_note());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_clear_selection_requested(move || c.clear_selection());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_archive_selected_requested(move || c.batch(NoteBatchAction::Archive));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_trash_selected_requested(move || c.batch(NoteBatchAction::Trash));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_restore_selected_requested(move || c.batch(NoteBatchAction::Restore));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_ask_batch_delete_requested(move || c.ask_batch_delete());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_select_requested(move |id| c.toggle_select(&id));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_open_requested(move |id| c.open_note(&id));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_pin_requested(move |id, pinned| c.pin(&id, pinned));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_archive_requested(move |id, archived| c.archive(&id, archived));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_trash_requested(move |id| c.trash(&id));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_delete_requested(move |id| c.ask_delete(&id));

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_dismiss_error_requested(move || c.dismiss_error());

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>()
        .on_editor_title_edited(move |text| {
            c.editor_title(&text);
            arm_autosave(&slot, &c);
        });

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>()
        .on_editor_content_edited(move |text| {
            c.editor_content(&text);
            arm_autosave(&slot, &c);
        });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_tag_input_edited(move |text| c.editor_tag_input(&text));

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>().on_editor_tag_added(move || {
        c.editor_add_tag();
        arm_autosave(&slot, &c);
    });

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>()
        .on_editor_tag_removed(move |index| {
            if let Ok(index) = usize::try_from(index) {
                c.editor_remove_tag(index);
                arm_autosave(&slot, &c);
            }
        });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_color_toggled(move || c.editor_toggle_color_open());

    // Saves at once inside the controller, so no debounce is armed.
    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_color_picked(move |name| c.editor_set_color(&name));

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>().on_editor_pin_toggled(move || {
        c.editor_toggle_pinned();
        arm_autosave(&slot, &c);
    });

    let (c, slot) = (controller.clone(), autosave.clone());
    ui.global::<NotesState>()
        .on_editor_archive_toggled(move || {
            c.editor_toggle_archived();
            arm_autosave(&slot, &c);
        });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_trash_requested(move || c.editor_ask_trash());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_restore_requested(move || c.editor_restore());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_delete_requested(move || c.editor_ask_delete());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_editor_dismissed(move || c.request_close());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_quick_submit_requested(move |title, content, tags| {
            c.quick_submit(&title, &content, &tags)
        });

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_quick_cancel_requested(move || c.quick_cancel());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_confirm_accepted(move || c.accept_confirm());

    let c = controller.clone();
    ui.global::<NotesState>()
        .on_confirm_cancelled(move || c.cancel_confirm());
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use i_slint_backend_testing::ElementHandle;
    use idantity_core::{ApiError, Note};
    use idantity_feature_notes::{Draft, NoteApi, NoteChange};
    use slint::Model;
    use slint::platform::PointerEventButton;

    use super::*;

    /// Records the calls the page makes. Lists two notes; every write succeeds.
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

    fn note(id: &str, title: &str, pinned: bool) -> Note {
        Note {
            id: id.to_owned(),
            title: title.to_owned(),
            content: format!("{title} body"),
            tags: vec!["home".to_owned()],
            color: "neutral".to_owned(),
            is_pinned: pinned,
            is_archived: false,
            is_trashed: false,
            updated_at: Some("2026-11-08T19:55:17.000Z".to_owned()),
        }
    }

    impl NoteApi for Recorder {
        fn list(&self, on_done: idantity_feature_notes::Done<Vec<Note>>) {
            self.record("list".to_owned());
            on_done(Ok(vec![
                note("a", "Pinned idea", true),
                note("b", "Groceries", false),
            ]));
        }

        fn create(&self, draft: Draft, on_done: idantity_feature_notes::Done<Note>) {
            self.record(format!("create:{}", draft.title));
            on_done(Err(ApiError::Transport("no network in tests".to_owned())));
        }

        fn update(&self, change: NoteChange, on_done: idantity_feature_notes::Done<()>) {
            self.record(format!(
                "update:{}:pinned={:?}",
                change.id, change.is_pinned
            ));
            on_done(Ok(()));
        }

        fn delete(&self, id: &str, on_done: idantity_feature_notes::Done<()>) {
            self.record(format!("delete:{id}"));
            on_done(Ok(()));
        }

        fn batch(
            &self,
            action: NoteBatchAction,
            ids: &[String],
            on_done: idantity_feature_notes::Done<()>,
        ) {
            self.record(format!("batch:{action:?}:{}", ids.join(",")));
            on_done(Ok(()));
        }
    }

    /// A real pointer press and release on the element with this accessible label.
    fn click(ui: &IdantityShell, label: &str) {
        let handle = ElementHandle::find_by_accessible_label(ui, label)
            .next()
            .unwrap_or_else(|| panic!("no element labelled {label:?}"));
        handle.mock_single_click(PointerEventButton::Left);
    }

    /// Clicks reach the controller, and the controller's view is what the page then shows.
    /// One test function, because the testing backend is installed once per process.
    /// Clicks reach the controller, and the controller's view is what the page then shows. Called by the page
    /// scenarios test in `pages/mod.rs`, which installs the headless backend first.
    pub(crate) fn scenario() {
        let ui = IdantityShell::new().expect("create the shell");
        let api = Arc::new(Recorder::default());
        let notes = Notes::new(&ui, api.clone());
        ui.set_route("/notes".into());
        ui.show().expect("show the shell");

        // Opening the route fetches the list. The headless backend runs no event loop, so the posted view never
        // arrives; the test applies the controller's current view itself, as the event loop would.
        notes.activate();
        show_view(&ui, &notes.controller().view());
        assert_eq!(api.calls(), ["list"]);
        assert_eq!(notes.controller().view().pinned.len(), 1);
        assert_eq!(ui.global::<NotesState>().get_tab_labels().row_count(), 3);

        // Pointer: the card's checkbox selects the note; the batch button then sends the selection.
        click(&ui, "Select Groceries");
        show_view(&ui, &notes.controller().view());
        assert_eq!(notes.controller().view().selected, 1);
        assert_eq!(ui.global::<NotesState>().get_selected_count(), 1);
        click(&ui, "Archive Selected");
        assert!(
            api.calls().contains(&"batch:Archive:b".to_owned()),
            "{:?}",
            api.calls()
        );
        show_view(&ui, &notes.controller().view());
        assert_eq!(notes.controller().view().selected, 0);

        // Pointer: the card's Pin button sends the flag, and the card moves to the pinned group.
        click(&ui, "Pin Groceries");
        assert!(
            api.calls()
                .contains(&"update:b:pinned=Some(true)".to_owned()),
            "{:?}",
            api.calls()
        );
        show_view(&ui, &notes.controller().view());
        assert_eq!(notes.controller().view().pinned.len(), 2);

        // Pointer: the Archive tab is selected and the list is empty (no archived notes).
        click(&ui, "Archive (0)");
        show_view(&ui, &notes.controller().view());
        assert_eq!(notes.controller().view().tab, NoteTab::Archived);
        assert!(
            notes.controller().view().pinned.is_empty()
                && notes.controller().view().others.is_empty()
        );
        click(&ui, "Active (2)");
        show_view(&ui, &notes.controller().view());

        // Pointer: the card's title opens the editor, which shows the note.
        click(&ui, "Open Groceries");
        show_view(&ui, &notes.controller().view());
        assert!(
            ui.global::<NotesState>().get_editor_visible(),
            "the editor opens"
        );
        assert_eq!(
            ui.global::<NotesState>().get_editor_title().as_str(),
            "Groceries"
        );
        assert_eq!(
            notes.controller().view().editor.map(|e| e.existing),
            Some(true)
        );
    }
}
