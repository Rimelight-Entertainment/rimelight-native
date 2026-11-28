//! Company > Locations. The table on the real window, with results injected through the page's handle (no session): the rows
//! and their states, and that only an admin can open the form or save (a reader's requests do nothing, an admin's save
//! replaces its row).

use std::time::Duration;

use slint::{ComponentHandle as _, Model};
use space_client::error::ClientError;
use space_proto::people::Location;
use space_ui::slint_ui::{LocationsState, PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const OFFICE: &str = "1a000000-0000-4000-8000-000000000001";
const HUB: &str = "1a000000-0000-4000-8000-000000000002";

fn location(id: &str, name: &str, kind: &str, city: &str, country: &str) -> Location {
    Location {
        id: id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        address: "1 Market Street".to_string(),
        city: city.to_string(),
        country: country.to_string(),
        timezone: "Europe/London".to_string(),
        note: String::new(),
    }
}

fn locations() -> Vec<Location> {
    vec![
        location(OFFICE, "London office", "office", "London", "UK"),
        location(HUB, "Northern hub", "remote_hub", "Leeds", "UK"),
    ]
}

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

/// The cell texts of row `i` of the locations table.
fn cells(ui: &space_ui::slint_ui::AppWindow, i: usize) -> Vec<String> {
    let row = ui
        .global::<LocationsState>()
        .get_table_rows()
        .row_data(i)
        .expect("row");
    (0..row.cells.row_count())
        .map(|c| row.cells.row_data(c).expect("cell").to_string())
        .collect()
}

pub fn locations_table_rows_and_states() {
    crate::support::init();
    let app = app_shell::build("/company/locations", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<LocationsState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);

    // Loaded: every signed-in person sees the rows, with the kind word and the timezone.
    app.locations
        .deliver(app.locations.generation(), Ok((false, locations())));
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    assert_eq!(
        cells(ui, 0),
        ["London office", "Office", "London", "UK", "Europe/London"]
    );
    assert_eq!(
        cells(ui, 1)[1],
        "Remote hub",
        "remote hubs have their own word"
    );
    assert!(!state.get_can_manage(), "a reader cannot manage");

    // Empty: no locations.
    app.locations
        .deliver(app.locations.generation(), Ok((false, vec![])));
    assert_eq!(state.get_state(), PageLoadState::Empty);

    // Error.
    app.locations
        .deliver(app.locations.generation(), Err(ClientError::Unauthorized));
    assert_eq!(state.get_state(), PageLoadState::Error);
}

pub fn locations_write_is_admin_only() {
    crate::support::init();
    let app = app_shell::build("/company/locations", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<LocationsState>();

    // A reader: pick and New do nothing, so no form opens.
    app.locations
        .deliver(app.locations.generation(), Ok((false, locations())));
    state.invoke_pick(OFFICE.into());
    state.invoke_new_item();
    assert!(!state.get_form_open(), "a reader gets no form");
    assert_eq!(app.locations.selected(), "");

    // An admin: pick opens the form filled from the row.
    app.locations
        .deliver(app.locations.generation(), Ok((true, locations())));
    assert!(state.get_can_manage());
    state.invoke_pick(OFFICE.into());
    assert!(state.get_form_open());
    assert_eq!(state.get_name().as_str(), "London office");
    assert_eq!(state.get_kind().as_str(), "office");
    assert_eq!(app.locations.selected(), OFFICE);

    // An empty name is refused in the form, which stays open.
    state.set_name("".into());
    state.invoke_save();
    assert_eq!(state.get_form_error().as_str(), "Enter a name.");
    assert!(state.get_form_open());
    assert_eq!(app.locations.write_generation(), 0, "nothing was sent");

    // A save goes out, and its result replaces the row and closes the form.
    state.set_name("London HQ".into());
    state.invoke_save();
    let write = app.locations.write_generation();
    assert_eq!(write, 1);
    app.locations.deliver_save(
        write,
        Ok(location(OFFICE, "London HQ", "office", "London", "UK")),
    );
    assert!(!state.get_form_open());
    assert_eq!(cells(ui, 0)[0], "London HQ");
    assert_eq!(app.locations.selected(), "");

    // New opens an empty form with the kind set to office.
    state.invoke_new_item();
    assert!(state.get_form_open());
    assert_eq!(state.get_name().as_str(), "");
    assert_eq!(state.get_kind().as_str(), "office");
}
