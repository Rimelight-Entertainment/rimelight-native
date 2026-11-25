//! Home > Dashboard (S2): the cards that have a backend today, on the page data pattern (`space_page_kit::page_data`).
//!
//! Two cards, each loading on its own (one failing never blanks the other):
//!
//! - **Assigned to me**: the caller's open issues across projects, from `GET /v1/issues/search` with [`ASSIGNED_QUERY`]. The
//!   server orders them (priority, critical first, then the most recently updated); the card shows the first
//!   [`ASSIGNED_ROWS`]. The API gives no total, so the request asks for [`ASSIGNED_FETCH`] issues and the count is the number
//!   that came back, written `50+` when the server says there are more ([`count_text`]).
//! - **Recent canvases**: the caller's personal canvases from `GET /v1/canvases?scope=personal`, newest update first, the
//!   first [`CANVAS_ROWS`]. A row opens `/home/canvases/{id}`, and View all opens `/home/canvases`.
//!
//! Each card is a [`Card`]: its load state, its error text and its rows. A request bumps the card's generation, and a result
//! for an older generation is dropped. The calendar, unread, timer and approvals cards come with their own slices.
//!
//! This module is plain Rust. The window side is [`HomeWindow`], which `wire_home!` implements for the generated window. The
//! page's markup is `ui/home.slint`.

use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, Weak};
use space_client::canvases::CanvasScope;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell};
use space_proto::canvases::CanvasView;
use space_proto::people::Page;
use space_proto::projects::IssueSummary;

/// The Dashboard's route.
pub const ROUTE: &str = "/home";
/// The route of the personal canvases list (the Recent canvases card's View all).
pub const CANVASES_ROUTE: &str = "/home/canvases";
/// The issue query: the caller's issues that are not done or cancelled, critical first, then the latest update first.
pub const ASSIGNED_QUERY: &str =
    "assignee: me state: #Unresolved sort by: priority desc, updated desc";
/// The issues the Assigned card shows.
pub const ASSIGNED_ROWS: usize = 8;
/// The issues one request asks for. The count in the card header is capped here (see [`count_text`]).
pub const ASSIGNED_FETCH: u32 = 50;
/// The canvases the Recent canvases card shows.
pub const CANVAS_ROWS: usize = 5;

/// The route of canvas `id` in the personal list.
pub fn canvas_route(id: &str) -> String {
    format!("{CANVASES_ROUTE}/{id}")
}

/// One issue of the Assigned card, as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueLine {
    /// The route the row opens: `/p/{project}/issues/{key}`.
    pub route: String,
    pub key: String,
    pub summary: String,
    pub state: String,
    pub priority: String,
    /// The project key: the issue key without its number.
    pub project: String,
}

/// What the Assigned card shows once loaded: the rows and the count for the header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assigned {
    pub rows: Vec<IssueLine>,
    /// The header count: `12`, or `50+` when there are more than one request returns. Empty when there are no issues.
    pub count: String,
}

/// One canvas of the Recent canvases card, as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasLine {
    /// The route the row opens: `/home/canvases/{id}`.
    pub route: String,
    pub title: String,
    /// `YYYY-MM-DD HH:MM` from the canvas's ISO update time.
    pub updated: String,
}

/// What the Recent canvases card shows once loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    pub rows: Vec<CanvasLine>,
}

/// A card's loaded content: it knows whether it has nothing to show.
pub trait Content: Default {
    fn is_empty(&self) -> bool;
}

impl Content for Assigned {
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl Content for Recent {
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The project key of an issue key: everything before the last `-` (`EMB-12` is `EMB`). A key with no `-` is its own project.
pub fn project_of(issue_key: &str) -> &str {
    issue_key
        .rsplit_once('-')
        .map_or(issue_key, |(project, _)| project)
}

/// The header count: the number of issues the request returned, with a `+` when the server has more.
pub fn count_text(returned: usize, more: bool) -> String {
    if returned == 0 {
        String::new()
    } else if more {
        format!("{returned}+")
    } else {
        returned.to_string()
    }
}

/// The Assigned card's content from one page of search results: the first [`ASSIGNED_ROWS`] issues in the server's order,
/// and the count of the whole page.
pub fn assigned_from(page: Page<IssueSummary>) -> Assigned {
    let count = count_text(page.items.len(), page.next_cursor.is_some());
    let rows = page
        .items
        .into_iter()
        .take(ASSIGNED_ROWS)
        .map(|issue| {
            let project = project_of(&issue.key).to_string();
            IssueLine {
                route: format!("/p/{project}/issues/{}", issue.key),
                key: issue.key,
                summary: issue.summary,
                state: issue.state.name,
                priority: issue.priority,
                project,
            }
        })
        .collect();
    Assigned { rows, count }
}

/// The time a canvas was updated, as the card shows it: `YYYY-MM-DD HH:MM` from the ISO timestamp.
pub fn updated_text(iso: &str) -> String {
    iso.chars().take(16).collect::<String>().replace('T', " ")
}

/// The Recent canvases card's content: the [`CANVAS_ROWS`] most recently updated canvases, newest first. The ISO update times
/// sort as text.
pub fn recent_from(mut canvases: Vec<CanvasView>) -> Recent {
    canvases.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let rows = canvases
        .into_iter()
        .take(CANVAS_ROWS)
        .map(|canvas| CanvasLine {
            route: canvas_route(&canvas.id),
            title: canvas.title,
            updated: updated_text(&canvas.updated_at),
        })
        .collect();
    Recent { rows }
}

/// One card's request state: what it shows, and the number of the latest request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card<T: Content> {
    generation: u64,
    state: LoadState,
    error: String,
    content: T,
}

impl<T: Content> Default for Card<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            state: LoadState::Loading,
            error: String::new(),
            content: T::default(),
        }
    }
}

impl<T: Content> Card<T> {
    /// Starts a request: the card goes back to loading with nothing shown, and the request's generation is returned.
    pub fn begin(&mut self) -> u64 {
        self.generation += 1;
        self.state = LoadState::Loading;
        self.error.clear();
        self.content = T::default();
        self.generation
    }

    /// Keeps the result of the request `generation` and returns true, or drops it and returns false when a newer request has
    /// begun since.
    pub fn finish(&mut self, generation: u64, result: Result<T, ClientError>) -> bool {
        if self.generation != generation {
            return false;
        }
        match result {
            Ok(content) => {
                self.state = if content.is_empty() {
                    LoadState::Empty
                } else {
                    LoadState::Loaded
                };
                self.error.clear();
                self.content = content;
            }
            Err(error) => {
                self.state = LoadState::Error;
                self.error = page_data::error_text(&error);
                self.content = T::default();
            }
        }
        true
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn state(&self) -> LoadState {
        self.state
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn content(&self) -> &T {
        &self.content
    }
}

/// The window side of the dashboard: what the cards draw and the callbacks the page listens to. `wire_home!` implements it
/// for the generated window. Opening a row is the window's navigation, in space_ui's app shell.
pub trait HomeWindow: ComponentHandle + 'static {
    /// The Assigned card: its load state, its error text, the header count and its rows.
    fn show_assigned(&self, state: LoadState, error: &str, count: &str, rows: &[IssueLine]);
    /// The Recent canvases card: its load state, its error text and its rows.
    fn show_canvases(&self, state: LoadState, error: &str, rows: &[CanvasLine]);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry_assigned(&self, f: impl Fn() + 'static);
    fn on_retry_canvases(&self, f: impl Fn() + 'static);
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    assigned: Card<Assigned>,
    canvases: Card<Recent>,
}

/// The dashboard: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Home<W: HomeWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: HomeWindow> Clone for Home<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, requests do nothing and the
/// cards stay in their loading state.
pub fn wire<W: HomeWindow>(ui: &W, data: DataCell) -> Home<W> {
    let home = Home {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };

    // The page opens (its `init`): both cards load again, so the dashboard is current whenever its route is shown.
    let page = home.clone();
    ui.on_opened(move || {
        page.load_assigned();
        page.load_canvases();
    });
    let page = home.clone();
    ui.on_retry_assigned(move || page.load_assigned());
    let page = home.clone();
    ui.on_retry_canvases(move || page.load_canvases());

    home
}

impl<W: HomeWindow> Home<W> {
    /// The generation of the latest Assigned request.
    pub fn assigned_generation(&self) -> u64 {
        lock(&self.shared).assigned.generation()
    }

    /// The generation of the latest Recent canvases request.
    pub fn canvases_generation(&self) -> u64 {
        lock(&self.shared).canvases.generation()
    }

    /// Delivers a result for the Assigned request with `generation`, as the worker would. A result for an older request is
    /// dropped. For tests: no source is set, so nothing is fetched.
    pub fn deliver_assigned(
        &self,
        generation: u64,
        result: Result<Page<IssueSummary>, ClientError>,
    ) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver_assigned(&ui, &self.shared, generation, result.map(assigned_from));
    }

    /// Delivers a result for the Recent canvases request with `generation`, as the worker would. A result for an older
    /// request is dropped. For tests: no source is set, so nothing is fetched.
    pub fn deliver_canvases(&self, generation: u64, result: Result<Vec<CanvasView>, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver_canvases(&ui, &self.shared, generation, result.map(recent_from));
    }

    /// Starts the Assigned request: the card shows loading, then the rows or the error.
    fn load_assigned(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            let generation = s.assigned.begin();
            draw_assigned(&ui, &s.assigned);
            generation
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.search_issues(&token, ASSIGNED_QUERY, None, Some(ASSIGNED_FETCH))
                    .await
                    .map(assigned_from)
            },
            move |ui, result| deliver_assigned(ui, &shared, generation, result),
        );
    }

    /// Starts the Recent canvases request: the card shows loading, then the rows or the error.
    fn load_canvases(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            let generation = s.canvases.begin();
            draw_canvases(&ui, &s.canvases);
            generation
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.canvases(&token, &CanvasScope::Personal, false)
                    .await
                    .map(recent_from)
            },
            move |ui, result| deliver_canvases(ui, &shared, generation, result),
        );
    }
}

/// Keeps the Assigned result if it is for the latest request, then draws the card.
fn deliver_assigned<W: HomeWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Assigned, ClientError>,
) {
    let mut s = lock(shared);
    if s.assigned.finish(generation, result) {
        draw_assigned(ui, &s.assigned);
    }
}

/// Keeps the Recent canvases result if it is for the latest request, then draws the card.
fn deliver_canvases<W: HomeWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Recent, ClientError>,
) {
    let mut s = lock(shared);
    if s.canvases.finish(generation, result) {
        draw_canvases(ui, &s.canvases);
    }
}

fn draw_assigned<W: HomeWindow>(ui: &W, card: &Card<Assigned>) {
    ui.show_assigned(
        card.state(),
        card.error(),
        &card.content().count,
        &card.content().rows,
    );
}

fn draw_canvases<W: HomeWindow>(ui: &W, card: &Card<Recent>) {
    ui.show_canvases(card.state(), card.error(), &card.content().rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_proto::projects::{UserRef, WorkflowState};

    fn issue(key: &str, summary: &str, priority: &str) -> IssueSummary {
        IssueSummary {
            id: format!("id-{key}"),
            key: key.to_string(),
            summary: summary.to_string(),
            issue_type: "Task".to_string(),
            priority: priority.to_string(),
            state: WorkflowState {
                id: "s1".to_string(),
                name: "In Progress".to_string(),
                category: "started".to_string(),
                position: 1,
            },
            assignee: Some(UserRef {
                id: "u1".to_string(),
                display_name: "Me".to_string(),
            }),
            parent_id: None,
            sprint_id: None,
            estimate_points: None,
            due_date: None,
            tags: Vec::new(),
            backlog_rank: String::new(),
            updated_at: 0,
        }
    }

    fn page(count: usize, next: Option<&str>) -> Page<IssueSummary> {
        Page {
            items: (1..=count)
                .map(|n| issue(&format!("EMB-{n}"), &format!("Issue {n}"), "High"))
                .collect(),
            next_cursor: next.map(str::to_string),
        }
    }

    fn canvas(id: &str, title: &str, updated_at: &str) -> CanvasView {
        CanvasView {
            id: id.to_string(),
            title: title.to_string(),
            home: "personal".to_string(),
            owner_id: Some("u1".to_string()),
            project_key: None,
            created_by: "u1".to_string(),
            seq: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            archived: false,
            my_access: "owner".to_string(),
        }
    }

    #[test]
    fn the_query_asks_for_my_open_issues_by_priority_then_update() {
        assert!(ASSIGNED_QUERY.contains("assignee: me"));
        assert!(ASSIGNED_QUERY.contains("state: #Unresolved"));
        assert!(ASSIGNED_QUERY.ends_with("sort by: priority desc, updated desc"));
        // The server's parser accepts it.
        assert!(space_proto::query::parse(ASSIGNED_QUERY).is_ok());
    }

    #[test]
    fn an_issue_row_has_its_key_summary_state_priority_project_and_route() {
        let assigned = assigned_from(page(1, None));
        assert_eq!(
            assigned.rows,
            vec![IssueLine {
                route: "/p/EMB/issues/EMB-1".to_string(),
                key: "EMB-1".to_string(),
                summary: "Issue 1".to_string(),
                state: "In Progress".to_string(),
                priority: "High".to_string(),
                project: "EMB".to_string(),
            }]
        );
        assert_eq!(assigned.count, "1");
    }

    #[test]
    fn the_project_is_the_key_without_its_number() {
        assert_eq!(project_of("EMB-12"), "EMB");
        assert_eq!(project_of("A-B-7"), "A-B");
        assert_eq!(project_of("NOKEY"), "NOKEY");
    }

    #[test]
    fn the_assigned_card_keeps_the_first_eight_in_the_servers_order() {
        let assigned = assigned_from(page(12, None));
        assert_eq!(assigned.rows.len(), ASSIGNED_ROWS);
        let keys: Vec<&str> = assigned.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys[0], "EMB-1");
        assert_eq!(keys[7], "EMB-8");
        assert_eq!(
            assigned.count, "12",
            "the count is the whole page, not the rows shown"
        );
    }

    #[test]
    fn the_count_says_more_when_the_server_has_another_page() {
        assert_eq!(count_text(0, false), "");
        assert_eq!(count_text(3, false), "3");
        assert_eq!(count_text(50, true), "50+");
        assert_eq!(assigned_from(page(50, Some("c2"))).count, "50+");
    }

    #[test]
    fn recent_canvases_are_the_five_newest_first() {
        let canvases = vec![
            canvas("c1", "One", "2026-11-01T10:00:00Z"),
            canvas("c2", "Two", "2026-11-07T10:00:00Z"),
            canvas("c3", "Three", "2026-11-03T10:00:00Z"),
            canvas("c4", "Four", "2026-11-06T10:00:00Z"),
            canvas("c5", "Five", "2026-11-05T10:00:00Z"),
            canvas("c6", "Six", "2026-11-04T10:00:00Z"),
            canvas("c7", "Seven", "2026-10-01T10:00:00Z"),
        ];
        let recent = recent_from(canvases);
        let ids: Vec<&str> = recent.rows.iter().map(|r| r.route.as_str()).collect();
        assert_eq!(
            ids,
            [
                "/home/canvases/c2",
                "/home/canvases/c4",
                "/home/canvases/c5",
                "/home/canvases/c6",
                "/home/canvases/c3",
            ]
        );
        assert_eq!(recent.rows[0].title, "Two");
        assert_eq!(recent.rows[0].updated, "2026-11-07 10:00");
    }

    #[test]
    fn a_card_starts_loading_and_shows_loaded_rows() {
        let mut card = Card::<Assigned>::default();
        assert_eq!(card.state(), LoadState::Loading);
        let generation = card.begin();
        assert!(card.finish(generation, Ok(assigned_from(page(2, None)))));
        assert_eq!(card.state(), LoadState::Loaded);
        assert_eq!(card.content().rows.len(), 2);
        assert_eq!(card.error(), "");
    }

    #[test]
    fn a_card_with_no_rows_is_empty() {
        let mut card = Card::<Recent>::default();
        let generation = card.begin();
        assert!(card.finish(generation, Ok(recent_from(Vec::new()))));
        assert_eq!(card.state(), LoadState::Empty);
        let mut card = Card::<Assigned>::default();
        let generation = card.begin();
        assert!(card.finish(generation, Ok(assigned_from(page(0, None)))));
        assert_eq!(card.state(), LoadState::Empty);
        assert_eq!(card.content().count, "");
    }

    #[test]
    fn a_failed_request_shows_its_error_and_no_rows() {
        let mut card = Card::<Assigned>::default();
        let first = card.begin();
        assert!(card.finish(first, Ok(assigned_from(page(2, None)))));
        let second = card.begin();
        assert_eq!(card.state(), LoadState::Loading);
        assert!(card.content().rows.is_empty(), "begin clears the old rows");
        assert!(card.finish(second, Err(ClientError::Network("refused".to_string()))));
        assert_eq!(card.state(), LoadState::Error);
        assert!(card.error().contains("could not be reached"));
        assert!(card.content().rows.is_empty());
    }

    #[test]
    fn a_retry_clears_the_error_and_a_late_answer_is_dropped() {
        let mut card = Card::<Recent>::default();
        let failed = card.begin();
        assert!(card.finish(failed, Err(ClientError::Unauthorized)));
        assert_eq!(card.state(), LoadState::Error);
        let retry = card.begin();
        assert_eq!(retry, failed + 1);
        assert_eq!(card.state(), LoadState::Loading);
        assert_eq!(card.error(), "");
        // The answer to the request before the retry changes nothing.
        assert!(!card.finish(
            failed,
            Ok(recent_from(vec![canvas(
                "c1",
                "One",
                "2026-11-01T10:00:00Z"
            )]))
        ));
        assert_eq!(card.state(), LoadState::Loading);
        assert!(card.finish(
            retry,
            Ok(recent_from(vec![canvas(
                "c1",
                "One",
                "2026-11-01T10:00:00Z"
            )]))
        ));
        assert_eq!(card.state(), LoadState::Loaded);
    }

    #[test]
    fn the_two_cards_do_not_share_a_generation_or_a_failure() {
        let mut shared = Shared::default();
        let assigned = shared.assigned.begin();
        let canvases = shared.canvases.begin();
        assert!(
            shared
                .assigned
                .finish(assigned, Err(ClientError::Network("down".to_string())))
        );
        assert!(shared.canvases.finish(
            canvases,
            Ok(recent_from(vec![canvas(
                "c1",
                "One",
                "2026-11-01T10:00:00Z"
            )]))
        ));
        assert_eq!(shared.assigned.state(), LoadState::Error);
        assert_eq!(shared.canvases.state(), LoadState::Loaded);
    }
}
