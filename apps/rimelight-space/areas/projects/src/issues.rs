//! Projects > Issues (S1.3): the issue list of one project at `/p/:key/issues`, on the page data pattern (`space_page_kit::page_data`).
//!
//! The query bar is sent as `q` (decision 0028): Enter sends it at once, and a change sends it [`DEBOUNCE`] after the last
//! change. A query the server refuses (`invalid_query`) is shown under the bar, with its span marked, and the list keeps no
//! rows for it. The chips toggle a clause of the query text, and a sortable column header writes `sort by:`. "Load more"
//! appends the next page by cursor. A row opens `/p/:key/issues/:key`. New issue (for a member, not a viewer) opens the
//! dialog of `new_issue.rs`. The page's markup is `ui/issues.slint`.
//!
//! Plain Rust. The window side is [`IssuesWindow`], which `wire_projects!` implements for the generated window. The query
//! helpers here are plain Rust, so the unit tests check them without a window.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode, Weak};
use space_client::error::ClientError;
use space_client::projects::IssueFilter;
use space_proto::projects::{Area, Issue, IssueSummary, Page, Project, UpdateIssue, WorkflowState};

use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataCell};
use space_page_kit::person::initials;

/// How long the query bar waits after the last change before the server is asked.
pub const DEBOUNCE: Duration = Duration::from_millis(400);
/// The issues per page that the server is asked for.
pub const PAGE_SIZE: u32 = 50;

/// The colour an area without a readable colour shows in: the default area blue.
pub const NEUTRAL_AREA: (u8, u8, u8) = (0x16, 0x59, 0xC4);

/// The quick filter chips, in the page's order: the label, and the clause a chip toggles. The page sends the chip's index.
pub const CHIPS: [(&str, &str); 3] = [
    ("Assigned to me", "assignee: me"),
    ("Unresolved", "state: #Unresolved"),
    ("Current sprint", "sprint: current"),
];

/// The sort fields a column header can write (decision 0028).
pub const SORT_FIELDS: [&str; 7] = [
    "priority", "updated", "created", "resolved", "estimate", "state", "key",
];

/// The route of one project's issue list.
pub fn list_route(key: &str) -> String {
    format!("/p/{key}/issues")
}

/// The route of one issue, `/p/:id/issues/:id`, with the issue's key as the id.
pub fn issue_route(key: &str, issue_key: &str) -> String {
    format!("/p/{key}/issues/{issue_key}")
}

/// The project key of an issue list route (`/p/<key>/issues`), or `None` for any other route.
pub fn project_of(route: &str) -> Option<&str> {
    let (key, tail) = route.strip_prefix("/p/")?.split_once('/')?;
    let plain = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    (plain && tail == "issues").then_some(key)
}

/// Whether a member with `role` can create issues: a member or an admin, not a viewer. No role (not a member) cannot.
pub fn can_create(role: Option<&str>) -> bool {
    matches!(role, Some("admin" | "member"))
}

/// The colour of a `#rrggbb` hex as its red, green and blue bytes, or `None` when it is not one.
pub fn parse_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// The colour of a `#rrggbb` hex, or the neutral area blue when it is not one.
pub fn colour_rgb(hex: &str) -> (u8, u8, u8) {
    parse_rgb(hex).unwrap_or(NEUTRAL_AREA)
}

/// The byte span of `clause` in `query` as a whole clause (whitespace or the ends on both sides), ignoring ASCII case.
fn find_clause(query: &str, clause: &str) -> Option<(usize, usize)> {
    let haystack = query.to_ascii_lowercase();
    let needle = clause.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        let starts_clause = start == 0 || haystack[..start].ends_with(char::is_whitespace);
        let ends_clause = end == haystack.len() || haystack[end..].starts_with(char::is_whitespace);
        if starts_clause && ends_clause {
            return Some((start, end));
        }
        from = end;
    }
    None
}

/// Joins two pieces of a query with one space, leaving out an empty piece.
fn join(left: &str, right: &str) -> String {
    match (left.trim(), right.trim()) {
        ("", right) => right.to_owned(),
        (left, "") => left.to_owned(),
        (left, right) => format!("{left} {right}"),
    }
}

/// Whether `query` has `clause` as a whole clause.
pub fn has_clause(query: &str, clause: &str) -> bool {
    find_clause(query, clause).is_some()
}

/// `query` with `clause` added at the end, or taken out when it is already there.
pub fn toggle_clause(query: &str, clause: &str) -> String {
    match find_clause(query, clause) {
        Some((start, end)) => join(&query[..start], &query[end..]),
        None => join(query, clause),
    }
}

/// A `sort by:` clause of a query: its byte span, and its first key with whether that key is descending.
struct SortClause {
    start: usize,
    end: usize,
    field: String,
    desc: bool,
}

/// The whitespace-separated words of `text`, as byte spans.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices() {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(index),
            (true, Some(begin)) => {
                spans.push((begin, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        spans.push((begin, text.len()));
    }
    spans
}

/// The first `sort by:` clause of `query`. Keys are `field [asc|desc]`, comma-separated, and the clause ends at the first
/// word that is not a sort field, so clauses after it are kept.
fn find_sort(query: &str) -> Option<SortClause> {
    let haystack = query.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = haystack[from..].find("sort by:") {
        let start = from + at;
        let body = start + "sort by:".len();
        from = body;
        if start != 0 && !haystack[..start].ends_with(char::is_whitespace) {
            continue;
        }
        let text = &query[body..];
        let words = word_spans(text);
        let mut first: Option<(String, bool)> = None;
        let mut end = body;
        let mut index = 0;
        let mut more = true;
        while more && index < words.len() {
            let (s, e) = words[index];
            let word = &text[s..e];
            let (key, mut comma) = match word.strip_suffix(',') {
                Some(key) => (key, true),
                None => (word, false),
            };
            let key_lower = key.to_ascii_lowercase();
            if !SORT_FIELDS.contains(&key_lower.as_str()) {
                break;
            }
            end = body + s + key.len();
            index += 1;
            let mut desc = false;
            // The direction may carry the comma that follows the key (`priority desc, key`).
            if let Some(&(ds, de)) = words.get(index) {
                let raw = &text[ds..de];
                let (direction, direction_comma) = match raw.strip_suffix(',') {
                    Some(direction) => (direction, true),
                    None => (raw, false),
                };
                match direction.to_ascii_lowercase().as_str() {
                    "asc" | "desc" => {
                        desc = direction.eq_ignore_ascii_case("desc");
                        end = body + ds + direction.len();
                        comma = direction_comma;
                        index += 1;
                    }
                    _ => {}
                }
            }
            if first.is_none() {
                first = Some((key_lower, desc));
            }
            more = comma;
        }
        if let Some((field, desc)) = first {
            return Some(SortClause {
                start,
                end,
                field,
                desc,
            });
        }
    }
    None
}

/// The field and direction of the query's `sort by:` clause, if it has one.
pub fn current_sort(query: &str) -> Option<(String, bool)> {
    find_sort(query).map(|sort| (sort.field, sort.desc))
}

/// `query` with its sort clause set to `field`, ascending or descending. An existing sort clause is replaced.
pub fn set_sort(query: &str, field: &str, desc: bool) -> String {
    let rest = match find_sort(query) {
        Some(sort) => join(&query[..sort.start], &query[sort.end..]),
        None => query.trim().to_owned(),
    };
    let direction = if desc { "desc" } else { "asc" };
    join(&rest, &format!("sort by: {field} {direction}"))
}

/// The query after a click on column `field`: ascending first, then descending, then ascending again. A field that is not a
/// sort field leaves the query as it is.
pub fn sort_click(query: &str, field: &str) -> String {
    if !SORT_FIELDS.contains(&field) {
        return query.to_owned();
    }
    let desc = matches!(current_sort(query), Some((ref current, false)) if current == field);
    set_sort(query, field, desc)
}

/// The three parts of a query around the span a refused query names: before, the span, and after. Offsets are character
/// offsets (end exclusive), and are clamped to the query.
pub fn error_parts(query: &str, start: usize, end: usize) -> (String, String, String) {
    let chars: Vec<char> = query.chars().collect();
    let start = start.min(chars.len());
    let end = end.clamp(start, chars.len());
    (
        chars[..start].iter().collect(),
        chars[start..end].iter().collect(),
        chars[end..].iter().collect(),
    )
}

/// How long ago an issue was updated, for the Updated column. `updated` and `now` are unix seconds.
pub fn relative_time(updated: i64, now: i64) -> String {
    let secs = (now - updated).max(0);
    let minutes = secs / 60;
    let hours = secs / 3600;
    let days = secs / 86_400;
    if secs < 60 {
        "just now".to_owned()
    } else if minutes < 60 {
        format!("{minutes} min ago")
    } else if hours < 24 {
        format!("{hours} h ago")
    } else if days < 30 {
        format!("{days} d ago")
    } else {
        format!("{} mo ago", days / 30)
    }
}

/// The issue keys of the list on show, in list order, with the project they belong to. The issue page reads it for its
/// previous and next arrows (`N of M`), and does nothing when the list is for another project.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListKeys {
    pub project: String,
    pub keys: Vec<String>,
}

/// The shared handle to [`ListKeys`]: the list writes it, the issue page reads it.
pub type SharedKeys = Arc<Mutex<ListKeys>>;

/// One row of the issue table as the window draws it (plain values).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueRowView {
    pub key: String,
    pub issue_type: String,
    pub summary: String,
    pub state: String,
    pub priority: String,
    pub assignee: String,
    pub updated: String,
}

/// One area chip on a board card: its name and its colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardAreaView {
    pub name: String,
    pub colour: (u8, u8, u8),
}

/// One card of the board as the window draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardCardView {
    pub key: String,
    pub summary: String,
    pub issue_type: String,
    pub priority: String,
    pub assignee: String,
    /// The estimate ("3 pt"), empty when the issue has none.
    pub meta: String,
    pub areas: Vec<BoardAreaView>,
}

/// One column of the board as the window draws it: the state's name and its cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumnView {
    pub name: String,
    pub cards: Vec<BoardCardView>,
}

/// The query message under the bar: the server's message and the refused span split into its three parts. All empty when the
/// query is fine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryNote {
    pub message: String,
    pub before: String,
    pub span: String,
    pub after: String,
}

/// The window side of the issue list. `wire_projects!` implements it for the generated window.
pub trait IssuesWindow: ComponentHandle + 'static {
    /// The text of the query bar as the person has it now.
    fn query(&self) -> String;
    fn show_query(&self, query: &str);
    /// The view on show: 0 the table, 1 the board.
    fn show_view_mode(&self, view: i32);
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_loading_more(&self, on: bool);
    fn show_has_more(&self, on: bool);
    fn show_query_note(&self, note: QueryNote);
    /// Whether each chip's clause is in the query, in chip order.
    fn show_chips(&self, on: Vec<bool>);
    fn show_sort(&self, field: &str, desc: bool);
    fn show_rows(&self, rows: Vec<IssueRowView>);
    fn show_board(&self, columns: Vec<BoardColumnView>);
    fn show_board_error(&self, text: &str);
    fn show_can_create(&self, on: bool);
    /// Opens the issue page at `route` (the app shell's issues-open).
    fn open_issues(&self, route: String);
    fn on_is_route(&self, f: impl Fn(String) -> bool + 'static);
    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_search(&self, f: impl Fn(String) + 'static);
    fn on_submit(&self, f: impl Fn(String) + 'static);
    fn on_chip(&self, f: impl Fn(i32) + 'static);
    fn on_sort(&self, f: impl Fn(String) + 'static);
    fn on_load_more(&self, f: impl Fn() + 'static);
    fn on_board_move(&self, f: impl Fn(String, i32) + 'static);
    /// The column index under a card drag's pointer, or -1 (see [`board_column_at`]).
    fn on_column_at(&self, f: impl Fn(f32, f32, f32, i32) -> i32 + 'static);
    fn on_view_changed(&self, f: impl Fn(i32) + 'static);
    fn on_open_issue(&self, f: impl Fn(String) + 'static);
}

/// The request state of the page. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every list request. A result for an older number is dropped.
    generation: u64,
    /// The route the page was last opened at.
    route: String,
    /// The key of the project the list is for.
    project: String,
    /// The query the list was asked with (the text of the bar when it was sent).
    query: String,
    /// The `next_cursor` of the last page, or `None` when the list is complete.
    cursor: Option<String>,
    rows: Vec<IssueSummary>,
    has_more: bool,
    /// The keys of the rows on show, for the issue page's previous and next arrows.
    keys: SharedKeys,
    /// True while the current request appends a page ("Load more") rather than replacing the list.
    appending: bool,
    /// The project's workflow states in position order: the board's columns. Read with the project's role.
    workflow: Vec<WorkflowState>,
    /// The issue areas, for the colour of each area chip on a card. Empty when they could not be read (neutral colour).
    areas: Vec<Area>,
    /// The view on show: 0 the table, 1 the board. Kept for the session; the app has no preferences store to persist it.
    view: i32,
}

/// The issue list page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Issues<W: IssuesWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: IssuesWindow> Clone for Issues<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, requests do nothing and
/// the page stays in its loading state.
pub fn wire<W: IssuesWindow>(ui: &W, data: DataCell) -> Issues<W> {
    let issues = Issues {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };

    ui.on_is_route(|route| project_of(&route).is_some());

    let page = issues.clone();
    ui.on_opened(move |route| page.open(&route));

    let page = issues.clone();
    ui.on_retry(move || {
        let route = lock(&page.shared).route.clone();
        page.open(&route);
    });

    // Each change restarts the timer, and a timer asks only when its text is still the latest one and not yet sent, so the
    // last text is searched once, and Enter does not ask twice.
    let page = issues.clone();
    let debounce = Rc::new(Timer::default());
    let latest = Rc::new(RefCell::new(String::new()));
    ui.on_search(move |text| {
        let query = text;
        *latest.borrow_mut() = query.clone();
        let (page, latest) = (page.clone(), latest.clone());
        debounce.start(TimerMode::SingleShot, DEBOUNCE, move || {
            if *latest.borrow() == query && lock(&page.shared).query != query {
                page.start(Some(query.clone()));
            }
        });
    });

    let page = issues.clone();
    ui.on_submit(move |text| page.start(Some(text)));

    let page = issues.clone();
    ui.on_chip(move |index| page.toggle_chip(index));

    let page = issues.clone();
    ui.on_sort(move |field| page.sort(&field));

    let page = issues.clone();
    ui.on_load_more(move || page.start(None));

    // A card moved to a board column: the issue's key and the column's index.
    let page = issues.clone();
    ui.on_board_move(move |key, target| page.move_card(&key, target));

    // The column under a card drag's pointer. Pure: the board passes its own geometry (see board_column_at).
    ui.on_column_at(board_column_at);

    let page = issues.clone();
    ui.on_view_changed(move |view| lock(&page.shared).view = view);

    // A row opens the issue at its route. The app shell opens that route in the same page tab (issues-open).
    let page = issues.clone();
    ui.on_open_issue(move |key| {
        if let Some(ui) = page.ui.upgrade() {
            let project = lock(&page.shared).project.clone();
            ui.open_issues(issue_route(&project, &key));
        }
    });

    issues
}

impl<W: IssuesWindow> Issues<W> {
    /// The keys of the list on show, shared with the issue page for its previous and next arrows.
    pub fn keys(&self) -> SharedKeys {
        lock(&self.shared).keys.clone()
    }

    /// The generation of the latest list request.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// The query the list was last asked with.
    pub fn query(&self) -> String {
        lock(&self.shared).query.clone()
    }

    /// Delivers a list result for the request with `generation`, as the worker would. A result for an older request is
    /// dropped. For tests: no source is set, so nothing is fetched.
    pub fn deliver(&self, generation: u64, result: Result<Page<IssueSummary>, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver(&ui, &self.shared, generation, result);
    }

    /// Delivers the caller's role in the project (`my_role`), as the worker would. For tests.
    pub fn deliver_role(&self, role: Option<&str>) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.show_can_create(can_create(role));
    }

    /// Opens the page at `route` (`/p/:key/issues`). A new project clears the query. The list and the role are asked again.
    fn open(&self, route: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(key) = project_of(route).map(str::to_owned) else {
            return;
        };
        {
            let mut s = lock(&self.shared);
            s.route = route.to_owned();
            if s.project != key {
                s.project = key.clone();
                s.query.clear();
                s.rows.clear();
                s.cursor = None;
                s.has_more = false;
                s.workflow.clear();
                s.areas.clear();
            }
            ui.show_view_mode(s.view);
        }
        let query = lock(&self.shared).query.clone();
        ui.show_query(&query);
        self.start(Some(query));
        self.fetch_role(&key);
    }

    /// Starts a request: a new list for `Some(query)`, or the next page for `None`. Does nothing for the next page when the
    /// list is complete.
    fn start(&self, query: Option<String>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let appending = query.is_none();
        let (filter, project, generation) = {
            let mut s = lock(&self.shared);
            if appending && !s.has_more {
                return;
            }
            if let Some(query) = query {
                s.query = query;
                s.cursor = None;
                s.rows.clear();
                s.has_more = false;
            }
            s.generation += 1;
            s.appending = appending;
            let filter = IssueFilter {
                q: Some(s.query.clone()),
                cursor: s.cursor.clone(),
                limit: Some(PAGE_SIZE),
                ..IssueFilter::default()
            };
            (filter, s.project.clone(), s.generation)
        };
        if appending {
            ui.show_loading_more(true);
        } else {
            ui.show_state(LoadState::Loading);
            ui.show_error("");
            let s = lock(&self.shared);
            show_query_state(&ui, &s);
            show_rows_state(&ui, &s);
        }

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.issues(&token, &project, &filter).await },
            move |ui, result| deliver(ui, &shared, generation, result),
        );
    }

    /// Asks for the project (its role, for the New issue button, and its workflow, for the board's columns) and the issue
    /// areas (for the board's chip colours, which are cosmetic: without them the areas show in the neutral colour).
    fn fetch_role(&self, key: &str) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let project = key.to_owned();
        let asked = key.to_owned();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let loaded: Project = api.project(&token, &project).await?;
                let areas = api.areas(&token).await.unwrap_or_default();
                Ok::<_, ClientError>((loaded, areas))
            },
            move |ui, result: Result<(Project, Vec<Area>), ClientError>| {
                let mut s = lock(&shared);
                if s.project == asked {
                    let (role, workflow, areas) = match result {
                        Ok((loaded, areas)) => (loaded.my_role, loaded.workflow, areas),
                        Err(_) => (None, Vec::new(), Vec::new()),
                    };
                    s.workflow = workflow;
                    s.areas = areas;
                    show_board_state(ui, &s);
                    ui.show_can_create(can_create(role.as_deref()));
                }
            },
        );
    }

    /// Moves issue `key` to the state of board column `target`. The card moves at once (optimistic). If the server refuses,
    /// the card goes back to its state and the message shows under the board.
    fn move_card(&self, key: &str, target: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Ok(target) = usize::try_from(target) else {
            return;
        };
        let (previous, to) = {
            let mut s = lock(&self.shared);
            let Some((to, _)) = board_groups(&s.workflow, &s.rows).into_iter().nth(target) else {
                return;
            };
            let Some(row) = s.rows.iter_mut().find(|row| row.key == key) else {
                return;
            };
            if row.state.id == to.id {
                return;
            }
            let previous = std::mem::replace(&mut row.state, to.clone());
            show_rows_state(&ui, &s);
            (previous, to)
        };
        ui.show_board_error("");
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let issue_key = key.to_owned();
        let done_key = key.to_owned();
        let update = UpdateIssue {
            state_id: Some(to.id.clone()),
            ..UpdateIssue::default()
        };
        source.fetch(
            &self.ui,
            move |api, token| async move { api.update_issue(&token, &issue_key, &update).await },
            move |ui, result: Result<Issue, ClientError>| {
                if let Err(error) = result {
                    let mut s = lock(&shared);
                    // Put the card back, unless it has been moved again since.
                    if let Some(row) = s.rows.iter_mut().find(|row| row.key == done_key)
                        && row.state.id == to.id
                    {
                        row.state = previous;
                    }
                    ui.show_board_error(&format!(
                        "Could not move {done_key}: {}",
                        page_data::error_text(&error)
                    ));
                    show_rows_state(ui, &s);
                }
            },
        );
    }

    /// The query text with chip `index`'s clause toggled. Sends the new query.
    fn toggle_chip(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(&(_, clause)) = usize::try_from(index).ok().and_then(|i| CHIPS.get(i)) else {
            return;
        };
        let query = toggle_clause(&ui.query(), clause);
        self.apply_query(&ui, query);
    }

    /// Writes the sort for a click on column `field` into the query, and sends it.
    fn sort(&self, field: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !SORT_FIELDS.contains(&field) {
            return;
        }
        let query = sort_click(&ui.query(), field);
        self.apply_query(&ui, query);
    }

    /// Shows `query` in the bar, and asks for the new list.
    fn apply_query(&self, ui: &W, query: String) {
        ui.show_query(&query);
        self.start(Some(query));
    }
}

/// Keeps a result if it is for the latest request, then shows the list, the query message or the error. A query the server
/// refused shows its message under the bar, with the span marked, and no rows.
fn deliver<W: IssuesWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    result: Result<Page<IssueSummary>, ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    ui.show_loading_more(false);
    match result {
        Ok(page) => {
            if s.appending {
                s.rows.extend(page.items);
            } else {
                s.rows = page.items;
            }
            s.cursor = page.next_cursor;
            *lock_keys(&s.keys) = ListKeys {
                project: s.project.clone(),
                keys: s.rows.iter().map(|row| row.key.clone()).collect(),
            };
            s.has_more = s.cursor.is_some();
            ui.show_has_more(s.has_more);
            ui.show_error("");
            ui.show_query_note(QueryNote::default());
            let state = if s.rows.is_empty() {
                LoadState::Empty
            } else {
                LoadState::Loaded
            };
            ui.show_state(state);
            show_rows_state(ui, &s);
        }
        Err(ClientError::InvalidQuery {
            message,
            start,
            end,
        }) => {
            let (before, span, after) = error_parts(&s.query, start, end);
            ui.show_query_note(QueryNote {
                message,
                before,
                span,
                after,
            });
            s.rows.clear();
            *lock_keys(&s.keys) = ListKeys::default();
            s.cursor = None;
            s.has_more = false;
            ui.show_has_more(false);
            ui.show_error("");
            ui.show_state(LoadState::Empty);
            show_rows_state(ui, &s);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// Shows the chips' state and the sort headers for the query the list was asked with.
fn show_query_state<W: IssuesWindow>(ui: &W, s: &Shared) {
    let on: Vec<bool> = CHIPS
        .iter()
        .map(|&(_, clause)| has_clause(&s.query, clause))
        .collect();
    ui.show_chips(on);
    let (field, desc) = current_sort(&s.query).unwrap_or_default();
    ui.show_sort(&field, desc);
}

/// Draws the rows from the request state. The Updated column is relative to now.
fn show_rows_state<W: IssuesWindow>(ui: &W, s: &Shared) {
    let now = chrono::Utc::now().timestamp();
    let rows: Vec<IssueRowView> = s
        .rows
        .iter()
        .map(|issue| IssueRowView {
            key: issue.key.clone(),
            issue_type: issue.issue_type.clone(),
            summary: issue.summary.clone(),
            state: issue.state.name.clone(),
            priority: issue.priority.clone(),
            assignee: issue
                .assignee
                .as_ref()
                .map(|user| initials(&user.display_name))
                .unwrap_or_default(),
            updated: relative_time(issue.updated_at, now),
        })
        .collect();
    ui.show_rows(rows);
    show_board_state(ui, s);
}

/// The board's columns: one per workflow state in position order. A state the workflow does not list (renamed or removed
/// since the project was read) gets a column after them, in the order the rows first meet it. Each column keeps its rows in
/// list order, so the board follows the table's sort. A column with no rows is kept (an empty column).
pub fn board_groups<'a>(
    workflow: &[WorkflowState],
    rows: &'a [IssueSummary],
) -> Vec<(WorkflowState, Vec<&'a IssueSummary>)> {
    let mut states = workflow.to_vec();
    states.sort_by_key(|state| state.position);
    let mut groups: Vec<(WorkflowState, Vec<&IssueSummary>)> = states
        .into_iter()
        .map(|state| (state, Vec::new()))
        .collect();
    for row in rows {
        let index = match groups
            .iter()
            .position(|(state, _)| state.id == row.state.id)
        {
            Some(index) => index,
            None => {
                groups.push((row.state.clone(), Vec::new()));
                groups.len() - 1
            }
        };
        groups[index].1.push(row);
    }
    groups
}

/// The index of the board column a drag's pointer is over, or `-1` when it is over none. `x` is the pointer's x in the
/// board's content in px (zero at the left edge of the first column, so the scroll offset is already taken off), `width`
/// is one column's width, `gap` the space between columns and `count` the number of columns. The gap belongs to the column
/// on its left, so the columns cover the whole run from the first column to the end of the last one.
pub fn board_column_at(x: f32, width: f32, gap: f32, count: i32) -> i32 {
    match drop_column(x, width, gap, usize::try_from(count).unwrap_or(0)) {
        Some(index) => i32::try_from(index).unwrap_or(-1),
        None => -1,
    }
}

/// The drop rule behind [`board_column_at`]: the column index for `x`, or `None` when `x` is left of the board, past its
/// last column, or the geometry is not usable (a NaN, a zero pitch or no columns).
pub fn drop_column(x: f32, width: f32, gap: f32, count: usize) -> Option<usize> {
    let pitch = width + gap;
    // NaN fails `x >= 0.0` and `pitch > 0.0`, so these checks reject NaN exactly as the negated comparisons did.
    if x.is_nan() || x < 0.0 || pitch.is_nan() || pitch <= 0.0 || count == 0 {
        return None;
    }
    let index = (x / pitch).floor() as usize;
    (index < count).then_some(index)
}

/// Draws the board from the request state: a column per workflow state, with the cards of the rows loaded so far.
fn show_board_state<W: IssuesWindow>(ui: &W, s: &Shared) {
    let columns: Vec<BoardColumnView> = board_groups(&s.workflow, &s.rows)
        .iter()
        .map(|(state, rows)| BoardColumnView {
            name: state.name.clone(),
            cards: rows
                .iter()
                .map(|issue| board_card(issue, &s.areas))
                .collect(),
        })
        .collect();
    ui.show_board(columns);
}

/// One card of the board: the issue's key, summary, type, priority, assignee initials and its area chips in their colours.
fn board_card(issue: &IssueSummary, areas: &[Area]) -> BoardCardView {
    let chips = issue
        .tags
        .iter()
        .map(|name| {
            let colour = areas
                .iter()
                .find(|area| &area.name == name)
                .map(|area| colour_rgb(&area.color))
                .unwrap_or(NEUTRAL_AREA);
            BoardAreaView {
                name: name.clone(),
                colour,
            }
        })
        .collect();
    BoardCardView {
        key: issue.key.clone(),
        summary: issue.summary.clone(),
        issue_type: issue.issue_type.clone(),
        priority: issue.priority.clone(),
        assignee: issue
            .assignee
            .as_ref()
            .map(|user| initials(&user.display_name))
            .unwrap_or_default(),
        meta: issue
            .estimate_points
            .map(|points| format!("{points} pt"))
            .unwrap_or_default(),
        areas: chips,
    }
}

/// Locks the request state. A panic in another holder does not stop the page.
fn lock_keys(keys: &Mutex<ListKeys>) -> MutexGuard<'_, ListKeys> {
    keys.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_of_takes_only_the_list_route() {
        assert_eq!(project_of("/p/EMB/issues"), Some("EMB"));
        assert_eq!(project_of("/p/grand-tale/issues"), Some("grand-tale"));
        assert_eq!(
            project_of("/p/EMB/issues/EMB-1"),
            None,
            "a detail route is not the list"
        );
        assert_eq!(project_of("/p/EMB/board"), None);
        assert_eq!(project_of("/p//issues"), None);
        assert_eq!(project_of("/p/../issues"), None);
    }

    #[test]
    fn roles_that_can_create() {
        assert!(can_create(Some("member")));
        assert!(can_create(Some("admin")));
        assert!(!can_create(Some("viewer")));
        assert!(!can_create(None), "not a member");
    }

    #[test]
    fn chips_toggle_whole_clauses_only() {
        let on = toggle_clause("", "assignee: me");
        assert_eq!(on, "assignee: me");
        let both = toggle_clause(&on, "state: #Unresolved");
        assert_eq!(both, "assignee: me state: #Unresolved");
        assert!(has_clause(&both, "assignee: me"));
        assert_eq!(toggle_clause(&both, "assignee: me"), "state: #Unresolved");
        assert_eq!(
            toggle_clause("assignee: meow", "assignee: me"),
            "assignee: meow assignee: me"
        );
        assert_eq!(toggle_clause("ASSIGNEE: ME bug", "assignee: me"), "bug");
    }

    #[test]
    fn a_header_click_sets_asc_then_desc_then_asc() {
        let asc = sort_click("bug", "updated");
        assert_eq!(asc, "bug sort by: updated asc");
        assert_eq!(current_sort(&asc), Some(("updated".to_owned(), false)));
        let desc = sort_click(&asc, "updated");
        assert_eq!(desc, "bug sort by: updated desc");
        assert_eq!(sort_click(&desc, "updated"), "bug sort by: updated asc");
        // Another column replaces the sort, and the clauses around it are kept.
        let moved = sort_click("assignee: me sort by: updated desc state: x", "priority");
        assert_eq!(moved, "assignee: me state: x sort by: priority asc");
        assert_eq!(sort_click("bug", "type"), "bug", "not a sort field");
    }

    #[test]
    fn a_sort_clause_ends_at_the_first_word_that_is_not_a_key() {
        assert_eq!(
            current_sort("sort by: priority desc, key"),
            Some(("priority".to_owned(), true))
        );
        assert_eq!(current_sort("sort by: nonsense"), None);
    }

    #[test]
    fn the_refused_span_is_split_by_characters() {
        assert_eq!(
            error_parts("staet: me", 0, 6),
            ("".to_owned(), "staet:".to_owned(), " me".to_owned())
        );
        assert_eq!(
            error_parts("é: x", 0, 99),
            ("".to_owned(), "é: x".to_owned(), "".to_owned()),
            "offsets are characters and are clamped"
        );
    }

    #[test]
    fn relative_times_read_naturally() {
        assert_eq!(relative_time(100, 110), "just now");
        assert_eq!(relative_time(0, 120), "2 min ago");
        assert_eq!(relative_time(0, 7200), "2 h ago");
        assert_eq!(relative_time(0, 3 * 86_400), "3 d ago");
        assert_eq!(relative_time(0, 90 * 86_400), "3 mo ago");
        assert_eq!(relative_time(500, 100), "just now", "a future time is now");
    }

    fn state(id: &str, name: &str, position: i32) -> WorkflowState {
        WorkflowState {
            id: id.to_owned(),
            name: name.to_owned(),
            category: "Unstarted".to_owned(),
            position,
        }
    }

    fn card(key: &str, state: &WorkflowState) -> IssueSummary {
        IssueSummary {
            id: format!("id-{key}"),
            key: key.to_owned(),
            summary: key.to_owned(),
            issue_type: "Bug".to_owned(),
            priority: "High".to_owned(),
            state: state.clone(),
            assignee: None,
            parent_id: None,
            sprint_id: None,
            estimate_points: None,
            due_date: None,
            tags: Vec::new(),
            backlog_rank: "00000001".to_owned(),
            updated_at: 0,
        }
    }

    fn keys_of<'a>(group: &'a (WorkflowState, Vec<&'a IssueSummary>)) -> Vec<&'a str> {
        group.1.iter().map(|row| row.key.as_str()).collect()
    }

    #[test]
    fn board_columns_follow_the_workflow_order_and_keep_the_list_order() {
        // The workflow arrives out of position order; the columns follow the positions.
        let done = state("s3", "Done", 3);
        let open = state("s1", "Open", 1);
        let doing = state("s2", "In progress", 2);
        let rows = vec![
            card("EMB-3", &doing),
            card("EMB-1", &done),
            card("EMB-2", &open),
            card("EMB-4", &doing),
        ];
        let groups = board_groups(&[done.clone(), open.clone(), doing.clone()], &rows);
        let names: Vec<&str> = groups.iter().map(|(s, _)| s.name.as_str()).collect();
        assert_eq!(names, ["Open", "In progress", "Done"]);
        assert_eq!(keys_of(&groups[1]), ["EMB-3", "EMB-4"], "list order kept");
        assert_eq!(keys_of(&groups[2]), ["EMB-1"]);
    }

    #[test]
    fn an_empty_state_keeps_its_column_and_an_unknown_state_gets_one_after_the_rest() {
        let open = state("s1", "Open", 1);
        let gone = state("s9", "Renamed away", 9);
        let rows = vec![card("EMB-1", &gone)];
        let groups = board_groups(std::slice::from_ref(&open), &rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0.name, "Open");
        assert!(groups[0].1.is_empty(), "a column with no rows is kept");
        assert_eq!(groups[1].0.id, "s9");
        assert_eq!(keys_of(&groups[1]), ["EMB-1"]);
    }

    #[test]
    fn no_workflow_yet_makes_columns_from_the_rows() {
        let open = state("s1", "Open", 1);
        let rows = vec![card("EMB-1", &open), card("EMB-2", &open)];
        let groups = board_groups(&[], &rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(keys_of(&groups[0]), ["EMB-1", "EMB-2"]);
    }

    #[test]
    fn area_colours_read_hex_and_fall_back() {
        assert!(parse_rgb("#1659C4").is_some());
        assert!(parse_rgb("1659C4").is_none(), "needs the #");
        assert!(parse_rgb("#12345").is_none(), "six digits");
        assert!(parse_rgb("#GGGGGG").is_none(), "hex digits only");
        assert_eq!(colour_rgb("#1659C4"), (0x16, 0x59, 0xC4));
        assert_eq!(colour_rgb("nope"), NEUTRAL_AREA);
    }

    #[test]
    fn list_and_issue_routes() {
        assert_eq!(list_route("EMB"), "/p/EMB/issues");
        assert_eq!(issue_route("EMB", "EMB-12"), "/p/EMB/issues/EMB-12");
    }

    #[test]
    fn a_drop_lands_on_the_column_under_the_pointer() {
        // Columns 272 px wide with a 12 px gap: column i starts at i * 284.
        assert_eq!(drop_column(0.0, 272.0, 12.0, 3), Some(0), "left edge");
        assert_eq!(
            drop_column(271.9, 272.0, 12.0, 3),
            Some(0),
            "right edge of column 0"
        );
        assert_eq!(
            drop_column(284.0, 272.0, 12.0, 3),
            Some(1),
            "left edge of column 1"
        );
        assert_eq!(
            drop_column(600.0, 272.0, 12.0, 3),
            Some(2),
            "inside column 2"
        );
    }

    #[test]
    fn the_gap_belongs_to_the_column_on_its_left() {
        assert_eq!(drop_column(275.0, 272.0, 12.0, 3), Some(0));
        assert_eq!(drop_column(283.9, 272.0, 12.0, 3), Some(0));
    }

    #[test]
    fn past_the_board_or_left_of_it_there_is_no_column() {
        assert_eq!(drop_column(-0.5, 272.0, 12.0, 3), None, "left of the board");
        assert_eq!(
            drop_column(3.0 * 284.0, 272.0, 12.0, 3),
            None,
            "past the last column"
        );
        assert_eq!(drop_column(10_000.0, 272.0, 12.0, 3), None, "far right");
    }

    #[test]
    fn unusable_geometry_has_no_column() {
        assert_eq!(drop_column(f32::NAN, 272.0, 12.0, 3), None, "NaN pointer");
        assert_eq!(drop_column(10.0, 0.0, 0.0, 3), None, "zero pitch");
        assert_eq!(drop_column(10.0, 272.0, 12.0, 0), None, "no columns");
    }

    #[test]
    fn one_column_board_and_no_gap() {
        assert_eq!(drop_column(0.0, 100.0, 0.0, 1), Some(0));
        assert_eq!(drop_column(99.0, 100.0, 0.0, 1), Some(0));
        assert_eq!(drop_column(100.0, 100.0, 0.0, 1), None);
    }

    #[test]
    fn the_slint_entry_point_answers_minus_one_for_no_column() {
        assert_eq!(board_column_at(284.0, 272.0, 12.0, 3), 1);
        assert_eq!(board_column_at(-1.0, 272.0, 12.0, 3), -1);
        assert_eq!(
            board_column_at(10.0, 272.0, 12.0, -2),
            -1,
            "a negative count"
        );
    }
}
