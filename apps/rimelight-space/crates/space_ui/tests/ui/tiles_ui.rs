//! Tile groups in the window (planned feature B, tiles; T2a; `PROJECT.md section 6.6` "Tile groups"). A workspace's page
//! tabs sit in groups, each with its own page tab strip. The window draws them from `tile-groups` and `tile-handles`, and
//! the test calls the callbacks the markup calls (the pointer is not driven: a split by drag is T2b).
//!
//! One test function: the testing backend is initialised once per process, so the steps run in order here.

use slint::{ComponentHandle as _, Model as _};
use space_nav::{Area, pages_in, parse};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppShell, TileGroup};
use space_ui::theme::ThemeMode;

/// The start route. Its workspace is the first one.
const START: &str = "/home";

/// The routes of the Home area that have a page other than the start route, in sidebar order. The start route is left out:
/// navigating to the page the tab already shows adds no history step.
fn home_routes() -> Vec<&'static str> {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .filter(|route| *route != START)
        .collect()
}

/// The sidebar row of `route` in the Home area.
fn row_of(route: &str) -> i32 {
    sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row") as i32
}

/// The title of `route`'s page.
fn title_of(route: &str) -> String {
    parse(route).expect("a known route").page.title.to_owned()
}

/// Shows a Home route in the focused group, as a sidebar click does.
fn navigate(app: &App, route: &str) {
    app.ui.invoke_sidebar_activated(row_of(route));
}

/// Opens a Home route in a new tab of the focused group, as a Ctrl+click does.
fn open_in_new_tab(app: &App, route: &str) {
    app.ui.invoke_sidebar_opened_in_new_tab(row_of(route));
}

/// The tile groups of the active workspace, in model order.
fn groups(ui: &AppShell) -> Vec<TileGroup> {
    ui.get_tile_groups().iter().collect()
}

/// The id of the focused group.
fn focused_id(ui: &AppShell) -> i32 {
    groups(ui)
        .iter()
        .find(|group| group.focused)
        .expect("one group is focused")
        .id
}

/// The labels of a group's page tabs, in strip order.
fn labels(group: &TileGroup) -> Vec<String> {
    group.tabs.iter().map(|tab| tab.label.to_string()).collect()
}

/// The group with id `id`.
fn group(ui: &AppShell, id: i32) -> TileGroup {
    groups(ui)
        .into_iter()
        .find(|group| group.id == id)
        .unwrap_or_else(|| panic!("no group {id}"))
}

/// Right-clicks tab `index` of group `id`, then picks `item` from its menu.
fn pick_on_tab(app: &App, id: i32, index: i32, item: &str) {
    app.ui
        .invoke_tile_tab_context_requested(id, index, 0.0, 0.0);
    app.ui.invoke_open_tab_menu_picked(index, item.into());
}

/// Whether the menu row `id` is enabled, or `None` when the menu has no such row.
fn menu_enabled(ui: &AppShell, id: &str) -> Option<bool> {
    ui.get_open_tab_menu()
        .iter()
        .find(|item| item.id.as_str() == id)
        .map(|item| !item.disabled)
}

/// Splits tab `index` of group `id` to the right, as the page tab menu's "Split right" does.
fn split_right(app: &App, id: i32, index: i32) {
    pick_on_tab(app, id, index, "split_right");
}

pub fn tile_groups_render_focus_split_resize_and_switch() {
    crate::support::init();
    let routes = home_routes();
    assert!(routes.len() >= 6, "Home has enough pages for the test");

    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");
    // The page area's size, as the window reports it. The bar is 6 px thick, so a 1000 px area splits in half at 497 px.
    ui.invoke_tile_area_resized(1000.0, 600.0);

    // One group renders as before: one strip over the page, no bars, and back and forward work.
    let one = groups(ui);
    assert_eq!(one.len(), 1, "one group at the start");
    assert!(one[0].focused, "the only group is focused");
    assert_eq!(one[0].w, 1000.0, "the group fills the page area");
    assert_eq!(one[0].h, 600.0, "the group fills the page area");
    assert_eq!(
        ui.get_tile_handles().row_count(),
        0,
        "one group has no bars"
    );
    assert_eq!(
        labels(&one[0]),
        [title_of(START)],
        "the start page is the only tab"
    );
    navigate(&app, routes[0]);
    assert!(ui.get_can_back(), "a navigation can go back");
    ui.invoke_back_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        START,
        "back returns to the start page"
    );
    ui.invoke_forward_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        routes[0],
        "forward returns to the page"
    );

    // Split right on a two-tab group: two groups, two strips, and the moved tab is the new group's only tab.
    open_in_new_tab(&app, routes[1]);
    assert_eq!(group(ui, 0).tabs.row_count(), 2, "the group has two tabs");
    split_right(&app, 0, 1);
    let two = groups(ui);
    assert_eq!(two.len(), 2, "the split makes two groups");
    assert_eq!(
        labels(&group(ui, 0)).len(),
        1,
        "the first group keeps one tab"
    );
    assert_eq!(
        labels(&group(ui, 1)).len(),
        1,
        "the new group holds the moved tab"
    );
    assert_eq!(focused_id(ui), 1, "the new group is focused");
    assert!(two[1].x > two[0].x, "the new group is to the right");
    assert_eq!(
        ui.get_tile_handles().row_count(),
        1,
        "one bar between two groups"
    );
    assert!(
        ui.get_tile_handles()
            .row_data(0)
            .expect("one bar")
            .vertical_bar,
        "a side-by-side split has a vertical bar"
    );

    // A click on the first group focuses it, and the sidebar click then acts on that group only.
    ui.invoke_tile_group_focused(0);
    assert_eq!(focused_id(ui), 0, "the click focuses the first group");
    let second_before = labels(&group(ui, 1));
    navigate(&app, routes[2]);
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(routes[2])],
        "the sidebar click replaces the focused group's tab"
    );
    assert_eq!(
        labels(&group(ui, 1)),
        second_before,
        "the other group is unchanged"
    );
    assert_eq!(
        ui.get_page_route().as_str(),
        routes[2],
        "the header shows the focused page"
    );

    // Closing the only tab of group 1 collapses the split, and group 0 takes the space.
    ui.invoke_tile_tab_closed(1, 0);
    assert_eq!(groups(ui).len(), 1, "the empty group closes");
    assert_eq!(ui.get_tile_handles().row_count(), 0, "the bar goes with it");
    assert_eq!(
        group(ui, 0).w,
        1000.0,
        "the remaining group takes the space"
    );

    // A resize handle changes the ratio, and is clamped. Build a split again first.
    open_in_new_tab(&app, routes[3]);
    split_right(&app, 0, 1);
    assert_eq!(groups(ui).len(), 2, "two groups again");
    ui.invoke_tile_handle_dragged(0, 250.0, 300.0);
    let dragged = group(ui, 0).w;
    assert!(
        (240.0..=255.0).contains(&dragged),
        "a drag to x = 250 gives the first group about 250 px, got {dragged}"
    );
    ui.invoke_tile_handle_dragged(0, 0.0, 300.0);
    let clamped = group(ui, 0).w;
    assert!(
        (clamped - 99.4).abs() < 1.0,
        "a drag past the minimum clamps the ratio to 0.1, got {clamped}"
    );

    // The six-group cap: split until six groups exist, then Split right and Split down are disabled.
    while groups(ui).len() < 6 {
        open_in_new_tab(&app, routes[4]);
        let index = ui.get_open_tabs().row_count() as i32 - 1;
        split_right(&app, focused_id(ui), index);
        assert!(groups(ui).len() <= 6, "the layout never passes the cap");
    }
    assert_eq!(groups(ui).len(), 6, "six groups");
    assert_eq!(
        ui.get_tile_handles().row_count(),
        5,
        "five bars between six groups"
    );
    open_in_new_tab(&app, routes[5]);
    ui.invoke_tile_tab_context_requested(focused_id(ui), 1, 0.0, 0.0);
    assert_eq!(
        menu_enabled(ui, "split_right"),
        Some(false),
        "Split right is disabled at the cap"
    );
    assert_eq!(
        menu_enabled(ui, "split_down"),
        Some(false),
        "Split down is disabled at the cap"
    );
    ui.invoke_open_tab_menu_picked(1, "split_down".into());
    assert_eq!(
        groups(ui).len(),
        6,
        "the picked split does nothing at the cap"
    );

    // Each workspace has its own groups: a new workspace has one, and switching back brings the six back.
    ui.invoke_new_workspace_requested();
    assert_eq!(groups(ui).len(), 1, "a new workspace has one group");
    ui.invoke_workspace_activated(0);
    assert_eq!(
        groups(ui).len(),
        6,
        "switching back swaps in the first workspace's groups"
    );
    assert_eq!(
        ui.get_workspace_tabs().row_count(),
        2,
        "two workspaces are open"
    );
}
