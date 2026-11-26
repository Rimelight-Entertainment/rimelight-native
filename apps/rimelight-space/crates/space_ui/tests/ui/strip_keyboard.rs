//! Keyboard on the tab strips (gaps D3, D4; `PROJECT.md section 6.6` section 9). The page-tab strip of the group and the
//! workspace strip are one Tab stop each. Right and Left move the focus, Home and End jump, Enter activates, Delete closes an
//! unpinned tab, Shift+F10 (or the Menu key) opens the tab menu, and Escape gives the focus back to the content. A pinned tab
//! shows its full title in a tooltip on keyboard focus. The key events are real window events; the menu picks are the same
//! keys (Down, Enter) a user sends to the open menu.
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

/// Owner of the workspace strip in `strip-focus-owner`.
const WORKSPACES: i32 = -1;
/// Owner value when no strip has the keyboard focus.
const NONE: i32 = -2;

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &AppShell, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Holds a modifier key down, as the OS does while another key is pressed.
fn hold(ui: &AppShell, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
}

/// Lets go of a modifier key.
fn release(ui: &AppShell, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}

/// Shift+`key`, with Shift held as the OS holds it.
fn shift_press(ui: &AppShell, key: Key) {
    hold(ui, Key::Shift);
    press(ui, key);
    release(ui, Key::Shift);
}

/// Ctrl+K or Cmd+K, with the modifier held as the OS holds it.
fn ctrl_press(ui: &AppShell, letter: &str) {
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    hold(ui, modifier);
    let text: SharedString = letter.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    release(ui, modifier);
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

/// The labels of the focused group's page tabs, in strip order.
fn tab_labels(ui: &AppShell) -> Vec<String> {
    ui.get_open_tabs()
        .iter()
        .map(|tab| tab.label.to_string())
        .collect()
}

/// The id of the first (and only) tile group.
fn group_id(ui: &AppShell) -> i32 {
    ui.get_tile_groups().row_data(0).expect("one tile group").id
}

/// Presses Tab until `owner` holds the keyboard focus. Fails when Tab never gets there.
fn tab_into(ui: &AppShell, owner: i32) {
    for _ in 0..200 {
        if ui.get_strip_focus_owner() == owner {
            return;
        }
        press(ui, Key::Tab);
    }
    panic!("Tab did not reach the strip {owner}");
}

/// The page tab menu's selectable rows, as the Down key moves through them (the separators are skipped).
fn selectable_rows(ui: &AppShell) -> Vec<(usize, String)> {
    ui.get_open_tab_menu()
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            !item.disabled && !matches!(item.kind, MenuKind::Separator | MenuKind::Heading)
        })
        .map(|(at, item)| (at, item.label.to_string()))
        .collect()
}

/// Presses Down in the open menu until the row labelled `label` is highlighted (the first row is highlighted on open),
/// then Enter. Fails when the row is not there.
fn pick_menu_row(ui: &AppShell, label: &str) {
    let rows = selectable_rows(ui);
    let steps = rows
        .iter()
        .position(|(_, text)| text == label)
        .unwrap_or_else(|| panic!("the menu has no row {label:?}"));
    for _ in 0..steps {
        press(ui, Key::DownArrow);
    }
    press(ui, Key::Return);
}

pub fn strip_keyboard_roving_focus_menus_and_tooltip() {
    crate::support::init();
    let routes = home_routes();
    assert_eq!(routes.len(), 2, "Home has two other pages");

    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");

    // Three page tabs in the one group: Home, and the two pages opened in new tabs.
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    assert_eq!(tab_labels(ui).len(), 3, "three page tabs are open");
    let group = group_id(ui);

    // Tab enters the page-tab strip on its active tab, and the strip's group is focused.
    tab_into(ui, group);
    let active = ui.get_open_active();
    assert_eq!(
        ui.get_strip_focus_tab(),
        active,
        "Tab enters the strip on the active tab"
    );
    assert!(
        ui.get_tile_groups().row_data(0).expect("group").focused,
        "the strip's group is focused"
    );

    // Right and Left move the focus only: the active tab does not change.
    press(ui, Key::RightArrow);
    assert_eq!(
        ui.get_strip_focus_tab(),
        (active + 1) % 3,
        "Right moves the focus to the next tab"
    );
    assert_eq!(ui.get_open_active(), active, "Right does not activate");
    press(ui, Key::LeftArrow);
    assert_eq!(ui.get_strip_focus_tab(), active, "Left moves back");

    // Home and End jump to the first and last tab.
    press(ui, Key::Home);
    assert_eq!(ui.get_strip_focus_tab(), 0, "Home goes to the first tab");
    press(ui, Key::End);
    assert_eq!(ui.get_strip_focus_tab(), 2, "End goes to the last tab");
    assert_eq!(ui.get_open_active(), active, "Home and End do not activate");

    // Enter activates the focused tab.
    press(ui, Key::Home);
    press(ui, Key::Return);
    assert_eq!(ui.get_open_active(), 0, "Enter activates the focused tab");

    // Pin the second tab from its menu (the same action as the menu test): the pinned tab moves to the front.
    ui.invoke_open_tab_menu_picked(1, "pin".into());
    let first = ui.get_open_tabs().row_data(0).expect("a first tab");
    assert!(first.pinned, "the tab moved to the front is pinned");
    let pinned_title = first.label.to_string();

    // A pinned tab shows its full title on keyboard focus, and an unpinned one shows nothing.
    press(ui, Key::Home);
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        pinned_title,
        "a pinned tab shows its full title on focus"
    );
    press(ui, Key::RightArrow);
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        "",
        "an unpinned tab shows no pinned tooltip"
    );
    press(ui, Key::LeftArrow);

    // Delete does nothing on a pinned tab, and closes an unpinned one.
    let before = tab_labels(ui).len();
    press(ui, Key::Home);
    press(ui, Key::Delete);
    assert_eq!(
        tab_labels(ui).len(),
        before,
        "Delete does not close a pinned tab"
    );
    press(ui, Key::RightArrow);
    press(ui, Key::Delete);
    assert_eq!(
        tab_labels(ui).len(),
        before - 1,
        "Delete closes the unpinned tab"
    );
    assert_eq!(
        ui.get_strip_focus_owner(),
        group,
        "the strip keeps the keyboard focus after a close"
    );

    // Shift+F10 opens the tab menu for the focused tab. Escape closes it and the focus goes back to the tab.
    let focused = ui.get_strip_focus_tab();
    shift_press(ui, Key::F10);
    assert!(ui.get_tab_menu_open(), "Shift+F10 opens the tab menu");
    press(ui, Key::Escape);
    assert!(!ui.get_tab_menu_open(), "Escape closes the tab menu");
    assert_eq!(
        ui.get_strip_focus_owner(),
        group,
        "Escape gives the focus back to the tab strip"
    );
    assert_eq!(
        ui.get_strip_focus_tab(),
        focused,
        "the focus is on the same tab"
    );

    // The menu's keys: Down moves to Pin, Enter picks it, and the menu closes.
    shift_press(ui, Key::F10);
    assert!(ui.get_tab_menu_open(), "the menu opens again");
    let pinned_before = ui.get_open_tabs().iter().filter(|tab| tab.pinned).count();
    pick_menu_row(ui, "Pin tab");
    assert!(
        !ui.get_tab_menu_open(),
        "Enter picks the row and closes the menu"
    );
    assert_eq!(
        ui.get_open_tabs().iter().filter(|tab| tab.pinned).count(),
        pinned_before + 1,
        "the Pin row pinned the focused tab"
    );

    // Close from the menu: the first row, closes the focused tab.
    let count = tab_labels(ui).len();
    shift_press(ui, Key::F10);
    pick_menu_row(ui, "Close");
    assert_eq!(
        tab_labels(ui).len(),
        count - 1,
        "the Close row closes the tab"
    );
    assert_eq!(
        ui.get_strip_focus_owner(),
        group,
        "the focus is back on the strip after a menu pick"
    );

    // Escape gives the focus back to the content.
    press(ui, Key::Escape);
    assert_eq!(ui.get_strip_focus_owner(), NONE, "Escape leaves the strip");

    // The workspace strip: a second workspace, then Tab into the strip.
    ui.invoke_new_workspace_requested();
    tab_into(ui, WORKSPACES);
    let workspace_active = ui.get_workspace_active();
    assert_eq!(
        ui.get_strip_focus_tab(),
        workspace_active,
        "Tab enters the workspace strip on the active workspace"
    );
    press(ui, Key::LeftArrow);
    assert_eq!(
        ui.get_workspace_active(),
        workspace_active,
        "Left does not switch the workspace"
    );
    press(ui, Key::Return);
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "Enter switches to the focused workspace"
    );

    // Shift+F10 opens the workspace menu on the focused workspace; Escape closes it and the focus is back on the strip.
    shift_press(ui, Key::F10);
    assert!(
        ui.get_workspace_menu_open(),
        "Shift+F10 opens the workspace menu"
    );
    press(ui, Key::Escape);
    assert!(
        !ui.get_workspace_menu_open(),
        "Escape closes the workspace menu"
    );
    assert_eq!(
        ui.get_strip_focus_owner(),
        WORKSPACES,
        "the workspace strip keeps the focus"
    );

    // Escape leaves the workspace strip, and a shortcut still works after the strip interaction.
    press(ui, Key::Escape);
    assert_eq!(
        ui.get_strip_focus_owner(),
        NONE,
        "Escape leaves the workspace strip"
    );
    ctrl_press(ui, "k");
    assert!(
        ui.get_palette_open(),
        "Ctrl+K still opens the palette after strip keys"
    );
}
