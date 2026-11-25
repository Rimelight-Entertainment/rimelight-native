//! Company > Employees (SP.3b). The page's loading, loaded, empty and error states, Try again, the search delay and the row
//! navigation, on the real window with results injected through the page's handle. No session is set, so nothing is fetched.

use std::time::Duration;

use slint::{ComponentHandle as _, Model};
use space_client::error::ClientError;
use space_proto::people::{Page, PersonSummary};
use space_ui::slint_ui::{EmployeesTable, PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const ANN: &str = "f0000000-0000-4000-8000-000000000001";
const BEN: &str = "f0000000-0000-4000-8000-000000000002";

/// The cell texts of row `i` of a table's rows.
fn cells(rows: &slint::ModelRc<space_ui::slint_ui::TableRow>, i: usize) -> Vec<String> {
    let row = rows.row_data(i).expect("row");
    (0..row.cells.row_count())
        .map(|c| row.cells.row_data(c).expect("cell").to_string())
        .collect()
}

fn person(
    id: &str,
    name: &str,
    title: Option<&str>,
    team: Option<&str>,
    avatar: Option<&str>,
) -> PersonSummary {
    PersonSummary {
        id: id.to_string(),
        display_name: name.to_string(),
        title: title.map(str::to_string),
        team: team.map(str::to_string),
        avatar_url: avatar.map(str::to_string),
    }
}

fn page(items: Vec<PersonSummary>, next_cursor: Option<&str>) -> Page<PersonSummary> {
    Page {
        items,
        next_cursor: next_cursor.map(str::to_string),
    }
}

/// Moves the mock clock on, so the search timer can fire.
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

pub fn employees_page_states_rows_and_retry() {
    crate::support::init();
    let app = app_shell::build("/company/employees", ThemeMode::Light).expect("window builds");
    // The page opens on the event loop (its `init`), so pump it once: its first load runs before the test starts.
    wait(10);
    let ui = &app.ui;

    // Loading: nothing has come back yet.
    assert_eq!(ui.get_employees_state(), PageLoadState::Loading);

    // Loaded: each row carries its name, title, team and initials. A cursor means there is a Load more.
    let first = page(
        vec![
            person(
                ANN,
                "Ann Adams",
                Some("Developer"),
                Some("Gameplay"),
                Some("/v1/avatars/a.png"),
            ),
            person(BEN, "Ben Brook", None, Some("Art"), None),
        ],
        Some("c2"),
    );
    app.employees.deliver(app.employees.generation(), Ok(first));
    assert_eq!(ui.get_employees_state(), PageLoadState::Loaded);
    let table = ui.global::<EmployeesTable>();
    assert_eq!(table.get_rows().row_count(), 2);
    assert_eq!(table.get_ids().row_data(0).expect("id").as_str(), ANN);
    assert_eq!(
        cells(&table.get_rows(), 0),
        vec!["Ann Adams", "Developer", "Gameplay"]
    );
    assert_eq!(
        cells(&table.get_rows(), 1)[1],
        "",
        "a missing title is empty, not a placeholder"
    );
    assert!(ui.get_employees_has_more());

    // Empty: a search that finds nobody.
    app.employees
        .deliver(app.employees.generation(), Ok(page(vec![], None)));
    assert_eq!(ui.get_employees_state(), PageLoadState::Empty);
    assert_eq!(ui.global::<EmployeesTable>().get_rows().row_count(), 0);
    assert!(!ui.get_employees_has_more());

    // Error, then Try again: the error shows its text, Try again starts a new request and shows the loading state.
    app.employees.deliver(
        app.employees.generation(),
        Err(ClientError::Network("refused".to_string())),
    );
    assert_eq!(ui.get_employees_state(), PageLoadState::Error);
    assert!(
        ui.get_employees_error()
            .as_str()
            .contains("could not be reached"),
        "the error line says what happened"
    );
    let failed = app.employees.generation();
    ui.invoke_employees_retry();
    assert_eq!(ui.get_employees_state(), PageLoadState::Loading);
    assert_eq!(ui.get_employees_error(), "", "a retry clears the error");
    assert_eq!(app.employees.generation(), failed + 1);

    // A late answer to the request before the retry is dropped.
    app.employees.deliver(failed, Ok(page(vec![], None)));
    assert_eq!(
        ui.get_employees_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );
}

pub fn employees_search_waits_then_asks_for_a_new_list() {
    crate::support::init();
    let app = app_shell::build("/company/employees", ThemeMode::Light).expect("window builds");
    // The page opens on the event loop (its `init`), so pump it once: its first load runs before the test starts.
    wait(10);
    let ui = &app.ui;
    let list = page(vec![person(ANN, "Ann Adams", None, None, None)], None);
    app.employees.deliver(app.employees.generation(), Ok(list));
    let before = app.employees.generation();

    ui.invoke_employees_search("a".into());
    ui.invoke_employees_search("an".into());
    assert_eq!(
        app.employees.generation(),
        before,
        "no query at the change itself"
    );

    // After the delay, one query runs for the last text.
    wait(1000);
    assert_eq!(app.employees.generation(), before + 1);
    assert_eq!(app.employees.query(), "an");
    assert_eq!(ui.get_employees_state(), PageLoadState::Loading);
    assert_eq!(
        ui.global::<EmployeesTable>().get_rows().row_count(),
        0,
        "the old rows go with the new query"
    );
}

pub fn employees_row_opens_the_person_profile_route() {
    crate::support::init();
    let app = app_shell::build("/company/employees", ThemeMode::Light).expect("window builds");
    // The page opens on the event loop (its `init`), so pump it once: its first load runs before the test starts.
    wait(10);
    let ui = &app.ui;
    app.employees.deliver(
        app.employees.generation(),
        Ok(page(vec![person(ANN, "Ann Adams", None, None, None)], None)),
    );
    ui.invoke_employees_open(ANN.into());
    assert_eq!(ui.get_page_route(), format!("/company/employees/{ANN}"));
}
