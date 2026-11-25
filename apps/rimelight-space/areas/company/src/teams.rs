//! Company > Teams (SP.3d): the team list, and the org chart (the Tree view) from `GET /v1/org/tree`, on the page data pattern
//! (`space_page_kit::page_data`).
//!
//! One load fetches the teams, the departments (for the names) and the whole-company org chart (for the leads and the tree).
//! A team row opens `/company/teams/{id}`. An org chart row's name opens the person's profile. The chart starts with two levels
//! open. A node's arrow expands and collapses it, and "Find person" expands the path to the first match and highlights it.
//! The page's markup is `ui/teams.slint`; the views (List and Tree) are switched there.
//!
//! This module is plain Rust. The window side is [`TeamsWindow`], which `wire_company!` implements for the generated window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use slint::{Image, Weak};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource, Pictures};
use space_page_kit::person::initials;
use space_proto::people::{Department, OrgTreeEntry, SeatNode, Team};

use crate::org_tree::{Chart, People, Row};

/// The teams page's route.
pub const ROUTE: &str = "/company/teams";

/// The route of one team's detail page (`/company/teams/{id}`).
pub fn team_route(id: &str) -> String {
    format!("{ROUTE}/{id}")
}

/// What one load brings back: the teams, the departments and the company's people.
type Fetched = (Vec<Team>, Vec<Department>, Vec<OrgTreeEntry>, Vec<SeatNode>);

/// The lists of the last successful load.
struct Loaded {
    teams: Vec<Team>,
    departments: Vec<Department>,
    people: People,
    chart: Chart,
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    loaded: Option<Loaded>,
    /// The text of the last failed load. `None` while loading or loaded.
    error: Option<String>,
    /// The org chart nodes that are open.
    expanded: HashSet<String>,
    /// The person the find box highlighted (empty for none).
    highlighted: String,
    /// What the find box says about its last search ("Showing Ann Adams", "No one matches.").
    find_note: String,
    pictures: Pictures,
}

/// One org chart row as the window draws it (plain values; `wire_company!` converts it to the generated row).
#[derive(Clone, Default)]
pub struct OrgNode {
    pub id: String,
    pub name: String,
    pub title: String,
    pub initials: String,
    pub avatar: Image,
    pub placeholder: bool,
    pub person_id: String,
    pub depth: i32,
    pub reports: i32,
    pub expandable: bool,
    pub expanded: bool,
    pub highlighted: bool,
}

/// The window side of the teams page. `wire_company!` implements it for the generated window.
pub trait TeamsWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_find_note(&self, text: &str);
    /// The team table: its columns (title and width) and its rows (id, selected, cells), in order.
    fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>);
    fn show_org_rows(&self, rows: Vec<OrgNode>);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_toggle(&self, f: impl Fn(String) + 'static);
    fn on_find(&self, f: impl Fn(String) + 'static);
}

/// The teams page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Teams<W: TeamsWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: TeamsWindow> Clone for Teams<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, loads do nothing and the page
/// stays in its loading state.
pub fn wire<W: TeamsWindow>(ui: &W, data: DataCell) -> Teams<W> {
    let teams = Teams {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    let page = teams.clone();
    ui.on_opened(move || page.load());

    let page = teams.clone();
    ui.on_retry(move || page.load());

    let page = teams.clone();
    ui.on_toggle(move |id| page.toggle(id.as_str()));

    let page = teams.clone();
    ui.on_find(move |text| page.find(text.as_str()));

    teams
}

impl<W: TeamsWindow> Teams<W> {
    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// Delivers a load's result for the load with `generation`, as the worker would. For tests: no source is set, so nothing is
    /// fetched.
    pub fn deliver(&self, generation: u64, result: Result<Fetched, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver(&ui, &self.shared, None, generation, result);
    }

    /// Starts a load of the three lists. Each load starts from the loading state.
    fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            s.generation += 1;
            s.loaded = None;
            s.error = None;
            s.find_note.clear();
            s.highlighted.clear();
            show(&ui, s);
            s.generation
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let done_source = source.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let teams = api.teams(&token).await?;
                let departments = api.departments(&token).await?;
                let people = api.org_tree(&token).await?;
                let seats = api.org_seats(&token).await?;
                Ok((teams, departments, people, seats))
            },
            move |ui, result| deliver(ui, &shared, Some(&done_source), generation, result),
        );
    }

    /// Opens or closes one org chart node.
    fn toggle(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let wanted = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            if !s.expanded.remove(id) {
                s.expanded.insert(id.to_string());
            }
            show(&ui, s)
        };
        self.fetch_pictures(wanted);
    }

    /// The find box. The first person whose name matches is highlighted, and the path to them opens. An empty box clears it.
    fn find(&self, text: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let wanted = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            s.highlighted.clear();
            s.find_note.clear();
            if text.trim().is_empty() {
                s.find_note.clear();
            } else if let Some(loaded) = &s.loaded {
                match loaded.people.find(text) {
                    Some(person) => match loaded.chart.seat_for_person(&person.id) {
                        Some(seat) => {
                            s.highlighted = seat.to_string();
                            s.find_note = format!("Showing {}", person.name);
                            loaded.chart.tree.reveal(&mut s.expanded, seat);
                        }
                        None => s.find_note = format!("{} has no seat in the chart.", person.name),
                    },
                    None => s.find_note = "No one matches.".to_string(),
                }
            }
            show(&ui, s)
        };
        self.fetch_pictures(wanted);
    }

    /// Loads the pictures that were newly drawn. Nothing is fetched without a session.
    fn fetch_pictures(&self, wanted: Vec<String>) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        for url in wanted {
            fetch_picture(self.ui.clone(), &source, self.shared.clone(), url);
        }
    }
}

/// Keeps a load's result if it is for the latest load, then shows the lists, the error or the empty message. Starts the picture
/// loads for the rows that have a photo and no picture yet. `source` is `None` in tests.
fn deliver<W: TeamsWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    source: Option<&Arc<DataSource>>,
    generation: u64,
    result: Result<Fetched, ClientError>,
) {
    let mut guard = lock(shared);
    if guard.generation != generation {
        return;
    }
    let s = &mut *guard;
    match result {
        Ok((teams, departments, people, seats)) => {
            let chart = Chart::new(&seats);
            s.expanded = chart.tree.initial_expanded();
            s.error = None;
            s.loaded = Some(Loaded {
                teams,
                departments,
                people: People::new(people),
                chart,
            });
            let wanted = show(ui, s);
            drop(guard);
            if let Some(source) = source {
                for url in wanted {
                    fetch_picture(ui.as_weak(), source, shared.clone(), url);
                }
            }
        }
        Err(error) => {
            s.loaded = None;
            s.error = Some(page_data::error_text(&error));
            show(ui, s);
        }
    }
}

/// Loads one photo into the picture cache and draws it in the rows that carry it.
fn fetch_picture<W: TeamsWindow>(
    ui: Weak<W>,
    source: &Arc<DataSource>,
    shared: Arc<Mutex<Shared>>,
    url: String,
) {
    let key = url.clone();
    source.fetch(
        &ui,
        move |api, token| async move {
            let bytes = api.avatar(&token, &url).await?;
            page_data::decode_picture(&bytes)
        },
        move |ui, result| {
            let mut guard = lock(&shared);
            guard.pictures.pending.remove(&key);
            if let Ok(buffer) = result {
                guard.pictures.ready.insert(key, buffer);
                show(ui, &mut guard);
            }
        },
    );
}

/// Draws the page from the request state and returns the photos that are newly needed (now marked as pending). The team rows
/// need the leads' photos; the org chart needs the photos of the rows it lists.
fn show<W: TeamsWindow>(ui: &W, s: &mut Shared) -> Vec<String> {
    let state = match &s.loaded {
        Some(loaded) if loaded.teams.is_empty() && loaded.people.all().is_empty() => {
            LoadState::Empty
        }
        Some(_) => LoadState::Loaded,
        None if s.error.is_some() => LoadState::Error,
        None => LoadState::Loading,
    };
    ui.show_state(state);
    ui.show_error(s.error.as_deref().unwrap_or_default());
    ui.show_find_note(&s.find_note);

    let mut wanted: Vec<String> = Vec::new();
    let (team_rows, org_rows) = match &s.loaded {
        Some(loaded) => {
            let team_rows = loaded
                .teams
                .iter()
                .map(|team| team_cells(team, loaded))
                .collect();
            let org_rows: Vec<OrgNode> = loaded
                .chart
                .tree
                .rows(&s.expanded)
                .iter()
                .filter_map(|row| org_row(row, loaded, &s.highlighted, &s.pictures))
                .collect();
            wanted.extend(org_rows.iter().filter_map(|row| row_url(row, loaded)));
            (team_rows, org_rows)
        }
        None => (Vec::new(), Vec::new()),
    };
    ui.show_table(
        &[
            ("Team", 280.0),
            ("Department", 240.0),
            ("Lead", 240.0),
            ("Members", 120.0),
        ],
        team_rows,
    );
    ui.show_org_rows(org_rows);

    // Only photos that are neither loaded nor on their way are asked for, once each.
    wanted.retain(|url| !s.pictures.ready.contains_key(url) && !s.pictures.pending.contains(url));
    wanted.sort();
    wanted.dedup();
    s.pictures.pending.extend(wanted.iter().cloned());
    wanted
}

/// The photo URL of a listed org row, if the seat is filled by a person who has one.
fn row_url(row: &OrgNode, loaded: &Loaded) -> Option<String> {
    let seat = loaded.chart.seat(row.id.as_str())?;
    loaded
        .people
        .get(seat.person_id.as_deref()?)
        .and_then(|person| person.avatar_url.clone())
}

/// One team as a table row: (id, selected, cells). The lead is "No lead yet" until one is set.
fn team_cells(team: &Team, loaded: &Loaded) -> (String, bool, Vec<String>) {
    let lead = team
        .lead_id
        .as_deref()
        .and_then(|id| loaded.people.get(id))
        .map_or_else(|| "No lead yet".to_string(), |p| p.name.clone());
    let department = loaded
        .departments
        .iter()
        .find(|d| d.id == team.department_id)
        .map(|d| d.name.clone())
        .unwrap_or_default();
    (
        team.id.clone(),
        false,
        vec![
            team.name.clone(),
            department,
            lead,
            team.member_count.to_string(),
        ],
    )
}

/// One listed org chart row: a filled seat shows its person; an open or planned seat shows as a placeholder.
/// None when the row's seat is not in the chart (cannot happen: the tree is built from the chart).
fn org_row(row: &Row, loaded: &Loaded, highlighted: &str, pictures: &Pictures) -> Option<OrgNode> {
    let seat = loaded.chart.seat(&row.id)?;
    let person = seat
        .person_id
        .as_deref()
        .and_then(|id| loaded.people.get(id));
    let (name, initials_text, avatar, placeholder) = match (person, seat.vacant) {
        (Some(person), false) => (
            person.name.clone(),
            initials(&person.name),
            picture_of(person.avatar_url.as_ref(), pictures),
            false,
        ),
        _ => (
            if seat.planned {
                "Planned seat".to_string()
            } else {
                "Open seat".to_string()
            },
            String::new(),
            Image::default(),
            true,
        ),
    };
    Some(OrgNode {
        id: seat.id.clone(),
        name,
        title: seat.title.clone(),
        initials: initials_text,
        avatar,
        placeholder,
        person_id: seat.person_id.clone().unwrap_or_default(),
        depth: i32::try_from(row.depth).unwrap_or(i32::MAX),
        reports: i32::try_from(row.children).unwrap_or(i32::MAX),
        expandable: row.children > 0,
        expanded: row.expanded,
        highlighted: row.id == highlighted,
    })
}

/// The decoded photo at `url`, or an empty image (the avatar then shows the initials).
fn picture_of(url: Option<&String>, pictures: &Pictures) -> Image {
    url.and_then(|url| pictures.ready.get(url))
        .map(page_data::picture)
        .unwrap_or_default()
}
