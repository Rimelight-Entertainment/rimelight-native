//! Company > Departments (SP.3d): the department tree at `/company/departments`, on the page data pattern
//! (`space_page_kit::page_data`). Each row shows the department's head and member count. Departments nest by their parent, and
//! the first two levels start open. A row's arrow opens and closes it, and selecting a row shows the department's head and its
//! teams below. A team opens its detail page and the head opens their profile (both navigate in space_ui's app shell). The
//! page's markup is `ui/departments.slint`.
//!
//! This module is plain Rust. The window side is [`DeptWindow`], which `wire_company!` implements for the generated window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use slint::{Image, Weak};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource, Pictures};
use space_page_kit::person::initials;
use space_proto::people::{Department, OrgTreeEntry, Team};

use crate::org_tree::{People, Row, Tree};

/// The departments page's route.
pub const ROUTE: &str = "/company/departments";

/// What one load brings back: the departments, the teams, and the company's people (for the heads).
type Fetched = (Vec<Department>, Vec<Team>, Vec<OrgTreeEntry>);

/// The lists of the last successful load.
struct Loaded {
    departments: Vec<Department>,
    teams: Vec<Team>,
    people: People,
    tree: Tree,
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    loaded: Option<Loaded>,
    /// The text of the last failed load. `None` while loading or loaded.
    error: Option<String>,
    /// The departments whose rows are open.
    expanded: HashSet<String>,
    /// The selected department's id (empty for none).
    selected: String,
    pictures: Pictures,
}

/// A team listed under the selected department, as the window draws it (plain values).
#[derive(Clone, Default)]
pub struct DeptTeam {
    pub id: String,
    pub name: String,
    pub department: String,
    pub lead_id: String,
    pub lead_name: String,
    pub lead_initials: String,
    pub lead_avatar: Image,
    pub members: i32,
}

/// The selected department's detail as the window draws it (plain values).
#[derive(Clone, Default)]
pub struct DeptDetail {
    pub id: String,
    pub name: String,
    pub head_id: String,
    pub head_name: String,
    pub head_initials: String,
    pub head_avatar: Image,
    pub members: i32,
    pub teams: Vec<DeptTeam>,
    pub expandable: bool,
    pub expanded: bool,
}

/// The window side of the departments page. `wire_company!` implements it for the generated window.
pub trait DeptWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    /// The department table: its columns (title and width) and its rows (id, selected, cells), in order.
    fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>);
    fn show_detail(&self, detail: DeptDetail);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_toggle(&self, f: impl Fn(String) + 'static);
    fn on_select(&self, f: impl Fn(String) + 'static);
}

/// The departments page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Departments<W: DeptWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: DeptWindow> Clone for Departments<W> {
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
pub fn wire<W: DeptWindow>(ui: &W, data: DataCell) -> Departments<W> {
    let page = Departments {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    let target = page.clone();
    ui.on_opened(move || target.load());

    let target = page.clone();
    ui.on_retry(move || target.load());

    let target = page.clone();
    ui.on_toggle(move |id| target.toggle(id.as_str()));

    let target = page.clone();
    ui.on_select(move |id| target.select(id.as_str()));

    page
}

impl<W: DeptWindow> Departments<W> {
    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// The selected department's id (empty for none).
    pub fn selected(&self) -> String {
        lock(&self.shared).selected.clone()
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
                let departments = api.departments(&token).await?;
                let teams = api.teams(&token).await?;
                let people = api.org_tree(&token).await?;
                Ok((departments, teams, people))
            },
            move |ui, result| deliver(ui, &shared, Some(&done_source), generation, result),
        );
    }

    /// Opens or closes one department's row.
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

    /// Selects a department, which shows its head and teams. Selecting it again keeps it selected.
    fn select(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let wanted = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            s.selected = id.to_string();
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

/// Keeps a load's result if it is for the latest load, then shows the tree, the error or the empty message. Starts the picture
/// loads for the heads and team leads that are drawn. `source` is `None` in tests.
fn deliver<W: DeptWindow>(
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
        Ok((departments, teams, people)) => {
            let tree = Tree::new(
                departments
                    .iter()
                    .map(|d| (d.id.clone(), d.parent_department_id.clone())),
            );
            s.expanded = tree.initial_expanded();
            s.error = None;
            s.loaded = Some(Loaded {
                departments,
                teams,
                people: People::new(people),
                tree,
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
fn fetch_picture<W: DeptWindow>(
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

/// Draws the page from the request state and returns the photos that are newly needed (now marked as pending).
fn show<W: DeptWindow>(ui: &W, s: &mut Shared) -> Vec<String> {
    let state = match &s.loaded {
        Some(loaded) if loaded.departments.is_empty() => LoadState::Empty,
        Some(_) => LoadState::Loaded,
        None if s.error.is_some() => LoadState::Error,
        None => LoadState::Loading,
    };
    ui.show_state(state);
    ui.show_error(s.error.as_deref().unwrap_or_default());

    let mut wanted: Vec<String> = Vec::new();
    let (rows, detail) = match &s.loaded {
        Some(loaded) => {
            let listed = loaded.tree.rows(&s.expanded);
            let rows = listed
                .iter()
                .filter_map(|row| dept_row(row, loaded, &s.selected))
                .collect();
            let mut detail = department_detail(loaded, &s.selected, &s.pictures, &mut wanted);
            if let Some(row) = listed.iter().find(|row| row.id == s.selected) {
                detail.expandable = row.children > 0;
                detail.expanded = row.expanded;
            }
            (rows, detail)
        }
        None => (Vec::new(), DeptDetail::default()),
    };
    ui.show_table(
        &[("Department", 320.0), ("Head", 240.0), ("People", 120.0)],
        rows,
    );
    ui.show_detail(detail);

    // Only photos that are neither loaded nor on their way are asked for, once each.
    wanted.retain(|url| !s.pictures.ready.contains_key(url) && !s.pictures.pending.contains(url));
    wanted.sort();
    wanted.dedup();
    s.pictures.pending.extend(wanted.iter().cloned());
    wanted
}

/// One listed department as a table row: (id, selected, cells). The name is indented by its depth, with "+" for a closed
/// department that has children and "-" for an open one. None when the department is not in the list (cannot happen: the tree
/// is built from it).
fn dept_row(row: &Row, loaded: &Loaded, selected: &str) -> Option<(String, bool, Vec<String>)> {
    let department = loaded.departments.iter().find(|d| d.id == row.id)?;
    let head = department
        .head_id
        .as_deref()
        .and_then(|id| loaded.people.get(id));
    let marker = match (row.children > 0, row.expanded) {
        (true, true) => "- ",
        (true, false) => "+ ",
        (false, _) => "  ",
    };
    let name = format!("{}{}{}", "    ".repeat(row.depth), marker, department.name);
    let head = head.map_or_else(|| "No head set".to_string(), |p| p.name.clone());
    Some((
        department.id.clone(),
        department.id == selected,
        vec![name, head, department.member_count.to_string()],
    ))
}

/// The selected department's head and teams. Empty when nothing is selected. Records the photos it draws in `wanted`.
fn department_detail(
    loaded: &Loaded,
    selected: &str,
    pictures: &Pictures,
    wanted: &mut Vec<String>,
) -> DeptDetail {
    let Some(department) = loaded.departments.iter().find(|d| d.id == selected) else {
        return DeptDetail::default();
    };
    let head = department
        .head_id
        .as_deref()
        .and_then(|id| loaded.people.get(id));
    wanted.extend(head.and_then(|p| p.avatar_url.clone()));
    let teams: Vec<DeptTeam> = loaded
        .teams
        .iter()
        .filter(|team| team.department_id == department.id)
        .map(|team| {
            let lead = team.lead_id.as_deref().and_then(|id| loaded.people.get(id));
            wanted.extend(lead.and_then(|p| p.avatar_url.clone()));
            DeptTeam {
                id: team.id.clone(),
                name: team.name.clone(),
                department: department.name.clone(),
                lead_id: lead.map(|p| p.id.clone()).unwrap_or_default(),
                lead_name: lead.map(|p| p.name.clone()).unwrap_or_default(),
                lead_initials: lead.map(|p| initials(&p.name)).unwrap_or_default(),
                lead_avatar: picture_of(lead.and_then(|p| p.avatar_url.as_ref()), pictures),
                members: i32::try_from(team.member_count).unwrap_or(i32::MAX),
            }
        })
        .collect();
    DeptDetail {
        id: department.id.clone(),
        name: department.name.clone(),
        head_id: head.map(|p| p.id.clone()).unwrap_or_default(),
        head_name: head.map(|p| p.name.clone()).unwrap_or_default(),
        head_initials: head.map(|p| initials(&p.name)).unwrap_or_default(),
        head_avatar: picture_of(head.and_then(|p| p.avatar_url.as_ref()), pictures),
        members: i32::try_from(department.member_count).unwrap_or(i32::MAX),
        teams,
        expandable: false,
        expanded: false,
    }
}

/// The decoded photo at `url`, or an empty image (the avatar then shows the initials).
fn picture_of(url: Option<&String>, pictures: &Pictures) -> Image {
    url.and_then(|url| pictures.ready.get(url))
        .map(page_data::picture)
        .unwrap_or_default()
}
