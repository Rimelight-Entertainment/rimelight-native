//! Admin > Users (SP.3e): the people list at `/admin/users`, on the page data pattern (`space_page_kit::page_data`).
//!
//! Each load asks `GET /v1/me/profile` first. Without the admin role the page shows "You need the admin role" and asks for
//! nothing else. Otherwise it loads the first page of `GET /v1/admin/users` (deactivated people included) and the reference
//! lists the pickers use. Search replaces the list; "Load more" appends by cursor. Selecting a row fills the detail panel, and
//! saves, activation and role changes update that row in place. Adding a person reloads the list. Server errors (409, 422 and
//! the rest) are shown inline in the form that caused them. The page's markup is `ui/users.slint`.
//!
//! Every write takes a write number, and a result for an older write is dropped. The list and the load take the generation
//! number, as the Company pages do.
//!
//! Plain Rust. The window side is [`UsersWindow`], which `wire_admin!` implements for the generated window.

use std::sync::{Arc, Mutex};

use rimelight_ui_kit::toast::{self, Tone};
use slint::Weak;
use space_client::error::ClientError;
use space_client::people::DirectoryQuery;
use space_page_kit::calendar::{self, Date};
use space_page_kit::dates::{self, CalendarCell, iso, parse_iso, today};
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::initials;
use space_page_kit::picker::{
    Choice, choice, department_choices, person_choices, position_choices, team_choices, valid_date,
};
use space_proto::people::{
    AdminCreateUser, AdminUpdateUser, Page as ProtoPage, PersonProfile, Profile, ROLE_ADMIN,
    ROLE_HR, SEAT_CLOSED, Seat, SeatInput,
};

use crate::{AdminCompany, Reference, fetch_reference};

/// Whether the caller may open the users page: admin or HR (the salary and role rules are checked by the server).
fn can_manage_users(me: &PersonProfile) -> bool {
    me.profile
        .roles
        .iter()
        .any(|role| role == ROLE_ADMIN || role == ROLE_HR)
}

/// A seat change for a save: the seat the person held (to vacate) and the seat they now take, each `None` when absent.
type SeatChange = Option<(Option<String>, Option<String>)>;

/// Which start date picker a callback is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Add,
    Detail,
}

/// People per page (the server's default; its maximum is 100).
pub const PAGE_SIZE: u32 = 50;

/// What a full load brings back: the first page and the reference lists, or `None` when the caller is not an admin.
pub type Loaded = Option<(ProtoPage<Profile>, Reference)>;

/// Which write a result is for. The toast and the place an error shows depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// `POST /v1/admin/users`: a new person. Errors show in the add form.
    Add,
    /// `PATCH /v1/admin/users/{id}` with the org fields. Errors show in the detail panel.
    Edit,
    /// `PATCH` with `is_active`. Errors show in the detail panel.
    Active,
    /// `PUT` or `DELETE` on a role. Errors show in the detail panel.
    Role,
}

/// The add form's fields as the user has them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddForm {
    pub email: String,
    pub name: String,
    pub start: String,
    pub position: String,
    pub department: String,
    pub team: String,
    pub manager: String,
}

/// The detail panel's editable fields as the user has them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetailForm {
    pub start: String,
    pub position: String,
    pub department: String,
    pub team: String,
    pub manager: String,
    pub seat: String,
}

/// One row of the list as the window draws it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserRowView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub title: String,
    pub department: String,
    pub team: String,
    pub status: String,
    pub active: bool,
    pub roles: String,
    pub initials: String,
    pub selected: bool,
}

/// The detail panel of the selected person.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetailView {
    pub name: String,
    pub email: String,
    pub status: String,
    pub roles: String,
    pub active: bool,
    pub admin: bool,
    pub hr: bool,
    pub position: String,
    pub department: String,
    pub team: String,
    pub manager: String,
    pub start: String,
    pub seat: String,
}

/// The choices of the pickers on the page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub position: Vec<Choice>,
    pub seat: Vec<Choice>,
    pub department: Vec<Choice>,
    pub team: Vec<Choice>,
    pub person: Vec<Choice>,
}

/// The window side of the users page. `wire_admin!` implements it for the generated window.
pub trait UsersWindow: AdminCompany + slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_denied(&self, denied: bool);
    fn show_error(&self, text: &str);
    fn show_loading_more(&self, on: bool);
    fn show_has_more(&self, on: bool);
    fn show_weekdays(&self, labels: Vec<String>);
    fn show_calendar(&self, field: Field, cells: Vec<CalendarCell>, title: String);
    fn show_rows(&self, rows: Vec<UserRowView>);
    fn show_options(&self, options: Options);
    fn show_detail(&self, detail: DetailView);
    fn show_detail_error(&self, text: &str);
    fn show_add_error(&self, text: &str);
    fn show_add_open(&self, on: bool);
    /// Closes the add form and clears its fields.
    fn clear_add_form(&self);
    /// The add form's fields, as the user has them.
    fn add_form(&self) -> AddForm;
    /// The detail panel's editable fields, as the user has them.
    fn detail_form(&self) -> DetailForm;
    /// The text of a start date picker's field.
    fn start_text(&self, field: Field) -> String;
    fn show_start(&self, field: Field, text: &str);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_search(&self, f: impl Fn(String) + 'static);
    fn on_load_more(&self, f: impl Fn() + 'static);
    fn on_select(&self, f: impl Fn(String) + 'static);
    fn on_add_submit(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn() + 'static);
    fn on_set_active(&self, f: impl Fn(bool) + 'static);
    fn on_set_role(&self, f: impl Fn(String, bool) + 'static);
    fn on_add_pick(&self, f: impl Fn(i32) + 'static);
    fn on_add_month_step(&self, f: impl Fn(i32) + 'static);
    fn on_d_pick(&self, f: impl Fn(i32) + 'static);
    fn on_d_month_step(&self, f: impl Fn(i32) + 'static);
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every list request and load. A result for an older number is dropped.
    generation: u64,
    /// Bumped by every write. A result for an older write is dropped.
    writes: u64,
    /// The search text of the list.
    query: String,
    /// The `next_cursor` of the last page of the list.
    cursor: Option<String>,
    has_more: bool,
    rows: Vec<Profile>,
    reference: Reference,
    /// The person whose detail panel is open.
    selected: Option<Profile>,
    /// True while a fresh list or a load is on its way (the page shows loading).
    loading: bool,
    /// True while "Load more" is on its way.
    loading_more: bool,
    /// The text of the last failed list request or load. `None` when there is none.
    error: Option<String>,
    /// The caller is not an admin.
    denied: bool,
    /// The month the add form's start date picker shows, or `None` for the chosen date's month (else this month).
    add_shown: Option<(i32, u32)>,
    /// The same for the detail panel's start date picker.
    detail_shown: Option<(i32, u32)>,
}

/// What a request needs to reach the window and the session. Cloned into each worker's callbacks, so it holds only
/// `Send` values.
struct Hub<W: UsersWindow> {
    ui: Weak<W>,
    shared: Arc<Mutex<Shared>>,
    source: Option<Arc<DataSource>>,
}

impl<W: UsersWindow> Clone for Hub<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.source.clone(),
        }
    }
}

/// The users page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Page<W: UsersWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: UsersWindow> Clone for Page<W> {
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
pub fn wire<W: UsersWindow>(ui: &W, data: DataCell) -> Page<W> {
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
    ui.on_search(move |text| target.search(&text));

    let target = page.clone();
    ui.on_load_more(move || target.more());

    let target = page.clone();
    ui.on_select(move |id| target.select(&id));

    let target = page.clone();
    ui.on_add_submit(move || target.add());

    let target = page.clone();
    ui.on_save(move || target.save());

    let target = page.clone();
    ui.on_set_active(move |active| target.set_active(active));

    let target = page.clone();
    ui.on_set_role(move |role, grant| target.set_role(&role, grant));

    let target = page.clone();
    ui.on_add_pick(move |index| target.pick(Field::Add, index));

    let target = page.clone();
    ui.on_add_month_step(move |step| target.step_month(Field::Add, step));

    let target = page.clone();
    ui.on_d_pick(move |index| target.pick(Field::Detail, index));

    let target = page.clone();
    ui.on_d_month_step(move |step| target.step_month(Field::Detail, step));

    page
}

impl<W: UsersWindow> Page<W> {
    fn hub(&self) -> Hub<W> {
        Hub {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.data.borrow().clone(),
        }
    }

    /// The generation of the latest list request or load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// The number of the latest write. A write's result is delivered with this number.
    pub fn write_generation(&self) -> u64 {
        lock(&self.shared).writes
    }

    /// Delivers a full load's result, as the worker would. For tests: no source is set, so nothing is fetched.
    pub fn deliver(&self, generation: u64, result: Result<Loaded, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_load(&ui, &self.hub(), generation, result);
        }
    }

    /// Delivers a list request's result (a search or a "Load more"). For tests.
    pub fn deliver_list(
        &self,
        generation: u64,
        appending: bool,
        result: Result<ProtoPage<Profile>, ClientError>,
    ) {
        if let Some(ui) = self.ui.upgrade() {
            apply_list(&ui, &self.hub(), generation, appending, result);
        }
    }

    /// Delivers a write's result, as the worker would. For tests.
    pub fn deliver_write(&self, write: u64, kind: Written, result: Result<Profile, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_write(&ui, &self.hub(), write, kind, result);
        }
    }

    /// Loads the first page, the reference lists and the caller's role. Every load starts from the loading state.
    fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            s.generation += 1;
            s.loading = true;
            s.error = None;
            s.rows.clear();
            s.has_more = false;
            show(&ui, &s);
            s.generation
        };
        let hub = self.hub();
        let Some(source) = hub.source.clone() else {
            return;
        };
        let query = lock(&self.shared).query.clone();
        let request = list_request(&query, None);
        let ui_weak = hub.ui.clone();
        let done = hub.clone();
        source.fetch(
            &ui_weak,
            move |api, token| async move {
                let me = api.my_profile(&token).await?;
                if !can_manage_users(&me) {
                    return Ok(None);
                }
                let page = api.admin_users(&token, &request).await?;
                let reference = fetch_reference(&api, &token).await?;
                Ok(Some((page, reference)))
            },
            move |ui, result| apply_load(ui, &done, generation, result),
        );
    }

    /// Starts a new search: the list is replaced by the people matching `text`.
    fn search(&self, text: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        {
            let mut s = lock(&self.shared);
            s.query = text.trim().to_string();
            s.cursor = None;
            s.has_more = false;
            s.rows.clear();
            s.error = None;
            show(&ui, &s);
        }
        request_list(&self.hub(), false);
    }

    /// Appends the next page of the list.
    fn more(&self) {
        request_list(&self.hub(), true);
    }

    /// Opens a person's detail panel. Nothing is sent: the row already has the profile.
    fn select(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let Some(profile) = s.rows.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let seat = seat_for(&s.reference.seats, &profile.id);
        ui.show_detail(detail_view(&profile, &seat));
        ui.show_detail_error("");
        s.detail_shown = None;
        s.selected = Some(profile);
        show(&ui, &s);
    }

    /// A day was picked in a start date picker: the cell at `index` of the shown month becomes the date.
    fn pick(&self, field: Field, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let (year, month) = grid_month(&s, field, &ui);
        let cell = usize::try_from(index)
            .ok()
            .and_then(|i| calendar::month_grid(year, month, 0).get(i).copied());
        let Some(cell) = cell.filter(|c| c.in_month) else {
            return;
        };
        ui.show_start(field, &iso(cell.date));
        set_shown(&mut s, field, Some((year, month)));
        show(&ui, &s);
    }

    /// The start date picker's shown month moves by `step` months.
    fn step_month(&self, field: Field, step: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let (year, month) = grid_month(&s, field, &ui);
        let moved = Date::new(year, month, 1)
            .expect("the first day of a month")
            .add_months(step);
        set_shown(&mut s, field, Some((moved.year, moved.month)));
        show(&ui, &s);
    }

    /// Adds a person from the add form. The fields are checked first; the server checks the rest.
    fn add(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.add_form();
        let email = form.email.trim().to_string();
        let name = form.name.trim().to_string();
        let start = form.start.trim().to_string();
        let problem = if !plausible_email(&email) {
            Some("Enter an email address, for example name@example.com.")
        } else if name.is_empty() {
            Some("Enter a display name.")
        } else if !valid_date(&start) {
            Some("Start date must be YYYY-MM-DD.")
        } else {
            None
        };
        if let Some(problem) = problem {
            ui.show_add_error(problem);
            return;
        }
        ui.show_add_error("");
        let body = AdminCreateUser {
            email,
            display_name: name,
            title: None,
            position_id: optional(&form.position),
            department_id: optional(&form.department),
            team_id: optional(&form.team),
            manager_id: optional(&form.manager),
            start_date: optional(&start),
        };
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let done = hub.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { api.admin_create_user(&token, &body).await },
            move |ui, result| apply_write(ui, &done, write, Written::Add, result),
        );
    }

    /// Saves the detail panel's org fields: position, department, team, manager and start date.
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.detail_form();
        let start = form.start.trim().to_string();
        if !valid_date(&start) {
            ui.show_detail_error("Start date must be YYYY-MM-DD.");
            return;
        }
        let Some(id) = lock(&self.shared).selected.as_ref().map(|p| p.id.clone()) else {
            return;
        };
        // Every org field is sent: an empty value clears it, and the same value again changes nothing.
        let body = AdminUpdateUser {
            position_id: Some(form.position.clone()),
            department_id: Some(form.department.clone()),
            team_id: Some(form.team.clone()),
            manager_id: Some(form.manager.clone()),
            start_date: Some(start),
            ..AdminUpdateUser::default()
        };
        // The seat: when it differs from the person's seat, the old seat is vacated and the new one is given the person.
        let seat_change = {
            let s = lock(&self.shared);
            let current = s
                .reference
                .seats
                .iter()
                .find(|seat| seat.person_id.as_deref() == Some(id.as_str()))
                .map(|seat| seat.id.clone());
            let wanted = optional(&form.seat);
            (current != wanted).then_some((current, wanted))
        };
        ui.show_detail_error("");
        self.send_edit(&id, body, seat_change);
    }

    /// Sends the org fields, after the seat change (if any) is made: the old seat is vacated, then the new seat is given
    /// the person. A seat the server refuses stops the save and shows its error in the detail panel.
    fn send_edit(&self, id: &str, body: AdminUpdateUser, seat_change: SeatChange) {
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let id = id.to_string();
        let done = hub.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move {
                if let Some((old, new)) = seat_change {
                    if let Some(old) = old {
                        let vacate = SeatInput {
                            person_id: Some(String::new()),
                            ..SeatInput::default()
                        };
                        api.update_seat(&token, &old, &vacate).await?;
                    }
                    if let Some(new) = new {
                        let seat = SeatInput {
                            person_id: Some(id.clone()),
                            ..SeatInput::default()
                        };
                        api.update_seat(&token, &new, &seat).await?;
                    }
                }
                api.admin_update_user(&token, &id, &body).await
            },
            move |ui, result| apply_write(ui, &done, write, Written::Edit, result),
        );
    }

    /// Deactivates or reactivates the selected person.
    fn set_active(&self, active: bool) {
        let Some(id) = lock(&self.shared).selected.as_ref().map(|p| p.id.clone()) else {
            return;
        };
        let body = AdminUpdateUser {
            is_active: Some(active),
            ..AdminUpdateUser::default()
        };
        self.send_patch(&id, body, Written::Active);
    }

    /// Grants or revokes a role on the selected person.
    fn set_role(&self, role: &str, grant: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(id) = lock(&self.shared).selected.as_ref().map(|p| p.id.clone()) else {
            return;
        };
        ui.show_detail_error("");
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let role = role.to_string();
        let done = hub.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { api.admin_set_role(&token, &id, &role, grant).await },
            move |ui, result| apply_write(ui, &done, write, Written::Role, result),
        );
    }

    /// Sends a PATCH for the selected person and delivers its result as `kind`.
    fn send_patch(&self, id: &str, body: AdminUpdateUser, kind: Written) {
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let id = id.to_string();
        let done = hub.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { api.admin_update_user(&token, &id, &body).await },
            move |ui, result| apply_write(ui, &done, write, kind, result),
        );
    }
}

/// Starts a list request: a fresh list (`appending` false) or the next page (true). A fresh list shows the loading state.
fn request_list<W: UsersWindow>(hub: &Hub<W>, appending: bool) {
    let (generation, request) = {
        let mut s = lock(&hub.shared);
        if appending && !s.has_more {
            return;
        }
        s.generation += 1;
        s.loading = !appending;
        s.loading_more = appending;
        let cursor = if appending { s.cursor.clone() } else { None };
        let request = list_request(&s.query, cursor);
        if let Some(ui) = hub.ui.upgrade() {
            show(&ui, &s);
        }
        (s.generation, request)
    };
    let Some(source) = hub.source.clone() else {
        return;
    };
    let ui_weak = hub.ui.clone();
    let done = hub.clone();
    source.fetch(
        &ui_weak,
        move |api, token| async move { api.admin_users(&token, &request).await },
        move |ui, result| apply_list(ui, &done, generation, appending, result),
    );
}

/// The query for one page of the admin list.
fn list_request(query: &str, cursor: Option<String>) -> DirectoryQuery {
    DirectoryQuery {
        q: Some(query.to_string()),
        cursor,
        limit: Some(PAGE_SIZE),
        ..DirectoryQuery::default()
    }
}

/// Takes the number of a new write. Only the latest write's result is shown.
fn begin_write<W: UsersWindow>(hub: &Hub<W>) -> u64 {
    let mut s = lock(&hub.shared);
    s.writes += 1;
    s.writes
}

/// Keeps a full load's result if it is the latest, then draws the page.
fn apply_load<W: UsersWindow>(
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
            s.rows.clear();
            s.has_more = false;
            s.error = None;
        }
        Ok(Some((page, reference))) => {
            s.denied = false;
            s.error = None;
            s.rows = page.items;
            s.cursor = page.next_cursor;
            s.has_more = s.cursor.is_some();
            s.reference = reference;
        }
        Err(error) => {
            s.error = Some(page_data::error_text(&error));
            s.rows.clear();
            s.has_more = false;
        }
    }
    show(ui, &s);
}

/// Keeps a list request's result if it is the latest, then draws the list (appended for "Load more").
fn apply_list<W: UsersWindow>(
    ui: &W,
    hub: &Hub<W>,
    generation: u64,
    appending: bool,
    result: Result<ProtoPage<Profile>, ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.generation != generation {
        return;
    }
    s.loading = false;
    s.loading_more = false;
    match result {
        Ok(page) => {
            if appending {
                s.rows.extend(page.items);
            } else {
                s.rows = page.items;
            }
            s.has_more = page.next_cursor.is_some();
            s.cursor = page.next_cursor;
            s.error = None;
        }
        Err(error) => {
            s.error = Some(page_data::error_text(&error));
        }
    }
    show(ui, &s);
}

/// Keeps a write's result if it is the latest write. An added person closes the form and reloads the list; an edit, an
/// activation or a role change updates the row and the detail panel. Errors show where the write was made.
fn apply_write<W: UsersWindow>(
    ui: &W,
    hub: &Hub<W>,
    write: u64,
    kind: Written,
    result: Result<Profile, ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(profile) if kind == Written::Add => {
            ui.clear_add_form();
            ui.show_add_error("");
            s.add_shown = None;
            toast::announce(
                Tone::Success,
                format!(
                    "Added {}. They can sign in with {}.",
                    profile.display_name, profile.email
                ),
            );
            drop(s);
            request_list(hub, false);
            ui.refresh_company();
        }
        Ok(profile) => {
            toast::announce(Tone::Success, notice(kind, &profile));
            ui.show_detail_error("");
            if let Some(row) = s.rows.iter_mut().find(|p| p.id == profile.id) {
                *row = profile.clone();
            }
            let seat = seat_for(&s.reference.seats, &profile.id);
            ui.show_detail(detail_view(&profile, &seat));
            s.selected = Some(profile);
            show(ui, &s);
            drop(s);
            ui.refresh_company();
        }
        Err(error) => {
            let text = page_data::error_text(&error);
            if kind == Written::Add {
                ui.show_add_error(&text);
            } else {
                ui.show_detail_error(&text);
            }
        }
    }
}

/// The line the page shows after a successful write.
fn notice(kind: Written, profile: &Profile) -> String {
    match kind {
        Written::Edit => format!("Saved changes to {}.", profile.display_name),
        Written::Active if profile.is_active => format!("Reactivated {}.", profile.display_name),
        Written::Active => format!("Deactivated {}.", profile.display_name),
        Written::Role => format!("Updated the roles of {}.", profile.display_name),
        Written::Add => format!("Added {}.", profile.display_name),
    }
}

/// Draws the page from the request state: the state line, the list, the pickers and the calendars.
fn show<W: UsersWindow>(ui: &W, s: &Shared) {
    ui.show_denied(s.denied);
    let page_state = if s.denied {
        LoadState::Loaded
    } else if s.loading {
        LoadState::Loading
    } else if s.error.is_some() {
        LoadState::Error
    } else if s.rows.is_empty() {
        LoadState::Empty
    } else {
        LoadState::Loaded
    };
    ui.show_state(page_state);
    ui.show_error(s.error.as_deref().unwrap_or_default());
    ui.show_loading_more(s.loading_more);
    ui.show_has_more(s.has_more);

    ui.show_weekdays(
        (0..7)
            .map(|day| calendar::weekday_label(day).to_string())
            .collect(),
    );
    let add_chosen = parse_iso(&ui.start_text(Field::Add));
    ui.show_calendar(
        Field::Add,
        dates::cells(month_shown(s.add_shown, add_chosen), add_chosen),
        dates::month_title(month_shown(s.add_shown, add_chosen)),
    );
    let detail_chosen = parse_iso(&ui.start_text(Field::Detail));
    ui.show_calendar(
        Field::Detail,
        dates::cells(month_shown(s.detail_shown, detail_chosen), detail_chosen),
        dates::month_title(month_shown(s.detail_shown, detail_chosen)),
    );

    let selected = s.selected.as_ref().map(|p| p.id.as_str());
    let rows: Vec<UserRowView> = s
        .rows
        .iter()
        .map(|p| row(p, selected == Some(p.id.as_str())))
        .collect();
    ui.show_rows(rows);

    let r = &s.reference;
    ui.show_options(Options {
        position: position_choices(&r.positions, true),
        seat: seat_choices(&r.seats),
        department: department_choices(&r.departments, true),
        team: team_choices(&r.teams, &r.departments, true),
        person: person_choices(&r.people, true),
    });
}

/// The id of the seat a person holds, or an empty string when none.
fn seat_for(seats: &[Seat], person_id: &str) -> String {
    seats
        .iter()
        .find(|seat| seat.person_id.as_deref() == Some(person_id))
        .map(|seat| seat.id.clone())
        .unwrap_or_default()
}

/// The seats a person can take, with "No seat" first. Closed seats are left out.
fn seat_choices(seats: &[Seat]) -> Vec<Choice> {
    let mut out = vec![choice("", "No seat")];
    out.extend(seats.iter().filter(|s| s.state != SEAT_CLOSED).map(|s| {
        let title = s.job_profile_title.as_deref().unwrap_or("No position");
        choice(&s.id, &format!("{title} · {}", s.team_name))
    }));
    out
}

/// The detail panel's values for a person: the saved values, and the seat they hold.
fn detail_view(p: &Profile, seat: &str) -> DetailView {
    DetailView {
        name: p.display_name.clone(),
        email: p.email.clone(),
        status: status(p.is_active).to_string(),
        roles: roles_text(&p.roles),
        active: p.is_active,
        admin: p.roles.iter().any(|r| r == ROLE_ADMIN),
        hr: p.roles.iter().any(|r| r == ROLE_HR),
        position: p.position_id.clone().unwrap_or_default(),
        department: p.department_id.clone().unwrap_or_default(),
        team: p.team_id.clone().unwrap_or_default(),
        manager: p.manager_id.clone().unwrap_or_default(),
        start: p.start_date.clone().unwrap_or_default(),
        seat: seat.to_string(),
    }
}

/// One row of the list.
fn row(p: &Profile, selected: bool) -> UserRowView {
    UserRowView {
        id: p.id.clone(),
        name: p.display_name.clone(),
        email: p.email.clone(),
        title: p.title.clone().unwrap_or_default(),
        department: p.department_name.clone().unwrap_or_default(),
        team: p.team_name.clone().unwrap_or_default(),
        status: status(p.is_active).to_string(),
        active: p.is_active,
        roles: roles_text(&p.roles),
        initials: initials(&p.display_name),
        selected,
    }
}

/// "Active" or "Deactivated".
pub fn status(active: bool) -> &'static str {
    if active { "Active" } else { "Deactivated" }
}

/// The role names a person holds, as the page shows them: "Admin, HR", or empty.
pub fn roles_text(roles: &[String]) -> String {
    roles
        .iter()
        .map(|role| match role.as_str() {
            ROLE_ADMIN => "Admin",
            ROLE_HR => "HR",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The month a start date picker shows: the month it was last moved to, else the chosen date's, else this month.
fn month_shown(shown: Option<(i32, u32)>, chosen: Option<Date>) -> (i32, u32) {
    shown
        .or_else(|| chosen.map(|date| (date.year, date.month)))
        .unwrap_or_else(|| {
            let now = today();
            (now.year, now.month)
        })
}

/// The month a start date picker shows now, from the page state (its text is the chosen date).
fn grid_month<W: UsersWindow>(s: &Shared, field: Field, ui: &W) -> (i32, u32) {
    let shown = match field {
        Field::Add => s.add_shown,
        Field::Detail => s.detail_shown,
    };
    month_shown(shown, parse_iso(&ui.start_text(field)))
}

/// Records the month a start date picker shows.
fn set_shown(s: &mut Shared, field: Field, shown: Option<(i32, u32)>) {
    match field {
        Field::Add => s.add_shown = shown,
        Field::Detail => s.detail_shown = shown,
    }
}

/// A form value as an optional field: empty (after trimming) is absent.
fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// A rough check that an address has a name, an `@`, a domain with a dot, and no spaces. The server decides the rest.
pub fn plausible_email(email: &str) -> bool {
    let Some(at) = email.find('@') else {
        return false;
    };
    at > 0
        && email[at..].contains('.')
        && !email[at..].ends_with('.')
        && !email.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_need_a_name_an_at_and_a_dotted_domain() {
        assert!(plausible_email("mina@example.com"));
        assert!(!plausible_email("mina.example.com"));
        assert!(!plausible_email("@example.com"));
        assert!(!plausible_email("mina@example"));
        assert!(!plausible_email("mina@example."));
        assert!(!plausible_email("mi na@example.com"));
    }

    #[test]
    fn roles_read_as_their_labels() {
        assert_eq!(roles_text(&["admin".into(), "hr".into()]), "Admin, HR");
        assert_eq!(roles_text(&[]), "");
        assert_eq!(status(false), "Deactivated");
    }

    #[test]
    fn blank_optional_fields_are_absent() {
        assert_eq!(optional("  "), None);
        assert_eq!(optional(" d1 "), Some("d1".to_string()));
    }
}
