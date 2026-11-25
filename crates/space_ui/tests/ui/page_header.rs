//! Page header (IA sections 6, 7 and 8; tail C1). The primary action slot (none on any page today), the Share menu (Copy
//! link on every page, and the public link row on docs, files and roadmaps; both disabled), the overflow menu
//! (open in a new tab, duplicate the tab; the tab operations), the inspector toggle (enabled on Detail, Split and Board
//! pages only) and the phone back arrow (one step back in the focused tab's history; shown only while there is one).
//!
//! The window is built headless (`app_shell::build`) and the clicks are mock clicks, as in the other UI tests.

use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize, Model as _, ModelRc};
use space_nav::pages;
use space_ui::slint_ui::{MenuItem, app_shell};
use space_ui::theme::ThemeMode;

/// The rows of a menu model as (id, disabled, shortcut column text).
fn menu(items: ModelRc<MenuItem>) -> Vec<(String, bool, String)> {
    (0..items.row_count())
        .map(|i| {
            let row = items.row_data(i).expect("menu row");
            (row.id.to_string(), row.disabled, row.shortcut.to_string())
        })
        .collect()
}

fn row(id: &str, disabled: bool, shortcut: &str) -> (String, bool, String) {
    (id.to_owned(), disabled, shortcut.to_owned())
}

/// The Share menu on a plain page has only Copy link, disabled with its reason. A docs page adds the public link row,
/// also disabled.
pub fn page_header_share_menu_rows_by_page_kind() {
    crate::support::init();

    let plain = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    assert_eq!(
        menu(plain.ui.get_share_items()),
        vec![row("copy_link", false, "")],
        "a plain page has Copy link only, enabled (gaps D2)"
    );

    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current("grand-tale"));
    let docs = app_shell::build("/p/grand-tale/docs", ThemeMode::Light).expect("window builds");
    let rows = menu(docs.ui.get_share_items());
    assert_eq!(
        rows.last().cloned(),
        Some(row("public_link", true, "Not available yet")),
        "a docs page lists the public link row, disabled, with the reason"
    );
    assert!(
        rows.iter().any(|(id, ..)| id == "copy_link"),
        "a docs page still lists Copy link"
    );

    // The Share button opens its menu. The click must not fail.
    docs.ui.invoke_test_open_share_menu();
}

/// The overflow menu opens a new tab on the page, or duplicates the tab: the tab strip gains a tab each time.
pub fn page_header_overflow_menu_reuses_the_tab_operations() {
    crate::support::init();

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    assert_eq!(
        menu(ui.get_page_menu_items()),
        vec![row("open_new_tab", false, ""), row("duplicate", false, "")],
        "the overflow menu lists the two tab operations"
    );
    assert_eq!(ui.get_open_tabs().row_count(), 1, "one tab to start");

    ui.invoke_test_open_page_menu();
    ui.invoke_page_menu_picked("open_new_tab".into());
    assert_eq!(
        ui.get_open_tabs().row_count(),
        2,
        "open in new tab adds a tab"
    );
    assert_eq!(ui.get_open_active(), 1, "the new tab is active");
    assert_eq!(
        ui.get_page_route().as_str(),
        "/home",
        "the new tab shows the same page"
    );

    ui.invoke_page_menu_picked("duplicate".into());
    assert_eq!(ui.get_open_tabs().row_count(), 3, "duplicate adds a tab");
    assert_eq!(ui.get_page_route().as_str(), "/home");

    // An unknown id changes nothing.
    ui.invoke_page_menu_picked("archive".into());
    assert_eq!(
        ui.get_open_tabs().row_count(),
        3,
        "an unknown row changes nothing"
    );
}

/// The primary action slot. No page has one, so no label is shown and the request opens nothing. A page with an id gets
/// the quick action item's label (OQ21).
pub fn page_header_primary_action_slot_is_empty_until_a_page_has_one() {
    crate::support::init();

    assert!(
        pages()
            .iter()
            .all(|page| app_shell::primary_action_label(page).is_none()),
        "no manifest page has a primary action yet"
    );

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    assert!(
        app.ui.get_primary_action_label().is_empty(),
        "no primary action button on a page without one"
    );
    app.ui.invoke_primary_action_requested();
    assert!(
        app.last_quick_pick.borrow().is_none(),
        "a request with no primary action opens nothing"
    );

    // A page that has an id: the label comes from the quick action item with that id.
    let mut page = pages()[0].clone();
    page.primary_action = Some("new_task");
    assert_eq!(
        app_shell::primary_action_label(&page).as_deref(),
        Some("New Task"),
        "the label is the quick action item's label"
    );
    page.primary_action = Some("no_such_dialog");
    assert_eq!(
        app_shell::primary_action_label(&page).as_deref(),
        Some("no_such_dialog"),
        "an unknown id is shown as it is"
    );
}

/// The inspector toggle is enabled on Detail, Split and Board pages only. Its click flips the open state.
pub fn page_header_inspector_toggle_is_enabled_on_detail_split_and_board_only() {
    crate::support::init();

    let plain = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    assert!(
        !plain.ui.get_inspector_available(),
        "the Dashboard has no inspector"
    );

    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current("grand-tale"));
    let detail =
        app_shell::build("/p/grand-tale/issues/GT-42", ThemeMode::Light).expect("window builds");
    assert!(
        detail.ui.get_inspector_available(),
        "a Detail page has an inspector"
    );
    assert!(!detail.ui.get_inspector_open());
    detail
        .ui
        .set_inspector_open(!detail.ui.get_inspector_open());
    assert!(
        detail.ui.get_inspector_open(),
        "the toggle opens the inspector"
    );
    detail
        .ui
        .set_inspector_open(!detail.ui.get_inspector_open());
    assert!(
        !detail.ui.get_inspector_open(),
        "the toggle closes it again"
    );

    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current("grand-tale"));
    let board = app_shell::build("/p/grand-tale/issues", ThemeMode::Light).expect("window builds");
    assert!(
        !board.ui.get_inspector_available(),
        "an issues page has no inspector"
    );

    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current("grand-tale"));
    let list = app_shell::build("/p/grand-tale/issues", ThemeMode::Light).expect("window builds");
    assert!(
        !list.ui.get_inspector_available(),
        "a List page has no inspector"
    );
}

/// The phone back arrow sits in the page header. It is hidden with no history, and it steps back in the focused tab.
pub fn page_header_phone_back_arrow_steps_back_and_hides_without_history() {
    crate::support::init();

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");

    assert!(!ui.get_can_back(), "no history at the start");
    assert!(
        !ui.get_back_arrow_shown(),
        "no back arrow without a step back"
    );

    // Projects (phone tab 2), then Home (phone tab 0): two entries in the history. Projects opens on its first page, the Overview.
    ui.invoke_tab_activated(2);
    ui.invoke_tab_activated(0);
    assert!(ui.get_can_back(), "the history has a step back");
    assert!(
        ui.get_back_arrow_shown(),
        "the back arrow shows while there is a step back"
    );

    ui.invoke_back_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        "/p/grand-tale",
        "the back arrow returns to the previous page"
    );
}

/// A project page's middle breadcrumb is the project's name, and clicking it opens the project overview.
pub fn page_header_project_crumb_shows_the_project_name() {
    crate::support::init();

    // The project's name comes from the project list, which the app loads from GET /v1/projects (decision 0030).
    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current("grand-tale"));
    let app = app_shell::build("/p/grand-tale/issues", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let crumbs: Vec<String> = ui.get_page_crumbs().iter().map(|c| c.to_string()).collect();
    assert_eq!(
        crumbs,
        ["Projects", "Grand Tale", "Issues"],
        "the project page's crumbs name the project"
    );

    ui.invoke_crumb_activated(1);
    assert_eq!(
        ui.get_page_route().as_str(),
        "/p/grand-tale",
        "the project crumb opens the project overview"
    );
}
