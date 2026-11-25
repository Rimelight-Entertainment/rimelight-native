//! Company > Locations: the company's offices, sites and remote hubs at `/company/locations`, on the page data pattern
//! (`space_page_kit::page_data`). Every signed-in person sees the table (name, kind, city, country, timezone). Only an admin
//! (`is_admin`, the same check as the admin pages) gets New, edit and delete: for anyone else the Rust side ignores those
//! requests and the form never opens. A save or delete that succeeds updates the table at once and closes the form; the
//! success is a toast. The page's markup is `ui/locations.slint`, whose `LocationsState` global holds the data.
//!
//! This module is plain Rust. The window side is [`LocationsWindow`], which `wire_company!` implements for the generated window.

use std::sync::{Arc, Mutex};

use rimelight_ui_kit::toast::{self, Tone};
use slint::Weak;
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::is_admin;
use space_page_kit::picker::{Choice, choice};
use space_proto::people::{LOCATION_KINDS, Location, LocationInput, PersonProfile};

/// The locations page's route.
pub const ROUTE: &str = "/company/locations";

/// What one load brings back: whether the caller is an admin, and every location.
pub type Fetched = (bool, Vec<Location>);

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    /// Bumped by every write. A write's result for an older number is dropped.
    writes: u64,
    /// True once a load has succeeded.
    loaded: bool,
    /// The text of the last failed load. `None` when there is none.
    error: Option<String>,
    can_manage: bool,
    locations: Vec<Location>,
    /// The id of the location in the form (empty for a new one).
    selected: String,
}

/// The window side of the locations page. `wire_company!` implements it for the generated window.
pub trait LocationsWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_can_manage(&self, on: bool);
    fn show_selected(&self, id: &str);
    /// The table: its columns (title and width) and its rows (id, selected, cells), in order.
    fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>);
    fn show_kind_options(&self, options: Vec<Choice>);
    /// Connects the kind picker's search callbacks (the Rust side of the select).
    fn wire_kind_picker(&self);
    /// Fills the form's fields and title, and clears its error and the delete confirmation.
    fn show_form(&self, fields: &Fields, title: &str);
    fn show_form_open(&self, open: bool);
    fn show_form_error(&self, text: &str);
    fn show_confirming(&self, on: bool);
    /// The form's fields as the user has them.
    fn read_form(&self) -> Fields;
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_pick(&self, f: impl Fn(String) + 'static);
    fn on_new_item(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn() + 'static);
    fn on_delete_item(&self, f: impl Fn() + 'static);
}

/// The locations page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Locations<W: LocationsWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: LocationsWindow> Clone for Locations<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// What a request needs to reach the window and the session, cloned into each worker's callbacks.
struct Hub<W: LocationsWindow> {
    ui: Weak<W>,
    shared: Arc<Mutex<Shared>>,
    source: Option<Arc<DataSource>>,
}

impl<W: LocationsWindow> Clone for Hub<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            shared: self.shared.clone(),
            source: self.source.clone(),
        }
    }
}

/// The form's fields for one location.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub kind: String,
    pub address: String,
    pub city: String,
    pub country: String,
    pub timezone: String,
    pub note: String,
}

impl Fields {
    fn of(location: &Location) -> Self {
        Self {
            name: location.name.clone(),
            kind: location.kind.clone(),
            address: location.address.clone(),
            city: location.city.clone(),
            country: location.country.clone(),
            timezone: location.timezone.clone(),
            note: location.note.clone(),
        }
    }

    /// The fields as a create or update body. Every field is sent, so an update keeps the form's values.
    fn input(&self) -> LocationInput {
        LocationInput {
            name: Some(self.name.clone()),
            kind: Some(self.kind.clone()),
            address: Some(self.address.clone()),
            city: Some(self.city.clone()),
            country: Some(self.country.clone()),
            timezone: Some(self.timezone.clone()),
            note: Some(self.note.clone()),
        }
    }
}

/// Wires the page's callbacks on the window. `data` is the slot the session driver fills; until it is set, requests do nothing
/// and the page stays in its loading state.
pub fn wire<W: LocationsWindow>(ui: &W, data: DataCell) -> Locations<W> {
    let page = Locations {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };
    // The kind picker needs the SelectNav callbacks (the index of the chosen value, keys), or it shows the first option.
    ui.wire_kind_picker();
    ui.show_kind_options(vec![
        choice(LOCATION_KINDS[0], "Office"),
        choice(LOCATION_KINDS[1], "Site"),
        choice(LOCATION_KINDS[2], "Remote hub"),
    ]);

    let target = page.clone();
    ui.on_opened(move || target.load());

    let target = page.clone();
    ui.on_retry(move || target.load());

    let target = page.clone();
    ui.on_pick(move |id| target.pick(id.as_str()));

    let target = page.clone();
    ui.on_new_item(move || target.new_item());

    let target = page.clone();
    ui.on_save(move || target.save());

    let target = page.clone();
    ui.on_delete_item(move || target.delete());

    page
}

impl<W: LocationsWindow> Locations<W> {
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

    /// The id of the location in the form (empty for none).
    pub fn selected(&self) -> String {
        lock(&self.shared).selected.clone()
    }

    /// Delivers a load's result, as the worker would. For tests: no source is set, so nothing is fetched.
    pub fn deliver(&self, generation: u64, result: Result<Fetched, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_load(&ui, &self.shared, generation, result);
        }
    }

    /// Delivers a save's result for the write with `write`, as the worker would. For tests.
    pub fn deliver_save(&self, write: u64, result: Result<Location, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_save(&ui, &self.shared, write, result);
        }
    }

    /// Loads the locations and the caller's admin flag. Every load starts from the loading state.
    fn load(&self) {
        let hub = self.hub();
        let generation = {
            let mut s = lock(&self.shared);
            s.generation += 1;
            s.loaded = false;
            s.error = None;
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
            move |api, token| async move { fetch_all(&api, &token).await },
            move |ui, result| apply_load(ui, &done.shared, generation, result),
        );
    }

    /// Opens the form for a location. Admins only.
    fn pick(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        if !s.can_manage {
            return;
        }
        let Some(location) = s.locations.iter().find(|l| l.id == id).cloned() else {
            return;
        };
        s.selected = location.id.clone();
        show(&ui, &s);
        ui.show_form(&Fields::of(&location), "Edit location");
        ui.show_form_open(true);
    }

    /// Opens an empty form for a new location. Admins only.
    fn new_item(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        if !s.can_manage {
            return;
        }
        s.selected = String::new();
        show(&ui, &s);
        let fields = Fields {
            kind: LOCATION_KINDS[0].to_string(),
            ..Fields::default()
        };
        ui.show_form(&fields, "New location");
        ui.show_form_open(true);
    }

    /// Creates the location in the form, or saves the selected one. Admins only.
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.read_form();
        let fields = Fields {
            name: form.name.trim().to_string(),
            kind: form.kind.trim().to_string(),
            address: form.address.trim().to_string(),
            city: form.city.trim().to_string(),
            country: form.country.trim().to_string(),
            timezone: form.timezone.trim().to_string(),
            note: form.note.trim().to_string(),
        };
        let id = {
            let s = lock(&self.shared);
            if !s.can_manage {
                return;
            }
            (!s.selected.is_empty()).then(|| s.selected.clone())
        };
        if fields.name.is_empty() {
            return ui.show_form_error("Enter a name.");
        }
        if !LOCATION_KINDS.contains(&fields.kind.as_str()) {
            return ui.show_form_error("Choose a kind.");
        }
        ui.show_form_error("");
        let hub = self.hub();
        let write = begin_write(&hub.shared);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let done = hub.clone();
        let input = fields.input();
        source.fetch(
            &hub.ui,
            move |api, token| async move { save_location(&api, &token, &input, id.as_deref()).await },
            move |ui, result| apply_save(ui, &done.shared, write, result),
        );
    }

    /// Deletes the selected location. Admins only. The form asks for a confirmation first (the window's `confirming`).
    fn delete(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let id = {
            let s = lock(&self.shared);
            if !s.can_manage || s.selected.is_empty() {
                return;
            }
            s.selected.clone()
        };
        ui.show_form_error("");
        let hub = self.hub();
        let write = begin_write(&hub.shared);
        let Some(source) = hub.source.clone() else {
            return;
        };
        let done = hub.clone();
        let work_id = id.clone();
        source.fetch(
            &hub.ui,
            move |api, token| async move { api.delete_location(&token, &work_id).await },
            move |ui, result| apply_delete(ui, &done.shared, write, &id, result),
        );
    }
}

/// Loads whether the caller is an admin, then the locations. Runs on a worker.
async fn fetch_all(api: &ApiClient, token: &str) -> Result<Fetched, ClientError> {
    let me: PersonProfile = api.my_profile(token).await?;
    let locations = api.locations(token).await?;
    Ok((is_admin(&me), locations))
}

/// Creates the location, or updates it when `id` is set. Runs on a worker.
async fn save_location(
    api: &ApiClient,
    token: &str,
    input: &LocationInput,
    id: Option<&str>,
) -> Result<Location, ClientError> {
    match id {
        Some(id) => api.update_location(token, id, input).await,
        None => api.create_location(token, input).await,
    }
}

/// Takes a write number for a save or delete. Its result is kept only while it is the latest write.
fn begin_write(shared: &Mutex<Shared>) -> u64 {
    let mut s = lock(shared);
    s.writes += 1;
    s.writes
}

/// Keeps a load's result if it is for the latest load, then shows the table or the error.
fn apply_load<W: LocationsWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Fetched, ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    match result {
        Ok((can_manage, locations)) => {
            s.can_manage = can_manage;
            s.locations = locations;
            s.loaded = true;
            s.error = None;
        }
        Err(error) => {
            s.loaded = false;
            s.locations.clear();
            s.error = Some(page_data::error_text(&error));
        }
    }
    show(ui, &s);
}

/// Keeps a save's result if it is for the latest write. A saved location replaces its row (or joins the list), the form
/// closes, and a toast says so. A refusal shows in the form.
fn apply_save<W: LocationsWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    write: u64,
    result: Result<Location, ClientError>,
) {
    let mut s = lock(shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(location) => {
            let created = s.selected.is_empty();
            match s.locations.iter().position(|l| l.id == location.id) {
                Some(i) => s.locations[i] = location,
                None => s.locations.push(location),
            }
            s.selected = String::new();
            show(ui, &s);
            ui.show_form_open(false);
            toast::announce(
                Tone::Success,
                if created {
                    "Location created"
                } else {
                    "Location saved"
                },
            );
        }
        Err(error) => ui.show_form_error(&page_data::error_text(&error)),
    }
}

/// Keeps a delete's result if it is for the latest write. A deleted location leaves the table, the form closes, and a toast
/// says so. A refusal shows in the form, which stays open.
fn apply_delete<W: LocationsWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    write: u64,
    id: &str,
    result: Result<(), ClientError>,
) {
    let mut s = lock(shared);
    if s.writes != write {
        return;
    }
    match result {
        Ok(()) => {
            s.locations.retain(|l| l.id != id);
            s.selected = String::new();
            show(ui, &s);
            ui.show_form_open(false);
            toast::announce(Tone::Success, "Location deleted");
        }
        Err(error) => {
            ui.show_confirming(false);
            ui.show_form_error(&page_data::error_text(&error));
        }
    }
}

/// The word a location kind goes by in the table.
fn kind_label(kind: &str) -> String {
    match kind {
        space_proto::people::LOCATION_OFFICE => "Office".to_string(),
        space_proto::people::LOCATION_SITE => "Site".to_string(),
        space_proto::people::LOCATION_REMOTE_HUB => "Remote hub".to_string(),
        other => other.to_string(),
    }
}

/// Draws the page from the request state: the load state, the admin flag, the table and the selection.
fn show<W: LocationsWindow>(ui: &W, s: &Shared) {
    let state = match (&s.error, s.loaded) {
        (Some(_), _) => LoadState::Error,
        (None, true) if s.locations.is_empty() => LoadState::Empty,
        (None, true) => LoadState::Loaded,
        (None, false) => LoadState::Loading,
    };
    ui.show_state(state);
    ui.show_error(s.error.as_deref().unwrap_or_default());
    ui.show_can_manage(s.can_manage);
    ui.show_selected(&s.selected);

    let rows = s
        .locations
        .iter()
        .map(|l| {
            (
                l.id.clone(),
                l.id == s.selected,
                vec![
                    l.name.clone(),
                    kind_label(&l.kind),
                    l.city.clone(),
                    l.country.clone(),
                    l.timezone.clone(),
                ],
            )
        })
        .collect();
    ui.show_table(
        &[
            ("Name", 280.0),
            ("Kind", 140.0),
            ("City", 180.0),
            ("Country", 160.0),
            ("Timezone", 200.0),
        ],
        rows,
    );
}
