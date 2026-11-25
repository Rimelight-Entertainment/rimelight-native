//! Company lists as tables (C1): Employees, Teams and Departments each fill a table global with its columns, its rows and the
//! id of each row, in the same order. No session is set, so nothing is fetched; the results are injected through the page
//! handles.

use slint::{ComponentHandle as _, Model};
use space_proto::people::{Department, OrgTreeEntry, Page, PersonSummary, Team};
use space_ui::slint_ui::{DeptsTable, EmployeesTable, TeamsTable, app_shell};
use space_ui::theme::ThemeMode;

fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(ms));
}

fn titles<G: Model<Data = space_ui::slint_ui::TableColumn>>(columns: &G) -> Vec<String> {
    (0..columns.row_count())
        .map(|i| columns.row_data(i).expect("column").title.to_string())
        .collect()
}

pub fn company_lists_are_tables_with_columns_and_ids() {
    crate::support::init();

    // Employees: name, title and team, with the person id of each row.
    let app = app_shell::build("/company/employees", ThemeMode::Light).expect("window builds");
    wait(10);
    app.employees.deliver(
        app.employees.generation(),
        Ok(Page {
            items: vec![PersonSummary {
                id: "p1".to_string(),
                display_name: "Ann Adams".to_string(),
                title: Some("Developer".to_string()),
                team: None,
                avatar_url: None,
            }],
            next_cursor: None,
        }),
    );
    let table = app.ui.global::<EmployeesTable>();
    assert_eq!(titles(&table.get_columns()), vec!["Name", "Title", "Team"]);
    assert_eq!(table.get_ids().row_data(0).expect("id").as_str(), "p1");
    assert_eq!(table.get_rows().row_count(), 1);
    drop(app);

    // Teams: team, department, lead and members.
    let app = app_shell::build("/company/teams", ThemeMode::Light).expect("window builds");
    wait(10);
    app.teams.deliver(
        app.teams.generation(),
        Ok((
            vec![Team {
                id: "t1".to_string(),
                name: "Gameplay".to_string(),
                department_id: "d1".to_string(),
                lead_id: None,
                member_count: 2,
            }],
            vec![Department {
                id: "d1".to_string(),
                name: "Engineering".to_string(),
                parent_department_id: None,
                head_id: None,
                member_count: 4,
            }],
            Vec::<OrgTreeEntry>::new(),
            Vec::new(),
        )),
    );
    let table = app.ui.global::<TeamsTable>();
    assert_eq!(
        titles(&table.get_columns()),
        vec!["Team", "Department", "Lead", "Members"]
    );
    assert_eq!(table.get_ids().row_data(0).expect("id").as_str(), "t1");
    drop(app);

    // Departments: name, head and people.
    let app = app_shell::build("/company/departments", ThemeMode::Light).expect("window builds");
    wait(10);
    app.departments.deliver(
        app.departments.generation(),
        Ok((
            vec![Department {
                id: "d1".to_string(),
                name: "Engineering".to_string(),
                parent_department_id: None,
                head_id: None,
                member_count: 4,
            }],
            Vec::new(),
            Vec::new(),
        )),
    );
    let table = app.ui.global::<DeptsTable>();
    assert_eq!(
        titles(&table.get_columns()),
        vec!["Department", "Head", "People"]
    );
    assert_eq!(table.get_ids().row_data(0).expect("id").as_str(), "d1");
}
