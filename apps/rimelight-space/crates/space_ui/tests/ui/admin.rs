//! Admin area (SP.3e): Admin > Users, Organisation and Roles on the real window, with results injected through each page's handle
//! (no session). Covers the admin gate, the list states, the add form's validation and the server's errors, the detail panel's
//! 422 and 409 errors, the organisation tabs with a delete refused by the server, and role add and remove.

use std::time::Duration;

use rimelight_ui_kit::toast::take_announced;
use slint::{ComponentHandle as _, Model};
use space_area_admin::Reference;
use space_area_admin::organisation::{Row, Saved};
use space_area_admin::roles::Change;
use space_area_admin::users::Written;
use space_client::error::ClientError;
use space_proto::people::{
    Department, OrgTreeEntry, Page, PersonSummary, Position, Profile, RoleMembers, SEAT_OPEN, Seat,
    Team, TeamBudget,
};
use space_ui::slint_ui::{
    AdminOrgState, AdminRolesState, AdminUsersState, PageLoadState, app_shell,
};
use space_ui::theme::ThemeMode;

const USERS: &str = "/admin/users";
const ORGANISATION: &str = "/admin/organisation";
const ROLES: &str = "/admin/roles";

const ADA: &str = "f0000000-0000-4000-8000-000000000002";
const BEN: &str = "f0000000-0000-4000-8000-000000000003";
const ENG: &str = "d0000000-0000-4000-8000-000000000001";
const ART: &str = "d0000000-0000-4000-8000-000000000003";
const TEAM: &str = "7e000000-0000-4000-8000-000000000001";
const POSITION: &str = "90000000-0000-4000-8000-000000000001";

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn api_error(status: u16, error: &str, message: &str) -> ClientError {
    ClientError::Api {
        status,
        error: error.to_string(),
        message: message.to_string(),
    }
}

fn profile(id: &str, name: &str, roles: &[&str], active: bool) -> Profile {
    Profile {
        id: id.to_string(),
        email: format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
        display_name: name.to_string(),
        preferred_name: None,
        pronouns: None,
        title: Some("Engineer".to_string()),
        position_id: Some(POSITION.to_string()),
        department_id: Some(ENG.to_string()),
        department_name: Some("Engineering".to_string()),
        team_id: Some(TEAM.to_string()),
        team_name: Some("Gameplay".to_string()),
        manager_id: None,
        location: None,
        timezone: None,
        work_phone: None,
        bio: None,
        skills: vec![],
        start_date: None,
        avatar_url: None,
        is_active: active,
        roles: roles.iter().map(|r| r.to_string()).collect(),
    }
}

fn page(items: Vec<Profile>) -> Page<Profile> {
    Page {
        items,
        next_cursor: None,
    }
}

fn reference() -> Reference {
    Reference {
        departments: vec![Department {
            id: ENG.to_string(),
            name: "Engineering".to_string(),
            parent_department_id: None,
            head_id: Some(ADA.to_string()),
            member_count: 9,
        }],
        teams: vec![Team {
            id: TEAM.to_string(),
            name: "Gameplay".to_string(),
            department_id: ENG.to_string(),
            lead_id: Some(ADA.to_string()),
            member_count: 3,
        }],
        positions: vec![Position {
            id: POSITION.to_string(),
            title: "Engineer".to_string(),
            level: "Senior".to_string(),
            description: String::new(),
            salary_min: None,
            salary_max: None,
            currency: None,
            seat_count: 2,
            filled_count: 1,
            open_count: 1,
        }],
        people: vec![
            person_entry(ADA, "Ada Lovelace"),
            person_entry(BEN, "Ben Kato"),
        ],
        seats: vec![],
        budgets: vec![],
    }
}

fn person_entry(id: &str, name: &str) -> OrgTreeEntry {
    OrgTreeEntry {
        id: id.to_string(),
        name: name.to_string(),
        title: None,
        avatar_url: None,
        manager_id: None,
        team: None,
    }
}

pub fn admin_users_states_and_the_admin_gate() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<AdminUsersState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);

    // Not an admin: the gate shows and nothing else.
    app.admin_users
        .deliver(app.admin_users.generation(), Ok(None));
    assert!(state.get_denied());

    // An admin with two people, one of them deactivated.
    let generation = app.admin_users.generation();
    app.admin_users.deliver(
        generation,
        Ok(Some((
            page(vec![
                profile(ADA, "Ada Lovelace", &["admin"], true),
                profile(BEN, "Ben Kato", &["hr"], false),
            ]),
            reference(),
        ))),
    );
    assert!(!state.get_denied());
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    let rows = state.get_rows();
    assert_eq!(rows.row_count(), 2);
    let ben = rows.row_data(1).expect("Ben");
    assert_eq!(ben.status, "Deactivated");
    assert_eq!(ben.roles, "HR");
    assert!(!ben.active);
    assert_eq!(
        state.get_position_options().row_count(),
        2,
        "None and one position"
    );

    // Empty, then an error, then Try again.
    app.admin_users
        .deliver(generation, Ok(Some((page(vec![]), reference()))));
    assert_eq!(state.get_state(), PageLoadState::Empty);
    app.admin_users
        .deliver(generation, Err(ClientError::Network("refused".to_string())));
    assert_eq!(state.get_state(), PageLoadState::Error);
    let failed = app.admin_users.generation();
    state.invoke_retry();
    assert_eq!(state.get_state(), PageLoadState::Loading);
    assert_eq!(app.admin_users.generation(), failed + 1);
    app.admin_users
        .deliver(failed, Ok(Some((page(vec![]), reference()))));
    assert_eq!(
        state.get_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );
}

pub fn admin_users_add_validates_then_saves() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<AdminUsersState>();
    let generation = app.admin_users.generation();
    app.admin_users.deliver(
        generation,
        Ok(Some((
            page(vec![profile(ADA, "Ada Lovelace", &["admin"], true)]),
            reference(),
        ))),
    );

    // The form is checked before anything is sent.
    state.set_add_open(true);
    state.invoke_add_submit();
    assert!(state.get_add_error().contains("email"));
    state.set_add_email("ana@example".into());
    state.invoke_add_submit();
    assert!(
        state.get_add_error().contains("email"),
        "no dot in the domain"
    );
    state.set_add_email("ana@example.com".into());
    state.invoke_add_submit();
    assert!(state.get_add_error().contains("display name"));
    state.set_add_name("Ana Lima".into());
    state.set_add_start("15/11/2026".into());
    state.invoke_add_submit();
    assert_eq!(state.get_add_error(), "Start date must be YYYY-MM-DD.");
    assert_eq!(app.admin_users.write_generation(), 0, "nothing was sent");

    // A valid form sends one write. The server's 409 shows in the form and the form stays open.
    state.set_add_start("2026-11-15".into());
    state.invoke_add_submit();
    assert_eq!(state.get_add_error(), "");
    let write = app.admin_users.write_generation();
    assert_eq!(write, 1, "one write for the valid form");
    app.admin_users.deliver_write(
        write,
        Written::Add,
        Err(api_error(
            409,
            "email_taken",
            "That email is already in use.",
        )),
    );
    assert_eq!(state.get_add_error(), "That email is already in use.");
    assert!(state.get_add_open());

    // Success closes the form, clears it and says who was added.
    state.invoke_add_submit();
    let write = app.admin_users.write_generation();
    app.admin_users.deliver_write(
        write,
        Written::Add,
        Ok(profile(
            "f0000000-0000-4000-8000-000000000009",
            "Ana Lima",
            &[],
            true,
        )),
    );
    assert!(!state.get_add_open());
    assert_eq!(state.get_add_email(), "");
    assert_eq!(
        toast_text(),
        "Added Ana Lima. They can sign in with ana.lima@example.com."
    );
}

pub fn admin_users_edit_shows_the_server_errors() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<AdminUsersState>();
    let generation = app.admin_users.generation();
    app.admin_users.deliver(
        generation,
        Ok(Some((
            page(vec![
                profile(ADA, "Ada Lovelace", &["admin"], true),
                profile(BEN, "Ben Kato", &[], true),
            ]),
            reference(),
        ))),
    );

    state.invoke_select(ADA.into());
    assert!(state.get_detail_open());
    assert_eq!(state.get_detail_name(), "Ada Lovelace");
    assert_eq!(state.get_d_department(), ENG);
    assert_eq!(state.get_detail_roles(), "Admin");

    // A bad start date is refused before anything is sent.
    state.set_d_start("next week".into());
    state.invoke_save();
    assert_eq!(state.get_detail_error(), "Start date must be YYYY-MM-DD.");
    assert_eq!(app.admin_users.write_generation(), 0);

    // A manager cycle comes back as 422 and shows in the panel.
    state.set_d_start("2026-11-15".into());
    state.invoke_save();
    let write = app.admin_users.write_generation();
    assert_eq!(write, 1);
    app.admin_users.deliver_write(
        write,
        Written::Edit,
        Err(api_error(
            422,
            "manager_cycle",
            "That manager reports to this person.",
        )),
    );
    assert_eq!(
        state.get_detail_error(),
        "That manager reports to this person."
    );

    // Deactivating yourself is refused with 409.
    state.invoke_set_active(false);
    let write = app.admin_users.write_generation();
    app.admin_users.deliver_write(
        write,
        Written::Active,
        Err(api_error(
            409,
            "self_deactivation",
            "You cannot deactivate your own account.",
        )),
    );
    assert_eq!(
        state.get_detail_error(),
        "You cannot deactivate your own account."
    );

    // Saved: the panel shows the new roles, the error clears, and a toast says what happened.
    let mut saved = profile(ADA, "Ada Lovelace", &["admin"], true);
    saved.start_date = Some("2026-11-15".to_string());
    state.invoke_save();
    let write = app.admin_users.write_generation();
    app.admin_users
        .deliver_write(write, Written::Edit, Ok(saved));
    assert_eq!(state.get_detail_error(), "");
    assert_eq!(toast_text(), "Saved changes to Ada Lovelace.");
    assert_eq!(state.get_d_start(), "2026-11-15");
}

pub fn admin_org_tabs_create_and_the_delete_conflict() {
    crate::support::init();
    let app = app_shell::build(ORGANISATION, ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<AdminOrgState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);

    app.admin_org
        .deliver(app.admin_org.generation(), Ok(Some(reference())));
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    assert_eq!(
        state.get_rows().row_count(),
        1,
        "the Departments tab lists Engineering"
    );
    assert_eq!(
        state.get_rows().row_data(0).expect("row").title,
        "Engineering"
    );
    assert_eq!(state.get_form_title(), "New department");

    // A new department needs a name; then the create is sent and its id is selected.
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Enter a name.");
    state.set_name("Art".into());
    state.invoke_save();
    assert_eq!(state.get_form_error(), "");
    let write = app.admin_org.write_generation();
    app.admin_org.deliver_save(
        write,
        Ok(Saved {
            id: ART.to_string(),
            name: "Art".to_string(),
            row: Row::Department(Department {
                id: ART.to_string(),
                name: "Art".to_string(),
                parent_department_id: None,
                head_id: None,
                member_count: 0,
            }),
        }),
    );
    assert_eq!(toast_text(), "Department created");
    assert_eq!(app.admin_org.selected(), ART);
    assert!(
        state.get_rows().iter().any(|row| row.title == "Art"),
        "the new department shows in the list at once"
    );

    // Engineering is picked, then deleted: the server refuses while it has people, and its message shows in the form.
    state.invoke_pick(ENG.into());
    assert_eq!(state.get_name(), "Engineering");
    assert_eq!(state.get_parent(), "");
    assert_eq!(state.get_person(), ADA);
    assert_eq!(state.get_form_title(), "Edit department");
    state.invoke_delete_item();
    let write = app.admin_org.write_generation();
    app.admin_org.deliver_delete(
        write,
        Err(api_error(
            409,
            "department_has_members",
            "Move its 9 people first.",
        )),
    );
    assert_eq!(state.get_form_error(), "Move its 9 people first.");
    assert_eq!(
        app.admin_org.selected(),
        ENG,
        "a refused delete keeps the selection"
    );

    // The Teams tab: a team needs a department.
    state.set_tab(1);
    state.invoke_tab_changed(1);
    assert_eq!(state.get_rows().row_count(), 1);
    assert_eq!(state.get_form_title(), "New team");
    state.set_name("Platform".into());
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Choose a department.");

    // A non-admin sees the gate.
    app.admin_org.deliver(app.admin_org.generation(), Ok(None));
    assert!(state.get_denied());
}

/// The Positions form: a position is a title with a level, a description and a salary range. It has no department field, so
/// picking a position leaves the form's team and department pick empty, and a save needs a title.
pub fn admin_org_position_form() {
    crate::support::init();
    let app = app_shell::build(ORGANISATION, ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<AdminOrgState>();
    app.admin_org
        .deliver(app.admin_org.generation(), Ok(Some(reference())));

    state.set_tab(2);
    state.invoke_tab_changed(2);
    assert_eq!(state.get_rows().row_count(), 1);
    assert_eq!(
        state.get_rows().row_data(0).expect("position").detail,
        "Senior · 1 filled · 1 open"
    );
    assert_eq!(state.get_form_title(), "New position");
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Enter a title.");

    state.invoke_pick(POSITION.into());
    assert_eq!(state.get_name(), "Engineer");
    assert_eq!(state.get_level(), "Senior");
    assert_eq!(state.get_parent(), "", "a position has no department");
    assert_eq!(state.get_salary_min(), "");
    assert_eq!(state.get_form_title(), "Edit position");

    state.set_name("".into());
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Enter a title.");
}

pub fn admin_roles_add_and_remove() {
    crate::support::init();
    let app = app_shell::build(ROLES, ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<AdminRolesState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);

    let roles = vec![
        RoleMembers {
            role: "admin".to_string(),
            members: vec![PersonSummary {
                id: ADA.to_string(),
                display_name: "Ada Lovelace".to_string(),
                title: Some("Engineer".to_string()),
                team: None,
                avatar_url: None,
            }],
        },
        RoleMembers {
            role: "hr".to_string(),
            members: vec![],
        },
    ];
    let people = vec![
        person_entry(ADA, "Ada Lovelace"),
        person_entry(BEN, "Ben Kato"),
    ];
    app.admin_roles
        .deliver(app.admin_roles.generation(), Ok(Some((roles, people))));
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    assert_eq!(state.get_admins().row_count(), 1);
    assert_eq!(state.get_hr().row_count(), 0);
    assert_eq!(state.get_person_options().row_count(), 2);

    // Adding needs a person picked; nothing is sent without one.
    state.invoke_add("admin".into());
    assert_eq!(state.get_action_error(), "Choose a person first.");
    assert_eq!(app.admin_roles.write_generation(), 0);

    // Picked, granted: the picker clears and the toast names the person.
    state.set_pick_admin(BEN.into());
    state.invoke_add("admin".into());
    assert_eq!(state.get_action_error(), "");
    let write = app.admin_roles.write_generation();
    assert_eq!(write, 1);
    app.admin_roles.deliver_change(
        write,
        Change::Grant,
        "admin",
        Ok(profile(BEN, "Ben Kato", &["admin"], true)),
    );
    assert_eq!(state.get_pick_admin(), "");
    assert_eq!(toast_text(), "Role granted to Ben Kato");

    // Removing the last admin is refused with 409, and the message shows.
    state.invoke_remove("admin".into(), ADA.into());
    let write = app.admin_roles.write_generation();
    app.admin_roles.deliver_change(
        write,
        Change::Revoke,
        "admin",
        Err(api_error(
            409,
            "last_admin",
            "At least one admin must remain.",
        )),
    );
    assert_eq!(state.get_action_error(), "At least one admin must remain.");

    // A removal that works names the person.
    state.invoke_remove("admin".into(), BEN.into());
    let write = app.admin_roles.write_generation();
    app.admin_roles.deliver_change(
        write,
        Change::Revoke,
        "admin",
        Ok(profile(BEN, "Ben Kato", &[], true)),
    );
    assert_eq!(toast_text(), "Role removed from Ben Kato");
    assert_eq!(state.get_action_error(), "");
}

const SEAT: &str = "5e000000-0000-4000-8000-000000000001";

/// The newest toast announced since the last call (the queue's timer is not running in a test), as its title.
fn toast_text() -> String {
    take_announced()
        .pop()
        .map(|(_, title)| title)
        .unwrap_or_default()
}

pub fn admin_org_seats_and_budgets() {
    crate::support::init();
    let app = app_shell::build(ORGANISATION, ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<AdminOrgState>();

    let mut reference = reference();
    reference.seats = vec![Seat {
        id: SEAT.to_string(),
        team_id: TEAM.to_string(),
        team_name: "Gameplay".to_string(),
        job_profile_id: Some(POSITION.to_string()),
        job_profile_title: Some("Lead".to_string()),
        level: "Senior".to_string(),
        state: SEAT_OPEN.to_string(),
        person_id: None,
        person_name: None,
        reports_to_seat_id: None,
        salary_min: Some("50000.00".to_string()),
        salary_max: Some("60000.00".to_string()),
        currency: Some("GBP".to_string()),
    }];
    reference.budgets = vec![TeamBudget {
        team_id: TEAM.to_string(),
        team_name: "Gameplay".to_string(),
        cap: Some("200000.00".to_string()),
        currency: Some("GBP".to_string()),
        used: "60000.00".to_string(),
        remaining: Some("140000.00".to_string()),
    }];
    app.admin_org
        .deliver(app.admin_org.generation(), Ok(Some(reference)));

    // Seats: the list shows the seat's position title with its team, level, state and vacancy. A new seat needs a team, then a
    // position; it has no title field of its own.
    state.set_tab(3);
    state.invoke_tab_changed(3);
    assert_eq!(state.get_rows().row_count(), 1);
    assert_eq!(state.get_rows().row_data(0).expect("seat").title, "Lead");
    assert_eq!(
        state.get_rows().row_data(0).expect("seat").detail,
        "Gameplay · Senior · open · vacant"
    );
    assert_eq!(state.get_form_title(), "New seat");
    assert_eq!(state.get_seat_state(), SEAT_OPEN, "a new seat starts open");
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Choose a team.");
    state.set_parent(TEAM.into());
    state.invoke_save();
    assert_eq!(state.get_form_error(), "Choose a position.");

    // Picking the seat fills the form with its team and position; a seat has no name or salary of its own.
    state.invoke_pick(SEAT.into());
    assert_eq!(
        state.get_name(),
        "",
        "a seat's title is its position, not a typed name"
    );
    assert_eq!(state.get_parent(), TEAM);
    assert_eq!(state.get_position(), POSITION);
    assert_eq!(state.get_form_title(), "Edit seat");

    // A save of the picked seat is sent; the saved seat stays selected.
    state.invoke_save();
    let write = app.admin_org.write_generation();
    app.admin_org.deliver_save(
        write,
        Ok(Saved {
            id: SEAT.to_string(),
            name: "Lead".to_string(),
            row: Row::Seat(Seat {
                id: SEAT.to_string(),
                team_id: TEAM.to_string(),
                team_name: "Gameplay".to_string(),
                job_profile_id: Some(POSITION.to_string()),
                job_profile_title: Some("Lead".to_string()),
                level: String::new(),
                state: SEAT_OPEN.to_string(),
                person_id: None,
                person_name: None,
                reports_to_seat_id: None,
                salary_min: None,
                salary_max: None,
                currency: None,
            }),
        }),
    );
    assert_eq!(toast_text(), "Seat saved");

    // A seat write over the team's budget shows the server's message in the form.
    state.invoke_pick(SEAT.into());
    state.invoke_save();
    let write = app.admin_org.write_generation();
    app.admin_org.deliver_save(
        write,
        Err(api_error(
            409,
            "over_budget",
            "This seat takes Gameplay over its budget.",
        )),
    );
    assert_eq!(
        state.get_form_error(),
        "This seat takes Gameplay over its budget."
    );

    // Budgets: each team's cap, used and remaining in the row; picking it fills the cap.
    state.set_tab(4);
    state.invoke_tab_changed(4);
    assert_eq!(state.get_rows().row_count(), 1);
    assert_eq!(
        state.get_rows().row_data(0).expect("budget").detail,
        "GBP 200000.00 cap · 60000.00 used · 140000.00 left"
    );
    state.invoke_pick(TEAM.into());
    assert_eq!(state.get_cap(), "200000.00");
    assert_eq!(state.get_currency(), "GBP");
    assert_eq!(state.get_form_title(), "Edit budget");

    // A budget is removed by clearing its cap, not by Delete, which says so.
    state.invoke_delete_item();
    assert_eq!(
        state.get_form_error(),
        "Clear the cap and save to remove a team's budget."
    );
}
