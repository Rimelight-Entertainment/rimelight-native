//! Page tab menu (planned feature A2b; IA sections 6 and 9). A right press on a page tab opens its menu at the pointer, with
//! the actions in the owner's order. The right press and the Escape key are real window events. The menu's picks go to its
//! callback, because the popup items are not reachable from the headless backend, so the close, pin and reopen steps are
//! checked on the tab strip model that the picks change.
//!
//! One test function: the testing backend is initialised once per process, so the steps run in order here.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model as _, SharedString};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppShell, MenuKind};
use space_ui::theme::ThemeMode;

/// The start route. Its tab is the first one.
const START: &str = "/home";

/// One key press and release, as an OS key event would deliver them.
fn press_key(ui: &AppShell, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Two Home routes other than the start route, in sidebar order.
fn home_routes() -> Vec<&'static str> {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .filter(|route| *route != START)
        .take(2)
        .collect()
}

/// Opens a Home route in a new tab, as a Ctrl+click on its sidebar row does.
fn open_in_new_tab(app: &App, route: &str) {
    let row = sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row");
    app.ui.invoke_sidebar_opened_in_new_tab(row as i32);
}

/// The labels of the open page tabs, in strip order.
fn tab_labels(ui: &AppShell) -> Vec<String> {
    ui.get_open_tabs()
        .iter()
        .map(|tab| tab.label.to_string())
        .collect()
}

/// A right press on tab `index` of the focused group: the strip's context request, then its menu opens (the same path as
/// the strip's own right press, without its pointer position).
fn right_click_tab(ui: &AppShell, index: usize) {
    let group = ui
        .get_tile_groups()
        .iter()
        .find(|group| group.focused)
        .expect("one group is focused")
        .id;
    ui.invoke_test_open_tab_menu(group, index as i32);
    slint::platform::update_timers_and_animations();
}

/// The page tab menu rows as the window holds them: (label, enabled, separator).
fn menu_rows(ui: &AppShell) -> Vec<(String, bool, bool)> {
    ui.get_open_tab_menu()
        .iter()
        .map(|item| {
            (
                item.label.to_string(),
                !item.disabled,
                matches!(item.kind, MenuKind::Separator),
            )
        })
        .collect()
}

pub fn page_tab_menu_right_click_and_actions() {
    crate::support::init();
    let routes = home_routes();
    assert_eq!(routes.len(), 2, "Home has two other pages");

    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");

    // Three tabs: Home, and the two pages opened in new tabs.
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    let labels = tab_labels(ui);
    assert_eq!(labels.len(), 3, "three page tabs are open");

    // A right press on the middle tab opens its menu, with the actions in the owner's order.
    right_click_tab(ui, 1);
    assert!(ui.get_tab_menu_open(), "a right press opens the tab menu");
    let rows = menu_rows(ui);
    let order: Vec<&str> = rows.iter().map(|(label, _, _)| label.as_str()).collect();
    assert_eq!(
        order,
        [
            "Close",
            "Close others",
            "Close to the left",
            "Close to the right",
            "Close all",
            "",
            "Duplicate tab",
            "Split right",
            "Split down",
            "Pin tab",
            "Copy link",
            "",
            "Reopen closed tab",
        ],
        "the actions are listed in the owner's order"
    );
    let separators: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, (_, _, separator))| *separator)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(separators, [5, 11], "two separators");
    assert!(
        rows[1].1 && rows[2].1 && rows[3].1 && rows[4].1,
        "the bulk closes are enabled with three unpinned tabs"
    );
    assert!(rows[10].1, "Copy link is enabled (gaps D2)");
    assert!(!rows[12].1, "Reopen is disabled while nothing was closed");
    press_key(ui, Key::Escape);
    assert!(!ui.get_tab_menu_open(), "Escape closes the menu");

    // Close others on the middle tab keeps that tab. The two others go to the closed stack.
    ui.invoke_open_tab_menu_picked(1, "close_others".into());
    assert_eq!(
        tab_labels(ui),
        [labels[1].clone()],
        "Close others keeps the tab it was opened on"
    );

    // Reopen is enabled after the close. Reopen brings back the last closed tab, the right-most one, at the end.
    right_click_tab(ui, 0);
    assert!(menu_rows(ui)[12].1, "Reopen is enabled after a close");
    press_key(ui, Key::Escape);
    ui.invoke_reopen_tab_requested();
    let reopened = tab_labels(ui);
    assert_eq!(reopened.len(), 2, "one tab is reopened");
    assert_eq!(
        reopened[1], labels[2],
        "the last closed tab comes back last"
    );
    assert_eq!(
        ui.get_page_route().as_str(),
        routes[1],
        "the reopened tab is the active page"
    );

    // Pin moves the right-most tab to the pinned group at the left, and it stays active.
    right_click_tab(ui, 1);
    press_key(ui, Key::Escape);
    ui.invoke_open_tab_menu_picked(1, "pin".into());
    let strip = ui.get_open_tabs();
    let first = strip.row_data(0).expect("the strip has a first tab");
    assert!(first.pinned, "the tab moved left is pinned");
    assert_eq!(
        first.label.as_str(),
        labels[2],
        "it is the tab that was pinned"
    );
    assert_eq!(ui.get_open_pinned_count(), 1, "one tab is pinned");
    assert_eq!(
        ui.get_page_route().as_str(),
        routes[1],
        "pinning keeps the active page"
    );

    // Plain Close closes a pinned tab too, with no confirmation.
    ui.invoke_open_tab_menu_picked(0, "close".into());
    assert_eq!(
        tab_labels(ui),
        [labels[1].clone()],
        "a pinned tab closes with plain Close"
    );
}
