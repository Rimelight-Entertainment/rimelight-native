//! The Notes page controller (ported from `crates/app/src/notes.rs`).
//!
//! Plain Rust with no Slint. The controller holds the page state (tab, search, tag filter, selection, the open
//! editor, the quick-create form, the confirmation), calls the notes endpoints through [`NoteApi`], and reports each
//! state change as a [`NotesView`]. The Slint glue in `apps/desktop/src/pages/notes.rs` turns those views into the
//! models of `ui/notes.slint`.
//!
//! Threading: API calls return at once and answer later on another thread (ehttp's worker). The controller locks its
//! state only around reads and writes, never across an API call, and calls `notify` after the lock is released. The
//! notify callback must hand the view to the UI thread itself (the glue uses `Weak::upgrade_in_event_loop`).
//!
//! Autosave: the controller does not own a timer. The glue arms a debounce timer after each editor edit and calls
//! [`NotesController::autosave`] when it fires. Colour changes save at once, as in the egui page.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rimelight_personal_core::{ApiClient, ApiError, NewNote, Note, NotePatch};

/// The batch actions the page sends. Re-exported so the desktop glue needs no direct `rimelight_personal_core` dependency.
pub use rimelight_personal_core::NoteBatchAction;

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@rimelight_personal_notes`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Solid `COLOR_OPTIONS`, in the same order.
pub const COLORS: [&str; 8] = [
    "neutral",
    "info",
    "success",
    "warning",
    "error",
    "commentary",
    "ideation",
    "source",
];

/// A callback that receives the result of one API call.
pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait NoteApi: Send + Sync {
    fn list(&self, on_done: Done<Vec<Note>>);
    fn create(&self, draft: Draft, on_done: Done<Note>);
    fn update(&self, change: NoteChange, on_done: Done<()>);
    fn delete(&self, id: &str, on_done: Done<()>);
    fn batch(&self, action: NoteBatchAction, ids: &[String], on_done: Done<()>);
}

impl NoteApi for ApiClient {
    fn list(&self, on_done: Done<Vec<Note>>) {
        ApiClient::notes(self, on_done);
    }

    fn create(&self, draft: Draft, on_done: Done<Note>) {
        let new = NewNote {
            title: &draft.title,
            content: &draft.content,
            tags: draft.tags.as_slice(),
            color: &draft.color,
            is_pinned: draft.pinned,
        };
        ApiClient::create_note(self, &new, on_done);
    }

    fn update(&self, change: NoteChange, on_done: Done<()>) {
        let patch = NotePatch {
            id: &change.id,
            title: change.title.as_deref(),
            content: change.content.as_deref(),
            tags: change.tags.as_deref(),
            color: change.color.as_deref(),
            is_pinned: change.is_pinned,
            is_archived: change.is_archived,
            is_trashed: change.is_trashed,
        };
        ApiClient::update_note(self, &patch, on_done);
    }

    fn delete(&self, id: &str, on_done: Done<()>) {
        ApiClient::delete_note(self, id, on_done);
    }

    fn batch(&self, action: NoteBatchAction, ids: &[String], on_done: Done<()>) {
        ApiClient::batch_notes(self, action, ids, on_done);
    }
}

/// The live API for `api_base`, for example `https://example.com/api/personal`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn NoteApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The three tabs of the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteTab {
    #[default]
    Active,
    Archived,
    Trashed,
}

impl NoteTab {
    pub const ALL: [NoteTab; 3] = [NoteTab::Active, NoteTab::Archived, NoteTab::Trashed];

    pub fn label(self) -> &'static str {
        match self {
            NoteTab::Active => "Active",
            NoteTab::Archived => "Archive",
            NoteTab::Trashed => "Trash",
        }
    }

    pub fn includes(self, note: &Note) -> bool {
        match self {
            NoteTab::Active => !note.is_archived && !note.is_trashed,
            NoteTab::Archived => note.is_archived && !note.is_trashed,
            NoteTab::Trashed => note.is_trashed,
        }
    }

    /// The position in [`NoteTab::ALL`], as the Slint tab bar counts them.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

/// The fields of a note as the editor edits them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub color: String,
    pub pinned: bool,
    pub archived: bool,
}

impl Draft {
    pub fn blank() -> Self {
        Self {
            title: String::new(),
            content: String::new(),
            tags: Vec::new(),
            color: "neutral".to_owned(),
            pinned: false,
            archived: false,
        }
    }

    fn from_note(note: &Note) -> Self {
        Self {
            title: note.title.clone(),
            content: note.content.clone(),
            tags: note.tags.clone(),
            color: note.color.clone(),
            pinned: note.is_pinned,
            archived: note.is_archived,
        }
    }

    /// The Solid `triggerSave` payload: trimmed text, with "Untitled" when the title is empty.
    fn payload(&self) -> Draft {
        let title = self.title.trim();
        Draft {
            title: if title.is_empty() {
                "Untitled".to_owned()
            } else {
                title.to_owned()
            },
            content: self.content.trim().to_owned(),
            tags: self.tags.clone(),
            color: self.color.clone(),
            pinned: self.pinned,
            archived: self.archived,
        }
    }

    /// Solid `isDefault`: nothing entered and no non-default setting.
    fn is_blank(&self) -> bool {
        self.title.trim().is_empty()
            && self.content.trim().is_empty()
            && self.tags.is_empty()
            && self.color == "neutral"
            && !self.pinned
            && !self.archived
    }
}

/// The fields of an existing note that a PATCH sends. `None` leaves a field alone on the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteChange {
    pub id: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub color: Option<String>,
    pub is_pinned: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_trashed: Option<bool>,
}

/// The flags an optimistic card action changes. `None` leaves a flag alone.
#[derive(Clone, Copy, Default)]
struct Flags {
    pinned: Option<bool>,
    archived: Option<bool>,
    trashed: Option<bool>,
}

impl Flags {
    fn apply(self, note: &mut Note) {
        if let Some(value) = self.pinned {
            note.is_pinned = value;
        }
        if let Some(value) = self.archived {
            note.is_archived = value;
        }
        if let Some(value) = self.trashed {
            note.is_trashed = value;
        }
    }

    fn change(self, id: &str) -> NoteChange {
        NoteChange {
            id: id.to_owned(),
            is_pinned: self.pinned,
            is_archived: self.archived,
            is_trashed: self.trashed,
            ..NoteChange::default()
        }
    }
}

/// One card of the list, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub id: String,
    pub title: String,
    /// The first three lines of the content, or "Empty note" when there is none.
    pub preview: String,
    pub empty: bool,
    pub tags: Vec<String>,
    /// The `YYYY-MM-DD` of the last update, or empty.
    pub date: String,
    pub color: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed: bool,
    pub selected: bool,
}

/// The open editor, while it is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorView {
    /// False until the first create succeeds.
    pub existing: bool,
    pub trashed: bool,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub tag_input: String,
    pub color: String,
    pub color_open: bool,
    pub pinned: bool,
    pub archived: bool,
    /// "", "Saving...", "Saved" or "Save failed".
    pub status: String,
}

/// The confirmation dialog, while it is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmView {
    pub title: String,
    pub message: String,
    pub action: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesView {
    /// "Active (3)", "Archive (1)", "Trash (0)": the tab labels with their counts, in [`NoteTab::ALL`] order.
    pub tabs: Vec<String>,
    pub tab: NoteTab,
    pub search: String,
    /// Every tag in use, sorted.
    pub tags: Vec<String>,
    pub tag_filter: Option<String>,
    pub selected: usize,
    pub loading: bool,
    /// True when no note has been loaded at all (the page shows "Loading notes" while loading).
    pub no_notes: bool,
    pub error: Option<String>,
    pub pinned: Vec<CardView>,
    pub others: Vec<CardView>,
    pub editor: Option<EditorView>,
    pub quick_open: bool,
    pub quick_creating: bool,
    pub confirm: Option<ConfirmView>,
}

/// The editor's save in progress, to be sent by the caller (outside the lock).
struct SaveRequest {
    session: u64,
    id: Option<String>,
    payload: Draft,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveStatus {
    Idle,
    Saving,
    Saved,
    Failed,
}

impl SaveStatus {
    fn label(self) -> &'static str {
        match self {
            SaveStatus::Idle => "",
            SaveStatus::Saving => "Saving...",
            SaveStatus::Saved => "Saved",
            SaveStatus::Failed => "Save failed",
        }
    }
}

/// One open editor. `session` tags its save requests so a late reply can only touch the editor that sent it.
struct Editor {
    session: u64,
    /// `None` until the first create succeeds.
    id: Option<String>,
    draft: Draft,
    /// The draft the server last confirmed.
    saved: Draft,
    /// The draft sent with the request in flight.
    in_flight: Option<Draft>,
    saving: bool,
    /// The draft changed while a save was in flight, so the settled save must send again.
    dirty: bool,
    status: SaveStatus,
    /// Close was requested; the editor is hidden and removed once any in-flight save settles.
    closing: bool,
    tag_input: String,
    color_open: bool,
}

impl Editor {
    /// The baseline is the state at open, so opening never triggers a save.
    fn new(session: u64, id: Option<String>, draft: Draft) -> Self {
        Self {
            session,
            id,
            saved: draft.clone(),
            draft,
            in_flight: None,
            saving: false,
            dirty: false,
            status: SaveStatus::Idle,
            closing: false,
            tag_input: String::new(),
            color_open: false,
        }
    }

    /// Solid `triggerSave` skips a new note that is still blank.
    fn needs_save(&self) -> bool {
        self.draft != self.saved && !(self.id.is_none() && self.draft.is_blank())
    }

    fn begin_save(&mut self) -> SaveRequest {
        self.saving = true;
        self.dirty = false;
        self.in_flight = Some(self.draft.clone());
        self.status = SaveStatus::Saving;
        SaveRequest {
            session: self.session,
            id: self.id.clone(),
            payload: self.draft.payload(),
        }
    }
}

/// The quick-create note form (Solid `CreateNoteModal`). Its fields live in the page; this is only its state.
struct Quick {
    session: u64,
    creating: bool,
}

#[derive(Clone)]
enum Confirm {
    Trash(String),
    Delete(String),
    BatchDelete,
}

impl Confirm {
    /// Title, body and confirm-button label, as in the Solid modals.
    fn copy(&self, selected: usize) -> ConfirmView {
        let (title, message, action) = match self {
            Confirm::Trash(_) => (
                "Move to trash".to_owned(),
                "Are you sure you want to move this note to the trash? You can restore it later.".to_owned(),
                "Move to Trash",
            ),
            Confirm::Delete(_) => (
                "Delete note".to_owned(),
                "Are you sure you want to permanently delete this note? This action cannot be undone.".to_owned(),
                "Delete Permanently",
            ),
            Confirm::BatchDelete => (
                "Delete selected notes".to_owned(),
                format!(
                    "Are you sure you want to permanently delete {selected} selected note(s)? This action cannot be undone."
                ),
                "Delete Permanently",
            ),
        };
        ConfirmView {
            title,
            message,
            action: action.to_owned(),
        }
    }
}

#[derive(Default)]
struct State {
    notes: Vec<Note>,
    loading: bool,
    error: Option<String>,
    tab: NoteTab,
    search: String,
    tag_filter: Option<String>,
    selected: Vec<String>,
    editor: Option<Editor>,
    quick: Option<Quick>,
    confirm: Option<Confirm>,
    sessions: u64,
}

impl State {
    fn next_session(&mut self) -> u64 {
        self.sessions += 1;
        self.sessions
    }

    fn card(&self, note: &Note) -> CardView {
        CardView {
            id: note.id.clone(),
            title: note.title.clone(),
            preview: note.content.lines().take(3).collect::<Vec<_>>().join("\n"),
            empty: note.content.is_empty(),
            tags: note.tags.clone(),
            date: note
                .updated_at
                .as_deref()
                .and_then(|t| t.get(..10))
                .unwrap_or("")
                .to_owned(),
            color: note.color.clone(),
            pinned: note.is_pinned,
            archived: note.is_archived,
            trashed: note.is_trashed,
            selected: self.selected.contains(&note.id),
        }
    }

    fn view(&self) -> NotesView {
        let query = self.search.trim().to_lowercase();
        let visible: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| {
                self.tab.includes(n)
                    && self.tag_filter.as_ref().is_none_or(|t| n.tags.contains(t))
                    && matches_query(n, &query)
            })
            .collect();
        let pinned = visible
            .iter()
            .copied()
            .filter(|n| n.is_pinned)
            .map(|n| self.card(n))
            .collect();
        let others = visible
            .iter()
            .copied()
            .filter(|n| !n.is_pinned)
            .map(|n| self.card(n))
            .collect();
        let tabs = NoteTab::ALL
            .iter()
            .map(|tab| {
                let count = self.notes.iter().filter(|n| tab.includes(n)).count();
                format!("{} ({count})", tab.label())
            })
            .collect();
        let editor = self
            .editor
            .as_ref()
            .filter(|e| !e.closing)
            .map(|e| EditorView {
                existing: e.id.is_some(),
                trashed: e
                    .id
                    .as_deref()
                    .and_then(|id| self.notes.iter().find(|n| n.id == id))
                    .is_some_and(|n| n.is_trashed),
                title: e.draft.title.clone(),
                content: e.draft.content.clone(),
                tags: e.draft.tags.clone(),
                tag_input: e.tag_input.clone(),
                color: e.draft.color.clone(),
                color_open: e.color_open,
                pinned: e.draft.pinned,
                archived: e.draft.archived,
                status: e.status.label().to_owned(),
            });
        NotesView {
            tabs,
            tab: self.tab,
            search: self.search.clone(),
            tags: all_tags(&self.notes),
            tag_filter: self.tag_filter.clone(),
            selected: self.selected.len(),
            loading: self.loading,
            no_notes: self.notes.is_empty(),
            error: self.error.clone(),
            pinned,
            others,
            editor,
            quick_open: self.quick.is_some(),
            quick_creating: self.quick.as_ref().is_some_and(|q| q.creating),
            confirm: self.confirm.as_ref().map(|c| c.copy(self.selected.len())),
        }
    }
}

type Notify = Arc<dyn Fn(NotesView) + Send + Sync>;

struct Shared {
    api: Arc<dyn NoteApi>,
    state: Mutex<State>,
    notify: Notify,
}

/// The Notes page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct NotesController {
    shared: Arc<Shared>,
}

impl NotesController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(api: Arc<dyn NoteApi>, notify: impl Fn(NotesView) + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::default(),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> NotesView {
        self.lock().view()
    }

    /// Fetches the list again. Shows the loading flag until the answer arrives.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared.api.list(Box::new(move |result| {
            me.change(|s| {
                s.loading = false;
                match result {
                    Ok(notes) => s.notes = notes,
                    Err(e) => s.error = Some(format!("Could not load notes: {e}")),
                }
            });
        }));
    }

    pub fn set_tab(&self, tab: NoteTab) {
        self.change(|s| s.tab = tab);
    }

    pub fn set_search(&self, search: &str) {
        self.change(|s| s.search = search.to_owned());
    }

    pub fn set_tag_filter(&self, tag: Option<String>) {
        self.change(|s| s.tag_filter = tag);
    }

    /// Adds `id` to the selection, or removes it when it is selected.
    pub fn toggle_select(&self, id: &str) {
        self.change(|s| {
            if let Some(index) = s.selected.iter().position(|x| x == id) {
                s.selected.remove(index);
            } else {
                s.selected.push(id.to_owned());
            }
        });
    }

    pub fn clear_selection(&self) {
        self.change(|s| s.selected.clear());
    }

    /// Runs a batch action on the selected notes, then fetches the list again and clears the selection. Does nothing
    /// when nothing is selected.
    pub fn batch(&self, action: NoteBatchAction) {
        let ids = self.lock().selected.clone();
        if ids.is_empty() {
            return;
        }
        let me = self.clone();
        self.shared.api.batch(
            action,
            &ids,
            Box::new(move |result| {
                me.change(|s| {
                    s.selected.clear();
                    if let Err(e) = &result {
                        s.error = Some(format!("Batch action failed: {e}"));
                    }
                });
                me.refresh();
            }),
        );
    }

    pub fn ask_batch_delete(&self) {
        self.change(|s| s.confirm = Some(Confirm::BatchDelete));
    }

    /// Opens the editor on the note `id`. Closes the current editor first (flushing its changes).
    pub fn open_note(&self, id: &str) {
        let next = self.change(|s| {
            let note = s.notes.iter().find(|n| n.id == id).cloned()?;
            let next = close_editor(s);
            let session = s.next_session();
            s.editor = Some(Editor::new(
                session,
                Some(note.id.clone()),
                Draft::from_note(&note),
            ));
            next
        });
        if let Some(request) = next {
            self.send_save(request);
        }
    }

    /// Opens a blank editor for a new note.
    pub fn new_note(&self) {
        let next = self.change(|s| {
            let next = close_editor(s);
            let session = s.next_session();
            s.editor = Some(Editor::new(session, None, Draft::blank()));
            next
        });
        if let Some(request) = next {
            self.send_save(request);
        }
    }

    /// Closes the editor: flushes pending changes, and closes once any in-flight save settles.
    pub fn request_close(&self) {
        let next = self.change(close_editor);
        if let Some(request) = next {
            self.send_save(request);
        }
    }

    /// Saves the open editor if its draft differs from the last saved one. Called by the glue's debounce timer.
    pub fn autosave(&self) {
        let next = self.change(|s| {
            s.editor
                .as_mut()
                .filter(|e| !e.saving && e.needs_save())
                .map(Editor::begin_save)
        });
        if let Some(request) = next {
            self.send_save(request);
        }
    }

    pub fn editor_title(&self, text: &str) {
        self.edit_editor(|ed| ed.draft.title = text.to_owned());
    }

    pub fn editor_content(&self, text: &str) {
        self.edit_editor(|ed| ed.draft.content = text.to_owned());
    }

    pub fn editor_tag_input(&self, text: &str) {
        self.change(|s| {
            if let Some(ed) = s.editor.as_mut() {
                ed.tag_input = text.to_owned();
            }
        });
    }

    /// Adds the typed tag (a leading `#` is dropped). Blank and duplicate tags are ignored.
    pub fn editor_add_tag(&self) {
        self.edit_editor(|ed| add_tag(&mut ed.draft.tags, &mut ed.tag_input));
    }

    pub fn editor_remove_tag(&self, index: usize) {
        self.edit_editor(|ed| {
            if index < ed.draft.tags.len() {
                ed.draft.tags.remove(index);
            }
        });
    }

    pub fn editor_toggle_color_open(&self) {
        self.change(|s| {
            if let Some(ed) = s.editor.as_mut() {
                ed.color_open = !ed.color_open;
            }
        });
    }

    /// Picks a colour. Saves at once, not after the debounce (as Solid does).
    pub fn editor_set_color(&self, color: &str) {
        self.edit_editor(|ed| {
            ed.draft.color = color.to_owned();
            ed.color_open = false;
        });
        self.autosave();
    }

    pub fn editor_toggle_pinned(&self) {
        self.edit_editor(|ed| ed.draft.pinned = !ed.draft.pinned);
    }

    pub fn editor_toggle_archived(&self) {
        self.edit_editor(|ed| ed.draft.archived = !ed.draft.archived);
    }

    /// The editor's Trash button: asks for confirmation.
    pub fn editor_ask_trash(&self) {
        if let Some(id) = self.editor_id() {
            self.change(|s| s.confirm = Some(Confirm::Trash(id)));
        }
    }

    /// The editor's Restore button, for a trashed note.
    pub fn editor_restore(&self) {
        if let Some(id) = self.editor_id() {
            self.update(
                &id,
                Flags {
                    trashed: Some(false),
                    ..Flags::default()
                },
            );
        }
    }

    /// The editor's Delete button, for a trashed note: asks for confirmation.
    pub fn editor_ask_delete(&self) {
        if let Some(id) = self.editor_id() {
            self.change(|s| s.confirm = Some(Confirm::Delete(id)));
        }
    }

    pub fn pin(&self, id: &str, pinned: bool) {
        self.update(
            id,
            Flags {
                pinned: Some(pinned),
                ..Flags::default()
            },
        );
    }

    pub fn archive(&self, id: &str, archived: bool) {
        self.update(
            id,
            Flags {
                archived: Some(archived),
                ..Flags::default()
            },
        );
    }

    /// The card's Trash button: moves the note to the trash at once, without confirmation (as Solid does).
    pub fn trash(&self, id: &str) {
        self.update(
            id,
            Flags {
                pinned: Some(false),
                trashed: Some(true),
                archived: None,
            },
        );
    }

    /// The card's Delete button (trash tab): asks for confirmation.
    pub fn ask_delete(&self, id: &str) {
        self.change(|s| s.confirm = Some(Confirm::Delete(id.to_owned())));
    }

    /// The confirmation's confirm button.
    pub fn accept_confirm(&self) {
        let confirm = self.change(|s| s.confirm.take());
        match confirm {
            Some(Confirm::Trash(id)) => {
                let editing = self.editing(&id);
                self.update(
                    &id,
                    Flags {
                        pinned: Some(false),
                        trashed: Some(true),
                        archived: None,
                    },
                );
                if editing {
                    self.change(|s| s.editor = None);
                }
            }
            Some(Confirm::Delete(id)) => {
                if self.editing(&id) {
                    self.change(|s| s.editor = None);
                }
                self.delete(&id);
            }
            Some(Confirm::BatchDelete) => self.batch(NoteBatchAction::Delete),
            None => {}
        }
    }

    /// The confirmation's cancel button, or its close.
    pub fn cancel_confirm(&self) {
        self.change(|s| s.confirm = None);
    }

    /// Opens the quick-create form. Does nothing when it is already open.
    pub fn open_quick(&self) {
        self.change(|s| {
            if s.quick.is_none() {
                let session = s.next_session();
                s.quick = Some(Quick {
                    session,
                    creating: false,
                });
            }
        });
    }

    /// Creates a note from the quick form. Refused (nothing is sent) when both title and content are blank or a save
    /// is already in flight. The form closes when the note is created, and stays open on failure.
    pub fn quick_submit(&self, title: &str, content: &str, tags: &str) {
        let job = self.change(|s| {
            let quick = s.quick.as_mut()?;
            if quick.creating || (title.trim().is_empty() && content.trim().is_empty()) {
                return None;
            }
            quick.creating = true;
            let draft = Draft {
                title: title.trim().to_owned(),
                content: content.trim().to_owned(),
                tags: parse_tags(tags),
                ..Draft::blank()
            };
            Some((quick.session, draft))
        });
        if let Some((session, draft)) = job {
            let me = self.clone();
            self.shared.api.create(
                draft,
                Box::new(move |result| me.settle_create(session, result)),
            );
        }
    }

    /// Closes the quick form (its cancel button, or its close).
    pub fn quick_cancel(&self) {
        self.change(|s| s.quick = None);
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in one callback must not make every later call panic too.
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change<R>(&self, update: impl FnOnce(&mut State) -> R) -> R {
        let (result, view) = {
            let mut state = self.lock();
            let result = update(&mut state);
            (result, state.view())
        };
        (self.shared.notify)(view);
        result
    }

    fn edit_editor(&self, edit: impl FnOnce(&mut Editor)) {
        self.change(|s| {
            if let Some(ed) = s.editor.as_mut() {
                ed.dirty |= ed.saving;
                edit(ed);
            }
        });
    }

    fn editor_id(&self) -> Option<String> {
        self.lock().editor.as_ref().and_then(|e| e.id.clone())
    }

    fn editing(&self, id: &str) -> bool {
        self.lock().editor.as_ref().and_then(|e| e.id.as_deref()) == Some(id)
    }

    /// Merges `flags` into the local note, then sends the PATCH. A failure shows an error (Solid only logs it).
    fn update(&self, id: &str, flags: Flags) {
        self.change(|s| {
            if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
                flags.apply(note);
            }
        });
        let me = self.clone();
        self.shared.api.update(
            flags.change(id),
            Box::new(move |result| {
                if let Err(e) = result {
                    me.change(|s| s.error = Some(format!("Update failed: {e}")));
                }
            }),
        );
    }

    /// Removes the note locally, then sends the DELETE.
    fn delete(&self, id: &str) {
        self.change(|s| s.notes.retain(|n| n.id != id));
        let me = self.clone();
        self.shared.api.delete(
            id,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.change(|s| s.error = Some(format!("Delete failed: {e}")));
                }
            }),
        );
    }

    fn send_save(&self, request: SaveRequest) {
        let me = self.clone();
        let session = request.session;
        match request.id {
            None => self.shared.api.create(
                request.payload,
                Box::new(move |result| me.settle_create(session, result)),
            ),
            Some(id) => {
                let payload = request.payload;
                let change = NoteChange {
                    id: id.clone(),
                    title: Some(payload.title.clone()),
                    content: Some(payload.content.clone()),
                    tags: Some(payload.tags.clone()),
                    color: Some(payload.color.clone()),
                    is_pinned: Some(payload.pinned),
                    is_archived: Some(payload.archived),
                    is_trashed: None,
                };
                self.shared.api.update(
                    change,
                    Box::new(move |result| me.settle_patch(session, id, payload, result)),
                );
            }
        }
    }

    /// Solid `handleSaveNote` for a new note: prepends the created note, then settles the editor or the quick form.
    fn settle_create(&self, session: u64, result: Result<Note, ApiError>) {
        let next = self.change(|s| match result {
            Ok(note) => {
                let id = note.id.clone();
                s.notes.insert(0, note);
                if s.quick.as_ref().is_some_and(|q| q.session == session) {
                    s.quick = None;
                }
                settle_editor(s, session, Some(id))
            }
            Err(e) => {
                s.error = Some(format!("Save failed: {e}"));
                if let Some(q) = s.quick.as_mut().filter(|q| q.session == session) {
                    q.creating = false;
                }
                settle_editor(s, session, None)
            }
        });
        if let Some(request) = next {
            self.send_save(request);
        }
    }

    /// Solid `handleSaveNote` for an existing note: updates the local note with what was saved, then settles.
    fn settle_patch(&self, session: u64, id: String, applied: Draft, result: Result<(), ApiError>) {
        let next = self.change(|s| match result {
            Ok(()) => {
                if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
                    note.title = applied.title;
                    note.content = applied.content;
                    note.tags = applied.tags;
                    note.color = applied.color;
                    note.is_pinned = applied.pinned;
                    note.is_archived = applied.archived;
                    note.updated_at = Some(now_iso());
                }
                settle_editor(s, session, Some(id))
            }
            Err(e) => {
                s.error = Some(format!("Save failed: {e}"));
                settle_editor(s, session, None)
            }
        });
        if let Some(request) = next {
            self.send_save(request);
        }
    }
}

/// Applies a save result to the editor that sent it. `saved` is the note id on success, `None` on failure. Returns
/// the next save to send: a change that arrived while the save was in flight, or a close that waited for it.
fn settle_editor(s: &mut State, session: u64, saved: Option<String>) -> Option<SaveRequest> {
    let ed = s.editor.as_mut().filter(|e| e.session == session)?;
    ed.saving = false;
    let sent = ed.in_flight.take();
    let succeeded = saved.is_some();
    match saved {
        Some(id) => {
            ed.id = Some(id);
            if let Some(sent) = sent {
                ed.saved = sent;
            }
            ed.status = SaveStatus::Saved;
        }
        None => ed.status = SaveStatus::Failed,
    }
    if ed.closing {
        if succeeded && ed.needs_save() {
            return Some(ed.begin_save());
        }
        s.editor = None;
        return None;
    }
    if ed.dirty && ed.needs_save() {
        return Some(ed.begin_save());
    }
    None
}

/// Closes the editor (Solid `handleClose`). Returns the save to send first, if the draft has changes. An in-flight
/// save is awaited first, so its result is not lost.
fn close_editor(s: &mut State) -> Option<SaveRequest> {
    let ed = s.editor.as_mut()?;
    ed.closing = true;
    if ed.saving {
        return None;
    }
    if ed.needs_save() {
        return Some(ed.begin_save());
    }
    s.editor = None;
    None
}

/// Trimmed text for the tag input, with one leading `#` removed (Solid `addModalTag`).
fn normalize_tag(input: &str) -> &str {
    let trimmed = input.trim();
    trimmed.strip_prefix('#').unwrap_or(trimmed)
}

/// Adds the tag typed in `input`, then clears `input`. Blank and duplicate tags are dropped.
pub fn add_tag(tags: &mut Vec<String>, input: &mut String) {
    let tag = normalize_tag(input).to_owned();
    input.clear();
    if !tag.is_empty() && !tags.contains(&tag) {
        tags.push(tag);
    }
}

/// Solid `CreateNoteModal` tags: comma separated, each trimmed with one leading `#` removed. Blanks are dropped.
fn parse_tags(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn all_tags(notes: &[Note]) -> Vec<String> {
    let tags: BTreeSet<&str> = notes
        .iter()
        .flat_map(|note| note.tags.iter())
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .collect();
    tags.into_iter().map(str::to_owned).collect()
}

/// `query` must already be trimmed and lowercased. Empty matches everything.
fn matches_query(note: &Note, query: &str) -> bool {
    query.is_empty()
        || note.title.to_lowercase().contains(query)
        || note.content.to_lowercase().contains(query)
        || note
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
}

/// Current UTC time in the server's `updatedAt` format, e.g. `2026-11-08T19:55:17.000Z`.
fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let in_day = secs.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        in_day / 3_600,
        in_day % 3_600 / 60,
        in_day % 60
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records each call and answers from scripted replies. When `hold` is set, answers wait until `release` runs.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        list: Mutex<Option<Result<Vec<Note>, ApiError>>>,
        creates: Mutex<VecDeque<Result<Note, ApiError>>>,
        writes: Mutex<VecDeque<Result<(), ApiError>>>,
        hold: Mutex<bool>,
        held: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn next_write(&self) -> Result<(), ApiError> {
            self.writes.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }

        /// Runs the answers held since `hold` was set, in the order the calls were made.
        fn release(&self) {
            *self.hold.lock().unwrap() = false;
            let held: Vec<_> = self.held.lock().unwrap().drain(..).collect();
            for answer in held {
                answer();
            }
        }

        fn answer<R: Send + 'static>(&self, reply: R, on_done: Box<dyn FnOnce(R) + Send>) {
            if *self.hold.lock().unwrap() {
                self.held
                    .lock()
                    .unwrap()
                    .push(Box::new(move || on_done(reply)));
            } else {
                on_done(reply);
            }
        }
    }

    impl NoteApi for Arc<Fake> {
        fn list(&self, on_done: Done<Vec<Note>>) {
            self.record("list".to_owned());
            let reply = self
                .list
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Ok(vec![note("a", "Pinned idea", true, false, false)]));
            self.answer(reply, on_done);
        }

        fn create(&self, draft: Draft, on_done: Done<Note>) {
            self.record(format!("create:{}:{}", draft.title, draft.content));
            let reply = self
                .creates
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(note("new-1", &draft.title, false, false, false)));
            self.answer(reply, on_done);
        }

        fn update(&self, change: NoteChange, on_done: Done<()>) {
            self.record(format!(
                "update:{}:pinned={:?}:archived={:?}:trashed={:?}:title={:?}:color={:?}",
                change.id,
                change.is_pinned,
                change.is_archived,
                change.is_trashed,
                change.title,
                change.color
            ));
            let reply = self.next_write();
            self.answer(reply, on_done);
        }

        fn delete(&self, id: &str, on_done: Done<()>) {
            self.record(format!("delete:{id}"));
            let reply = self.next_write();
            self.answer(reply, on_done);
        }

        fn batch(&self, action: NoteBatchAction, ids: &[String], on_done: Done<()>) {
            self.record(format!("batch:{action:?}:{}", ids.join(",")));
            let reply = self.next_write();
            self.answer(reply, on_done);
        }
    }

    fn note(id: &str, title: &str, pinned: bool, archived: bool, trashed: bool) -> Note {
        Note {
            id: id.to_owned(),
            title: title.to_owned(),
            content: format!("{title} body"),
            tags: vec!["work".to_owned()],
            color: "neutral".to_owned(),
            is_pinned: pinned,
            is_archived: archived,
            is_trashed: trashed,
            updated_at: Some("2026-11-08T19:55:17.000Z".to_owned()),
        }
    }

    fn controller(fake: &Arc<Fake>) -> (NotesController, Arc<Mutex<Vec<NotesView>>>) {
        let seen: Arc<Mutex<Vec<NotesView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn NoteApi> = Arc::new(fake.clone());
        let c = NotesController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    /// A controller with the list loaded: "Pinned idea" (pinned), plus an archived note and a trashed note.
    fn loaded() -> (Arc<Fake>, NotesController) {
        let fake: Arc<Fake> = Arc::default();
        *fake.list.lock().unwrap() = Some(Ok(vec![
            note("a", "Pinned idea", true, false, false),
            note("b", "Groceries", false, false, false),
            note("c", "Old plan", false, true, false),
            note("d", "Gone", false, false, true),
        ]));
        let (c, _) = controller(&fake);
        c.refresh();
        (fake, c)
    }

    #[test]
    fn refresh_loads_and_groups_cards_by_pin() {
        let (_fake, c) = loaded();
        let view = c.view();
        assert!(!view.loading);
        assert_eq!(view.pinned.len(), 1);
        assert_eq!(view.pinned[0].title, "Pinned idea");
        assert_eq!(
            view.others.len(),
            1,
            "the active tab shows only active notes"
        );
        assert_eq!(view.others[0].title, "Groceries");
        assert_eq!(view.tabs, ["Active (2)", "Archive (1)", "Trash (1)"]);
        assert_eq!(view.tags, ["work"]);
        assert_eq!(view.pinned[0].date, "2026-11-08");
    }

    #[test]
    fn loading_flag_is_shown_before_the_answer() {
        let fake: Arc<Fake> = Arc::default();
        let (c, seen) = controller(&fake);
        c.refresh();
        assert!(seen.lock().unwrap()[0].loading);
    }

    #[test]
    fn failed_refresh_sets_error_and_dismiss_clears_it() {
        let fake: Arc<Fake> = Arc::default();
        *fake.list.lock().unwrap() = Some(Err(ApiError::Transport("down".to_owned())));
        let (c, _) = controller(&fake);
        c.refresh();
        assert_eq!(
            c.view().error.as_deref(),
            Some("Could not load notes: network error: down")
        );
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn tab_search_and_tag_filter_narrow_the_list() {
        let (_fake, c) = loaded();
        c.set_tab(NoteTab::Archived);
        assert_eq!(
            c.view()
                .others
                .iter()
                .map(|n| n.title.as_str())
                .collect::<Vec<_>>(),
            ["Old plan"]
        );
        c.set_tab(NoteTab::Trashed);
        assert_eq!(
            c.view()
                .others
                .iter()
                .map(|n| n.title.as_str())
                .collect::<Vec<_>>(),
            ["Gone"]
        );

        c.set_tab(NoteTab::Active);
        c.set_search("  GROCER ");
        let view = c.view();
        assert!(view.pinned.is_empty());
        assert_eq!(
            view.others[0].title, "Groceries",
            "search ignores case and surrounding spaces"
        );

        c.set_search("");
        c.set_tag_filter(Some("nope".to_owned()));
        assert!(c.view().others.is_empty() && c.view().pinned.is_empty());
        c.set_tag_filter(Some("work".to_owned()));
        assert_eq!(c.view().pinned.len(), 1);
    }

    #[test]
    fn batch_sends_the_selected_ids_then_clears_and_refreshes() {
        let (fake, c) = loaded();
        c.toggle_select("b");
        c.toggle_select("a");
        c.toggle_select("a");
        assert_eq!(c.view().selected, 1);
        c.batch(NoteBatchAction::Archive);
        let calls = fake.calls();
        assert!(calls.contains(&"batch:Archive:b".to_owned()), "{calls:?}");
        assert_eq!(calls.last().unwrap(), "list");
        assert_eq!(c.view().selected, 0);
    }

    #[test]
    fn batch_with_nothing_selected_sends_nothing() {
        let (fake, c) = loaded();
        c.batch(NoteBatchAction::Trash);
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn pin_updates_locally_and_sends_the_flag() {
        let (fake, c) = loaded();
        c.pin("b", true);
        assert_eq!(
            c.view()
                .pinned
                .iter()
                .map(|n| n.title.as_str())
                .collect::<Vec<_>>(),
            ["Pinned idea", "Groceries"]
        );
        assert!(
            fake.calls().contains(
                &"update:b:pinned=Some(true):archived=None:trashed=None:title=None:color=None"
                    .to_owned()
            )
        );
    }

    #[test]
    fn card_trash_moves_without_confirmation() {
        let (fake, c) = loaded();
        c.trash("b");
        assert!(c.view().confirm.is_none());
        assert!(fake.calls().contains(&"update:b:pinned=Some(false):archived=None:trashed=Some(true):title=None:color=None".to_owned()));
        assert!(c.view().others.is_empty());
    }

    #[test]
    fn card_delete_asks_first_then_removes_locally() {
        let (fake, c) = loaded();
        c.ask_delete("d");
        assert_eq!(
            c.view().confirm.as_ref().map(|v| v.action.as_str()),
            Some("Delete Permanently")
        );
        c.cancel_confirm();
        assert!(c.view().confirm.is_none());
        assert!(!fake.calls().iter().any(|call| call.starts_with("delete")));

        c.ask_delete("d");
        c.accept_confirm();
        assert!(fake.calls().contains(&"delete:d".to_owned()));
        c.set_tab(NoteTab::Trashed);
        assert!(c.view().others.is_empty());
    }

    #[test]
    fn new_blank_note_is_closed_without_a_save() {
        let (fake, c) = loaded();
        c.new_note();
        assert!(c.view().editor.is_some());
        c.request_close();
        assert!(c.view().editor.is_none());
        assert_eq!(fake.calls(), ["list"], "a blank new note is not saved");
    }

    #[test]
    fn titled_new_note_is_created_and_then_patched_on_later_edits() {
        let (fake, c) = loaded();
        c.new_note();
        c.editor_title("  Shopping  ");
        c.autosave();
        assert!(
            fake.calls().contains(&"create:Shopping:".to_owned()),
            "{:?}",
            fake.calls()
        );
        let view = c.view();
        let editor = view.editor.expect("still open");
        assert!(editor.existing, "the create gave the editor its id");
        assert_eq!(editor.status, "Saved");
        assert_eq!(
            view.pinned.len() + view.others.len(),
            3,
            "the created note is prepended to the list"
        );

        c.editor_content("body");
        c.autosave();
        assert!(
            fake.calls().iter().any(|x| x.starts_with("update:new-1")),
            "{:?}",
            fake.calls()
        );
    }

    #[test]
    fn unchanged_editor_does_not_save() {
        let (fake, c) = loaded();
        c.open_note("b");
        c.autosave();
        c.request_close();
        assert_eq!(fake.calls(), ["list"]);
        assert!(c.view().editor.is_none());
    }

    #[test]
    fn colour_change_saves_at_once() {
        let (fake, c) = loaded();
        c.open_note("b");
        c.editor_toggle_color_open();
        c.editor_set_color("info");
        let view = c.view().editor.unwrap();
        assert_eq!(view.color, "info");
        assert!(!view.color_open);
        assert!(
            fake.calls()
                .iter()
                .any(|x| x.starts_with("update:b") && x.contains("color=Some(\"info\")"))
        );
    }

    #[test]
    fn tags_are_added_trimmed_and_removed() {
        let (_fake, c) = loaded();
        c.open_note("b");
        c.editor_tag_input("#home");
        c.editor_add_tag();
        c.editor_tag_input("  home ");
        c.editor_add_tag();
        let view = c.view().editor.unwrap();
        assert_eq!(
            view.tags,
            ["work", "home"],
            "duplicates are dropped, one leading # is removed"
        );
        assert_eq!(view.tag_input, "", "the input clears after a tag is added");
        c.editor_remove_tag(0);
        assert_eq!(c.view().editor.unwrap().tags, ["home"]);
    }

    #[test]
    fn edit_while_saving_is_sent_when_the_save_settles() {
        let (fake, c) = loaded();
        c.open_note("b");
        *fake.hold.lock().unwrap() = true;
        c.editor_title("first");
        c.autosave();
        c.editor_title("second");
        c.autosave();
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|x| x.starts_with("update:b"))
                .count(),
            1,
            "one save in flight"
        );
        fake.release();
        let updates: Vec<String> = fake
            .calls()
            .into_iter()
            .filter(|x| x.starts_with("update:b"))
            .collect();
        assert_eq!(
            updates.len(),
            2,
            "the newest draft is sent once the first save settles"
        );
        assert!(updates[1].contains("title=Some(\"second\")"), "{updates:?}");
        assert_eq!(c.view().editor.unwrap().status, "Saved");
    }

    #[test]
    fn closing_waits_for_the_save_in_flight() {
        let (fake, c) = loaded();
        c.open_note("b");
        c.editor_title("kept");
        *fake.hold.lock().unwrap() = true;
        c.request_close();
        assert!(c.view().editor.is_none(), "a closing editor is hidden");
        fake.release();
        assert!(c.view().editor.is_none());
        assert!(
            fake.calls()
                .iter()
                .any(|x| x.starts_with("update:b") && x.contains("title=Some(\"kept\")"))
        );
        assert!(
            c.view().others.iter().any(|n| n.title == "kept"),
            "the settled save updates the list"
        );
    }

    #[test]
    fn failed_save_keeps_the_editor_open_with_a_banner() {
        let (fake, c) = loaded();
        c.open_note("b");
        fake.writes.lock().unwrap().push_back(Err(ApiError::Status {
            code: 500,
            body: "boom".to_owned(),
        }));
        c.editor_title("changed");
        c.autosave();
        let view = c.view();
        assert_eq!(
            view.editor.as_ref().map(|e| e.status.as_str()),
            Some("Save failed")
        );
        assert_eq!(view.error.as_deref(), Some("Save failed: HTTP 500: boom"));
    }

    #[test]
    fn editor_trash_asks_then_closes_the_editor() {
        let (fake, c) = loaded();
        c.open_note("b");
        c.editor_ask_trash();
        assert!(c.view().confirm.is_some());
        c.accept_confirm();
        assert!(c.view().editor.is_none());
        assert!(
            fake.calls()
                .iter()
                .any(|x| x.starts_with("update:b") && x.contains("trashed=Some(true)"))
        );
    }

    #[test]
    fn editor_restore_keeps_the_editor_open() {
        let (fake, c) = loaded();
        c.open_note("d");
        assert!(c.view().editor.unwrap().trashed);
        c.editor_restore();
        assert!(c.view().editor.is_some());
        assert!(
            fake.calls()
                .iter()
                .any(|x| x.starts_with("update:d") && x.contains("trashed=Some(false)"))
        );
    }

    #[test]
    fn quick_create_refuses_blank_input() {
        let (fake, c) = loaded();
        c.open_quick();
        c.quick_submit("   ", " ", "x");
        assert!(c.view().quick_open);
        assert!(!fake.calls().iter().any(|x| x.starts_with("create")));
    }

    #[test]
    fn quick_create_closes_on_success_and_stays_open_on_failure() {
        let (fake, c) = loaded();
        c.open_quick();
        c.quick_submit(" Idea ", "text", "one, #two,, one");
        assert!(!c.view().quick_open);
        assert!(fake.calls().contains(&"create:Idea:text".to_owned()));

        c.open_quick();
        fake.creates
            .lock()
            .unwrap()
            .push_back(Err(ApiError::Transport("down".to_owned())));
        c.quick_submit("Again", "", "");
        let view = c.view();
        assert!(view.quick_open, "a failed save keeps the form for a retry");
        assert!(!view.quick_creating);
        assert_eq!(
            view.error.as_deref(),
            Some("Save failed: network error: down")
        );
    }

    #[test]
    fn quick_tags_are_comma_separated() {
        assert_eq!(parse_tags(" a, #b ,, ,c"), ["a", "b", "c"]);
    }

    #[test]
    fn quick_create_shows_creating_while_in_flight() {
        let (fake, c) = loaded();
        c.open_quick();
        *fake.hold.lock().unwrap() = true;
        c.quick_submit("Soon", "", "");
        assert!(c.view().quick_creating);
        c.quick_submit("Twice", "", "");
        fake.release();
        assert!(!c.view().quick_open);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|x| x.starts_with("create"))
                .count(),
            1
        );
    }

    #[test]
    fn tab_index_round_trips() {
        for tab in NoteTab::ALL {
            assert_eq!(NoteTab::from_index(tab.index()), Some(tab));
        }
        assert_eq!(NoteTab::from_index(3), None);
    }

    #[test]
    fn add_tag_strips_one_hash_and_drops_blanks_and_duplicates() {
        let mut tags = Vec::new();
        let mut input = "#alpha".to_owned();
        add_tag(&mut tags, &mut input);
        assert_eq!(tags, ["alpha"]);
        let mut input = "   ".to_owned();
        add_tag(&mut tags, &mut input);
        let mut input = "alpha".to_owned();
        add_tag(&mut tags, &mut input);
        assert_eq!(tags, ["alpha"]);
        let mut input = "##beta".to_owned();
        add_tag(&mut tags, &mut input);
        assert_eq!(tags, ["alpha", "#beta"], "only one leading # is removed");
    }

    #[test]
    fn payload_trims_and_falls_back_to_untitled() {
        let mut draft = Draft::blank();
        draft.title = "   ".to_owned();
        draft.content = "  body \n".to_owned();
        let payload = draft.payload();
        assert_eq!(payload.title, "Untitled");
        assert_eq!(payload.content, "body");
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn now_iso_uses_the_server_timestamp_format() {
        let stamp = now_iso();
        assert_eq!(stamp.len(), 24, "{stamp}");
        assert!(stamp.ends_with(".000Z"), "{stamp}");
        assert_eq!(&stamp[10..11], "T", "{stamp}");
    }
}
