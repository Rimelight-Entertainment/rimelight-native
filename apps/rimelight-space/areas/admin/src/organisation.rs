//! Admin > Organisation (SP.3e): departments, teams and positions at `/admin/organisation`, on the page data pattern
//! (`space_page_kit::page_data`). The tab is the window's `tab` (0 Departments, 1 Teams, 2 Positions, 3 Seats, 4 Budgets). A row
//! fills the form in a modal dialog over the page; "New" clears it. Saving creates the item, or updates the selected one, and a
//! successful save or delete closes the dialog (`form-open`). Delete removes it, and the server's refusal (409, with a message
//! such as "Move its 3 people first.") shows in the dialog. The page's markup is `ui/organisation.slint`.
//!
//! The page loads `GET /v1/me/profile` first (the admin gate), then the departments, teams, positions and the org chart's
//! people. Every write takes a write number and a result for an older write is dropped.
//!
//! Plain Rust. The window side is [`OrganisationWindow`], which `wire_admin!` implements for the generated window.

use std::sync::{Arc, Mutex};

use rimelight_ui_kit::toast::{self, Tone};
use slint::Weak;
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::is_admin;
use space_page_kit::picker::{Choice, choice, department_choices, person_choices};
use space_proto::people::{
    Department, DepartmentInput, Position, PositionInput, SEAT_CLOSED, SEAT_FILLED, SEAT_OPEN,
    SEAT_PLANNED, Seat, SeatInput, Team, TeamBudget, TeamBudgetInput, TeamInput,
};

use crate::{AdminCompany, Reference, fetch_reference};

/// What a full load brings back: the reference lists, or `None` when the caller is not an admin.
pub type Loaded = Option<Reference>;

/// The result of a save: the item's id and name, as the server returned them, and the whole row, so the page can show it
/// at once (the lists still reload after it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub name: String,
    pub row: Row,
}

/// A saved item as the server returned it, in the shape of its list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Department(Department),
    Team(Team),
    Position(Position),
    Seat(Seat),
    Budget(TeamBudget),
}

impl Saved {
    fn department(d: Department) -> Self {
        Self {
            id: d.id.clone(),
            name: d.name.clone(),
            row: Row::Department(d),
        }
    }

    fn team(t: Team) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            row: Row::Team(t),
        }
    }

    fn position(p: Position) -> Self {
        Self {
            id: p.id.clone(),
            name: p.title.clone(),
            row: Row::Position(p),
        }
    }

    fn seat(s: Seat) -> Self {
        Self {
            id: s.id.clone(),
            name: s.job_profile_title.clone().unwrap_or_default(),
            row: Row::Seat(s),
        }
    }

    fn budget(b: TeamBudget) -> Self {
        Self {
            id: b.team_id.clone(),
            name: b.team_name.clone(),
            row: Row::Budget(b),
        }
    }
}

/// One item to create or update, with its body.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Department(DepartmentInput),
    Team(TeamInput),
    Position(PositionInput),
    Seat(SeatInput),
    /// A team's budget. The selected id is the team's.
    Budget(TeamBudgetInput),
}

/// The form's fields for one item. The window holds them as text; the same struct carries them both ways.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub parent: String,
    /// A seat's position (its job profile id).
    pub position: String,
    pub person: String,
    pub level: String,
    pub description: String,
    pub reports: String,
    pub seat_state: String,
    pub salary_min: String,
    pub salary_max: String,
    pub currency: String,
    pub cap: String,
}

/// One row of the list as the window draws it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowView {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub selected: bool,
}

/// The table over the rows: its columns (title and width) and, per row, (selected, cells).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableView {
    pub columns: Vec<(&'static str, f32)>,
    pub rows: Vec<(String, bool, Vec<String>)>,
}

/// The choices of the pickers on the page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrgOptions {
    pub parent: Vec<Choice>,
    pub person: Vec<Choice>,
    pub level: Vec<Choice>,
    pub team: Vec<Choice>,
    pub position: Vec<Choice>,
    pub seat: Vec<Choice>,
    pub state: Vec<Choice>,
}

/// The window side of the organisation page. `wire_admin!` implements it for the generated window.
pub trait OrganisationWindow: AdminCompany + slint::ComponentHandle + 'static {
    /// The tab the window shows: 0 Departments, 1 Teams, 2 Positions, 3 Seats, 4 Budgets.
    fn tab(&self) -> i32;
    fn show_state(&self, state: LoadState);
    fn show_denied(&self, denied: bool);
    fn show_error(&self, text: &str);
    fn show_rows(&self, rows: Vec<RowView>);
    fn show_table(&self, table: TableView);
    fn show_options(&self, options: OrgOptions);
    fn show_level_choice(&self, value: &str);
    fn show_form_title(&self, title: &str);
    fn show_fields(&self, fields: &Fields);
    /// The form's fields, as the user has them.
    fn read_fields(&self) -> Fields;
    fn show_form_error(&self, text: &str);
    fn show_form_open(&self, on: bool);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_tab_changed(&self, f: impl Fn(i32) + 'static);
    fn on_pick(&self, f: impl Fn(String) + 'static);
    fn on_new_item(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn() + 'static);
    fn on_delete_item(&self, f: impl Fn() + 'static);
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A result for an older number is dropped.
    generation: u64,
    /// Bumped by every write. A result for an older write is dropped.
    writes: u64,
    reference: Reference,
    /// The selected item's id, or empty for a new one.
    selected: String,
    loading: bool,
    /// The text of the last failed load. `None` when there is none.
    error: Option<String>,
    denied: bool,
}

/// What a request needs to reach the window and the session, cloned into each worker's callbacks.
struct Hub<W: OrganisationWindow> {
    ui: Weak<W>,
    shared: Arc<Mutex<Shared>>,
    source: Option<Arc<DataSource>>,
}

impl<W: OrganisationWindow> Clone for Hub<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.source.clone(),
        }
    }
}

/// The organisation page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Page<W: OrganisationWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: OrganisationWindow> Clone for Page<W> {
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
pub fn wire<W: OrganisationWindow>(ui: &W, data: DataCell) -> Page<W> {
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
    ui.on_tab_changed(move |_| target.new_item());

    let target = page.clone();
    ui.on_pick(move |id| target.pick(&id));

    let target = page.clone();
    ui.on_new_item(move || target.new_item());

    let target = page.clone();
    ui.on_save(move || target.save());

    let target = page.clone();
    ui.on_delete_item(move || target.delete());

    page
}

impl<W: OrganisationWindow> Page<W> {
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

    /// The selected item's id (empty for none).
    pub fn selected(&self) -> String {
        lock(&self.shared).selected.clone()
    }

    /// Delivers a load's result, as the worker would. For tests.
    pub fn deliver(&self, generation: u64, result: Result<Loaded, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_load(&ui, &self.hub(), generation, result);
        }
    }

    /// Delivers a save's result for the current tab, as the worker would. For tests.
    pub fn deliver_save(&self, write: u64, result: Result<Saved, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            let tab = ui.tab();
            apply_save(&ui, &self.hub(), write, tab, result);
        }
    }

    /// Delivers a delete's result for the selected item, as the worker would. For tests.
    pub fn deliver_delete(&self, write: u64, result: Result<(), ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            let tab = ui.tab();
            let id = self.selected();
            apply_delete(&ui, &self.hub(), write, tab, &id, result);
        }
    }

    /// Loads the lists for the page. Every load starts from the loading state.
    fn load(&self) {
        fetch_all(&self.hub(), false);
    }

    /// Opens an item's form: its fields are filled from the reference lists.
    fn pick(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let tab = ui.tab();
        let r = &s.reference;
        let fields = match tab {
            1 => r.teams.iter().find(|t| t.id == id).map(|t| Fields {
                name: t.name.clone(),
                parent: t.department_id.clone(),
                person: t.lead_id.clone().unwrap_or_default(),
                ..Fields::default()
            }),
            2 => r.positions.iter().find(|p| p.id == id).map(|p| Fields {
                name: p.title.clone(),
                level: p.level.clone(),
                description: p.description.clone(),
                salary_min: p.salary_min.clone().unwrap_or_default(),
                salary_max: p.salary_max.clone().unwrap_or_default(),
                currency: p.currency.clone().unwrap_or_default(),
                ..Fields::default()
            }),
            3 => r.seats.iter().find(|s| s.id == id).map(|s| Fields {
                parent: s.team_id.clone(),
                position: s.job_profile_id.clone().unwrap_or_default(),
                person: s.person_id.clone().unwrap_or_default(),
                reports: s.reports_to_seat_id.clone().unwrap_or_default(),
                seat_state: s.state.clone(),
                ..Fields::default()
            }),
            4 => r.budgets.iter().find(|b| b.team_id == id).map(|b| Fields {
                name: b.team_name.clone(),
                cap: b.cap.clone().unwrap_or_default(),
                currency: b.currency.clone().unwrap_or_default(),
                ..Fields::default()
            }),
            _ => r.departments.iter().find(|d| d.id == id).map(|d| Fields {
                name: d.name.clone(),
                parent: d.parent_department_id.clone().unwrap_or_default(),
                person: d.head_id.clone().unwrap_or_default(),
                ..Fields::default()
            }),
        };
        let Some(fields) = fields else { return };
        s.selected = id.to_string();
        show_fields(&ui, &fields);
        show(&ui, &s);
    }

    /// Clears the form for a new item of the current tab.
    fn new_item(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        s.selected = String::new();
        let fields = if ui.tab() == 3 {
            Fields {
                seat_state: SEAT_OPEN.to_string(),
                ..Fields::default()
            }
        } else {
            Fields::default()
        };
        show_fields(&ui, &fields);
        ui.show_form_error("");
        show(&ui, &s);
    }

    /// Creates the item in the form, or saves the selected one.
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let tab = ui.tab();
        let form = ui.read_fields();
        let name = form.name.trim().to_string();
        let parent = form.parent.trim().to_string();
        let person = form.person.trim().to_string();
        let level = form.level.trim().to_string();
        let description = form.description.trim().to_string();
        let reports = form.reports.trim().to_string();
        let seat_state = form.seat_state.trim().to_string();
        let position = form.position.trim().to_string();
        let salary_min = form.salary_min.trim().to_string();
        let salary_max = form.salary_max.trim().to_string();
        let currency = form.currency.trim().to_string();
        let cap = form.cap.trim().to_string();
        let id = {
            let s = lock(&self.shared);
            (!s.selected.is_empty()).then(|| s.selected.clone())
        };
        let item = match tab {
            3 if parent.is_empty() => return ui.show_form_error("Choose a team."),
            3 if position.is_empty() => return ui.show_form_error("Choose a position."),
            3 => Item::Seat(SeatInput {
                team_id: Some(parent),
                job_profile_id: Some(position),
                state: Some(seat_state),
                person_id: Some(person),
                reports_to_seat_id: Some(reports),
            }),
            4 if id.is_none() => return ui.show_form_error("Pick a team to set its budget."),
            4 => Item::Budget(TeamBudgetInput {
                cap: Some(cap),
                currency: Some(currency),
            }),
            1 if name.is_empty() => return ui.show_form_error("Enter a name."),
            1 if parent.is_empty() => return ui.show_form_error("Choose a department."),
            1 => Item::Team(TeamInput {
                name: Some(name),
                department_id: Some(parent),
                lead_id: Some(person),
            }),
            2 if name.is_empty() => return ui.show_form_error("Enter a title."),
            2 => Item::Position(PositionInput {
                title: Some(name),
                level: Some(level),
                description: Some(description),
                salary_min: Some(salary_min),
                salary_max: Some(salary_max),
                currency: Some(currency),
            }),
            _ if name.is_empty() => return ui.show_form_error("Enter a name."),
            _ => Item::Department(DepartmentInput {
                name: Some(name),
                parent_department_id: Some(parent),
                head_id: Some(person),
            }),
        };
        ui.show_form_error("");
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let done = hub.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { save_item(&api, &token, item, id).await },
            move |ui, result| apply_save(ui, &done, write, tab, result),
        );
    }

    /// Deletes the selected item of the current tab. The server refuses while it is in use; the message shows in the form.
    fn delete(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let tab = ui.tab();
        let id = lock(&self.shared).selected.clone();
        if id.is_empty() {
            return;
        }
        if tab == 4 {
            return ui.show_form_error("Clear the cap and save to remove a team's budget.");
        }
        ui.show_form_error("");
        let hub = self.hub();
        let write = begin_write(&hub);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let done = hub.clone();
        let work_id = id.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { delete_item(&api, &token, tab, &work_id).await },
            move |ui, result| apply_delete(ui, &done, write, tab, &id, result),
        );
    }
}

/// Loads the lists. `quiet` keeps the page on what it shows (a reload after a write); otherwise it shows the loading state.
fn fetch_all<W: OrganisationWindow>(hub: &Hub<W>, quiet: bool) {
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
            Ok(Some(fetch_reference(&api, &token).await?))
        },
        move |ui, result| apply_load(ui, &done, generation, result),
    );
}

/// Saves an item: creates it, or updates it when `id` is set. Runs on a worker.
async fn save_item(
    api: &ApiClient,
    token: &str,
    item: Item,
    id: Option<String>,
) -> Result<Saved, ClientError> {
    match (item, id) {
        (Item::Department(body), None) => Ok(Saved::department(
            api.create_department(token, &body).await?,
        )),
        (Item::Department(body), Some(id)) => Ok(Saved::department(
            api.update_department(token, &id, &body).await?,
        )),
        (Item::Team(body), None) => Ok(Saved::team(api.create_team(token, &body).await?)),
        (Item::Team(body), Some(id)) => Ok(Saved::team(api.update_team(token, &id, &body).await?)),
        (Item::Position(body), None) => {
            Ok(Saved::position(api.create_position(token, &body).await?))
        }
        (Item::Position(body), Some(id)) => Ok(Saved::position(
            api.update_position(token, &id, &body).await?,
        )),
        (Item::Seat(body), None) => Ok(Saved::seat(api.create_seat(token, &body).await?)),
        (Item::Seat(body), Some(id)) => Ok(Saved::seat(api.update_seat(token, &id, &body).await?)),
        (Item::Budget(body), Some(team)) => Ok(Saved::budget(
            api.set_team_budget(token, &team, &body).await?,
        )),
        (Item::Budget(_), None) => Err(ClientError::Protocol("Pick a team first.".to_string())),
    }
}

/// Deletes an item of the tab (0 departments, 1 teams, 2 positions). Runs on a worker.
async fn delete_item(api: &ApiClient, token: &str, tab: i32, id: &str) -> Result<(), ClientError> {
    match tab {
        0 => api.delete_department(token, id).await,
        1 => api.delete_team(token, id).await,
        3 => api.delete_seat(token, id).await,
        _ => api.delete_position(token, id).await,
    }
}

/// Takes the number of a new write. Only the latest write's result is shown.
fn begin_write<W: OrganisationWindow>(hub: &Hub<W>) -> u64 {
    let mut s = lock(&hub.shared);
    s.writes += 1;
    s.writes
}

/// Keeps a load's result if it is the latest, then draws the page.
fn apply_load<W: OrganisationWindow>(
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
            s.reference = Reference::default();
            s.error = None;
        }
        Ok(Some(reference)) => {
            s.denied = false;
            s.error = None;
            s.reference = reference;
        }
        Err(error) => {
            s.error = Some(page_data::error_text(&error));
            s.reference = Reference::default();
        }
    }
    show(ui, &s);
}

/// The word a tab's item goes by in its toast ("Team saved"). Tab 4, the budgets, is a budget.
fn kind_word(tab: i32) -> &'static str {
    match tab {
        1 => "Team",
        2 => "Position",
        3 => "Seat",
        4 => "Budget",
        _ => "Department",
    }
}

/// Puts a saved row into the reference lists, replacing the row with the same key or adding it. The row shows at once; the
/// reload that follows brings the same data back from the server.
fn put_row(r: &mut Reference, row: Row) {
    fn put<T>(list: &mut Vec<T>, item: T, key: impl Fn(&T) -> &str) {
        match list.iter().position(|x| key(x) == key(&item)) {
            Some(index) => list[index] = item,
            None => list.push(item),
        }
    }
    match row {
        Row::Department(d) => put(&mut r.departments, d, |x| x.id.as_str()),
        Row::Team(t) => put(&mut r.teams, t, |x| x.id.as_str()),
        Row::Position(p) => put(&mut r.positions, p, |x| x.id.as_str()),
        Row::Seat(s) => put(&mut r.seats, s, |x| x.id.as_str()),
        Row::Budget(b) => put(&mut r.budgets, b, |x| x.team_id.as_str()),
    }
}

/// Takes a row with `id` out of the list of `tab`, for a delete. A seat or a position also leaves the lists that show it.
fn drop_row(r: &mut Reference, tab: i32, id: &str) {
    match tab {
        1 => r.teams.retain(|t| t.id != id),
        2 => r.positions.retain(|p| p.id != id),
        3 => r.seats.retain(|s| s.id != id),
        _ => r.departments.retain(|d| d.id != id),
    }
}

/// Keeps a save's result if it is the latest write. The saved item shows at once and stays selected, the lists reload, and a
/// toast says it was saved.
fn apply_save<W: OrganisationWindow>(
    ui: &W,
    hub: &Hub<W>,
    write: u64,
    tab: i32,
    result: Result<Saved, ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(saved) => {
            let created = s.selected.is_empty();
            s.selected = saved.id;
            put_row(&mut s.reference, saved.row);
            ui.show_form_error("");
            ui.show_form_open(false);
            let verb = if created { "created" } else { "saved" };
            toast::announce(Tone::Success, format!("{} {verb}", kind_word(tab)));
            show(ui, &s);
            drop(s);
            fetch_all(hub, true);
            ui.refresh_company();
        }
        Err(error) => {
            ui.show_form_error(&page_data::error_text(&error));
        }
    }
}

/// Keeps a delete's result if it is the latest write. The row leaves the lists at once, a deleted item clears the form, a
/// toast says it was deleted, and the lists reload.
fn apply_delete<W: OrganisationWindow>(
    ui: &W,
    hub: &Hub<W>,
    write: u64,
    tab: i32,
    id: &str,
    result: Result<(), ClientError>,
) {
    let mut s = lock(&hub.shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(()) => {
            s.selected = String::new();
            drop_row(&mut s.reference, tab, id);
            show_fields(ui, &Fields::default());
            ui.show_form_error("");
            ui.show_form_open(false);
            toast::announce(Tone::Success, format!("{} deleted", kind_word(tab)));
            show(ui, &s);
            drop(s);
            fetch_all(hub, true);
            ui.refresh_company();
        }
        Err(error) => {
            ui.show_form_error(&page_data::error_text(&error));
        }
    }
}

/// Writes the form's fields to the window, and the Level picker's value for the level.
fn show_fields<W: OrganisationWindow>(ui: &W, fields: &Fields) {
    ui.show_fields(fields);
    ui.show_level_choice(level_choice(&fields.level));
}

/// The state choices for a seat. A filled seat takes its state from its person.
fn state_choices() -> Vec<Choice> {
    vec![
        choice(SEAT_PLANNED, "Planned"),
        choice(SEAT_OPEN, "Open"),
        choice(SEAT_FILLED, "Filled (set by the person)"),
        choice(SEAT_CLOSED, "Closed"),
    ]
}

/// The level presets a Level picker offers, in order, for positions and seats. The saved value is the label.
const LEVEL_PRESETS: [&str; 5] = ["Junior", "Associate", "Senior", "Lead", "Director"];

/// The level choices for a position or seat, with "None" first (value "", saved as empty).
fn level_choices() -> Vec<Choice> {
    let mut out = vec![choice("", "None")];
    out.extend(LEVEL_PRESETS.iter().map(|level| choice(level, level)));
    out
}

/// The picker's value for a stored level: the preset it equals, or "" (None). An old free-text level shows as None, and it
/// is kept as it is until the user picks a preset.
fn level_choice(level: &str) -> &str {
    LEVEL_PRESETS
        .iter()
        .copied()
        .find(|preset| *preset == level)
        .unwrap_or("")
}

/// The teams a seat can sit in.
fn team_choices(teams: &[Team]) -> Vec<Choice> {
    teams.iter().map(|t| choice(&t.id, &t.name)).collect()
}

/// The positions a seat can hold, by title.
fn position_choices(positions: &[Position]) -> Vec<Choice> {
    positions.iter().map(|p| choice(&p.id, &p.title)).collect()
}

/// The seats a seat can report to, with "None" first. Closed seats are left out.
fn seat_choices(seats: &[Seat]) -> Vec<Choice> {
    let mut out = vec![choice("", "None")];
    out.extend(seats.iter().filter(|s| s.state != SEAT_CLOSED).map(|s| {
        let title = s.job_profile_title.as_deref().unwrap_or("No position");
        choice(&s.id, &format!("{title} · {}", s.team_name))
    }));
    out
}

/// A level as a row shows it: the level, or "No level" when it is empty.
fn seat_level(level: &str) -> String {
    if level.is_empty() {
        "No level".to_string()
    } else {
        level.to_string()
    }
}

/// A budget's detail line: the cap, what is used and what remains, or "No budget".
fn budget_detail(b: &TeamBudget) -> String {
    match (&b.cap, &b.currency) {
        (Some(cap), Some(currency)) => format!(
            "{currency} {cap} cap · {} used · {} left",
            b.used,
            b.remaining.as_deref().unwrap_or("0.00")
        ),
        _ => "No budget".to_string(),
    }
}

/// Draws the page from the request state: the state line, the rows of the current tab, the pickers and the form's title.
fn show<W: OrganisationWindow>(ui: &W, s: &Shared) {
    let tab = ui.tab();
    ui.show_denied(s.denied);
    let page_state = if s.denied {
        LoadState::Loaded
    } else if s.loading {
        LoadState::Loading
    } else if s.error.is_some() {
        LoadState::Error
    } else if tab_is_empty(&s.reference, tab) {
        LoadState::Empty
    } else {
        LoadState::Loaded
    };
    ui.show_state(page_state);
    ui.show_error(s.error.as_deref().unwrap_or_default());

    ui.show_rows(rows_of(&s.reference, tab, &s.selected));
    ui.show_table(table_of(&s.reference, tab, &s.selected));

    let r = &s.reference;
    // Departments may have no parent; a team needs a department.
    ui.show_options(OrgOptions {
        parent: department_choices(&r.departments, tab != 1),
        person: person_choices(&r.people, true),
        level: level_choices(),
        team: team_choices(&r.teams),
        position: position_choices(&r.positions),
        seat: seat_choices(&r.seats),
        state: state_choices(),
    });

    let kind = match tab {
        1 => "team",
        2 => "position",
        3 => "seat",
        4 => "budget",
        _ => "department",
    };
    ui.show_form_title(&if s.selected.is_empty() {
        format!("New {kind}")
    } else {
        format!("Edit {kind}")
    });
}

/// True when the current tab has nothing to list.
fn tab_is_empty(r: &Reference, tab: i32) -> bool {
    match tab {
        1 => r.teams.is_empty(),
        2 => r.positions.is_empty(),
        3 => r.seats.is_empty(),
        4 => r.budgets.is_empty(),
        _ => r.departments.is_empty(),
    }
}

/// The rows of the current tab, each with its detail line. The selected one is marked.
fn rows_of(r: &Reference, tab: i32, selected: &str) -> Vec<RowView> {
    let dept_name = |id: &str| {
        r.departments
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    match tab {
        3 => r
            .seats
            .iter()
            .map(|s| RowView {
                id: s.id.clone(),
                title: s
                    .job_profile_title
                    .as_deref()
                    .unwrap_or("No position")
                    .to_string(),
                detail: format!(
                    "{} · {} · {} · {}",
                    s.team_name,
                    seat_level(&s.level),
                    s.state,
                    s.person_name.as_deref().unwrap_or("vacant")
                ),
                selected: s.id == selected,
            })
            .collect(),
        4 => r
            .budgets
            .iter()
            .map(|b| RowView {
                id: b.team_id.clone(),
                title: b.team_name.clone(),
                detail: budget_detail(b),
                selected: b.team_id == selected,
            })
            .collect(),
        1 => r
            .teams
            .iter()
            .map(|t| RowView {
                id: t.id.clone(),
                title: t.name.clone(),
                detail: format!(
                    "{} · {} people · {}",
                    dept_name(&t.department_id),
                    t.member_count,
                    r.budgets
                        .iter()
                        .find(|b| b.team_id == t.id)
                        .map_or_else(|| "No budget".to_string(), budget_detail)
                ),
                selected: t.id == selected,
            })
            .collect(),
        2 => r
            .positions
            .iter()
            .map(|p| RowView {
                id: p.id.clone(),
                title: p.title.clone(),
                detail: format!(
                    "{} · {} filled · {} open",
                    seat_level(&p.level),
                    p.filled_count,
                    p.open_count
                ),
                selected: p.id == selected,
            })
            .collect(),
        _ => r
            .departments
            .iter()
            .map(|d| RowView {
                id: d.id.clone(),
                title: d.name.clone(),
                detail: format!("{} people", d.member_count),
                selected: d.id == selected,
            })
            .collect(),
    }
}

/// The table of the current tab: its columns, and one row per item in the order of [`rows_of`], so a pick by row index
/// maps to the same id. The selected item is marked. Positions carry no salary, so their salary range is the range of
/// their seats, and their seat counts are the seats that hold them.
fn table_of(r: &Reference, tab: i32, selected: &str) -> TableView {
    let dept = |id: Option<&str>| -> String {
        id.and_then(|id| r.departments.iter().find(|d| d.id == id))
            .map_or_else(|| "None".to_string(), |d| d.name.clone())
    };
    let person = |id: Option<&str>| -> String {
        id.and_then(|id| r.people.iter().find(|p| p.id == id))
            .map_or_else(|| "None".to_string(), |p| p.name.clone())
    };
    let seat_title = |id: Option<&str>| -> String {
        id.and_then(|id| r.seats.iter().find(|s| s.id == id))
            .map_or_else(|| "None".to_string(), seat_position)
    };
    let budget_of =
        |team_id: &str| -> Option<&TeamBudget> { r.budgets.iter().find(|b| b.team_id == team_id) };
    match tab {
        3 => TableView {
            columns: vec![
                ("Position", 200.0),
                ("Team", 180.0),
                ("State", 120.0),
                ("Person", 180.0),
                ("Reports to", 180.0),
            ],
            rows: r
                .seats
                .iter()
                .map(|s| {
                    (
                        s.id.clone(),
                        s.id == selected,
                        vec![
                            seat_position(s),
                            s.team_name.clone(),
                            state_word(&s.state),
                            s.person_name
                                .clone()
                                .unwrap_or_else(|| "Vacant".to_string()),
                            seat_title(s.reports_to_seat_id.as_deref()),
                        ],
                    )
                })
                .collect(),
        },
        4 => TableView {
            columns: vec![
                ("Team", 220.0),
                ("Budget used / cap", 260.0),
                ("Remaining", 160.0),
            ],
            rows: r
                .budgets
                .iter()
                .map(|b| {
                    (
                        b.team_id.clone(),
                        b.team_id == selected,
                        vec![
                            b.team_name.clone(),
                            budget_cell(b),
                            b.remaining.clone().unwrap_or_else(|| "None".to_string()),
                        ],
                    )
                })
                .collect(),
        },
        1 => TableView {
            columns: vec![
                ("Name", 220.0),
                ("Department", 180.0),
                ("Lead", 180.0),
                ("Members", 110.0),
                ("Budget used / cap", 260.0),
            ],
            rows: r
                .teams
                .iter()
                .map(|t| {
                    (
                        t.id.clone(),
                        t.id == selected,
                        vec![
                            t.name.clone(),
                            dept(Some(&t.department_id)),
                            person(t.lead_id.as_deref()),
                            t.member_count.to_string(),
                            budget_of(&t.id).map_or_else(|| "No budget".to_string(), budget_cell),
                        ],
                    )
                })
                .collect(),
        },
        2 => TableView {
            columns: vec![
                ("Title", 220.0),
                ("Level", 130.0),
                ("Salary", 200.0),
                ("Seats filled / open", 200.0),
            ],
            rows: r
                .positions
                .iter()
                .map(|p| {
                    let seats: Vec<&Seat> = r
                        .seats
                        .iter()
                        .filter(|s| s.job_profile_id.as_deref() == Some(p.id.as_str()))
                        .collect();
                    let filled = seats.iter().filter(|s| s.state == SEAT_FILLED).count();
                    let open = seats.iter().filter(|s| s.state == SEAT_OPEN).count();
                    (
                        p.id.clone(),
                        p.id == selected,
                        vec![
                            p.title.clone(),
                            seat_level(&p.level),
                            salary_cell(&seats),
                            format!("{filled} filled / {open} open"),
                        ],
                    )
                })
                .collect(),
        },
        _ => TableView {
            columns: vec![
                ("Name", 220.0),
                ("Parent", 180.0),
                ("Head", 180.0),
                ("Members", 110.0),
            ],
            rows: r
                .departments
                .iter()
                .map(|d| {
                    (
                        d.id.clone(),
                        d.id == selected,
                        vec![
                            d.name.clone(),
                            dept(d.parent_department_id.as_deref()),
                            person(d.head_id.as_deref()),
                            d.member_count.to_string(),
                        ],
                    )
                })
                .collect(),
        },
    }
}

/// A seat's position title, as the table shows it: the job profile's title, or "No position".
fn seat_position(s: &Seat) -> String {
    s.job_profile_title
        .clone()
        .unwrap_or_else(|| "No position".to_string())
}

/// A seat state as the badge word shown in the table: Planned, Open, Filled or Closed.
fn state_word(state: &str) -> String {
    let mut chars = state.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A budget's used and cap in one cell, for example "60000.00 / 200000.00 GBP", or "No budget".
fn budget_cell(b: &TeamBudget) -> String {
    match (&b.cap, &b.currency) {
        (Some(cap), Some(currency)) => format!("{} / {cap} {currency}", b.used),
        _ => "No budget".to_string(),
    }
}

/// A salary range for the seats of one position, as "min-max CUR", or "None" when no seat has a salary.
fn salary_cell(seats: &[&Seat]) -> String {
    let mins = seats
        .iter()
        .filter_map(|s| s.salary_min.as_deref()?.parse::<f64>().ok());
    let maxes = seats
        .iter()
        .filter_map(|s| s.salary_max.as_deref()?.parse::<f64>().ok());
    let low = mins.clone().reduce(f64::min);
    let high = maxes.clone().reduce(f64::max);
    let (low, high) = match (low, high) {
        (Some(lo), Some(hi)) => (lo, hi),
        (Some(lo), None) => (lo, lo),
        (None, Some(hi)) => (hi, hi),
        (None, None) => return "None".to_string(),
    };
    let currency = seats
        .iter()
        .find_map(|s| s.currency.as_deref())
        .unwrap_or("");
    format!("{low:.2}-{high:.2} {currency}")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_proto::people::{Department, Position, SEAT_OPEN, Seat, Team, TeamBudget};

    #[test]
    fn seat_states_read_as_badge_words() {
        assert_eq!(state_word(SEAT_PLANNED), "Planned");
        assert_eq!(state_word(SEAT_OPEN), "Open");
        assert_eq!(state_word(SEAT_FILLED), "Filled");
        assert_eq!(state_word(SEAT_CLOSED), "Closed");
        assert_eq!(state_word(""), "");
    }

    #[test]
    fn salary_range_comes_from_the_seats_of_a_position() {
        let seat = |min: Option<&str>, max: Option<&str>| Seat {
            id: String::new(),
            team_id: String::new(),
            team_name: String::new(),
            job_profile_id: None,
            job_profile_title: None,
            level: String::new(),
            state: SEAT_OPEN.into(),
            person_id: None,
            person_name: None,
            reports_to_seat_id: None,
            salary_min: min.map(str::to_string),
            salary_max: max.map(str::to_string),
            currency: Some("GBP".into()),
        };
        let a = seat(Some("40000.00"), Some("52000.00"));
        let b = seat(Some("36000.00"), Some("60000.00"));
        let none = seat(None, None);
        assert_eq!(salary_cell(&[&a, &b, &none]), "36000.00-60000.00 GBP");
        assert_eq!(salary_cell(&[&none]), "None");
    }

    #[test]
    fn tables_have_the_columns_and_rows_of_each_tab() {
        let r = Reference {
            departments: vec![Department {
                id: "d1".into(),
                name: "Art".into(),
                parent_department_id: None,
                head_id: None,
                member_count: 2,
            }],
            ..Reference::default()
        };
        let table = table_of(&r, 0, "d1");
        let titles: Vec<&str> = table.columns.iter().map(|c| c.0).collect();
        assert_eq!(titles, ["Name", "Parent", "Head", "Members"]);
        assert_eq!(table.rows.len(), 1);
        assert!(table.rows[0].1);
        assert_eq!(table.rows[0].2, ["Art", "None", "None", "2"]);
        assert_eq!(table_of(&r, 1, "").columns.len(), 5, "teams: five columns");
        assert_eq!(
            table_of(&r, 2, "").columns.len(),
            4,
            "positions: four columns"
        );
        assert_eq!(table_of(&r, 3, "").columns.len(), 5, "seats: five columns");
    }

    #[test]
    fn position_levels_map_presets_and_old_text_to_none() {
        let options = level_choices();
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            ["None", "Junior", "Associate", "Senior", "Lead", "Director"]
        );
        assert_eq!(options[0].value, "", "None is saved as empty");
        assert_eq!(options[3].value, "Senior", "a preset's value is its text");
        assert_eq!(options[4].value, "Lead", "a preset's value is its text");
        assert_eq!(level_choice("Associate"), "Associate");
        assert_eq!(level_choice("Senior"), "Senior");
        assert_eq!(level_choice(""), "", "no level shows None");
        assert_eq!(level_choice("Wizard"), "", "old free text shows None");
        assert_eq!(level_choice("senior"), "", "presets match exactly");
    }

    #[test]
    fn rows_follow_the_tab_and_mark_the_selection() {
        let r = Reference {
            departments: vec![Department {
                id: "d1".into(),
                name: "Art".into(),
                parent_department_id: None,
                head_id: None,
                member_count: 2,
            }],
            teams: vec![Team {
                id: "t1".into(),
                name: "Gameplay".into(),
                department_id: "d1".into(),
                lead_id: None,
                member_count: 3,
            }],
            positions: vec![Position {
                id: "p1".into(),
                title: "Artist".into(),
                level: String::new(),
                description: String::new(),
                salary_min: None,
                salary_max: None,
                currency: None,
                seat_count: 2,
                filled_count: 1,
                open_count: 1,
            }],
            people: vec![],
            seats: vec![Seat {
                id: "s1".into(),
                team_id: "t1".into(),
                team_name: "Gameplay".into(),
                job_profile_id: Some("p1".into()),
                job_profile_title: Some("Artist".into()),
                level: String::new(),
                salary_min: None,
                salary_max: None,
                currency: None,
                state: SEAT_OPEN.into(),
                person_id: None,
                person_name: None,
                reports_to_seat_id: None,
            }],
            budgets: vec![TeamBudget {
                team_id: "t1".into(),
                team_name: "Gameplay".into(),
                cap: Some("200000.00".into()),
                currency: Some("GBP".into()),
                used: "60000.00".into(),
                remaining: Some("140000.00".into()),
            }],
        };
        let seats = rows_of(&r, 3, "");
        assert_eq!(seats[0].title, "Artist", "a seat's title is its position's");
        assert_eq!(seats[0].detail, "Gameplay · No level · open · vacant");
        assert_eq!(
            rows_of(&r, 4, "t1")[0].detail,
            "GBP 200000.00 cap · 60000.00 used · 140000.00 left"
        );
        assert!(rows_of(&r, 4, "t1")[0].selected);
        assert!(!tab_is_empty(&r, 3));
        let teams = rows_of(&r, 1, "t1");
        assert_eq!(
            teams[0].detail,
            "Art · 3 people · GBP 200000.00 cap · 60000.00 used · 140000.00 left"
        );
        assert!(teams[0].selected);
        let positions = rows_of(&r, 2, "");
        assert_eq!(positions[0].title, "Artist");
        assert_eq!(positions[0].detail, "No level · 1 filled · 1 open");
        assert_eq!(rows_of(&r, 0, "")[0].title, "Art");
        assert!(tab_is_empty(&Reference::default(), 0));
    }
}
