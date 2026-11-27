//! The Projects page controller (web: `src/components/projects/KanbanBoard.tsx`, `KanbanColumn.tsx`,
//! `KanbanCardItem.tsx`). There is no egui or core version: this crate is the first port.
//!
//! Plain Rust with no Slint. The controller holds the boards, lists and cards, the active board, and the open
//! input fields. It calls `/api/projects/cards` through [`ProjectsApi`] and reports each state change as a
//! [`ProjectsView`]. [`HttpProjects`] is the live client; it lives here because rimelight_personal_core has no projects module.
//!
//! Threading follows the Todos page: calls return at once and answer on a worker thread, the lock is never held
//! across a call, and `notify` is called after the lock is released.
//!
//! Server notes (see docs/inventory.md 2.5): `GET` returns every user's rows, `POST` always inserts a card row, and
//! `PATCH` needs `id`. This client sends `id` for moves (the web sends `cardId`, which the server rejects with 400),
//! so a moved card stays moved.

use std::sync::{Arc, Mutex, MutexGuard};

use ehttp::{Headers, Method, Request};
use rimelight_personal_core::ApiError;
use serde::Deserialize;

/// The page's Slint markup, for the shell's build script (added as `@rimelight_personal_feature_projects`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The four default columns a new board gets, in order (web `createDefaultLists`).
pub const DEFAULT_LISTS: [&str; 4] = ["Backlog", "In Progress", "In Review", "Done"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub id: String,
    #[serde(default)]
    pub board_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    #[serde(default)]
    pub list_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub order: i64,
}

/// The body of `GET /api/projects/cards`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ProjectsData {
    #[serde(default)]
    pub boards: Vec<Board>,
    #[serde(default)]
    pub lists: Vec<List>,
    #[serde(default)]
    pub cards: Vec<Card>,
}

/// One `POST /api/projects/cards`, with the web's `type` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewItem {
    Board {
        title: String,
    },
    List {
        board_id: String,
        title: String,
        order: i64,
    },
    Card {
        list_id: String,
        title: String,
        order: i64,
    },
}

pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the four calls the page makes. [`HttpProjects`] implements it over HTTP; tests inject a fake.
pub trait ProjectsApi: Send + Sync {
    fn list(&self, on_done: Done<ProjectsData>);
    /// Creates the item and answers with its id.
    fn create(&self, item: NewItem, on_done: Done<String>);
    /// Moves card `card_id` into list `list_id`.
    fn move_card(&self, card_id: &str, list_id: &str, on_done: Done<()>);
    fn delete_card(&self, card_id: &str, on_done: Done<()>);
}

/// The live client for `api_base`, for example `https://example.com/api/personal`.
/// The live API for `api_base`, signing each request with the session's header.
pub fn http_api_signed(
    api_base: impl Into<String>,
    credential: Arc<dyn rimelight_personal_core::BearerSource>,
) -> Arc<dyn ProjectsApi> {
    Arc::new(HttpProjects {
        base: api_base.into().trim_end_matches('/').to_owned(),
        credential: Some(credential),
    })
}

pub fn http_api(api_base: impl Into<String>) -> Arc<dyn ProjectsApi> {
    Arc::new(HttpProjects {
        base: api_base.into().trim_end_matches('/').to_owned(),
        credential: None,
    })
}

/// The projects endpoints over HTTP.
pub struct HttpProjects {
    base: String,
    /// The signed-in session's header (Cloudflare Access), added to every request.
    credential: Option<Arc<dyn rimelight_personal_core::BearerSource>>,
}

impl HttpProjects {
    fn url(&self) -> String {
        format!("{}/projects/cards", self.base)
    }
}

impl HttpProjects {
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

impl ProjectsApi for HttpProjects {
    fn list(&self, on_done: Done<ProjectsData>) {
        self.signed(Request::get(self.url()), decode_data, on_done);
    }

    fn create(&self, item: NewItem, on_done: Done<String>) {
        let body = match item {
            NewItem::Board { title } => serde_json::json!({ "type": "board", "title": title }),
            NewItem::List {
                board_id,
                title,
                order,
            } => {
                serde_json::json!({ "type": "list", "boardId": board_id, "title": title, "order": order })
            }
            NewItem::Card {
                list_id,
                title,
                order,
            } => {
                serde_json::json!({ "type": "card", "listId": list_id, "title": title, "order": order })
            }
        };
        self.signed(
            json_request(Method::POST, self.url(), &body),
            decode_id,
            on_done,
        );
    }

    fn move_card(&self, card_id: &str, list_id: &str, on_done: Done<()>) {
        let body = serde_json::json!({ "id": card_id, "listId": list_id });
        self.signed(
            json_request(Method::PATCH, self.url(), &body),
            decode_ok,
            on_done,
        );
    }

    fn delete_card(&self, card_id: &str, on_done: Done<()>) {
        let url = format!("{}?cardId={}", self.url(), percent_encode(card_id));
        self.signed(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ok,
            on_done,
        );
    }
}

fn json_request(method: Method, url: String, body: &serde_json::Value) -> Request {
    let mut request = Request::new(
        method,
        url,
        Headers::new(&[("Content-Type", "application/json")]),
    );
    request.body = serde_json::to_vec(body).expect("a JSON value always serializes");
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

fn decode_data(bytes: &[u8]) -> Result<ProjectsData, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

fn decode_id(bytes: &[u8]) -> Result<String, ApiError> {
    #[derive(Deserialize)]
    struct Row {
        id: String,
    }
    serde_json::from_slice::<Row>(bytes)
        .map(|row| row.id)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

fn decode_ok(_bytes: &[u8]) -> Result<(), ApiError> {
    Ok(())
}

/// Percent-encodes a query value (ids are UUIDs, so only reserved characters matter).
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

/// A board tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTab {
    pub id: String,
    pub title: String,
    pub active: bool,
}

/// Where a card can move: the neighbouring column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTarget {
    pub list_id: String,
    /// The column title, for the button's "Move to <title>" label.
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub move_left: Option<MoveTarget>,
    pub move_right: Option<MoveTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnView {
    pub id: String,
    pub title: String,
    pub count: usize,
    pub cards: Vec<CardView>,
    /// True while the "Add task" input is open in this column.
    pub adding: bool,
}

/// One card in the board's shape (the shared BoardPage's `BoardItem`): its key, title and a second line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub key: String,
    pub title: String,
    pub detail: String,
}

/// One lane (column) in the board's shape (the shared BoardPage's `BoardLane`), cards top first. A lane's index is its
/// position in [`ProjectsView::lanes`], which is what [`ProjectsController::move_card`] takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneView {
    pub name: String,
    pub cards: Vec<ItemView>,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectsView {
    /// The active board's lanes, in board order.
    pub lanes: Vec<LaneView>,
    /// The index of the lane whose "Task card title" input is open, if any.
    pub adding_lane: Option<usize>,
    pub boards: Vec<BoardTab>,
    /// True when a board is selected, so the columns and "Add Column" apply.
    pub has_active_board: bool,
    pub columns: Vec<ColumnView>,
    pub new_board_title: String,
    pub new_list_title: String,
    /// The text of the open "Task card title" input.
    pub card_title: String,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    boards: Vec<Board>,
    lists: Vec<List>,
    cards: Vec<Card>,
    active: String,
    loading: bool,
    error: Option<String>,
    new_board_title: String,
    new_list_title: String,
    adding_card: Option<String>,
    card_title: String,
}

impl State {
    /// The active board's columns, in the order the lists were loaded (web `currentLists`).
    fn current_lists(&self) -> Vec<&List> {
        self.lists
            .iter()
            .filter(|l| l.board_id.as_deref() == Some(self.active.as_str()))
            .collect()
    }

    fn view(&self) -> ProjectsView {
        let lists = self.current_lists();
        let columns = lists
            .iter()
            .enumerate()
            .map(|(index, list)| {
                let cards: Vec<CardView> = self
                    .cards
                    .iter()
                    .filter(|c| c.list_id.as_deref() == Some(list.id.as_str()))
                    .map(|card| CardView {
                        id: card.id.clone(),
                        title: card.title.clone(),
                        description: card.description.clone().unwrap_or_default(),
                        move_left: index.checked_sub(1).and_then(|i| lists.get(i)).map(|l| {
                            MoveTarget {
                                list_id: l.id.clone(),
                                title: l.title.clone(),
                            }
                        }),
                        move_right: lists.get(index + 1).map(|l| MoveTarget {
                            list_id: l.id.clone(),
                            title: l.title.clone(),
                        }),
                    })
                    .collect();
                ColumnView {
                    id: list.id.clone(),
                    title: list.title.clone(),
                    count: cards.len(),
                    cards,
                    adding: self.adding_card.as_deref() == Some(list.id.as_str()),
                }
            })
            .collect();

        let lanes: Vec<LaneView> = lists
            .iter()
            .map(|list| LaneView {
                name: list.title.clone(),
                cards: self
                    .cards
                    .iter()
                    .filter(|c| c.list_id.as_deref() == Some(list.id.as_str()))
                    .map(|card| ItemView {
                        key: card.id.clone(),
                        title: card.title.clone(),
                        detail: card.description.clone().unwrap_or_default(),
                    })
                    .collect(),
            })
            .collect();
        let adding_lane = self
            .adding_card
            .as_deref()
            .and_then(|id| lists.iter().position(|l| l.id == id));

        ProjectsView {
            lanes,
            adding_lane,
            boards: self
                .boards
                .iter()
                .map(|b| BoardTab {
                    id: b.id.clone(),
                    title: b.title.clone(),
                    active: b.id == self.active,
                })
                .collect(),
            has_active_board: !self.active.is_empty(),
            columns,
            new_board_title: self.new_board_title.clone(),
            new_list_title: self.new_list_title.clone(),
            card_title: self.card_title.clone(),
            loading: self.loading,
            error: self.error.clone(),
        }
    }
}

type Notify = Arc<dyn Fn(ProjectsView) + Send + Sync>;

struct Shared {
    api: Arc<dyn ProjectsApi>,
    state: Mutex<State>,
    notify: Notify,
}

/// The Projects page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct ProjectsController {
    shared: Arc<Shared>,
}

impl ProjectsController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn ProjectsApi>,
        notify: impl Fn(ProjectsView) + Send + Sync + 'static,
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
    pub fn view(&self) -> ProjectsView {
        self.lock().view()
    }

    /// Loads the boards, lists and cards. The first board becomes active when none is.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared.api.list(Box::new(move |result| {
            me.change(|s| {
                s.loading = false;
                match result {
                    Ok(data) => {
                        s.boards = data.boards;
                        s.lists = data.lists;
                        s.cards = data.cards;
                        if s.active.is_empty()
                            && let Some(first) = s.boards.first()
                        {
                            s.active = first.id.clone();
                        }
                        // A good answer keeps the error of the write that triggered this refresh until dismissed.
                    }
                    Err(e) => s.error = Some(e.to_string()),
                }
            });
        }));
    }

    /// Makes `board_id` the active board (a tab click).
    pub fn select_board(&self, board_id: &str) {
        self.change(|s| {
            if s.boards.iter().any(|b| b.id == board_id) {
                s.active = board_id.to_owned();
                s.adding_card = None;
            }
        });
    }

    pub fn set_new_board_title(&self, title: &str) {
        self.change(|s| s.new_board_title = title.to_owned());
    }

    pub fn set_new_list_title(&self, title: &str) {
        self.change(|s| s.new_list_title = title.to_owned());
    }

    pub fn set_card_title(&self, title: &str) {
        self.change(|s| s.card_title = title.to_owned());
    }

    /// Creates a board from the new-board field, then its four default columns. Returns `false`, and calls nothing,
    /// when the title is blank.
    pub fn create_board(&self) -> bool {
        let title = {
            let state = self.lock();
            state.new_board_title.trim().to_owned()
        };
        if title.is_empty() {
            return false;
        }
        let me = self.clone();
        self.shared.api.create(
            NewItem::Board {
                title: title.clone(),
            },
            Box::new(move |result| match result {
                Ok(id) => {
                    me.change(|s| {
                        s.boards.push(Board {
                            id: id.clone(),
                            title,
                        });
                        s.active = id.clone();
                        s.new_board_title.clear();
                    });
                    me.create_default_list(id, 0);
                }
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// Creates default column `index` of `board_id`, then the next one. A failed column is reported and skipped.
    fn create_default_list(&self, board_id: String, index: usize) {
        let Some(title) = DEFAULT_LISTS.get(index) else {
            return;
        };
        let me = self.clone();
        let title = (*title).to_owned();
        let item = NewItem::List {
            board_id: board_id.clone(),
            title: title.clone(),
            order: index as i64,
        };
        self.shared.api.create(
            item,
            Box::new(move |result| {
                match result {
                    Ok(id) => me.change(|s| {
                        s.lists.push(List {
                            id,
                            board_id: Some(board_id.clone()),
                            title,
                            order: index as i64,
                        });
                    }),
                    Err(e) => me.fail(e),
                }
                me.create_default_list(board_id, index + 1);
            }),
        );
    }

    /// Adds a column to the active board from the new-column field. Returns `false`, and calls nothing, when the
    /// title is blank or no board is active.
    pub fn create_list(&self) -> bool {
        let (board_id, title, order) = {
            let state = self.lock();
            let title = state.new_list_title.trim().to_owned();
            if title.is_empty() || state.active.is_empty() {
                return false;
            }
            // The new list goes last on the active board: its order is the count of that board's lists.
            let order = state
                .lists
                .iter()
                .filter(|l| l.board_id.as_deref() == Some(state.active.as_str()))
                .count() as i64;
            (state.active.clone(), title, order)
        };
        let me = self.clone();
        let board_for_state = board_id.clone();
        let title_for_state = title.clone();
        self.shared.api.create(
            NewItem::List {
                board_id,
                title,
                order,
            },
            Box::new(move |result| match result {
                Ok(id) => me.change(|s| {
                    s.lists.push(List {
                        id,
                        board_id: Some(board_for_state),
                        title: title_for_state,
                        order,
                    });
                    s.new_list_title.clear();
                }),
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// The list id of lane `lane` on the active board (its index in [`ProjectsView::lanes`]), or `None` when there is
    /// no such lane.
    pub fn list_id_at(&self, lane: usize) -> Option<String> {
        let state = self.lock();
        state.current_lists().get(lane).map(|l| l.id.clone())
    }

    /// Opens the "Task card title" input in column `list_id`.
    pub fn start_card(&self, list_id: &str) {
        self.change(|s| {
            s.adding_card = Some(list_id.to_owned());
            s.card_title.clear();
        });
    }

    /// Closes the card input and clears its text.
    pub fn cancel_card(&self) {
        self.change(|s| {
            s.adding_card = None;
            s.card_title.clear();
        });
    }

    /// Creates a card in the column whose input is open, from the card field. Returns `false`, and calls nothing,
    /// when the title is blank or no input is open.
    pub fn submit_card(&self) -> bool {
        let (list_id, title, order) = {
            let state = self.lock();
            let title = state.card_title.trim().to_owned();
            match state.adding_card.clone() {
                Some(list_id) if !title.is_empty() => (list_id, title, state.cards.len() as i64),
                _ => return false,
            }
        };
        let me = self.clone();
        let list_for_state = list_id.clone();
        let title_for_state = title.clone();
        self.shared.api.create(
            NewItem::Card {
                list_id,
                title,
                order,
            },
            Box::new(move |result| match result {
                Ok(id) => me.change(|s| {
                    s.cards.push(Card {
                        id,
                        list_id: Some(list_for_state),
                        title: title_for_state,
                        description: None,
                        order,
                    });
                    s.adding_card = None;
                    s.card_title.clear();
                }),
                Err(e) => me.fail(e),
            }),
        );
        true
    }

    /// Moves card `card_id` into lane `lane` (an index into [`ProjectsView::lanes`], as the shared board reports a
    /// drop, a keyboard step or a drop target). Returns `false`, and calls nothing, when the card or the lane is
    /// unknown. The card moves at once; if the server refuses, the page reloads so the card goes back.
    pub fn move_card(&self, card_id: &str, lane: usize) -> bool {
        let Some(target_list_id) = self.list_id_at(lane) else {
            return false;
        };
        let known = self.lock().cards.iter().any(|c| c.id == card_id);
        if !known {
            return false;
        }
        self.change(|s| {
            for card in s.cards.iter_mut().filter(|c| c.id == card_id) {
                card.list_id = Some(target_list_id.clone());
            }
        });
        let me = self.clone();
        self.shared.api.move_card(
            card_id,
            &target_list_id,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.fail(e);
                    me.refresh();
                }
            }),
        );
        true
    }

    /// Deletes card `card_id`. The card disappears at once; a refused delete reloads the page.
    pub fn delete_card(&self, card_id: &str) -> bool {
        let known = self.lock().cards.iter().any(|c| c.id == card_id);
        if !known {
            return false;
        }
        self.change(|s| s.cards.retain(|c| c.id != card_id));
        let me = self.clone();
        self.shared.api.delete_card(
            card_id,
            Box::new(move |result| {
                if let Err(e) = result {
                    me.fail(e);
                    me.refresh();
                }
            }),
        );
        true
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| s.error = Some(error.to_string()));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, update: impl FnOnce(&mut State)) {
        let view = {
            let mut state = self.lock();
            update(&mut state);
            state.view()
        };
        (self.shared.notify)(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records each call. Creates answer with ids `id-1`, `id-2`, ...; writes answer from scripted replies.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        data: Mutex<Option<ProjectsData>>,
        next_id: Mutex<u32>,
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

    impl ProjectsApi for Arc<Fake> {
        fn list(&self, on_done: Done<ProjectsData>) {
            self.record("list".to_owned());
            let data = self.data.lock().unwrap().clone().unwrap_or_else(sample);
            on_done(Ok(data));
        }

        fn create(&self, item: NewItem, on_done: Done<String>) {
            let call = match &item {
                NewItem::Board { title } => format!("create:board:{title}"),
                NewItem::List {
                    board_id,
                    title,
                    order,
                } => format!("create:list:{board_id}:{title}:{order}"),
                NewItem::Card {
                    list_id,
                    title,
                    order,
                } => format!("create:card:{list_id}:{title}:{order}"),
            };
            self.record(call);
            // Release the counter before answering: the answer can start another create, which takes it again.
            let id = {
                let mut next = self.next_id.lock().unwrap();
                *next += 1;
                format!("id-{next}")
            };
            match self.next_write() {
                Ok(()) => on_done(Ok(id)),
                Err(e) => on_done(Err(e)),
            }
        }

        fn move_card(&self, card_id: &str, list_id: &str, on_done: Done<()>) {
            self.record(format!("move:{card_id}:{list_id}"));
            on_done(self.next_write());
        }

        fn delete_card(&self, card_id: &str, on_done: Done<()>) {
            self.record(format!("delete:{card_id}"));
            on_done(self.next_write());
        }
    }

    fn sample() -> ProjectsData {
        ProjectsData {
            boards: vec![
                Board {
                    id: "b1".into(),
                    title: "Work".into(),
                },
                Board {
                    id: "b2".into(),
                    title: "Home".into(),
                },
            ],
            lists: vec![
                List {
                    id: "l1".into(),
                    board_id: Some("b1".into()),
                    title: "Todo".into(),
                    order: 0,
                },
                List {
                    id: "l2".into(),
                    board_id: Some("b1".into()),
                    title: "Doing".into(),
                    order: 1,
                },
                List {
                    id: "l3".into(),
                    board_id: Some("b2".into()),
                    title: "Chores".into(),
                    order: 0,
                },
            ],
            cards: vec![
                Card {
                    id: "c1".into(),
                    list_id: Some("l1".into()),
                    title: "Write spec".into(),
                    description: Some("draft".into()),
                    order: 0,
                },
                Card {
                    id: "c2".into(),
                    list_id: Some("l2".into()),
                    title: "Build".into(),
                    description: None,
                    order: 1,
                },
            ],
        }
    }

    fn controller(fake: &Arc<Fake>) -> (ProjectsController, Arc<Mutex<Vec<ProjectsView>>>) {
        let seen: Arc<Mutex<Vec<ProjectsView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn ProjectsApi> = Arc::new(fake.clone());
        let c = ProjectsController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn loaded(fake: &Arc<Fake>) -> ProjectsController {
        let (c, _) = controller(fake);
        c.refresh();
        c
    }

    #[test]
    fn refresh_activates_the_first_board_and_shows_its_columns() {
        let fake = Arc::default();
        let c = loaded(&fake);
        let view = c.view();
        assert!(!view.loading);
        assert!(view.boards[0].active);
        assert_eq!(view.columns.len(), 2, "Work has two columns");
        assert_eq!(view.columns[0].title, "Todo");
        assert_eq!(view.columns[0].count, 1);
        assert_eq!(view.columns[0].cards[0].description, "draft");
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn switching_boards_shows_the_other_columns() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.select_board("b2");
        let view = c.view();
        assert_eq!(view.columns.len(), 1);
        assert_eq!(view.columns[0].title, "Chores");
        assert!(view.boards[1].active);
    }

    #[test]
    fn create_board_refuses_a_blank_title() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.set_new_board_title("   ");
        assert!(!c.create_board());
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn create_board_adds_it_and_its_four_default_columns_in_order() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.set_new_board_title("  Garden  ");
        assert!(c.create_board());
        let calls = fake.calls();
        assert_eq!(calls[1], "create:board:Garden");
        assert_eq!(calls[2], "create:list:id-1:Backlog:0");
        assert_eq!(calls[5], "create:list:id-1:Done:3");
        let view = c.view();
        assert_eq!(view.boards.last().unwrap().title, "Garden");
        assert!(view.boards.last().unwrap().active);
        let titles: Vec<&str> = view.columns.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, DEFAULT_LISTS);
        assert!(view.new_board_title.is_empty());
    }

    #[test]
    fn create_list_needs_a_title_and_an_active_board() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.set_new_list_title("");
        assert!(!c.create_list());
        c.set_new_list_title("Blocked");
        assert!(c.create_list());
        assert!(
            fake.calls()
                .contains(&"create:list:b1:Blocked:2".to_owned())
        );
        let view = c.view();
        assert_eq!(view.columns.last().unwrap().title, "Blocked");
        assert!(view.new_list_title.is_empty());
    }

    #[test]
    fn a_card_is_created_in_the_open_column_and_blank_titles_are_refused() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.start_card("l1");
        assert!(c.view().columns[0].adding);
        c.set_card_title("  ");
        assert!(!c.submit_card());
        c.set_card_title("Review");
        assert!(c.submit_card());
        assert!(fake.calls().contains(&"create:card:l1:Review:2".to_owned()));
        let view = c.view();
        assert_eq!(view.columns[0].count, 2);
        assert!(!view.columns[0].adding);
        assert!(view.card_title.is_empty());
    }

    #[test]
    fn cancelling_the_card_input_clears_it() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.start_card("l1");
        c.set_card_title("half typed");
        c.cancel_card();
        let view = c.view();
        assert!(!view.columns[0].adding);
        assert!(view.card_title.is_empty());
        assert!(!c.submit_card());
    }

    #[test]
    fn move_targets_are_the_neighbouring_columns() {
        let fake = Arc::default();
        let c = loaded(&fake);
        let view = c.view();
        let first = &view.columns[0].cards[0];
        assert!(first.move_left.is_none());
        assert_eq!(first.move_right.as_ref().unwrap().title, "Doing");
        let second = &view.columns[1].cards[0];
        assert_eq!(second.move_left.as_ref().unwrap().list_id, "l1");
        assert!(second.move_right.is_none());
    }

    #[test]
    fn move_updates_the_column_and_sends_id_with_the_list() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(c.move_card("c1", 1));
        assert!(fake.calls().contains(&"move:c1:l2".to_owned()));
        let view = c.view();
        assert_eq!(view.columns[0].count, 0);
        assert_eq!(view.columns[1].count, 2);
        assert!(!c.move_card("nope", 1));
        assert!(!c.move_card("c1", 9), "no lane 9 on the board");
    }

    #[test]
    fn lanes_mirror_the_columns_in_board_shape() {
        let fake = Arc::default();
        let c = loaded(&fake);
        let view = c.view();
        assert_eq!(view.lanes.len(), view.columns.len());
        assert_eq!(view.lanes[0].name, "Todo");
        assert_eq!(view.lanes[0].cards[0].key, "c1");
        assert_eq!(view.lanes[0].cards[0].detail, "draft");
        assert_eq!(view.adding_lane, None);
        c.start_card("l2");
        assert_eq!(c.view().adding_lane, Some(1));
        assert_eq!(c.list_id_at(1).as_deref(), Some("l2"));
        assert_eq!(c.list_id_at(5), None);
    }

    #[test]
    fn a_refused_move_reports_the_error_and_reloads() {
        let fake: Arc<Fake> = Arc::default();
        let c = loaded(&fake);
        fake.writes.lock().unwrap().push_back(Err(ApiError::Status {
            code: 400,
            body: "id required".into(),
        }));
        c.move_card("c1", 1);
        assert_eq!(fake.calls(), ["list", "move:c1:l2", "list"]);
        assert_eq!(c.view().error.as_deref(), Some("HTTP 400: id required"));
    }

    #[test]
    fn delete_removes_the_card() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(c.delete_card("c2"));
        assert_eq!(fake.calls(), ["list", "delete:c2"]);
        assert_eq!(c.view().columns[1].count, 0);
    }

    #[test]
    fn a_failed_create_sets_the_error_and_adds_nothing() {
        let fake: Arc<Fake> = Arc::default();
        let c = loaded(&fake);
        fake.writes
            .lock()
            .unwrap()
            .push_back(Err(ApiError::Transport("down".into())));
        c.start_card("l1");
        c.set_card_title("Oops");
        c.submit_card();
        let view = c.view();
        assert_eq!(view.columns[0].count, 1);
        assert_eq!(view.error.as_deref(), Some("network error: down"));
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }
}
