//! Admin > Users (SP.3e, U1..U4): the start date pickers, the edit dialog, the users table and the seat picker, on the real
//! window with results injected through the page's handle (no session).

use slint::{ComponentHandle as _, Model};
use space_page_kit::calendar::{self, Date};
use space_proto::people::{Page, Profile};
use space_ui::slint_ui::{AdminUsersState, app_shell};
use space_ui::theme::ThemeMode;

const USERS: &str = "/admin/users";
const ADA: &str = "f0000000-0000-4000-8000-000000000002";
const BEN: &str = "f0000000-0000-4000-8000-000000000003";

fn profile(id: &str, name: &str, start: Option<&str>) -> Profile {
    Profile {
        id: id.to_string(),
        email: format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
        display_name: name.to_string(),
        preferred_name: None,
        pronouns: None,
        title: None,
        position_id: None,
        department_id: None,
        department_name: None,
        team_id: None,
        team_name: None,
        manager_id: None,
        location: None,
        timezone: None,
        work_phone: None,
        bio: None,
        skills: Vec::new(),
        start_date: start.map(str::to_string),
        avatar_url: None,
        is_active: true,
        roles: Vec::new(),
    }
}

/// The index of `date` in the 42-cell grid of its month (Sunday first).
fn cell_index(date: Date) -> i32 {
    calendar::month_grid(date.year, date.month, 0)
        .iter()
        .position(|cell| cell.date == date)
        .expect("the date is in its own month grid") as i32
}

pub fn users_start_date_picks_an_iso_date() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    let state = app.ui.global::<AdminUsersState>();
    let generation = app.admin_users.generation();
    app.admin_users.deliver(
        generation,
        Ok(Some((
            Page {
                items: vec![profile(ADA, "Ada Lovelace", Some("2026-11-02"))],
                next_cursor: None,
            },
            Default::default(),
        ))),
    );

    // The add form: a picked day becomes the ISO start date, and the grid shows that month with 42 cells.
    state.set_add_open(true);
    state.set_add_start("2026-11-10".into());
    assert_eq!(state.get_add_month_title(), "November 2026");
    assert_eq!(state.get_add_days().row_count(), 42);
    state.invoke_add_pick(cell_index(Date::new(2026, 11, 15).expect("a date")));
    assert_eq!(state.get_add_start(), "2026-11-15");
    // A day from the neighbouring month is not a pick.
    let outside = calendar::month_grid(2026, 11, 0)
        .iter()
        .position(|cell| !cell.in_month)
        .expect("a November grid has days from October or December") as i32;
    state.invoke_add_pick(outside);
    assert_eq!(state.get_add_start(), "2026-11-15");
    // Moving the month shows December; the chosen day stays set.
    state.invoke_add_month_step(1);
    assert_eq!(state.get_add_month_title(), "December 2026");
    assert_eq!(state.get_add_start(), "2026-11-15");

    // The detail panel: the person's start date is the chosen day, and a pick saves an ISO date.
    state.invoke_select(ADA.into());
    assert_eq!(state.get_d_start(), "2026-11-02");
    assert_eq!(state.get_d_month_title(), "November 2026");
    state.invoke_d_pick(cell_index(Date::new(2026, 11, 20).expect("a date")));
    assert_eq!(state.get_d_start(), "2026-11-20");
}

pub fn users_table_shows_one_row_per_person_with_seven_cells() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    let state = app.ui.global::<AdminUsersState>();
    let generation = app.admin_users.generation();
    let mut ada = profile(ADA, "Ada Lovelace", None);
    ada.roles = vec!["admin".to_string(), "hr".to_string()];
    app.admin_users.deliver(
        generation,
        Ok(Some((
            Page {
                items: vec![ada, profile(BEN, "Ben Ito", None)],
                next_cursor: None,
            },
            Default::default(),
        ))),
    );
    // The page's rows carry the seven table columns: name, email, position, department, team, status and roles.
    let rows = state.get_rows();
    assert_eq!(rows.row_count(), 2);
    let first = rows.row_data(0).expect("row 0");
    let cells = [
        first.name.to_string(),
        first.email.to_string(),
        first.title.to_string(),
        first.department.to_string(),
        first.team.to_string(),
        first.status.to_string(),
        first.roles.to_string(),
    ];
    assert_eq!(
        cells,
        [
            "Ada Lovelace",
            "ada.lovelace@example.com",
            "",
            "",
            "",
            "Active",
            "Admin, HR",
        ]
    );
}

pub fn users_edit_opens_a_modal_for_the_picked_person() {
    crate::support::init();
    let app = app_shell::build(USERS, ThemeMode::Light).expect("window builds");
    let state = app.ui.global::<AdminUsersState>();
    let generation = app.admin_users.generation();
    app.admin_users.deliver(
        generation,
        Ok(Some((
            Page {
                items: vec![profile(ADA, "Ada Lovelace", None)],
                next_cursor: None,
            },
            Default::default(),
        ))),
    );
    // Picking a row opens the modal for that person; the list keeps its row. Closing clears the open state.
    assert!(!state.get_detail_open());
    state.invoke_select(ADA.into());
    assert!(state.get_detail_open());
    assert_eq!(state.get_detail_name(), "Ada Lovelace");
    assert_eq!(state.get_rows().row_count(), 1);
    state.set_detail_open(false);
    assert!(!state.get_detail_open());
}
