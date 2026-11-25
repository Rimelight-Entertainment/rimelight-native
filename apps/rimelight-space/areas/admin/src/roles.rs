//! Admin > Roles (SP.3e): the built-in roles admin and HR with their members, at `/admin/roles`, on the page data pattern
//! (`space_page_kit::page_data`). Each load asks `GET /v1/me/profile` first (the admin gate), then `GET /v1/admin/roles` and
//! the org chart's people for the picker. Add grants the role to the person picked for it, and Remove revokes it. The server
//! keeps at least one admin, and its 409 message shows in the action line. The page's markup is `ui/roles.slint`.
//!
//! Plain Rust. The window side is [`RolesWindow`], which `wire_admin!` implements for the generated window.

use std::sync::{Arc, Mutex};

use rimelight_ui_kit::toast::{self, Tone};
use slint::Weak;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::{initials, is_admin};
use space_page_kit::picker::{Choice, person_choices};
use space_proto::people::{OrgTreeEntry, PersonSummary, Profile, ROLE_ADMIN, ROLE_HR, RoleMembers};

use crate::AdminCompany;

/// The role keys the page knows, in the order the page shows them.
const ROLE_KEYS: [&str; 2] = [ROLE_ADMIN, ROLE_HR];

/// What a full load brings back: the roles with their members and the people for the picker, or `None` for a non-admin.
pub type Loaded = Option<(Vec<RoleMembers>, Vec<OrgTreeEntry>)>;

/// Whether a write grants a role or revokes it. The toast and the picker depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Grant,
    Revoke,
}

/// One member of a role as the window draws them (plain values).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonView {
    pub id: String,
    pub name: String,
    pub title: String,
    pub initials: String,
}

/// The window side of the roles page. `wire_admin!` implements it for the generated window.
pub trait RolesWindow: AdminCompany + slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_denied(&self, denied: bool);
    fn show_error(&self, text: &str);
    fn show_action_error(&self, text: &str);
    fn show_admins(&self, members: Vec<PersonView>);
    fn show_hr(&self, members: Vec<PersonView>);
    fn show_person_options(&self, options: Vec<Choice>);
    /// The person picked for a role, as the user has it (empty for none). `role` is `ROLE_ADMIN` or `ROLE_HR`.
    fn picked(&self, role: &str) -> String;
    fn show_picked(&self, role: &str, person: &str);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_add(&self, f: impl Fn(String) + 'static);
    fn on_remove(&self, f: impl Fn(String, String) + 'static);
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A result for an older number is dropped.
    generation: u64,
    /// Bumped by every write. A result for an older write is dropped.
    writes: u64,
    roles: Vec<RoleMembers>,
    people: Vec<OrgTreeEntry>,
    loading: bool,
    error: Option<String>,
    denied: bool,
}

/// What a request needs to reach the window and the session, cloned into each worker's callbacks.
struct Hub<W: RolesWindow> {
    ui: Weak<W>,
    shared: Arc<Mutex<Shared>>,
    source: Option<Arc<DataSource>>,
}

impl<W: RolesWindow> Clone for Hub<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.source.clone(),
        }
    }
}

/// The roles page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Page<W: RolesWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: RolesWindow> Clone for Page<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on the window. `data` is the slot the session driver fills; until it is set, requests do nothing
/// and the page stays in its loading state.
pub fn wire<W: RolesWindow>(ui: &W, data: DataCell) -> Page<W> {
    let page = Page {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    let target = page.clone();
    ui.on_opened(move || target.load());

    let target = page.clone();
    ui.on_retry(move || target.load());

    let target = page.clone();
    ui.on_add(move |role| target.add(&role));

    let target = page.clone();
    ui.on_remove(move |role, id| target.remove(&role, &id));

    page
}

impl<W: RolesWindow> Page<W> {
    fn hub(&self) -> Hub<W> {
        Hub {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.data.borrow().clone(),
        }
    }

    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// The number of the latest write.
    pub fn write_generation(&self) -> u64 {
        lock(&self.shared).writes
    }

    /// Delivers a load's result, as the worker would. For tests.
    pub fn deliver(&self, generation: u64, result: Result<Loaded, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_load(&ui, &self.hub(), generation, result);
        }
    }

    /// Delivers a grant's or a revoke's result, as the worker would. For tests.
    pub fn deliver_change(
        &self,
        write: u64,
        change: Change,
        role: &str,
        result: Result<Profile, ClientError>,
    ) {
        if let Some(ui) = self.ui.upgrade() {
            apply_change(&ui, &self.hub(), write, change, role, result);
        }
    }

    /// Loads the roles and the picker's people. Every load starts from the loading state.
    fn load(&self) {
        fetch_all(&self.hub(), false);
    }

    /// Grants `role` to the person picked for it.
    fn add(&self, role: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let picked = ui.picked(role);
        if picked.is_empty() {
            ui.show_action_error("Choose a person first.");
            return;
        }
        ui.show_action_error("");
        self.send(picked, role, Change::Grant);
    }

    /// Revokes `role` from a person.
    fn remove(&self, role: &str, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.show_action_error("");
        self.send(id.to_string(), role, Change::Revoke);
    }

    fn send(&self, id: String, role: &str, change: Change) {
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let role = role.to_string();
        let done = hub.clone();
        let grant = change == Change::Grant;
        let (work_id, work_role) = (id, role.clone());
        source.fetch(
            &hub.ui,
            move |api, token| async move {
                api.admin_set_role(&token, &work_id, &work_role, grant)
                    .await
            },
            move |ui, result| apply_change(ui, &done, write, change, &role, result),
        );
    }
}

/// Loads the roles and the people. `quiet` keeps the page on what it shows (a reload after a change).
fn fetch_all<W: RolesWindow>(hub: &Hub<W>, quiet: bool) {
    let generation = {
        let mut s = lock(&hub.shared);
        s.generation += 1;
        if !quiet {
            s.loading = true;
            s.error = None;
        }
        if let Some(ui) = hub.ui.upgrade() {
            show(&ui, &s);
        }
        s.generation
    };
    let Some(source) = hub.source.clone() else {
        return;
    };
    let done = hub.clone();
    source.fetch(
        &hub.ui,
        move |api, token| async move {
            let me = api.my_profile(&token).await?;
            if !is_admin(&me) {
                return Ok(None);
            }
            let roles = api.admin_roles(&token).await?;
            let people = api.org_tree(&token).await?;
            Ok(Some((roles, people)))
        },
        move |ui, result| apply_load(ui, &done, generation, result),
    );
}

/// Takes the number of a new write. Only the latest write's result is shown.
fn begin_write<W: RolesWindow>(hub: &Hub<W>) -> u64 {
    let mut s = lock(&hub.shared);
    s.writes += 1;
    s.writes
}

/// Keeps a load's result if it is the latest, then draws the page.
fn apply_load<W: RolesWindow>(
    ui: &W,
    hub: &Hub<W>,
    generation: u64,
    result: Result<Loaded, ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.generation != generation {
        return;
    }
    s.loading = false;
    match result {
        Ok(None) => {
            s.denied = true;
            s.roles.clear();
            s.people.clear();
            s.error = None;
        }
        Ok(Some((roles, people))) => {
            s.denied = false;
            s.error = None;
            s.roles = roles;
            s.people = people;
        }
        Err(error) => {
            s.error = Some(page_data::error_text(&error));
            s.roles.clear();
        }
    }
    show(ui, &s);
}

/// Puts a granted person into the role's members, or takes a revoked one out, so the member shows at once. The reload that
/// follows brings the same list back from the server.
fn put_member(roles: &mut [RoleMembers], role: &str, change: Change, profile: &Profile) {
    let Some(entry) = roles.iter_mut().find(|r| r.role == role) else {
        return;
    };
    match change {
        Change::Grant => {
            if !entry.members.iter().any(|m| m.id == profile.id) {
                entry.members.push(PersonSummary {
                    id: profile.id.clone(),
                    display_name: profile.display_name.clone(),
                    title: profile.title.clone(),
                    team: None,
                    avatar_url: None,
                });
            }
        }
        Change::Revoke => entry.members.retain(|m| m.id != profile.id),
    }
}

/// Keeps a change's result if it is the latest write. The member shows at once, a grant clears the picker for the role, a
/// toast says what happened, and the lists reload.
fn apply_change<W: RolesWindow>(
    ui: &W,
    hub: &Hub<W>,
    write: u64,
    change: Change,
    role: &str,
    result: Result<Profile, ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(profile) => {
            put_member(&mut s.roles, role, change, &profile);
            ui.show_action_error("");
            if change == Change::Grant {
                ui.show_picked(role, "");
            }
            toast::announce(
                Tone::Success,
                match change {
                    Change::Grant => format!("Role granted to {}", profile.display_name),
                    Change::Revoke => format!("Role removed from {}", profile.display_name),
                },
            );
            show(ui, &s);
            drop(s);
            fetch_all(hub, true);
            ui.refresh_company();
        }
        Err(error) => {
            ui.show_action_error(&page_data::error_text(&error));
        }
    }
}

/// Draws the page from the request state: the state line, both member lists and the picker's people.
fn show<W: RolesWindow>(ui: &W, s: &Shared) {
    ui.show_denied(s.denied);
    let page_state = if s.denied {
        LoadState::Loaded
    } else if s.loading {
        LoadState::Loading
    } else if s.error.is_some() {
        LoadState::Error
    } else {
        LoadState::Loaded
    };
    ui.show_state(page_state);
    ui.show_error(s.error.as_deref().unwrap_or_default());

    let members_of = |key: &str| -> Vec<PersonView> {
        s.roles
            .iter()
            .find(|r| r.role == key)
            .map(|r| r.members.iter().map(role_person).collect())
            .unwrap_or_default()
    };
    ui.show_admins(members_of(ROLE_KEYS[0]));
    ui.show_hr(members_of(ROLE_KEYS[1]));
    ui.show_person_options(person_choices(&s.people, false));
}

/// One member of a role, as the page shows them.
fn role_person(person: &PersonSummary) -> PersonView {
    PersonView {
        id: person.id.clone(),
        name: person.display_name.clone(),
        title: person.title.clone().unwrap_or_default(),
        initials: initials(&person.display_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_shows_the_name_title_and_initials() {
        let person = PersonSummary {
            id: "p1".into(),
            display_name: "Ana Lima".into(),
            title: None,
            team: None,
            avatar_url: None,
        };
        let row = role_person(&person);
        assert_eq!(row.name, "Ana Lima");
        assert_eq!(row.initials, "AL");
        assert_eq!(row.title, "");
    }
}
