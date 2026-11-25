//! Company > Teams > one team (SP.3d). The team page's loading, loaded, error and Try again states on the real window (results
//! injected through the page's handle, no session), a team with no members, and the lead and member rows opening profiles.

use std::time::Duration;

use slint::Model;
use space_client::error::ClientError;
use space_proto::people::{Department, OrgTreeEntry, PersonSummary, Team, TeamDetail};
use space_ui::slint_ui::{PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const ANA: &str = "f0000000-0000-4000-8000-000000000002";
const BEN: &str = "f0000000-0000-4000-8000-000000000003";
const TEAM: &str = "7e000000-0000-4000-8000-000000000001";
const DEPT: &str = "d0000000-0000-4000-8000-000000000001";

fn route() -> String {
    format!("/company/teams/{TEAM}")
}

fn summary(id: &str, name: &str) -> PersonSummary {
    PersonSummary {
        id: id.to_string(),
        display_name: name.to_string(),
        title: Some("Developer".to_string()),
        team: Some("Gameplay".to_string()),
        avatar_url: None,
    }
}

fn detail(members: Vec<PersonSummary>) -> TeamDetail {
    TeamDetail {
        team: Team {
            id: TEAM.to_string(),
            name: "Gameplay".to_string(),
            department_id: DEPT.to_string(),
            lead_id: Some(ANA.to_string()),
            member_count: members.len() as u32,
        },
        members,
    }
}

fn departments() -> Vec<Department> {
    vec![Department {
        id: DEPT.to_string(),
        name: "Engineering".to_string(),
        parent_department_id: None,
        head_id: None,
        member_count: 5,
    }]
}

fn people() -> Vec<OrgTreeEntry> {
    vec![OrgTreeEntry {
        id: ANA.to_string(),
        name: "Ana Lima".to_string(),
        title: None,
        avatar_url: None,
        manager_id: None,
        team: Some("Gameplay".to_string()),
    }]
}

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

pub fn team_page_states_and_retry() {
    crate::support::init();
    let app = app_shell::build(&route(), ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    assert_eq!(ui.get_team_state(), PageLoadState::Loading);

    // Loaded: the header and the members.
    app.team.deliver(
        app.team.generation(),
        Ok((
            detail(vec![summary(ANA, "Ana Lima"), summary(BEN, "Ben Brook")]),
            departments(),
            people(),
        )),
    );
    assert_eq!(ui.get_team_state(), PageLoadState::Loaded);
    let view = ui.get_team_view();
    assert_eq!(view.name, "Gameplay");
    assert_eq!(view.department, "Engineering");
    assert_eq!(view.lead_name, "Ana Lima");
    assert_eq!(view.lead_initials, "AL");
    assert_eq!(view.members, 2);
    let members = ui.get_team_members();
    assert_eq!(members.row_count(), 2);
    assert_eq!(members.row_data(1).expect("Ben").name, "Ben Brook");
    assert_eq!(members.row_data(1).expect("Ben").title, "Developer");

    // Error (the server does not know the team), then Try again.
    app.team.deliver(
        app.team.generation(),
        Err(ClientError::Api {
            status: 404,
            error: "not_found".to_string(),
            message: "There is no such team.".to_string(),
        }),
    );
    assert_eq!(ui.get_team_state(), PageLoadState::Error);
    assert_eq!(ui.get_team_error(), "There is no such team.");
    let failed = app.team.generation();
    ui.invoke_team_retry();
    assert_eq!(ui.get_team_state(), PageLoadState::Loading);
    assert_eq!(app.team.generation(), failed + 1);
    app.team
        .deliver(failed, Ok((detail(vec![]), departments(), people())));
    assert_eq!(
        ui.get_team_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );
}

pub fn team_without_members_is_loaded_and_empty_of_people() {
    crate::support::init();
    let app = app_shell::build(&route(), ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.team.deliver(
        app.team.generation(),
        Ok((detail(vec![]), departments(), people())),
    );
    assert_eq!(ui.get_team_state(), PageLoadState::Loaded);
    assert_eq!(ui.get_team_members().row_count(), 0);
    assert_eq!(ui.get_team_view().members, 0);
}

pub fn team_lead_and_member_rows_open_profiles() {
    crate::support::init();
    let app = app_shell::build(&route(), ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.team.deliver(
        app.team.generation(),
        Ok((
            detail(vec![summary(BEN, "Ben Brook")]),
            departments(),
            people(),
        )),
    );
    ui.invoke_team_open_person(BEN.into());
    assert_eq!(ui.get_page_route(), format!("/company/employees/{BEN}"));
}
