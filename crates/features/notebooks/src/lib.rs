//! The Notebooks page controller (ported from `crates/app/src/notebooks.rs`).
//!
//! Plain Rust with no Slint. The controller holds the page state (tab, search, the notebook list, the create form, the
//! open thread with its drafts, and the pending confirmation), calls the Notebooks endpoints through [`NotebookApi`],
//! and reports each state change as a [`NotebooksView`]. The Slint glue in `apps/desktop/src/pages/notebooks.rs` turns
//! those views into the models of `ui/notebooks.slint`.
//!
//! Drafts (the create form's fields, the thread title, the entry being added, the entry being edited) live here, not in
//! the Slint properties, so a rebuilt view never loses what the user is typing.
//!
//! Threading: the API calls return at once and answer later on another thread (ehttp's worker). The controller locks
//! its state only around reads and writes, never across an API call, and `notify` is called after the lock is released.
//! The notify callback must hand the view to the UI thread itself (the glue uses `Weak::upgrade_in_event_loop`).

use std::sync::{Arc, Mutex, MutexGuard};

use idantity_core::local_date;
use idantity_core::notebooks::{EntryPatch, Notebook, NotebookEntry, NotebookPatch};
use idantity_core::{ApiClient, ApiError};

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@idantity_notebooks`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The answer callback of one API call. It runs on ehttp's worker thread.
pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait NotebookApi: Send + Sync {
    fn list(&self, on_done: Done<Vec<Notebook>>);
    fn create(&self, title: &str, content: &str, on_done: Done<Notebook>);
    fn update(&self, change: &NotebookChange, on_done: Done<()>);
    fn delete(&self, id: &str, on_done: Done<()>);
    fn entries(&self, notebook_id: &str, on_done: Done<Vec<NotebookEntry>>);
    fn add_entry(&self, notebook_id: &str, content: &str, on_done: Done<NotebookEntry>);
    fn update_entry(&self, notebook_id: &str, entry_id: &str, content: &str, on_done: Done<()>);
    fn delete_entry(&self, notebook_id: &str, entry_id: &str, on_done: Done<()>);
}

impl NotebookApi for ApiClient {
    fn list(&self, on_done: Done<Vec<Notebook>>) {
        ApiClient::notebooks(self, on_done);
    }

    fn create(&self, title: &str, content: &str, on_done: Done<Notebook>) {
        ApiClient::create_notebook(self, title, content, on_done);
    }

    fn update(&self, change: &NotebookChange, on_done: Done<()>) {
        let patch = NotebookPatch {
            id: &change.id,
            title: change.title.as_deref(),
            is_pinned: change.pinned,
            is_archived: change.archived,
            is_trashed: change.trashed,
            ..NotebookPatch::default()
        };
        ApiClient::update_notebook(self, &patch, on_done);
    }

    fn delete(&self, id: &str, on_done: Done<()>) {
        ApiClient::delete_notebook(self, id, on_done);
    }

    fn entries(&self, notebook_id: &str, on_done: Done<Vec<NotebookEntry>>) {
        ApiClient::notebook_entries(self, notebook_id, on_done);
    }

    fn add_entry(&self, notebook_id: &str, content: &str, on_done: Done<NotebookEntry>) {
        ApiClient::create_notebook_entry(self, notebook_id, content, on_done);
    }

    fn update_entry(&self, notebook_id: &str, entry_id: &str, content: &str, on_done: Done<()>) {
        let patch = EntryPatch {
            id: entry_id,
            content,
            notebook_id,
        };
        ApiClient::update_notebook_entry(self, &patch, on_done);
    }

    fn delete_entry(&self, notebook_id: &str, entry_id: &str, on_done: Done<()>) {
        ApiClient::delete_notebook_entry(self, entry_id, notebook_id, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn NotebookApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The fields a notebook action changes. `None` leaves a field alone, as Solid's `updateNotebookDirect` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotebookChange {
    pub id: String,
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub trashed: Option<bool>,
}

impl NotebookChange {
    fn for_id(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            ..Self::default()
        }
    }

    /// Merges the set fields into `notebook`, so the list shows the change before the server answers.
    fn apply(&self, notebook: &mut Notebook) {
        if let Some(title) = &self.title {
            notebook.title = title.clone();
        }
        if let Some(value) = self.pinned {
            notebook.is_pinned = value;
        }
        if let Some(value) = self.archived {
            notebook.is_archived = value;
        }
        if let Some(value) = self.trashed {
            notebook.is_trashed = value;
        }
    }
}

/// The tabs of the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotebookTab {
    #[default]
    Active,
    Archived,
    Trashed,
}

impl NotebookTab {
    pub const ALL: [NotebookTab; 3] = [
        NotebookTab::Active,
        NotebookTab::Archived,
        NotebookTab::Trashed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NotebookTab::Active => "Active",
            NotebookTab::Archived => "Archive",
            NotebookTab::Trashed => "Trash",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    fn includes(self, notebook: &Notebook) -> bool {
        match self {
            NotebookTab::Active => !notebook.is_archived && !notebook.is_trashed,
            NotebookTab::Archived => notebook.is_archived && !notebook.is_trashed,
            NotebookTab::Trashed => notebook.is_trashed,
        }
    }
}

/// One notebook card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub id: String,
    /// The title, or "Untitled" when it is empty.
    pub title: String,
    /// "1 entry" or "N entries".
    pub count: String,
    /// The `YYYY-MM-DD` of the last update, or empty.
    pub date: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed: bool,
}

/// One entry of the open thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryView {
    pub id: String,
    /// The `YYYY-MM-DD` the entry was created, or empty.
    pub date: String,
    pub content: String,
    /// True while this entry is being edited. Its text is in [`ThreadView::edit_text`].
    pub editing: bool,
}

/// The open thread: its title, entries, the add draft and the edit draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadView {
    pub id: String,
    /// The title field's text (a draft until the rename is committed).
    pub title: String,
    pub entries: Vec<EntryView>,
    pub entries_loading: bool,
    /// "2 entries in this thread".
    pub count: String,
    /// The new-entry field's text.
    pub draft: String,
    pub adding: bool,
    /// The text of the entry being edited.
    pub edit_text: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed: bool,
}

/// The create form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateView {
    pub title: String,
    pub content: String,
    pub creating: bool,
}

/// Which confirmation is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    Trash,
    Delete,
}

/// The confirmation dialog's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmView {
    pub kind: ConfirmKind,
    pub title: String,
    pub message: String,
    pub action: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotebooksView {
    /// The tab labels with their counts, "Active (2)".
    pub tabs: Vec<String>,
    /// The selected tab, as an index into [`NotebookTab::ALL`].
    pub tab: usize,
    pub search: String,
    /// The list is loading and nothing is shown yet.
    pub loading: bool,
    /// No notebook matches the tab and search (and nothing is loading).
    pub empty: bool,
    /// The message of the last failed call, until the user dismisses it.
    pub error: Option<String>,
    pub pinned: Vec<CardView>,
    pub others: Vec<CardView>,
    pub thread: Option<ThreadView>,
    pub create: Option<CreateView>,
    pub confirm: Option<ConfirmView>,
}

#[derive(Default, Clone)]
struct Create {
    title: String,
    content: String,
    creating: bool,
}

#[derive(Clone)]
struct Editing {
    entry_id: String,
    text: String,
    saving: bool,
}

#[derive(Clone)]
struct Thread {
    id: String,
    title: String,
    entries: Vec<NotebookEntry>,
    entries_loading: bool,
    draft: String,
    adding: bool,
    editing: Option<Editing>,
}

#[derive(Clone)]
struct Confirm {
    kind: ConfirmKind,
    id: String,
}

#[derive(Default)]
struct State {
    notebooks: Vec<Notebook>,
    loading: bool,
    error: Option<String>,
    tab: NotebookTab,
    search: String,
    create: Option<Create>,
    thread: Option<Thread>,
    confirm: Option<Confirm>,
}

impl State {
    fn view(&self) -> NotebooksView {
        let query = self.search.trim().to_lowercase();
        let visible: Vec<&Notebook> = self
            .notebooks
            .iter()
            .filter(|n| self.tab.includes(n))
            .filter(|n| query.is_empty() || n.title.to_lowercase().contains(&query))
            .collect();
        let card = |n: &&Notebook| CardView {
            id: n.id.clone(),
            title: shown_title(&n.title),
            count: count_text(n.entry_count),
            date: local_date_text(n.updated_at.as_deref()),
            pinned: n.is_pinned,
            archived: n.is_archived,
            trashed: n.is_trashed,
        };
        let loading = self.loading && self.notebooks.is_empty();

        NotebooksView {
            tabs: NotebookTab::ALL
                .iter()
                .map(|tab| {
                    let count = self.notebooks.iter().filter(|n| tab.includes(n)).count();
                    format!("{} ({count})", tab.label())
                })
                .collect(),
            tab: self.tab.index(),
            search: self.search.clone(),
            loading,
            empty: visible.is_empty() && !loading,
            error: self.error.clone(),
            pinned: visible.iter().filter(|n| n.is_pinned).map(card).collect(),
            others: visible.iter().filter(|n| !n.is_pinned).map(card).collect(),
            thread: self.thread.as_ref().and_then(|t| self.thread_view(t)),
            create: self.create.as_ref().map(|c| CreateView {
                title: c.title.clone(),
                content: c.content.clone(),
                creating: c.creating,
            }),
            confirm: self.confirm.as_ref().map(confirm_view),
        }
    }

    /// `None` when the open notebook is no longer in the list.
    fn thread_view(&self, thread: &Thread) -> Option<ThreadView> {
        let notebook = notebook(&self.notebooks, &thread.id)?;
        let entries: Vec<EntryView> = thread
            .entries
            .iter()
            .map(|entry| EntryView {
                id: entry.id.clone(),
                date: local_date_text(entry.created_at.as_deref()),
                content: entry.content.clone(),
                editing: thread
                    .editing
                    .as_ref()
                    .is_some_and(|e| e.entry_id == entry.id),
            })
            .collect();
        let noun = if entries.len() == 1 {
            "entry"
        } else {
            "entries"
        };
        Some(ThreadView {
            id: thread.id.clone(),
            title: thread.title.clone(),
            count: format!("{} {noun} in this thread", entries.len()),
            entries,
            entries_loading: thread.entries_loading,
            draft: thread.draft.clone(),
            adding: thread.adding,
            edit_text: thread
                .editing
                .as_ref()
                .map(|e| e.text.clone())
                .unwrap_or_default(),
            pinned: notebook.is_pinned,
            archived: notebook.is_archived,
            trashed: notebook.is_trashed,
        })
    }
}

fn confirm_view(confirm: &Confirm) -> ConfirmView {
    match confirm.kind {
        ConfirmKind::Trash => ConfirmView {
            kind: ConfirmKind::Trash,
            title: "Move to Trash".to_owned(),
            message: "Are you sure you want to move this notebook to the trash? You can restore it later.".to_owned(),
            action: "Move to Trash".to_owned(),
        },
        ConfirmKind::Delete => ConfirmView {
            kind: ConfirmKind::Delete,
            title: "Delete Notebook".to_owned(),
            message: "Are you sure you want to permanently delete this notebook and all of its entries? This action cannot be undone.".to_owned(),
            action: "Delete Permanently".to_owned(),
        },
    }
}

fn notebook<'a>(notebooks: &'a [Notebook], id: &str) -> Option<&'a Notebook> {
    notebooks.iter().find(|n| n.id == id)
}

/// The open thread, when it is the one with this id.
fn thread_mut<'a>(state: &'a mut State, id: &str) -> Option<&'a mut Thread> {
    state.thread.as_mut().filter(|t| t.id == id)
}

/// Solid shows `nb.title || "Untitled"`.
fn shown_title(title: &str) -> String {
    if title.is_empty() {
        "Untitled".to_owned()
    } else {
        title.to_owned()
    }
}

fn count_text(entries: u32) -> String {
    if entries == 1 {
        "1 entry".to_owned()
    } else {
        format!("{entries} entries")
    }
}

/// Solid `toLocaleDateString` (en-US month/day/year), in the device's time zone, as the egui page showed it.
fn local_date_text(value: Option<&str>) -> String {
    value
        .and_then(local_date::local_day_of)
        .map(|day| format!("{}/{}/{}", day.month, day.day, day.year))
        .unwrap_or_default()
}

fn error_text(what: &str, error: &ApiError) -> String {
    format!("{what}: {error}")
}

/// The page's state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct NotebooksController {
    shared: Arc<Shared>,
}

type Notify = Arc<dyn Fn(NotebooksView) + Send + Sync>;

struct Shared {
    api: Arc<dyn NotebookApi>,
    state: Mutex<State>,
    notify: Notify,
}

impl NotebooksController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn NotebookApi>,
        notify: impl Fn(NotebooksView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::default(),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> NotebooksView {
        self.lock().view()
    }

    /// Fetches the list again. Shows the loading flag until the answer arrives. This is the `refresh-notebooks` hook: the
    /// shell calls it when another part of the app (the FAB's New Notebook quick modal) changes notebooks.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared.api.list(Box::new(move |result| {
            me.change(|s| {
                s.loading = false;
                match result {
                    Ok(list) => s.notebooks = list,
                    Err(e) => s.error = Some(error_text("Failed to load notebooks", &e)),
                }
            });
        }));
    }

    pub fn set_tab(&self, tab: NotebookTab) {
        self.change(|s| s.tab = tab);
    }

    pub fn set_search(&self, search: &str) {
        self.change(|s| s.search = search.to_owned());
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    /// Opens the create form, empty.
    pub fn open_create(&self) {
        self.change(|s| s.create = Some(Create::default()));
    }

    pub fn create_title(&self, text: &str) {
        self.change(|s| {
            if let Some(c) = s.create.as_mut() {
                c.title = text.to_owned();
            }
        });
    }

    pub fn create_content(&self, text: &str) {
        self.change(|s| {
            if let Some(c) = s.create.as_mut() {
                c.content = text.to_owned();
            }
        });
    }

    /// Closes the create form and drops its fields.
    pub fn close_create(&self) {
        self.change(|s| s.create = None);
    }

    /// Creates a notebook from the form. Returns `false`, and calls nothing, when the form is closed, already creating,
    /// or blank in both fields. A blank title becomes "Untitled". On success the new notebook opens as a thread.
    pub fn create(&self) -> bool {
        let plan = self.change(|s| {
            let c = s.create.as_mut()?;
            let title = c.title.trim().to_owned();
            let content = c.content.trim().to_owned();
            if c.creating || (title.is_empty() && content.is_empty()) {
                return None;
            }
            c.creating = true;
            let title = if title.is_empty() {
                "Untitled".to_owned()
            } else {
                title
            };
            Some((title, content))
        });
        let Some((title, content)) = plan else {
            return false;
        };
        let me = self.clone();
        self.shared.api.create(
            &title,
            &content,
            Box::new(move |result| match result {
                Ok(notebook) => {
                    let id = notebook.id.clone();
                    me.change(|s| {
                        s.notebooks.insert(0, notebook);
                        s.create = None;
                    });
                    me.open(&id);
                }
                Err(e) => me.change(|s| {
                    if let Some(c) = s.create.as_mut() {
                        c.creating = false;
                    }
                    s.error = Some(error_text("Create failed", &e));
                }),
            }),
        );
        true
    }

    /// Opens the thread of notebook `id` and loads its entries.
    pub fn open(&self, id: &str) {
        self.change(|s| {
            let title = notebook(&s.notebooks, id)
                .map(|n| shown_title(&n.title))
                .unwrap_or_default();
            s.thread = Some(Thread {
                id: id.to_owned(),
                title,
                entries: Vec::new(),
                entries_loading: true,
                draft: String::new(),
                adding: false,
                editing: None,
            });
        });
        self.load_entries(id);
    }

    /// Fetches the entries of the open thread `id` again. The drafts are kept. Ignored when that thread is not open.
    pub fn load_entries(&self, id: &str) {
        self.change(|s| {
            if let Some(t) = thread_mut(s, id) {
                t.entries_loading = true;
            }
        });
        let me = self.clone();
        let key = id.to_owned();
        self.shared.api.entries(
            id,
            Box::new(move |result| {
                me.change(|s| {
                    // A late answer for a thread that was closed or replaced is dropped.
                    if thread_mut(s, &key).is_none() {
                        return;
                    }
                    let error = match result {
                        Ok(list) => {
                            if let Some(t) = thread_mut(s, &key) {
                                t.entries = list;
                            }
                            None
                        }
                        Err(e) => Some(error_text("Failed to load entries", &e)),
                    };
                    if let Some(t) = thread_mut(s, &key) {
                        t.entries_loading = false;
                    }
                    if error.is_some() {
                        s.error = error;
                    }
                });
            }),
        );
    }

    pub fn close_thread(&self) {
        self.change(|s| {
            s.thread = None;
            s.confirm = None;
        });
    }

    /// The thread title field's text, before it is committed.
    pub fn thread_title(&self, text: &str) {
        self.change(|s| {
            if let Some(t) = s.thread.as_mut() {
                t.title = text.to_owned();
            }
        });
    }

    /// Commits the thread title (Enter in the field). Solid `handleRenameNotebook`: an empty value reverts to the stored
    /// title; an unchanged one is skipped; otherwise the notebook is renamed.
    pub fn rename_thread(&self, text: &str) {
        let plan = self.change(|s| {
            let id = s.thread.as_ref()?.id.clone();
            let stored = notebook(&s.notebooks, &id).map(|n| shown_title(&n.title))?;
            let title = text.trim().to_owned();
            let t = s.thread.as_mut()?;
            if title.is_empty() {
                t.title = stored;
                return None;
            }
            if title == stored {
                t.title = title;
                return None;
            }
            t.title = title.clone();
            Some((id, title))
        });
        if let Some((id, title)) = plan {
            // Solid reloads the list and the entries after a rename (decision ak): the server's title replaces the
            // local one, and the thread's entries are fetched again. The title draft is kept.
            self.update(
                NotebookChange {
                    title: Some(title),
                    ..NotebookChange::for_id(&id)
                },
                true,
            );
        }
    }

    pub fn thread_draft(&self, text: &str) {
        self.change(|s| {
            if let Some(t) = s.thread.as_mut() {
                t.draft = text.to_owned();
            }
        });
    }

    /// Adds the draft as an entry of the open thread (Solid `handleAddEntry`). Returns `false`, and calls nothing, when
    /// the draft is blank or an add is already running. The draft clears when the entry is saved.
    pub fn add_entry(&self) -> bool {
        let plan = self.change(|s| {
            let t = s.thread.as_mut()?;
            let content = t.draft.trim().to_owned();
            if content.is_empty() || t.adding {
                return None;
            }
            t.adding = true;
            Some((t.id.clone(), content))
        });
        let Some((id, content)) = plan else {
            return false;
        };
        let me = self.clone();
        let key = id.clone();
        self.shared.api.add_entry(
            &id,
            &content,
            Box::new(move |result| {
                me.change(|s| {
                    let error = match result {
                        Ok(entry) => {
                            if let Some(t) = thread_mut(s, &key) {
                                t.entries.push(entry);
                                t.draft.clear();
                            }
                            None
                        }
                        Err(e) => Some(error_text("Add failed", &e)),
                    };
                    if let Some(t) = thread_mut(s, &key) {
                        t.adding = false;
                    }
                    if error.is_some() {
                        s.error = error;
                    }
                });
                me.refresh();
            }),
        );
        true
    }

    /// Starts editing an entry of the open thread, with its current text.
    pub fn start_edit(&self, entry_id: &str) {
        self.change(|s| {
            if let Some(t) = s.thread.as_mut() {
                let editing = t
                    .entries
                    .iter()
                    .find(|e| e.id == entry_id)
                    .map(|e| Editing {
                        entry_id: e.id.clone(),
                        text: e.content.clone(),
                        saving: false,
                    });
                if editing.is_some() {
                    t.editing = editing;
                }
            }
        });
    }

    pub fn edit_text(&self, text: &str) {
        self.change(|s| {
            if let Some(e) = s.thread.as_mut().and_then(|t| t.editing.as_mut()) {
                e.text = text.to_owned();
            }
        });
    }

    pub fn cancel_edit(&self) {
        self.change(|s| {
            if let Some(t) = s.thread.as_mut() {
                t.editing = None;
            }
        });
    }

    /// Saves the edited entry (Solid `handleSaveEdit`). Returns `false`, and calls nothing, when the text is blank or a
    /// save is already running.
    pub fn save_edit(&self) -> bool {
        let plan = self.change(|s| {
            let t = s.thread.as_mut()?;
            let id = t.id.clone();
            let e = t.editing.as_mut()?;
            let text = e.text.trim().to_owned();
            if text.is_empty() || e.saving {
                return None;
            }
            e.saving = true;
            Some((id, e.entry_id.clone(), text))
        });
        let Some((notebook_id, entry_id, text)) = plan else {
            return false;
        };
        let me = self.clone();
        let key = notebook_id.clone();
        let entry_key = entry_id.clone();
        let saved = text.clone();
        self.shared.api.update_entry(
            &notebook_id,
            &entry_id,
            &text,
            Box::new(move |result| {
                me.change(|s| {
                    let error = match result {
                        Ok(()) => {
                            if let Some(t) = thread_mut(s, &key) {
                                if let Some(entry) =
                                    t.entries.iter_mut().find(|e| e.id == entry_key)
                                {
                                    entry.content = saved;
                                }
                                if t.editing.as_ref().is_some_and(|e| e.entry_id == entry_key) {
                                    t.editing = None;
                                }
                            }
                            None
                        }
                        Err(e) => {
                            if let Some(e2) = thread_mut(s, &key).and_then(|t| t.editing.as_mut()) {
                                e2.saving = false;
                            }
                            Some(error_text("Save failed", &e))
                        }
                    };
                    if error.is_some() {
                        s.error = error;
                    }
                });
                me.refresh();
            }),
        );
        true
    }

    /// Removes one entry of the open thread. No confirmation, as in the egui page.
    pub fn delete_entry(&self, entry_id: &str) {
        let notebook_id = self.change(|s| {
            let t = s.thread.as_mut()?;
            t.entries.retain(|e| e.id != entry_id);
            if t.editing.as_ref().is_some_and(|e| e.entry_id == entry_id) {
                t.editing = None;
            }
            Some(t.id.clone())
        });
        let Some(notebook_id) = notebook_id else {
            return;
        };
        let me = self.clone();
        self.shared.api.delete_entry(
            &notebook_id,
            entry_id,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.change(|s| s.error = Some(error_text("Delete entry failed", &e)));
                }
                me.refresh();
            }),
        );
    }

    /// Flips the pinned flag of notebook `id`.
    pub fn toggle_pin(&self, id: &str) {
        if let Some(pinned) = self.flag(id, |n| !n.is_pinned) {
            self.update(
                NotebookChange {
                    pinned: Some(pinned),
                    ..NotebookChange::for_id(id)
                },
                false,
            );
        }
    }

    /// Flips the archived flag of notebook `id`.
    pub fn toggle_archive(&self, id: &str) {
        if let Some(archived) = self.flag(id, |n| !n.is_archived) {
            self.update(
                NotebookChange {
                    archived: Some(archived),
                    ..NotebookChange::for_id(id)
                },
                false,
            );
        }
    }

    /// Moves notebook `id` to the trash, unpinned.
    pub fn trash(&self, id: &str) {
        self.update(
            NotebookChange {
                pinned: Some(false),
                trashed: Some(true),
                ..NotebookChange::for_id(id)
            },
            false,
        );
    }

    /// Takes a trashed notebook back out of the trash.
    pub fn restore(&self, id: &str) {
        self.update(
            NotebookChange {
                trashed: Some(false),
                ..NotebookChange::for_id(id)
            },
            false,
        );
    }

    /// Asks before trashing (the thread's Trash button).
    pub fn ask_trash(&self, id: &str) {
        self.ask(ConfirmKind::Trash, id);
    }

    /// Asks before deleting for good.
    pub fn ask_delete(&self, id: &str) {
        self.ask(ConfirmKind::Delete, id);
    }

    pub fn confirm_cancel(&self) {
        self.change(|s| s.confirm = None);
    }

    /// Runs the pending confirmation. Trashing closes the thread when it is the open one. Deleting removes the notebook
    /// locally, then calls the API.
    pub fn confirm_accept(&self) {
        let Some(confirm) = self.change(|s| s.confirm.take()) else {
            return;
        };
        match confirm.kind {
            ConfirmKind::Trash => {
                self.close_if(&confirm.id);
                self.trash(&confirm.id);
            }
            ConfirmKind::Delete => {
                let id = confirm.id.clone();
                self.change(|s| {
                    s.notebooks.retain(|n| n.id != id);
                    if s.thread.as_ref().is_some_and(|t| t.id == id) {
                        s.thread = None;
                    }
                });
                let me = self.clone();
                self.shared.api.delete(
                    &confirm.id,
                    Box::new(move |result| {
                        if let Err(e) = result {
                            me.change(|s| s.error = Some(error_text("Delete failed", &e)));
                        }
                    }),
                );
            }
        }
    }

    fn ask(&self, kind: ConfirmKind, id: &str) {
        self.change(|s| {
            s.confirm = Some(Confirm {
                kind,
                id: id.to_owned(),
            })
        });
    }

    fn close_if(&self, id: &str) {
        self.change(|s| {
            if s.thread.as_ref().is_some_and(|t| t.id == id) {
                s.thread = None;
            }
        });
    }

    /// Reads a flag of notebook `id` and returns the value that replaces it.
    fn flag(&self, id: &str, next: impl Fn(&Notebook) -> bool) -> Option<bool> {
        let state = self.lock();
        notebook(&state.notebooks, id).map(next)
    }

    /// Merges `edit` into the list at once, then sends it. A failure shows a banner and keeps the local change, as the
    /// egui page did.
    /// `reload` also fetches the list and the open thread's entries once the write answers (decision ak).
    fn update(&self, edit: NotebookChange, reload: bool) {
        self.change(|s| {
            if let Some(n) = s.notebooks.iter_mut().find(|n| n.id == edit.id) {
                edit.apply(n);
            }
        });
        let me = self.clone();
        let id = edit.id.clone();
        self.shared.api.update(
            &edit,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.change(|s| s.error = Some(error_text("Update failed", &e)));
                }
                if reload {
                    me.refresh();
                    me.load_entries(&id);
                }
            }),
        );
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records each call and answers from scripted replies. Writes succeed unless a reply is queued.
    struct Fake {
        calls: Mutex<Vec<String>>,
        list: Mutex<Result<Vec<Notebook>, ApiError>>,
        entries: Mutex<Result<Vec<NotebookEntry>, ApiError>>,
        writes: Mutex<VecDeque<Result<(), ApiError>>>,
        add_error: Mutex<Option<ApiError>>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                add_error: Mutex::default(),
                calls: Mutex::default(),
                list: Mutex::new(Ok(vec![
                    notebook("id-a", "Homelab", true, false, false, 2),
                    notebook("id-b", "Recipes", false, false, false, 0),
                    notebook("id-c", "Old ideas", false, true, false, 1),
                    notebook("id-d", "Gone", false, false, true, 3),
                ])),
                entries: Mutex::new(Ok(vec![entry("e1", "first")])),
                writes: Mutex::default(),
            }
        }
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn write(&self) -> Result<(), ApiError> {
            self.writes.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    impl NotebookApi for Arc<Fake> {
        fn list(&self, on_done: Done<Vec<Notebook>>) {
            self.record("list".to_owned());
            on_done(self.list.lock().unwrap().clone());
        }

        fn create(&self, title: &str, content: &str, on_done: Done<Notebook>) {
            self.record(format!("create:{title}:{content}"));
            on_done(Ok(notebook("id-new", title, false, false, false, 0)));
        }

        fn update(&self, change: &NotebookChange, on_done: Done<()>) {
            self.record(format!(
                "update:{}:title={:?}:pinned={:?}:archived={:?}:trashed={:?}",
                change.id, change.title, change.pinned, change.archived, change.trashed
            ));
            on_done(self.write());
        }

        fn delete(&self, id: &str, on_done: Done<()>) {
            self.record(format!("delete:{id}"));
            on_done(self.write());
        }

        fn entries(&self, notebook_id: &str, on_done: Done<Vec<NotebookEntry>>) {
            self.record(format!("entries:{notebook_id}"));
            on_done(self.entries.lock().unwrap().clone());
        }

        fn add_entry(&self, notebook_id: &str, content: &str, on_done: Done<NotebookEntry>) {
            self.record(format!("add:{notebook_id}:{content}"));
            let failure = self.add_error.lock().unwrap().take();
            on_done(match failure {
                Some(e) => Err(e),
                None => Ok(entry("e2", content)),
            });
        }

        fn update_entry(
            &self,
            notebook_id: &str,
            entry_id: &str,
            content: &str,
            on_done: Done<()>,
        ) {
            self.record(format!("update_entry:{notebook_id}:{entry_id}:{content}"));
            on_done(self.write());
        }

        fn delete_entry(&self, notebook_id: &str, entry_id: &str, on_done: Done<()>) {
            self.record(format!("delete_entry:{notebook_id}:{entry_id}"));
            on_done(self.write());
        }
    }

    fn notebook(
        id: &str,
        title: &str,
        pinned: bool,
        archived: bool,
        trashed: bool,
        entries: u32,
    ) -> Notebook {
        Notebook {
            id: id.to_owned(),
            title: title.to_owned(),
            color: "neutral".to_owned(),
            is_pinned: pinned,
            is_archived: archived,
            is_trashed: trashed,
            created_at: Some("2026-05-01T10:00:00Z".to_owned()),
            updated_at: Some("2026-05-02T09:30:00Z".to_owned()),
            entry_count: entries,
        }
    }

    fn entry(id: &str, content: &str) -> NotebookEntry {
        NotebookEntry {
            id: id.to_owned(),
            notebook_id: "id-a".to_owned(),
            content: content.to_owned(),
            created_at: Some("2026-05-03T12:00:00Z".to_owned()),
            updated_at: None,
        }
    }

    fn controller(fake: &Arc<Fake>) -> NotebooksController {
        let api: Arc<dyn NotebookApi> = Arc::new(fake.clone());
        NotebooksController::new(api, |_view| {})
    }

    #[test]
    fn refresh_splits_pinned_and_counts_tabs() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();

        let view = c.view();
        assert!(!view.loading);
        assert_eq!(view.tabs, ["Active (2)", "Archive (1)", "Trash (1)"]);
        assert_eq!(view.pinned.len(), 1);
        assert_eq!(view.pinned[0].title, "Homelab");
        assert_eq!(view.pinned[0].count, "2 entries");
        // The local day of 2026-05-02T09:30Z: the 1st or the 2nd, depending on the device's time zone.
        assert!(
            view.pinned[0].date.ends_with("/2026"),
            "{}",
            view.pinned[0].date
        );
        assert_eq!(view.others.len(), 1);
        assert_eq!(view.others[0].title, "Recipes");
        assert_eq!(view.others[0].count, "0 entries");
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn tab_and_search_filter_the_list() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();

        c.set_tab(NotebookTab::Trashed);
        let view = c.view();
        assert_eq!(view.tab, 2);
        assert_eq!(view.pinned.len() + view.others.len(), 1);
        assert_eq!(
            view.others.first().map(|card| card.title.as_str()),
            Some("Gone")
        );

        c.set_tab(NotebookTab::Active);
        c.set_search("  REC ");
        let view = c.view();
        assert_eq!(view.pinned.len() + view.others.len(), 1);
        assert_eq!(view.others[0].title, "Recipes");

        c.set_search("nothing matches");
        assert!(c.view().empty);
    }

    #[test]
    fn failed_refresh_sets_error_and_dismiss_clears_it() {
        let fake: Arc<Fake> = Arc::default();
        *fake.list.lock().unwrap() = Err(ApiError::Transport("down".to_owned()));
        let c = controller(&fake);
        c.refresh();

        let view = c.view();
        assert!(!view.loading);
        assert_eq!(
            view.error.as_deref(),
            Some("Failed to load notebooks: network error: down")
        );
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn create_refuses_a_blank_form_and_defaults_the_title() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.open_create();
        c.create_title("   ");
        assert!(!c.create());
        assert!(fake.calls().is_empty());

        c.create_content(" first thought ");
        assert!(c.create());
        assert_eq!(fake.calls()[0], "create:Untitled:first thought");
    }

    #[test]
    fn create_success_opens_the_new_thread() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.open_create();
        c.create_title("Books");
        assert!(c.create());

        let view = c.view();
        assert!(view.create.is_none(), "the form closes on success");
        let thread = view.thread.expect("the new notebook opens");
        assert_eq!(thread.id, "id-new");
        assert_eq!(thread.title, "Books");
        assert_eq!(fake.calls(), ["create:Books:", "entries:id-new"]);
    }

    #[test]
    fn open_loads_the_entries_of_the_thread() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        let thread = c.view().thread.expect("thread open");
        assert_eq!(thread.title, "Homelab");
        assert_eq!(thread.count, "1 entry in this thread");
        assert_eq!(thread.entries[0].content, "first");
        assert!(!thread.entries_loading);
        assert_eq!(fake.calls(), ["list", "entries:id-a"]);
    }

    #[test]
    fn rename_trims_skips_unchanged_and_reverts_blank() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.rename_thread("Homelab");
        assert!(!fake.calls().iter().any(|call| call.starts_with("update")));

        c.rename_thread("   ");
        assert_eq!(c.view().thread.unwrap().title, "Homelab");
        assert!(!fake.calls().iter().any(|call| call.starts_with("update")));

        c.rename_thread("  Lab  ");
        assert!(fake.calls().contains(
            &"update:id-a:title=Some(\"Lab\"):pinned=None:archived=None:trashed=None".to_owned()
        ));
        assert_eq!(c.view().thread.unwrap().title, "Lab");
    }

    #[test]
    fn add_refuses_blank_and_clears_the_draft_on_success() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.thread_draft("   ");
        assert!(!c.add_entry());

        c.thread_draft("  new note ");
        assert!(c.add_entry());
        let thread = c.view().thread.unwrap();
        assert_eq!(thread.draft, "");
        assert_eq!(thread.entries.len(), 2);
        assert_eq!(fake.calls()[2..], ["add:id-a:new note", "list"]);
    }

    #[test]
    fn failed_add_keeps_the_draft_and_shows_the_error() {
        let fake: Arc<Fake> = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");
        *fake.add_error.lock().unwrap() = Some(ApiError::Transport("gone".to_owned()));

        c.thread_draft("keep me");
        assert!(c.add_entry());
        let view = c.view();
        assert_eq!(view.thread.as_ref().unwrap().draft, "keep me");
        assert!(!view.thread.unwrap().adding);
        assert_eq!(
            view.error.as_deref(),
            Some("Add failed: network error: gone")
        );
    }

    #[test]
    fn edit_save_updates_the_entry_and_refreshes() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.start_edit("e1");
        assert!(c.view().thread.unwrap().entries[0].editing);
        c.edit_text("   ");
        assert!(!c.save_edit(), "a blank edit is refused");

        c.edit_text(" second draft ");
        assert!(c.save_edit());
        let thread = c.view().thread.unwrap();
        assert_eq!(thread.entries[0].content, "second draft");
        assert!(!thread.entries[0].editing);
        assert!(
            fake.calls()
                .contains(&"update_entry:id-a:e1:second draft".to_owned())
        );
    }

    #[test]
    fn trash_from_the_thread_waits_for_confirmation() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.ask_trash("id-a");
        let confirm = c.view().confirm.expect("confirmation open");
        assert_eq!(confirm.kind, ConfirmKind::Trash);
        c.confirm_cancel();
        assert!(c.view().confirm.is_none());
        assert!(!fake.calls().iter().any(|call| call.starts_with("update")));

        c.ask_trash("id-a");
        c.confirm_accept();
        assert!(c.view().thread.is_none(), "trashing closes the open thread");
        assert!(
            fake.calls().contains(
                &"update:id-a:title=None:pinned=Some(false):archived=None:trashed=Some(true)"
                    .to_owned()
            )
        );
    }

    #[test]
    fn delete_removes_the_notebook_locally_and_calls_the_api() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.ask_delete("id-a");
        assert_eq!(c.view().confirm.unwrap().kind, ConfirmKind::Delete);
        c.confirm_accept();

        let view = c.view();
        assert!(view.thread.is_none());
        assert!(view.pinned.iter().all(|card| card.id != "id-a"));
        assert_eq!(fake.calls().last().map(String::as_str), Some("delete:id-a"));
    }

    #[test]
    fn rename_reloads_the_list_and_the_entries_and_keeps_the_draft() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");
        c.thread_draft("half typed");

        c.thread_title(" Renamed ");
        c.rename_thread(" Renamed ");
        let calls = fake.calls();
        assert_eq!(
            &calls[calls.len() - 3..],
            [
                "update:id-a:title=Some(\"Renamed\"):pinned=None:archived=None:trashed=None",
                "list",
                "entries:id-a",
            ]
        );
        let thread = c.view().thread.unwrap();
        assert_eq!(thread.title, "Renamed");
        assert_eq!(
            thread.draft, "half typed",
            "the entry draft survives the reload"
        );
    }

    #[test]
    fn refresh_hook_refetches_the_list() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.refresh();
        assert_eq!(fake.calls(), ["list", "list"]);
        assert_eq!(c.view().tabs[0], "Active (2)");
    }

    #[test]
    fn open_entry_for_edit_and_cancel_it() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.start_edit("missing");
        assert!(
            !c.view().thread.unwrap().entries[0].editing,
            "an unknown entry does not open"
        );
        c.start_edit("e1");
        c.edit_text("typing");
        assert_eq!(c.view().thread.unwrap().edit_text, "typing");
        c.cancel_edit();
        let thread = c.view().thread.unwrap();
        assert!(!thread.entries[0].editing);
        assert_eq!(thread.edit_text, "");
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| call.starts_with("update_entry"))
        );
    }

    #[test]
    fn delete_entry_removes_it_and_reloads_the_list() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();
        c.open("id-a");

        c.delete_entry("e1");
        assert!(c.view().thread.unwrap().entries.is_empty());
        let calls = fake.calls();
        assert_eq!(&calls[calls.len() - 2..], ["delete_entry:id-a:e1", "list"]);
    }

    #[test]
    fn create_form_fields_and_close() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.create_title("ignored without a form");
        assert!(c.view().create.is_none());

        c.open_create();
        c.create_title("Recipes");
        c.create_content("soup");
        let form = c.view().create.unwrap();
        assert_eq!(
            (form.title.as_str(), form.content.as_str()),
            ("Recipes", "soup")
        );
        c.close_create();
        assert!(c.view().create.is_none());
        assert!(
            fake.calls().is_empty(),
            "the form sends nothing until create"
        );
    }

    #[test]
    fn cancelled_delete_sends_nothing() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();

        c.ask_delete("id-b");
        c.confirm_cancel();
        assert!(c.view().confirm.is_none());
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn archive_and_restore_send_the_flags() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();

        c.toggle_archive("id-b");
        c.restore("id-d");
        assert!(fake.calls().contains(
            &"update:id-b:title=None:pinned=None:archived=Some(true):trashed=None".to_owned()
        ));
        assert!(fake.calls().contains(
            &"update:id-d:title=None:pinned=None:archived=None:trashed=Some(false)".to_owned()
        ));
    }

    #[test]
    fn pin_flips_the_flag_sent_to_the_api() {
        let fake = Arc::default();
        let c = controller(&fake);
        c.refresh();

        c.toggle_pin("id-b");
        assert!(fake.calls().contains(
            &"update:id-b:title=None:pinned=Some(true):archived=None:trashed=None".to_owned()
        ));
        assert_eq!(
            c.view().pinned.len(),
            2,
            "the card moves to the pinned grid at once"
        );
    }
}
