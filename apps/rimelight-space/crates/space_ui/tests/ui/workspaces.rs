//! Workspaces (planned feature B2; IA part B and section 9). The workspace strip sits in its own row above the top bar, with one
//! tab per workspace and a "+" button. Ctrl+Alt+T, Ctrl+Alt+W and Ctrl+Alt+1 to 9 are real key events; the workspace menu's
//! picks go to its callback, because the popup items are not reachable from the headless backend. The empty state shows
//! when the last workspace closes, and "New workspace" brings the normal view back.
//!
//! One test function: the testing backend is initialised once per process, so the steps run in order here.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model as _, SharedString};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppWindow, MenuKind};
use space_ui::theme::ThemeMode;

/// The start route. Its workspace is the first one.
const START: &str = "/home";

/// The Home page a new workspace opens on.
const HOME: &str = "/home";

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &AppWindow, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Holds a modifier key down, as the OS does while another key is pressed.
fn hold(ui: &AppWindow, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
}

/// Lets go of a modifier key.
fn release(ui: &AppWindow, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}

/// Ctrl+Alt+`key` (Cmd+Option+`key` on macOS), with the modifiers held as the OS holds them.
fn chord(ui: &AppWindow, key: &str) {
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    hold(ui, modifier);
    hold(ui, Key::Alt);
    press(ui, key);
    release(ui, Key::Alt);
    release(ui, modifier);
}

/// A Home page other than the start route, for a second page tab.
fn other_home_route() -> &'static str {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .find(|route| *route != START)
        .expect("Home has another page")
}

/// Opens a Home route in a new page tab, as a Ctrl+click on its sidebar row does.
fn open_in_new_tab(app: &App, route: &str) {
    let row = sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row");
    app.ui.invoke_sidebar_opened_in_new_tab(row as i32);
}

/// The number of open workspaces, as the strip shows them.
fn workspace_count(ui: &AppWindow) -> usize {
    ui.get_workspace_tabs().row_count()
}

/// The number of page tabs of the active workspace, as the page strip shows them.
fn page_tab_count(ui: &AppWindow) -> usize {
    ui.get_open_tabs().row_count()
}

/// The workspace menu rows as the window holds them: (label, enabled, separator).
fn workspace_menu_rows(ui: &AppWindow) -> Vec<(String, bool, bool)> {
    ui.get_open_workspace_menu()
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

pub fn workspace_strip_shortcuts_menu_and_empty_state() {
    crate::support::init();
    let route = other_home_route();

    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");

    // The strip shows the one start workspace, the "+" button is drawn, and the window is not in the empty state.
    assert_eq!(workspace_count(ui), 1, "one workspace at the start");
    assert!(
        ui.get_workspace_can_add(),
        "a workspace can be added below the cap"
    );
    assert!(!ui.get_no_workspace(), "the start has a workspace");
    assert!(
        ui.get_workspace_add_shown(),
        "the + button is drawn in the strip"
    );

    // The workspace strip is as tall as the sidebar context header (SidebarSize.header-height, 36 px), not the page-tab height.
    // The strip's row is given that height (`workspace-height`), and the strip fills it.
    assert_eq!(
        ui.get_workspace_height(),
        36.0,
        "the workspace strip matches the sidebar header height"
    );

    // "+" opens a workspace on Home and makes it active. Its strip tab is the active one.
    ui.invoke_new_workspace_requested();
    assert_eq!(workspace_count(ui), 2, "the + button adds a workspace");
    assert_eq!(ui.get_workspace_active(), 1, "the new workspace is active");
    assert_eq!(ui.get_page_route().as_str(), HOME, "it opens on Home");
    assert_eq!(page_tab_count(ui), 1, "it has one page tab");

    // In the second workspace, a second page tab.
    open_in_new_tab(&app, route);
    assert_eq!(
        page_tab_count(ui),
        2,
        "two page tabs in the second workspace"
    );
    assert_eq!(ui.get_page_route().as_str(), route);

    // Ctrl+Alt+1 switches to the start workspace: its one page tab and its page come back.
    chord(ui, "1");
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "Ctrl+Alt+1 activates workspace 1"
    );
    assert_eq!(
        page_tab_count(ui),
        1,
        "the page tabs swap with the workspace"
    );
    assert_eq!(ui.get_page_route().as_str(), START);

    // Ctrl+Alt+2 switches back: its two page tabs and its active page.
    chord(ui, "2");
    assert_eq!(
        ui.get_workspace_active(),
        1,
        "Ctrl+Alt+2 activates workspace 2"
    );
    assert_eq!(
        page_tab_count(ui),
        2,
        "the second workspace keeps its two tabs"
    );
    assert_eq!(ui.get_page_route().as_str(), route, "and its active page");

    // Plain Ctrl+Alt+T (no Shift) adds a workspace on Home, after the last one.
    chord(ui, "t");
    assert_eq!(workspace_count(ui), 3, "Ctrl+Alt+T adds a workspace");
    assert_eq!(ui.get_workspace_active(), 2);
    assert_eq!(ui.get_page_route().as_str(), HOME);

    // Ctrl+Alt+W closes the active (third) workspace; the one before it becomes active.
    chord(ui, "w");
    assert_eq!(
        workspace_count(ui),
        2,
        "Ctrl+Alt+W closes the active workspace"
    );
    assert_eq!(
        ui.get_workspace_active(),
        1,
        "the workspace before it is active"
    );
    assert_eq!(ui.get_page_route().as_str(), route);

    // The menu of the second workspace (the last one): the bulk closes that would do something are enabled, Close to the
    // right is not, Copy link is enabled, and Reopen is enabled (Ctrl+Alt+W closed one above), in the menu's order.
    ui.invoke_workspace_context_requested(1, 0.0, 0.0);
    let rows = workspace_menu_rows(ui);
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
            "Duplicate workspace",
            "Rename",
            "Pin workspace",
            "Copy link",
            "",
            "Reopen closed workspace",
        ],
        "the workspace menu lists the actions in the owner's order"
    );
    assert!(
        rows[1].1 && rows[2].1 && rows[4].1,
        "close others, left and all are enabled"
    );
    assert!(!rows[3].1, "nothing is to the right of the last workspace");
    assert!(rows[9].1, "Copy link is enabled (gaps D2)");
    assert!(
        rows[11].1,
        "Reopen is enabled: the workspace that Ctrl+Alt+W closed is on the closed stack"
    );
    let separators: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, (_, _, separator))| *separator)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(separators, [5, 10], "two separators");

    // Close others on the second workspace keeps it. The first one goes onto the closed stack.
    ui.invoke_workspace_menu_picked(1, "close_others".into());
    assert_eq!(workspace_count(ui), 1, "Close others leaves one workspace");
    assert_eq!(ui.get_workspace_active(), 0);
    assert_eq!(
        ui.get_page_route().as_str(),
        route,
        "the kept workspace is shown"
    );

    // Reopen is enabled now, and brings back the closed start workspace at the end, as the active one.
    ui.invoke_workspace_context_requested(0, 0.0, 0.0);
    assert!(
        workspace_menu_rows(ui)[11].1,
        "Reopen is enabled after a close"
    );
    ui.invoke_workspace_menu_picked(0, "reopen".into());
    assert_eq!(workspace_count(ui), 2, "the closed workspace comes back");
    assert_eq!(
        ui.get_workspace_active(),
        1,
        "the reopened workspace is active"
    );
    assert_eq!(ui.get_page_route().as_str(), START, "with its page");

    // Pin on the first workspace moves it to the pinned group (it is already first), and the active one stays active.
    ui.invoke_workspace_menu_picked(0, "pin".into());
    assert_eq!(
        ui.get_workspace_pinned_count(),
        1,
        "one workspace is pinned"
    );
    assert_eq!(
        ui.get_workspace_active(),
        1,
        "pinning keeps the active workspace"
    );

    // Rename on the first workspace opens the dialog, and the name it is sent shows on its strip tab. An empty name is
    // automatic again: the strip then shows the page title.
    ui.invoke_workspace_menu_picked(0, "rename".into());
    ui.invoke_workspace_renamed(0, "Sales".into());
    let first = ui.get_workspace_tabs().row_data(0).expect("first tab");
    assert_eq!(
        first.label.as_str(),
        "Sales",
        "the rename shows on the strip"
    );
    assert!(first.pinned, "the renamed workspace stays pinned");
    ui.invoke_workspace_renamed(0, "   ".into());
    let first = ui.get_workspace_tabs().row_data(0).expect("first tab");
    assert_ne!(
        first.label.as_str(),
        "Sales",
        "an empty name clears the name"
    );
    assert!(
        !first.label.is_empty(),
        "the automatic name is the page title"
    );

    // Plain Close closes a pinned workspace too. Ctrl+Alt+W then closes the last one: the empty state shows.
    ui.invoke_workspace_menu_picked(0, "close".into());
    assert_eq!(workspace_count(ui), 1);
    ui.invoke_close_workspace_requested();
    assert_eq!(workspace_count(ui), 0, "the last workspace closes");
    assert!(
        ui.get_no_workspace(),
        "the empty state shows with no workspace"
    );
    assert_eq!(page_tab_count(ui), 0, "no page tab is left");
    assert_eq!(ui.get_page_route().as_str(), "", "no page is shown");
    assert!(
        ui.get_workspace_active() < 0,
        "no workspace is active in the empty state"
    );

    // The rail still works with no workspace: a pick opens a workspace for its page.
    ui.invoke_rail_activated(0);
    assert_eq!(workspace_count(ui), 1, "a rail pick opens a workspace");
    assert!(!ui.get_no_workspace());
    assert!(
        !ui.get_page_route().is_empty(),
        "the page the rail opened is shown"
    );

    // The empty state again, then "New workspace" restores the normal view on Home.
    ui.invoke_close_workspace_requested();
    assert!(ui.get_no_workspace());
    ui.invoke_new_workspace_requested();
    assert!(!ui.get_no_workspace(), "New workspace restores the view");
    assert_eq!(workspace_count(ui), 1);
    assert_eq!(ui.get_page_route().as_str(), HOME);

    // The cap: nine more workspaces make ten, and the "+" is then disabled. A tenth request adds nothing.
    for _ in 0..9 {
        ui.invoke_new_workspace_requested();
    }
    assert_eq!(workspace_count(ui), 10, "ten workspaces is the cap");
    assert!(
        !ui.get_workspace_can_add(),
        "the + button is disabled at the cap"
    );
    ui.invoke_new_workspace_requested();
    assert_eq!(workspace_count(ui), 10, "no eleventh workspace");

    // Ctrl+Alt+T at the cap is a no-op too.
    chord(ui, "t");
    assert_eq!(
        workspace_count(ui),
        10,
        "Ctrl+Alt+T does nothing at the cap"
    );
}
