//! Company > Teams > one team (SP.3d): `/company/teams/{id}`. The team's header (name, department, lead) and its members. The lead
//! and each member open their profile (the window's navigation, in space_ui's app shell). The page data pattern
//! (`space_page_kit::page_data`). The page's markup is `ui/team.slint`.
//!
//! A team with no members shows an empty message inside the loaded page. A team the server does not know is an error.
//! This module is plain Rust. The window side is [`TeamWindow`], which `wire_company!` implements for the generated window.

use std::sync::{Arc, Mutex};

use slint::{Image, Weak};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource, Pictures};
use space_page_kit::person::initials;
use space_proto::people::{Department, OrgTreeEntry, TeamDetail};

use crate::org_tree::People;

/// The route prefix of a team's detail page.
const PREFIX: &str = "/company/teams/";

/// The team id of a team route (`/company/teams/{id}`), or `None` for any other route. The id is a plain UUID-like segment.
pub fn id_of(route: &str) -> Option<&str> {
    let id = route.strip_prefix(PREFIX)?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')).then_some(id)
}

/// What one load brings back: the team with its members, the departments, and the company's people.
type Fetched = (TeamDetail, Vec<Department>, Vec<OrgTreeEntry>);

/// The lists of the last successful load.
struct Loaded {
    detail: TeamDetail,
    departments: Vec<Department>,
    people: People,
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    /// The route on show.
    route: String,
    loaded: Option<Loaded>,
    /// The text of the last failed load. `None` while loading or loaded.
    error: Option<String>,
    pictures: Pictures,
}

/// The team's header as the window draws it (plain values).
#[derive(Clone, Default)]
pub struct TeamHeader {
    pub id: String,
    pub name: String,
    pub department: String,
    pub lead_id: String,
    pub lead_name: String,
    pub lead_initials: String,
    pub lead_avatar: Image,
    pub members: i32,
}

/// One member as the window draws it (plain values).
#[derive(Clone, Default)]
pub struct MemberRow {
    pub id: String,
    pub name: String,
    pub title: String,
    pub team: String,
    pub initials: String,
    pub avatar: Image,
}

/// The window side of the team page. `wire_company!` implements it for the generated window.
pub trait TeamWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_header(&self, header: TeamHeader);
    fn show_members(&self, members: Vec<MemberRow>);
    fn on_route_is_team(&self, f: impl Fn(&str) -> bool + 'static);
    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
}

/// The team page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct TeamPage<W: TeamWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: TeamWindow> Clone for TeamPage<W> {
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
pub fn wire<W: TeamWindow>(ui: &W, data: DataCell) -> TeamPage<W> {
    let page = TeamPage {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    ui.on_route_is_team(|route| id_of(route).is_some());

    let target = page.clone();
    ui.on_opened(move |route| target.open(route.as_str(), false));

    let target = page.clone();
    ui.on_retry(move || {
        let route = lock(&target.shared).route.clone();
        target.open(&route, true);
    });

    page
}

impl<W: TeamWindow> TeamPage<W> {
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

    /// Starts a load of the team at `route` (ignored when it is not a team route). A load for the route already on show is not
    /// repeated, unless `force` (Try again).
    fn open(&self, route: &str, force: bool) {
        let Some(id) = id_of(route).map(str::to_string) else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            if !force && s.route == route && s.error.is_none() {
                return;
            }
            s.generation += 1;
            s.route = route.to_string();
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
                let detail = api.team(&token, &id).await?;
                let departments = api.departments(&token).await?;
                let people = api.org_tree(&token).await?;
                Ok((detail, departments, people))
            },
            move |ui, result| deliver(ui, &shared, Some(&done_source), generation, result),
        );
    }
}

/// Keeps a load's result if it is for the latest load, then shows the team or the error. Starts the picture loads.
fn deliver<W: TeamWindow>(
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
        Ok((detail, departments, people)) => {
            s.error = None;
            s.loaded = Some(Loaded {
                detail,
                departments,
                people: People::new(people),
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

/// Loads one photo into the picture cache and shows it.
fn fetch_picture<W: TeamWindow>(
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

/// The window's state, from the request state. Returns the photos that are newly needed (now marked as pending).
fn show<W: TeamWindow>(ui: &W, s: &mut Shared) -> Vec<String> {
    let state = if s.loaded.is_some() {
        LoadState::Loaded
    } else if s.error.is_some() {
        LoadState::Error
    } else {
        LoadState::Loading
    };
    ui.show_state(state);
    ui.show_error(s.error.as_deref().unwrap_or_default());

    let mut wanted: Vec<String> = Vec::new();
    let (header, members) = match &s.loaded {
        Some(loaded) => {
            let team = &loaded.detail.team;
            let lead = team.lead_id.as_deref().and_then(|id| loaded.people.get(id));
            let department = loaded
                .departments
                .iter()
                .find(|d| d.id == team.department_id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let header = TeamHeader {
                id: team.id.clone(),
                name: team.name.clone(),
                department,
                lead_id: lead.map(|p| p.id.clone()).unwrap_or_default(),
                lead_name: lead.map(|p| p.name.clone()).unwrap_or_default(),
                lead_initials: lead.map(|p| initials(&p.name)).unwrap_or_default(),
                lead_avatar: picture_of(lead.and_then(|p| p.avatar_url.as_ref()), &s.pictures),
                members: i32::try_from(team.member_count).unwrap_or(i32::MAX),
            };
            let members: Vec<MemberRow> = loaded
                .detail
                .members
                .iter()
                .map(|person| MemberRow {
                    id: person.id.clone(),
                    name: person.display_name.clone(),
                    title: person.title.clone().unwrap_or_default(),
                    team: String::new(),
                    initials: initials(&person.display_name),
                    avatar: picture_of(person.avatar_url.as_ref(), &s.pictures),
                })
                .collect();
            wanted.extend(lead.and_then(|p| p.avatar_url.clone()));
            wanted.extend(
                loaded
                    .detail
                    .members
                    .iter()
                    .filter_map(|p| p.avatar_url.clone()),
            );
            (header, members)
        }
        None => (TeamHeader::default(), Vec::new()),
    };
    ui.show_header(header);
    ui.show_members(members);

    // Only photos that are neither loaded nor on their way are asked for, once each.
    wanted.retain(|url| !s.pictures.ready.contains_key(url) && !s.pictures.pending.contains(url));
    wanted.sort();
    wanted.dedup();
    s.pictures.pending.extend(wanted.iter().cloned());
    wanted
}

/// The decoded photo at `url`, or an empty image (the avatar then shows the initials).
fn picture_of(url: Option<&String>, pictures: &Pictures) -> Image {
    url.and_then(|url| pictures.ready.get(url))
        .map(page_data::picture)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_team_route_gives_its_id_and_other_routes_do_not() {
        assert_eq!(
            id_of("/company/teams/7e000000-0000-4000-8000-000000000001"),
            Some("7e000000-0000-4000-8000-000000000001")
        );
        assert_eq!(id_of("/company/teams"), None);
        assert_eq!(id_of("/company/teams/"), None);
        assert_eq!(id_of("/company/teams/../admin"), None);
        assert_eq!(id_of("/company/employees/abc"), None);
    }
}
