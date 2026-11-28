//! Company > Departments (SP.3d). The department tree on the real window, with results injected through the page's handle (no
//! session): the states and Try again, the tree's arrows, selecting a department (its head and its teams), and the rows that
//! open a team or a head's profile.

use std::time::Duration;

use slint::{ComponentHandle as _, Model};
use space_client::error::ClientError;
use space_proto::people::{Department, OrgTreeEntry, Team};
use space_ui::slint_ui::{DeptsTable, PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const ANA: &str = "f0000000-0000-4000-8000-000000000002";
const ENG: &str = "d0000000-0000-4000-8000-000000000001";
const GAME: &str = "d0000000-0000-4000-8000-000000000002";
const ART: &str = "d0000000-0000-4000-8000-000000000003";
const TEAM: &str = "7e000000-0000-4000-8000-000000000001";

fn department(
    id: &str,
    name: &str,
    parent: Option<&str>,
    head: Option<&str>,
    members: u32,
) -> Department {
    Department {
        id: id.to_string(),
        name: name.to_string(),
        parent_department_id: parent.map(str::to_string),
        head_id: head.map(str::to_string),
        member_count: members,
    }
}

/// Engineering (headed by Ana) with Game under it; Art is a top-level department with no head.
fn departments() -> Vec<Department> {
    vec![
        department(ENG, "Engineering", None, Some(ANA), 9),
        department(GAME, "Game", Some(ENG), None, 4),
        department(ART, "Art", None, None, 2),
    ]
}

fn teams() -> Vec<Team> {
    vec![Team {
        id: TEAM.to_string(),
        name: "Gameplay".to_string(),
        department_id: ENG.to_string(),
        lead_id: Some(ANA.to_string()),
        member_count: 3,
    }]
}

fn people() -> Vec<OrgTreeEntry> {
    vec![OrgTreeEntry {
        id: ANA.to_string(),
        name: "Ana Lima".to_string(),
        title: None,
        avatar_url: None,
        manager_id: None,
        team: None,
    }]
}

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

/// The cell texts of row `i` of the departments table.
fn cells(ui: &space_ui::slint_ui::AppWindow, i: usize) -> Vec<String> {
    let row = ui
        .global::<DeptsTable>()
        .get_rows()
        .row_data(i)
        .expect("row");
    (0..row.cells.row_count())
        .map(|c| row.cells.row_data(c).expect("cell").to_string())
        .collect()
}

/// The department names on show, in order: the indent and the "+" or "-" marker are left out.
fn names(ui: &space_ui::slint_ui::AppWindow) -> Vec<String> {
    let rows = ui.global::<DeptsTable>().get_rows();
    (0..rows.row_count())
        .map(|i| {
            cells(ui, i)[0]
                .trim_start()
                .trim_start_matches(['+', '-'])
                .trim_start()
                .to_string()
        })
        .collect()
}

pub fn departments_states_and_retry() {
    crate::support::init();
    let app = app_shell::build("/company/departments", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    assert_eq!(ui.get_dept_state(), PageLoadState::Loading);

    // Loaded: the top-level departments and the first level under them show; Game is under Engineering.
    app.departments.deliver(
        app.departments.generation(),
        Ok((departments(), teams(), people())),
    );
    assert_eq!(ui.get_dept_state(), PageLoadState::Loaded);
    assert_eq!(names(ui), vec!["Engineering", "Game", "Art"]);
    assert_eq!(cells(ui, 0)[1..], ["Ana Lima", "9"]);
    assert!(cells(ui, 0)[0].starts_with("- "), "two levels start open");
    assert_eq!(cells(ui, 2)[1], "No head set", "no head says so");

    // Empty: no departments.
    app.departments
        .deliver(app.departments.generation(), Ok((vec![], vec![], vec![])));
    assert_eq!(ui.get_dept_state(), PageLoadState::Empty);

    // Error, then Try again.
    app.departments.deliver(
        app.departments.generation(),
        Err(ClientError::Network("refused".to_string())),
    );
    assert_eq!(ui.get_dept_state(), PageLoadState::Error);
    let failed = app.departments.generation();
    ui.invoke_dept_retry();
    assert_eq!(ui.get_dept_state(), PageLoadState::Loading);
    assert_eq!(app.departments.generation(), failed + 1);
    app.departments
        .deliver(failed, Ok((vec![], vec![], vec![])));
    assert_eq!(
        ui.get_dept_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );
}

pub fn departments_arrows_and_selection_show_head_and_teams() {
    crate::support::init();
    let app = app_shell::build("/company/departments", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.departments.deliver(
        app.departments.generation(),
        Ok((departments(), teams(), people())),
    );

    // Nothing is selected at first.
    assert_eq!(ui.get_dept_view().id, "", "no selection shows no detail");

    // The arrow closes Engineering: Game goes out of the list.
    ui.invoke_dept_toggle(ENG.into());
    assert_eq!(names(ui), vec!["Engineering", "Art"]);
    ui.invoke_dept_toggle(ENG.into());
    assert_eq!(names(ui), vec!["Engineering", "Game", "Art"]);

    // Selecting Engineering shows its head and its teams.
    ui.invoke_dept_select(ENG.into());
    let view = ui.get_dept_view();
    assert_eq!(view.id, ENG);
    assert_eq!(view.name, "Engineering");
    assert_eq!(view.head_id, ANA);
    assert_eq!(view.head_name, "Ana Lima");
    assert_eq!(view.members, 9);
    assert_eq!(view.teams.row_count(), 1);
    let team = view.teams.row_data(0).expect("team");
    assert_eq!(team.name, "Gameplay");
    assert_eq!(team.department, "Engineering");
    assert_eq!(team.lead_name, "Ana Lima");
    assert_eq!(app.departments.selected(), ENG);

    // Art has no head and no teams.
    ui.invoke_dept_select(ART.into());
    let view = ui.get_dept_view();
    assert_eq!(view.head_id, "", "no head");
    assert_eq!(view.teams.row_count(), 0);
}

pub fn departments_team_and_head_rows_open_their_pages() {
    crate::support::init();
    let app = app_shell::build("/company/departments", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.departments.deliver(
        app.departments.generation(),
        Ok((departments(), teams(), people())),
    );
    ui.invoke_dept_select(ENG.into());

    ui.invoke_dept_open_team(TEAM.into());
    assert_eq!(ui.get_page_route(), format!("/company/teams/{TEAM}"));
}

/// The page body fills the remaining height of its tile group. A page root is a Rectangle, and its layout must size to it,
/// or the body shrinks to its content (the owner's report, 2026-11-15: about 100 px tall on a 1200 by 800 window).
pub fn departments_page_body_fills_the_group() {
    use slint::ComponentHandle;

    crate::support::init();
    let app = app_shell::build("/company/departments", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
    wait(10);
    app.departments.deliver(
        app.departments.generation(),
        Ok((departments(), teams(), people())),
    );
    wait(10);

    let height = ui.get_test_dept_body_height();
    assert!(
        height >= 500.0,
        "the page body fills its group (got {height} px)"
    );
}
