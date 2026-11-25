//! Android Back closes the topmost overlay first (gaps D7, owner decision; planned feature A3). Each overlay kind is opened, then
//! Back (`Key.Back`) closes it and leaves the page history alone. With nothing open, Back steps the active tab's history. With no
//! history left, Back is not accepted (`system-back` returns false), so the system finishes the activity.
//!
//! The headless backend has no hardware keys, so Back is sent as a window key event, and the clicks as mock clicks.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model as _, ModelRc, SharedString, VecModel};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppShell, JobItem};
use space_ui::theme::ThemeMode;

/// The start route of every test.
const START: &str = "/home";

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &impl ComponentHandle, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Holds a modifier key down, as the OS does while the other key is pressed.
fn hold(ui: &impl ComponentHandle, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
}

/// Lets go of a modifier key.
fn release(ui: &impl ComponentHandle, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}

/// The Android Back key. A popup or sheet may close after its exit animation, so a second of mock time runs before the state is
/// read (the testing backend only runs animations when time moves).
fn back(ui: &impl ComponentHandle) {
    press(ui, Key::Back);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    slint::platform::update_timers_and_animations();
}

/// A phone-sized window (below the phone breakpoint).
fn to_phone(ui: &AppShell) {
    let size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(size);
    ui.window().dispatch_event(WindowEvent::Resized { size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");
}

/// Two Home pages other than the start route, as (sidebar label, route).
fn home_pages() -> Vec<(String, &'static str)> {
    let mut pages: Vec<(String, &'static str)> = Vec::new();
    for page in pages_in(Area::Home).filter(|page| !page.has_params()) {
        let label = page.title.to_string();
        if page.route != START && !pages.iter().any(|(seen, _)| *seen == label) {
            pages.push((label, page.route));
        }
    }
    assert!(!pages.is_empty(), "Home has another page");
    pages.truncate(1);
    pages
}

/// A sidebar row pick of the Home page `route`, as the sidebar's row click does (its index in the Home rows).
fn pick_home_row(ui: &AppShell, route: &str) {
    let row = sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row");
    ui.invoke_sidebar_activated(row as i32);
    slint::platform::update_timers_and_animations();
}

/// A window at the start route with one history step (the first Home page after the start). Back has a step to take.
fn with_history() -> (App, &'static str) {
    crate::support::init();
    let pages = home_pages();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    pick_home_row(&app.ui, pages[0].1);
    assert_eq!(app.ui.get_page_route().as_str(), pages[0].1);
    assert!(app.ui.get_can_back(), "one history step to go back to");
    (app, pages[0].1)
}

/// Opens a non-Slint-owned state check: Back closes the overlay and leaves the route and the history step alone. Then the
/// closed state is asserted with `closed`.
fn assert_back_closes_without_history(
    app: &App,
    route: &str,
    what: &str,
    closed: impl Fn(&AppShell) -> bool,
) {
    let ui = &app.ui;
    assert!(!closed(ui), "{what} is open before Back");
    back(ui);
    assert!(closed(ui), "Back closes {what}");
    assert_eq!(
        ui.get_page_route().as_str(),
        route,
        "Back on {what} leaves the page alone"
    );
    assert!(
        ui.get_can_back(),
        "Back on {what} leaves the history step in place"
    );
}

pub fn back_closes_page_tab_menu() {
    let (app, route) = with_history();
    let ui = &app.ui;
    right_click_first_tab(ui);
    assert!(ui.get_tab_menu_open(), "the page tab menu is open");
    assert_back_closes_without_history(&app, route, "the page tab menu", |ui| {
        !ui.get_tab_menu_open()
    });
}

pub fn back_closes_share_menu() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_test_open_share_menu();
    assert!(ui.get_share_menu_open(), "the Share menu is open");
    assert_back_closes_without_history(&app, route, "the Share menu", |ui| {
        !ui.get_share_menu_open()
    });
}

pub fn back_closes_page_overflow_menu() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_test_open_page_menu();
    assert!(ui.get_page_menu_open(), "the page overflow menu is open");
    assert_back_closes_without_history(&app, route, "the page overflow menu", |ui| {
        !ui.get_page_menu_open()
    });
}

pub fn back_closes_workspace_menu_on_the_phone_sheet() {
    let (app, route) = with_history();
    let ui = &app.ui;
    to_phone(ui);
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_test_open_sheet_row_menu(0);
    // The sheet's focus scope keeps the keyboard focus over the menu, so Back reaches the sheet first. The sheet hands that
    // Back to the open menu (`child-open`, `back-to-child`), so the menu closes and the sheet stays.
    slint::platform::update_timers_and_animations();
    assert!(
        ui.get_workspace_menu_open(),
        "the workspace row menu is open"
    );
    assert_back_closes_without_history(&app, route, "the workspace row menu", |ui| {
        !ui.get_workspace_menu_open()
    });
    assert!(
        ui.get_workspace_sheet_open(),
        "Back closed the menu, not the sheet"
    );
}

pub fn back_closes_profile_popover() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_test_open_profile();
    assert!(ui.get_profile_open(), "the profile menu is open");
    assert_back_closes_without_history(&app, route, "the profile menu", |ui| {
        !ui.get_profile_open()
    });
}

pub fn back_closes_quick_action_menu() {
    let (app, route) = with_history();
    let ui = &app.ui;
    hold(ui, Key::Control);
    hold(ui, Key::Alt);
    press(ui, "n");
    release(ui, Key::Alt);
    release(ui, Key::Control);
    assert!(ui.get_quick_menu_open(), "the quick action menu is open");
    assert_back_closes_without_history(&app, route, "the quick action menu", |ui| {
        !ui.get_quick_menu_open()
    });
}

pub fn back_closes_project_switcher() {
    crate::support::init();
    let app = app_shell::build("/p/grand-tale/issues", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.invoke_test_open_project_switcher();
    assert!(
        ui.get_project_switcher_open(),
        "the project switcher is open"
    );
    let route = ui.get_page_route().to_string();
    back(ui);
    assert!(
        !ui.get_project_switcher_open(),
        "Back closes the project switcher"
    );
    assert_eq!(
        ui.get_page_route().as_str(),
        route,
        "Back leaves the page alone"
    );
}

pub fn back_closes_status_bar_jobs_popover() {
    crate::support::init();
    let (app, route) = with_history();
    let ui = &app.ui;
    let jobs = VecModel::from(vec![JobItem {
        title: "Upload report.pdf".into(),
        state: "40%".into(),
    }]);
    ui.set_jobs(ModelRc::new(jobs));
    ui.invoke_test_click_jobs();
    slint::platform::update_timers_and_animations();
    assert!(ui.get_status_jobs_open(), "the jobs popover is open");
    assert_back_closes_without_history(&app, route, "the jobs popover", |ui| {
        !ui.get_status_jobs_open()
    });
}

pub fn back_closes_status_bar_language_popover() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_test_click_language();
    slint::platform::update_timers_and_animations();
    assert!(ui.get_status_language_open(), "the Language menu is open");
    assert_back_closes_without_history(&app, route, "the Language menu", |ui| {
        !ui.get_status_language_open()
    });
}

pub fn back_closes_rename_dialog() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_open_workspace_rename(0, "Launch".into());
    assert!(ui.get_workspace_rename_open(), "the rename dialog is open");
    assert_back_closes_without_history(&app, route, "the rename dialog", |ui| {
        !ui.get_workspace_rename_open()
    });
}

pub fn back_closes_shortcuts_dialog() {
    let (app, route) = with_history();
    let ui = &app.ui;
    press(ui, "?");
    assert!(ui.get_shortcuts_open(), "the shortcuts dialog is open");
    assert_back_closes_without_history(&app, route, "the shortcuts dialog", |ui| {
        !ui.get_shortcuts_open()
    });
}

pub fn back_closes_command_palette() {
    let (app, route) = with_history();
    let ui = &app.ui;
    hold(ui, Key::Control);
    press(ui, "k");
    release(ui, Key::Control);
    assert!(ui.get_palette_open(), "the command palette is open");
    assert_back_closes_without_history(&app, route, "the command palette", |ui| {
        !ui.get_palette_open()
    });
}

pub fn back_closes_workspace_sheet() {
    let (app, route) = with_history();
    let ui = &app.ui;
    to_phone(ui);
    ui.invoke_test_open_workspace_sheet();
    assert!(ui.get_workspace_sheet_open(), "the workspace sheet is open");
    assert_back_closes_without_history(&app, route, "the workspace sheet", |ui| {
        !ui.get_workspace_sheet_open()
    });
}

pub fn back_on_the_phone_drawer_returns_to_more_then_closes_the_drawer() {
    crate::support::init();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    to_phone(ui);
    let route = ui.get_page_route().to_string();

    // The More list opens the drawer.
    ui.invoke_tab_activated(4);
    assert!(
        ui.get_drawer_open() && ui.get_more_view(),
        "the More list is open"
    );
    // An area tree opened from More: Back returns to the More list, and the drawer stays open.
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    let people = ui
        .get_more_areas()
        .iter()
        .position(|area| area.label.as_str() == "People")
        .expect("the More list has People");
    ui.invoke_more_area_activated(people as i32);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    assert!(!ui.get_more_view(), "the area tree is open");
    back(ui);
    assert!(ui.get_drawer_open(), "Back keeps the drawer open");
    assert!(ui.get_more_view(), "Back returns to the More list first");
    // Back on the More list closes the drawer.
    back(ui);
    assert!(!ui.get_drawer_open(), "Back closes the drawer");
    assert_eq!(
        ui.get_page_route().as_str(),
        route,
        "Back on the drawer leaves the page alone"
    );
}

pub fn back_closes_about_report_and_settings_screens() {
    let (app, route) = with_history();
    let ui = &app.ui;
    ui.invoke_about_clicked();
    assert!(ui.get_about_open(), "About is open");
    assert_back_closes_without_history(&app, route, "About", |ui| !ui.get_about_open());

    ui.invoke_report_issue_clicked();
    assert!(ui.get_report_open(), "Report Issue is open");
    assert_back_closes_without_history(&app, route, "Report Issue", |ui| !ui.get_report_open());

    ui.invoke_settings_clicked();
    assert!(ui.get_settings_open(), "Settings is open");
    assert_back_closes_without_history(&app, route, "Settings", |ui| !ui.get_settings_open());
}

pub fn back_with_nothing_open_steps_history_then_is_not_accepted() {
    let (app, _route) = with_history();
    let ui = &app.ui;
    back(ui);
    assert_eq!(
        ui.get_page_route().as_str(),
        START,
        "Back with nothing open steps the history"
    );
    assert!(!ui.get_can_back(), "no step left to go back to");

    // At the first entry with nothing open, Back is not accepted: the system finishes the activity.
    assert!(
        !ui.invoke_system_back(),
        "system-back returns false with no history left"
    );
    back(ui);
    assert_eq!(ui.get_page_route().as_str(), START, "the first entry stays");
}

/// The right press on the first page tab of the focused group: the strip's context request, then its menu opens.
fn right_click_first_tab(ui: &AppShell) {
    let group = ui
        .get_tile_groups()
        .iter()
        .find(|group| group.focused)
        .expect("one group is focused")
        .id;
    ui.invoke_test_open_tab_menu(group, 0);
    slint::platform::update_timers_and_animations();
}
