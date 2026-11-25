use std::fmt;
use std::sync::Arc;

use ehttp::{Headers, Method, Request};

/// Default API root. The backend moved from personal.idantity.me to idantity.me (2026-11-22), under `/api/personal`
/// (protected by a Cloudflare Access application). Callers can override it with their own base (the desktop `--api` flag).
pub const DEFAULT_API_BASE: &str = "https://idantity.me/api/personal";

use crate::finance::{FinanceData, decode_finances};
use crate::habits::{HabitAction, HabitData, decode_habits};
use crate::notes::{
    NewNote, Note, NoteBatch, NoteBatchAction, NotePatch, decode_note, decode_notes,
};
use crate::projects::{Card, CardPatch, NewProjectCard, Projects, decode_card, decode_projects};
use crate::todo::{
    NewDatedTodo, NewQuickTodo, NewTodo, Todo, TogglePatch, decode_ack, decode_todo, decode_todos,
};

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The request never completed (DNS, connection refused, CORS, etc.).
    Transport(String),
    /// The server answered with a non-2xx status.
    Status { code: u16, body: String },
    /// The body did not match the expected shape.
    Decode(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "network error: {e}"),
            Self::Status { code, body } => write!(f, "HTTP {code}: {body}"),
            Self::Decode(e) => write!(f, "unexpected response: {e}"),
        }
    }
}

/// One credential sent with every API request as a request header, for example `cf-access-token: <jwt>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderToken {
    pub name: String,
    pub value: String,
}

/// Supplies the credential that [`ApiClient`] sends with each request.
///
/// It is read once per request, so a sign-out or a refreshed token takes effect at the next call. A source that returns
/// `None` sends the request without the header (the API answers as signed out). The web build returns `None`: the browser
/// sends its Cloudflare Access cookie with same-origin requests.
pub trait BearerSource: Send + Sync {
    fn credential(&self) -> Option<HeaderToken>;
}

/// Calls the personal.idantity.me API. Callbacks run on ehttp's worker thread (native) or the browser event loop (wasm),
/// so UI code must hand results back through shared state and call `request_repaint`.
///
/// Every API request goes through [`ApiClient::send`], which adds the bearer token when a [`BearerSource`] is set.
#[derive(Clone)]
pub struct ApiClient {
    base: String,
    bearer: Option<Arc<dyn BearerSource>>,
}

impl fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The bearer source is not printed: it may hold a session token.
        f.debug_struct("ApiClient")
            .field("base", &self.base)
            .field("has_bearer_source", &self.bearer.is_some())
            .finish()
    }
}

impl ApiClient {
    /// `base` is the API root, for example `http://localhost:5173/api` or `/api`.
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            bearer: None,
        }
    }

    /// The same client, sending the token from `source` with every request.
    pub fn with_bearer(mut self, source: Arc<dyn BearerSource>) -> Self {
        self.bearer = Some(source);
        self
    }

    pub fn list(&self, on_done: impl FnOnce(Result<Vec<Todo>, ApiError>) + Send + 'static) {
        self.send(Request::get(self.url("/todos")), decode_todos, on_done);
    }

    pub fn create(
        &self,
        title: &str,
        on_done: impl FnOnce(Result<Todo, ApiError>) + Send + 'static,
    ) {
        let body = to_json(&NewTodo { title });
        self.send(
            json_request(Method::POST, self.url("/todos"), body),
            decode_todo,
            on_done,
        );
    }

    pub fn create_dated(
        &self,
        todo: &NewDatedTodo<'_>,
        on_done: impl FnOnce(Result<Todo, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/todos"), to_json(todo)),
            decode_todo,
            on_done,
        );
    }

    /// `POST /api/todos` with the quick-create modal's body (category, priority, `isCompleted: false`).
    pub fn create_quick(
        &self,
        todo: &NewQuickTodo<'_>,
        on_done: impl FnOnce(Result<Todo, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/todos"), to_json(todo)),
            decode_todo,
            on_done,
        );
    }

    pub fn set_completed(
        &self,
        id: &str,
        is_completed: bool,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = to_json(&TogglePatch { id, is_completed });
        self.send(
            json_request(Method::PATCH, self.url("/todos"), body),
            decode_ack,
            on_done,
        );
    }

    pub fn delete(&self, id: &str, on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static) {
        self.delete_by_id("/todos", id, on_done);
    }

    pub fn delete_note(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.delete_by_id("/notes", id, on_done);
    }

    fn delete_by_id(
        &self,
        path: &str,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = format!("{}?id={}", self.url(path), percent_encode(id));
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ack,
            on_done,
        );
    }

    pub fn habits(&self, on_done: impl FnOnce(Result<HabitData, ApiError>) + Send + 'static) {
        self.send(Request::get(self.url("/habits")), decode_habits, on_done);
    }

    pub fn finances(&self, on_done: impl FnOnce(Result<FinanceData, ApiError>) + Send + 'static) {
        self.send(
            Request::get(self.url("/finances")),
            decode_finances,
            on_done,
        );
    }

    pub fn habit_action(
        &self,
        action: &HabitAction<'_>,
        on_done: impl FnOnce(Result<HabitData, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/habits"), to_json(action)),
            decode_habits,
            on_done,
        );
    }

    pub fn notes(&self, on_done: impl FnOnce(Result<Vec<Note>, ApiError>) + Send + 'static) {
        self.send(Request::get(self.url("/notes")), decode_notes, on_done);
    }

    pub fn create_note(
        &self,
        note: &NewNote<'_>,
        on_done: impl FnOnce(Result<Note, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/notes"), to_json(note)),
            decode_note,
            on_done,
        );
    }

    pub fn update_note(
        &self,
        patch: &NotePatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::PATCH, self.url("/notes"), to_json(patch)),
            decode_ack,
            on_done,
        );
    }

    /// Batch actions share `POST /api/notes` with `create_note`. The response body is
    /// ignored; only the status matters.
    pub fn batch_notes(
        &self,
        action: NoteBatchAction,
        ids: &[String],
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = to_json(&NoteBatch { action, ids });
        self.send(
            json_request(Method::POST, self.url("/notes"), body),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// `GET /api/projects/cards`: the signed-in user's boards, lists and cards.
    pub fn projects(&self, on_done: impl FnOnce(Result<Projects, ApiError>) + Send + 'static) {
        self.send(
            Request::get(self.url("/projects/cards")),
            decode_projects,
            on_done,
        );
    }

    /// `POST /api/projects/cards`. See the TODO on [`NewProjectCard`] about the web API's insert.
    pub fn create_project_card(
        &self,
        card: &NewProjectCard<'_>,
        on_done: impl FnOnce(Result<Card, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/projects/cards"), to_json(card)),
            decode_card,
            on_done,
        );
    }

    /// `PATCH /api/projects/cards`. Only the fields set on `patch` are sent.
    pub fn update_project_card(
        &self,
        patch: &CardPatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::PATCH, self.url("/projects/cards"), to_json(patch)),
            decode_ack,
            on_done,
        );
    }

    /// `DELETE /api/projects/cards?cardId=<id>`.
    pub fn delete_project_card(
        &self,
        card_id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = format!(
            "{}?cardId={}",
            self.url("/projects/cards"),
            percent_encode(card_id)
        );
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ack,
            on_done,
        );
    }

    /// Whether the server answers at all. Any HTTP answer counts (the unauthenticated API answers 401 or an Access
    /// redirect); only a request that never completes (offline, DNS, refused) is `false`. Sends no session token. The
    /// sign-in driver uses it for the start-up check and the periodic connection check.
    pub fn reachable(&self, on_done: impl FnOnce(bool) + Send + 'static) {
        ehttp::fetch(Request::get(self.url("/todos")), move |result| {
            on_done(result.is_ok());
        });
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Sends an API request with the bearer token (when a source is set) and hands the decoded result to `on_done`.
    pub(crate) fn send<T: Send + 'static>(
        &self,
        mut request: Request,
        decode: impl FnOnce(&[u8]) -> Result<T, ApiError> + Send + 'static,
        on_done: impl FnOnce(Result<T, ApiError>) + Send + 'static,
    ) {
        if let Some(credential) = self.bearer.as_ref().and_then(|source| source.credential()) {
            request
                .headers
                .headers
                .push((credential.name, credential.value));
        }
        send_anonymous(request, decode, on_done);
    }
}

fn to_json(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("request bodies are plain structs and always serialize")
}

pub(crate) fn json_request(method: Method, url: String, body: Vec<u8>) -> Request {
    let mut request = Request::new(
        method,
        url,
        Headers::new(&[("Content-Type", "application/json")]),
    );
    request.body = body;
    request
}

/// Sends `request` without any session token. Used for calls to other hosts (the FX rates), which must never receive the
/// bearer, and as the transport under [`ApiClient::send`].
pub(crate) fn send_anonymous<T: Send + 'static>(
    request: Request,
    decode: impl FnOnce(&[u8]) -> Result<T, ApiError> + Send + 'static,
    on_done: impl FnOnce(Result<T, ApiError>) + Send + 'static,
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

/// Encodes a query value. Todo ids are UUIDs, so only reserved characters matter.
pub(crate) fn percent_encode(value: &str) -> String {
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
