//! Project > Members (`/p/:key/members`): the project's team, on the page data pattern (`space_page_kit::page_data`). The project
//! is the one on the route (`project_of_route`). A load asks `GET /v1/me/profile`, the project, its members and the
//! organisation's people, in one worker. The caller may change the team when they are a project admin or an admin of the
//! organisation. Add, a role change and Remove each send one request, then reload the team.
//!
//! Plain Rust. The window side is [`MembersWindow`], which `wire_management!` implements for the generated window.

use std::sync::{Arc, Mutex};

use slint::Weak;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::{initials, is_admin};
use space_page_kit::picker::{Choice, choice};
use space_page_kit::projects::project_of_route;
use space_proto::people::OrgTreeEntry;
use space_proto::projects::{Member, PROJECT_ROLES};

/// Shown when a change is asked for on a route that is not a project's.
const NOT_A_PROJECT: &str = "Open a project to change its team.";

/// The page's request state. Behind a `Mutex`, because a worker's result reads it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A result for an older number is dropped.
    generation: u64,
}

/// What a load brings back: the caller's rights, the team and the organisation's people for the picker.
struct Loaded {
    can_edit: bool,
    my_id: String,
    members: Vec<Member>,
    people: Vec<OrgTreeEntry>,
}

/// One member of the project as the window draws them (plain values).
#[derive(Debug, Clone, Default)]
pub struct MemberView {
    pub id: String,
    pub name: String,
    pub initials: String,
    pub role: String,
    pub role_label: String,
    pub is_me: bool,
}

/// The window side of the members page. `wire_management!` implements it for the generated window.
pub trait MembersWindow: slint::ComponentHandle + 'static {
    /// The route the window is on.
    fn route(&self) -> String;
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_action_error(&self, text: &str);
    fn show_count(&self, count: i32);
    fn show_members(&self, members: Vec<MemberView>);
    fn show_person_options(&self, options: Vec<Choice>);
    fn show_role_options(&self, options: Vec<Choice>);
    fn show_can_edit(&self, on: bool);
    /// The picker's role, as the user has it (empty for none).
    fn pick_role(&self) -> String;
    fn show_pick_role(&self, role: &str);
    /// The picker's person, as the user has it (empty for none).
    fn pick_person(&self) -> String;
    fn show_pick_person(&self, person: &str);
    fn show_add_open(&self, open: bool);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_open_add(&self, f: impl Fn() + 'static);
    fn on_close_add(&self, f: impl Fn() + 'static);
    fn on_add(&self, f: impl Fn() + 'static);
    fn on_set_role(&self, f: impl Fn(String, String) + 'static);
    fn on_remove(&self, f: impl Fn(String) + 'static);
}

/// What a request needs to reach the window and the session, cloned into each worker's callbacks.
struct Hub<W: MembersWindow> {
    ui: Weak<W>,
    shared: Arc<Mutex<Shared>>,
    source: Arc<DataSource>,
}

impl<W: MembersWindow> Clone for Hub<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.source.clone(),
        }
    }
}

/// The members page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Page<W: MembersWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: MembersWindow> Clone for Page<W> {
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
pub fn wire<W: MembersWindow>(ui: &W, data: DataCell) -> Page<W> {
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
    ui.on_open_add(move || target.set_add_open(true));

    let target = page.clone();
    ui.on_close_add(move || target.set_add_open(false));

    let target = page.clone();
    ui.on_add(move || target.add());

    let target = page.clone();
    ui.on_set_role(move |id, role| target.set_role(&id, &role));

    let target = page.clone();
    ui.on_remove(move |id| target.remove(&id));

    page
}

impl<W: MembersWindow> Page<W> {
    fn hub(&self) -> Option<Hub<W>> {
        let source = self.data.borrow().clone()?;
        Some(Hub {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source,
        })
    }

    /// The project key of the route the window is on, or `None` off a project's route.
    fn route_key(&self) -> Option<String> {
        let ui = self.ui.upgrade()?;
        project_of_route(&ui.route()).map(str::to_owned)
    }

    /// Loads the team from the first screen of the page (and on retry). Every load starts from the loading state.
    fn load(&self) {
        let Some(hub) = self.hub() else { return };
        match self.route_key() {
            Some(key) => fetch_members(&hub, key, false),
            None => show_error(&self.ui, "Open a project to see its team."),
        }
    }

    /// Adds the person picked in the picker, with the picked role (`member` when none is picked).
    fn add(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let person = ui.pick_person();
        let role = ui.pick_role();
        if person.is_empty() {
            ui.show_action_error("Choose a person first.");
            return;
        }
        ui.show_action_error("");
        let role = if role.is_empty() {
            "member".to_owned()
        } else {
            role
        };
        self.write_member(person, role, true);
    }

    /// Opens or closes the Add people dialog (the page shows it while `add-open` is set).
    fn set_add_open(&self, open: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.show_add_open(open);
        }
    }

    /// Changes a member's role.
    fn set_role(&self, user_id: &str, role: &str) {
        self.write_member(user_id.to_owned(), role.to_owned(), false);
    }

    /// Takes a member off the project.
    fn remove(&self, user_id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(key) = self.route_key() else {
            action_error(&ui, NOT_A_PROJECT);
            return;
        };
        let Some(hub) = self.hub() else { return };
        ui.show_action_error("");
        let work_key = key.clone();
        let work_id = user_id.to_owned();
        let done = hub.clone();
        hub.source.fetch(
            &hub.ui,
            move |api, token| async move { api.remove_member(&token, &work_key, &work_id).await },
            move |ui, result: Result<(), ClientError>| match result {
                Ok(()) => {
                    ui.show_action_error("");
                    fetch_members(&done, key, true);
                }
                Err(error) => action_error(ui, &page_data::error_text(&error)),
            },
        );
    }

    /// Sets a person's role on the project (adds them when they are not on it), then reloads the team. `adding` clears the
    /// picker once the person is on the team.
    fn write_member(&self, user_id: String, role: String, adding: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(key) = self.route_key() else {
            action_error(&ui, NOT_A_PROJECT);
            return;
        };
        let Some(hub) = self.hub() else { return };
        let work_key = key.clone();
        let done = hub.clone();
        hub.source.fetch(
            &hub.ui,
            move |api, token| async move {
                api.set_member(&token, &work_key, &user_id, &role).await
            },
            move |ui, result| match result {
                Ok(_) => {
                    ui.show_action_error("");
                    if adding {
                        ui.show_pick_person("");
                        ui.show_add_open(false);
                    }
                    fetch_members(&done, key, true);
                }
                Err(error) => action_error(ui, &page_data::error_text(&error)),
            },
        );
    }
}

/// Loads the team, the caller's rights and the organisation's people. `quiet` keeps the page on what it shows (a reload after a
/// change).
fn fetch_members<W: MembersWindow>(hub: &Hub<W>, key: String, quiet: bool) {
    let generation = {
        let mut s = lock(&hub.shared);
        s.generation += 1;
        s.generation
    };
    if !quiet && let Some(ui) = hub.ui.upgrade() {
        ui.show_state(LoadState::Loading);
        ui.show_error("");
    }
    let done = hub.clone();
    hub.source.fetch(
        &hub.ui,
        move |api, token| async move {
            let me = api.my_profile(&token).await?;
            let project = api.project(&token, &key).await?;
            let members = api.project_members(&token, &key).await?;
            let people = api.org_tree(&token).await?;
            Ok(Loaded {
                can_edit: project.my_role.as_deref() == Some("admin") || is_admin(&me),
                my_id: me.profile.id.clone(),
                members,
                people,
            })
        },
        move |ui, result| deliver(ui, &done.shared, generation, result),
    );
}

/// Draws the team when the load is the latest one. A failed load shows its message in place of the list.
fn deliver<W: MembersWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Loaded, ClientError>,
) {
    if lock(shared).generation != generation {
        return;
    }
    match result {
        Ok(loaded) => {
            let rows: Vec<MemberView> = loaded
                .members
                .iter()
                .map(|member| member_row(member, &loaded.my_id))
                .collect();
            let taken: Vec<&str> = loaded
                .members
                .iter()
                .map(|member| member.user.id.as_str())
                .collect();
            let options: Vec<Choice> = loaded
                .people
                .iter()
                .filter(|person| !taken.contains(&person.id.as_str()))
                .map(|person| choice(&person.id, &person.name))
                .collect();
            let count = i32::try_from(rows.len()).unwrap_or(i32::MAX);
            ui.show_count(count);
            ui.show_members(rows);
            ui.show_person_options(options);
            ui.show_role_options(role_options());
            if ui.pick_role().is_empty() {
                ui.show_pick_role("member");
            }
            ui.show_can_edit(loaded.can_edit);
            ui.show_error("");
            ui.show_state(LoadState::Loaded);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// One member of the project, as the page shows them.
fn member_row(member: &Member, my_id: &str) -> MemberView {
    MemberView {
        id: member.user.id.clone(),
        name: member.user.display_name.clone(),
        initials: initials(&member.user.display_name),
        role: member.role.clone(),
        role_label: capitalised(&member.role),
        is_me: member.user.id == my_id,
    }
}

/// The project roles for the role Select, labelled capitalised.
fn role_options() -> Vec<Choice> {
    PROJECT_ROLES
        .iter()
        .map(|role| choice(role, &capitalised(role)))
        .collect()
}

/// `"member"` becomes `"Member"`.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Shows a load's error in place of the team.
fn show_error<W: MembersWindow>(ui: &Weak<W>, message: &str) {
    if let Some(ui) = ui.upgrade() {
        ui.show_error(message);
        ui.show_state(LoadState::Error);
    }
}

/// Shows a change's error in the page's banner.
fn action_error<W: MembersWindow>(ui: &W, message: &str) {
    ui.show_action_error(message);
}
