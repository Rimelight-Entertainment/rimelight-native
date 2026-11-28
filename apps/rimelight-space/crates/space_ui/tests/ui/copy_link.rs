//! Copy link (gaps D2): the page tab menu, the workspace menu and the Share menu each enable "Copy link", and a pick writes
//! the web link of the right page through the clipboard writer and shows the "Link copied" toast. The Ctrl+K shortcut still
//! works after a copy.
//!
//! The headless testing backend has no readback of the OS clipboard, so the text is checked on the AppWindow's `last-copied`
//! property, which the clipboard writer sets. The OS clipboard itself is not verified by these tests.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle as _, Model as _, ModelRc, SharedString};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::history::link_for;
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppWindow, MenuItem};
use space_ui::theme::ThemeMode;

/// The start route. Its tab is the first one.
const START: &str = "/home";

/// The title of the toast a copy shows.
const TOAST: &str = "Link copied";

/// A Home page other than the start route, without a parameter.
fn other_home_route() -> &'static str {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .find(|route| *route != START)
        .expect("Home has a second page")
}

/// Opens a Home route in a new tab, as a Ctrl+click on its sidebar row does.
fn open_in_new_tab(app: &App, route: &str) {
    let row = sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row");
    app.ui.invoke_sidebar_opened_in_new_tab(row as i32);
}

/// Whether the menu model has the row `id` and that row is enabled.
fn enabled(items: &ModelRc<MenuItem>, id: &str) -> bool {
    (0..items.row_count()).any(|i| {
        let row = items.row_data(i).expect("menu row");
        row.id == id && !row.disabled
    })
}

/// The number of toasts with the copy title on the toast stack.
fn toast_count(ui: &AppWindow) -> usize {
    ui.get_toasts()
        .iter()
        .filter(|toast| toast.title == TOAST)
        .count()
}

/// Ctrl+K, as the OS delivers it: Control down, K, Control up.
fn ctrl_k(ui: &AppWindow) {
    let control: SharedString = Key::Control.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: control.clone(),
    });
    let k: SharedString = "k".into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: k.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: k });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: control });
}

pub fn copy_link_enabled_in_each_menu_and_writes_the_page_link() {
    crate::support::init();
    let second = other_home_route();
    let start_link = link_for(START);
    let second_link = link_for(second);

    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    open_in_new_tab(&app, second);
    assert_eq!(
        ui.get_page_route().as_str(),
        second,
        "the new tab is active"
    );

    // Page tab menu: the first tab's menu has Copy link enabled. A pick copies that tab's link (the start route).
    let group = ui.get_tile_groups().row_data(0).expect("a tile group").id;
    ui.invoke_tile_tab_context_requested(group, 0, 0.0, 0.0);
    assert!(
        enabled(&ui.get_open_tab_menu(), "copy_link"),
        "the page tab menu enables Copy link"
    );
    ui.invoke_open_tab_menu_picked(0, "copy_link".into());
    assert_eq!(
        ui.get_last_copied().as_str(),
        start_link,
        "the tab menu copies the first tab's web link"
    );
    assert_eq!(toast_count(ui), 1, "a copy shows the toast once");

    // Share menu: enabled on the active page, and it copies the active page's link.
    assert!(
        enabled(&ui.get_share_items(), "copy_link"),
        "the Share menu enables Copy link"
    );
    ui.invoke_share_menu_picked("copy_link".into());
    assert_eq!(
        ui.get_last_copied().as_str(),
        second_link,
        "Share copies the focused tab's web link"
    );
    assert_eq!(toast_count(ui), 2, "the Share copy shows its toast");

    // Workspace menu: enabled, and it copies the workspace's active page link (the second tab is active).
    ui.invoke_workspace_context_requested(0, 0.0, 0.0);
    assert!(
        enabled(&ui.get_open_workspace_menu(), "copy_link"),
        "the workspace menu enables Copy link"
    );
    ui.invoke_workspace_menu_picked(0, "copy_link".into());
    assert_eq!(
        ui.get_last_copied().as_str(),
        second_link,
        "the workspace menu copies the workspace's active page link"
    );
    assert_eq!(toast_count(ui), 3, "the workspace copy shows its toast");

    // Ctrl+K still opens the palette after the copies: the writer took no focus from the shell's key scope.
    ctrl_k(ui);
    assert!(ui.get_palette_open(), "Ctrl+K works after a copy");
    ui.invoke_palette_closed();
    assert!(!ui.get_palette_open());
}
