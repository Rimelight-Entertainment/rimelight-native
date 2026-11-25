//! The Todos page controller (ported from `crates/app/src/todos.rs`, with the web `TodosManager` feature set).
//!
//! Plain Rust with no Slint. [`TodosController`] holds the page state (todos, filter, search, batch selection, the
//! batch-delete confirmation, the error message), calls the todo endpoints through [`TodoApi`], and reports each state
//! change as a [`TodosView`]. The Slint glue in `apps/desktop/src/pages/todos.rs` turns those views into the models of
//! `ui/todos.slint`. [`QuickTodoController`] is the global quick-create modal (`QuickTodoModal` in `ui/todos.slint`).
//!
//! Threading: the API calls return at once and answer later on another thread (ehttp's worker). Each controller locks
//! its state only around reads and writes, never across an API call, and calls `notify` after the lock is released.
//! The notify callback must hand the view to the UI thread itself (the glue uses `Weak::upgrade_in_event_loop`).

use std::sync::{Arc, Mutex, MutexGuard};

use ehttp::{Headers, Method, Request};
use idantity_core::ApiError;
use idantity_core::local_date;
use serde::{Deserialize, Serialize};

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@idantity_todos`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Categories the quick-create modal offers (the egui port's list).
pub const QUICK_CATEGORIES: [&str; 5] = ["General", "Work", "Home", "Shopping", "Health"];
/// Priorities the quick-create modal offers, as `(value, label)`.
pub const QUICK_PRIORITIES: [(&str, &str); 3] = [
    ("low", "Low priority"),
    ("medium", "Medium priority"),
    ("high", "High priority"),
];
/// The recurring intervals the add form offers, in its order.
pub const INTERVALS: [&str; 3] = ["daily", "weekly", "monthly"];
/// The priorities the add form offers, in its order.
pub const PRIORITIES: [&str; 4] = ["low", "medium", "high", "urgent"];

type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// One todo as the page reads it. `GET /api/todos` returns the full rows, recurring fields included.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub is_completed: bool,
    pub due_date: Option<String>,
    pub is_recurring: bool,
    pub recurring_interval: Option<String>,
}

/// Body for `POST /api/todos`. The server ignores `isRecurring` and `recurringInterval` today; the page sends them as
/// the web does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTodo {
    pub title: String,
    pub category: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub is_recurring: bool,
    pub recurring_interval: Option<String>,
}

#[derive(Deserialize)]
struct TodoList {
    #[serde(default)]
    todos: Vec<TodoItem>,
}

/// The API boundary: the todo calls the page makes. [`HttpTodoApi`] talks to the server; tests inject a fake.
///
/// Batch actions are not a server call: the controller sends one PATCH or DELETE per selected todo. The web's
/// `POST {action: "batchComplete" | "batchDelete"}` is not handled by the server (it creates a blank todo).
pub trait TodoApi: Send + Sync {
    fn list(&self, on_done: Done<Vec<TodoItem>>);
    fn create(&self, todo: &NewTodo, on_done: Done<()>);
    fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>);
    fn delete(&self, id: &str, on_done: Done<()>);
}

/// The live API over HTTP, for `api_base` (for example `https://personal.idantity.me/api`).
pub struct HttpTodoApi {
    base: String,
    /// The signed-in session's header (Cloudflare Access), added to every request.
    credential: Option<Arc<dyn idantity_core::BearerSource>>,
}

/// The live API for `api_base`.
/// The live API for `api_base`, signing each request with the session's header.
pub fn http_api_signed(
    api_base: impl Into<String>,
    credential: Arc<dyn idantity_core::BearerSource>,
) -> Arc<dyn TodoApi> {
    Arc::new(HttpTodoApi {
        base: api_base.into().trim_end_matches('/').to_owned(),
        credential: Some(credential),
    })
}

pub fn http_api(api_base: impl Into<String>) -> Arc<dyn TodoApi> {
    Arc::new(HttpTodoApi {
        base: api_base.into().trim_end_matches('/').to_owned(),
        credential: None,
    })
}

fn json_request(method: Method, url: String, body: Vec<u8>) -> Request {
    let mut request = Request::new(
        method,
        url,
        Headers::new(&[("Content-Type", "application/json")]),
    );
    request.body = body;
    request
}

fn send<T: Send + 'static>(
    request: Request,
    decode: impl FnOnce(&[u8]) -> Result<T, ApiError> + Send + 'static,
    on_done: Done<T>,
) {
    ehttp::fetch(request, move |result| {
        let outcome = match result {
            Err(e) => Err(ApiError::Transport(e)),
            Ok(resp) if !resp.ok => Err(ApiError::Status {
                code: resp.status,
                body: String::from_utf8_lossy(&resp.bytes).into_owned(),
            }),
            Ok(resp) => decode(&resp.bytes),
        };
        on_done(outcome);
    });
}

impl HttpTodoApi {
    fn signed<T: Send + 'static>(
        &self,
        mut request: Request,
        decode: impl FnOnce(&[u8]) -> Result<T, ApiError> + Send + 'static,
        on_done: Done<T>,
    ) {
        if let Some(token) = self.credential.as_ref().and_then(|c| c.credential()) {
            request.headers.insert(token.name, token.value);
        }
        send(request, decode, on_done);
    }
}

impl TodoApi for HttpTodoApi {
    fn list(&self, on_done: Done<Vec<TodoItem>>) {
        // The web page asks for fresh data every time: a timestamp query and no-store.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut request = Request::get(format!("{}/todos?t={stamp}", self.base));
        request.headers = Headers::new(&[("Cache-Control", "no-store, no-cache, must-revalidate")]);
        self.signed(
            request,
            |bytes| {
                serde_json::from_slice::<TodoList>(bytes)
                    .map(|list| list.todos)
                    .map_err(|e| ApiError::Decode(e.to_string()))
            },
            on_done,
        );
    }

    fn create(&self, todo: &NewTodo, on_done: Done<()>) {
        let body = serde_json::to_vec(todo).expect("plain struct always serializes");
        self.signed(
            json_request(Method::POST, format!("{}/todos", self.base), body),
            |_| Ok(()),
            on_done,
        );
    }

    fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>) {
        #[derive(Serialize)]
        struct Patch<'a> {
            id: &'a str,
            #[serde(rename = "isCompleted")]
            is_completed: bool,
        }
        let body = serde_json::to_vec(&Patch { id, is_completed })
            .expect("plain struct always serializes");
        self.signed(
            json_request(Method::PATCH, format!("{}/todos", self.base), body),
            |_| Ok(()),
            on_done,
        );
    }

    fn delete(&self, id: &str, on_done: Done<()>) {
        let url = format!("{}/todos?id={}", self.base, percent_encode(id));
        self.signed(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            |_| Ok(()),
            on_done,
        );
    }
}

/// Encodes a query value. Todo ids are UUIDs, so only reserved characters matter.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The six filter tabs, in the web's order. [`Filter::Pending`] is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Pending,
    Today,
    High,
    Recurring,
    Completed,
    All,
}

impl Filter {
    /// The tabs in display order. A tab's index in this array is the index the Slint page sends back.
    pub const TABS: [Filter; 6] = [
        Filter::Pending,
        Filter::Today,
        Filter::High,
        Filter::Recurring,
        Filter::Completed,
        Filter::All,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::Pending => "Pending",
            Filter::Today => "Due Today",
            Filter::High => "High Priority",
            Filter::Recurring => "Recurring",
            Filter::Completed => "Completed",
            Filter::All => "All",
        }
    }

    /// The tab at `index`, or [`Filter::Pending`] for an index out of range.
    pub fn from_index(index: usize) -> Self {
        Self::TABS.get(index).copied().unwrap_or_default()
    }

    /// The position of this filter's tab.
    pub fn index(self) -> usize {
        Self::TABS.iter().position(|f| *f == self).unwrap_or(0)
    }

    fn keeps(self, todo: &TodoItem) -> bool {
        match self {
            Filter::Pending => !todo.is_completed,
            Filter::Today => !todo.is_completed && is_due_today(todo),
            Filter::High => {
                !todo.is_completed && (todo.priority == "high" || todo.priority == "urgent")
            }
            Filter::Recurring => todo.is_recurring,
            Filter::Completed => todo.is_completed,
            Filter::All => true,
        }
    }
}

/// True when the todo's due date falls on today's local day (`isToday` in the web page).
fn is_due_today(todo: &TodoItem) -> bool {
    match todo.due_date.as_deref().and_then(local_date::local_day_of) {
        Some(day) => day == local_date::today(),
        None => false,
    }
}

/// The local day of a due date, as `YYYY-MM-DD`, or empty when there is none.
fn due_label(value: Option<&str>) -> String {
    let Some(day) = value.and_then(local_date::local_day_of) else {
        return String::new();
    };
    format!("• Due: {:04}-{:02}-{:02}", day.year, day.month, day.day)
}

/// One filter tab: its label and the count it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTabView {
    pub label: String,
    pub count: usize,
}

/// One row of the list, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoRowView {
    pub id: String,
    pub title: String,
    pub category: String,
    /// `low`, `medium`, `high` or `urgent`. The Slint badge picks its colour from it.
    pub priority: String,
    pub completed: bool,
    pub selected: bool,
    pub recurring: bool,
    /// The interval text on the recurring badge (`daily`, `weekly`, `monthly`, or `recurring`).
    pub recurring_label: String,
    /// `• Due: YYYY-MM-DD`, or empty when the todo has no due date.
    pub due_label: String,
    pub due_today: bool,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodosView {
    /// The rows that pass the filter and the search.
    pub rows: Vec<TodoRowView>,
    /// The six tabs, in [`Filter::TABS`] order, with their counts.
    pub tabs: Vec<FilterTabView>,
    /// The selected tab's index.
    pub filter: usize,
    pub search: String,
    /// How many todos are selected (for the batch toolbar).
    pub selected_count: usize,
    /// True while the batch-delete confirmation is open.
    pub confirm_delete_open: bool,
    pub loading: bool,
    /// The message of the last failed call, until the user dismisses it.
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    todos: Vec<TodoItem>,
    filter: Filter,
    search: String,
    selected: Vec<String>,
    confirm_delete: bool,
    /// Writes sent and not yet answered. The list is fetched again when the last one answers.
    pending_writes: usize,
    loading: bool,
    error: Option<String>,
}

impl State {
    fn view(&self) -> TodosView {
        let q = self.search.trim().to_lowercase();
        let matches_search = |todo: &TodoItem| {
            q.is_empty()
                || todo.title.to_lowercase().contains(&q)
                || todo.category.to_lowercase().contains(&q)
        };
        let rows = self
            .todos
            .iter()
            .filter(|todo| matches_search(todo) && self.filter.keeps(todo))
            .map(|todo| TodoRowView {
                id: todo.id.clone(),
                title: todo.title.clone(),
                category: todo.category.clone(),
                priority: todo.priority.clone(),
                completed: todo.is_completed,
                selected: self.selected.contains(&todo.id),
                recurring: todo.is_recurring,
                recurring_label: todo
                    .recurring_interval
                    .clone()
                    .unwrap_or_else(|| "recurring".to_owned()),
                due_label: due_label(todo.due_date.as_deref()),
                due_today: is_due_today(todo),
            })
            .collect();

        let count = |keep: fn(&TodoItem) -> bool| self.todos.iter().filter(|t| keep(t)).count();
        let counts = [
            count(|t| !t.is_completed),
            count(|t| !t.is_completed && is_due_today(t)),
            count(|t| !t.is_completed && (t.priority == "high" || t.priority == "urgent")),
            count(|t| t.is_recurring),
            count(|t| t.is_completed),
            self.todos.len(),
        ];
        let tabs = Filter::TABS
            .iter()
            .zip(counts)
            .map(|(filter, count)| FilterTabView {
                label: filter.label().to_owned(),
                count,
            })
            .collect();

        TodosView {
            rows,
            tabs,
            filter: self.filter.index(),
            search: self.search.clone(),
            selected_count: self.selected.len(),
            confirm_delete_open: self.confirm_delete,
            loading: self.loading,
            error: self.error.clone(),
        }
    }
}

type Notify = Arc<dyn Fn(TodosView) + Send + Sync>;

struct Shared {
    api: Arc<dyn TodoApi>,
    state: Mutex<State>,
    notify: Notify,
}

/// One write to send.
enum Write {
    SetCompleted(String, bool),
    Delete(String),
}

/// The Todos page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct TodosController {
    shared: Arc<Shared>,
}

impl TodosController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(api: Arc<dyn TodoApi>, notify: impl Fn(TodosView) + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::default(),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> TodosView {
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
                    Ok(todos) => {
                        // Drop selections of todos that no longer exist.
                        s.selected.retain(|id| todos.iter().any(|t| &t.id == id));
                        s.todos = todos;
                        s.error = None;
                    }
                    Err(e) => s.error = Some(e.to_string()),
                }
            });
        }));
    }

    /// Selects the tab at `index` (see [`Filter::TABS`]).
    pub fn set_filter(&self, index: usize) {
        self.change(|s| s.filter = Filter::from_index(index));
    }

    /// Sets the search text: matches title or category, case-insensitively.
    pub fn set_search(&self, text: &str) {
        self.change(|s| s.search = text.to_owned());
    }

    /// Adds `id` to the batch selection, or removes it when it is already there.
    pub fn toggle_select(&self, id: &str) {
        self.change(|s| {
            if let Some(pos) = s.selected.iter().position(|i| i == id) {
                s.selected.remove(pos);
            } else {
                s.selected.push(id.to_owned());
            }
        });
    }

    /// Clears the batch selection (the toolbar's Cancel).
    pub fn clear_selection(&self) {
        self.change(|s| s.selected.clear());
    }

    /// Marks every selected todo complete, then fetches the list again. Clears the selection.
    pub fn batch_complete(&self) {
        let ids = self.take_selection();
        if ids.is_empty() {
            return;
        }
        self.change(|s| {
            for todo in s.todos.iter_mut().filter(|t| ids.contains(&t.id)) {
                todo.is_completed = true;
            }
        });
        self.dispatch(
            ids.into_iter()
                .map(|id| Write::SetCompleted(id, true))
                .collect(),
        );
    }

    /// Opens the batch-delete confirmation. Does nothing when nothing is selected.
    pub fn request_batch_delete(&self) {
        self.change(|s| {
            if !s.selected.is_empty() {
                s.confirm_delete = true;
            }
        });
    }

    /// Closes the batch-delete confirmation without deleting.
    pub fn cancel_batch_delete(&self) {
        self.change(|s| s.confirm_delete = false);
    }

    /// Deletes every selected todo, then fetches the list again. Closes the confirmation and clears the selection.
    pub fn confirm_batch_delete(&self) {
        let ids = self.take_selection();
        self.change(|s| s.confirm_delete = false);
        if ids.is_empty() {
            return;
        }
        self.change(|s| s.todos.retain(|t| !ids.contains(&t.id)));
        self.dispatch(ids.into_iter().map(Write::Delete).collect());
    }

    /// Creates a todo from the add form. Returns `false`, and calls nothing, when the title is blank. The category
    /// defaults to `General` and is trimmed. `due_date` is `YYYY-MM-DD` or empty. The interval counts only when
    /// `recurring` is set. On success the list is fetched again.
    pub fn add(
        &self,
        title: &str,
        category: &str,
        priority: &str,
        due_date: &str,
        recurring: bool,
        interval: &str,
    ) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let category = category.trim();
        let due = due_date.trim();
        let todo = NewTodo {
            title: title.to_owned(),
            category: if category.is_empty() {
                "General"
            } else {
                category
            }
            .to_owned(),
            priority: priority.to_owned(),
            due_date: (!due.is_empty()).then(|| due.to_owned()),
            is_recurring: recurring,
            recurring_interval: recurring.then(|| interval.to_owned()),
        };
        let me = self.clone();
        self.shared.api.create(
            &todo,
            Box::new(move |result| match result {
                Ok(()) => me.refresh(),
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// Sets the completed flag of the todo `id`, then fetches the list again.
    pub fn toggle(&self, id: &str, is_completed: bool) {
        self.dispatch(vec![Write::SetCompleted(id.to_owned(), is_completed)]);
    }

    /// Deletes the todo `id`, then fetches the list again.
    pub fn delete(&self, id: &str) {
        self.dispatch(vec![Write::Delete(id.to_owned())]);
    }

    /// Clears the error message.
    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn take_selection(&self) -> Vec<String> {
        self.change(|s| std::mem::take(&mut s.selected))
    }

    /// Sends the writes. The list is fetched again after the last one answers (or at once after a failure).
    fn dispatch(&self, writes: Vec<Write>) {
        if writes.is_empty() {
            return;
        }
        let count = writes.len();
        self.change(|s| s.pending_writes += count);
        for write in writes {
            let me = self.clone();
            let done: Done<()> = Box::new(move |result| me.write_done(result));
            match write {
                Write::SetCompleted(id, completed) => {
                    self.shared.api.set_completed(&id, completed, done);
                }
                Write::Delete(id) => self.shared.api.delete(&id, done),
            }
        }
    }

    fn write_done(&self, result: Result<(), ApiError>) {
        let last = self.change(|s| {
            s.pending_writes = s.pending_writes.saturating_sub(1);
            if let Err(e) = &result {
                s.error = Some(e.to_string());
            }
            s.pending_writes == 0
        });
        if last {
            self.refresh();
        }
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| s.error = Some(error.to_string()));
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

/// The state of the global quick-create to-do modal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickTodoView {
    pub open: bool,
    pub busy: bool,
    pub error: Option<String>,
    /// Counts up after each saved to-do. The shell reloads the Todos page when it changes.
    pub saved_seq: u64,
}

#[derive(Default)]
struct QuickState {
    open: bool,
    busy: bool,
    error: Option<String>,
    saved_seq: u64,
}

impl QuickState {
    fn view(&self) -> QuickTodoView {
        QuickTodoView {
            open: self.open,
            busy: self.busy,
            error: self.error.clone(),
            saved_seq: self.saved_seq,
        }
    }
}

struct QuickShared {
    api: Arc<dyn TodoApi>,
    state: Mutex<QuickState>,
    notify: Arc<dyn Fn(QuickTodoView) + Send + Sync>,
}

/// The quick-create modal: title, category and priority, saved as a new to-do that is not completed. The title,
/// category and priority fields live in the Slint modal; [`QuickTodoController::save`] gets them as arguments.
#[derive(Clone)]
pub struct QuickTodoController {
    shared: Arc<QuickShared>,
}

impl QuickTodoController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn TodoApi>,
        notify: impl Fn(QuickTodoView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(QuickShared {
                api,
                state: Mutex::default(),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> QuickTodoView {
        self.lock().view()
    }

    /// Shows the modal with no error.
    pub fn open(&self) {
        self.change(|s| {
            s.open = true;
            s.error = None;
        });
    }

    /// Hides the modal. A save still in flight finishes in the background.
    pub fn close(&self) {
        self.change(|s| s.open = false);
    }

    /// Saves a to-do. Returns `false`, and calls nothing, when the title is blank or a save is already running.
    /// On success the modal closes and `saved_seq` counts up.
    pub fn save(&self, title: &str, category: &str, priority: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let started = self.change(|s| {
            if s.busy {
                false
            } else {
                s.busy = true;
                s.error = None;
                true
            }
        });
        if !started {
            return false;
        }
        let todo = NewTodo {
            title: title.to_owned(),
            category: category.to_owned(),
            priority: priority.to_owned(),
            due_date: None,
            is_recurring: false,
            recurring_interval: None,
        };
        let me = self.clone();
        self.shared.api.create(
            &todo,
            Box::new(move |result| {
                me.change(|s| {
                    s.busy = false;
                    match result {
                        Ok(()) => {
                            s.open = false;
                            s.saved_seq += 1;
                        }
                        Err(e) => s.error = Some(e.to_string()),
                    }
                });
            }),
        );
        true
    }

    fn lock(&self) -> MutexGuard<'_, QuickState> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn change<R>(&self, update: impl FnOnce(&mut QuickState) -> R) -> R {
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

    /// Records each call and answers at once from scripted replies.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        list: Mutex<Option<Result<Vec<TodoItem>, ApiError>>>,
        writes: Mutex<VecDeque<Result<(), ApiError>>>,
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
    }

    impl TodoApi for Arc<Fake> {
        fn list(&self, on_done: Done<Vec<TodoItem>>) {
            self.record("list".to_owned());
            let reply =
                self.list.lock().unwrap().clone().unwrap_or_else(|| {
                    Ok(vec![todo("id-a", "Water plants", "Home", "high", false)])
                });
            on_done(reply);
        }

        fn create(&self, todo: &NewTodo, on_done: Done<()>) {
            self.record(format!(
                "create:{}|{}|{}|{:?}|{}|{:?}",
                todo.title,
                todo.category,
                todo.priority,
                todo.due_date,
                todo.is_recurring,
                todo.recurring_interval
            ));
            on_done(self.next_write());
        }

        fn set_completed(&self, id: &str, is_completed: bool, on_done: Done<()>) {
            self.record(format!("toggle:{id}:{is_completed}"));
            on_done(self.next_write());
        }

        fn delete(&self, id: &str, on_done: Done<()>) {
            self.record(format!("delete:{id}"));
            on_done(self.next_write());
        }
    }

    fn todo(id: &str, title: &str, category: &str, priority: &str, is_completed: bool) -> TodoItem {
        TodoItem {
            id: id.to_owned(),
            title: title.to_owned(),
            category: category.to_owned(),
            priority: priority.to_owned(),
            is_completed,
            ..TodoItem::default()
        }
    }

    fn controller(fake: &Arc<Fake>) -> (TodosController, Arc<Mutex<Vec<TodosView>>>) {
        let seen: Arc<Mutex<Vec<TodosView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn TodoApi> = Arc::new(fake.clone());
        let c = TodosController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn with_list(fake: &Arc<Fake>, todos: Vec<TodoItem>) {
        *fake.list.lock().unwrap() = Some(Ok(todos));
    }

    #[test]
    fn refresh_shows_loading_then_rows() {
        let fake = Arc::default();
        let (c, seen) = controller(&fake);
        c.refresh();

        let views = seen.lock().unwrap().clone();
        assert!(
            views[0].loading,
            "the first notification is the loading flag"
        );
        let last = views.last().unwrap();
        assert!(!last.loading);
        assert_eq!(last.rows.len(), 1);
        assert_eq!(last.rows[0].title, "Water plants");
        assert_eq!(last.rows[0].category, "Home");
        assert_eq!(last.rows[0].priority, "high");
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn failed_refresh_sets_error_and_dismiss_clears_it() {
        let fake: Arc<Fake> = Arc::default();
        *fake.list.lock().unwrap() = Some(Err(ApiError::Transport("down".to_owned())));
        let (c, _seen) = controller(&fake);
        c.refresh();

        let view = c.view();
        assert!(!view.loading);
        assert_eq!(view.error.as_deref(), Some("network error: down"));

        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn default_filter_is_pending_and_hides_completed() {
        let fake: Arc<Fake> = Arc::default();
        with_list(
            &fake,
            vec![
                todo("a", "Open", "General", "low", false),
                todo("b", "Done", "General", "low", true),
            ],
        );
        let (c, _seen) = controller(&fake);
        c.refresh();
        let view = c.view();
        assert_eq!(view.filter, 0);
        assert_eq!(
            view.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["a"]
        );
        assert_eq!(
            view.tabs[0],
            FilterTabView {
                label: "Pending".into(),
                count: 1
            }
        );
        assert_eq!(
            view.tabs[4],
            FilterTabView {
                label: "Completed".into(),
                count: 1
            }
        );
        assert_eq!(
            view.tabs[5],
            FilterTabView {
                label: "All".into(),
                count: 2
            }
        );
    }

    #[test]
    fn high_and_recurring_filters_and_counts() {
        let fake: Arc<Fake> = Arc::default();
        let mut recurring = todo("r", "Stretch", "Health", "urgent", false);
        recurring.is_recurring = true;
        recurring.recurring_interval = Some("weekly".to_owned());
        with_list(
            &fake,
            vec![
                todo("h", "Pay rent", "Home", "high", false),
                todo("l", "Read", "General", "low", false),
                recurring,
            ],
        );
        let (c, _seen) = controller(&fake);
        c.refresh();

        c.set_filter(2);
        let view = c.view();
        assert_eq!(view.rows.len(), 2, "high and urgent pending");
        assert_eq!(view.tabs[2].count, 2);

        c.set_filter(3);
        let view = c.view();
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].recurring_label, "weekly");
    }

    #[test]
    fn search_matches_title_or_category() {
        let fake: Arc<Fake> = Arc::default();
        with_list(
            &fake,
            vec![
                todo("a", "Water plants", "Home", "low", false),
                todo("b", "Gym", "Health", "low", false),
            ],
        );
        let (c, _seen) = controller(&fake);
        c.refresh();

        c.set_search("  HEALTH ");
        assert_eq!(
            c.view()
                .rows
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        c.set_search("plants");
        assert_eq!(
            c.view()
                .rows
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
    }

    #[test]
    fn add_sends_the_form_and_refreshes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        assert!(c.add("  Buy milk  ", "  ", "medium", "2026-11-30", true, "weekly"));
        assert_eq!(
            fake.calls(),
            [
                "create:Buy milk|General|medium|Some(\"2026-11-30\")|true|Some(\"weekly\")",
                "list"
            ]
        );
    }

    #[test]
    fn add_without_recurring_drops_the_interval() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        assert!(c.add("Call", "Work", "high", "", false, "monthly"));
        assert_eq!(fake.calls()[0], "create:Call|Work|high|None|false|None");
    }

    #[test]
    fn blank_title_is_refused_without_a_call() {
        let fake = Arc::default();
        let (c, seen) = controller(&fake);
        assert!(!c.add("   ", "General", "medium", "", false, "daily"));
        assert!(fake.calls().is_empty());
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn toggle_sends_the_flag_and_refreshes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.toggle("id-a", true);
        assert_eq!(fake.calls(), ["toggle:id-a:true", "list"]);
    }

    #[test]
    fn delete_sends_the_id_and_refreshes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.delete("id-a");
        assert_eq!(fake.calls(), ["delete:id-a", "list"]);
    }

    #[test]
    fn batch_complete_sends_one_patch_per_selected_todo() {
        let fake: Arc<Fake> = Arc::default();
        with_list(
            &fake,
            vec![
                todo("a", "One", "General", "low", false),
                todo("b", "Two", "General", "low", false),
                todo("c", "Three", "General", "low", false),
            ],
        );
        let (c, _seen) = controller(&fake);
        c.refresh();
        c.toggle_select("a");
        c.toggle_select("c");
        assert_eq!(c.view().selected_count, 2);

        c.batch_complete();
        assert_eq!(
            fake.calls(),
            ["list", "toggle:a:true", "toggle:c:true", "list"]
        );
        let view = c.view();
        assert_eq!(view.selected_count, 0, "the selection is cleared");
    }

    #[test]
    fn toggle_select_adds_and_removes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.toggle_select("a");
        assert_eq!(c.view().selected_count, 1);
        c.toggle_select("a");
        assert_eq!(c.view().selected_count, 0);
        c.toggle_select("a");
        c.clear_selection();
        assert_eq!(c.view().selected_count, 0);
    }

    #[test]
    fn batch_delete_needs_a_selection_and_a_confirmation() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.request_batch_delete();
        assert!(
            !c.view().confirm_delete_open,
            "nothing selected, nothing to confirm"
        );

        c.toggle_select("a");
        c.request_batch_delete();
        assert!(c.view().confirm_delete_open);
        assert!(fake.calls().is_empty(), "no call before the confirmation");

        c.cancel_batch_delete();
        assert!(!c.view().confirm_delete_open);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn confirmed_batch_delete_deletes_each_and_clears_selection() {
        let fake: Arc<Fake> = Arc::default();
        with_list(&fake, vec![todo("a", "One", "General", "low", false)]);
        let (c, _seen) = controller(&fake);
        c.refresh();
        c.toggle_select("a");
        c.toggle_select("b");
        c.request_batch_delete();
        c.confirm_batch_delete();

        assert_eq!(fake.calls(), ["list", "delete:a", "delete:b", "list"]);
        let view = c.view();
        assert!(!view.confirm_delete_open);
        assert_eq!(view.selected_count, 0);
    }

    #[test]
    fn failed_write_sets_error_and_still_refreshes_after_the_last_answer() {
        let fake: Arc<Fake> = Arc::default();
        fake.writes.lock().unwrap().push_back(Err(ApiError::Status {
            code: 500,
            body: "boom".to_owned(),
        }));
        let (c, _seen) = controller(&fake);
        c.delete("id-a");
        assert_eq!(fake.calls(), ["delete:id-a", "list"]);
        assert_eq!(c.view().error.as_deref(), Some("HTTP 500: boom"));
    }

    #[test]
    fn quick_todo_saves_and_closes() {
        let fake: Arc<Fake> = Arc::default();
        let seen: Arc<Mutex<Vec<QuickTodoView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn TodoApi> = Arc::new(fake.clone());
        let q = QuickTodoController::new(api, move |v| sink.lock().unwrap().push(v));

        q.open();
        assert!(!q.save("   ", "Work", "low"), "blank title is refused");
        assert!(q.save(" Ship it ", "Work", "high"));
        assert_eq!(fake.calls(), ["create:Ship it|Work|high|None|false|None"]);
        let view = q.view();
        assert!(!view.open);
        assert!(!view.busy);
        assert_eq!(view.saved_seq, 1);
    }

    #[test]
    fn quick_todo_failure_keeps_the_modal_open_with_error() {
        let fake: Arc<Fake> = Arc::default();
        fake.writes
            .lock()
            .unwrap()
            .push_back(Err(ApiError::Transport("offline".to_owned())));
        let api: Arc<dyn TodoApi> = Arc::new(fake.clone());
        let q = QuickTodoController::new(api, |_| {});
        q.open();
        assert!(q.save("Call", "Home", "medium"));
        let view = q.view();
        assert!(view.open);
        assert_eq!(view.saved_seq, 0);
        assert_eq!(view.error.as_deref(), Some("network error: offline"));
    }
}
