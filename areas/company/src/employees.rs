//! Company > Employees (SP.3b): the people directory from `GET /v1/people`, on the page data pattern (`space_page_kit::page_data`).
//!
//! The people are a table (name, title, team). The first page loads when the page appears (its `opened` callback). Typing
//! starts a new query 300 ms after the last change. "Load more" appends the next page by cursor. A pick opens
//! `/company/employees/{id}` (the window's navigation, in space_ui's app shell). The page's markup is `ui/employees.slint`.
//!
//! This module is plain Rust. The window side is [`EmployeesWindow`], which `wire_company!` implements for the generated
//! window.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode, Weak};
use space_client::error::ClientError;
use space_client::people::DirectoryQuery;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell};
use space_proto::people::{Page, PersonSummary};

/// The people page's route.
pub const ROUTE: &str = "/company/employees";
/// The people per page the server is asked for (the server's default is 50, its maximum 100).
pub const PAGE_SIZE: u32 = 50;
/// How long the search waits after the last change before the server is asked.
pub const SEARCH_DELAY: Duration = Duration::from_millis(300);

/// The route of one person's profile.
pub fn person_route(id: &str) -> String {
    format!("{ROUTE}/{id}")
}

/// The window side of the employees page: what the page draws and the callbacks it listens to. `wire_company!` implements it
/// for the generated window.
pub trait EmployeesWindow: ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_loading_more(&self, on: bool);
    fn show_has_more(&self, more: bool);
    /// The table: its columns (title and width) and its rows (id, selected, cells), in order.
    fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_search(&self, f: impl Fn(String) + 'static);
    fn on_load_more(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every request. A result for an older number is dropped.
    generation: u64,
    /// The search text of the current list.
    query: String,
    /// The `next_cursor` of the last page, or `None` when the list is complete.
    cursor: Option<String>,
    rows: Vec<PersonSummary>,
    has_more: bool,
    /// True while the current request appends a page ("Load more") rather than replacing the list.
    appending: bool,
}

/// The employees page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Employees<W: EmployeesWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: EmployeesWindow> Clone for Employees<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, requests do nothing and the
/// page stays in its loading state.
pub fn wire<W: EmployeesWindow>(ui: &W, data: DataCell) -> Employees<W> {
    let employees = Employees {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    let page = employees.clone();
    ui.on_opened(move || page.start(Some(String::new())));

    // Each change restarts the timer, and a timer asks only when its text is still the latest one, so only the last text is
    // searched, once.
    let page = employees.clone();
    let debounce = Rc::new(Timer::default());
    let latest = Rc::new(RefCell::new(String::new()));
    ui.on_search(move |text| {
        let query = text;
        *latest.borrow_mut() = query.clone();
        let (page, latest) = (page.clone(), latest.clone());
        debounce.start(TimerMode::SingleShot, SEARCH_DELAY, move || {
            if *latest.borrow() == query {
                page.start(Some(query.clone()));
            }
        });
    });

    let page = employees.clone();
    ui.on_load_more(move || page.start(None));

    let page = employees.clone();
    ui.on_retry(move || {
        let query = lock(&page.shared).query.clone();
        page.start(Some(query));
    });

    employees
}

impl<W: EmployeesWindow> Employees<W> {
    /// The search text of the current list.
    pub fn query(&self) -> String {
        lock(&self.shared).query.clone()
    }

    /// The generation of the latest request.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// Delivers a result for the request with `generation`, as the worker would. A result for an older request is dropped.
    /// For tests: no source is set, so nothing is fetched.
    pub fn deliver(&self, generation: u64, result: Result<Page<PersonSummary>, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver(&ui, &self.shared, generation, result);
    }

    /// Starts a request: a new list for `Some(query)`, or the next page for `None`. Does nothing for the next page when the
    /// list is complete.
    fn start(&self, query: Option<String>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let appending = query.is_none();
        let (request, generation) = {
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
            let request = DirectoryQuery {
                q: Some(s.query.clone()),
                cursor: s.cursor.clone(),
                limit: Some(PAGE_SIZE),
                ..DirectoryQuery::default()
            };
            (request, s.generation)
        };
        if appending {
            ui.show_loading_more(true);
        } else {
            ui.show_state(LoadState::Loading);
            ui.show_error("");
            show_rows(&ui, &lock(&self.shared));
        }

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.directory(&token, &request).await },
            move |ui, result| deliver(ui, &shared, generation, result),
        );
    }
}

/// Keeps a result if it is for the latest request, then shows the list, the error or the empty message.
fn deliver<W: EmployeesWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    result: Result<Page<PersonSummary>, ClientError>,
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
            s.has_more = s.cursor.is_some();
            ui.show_has_more(s.has_more);
            ui.show_error("");
            let state = if s.rows.is_empty() {
                LoadState::Empty
            } else {
                LoadState::Loaded
            };
            ui.show_state(state);
            show_rows(ui, &s);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// Draws the people as the table's rows, in the order of the list.
fn show_rows<W: EmployeesWindow>(ui: &W, s: &Shared) {
    let rows = s
        .rows
        .iter()
        .map(|person| {
            (
                person.id.clone(),
                false,
                vec![
                    person.display_name.clone(),
                    person.title.clone().unwrap_or_default(),
                    person.team.clone().unwrap_or_default(),
                ],
            )
        })
        .collect();
    ui.show_table(&[("Name", 320.0), ("Title", 260.0), ("Team", 240.0)], rows);
}
