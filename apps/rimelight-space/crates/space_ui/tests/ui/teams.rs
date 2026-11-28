//! Company > Teams (SP.3d). The team list and the org chart on the real window, with results injected through the page's handle
//! (no session is set, so nothing is fetched): the loading, loaded, empty and error states with Try again, the org chart's
//! arrows (expand and collapse), Find person (the path opens and the match is highlighted), and the row navigation.

use std::time::Duration;

use slint::{ComponentHandle as _, Model};
use space_client::error::ClientError;
use space_proto::people::{Department, OrgTreeEntry, SEAT_FILLED, Seat, SeatNode, Team};
use space_ui::slint_ui::{PageLoadState, TeamsTable, app_shell};
use space_ui::theme::ThemeMode;

const CEO: &str = "f0000000-0000-4000-8000-000000000001";
const ANA: &str = "f0000000-0000-4000-8000-000000000002";
const BEN: &str = "f0000000-0000-4000-8000-000000000003";
const CAT: &str = "f0000000-0000-4000-8000-000000000004";
const DANA: &str = "f0000000-0000-4000-8000-000000000005";
const EVE: &str = "f0000000-0000-4000-8000-000000000006";
const TEAM: &str = "7e000000-0000-4000-8000-000000000001";
const DEPT: &str = "d0000000-0000-4000-8000-000000000001";

fn person(id: &str, name: &str, manager: Option<&str>) -> OrgTreeEntry {
    OrgTreeEntry {
        id: id.to_string(),
        name: name.to_string(),
        title: Some("Developer".to_string()),
        avatar_url: None,
        manager_id: manager.map(str::to_string),
        team: Some("Gameplay".to_string()),
    }
}

fn team(lead: Option<&str>) -> Team {
    Team {
        id: TEAM.to_string(),
        name: "Gameplay".to_string(),
        department_id: DEPT.to_string(),
        lead_id: lead.map(str::to_string),
        member_count: 3,
    }
}

fn department() -> Department {
    Department {
        id: DEPT.to_string(),
        name: "Engineering".to_string(),
        parent_department_id: None,
        head_id: None,
        member_count: 5,
    }
}

/// CEO, then Ana under the CEO, then Ben and Eve under Ana, then Cat under Ben, then Dana under Cat.
fn company() -> Vec<OrgTreeEntry> {
    vec![
        person(CEO, "Chief Officer", None),
        person(ANA, "Ana Lima", Some(CEO)),
        person(BEN, "Ben Brook", Some(ANA)),
        person(EVE, "Eve Stone", Some(ANA)),
        person(CAT, "Cat Ortiz", Some(BEN)),
        person(DANA, "Dana Doe", Some(CAT)),
    ]
}

/// The seats of the company: each person fills a filled seat under their manager's seat.
fn chart(people: &[OrgTreeEntry]) -> Vec<SeatNode> {
    fn node(people: &[OrgTreeEntry], p: &OrgTreeEntry) -> SeatNode {
        SeatNode {
            seat: Seat {
                id: format!("seat-{}", p.id),
                team_id: TEAM.to_string(),
                team_name: "Gameplay".to_string(),
                job_profile_id: None,
                job_profile_title: p.title.clone(),
                level: String::new(),
                state: SEAT_FILLED.to_string(),
                person_id: Some(p.id.clone()),
                person_name: Some(p.name.clone()),
                reports_to_seat_id: p.manager_id.as_ref().map(|m| format!("seat-{m}")),
                salary_min: None,
                salary_max: None,
                currency: None,
            },
            reports: people
                .iter()
                .filter(|q| q.manager_id.as_deref() == Some(p.id.as_str()))
                .map(|q| node(people, q))
                .collect(),
        }
    }
    people
        .iter()
        .filter(|p| p.manager_id.is_none())
        .map(|p| node(people, p))
        .collect()
}

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

/// The cell texts of row `i` of a table's rows.
fn cells(rows: &slint::ModelRc<space_ui::slint_ui::TableRow>, i: usize) -> Vec<String> {
    let row = rows.row_data(i).expect("row");
    (0..row.cells.row_count())
        .map(|c| row.cells.row_data(c).expect("cell").to_string())
        .collect()
}

/// The names of the org chart rows on show, in order.
fn org_names(ui: &space_ui::slint_ui::AppWindow) -> Vec<String> {
    let rows = ui.get_teams_org_rows();
    (0..rows.row_count())
        .map(|i| rows.row_data(i).expect("row").name.to_string())
        .collect()
}

pub fn teams_list_states_and_team_rows() {
    crate::support::init();
    let app = app_shell::build("/company/teams", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;

    // Loading: the lists have not come back.
    assert_eq!(ui.get_teams_state(), PageLoadState::Loading);

    // Loaded: the team row carries its department, lead and member count.
    let fetched = (
        vec![team(Some(ANA))],
        vec![department()],
        company(),
        chart(&company()),
    );
    app.teams.deliver(app.teams.generation(), Ok(fetched));
    assert_eq!(ui.get_teams_state(), PageLoadState::Loaded);
    let table = ui.global::<TeamsTable>();
    assert_eq!(table.get_rows().row_count(), 1);
    assert_eq!(table.get_ids().row_data(0).expect("id").as_str(), TEAM);
    assert_eq!(
        cells(&table.get_rows(), 0),
        vec!["Gameplay", "Engineering", "Ana Lima", "3"]
    );

    // A team with no lead says so in its Lead cell, and an unknown department is empty.
    app.teams.deliver(
        app.teams.generation(),
        Ok((vec![team(None)], vec![], company(), chart(&company()))),
    );
    assert_eq!(cells(&table.get_rows(), 0)[1..3], ["", "No lead yet"]);

    // Empty: no teams and nobody in the org.
    app.teams
        .deliver(app.teams.generation(), Ok((vec![], vec![], vec![], vec![])));
    assert_eq!(ui.get_teams_state(), PageLoadState::Empty);
    assert_eq!(table.get_rows().row_count(), 0);

    // Error, then Try again: the error says what happened, and Try again loads again.
    app.teams.deliver(
        app.teams.generation(),
        Err(ClientError::Network("refused".to_string())),
    );
    assert_eq!(ui.get_teams_state(), PageLoadState::Error);
    assert!(
        ui.get_teams_error()
            .as_str()
            .contains("could not be reached")
    );
    let failed = app.teams.generation();
    ui.invoke_teams_retry();
    assert_eq!(ui.get_teams_state(), PageLoadState::Loading);
    assert_eq!(ui.get_teams_error(), "", "a retry clears the error");
    assert_eq!(app.teams.generation(), failed + 1);

    // A late answer to the request before the retry is dropped.
    app.teams
        .deliver(failed, Ok((vec![team(None)], vec![], vec![], vec![])));
    assert_eq!(
        ui.get_teams_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );
}

pub fn org_chart_arrows_and_find_person() {
    crate::support::init();
    let app = app_shell::build("/company/teams", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.teams.deliver(
        app.teams.generation(),
        Ok((vec![], vec![], company(), chart(&company()))),
    );
    assert_eq!(
        ui.get_teams_state(),
        PageLoadState::Loaded,
        "people alone make the page loaded"
    );

    // The root and its children start open, so Ben (two levels down) is listed but closed.
    assert_eq!(
        org_names(ui),
        vec!["Chief Officer", "Ana Lima", "Ben Brook", "Eve Stone"]
    );
    let ben = ui.get_teams_org_rows().row_data(2).expect("Ben");
    assert_eq!(ben.depth, 2);
    assert_eq!(ben.reports, 1);
    assert!(ben.expandable && !ben.expanded);

    // The arrow opens Ben: Cat appears one level deeper. Pressing it again closes him.
    ui.invoke_teams_toggle(format!("seat-{BEN}").into());
    assert_eq!(
        org_names(ui),
        vec![
            "Chief Officer",
            "Ana Lima",
            "Ben Brook",
            "Cat Ortiz",
            "Eve Stone"
        ]
    );
    assert_eq!(ui.get_teams_org_rows().row_data(3).expect("Cat").depth, 3);
    ui.invoke_teams_toggle(format!("seat-{BEN}").into());
    assert_eq!(org_names(ui).len(), 4);

    // Find person: the path to Dana opens (Ben and Cat), Dana is highlighted, and the note says so.
    ui.invoke_teams_find("dana".into());
    assert_eq!(
        org_names(ui),
        vec![
            "Chief Officer",
            "Ana Lima",
            "Ben Brook",
            "Cat Ortiz",
            "Dana Doe",
            "Eve Stone"
        ]
    );
    let rows = ui.get_teams_org_rows();
    let highlighted: Vec<String> = (0..rows.row_count())
        .filter_map(|i| rows.row_data(i))
        .filter(|row| row.highlighted)
        .map(|row| row.name.to_string())
        .collect();
    assert_eq!(highlighted, vec!["Dana Doe"]);
    assert_eq!(ui.get_teams_find_note(), "Showing Dana Doe");

    // A search with no match highlights nobody and says so. An empty box clears the note.
    ui.invoke_teams_find("zed".into());
    assert_eq!(ui.get_teams_find_note(), "No one matches.");
    assert!(
        (0..ui.get_teams_org_rows().row_count())
            .filter_map(|i| ui.get_teams_org_rows().row_data(i))
            .all(|row| !row.highlighted)
    );
    ui.invoke_teams_find("".into());
    assert_eq!(ui.get_teams_find_note(), "");

    // The name opens that person's profile (the same tab, as the employees list does).
    ui.invoke_teams_open_person(EVE.into());
    assert_eq!(ui.get_page_route(), format!("/company/employees/{EVE}"));
}

pub fn teams_rows_open_the_team_route() {
    crate::support::init();
    let app = app_shell::build("/company/teams", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.teams.deliver(
        app.teams.generation(),
        Ok((
            vec![team(Some(ANA))],
            vec![department()],
            company(),
            chart(&company()),
        )),
    );
    ui.invoke_teams_open_team(TEAM.into());
    assert_eq!(ui.get_page_route(), format!("/company/teams/{TEAM}"));
}
