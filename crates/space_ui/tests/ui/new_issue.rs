//! Projects > New issue (S1.5c): the dialog that New issue opens (from the issues page and from the quick action). The project
//! is the route's when the person may create issues there, else the first one they may; the defaults are a task and normal;
//! a blank summary sends nothing; Create sends the body and opens the issue. No session is set, so no request leaves the
//! window: the create body is read back from the page.

use std::time::Duration;

use slint::Model;
use space_page_kit::projects;
use space_ui::slint_ui::app_shell;
use space_ui::theme::ThemeMode;

/// Sets the project list the person sees: EMB and OPS may create issues, VIEW is a viewer's project.
fn set_projects() {
    projects::set_list(vec![
        projects::Project {
            key: "EMB".to_owned(),
            name: "Embers".to_owned(),
        },
        projects::Project {
            key: "OPS".to_owned(),
            name: "Operations".to_owned(),
        },
        projects::Project {
            key: "VIEW".to_owned(),
            name: "Viewed".to_owned(),
        },
    ]);
    projects::set_editable(vec!["EMB".to_owned(), "OPS".to_owned()]);
}

/// Moves the mock clock on and pumps the page's first events.
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn open_window(route: &str) -> app_shell::App {
    crate::support::init();
    let app = app_shell::build(route, ThemeMode::Light).expect("window builds");
    wait(10);
    app
}

pub fn dialog_opens_on_the_route_project_with_the_defaults() {
    set_projects();
    let app = open_window("/p/OPS/issues");
    let ui = &app.ui;
    assert!(!ui.get_new_issue_is_open());

    ui.invoke_new_issue_requested();
    assert!(ui.get_new_issue_is_open(), "New issue opens the dialog");
    assert_eq!(ui.get_new_issue_project(), "OPS", "the route's project");
    assert_eq!(ui.get_new_issue_type(), "Task");
    assert_eq!(ui.get_new_issue_priority(), "Normal");
    assert_eq!(ui.get_new_issue_summary(), "");
    assert!(!ui.get_new_issue_can_create(), "Create waits for a summary");
    assert_eq!(
        ui.get_new_issue_project_options().row_count(),
        2,
        "only the projects the person may create in"
    );
}

pub fn dialog_takes_the_first_editable_project_when_the_route_has_none() {
    set_projects();
    let app = open_window("/p/VIEW/issues");
    let ui = &app.ui;
    ui.invoke_new_issue_requested();
    assert!(ui.get_new_issue_is_open());
    assert_eq!(
        ui.get_new_issue_project(),
        "EMB",
        "a viewer's project is not offered, so the first editable one is"
    );
}

pub fn a_blank_summary_blocks_create() {
    set_projects();
    let app = open_window("/p/EMB/issues");
    let ui = &app.ui;
    ui.invoke_new_issue_requested();

    ui.set_new_issue_summary("   ".into());
    wait(10);
    assert!(
        !ui.get_new_issue_can_create(),
        "a blank summary cannot be created"
    );
    ui.invoke_new_issue_create();
    assert!(app.new_issue.last_body().is_none(), "nothing is sent");
    assert!(!ui.get_new_issue_creating());
}

pub fn create_sends_the_body_then_opens_the_issue() {
    set_projects();
    let app = open_window("/p/OPS/issues");
    let ui = &app.ui;
    ui.invoke_new_issue_requested();

    ui.set_new_issue_summary("  Crash on load ".into());
    wait(10);
    ui.set_new_issue_description("Steps below".into());
    ui.set_new_issue_due_date("2026-11-30".into());
    ui.set_new_issue_tags("ui, backend".into());
    assert!(ui.get_new_issue_can_create());

    // The body: the summary trimmed, the defaults as chosen, and the optional fields as typed.
    ui.invoke_new_issue_create();
    assert!(
        ui.get_new_issue_creating(),
        "busy until the answer comes back"
    );
    let body = app.new_issue.last_body().expect("the create body");
    assert_eq!(body.summary, "Crash on load");
    assert_eq!(body.description.as_deref(), Some("Steps below"));
    assert_eq!(body.issue_type.as_deref(), Some("Task"));
    assert_eq!(body.priority.as_deref(), Some("Normal"));
    assert_eq!(body.estimate_points, None, "no estimation field");
    assert_eq!(body.due_date.as_deref(), Some("2026-11-30"));
    assert_eq!(body.tags, Some(vec!["ui".to_owned(), "backend".to_owned()]));
    assert_eq!(
        ui.get_page_route(),
        "/p/OPS/issues",
        "nothing opens before the answer"
    );

    // The answer opens the new issue and closes the dialog.
    app.new_issue.deliver_created_key("OPS-15");
    assert!(!ui.get_new_issue_creating());
    assert!(!ui.get_new_issue_is_open());
    assert_eq!(ui.get_page_route(), "/p/OPS/issues/OPS-15");
}

pub fn priority_choices_show_the_stored_values() {
    set_projects();
    let app = open_window("/p/EMB/issues");
    let ui = &app.ui;
    ui.invoke_new_issue_requested();
    let priorities = ui.get_new_issue_priority_options();
    let first = priorities.row_data(0).expect("the first priority");
    assert_eq!(first.value, "Critical");
    assert_eq!(first.label, first.value, "the label is the stored value");
    let normal = priorities.row_data(2).expect("Normal");
    assert_eq!(normal.value, "Normal");
    assert_eq!(normal.label, "Normal");
}

pub fn quick_action_opens_the_dialog_with_the_project_preselected() {
    set_projects();
    let app = open_window("/p/OPS/issues");
    let ui = &app.ui;
    ui.invoke_quick_item_picked("new_issue".into());
    assert!(
        ui.get_new_issue_is_open(),
        "the quick action opens the dialog"
    );
    assert_eq!(ui.get_new_issue_project(), "OPS", "the route's project");

    // Off a project route, the first editable project is chosen.
    let app = open_window("/home");
    let ui = &app.ui;
    ui.invoke_quick_item_picked("new_issue".into());
    assert!(ui.get_new_issue_is_open());
    assert_eq!(ui.get_new_issue_project(), "EMB");
}

pub fn quick_action_new_issue_is_disabled_without_an_editable_project() {
    projects::set_list(Vec::new());
    projects::set_editable(Vec::new());
    let app = open_window("/home");
    let categories = app.ui.get_quick_categories();
    let entry = (0..categories.row_count())
        .filter_map(|row| categories.row_data(row))
        .flat_map(|category| {
            (0..category.items.row_count()).filter_map(move |i| category.items.row_data(i))
        })
        .find(|item| item.id == "new_issue")
        .expect("the New issue entry");
    assert!(!entry.enabled, "no project lets the person create issues");
    assert!(!entry.hint.is_empty(), "a disabled entry says why");
    set_projects();
}

pub fn areas_are_picked_from_the_admin_catalogue_and_the_due_date_from_the_grid() {
    set_projects();
    let app = open_window("/p/EMB/issues");
    let ui = &app.ui;
    ui.invoke_new_issue_requested();

    let area = |name: &str| space_proto::projects::Area {
        id: format!("id-{name}"),
        name: name.to_owned(),
        color: "#1659C4".to_owned(),
    };
    app.new_issue
        .deliver_areas(vec![area("Art"), area("Code"), area("UI")]);
    assert_eq!(
        ui.get_new_issue_area_options().row_count(),
        3,
        "the catalogue"
    );

    ui.invoke_new_issue_area_toggled("UI".into());
    ui.invoke_new_issue_area_toggled("Code".into());
    assert_eq!(ui.get_new_issue_tags(), "UI, Code", "chosen in order");
    assert_eq!(ui.get_new_issue_area_summary(), "UI, Code");
    ui.invoke_new_issue_area_toggled("UI".into());
    assert_eq!(
        ui.get_new_issue_tags(),
        "Code",
        "a second click takes it off"
    );
    let checked = ui.get_new_issue_area_options();
    assert!(
        (0..checked.row_count()).any(|i| checked
            .row_data(i)
            .is_some_and(|row| row.value == "Code" && row.checked)),
        "the chosen area is checked in the picker"
    );

    let days = ui.get_new_issue_due_days();
    let index = (0..days.row_count())
        .find(|&i| {
            days.row_data(i)
                .is_some_and(|day| day.in_month && day.day == 15)
        })
        .expect("the 15th is in the grid");
    ui.invoke_new_issue_due_picked(index as i32);
    let due = ui.get_new_issue_due_date().to_string();
    assert_eq!(due.len(), 10, "YYYY-MM-DD: {due}");
    assert!(due.ends_with("-15"), "the picked day: {due}");
}
