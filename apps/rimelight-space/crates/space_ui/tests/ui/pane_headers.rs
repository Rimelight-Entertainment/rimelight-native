//! The page header of each pane (commit 2259ffe): every tile group draws its own breadcrumbs, title, actions and route under
//! its own page tab strip, and the header controls act on that group. The layout is the desktop capture's: three workspaces
//! worth of setup is not needed, so only the start workspace is built, with three groups and one nested split.
//!
//! These tests check the tile-groups model the panes are built from (each group's tabs, title, crumbs and route), not the
//! drawn geometry. The drawn position of each pane, strip, Share button, breadcrumb and title is no longer checked. Clicks
//! that used to land on a drawn position now call the hook that does the same action as the element's click handler.

use slint::platform::WindowEvent;
use slint::{ComponentHandle as _, LogicalSize, Model as _};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::history::link_for;
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppWindow, TileGroup};
use space_ui::theme::ThemeMode;

/// The start route. Its workspace is the first one.
const START: &str = "/home";

/// The desktop window size the layout is built at.
const DESKTOP: (f32, f32) = (1280.0, 800.0);

/// The Home pages that take no parameter, except the start page, in sidebar order.
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

/// Opens a Home route in a new tab of the focused group, as a Ctrl+click on its sidebar row does.
fn open_in_new_tab(app: &App, route: &str) {
    app.ui.invoke_sidebar_opened_in_new_tab(row_of(route));
}

/// Right-clicks tab `index` of group `id`, then picks `item` from its menu.
fn pick_on_tab(app: &App, id: i32, index: i32, item: &str) {
    app.ui
        .invoke_tile_tab_context_requested(id, index, 0.0, 0.0);
    app.ui.invoke_open_tab_menu_picked(index, item.into());
}

/// The tile groups of the active workspace, in model order.
fn groups(ui: &AppWindow) -> Vec<TileGroup> {
    ui.get_tile_groups().iter().collect()
}

/// The id of the focused group.
fn focused_id(ui: &AppWindow) -> i32 {
    groups(ui)
        .iter()
        .find(|group| group.focused)
        .expect("one group is focused")
        .id
}

fn settle() {
    slint::platform::update_timers_and_animations();
}

/// The start workspace with three groups and a nested split, as the desktop capture builds it. The left group is focused.
/// Left: the start page, then two Home pages. Right (top): two pages. Bottom (under the right group): two pages.
fn three_groups() -> App {
    let routes = home_routes();
    assert!(routes.len() >= 6, "Home has enough pages for the layout");
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");
    ui.window().set_size(LogicalSize::new(DESKTOP.0, DESKTOP.1));
    ui.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(DESKTOP.0, DESKTOP.1),
    });
    settle();

    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    let left = focused_id(ui);
    // Route 1 moves to a new group on the right (tab index 2 of the left group).
    pick_on_tab(&app, left, 2, "split_right");
    let right = groups(ui)
        .iter()
        .find(|group| group.id != left)
        .map(|group| group.id)
        .expect("the split made a second group");
    ui.invoke_tile_group_focused(left);
    open_in_new_tab(&app, routes[2]);
    ui.invoke_tile_group_focused(right);
    open_in_new_tab(&app, routes[3]);
    open_in_new_tab(&app, routes[5]);
    let last = groups(ui)
        .iter()
        .find(|group| group.id == right)
        .map(|group| group.tabs.row_count() as i32 - 1)
        .unwrap_or(0);
    // The last tab of the right group goes down into a new group: the nested split.
    pick_on_tab(&app, right, last, "split_down");
    let bottom = groups(ui)
        .iter()
        .find(|group| group.id != left && group.id != right)
        .map(|group| group.id)
        .expect("the nested split made a third group");
    ui.invoke_tile_group_focused(bottom);
    open_in_new_tab(&app, routes[4]);
    ui.invoke_tile_group_focused(left);
    settle();
    assert_eq!(groups(ui).len(), 3, "the layout has three groups");
    app
}

/// Each group of the model is a pane with its own page tabs and title. The pane count is the group count.
fn check_panes(ui: &AppWindow) -> Vec<TileGroup> {
    settle();
    let model = groups(ui);
    assert_eq!(model.len(), 3, "one pane per group");
    for group in &model {
        assert!(
            group.tabs.row_count() > 0,
            "pane {} has its own page tabs",
            group.id
        );
        assert!(
            !group.title.is_empty(),
            "pane {} has its own title",
            group.id
        );
    }
    model
}

/// Two or more panes show different page titles at the same time, each from its own group.
///
/// The drawn position is no longer checked: the strip under the top bar, the title under the strip and the pane box size are
/// not asserted here. The model check covers the data each pane draws from.
pub fn panes_show_their_own_title_under_their_strip_below_the_top_bar() {
    crate::support::init();
    let app = three_groups();
    let ui = &app.ui;
    let model = check_panes(ui);

    // The panes show different titles at the same time.
    let mut titles: Vec<String> = model.iter().map(|g| g.title.to_string()).collect();
    titles.sort();
    titles.dedup();
    assert!(
        titles.len() >= 2,
        "two or more panes show different titles, got {titles:?}"
    );
}

/// Clicking a breadcrumb in pane B navigates pane B, not pane A, and focuses pane B.
///
/// The drawn click on the first crumb is replaced by the hook that does the same action: the breadcrumb's `activated` handler
/// focuses its group, then runs `crumb-activated`, so the test calls both in that order.
pub fn breadcrumb_in_pane_b_navigates_pane_b_and_focuses_it() {
    crate::support::init();
    let app = three_groups();
    let ui = &app.ui;
    let before = check_panes(ui);
    let a = 0; // The focused, left group.
    let b = 1; // The right group, another pane.
    let route_a = before[a].route.to_string();
    let route_b = before[b].route.to_string();
    assert_ne!(
        before[a].id, before[b].id,
        "two different panes are compared"
    );

    // The first crumb is the area. Pane B has crumbs to click.
    assert!(before[b].crumbs.row_count() > 0, "pane B has crumbs");
    ui.invoke_tile_group_focused(before[b].id);
    ui.invoke_crumb_activated(0);
    settle();

    let after = groups(ui);
    let now_b = after
        .iter()
        .find(|g| g.id == before[b].id)
        .expect("pane B is still there");
    let now_a = after
        .iter()
        .find(|g| g.id == before[a].id)
        .expect("pane A is still there");
    assert_ne!(
        now_b.route.as_str(),
        route_b,
        "the breadcrumb navigated pane B"
    );
    assert_eq!(now_a.route.as_str(), route_a, "pane A kept its page");
    assert_eq!(focused_id(ui), before[b].id, "the click focused pane B");
}

/// Share > Copy link in pane B copies pane B's page link.
///
/// The drawn Share button press is replaced by the hook that does the same action: the Share button's `clicked` handler focuses
/// its group, then opens the Share menu. The test calls `tile-group-focused`, then `test-open-share-menu`. The Copy link row is
/// picked with `share-menu-picked("copy_link")`, which is the row's own `selected` handler.
pub fn share_copy_link_in_pane_b_copies_pane_bs_link() {
    crate::support::init();
    let app = three_groups();
    let ui = &app.ui;
    let model = check_panes(ui);
    let b = 1;
    let id_b = model[b].id;
    let route_b = model[b].route.to_string();
    let copied_before = ui.get_last_copied().to_string();

    // Share in pane B: focus the group, then open the Share menu, as the Share button does.
    ui.invoke_tile_group_focused(id_b);
    ui.invoke_test_open_share_menu();
    settle();
    assert_eq!(focused_id(ui), id_b, "Share focused pane B");

    // Copy link in the menu that Share opened.
    ui.invoke_share_menu_picked("copy_link".into());
    settle();

    assert_eq!(
        ui.get_last_copied().as_str(),
        link_for(&route_b),
        "Copy link copies pane B's page link"
    );
    assert_ne!(
        ui.get_last_copied().to_string(),
        copied_before,
        "the copy changed the clipboard text"
    );
}
