//! Projects tree and project switcher (IA sections 2.6 and 3; tail C3). The Projects sidebar lists the project pages of the
//! current project, under the IA's group headings, with the key filled in. Its header is the project switcher: the name and
//! a list of every project, with a filter. A pick changes the key of every project row, and an active project page moves to
//! the same page of the new project. The live children and the counts are mock data (`shell::mock_children`,
//! `shell::mock_count`). The current project is a per-thread state, so each test sets it first and restores it at the end.

use slint::Model as _;
use space_nav::Area;
use space_page_kit::projects;
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{app_shell, palette};
use space_ui::theme::ThemeMode;

/// The first project of the fixture list (the start key). The list comes from `GET /v1/projects` in the app (decision 0030);
/// these tests load the fixture first and make this project current.
const START: &str = "grand-tale";

/// The routes of the project rows of the Projects sidebar (the `/p/` rows, not the live children).
fn project_row_routes() -> Vec<String> {
    sidebar_entries(Area::Projects)
        .into_iter()
        .filter(|entry| !entry.item.header && !entry.live)
        .filter_map(|entry| entry.route)
        .filter(|route| route.starts_with("/p/"))
        .collect()
}

/// The Projects sidebar lists the built project pages for the current key.
pub fn project_tree_lists_each_project_page_for_the_current_key() {
    crate::support::init();
    projects::set_list(projects::fixture());
    assert!(projects::set_current(START));

    let entries = sidebar_entries(Area::Projects);
    let headings: Vec<String> = entries
        .iter()
        .filter(|entry| entry.item.header)
        .map(|entry| entry.item.label.to_string())
        .collect();
    assert_eq!(
        headings.first().map(String::as_str),
        Some("PROJECT"),
        "the Project heading comes first"
    );
    assert!(
        headings.iter().any(|heading| heading == "PROJECT"),
        "the built project pages sit under the Project heading"
    );
    assert!(
        !headings
            .iter()
            .any(|heading| heading == "TEAM AND EQUIPMENT"),
        "a group with no built page has no heading"
    );

    assert!(
        !entries
            .iter()
            .any(|entry| entry.item.label == "All projects"),
        "All projects is gone from the sidebar"
    );

    // Under a current project only the built project pages (BUILT_PROJECT_PAGES in shell.rs) are rows, with the key filled in.
    let expected = [
        "overview", "issues", "content", "canvases", "settings", "members",
    ];
    let rows = project_row_routes();
    assert_eq!(
        rows.len(),
        expected.len(),
        "every built project page is a row"
    );
    assert!(
        rows.iter().all(|route| route.starts_with("/p/grand-tale")),
        "every project row has the current key: {rows:?}"
    );
    assert!(
        !rows
            .iter()
            .any(|route| route.contains(":id") || route.contains(":page")),
        "record pages are not rows"
    );

    // No mock counts on project pages and no live children: the Issues row has no count, and no row is a live child.
    let issues = entries
        .iter()
        .find(|entry| entry.route.as_deref() == Some("/p/grand-tale/issues"))
        .expect("Issues is a row");
    assert_eq!(issues.item.count, 0, "the Issues view count (no mock data)");

    assert!(
        entries.iter().all(|entry| !entry.live),
        "no live children under the project pages"
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.route.as_deref() != Some("/p/grand-tale/code/builds")),
        "Pipelines is not built, so it is not a row"
    );

    projects::set_current(START);
}

/// Choosing another project changes the key of every project row. An active project page moves to the same page of the new
/// project, as a new step in the history. The header shows the new name. An unknown key changes nothing.
pub fn project_switch_moves_every_project_row_and_the_active_page() {
    crate::support::init();
    projects::set_list(projects::fixture());
    assert!(projects::set_current(START));

    let app = app_shell::build("/p/grand-tale/issues", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    assert!(
        ui.get_sidebar_switcher(),
        "Projects has the project switcher in its header"
    );
    assert_eq!(ui.get_sidebar_title().as_str(), "Grand Tale");
    assert_eq!(ui.get_project_key().as_str(), START);

    ui.invoke_project_picked("harbor-run".into());
    assert_eq!(ui.get_project_key().as_str(), "harbor-run");
    assert_eq!(ui.get_sidebar_title().as_str(), "Harbor Run");
    assert_eq!(
        ui.get_page_route().as_str(),
        "/p/harbor-run/issues",
        "the active project page moves to the new project"
    );
    assert!(ui.get_can_back(), "the move is a step in the history");
    assert!(
        project_row_routes()
            .iter()
            .all(|route| route.starts_with("/p/harbor-run")),
        "every project row takes the new key"
    );
    assert!(
        palette::entries()
            .iter()
            .any(|entry| entry.route == "/p/harbor-run/issues"),
        "the palette follows the current project"
    );

    // A pick of an unknown key changes nothing.
    ui.invoke_project_picked("no-such-project".into());
    assert_eq!(ui.get_project_key().as_str(), "harbor-run");

    projects::set_current(START);
}

/// The switcher's filter matches the name or the key, ignoring case. A blank filter lists every project.
pub fn project_switcher_filter_narrows_by_name_or_key() {
    crate::support::init();
    projects::set_list(projects::fixture());
    assert!(projects::set_current(START));

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let choices = || -> Vec<(String, String)> {
        let rows = ui.get_project_choices();
        (0..rows.row_count())
            .map(|i| {
                let row = rows.row_data(i).expect("project row");
                (row.key.to_string(), row.name.to_string())
            })
            .collect()
    };
    assert_eq!(choices().len(), 3, "the switcher lists every project");

    ui.invoke_project_filter_changed("LAN".into());
    assert_eq!(
        choices(),
        vec![("lantern-keep".to_owned(), "Lantern Keep".to_owned())],
        "a name match ignores case"
    );

    ui.invoke_project_filter_changed("harb".into());
    assert_eq!(
        choices(),
        vec![("harbor-run".to_owned(), "Harbor Run".to_owned())],
        "a key match"
    );

    ui.invoke_project_filter_changed("zzz".into());
    assert!(choices().is_empty(), "no match, no rows");

    ui.invoke_project_filter_changed("  ".into());
    assert_eq!(choices().len(), 3, "a blank filter lists every project");

    // Pages outside Projects show their own header, not the switcher.
    let home = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    assert!(!home.ui.get_sidebar_switcher(), "Home has no switcher");

    projects::set_current(START);
}
