//! Page tab drags in the window (planned feature B, tiles, T2b; `PROJECT.md section 6.6` "Tile groups"). The markup reports
//! the pointer of a tab drag to `tile-drag-moved` and `tile-drag-dropped` (page area px), and Escape to `tile-drag-escaped`.
//! The tests call those callbacks in the order a real drag sends them (moves, then the release): the pointer itself is not
//! driven. Each test builds its own window.

use slint::{ComponentHandle as _, Model as _};
use space_nav::{Area, pages_in, parse};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppWindow, TileGroup};
use space_ui::theme::ThemeMode;

/// The start route. Its workspace is the first one, and its tab is the first tab of the first group.
const START: &str = "/home";

/// The page area the tests lay out in: a 1000 px by 600 px area, so one group is 1000 px wide and a split in two is 497 px
/// and 497 px (the bar is 6 px thick).
const AREA_W: f32 = 1000.0;
const AREA_H: f32 = 600.0;

/// The routes of the Home area that have a page other than the start route, in sidebar order.
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

/// Opens a Home route in a new tab of the focused group, as a Ctrl+click does.
fn open_in_new_tab(app: &App, route: &str) {
    app.ui.invoke_sidebar_opened_in_new_tab(row_of(route));
}

/// The tile groups of the active workspace, in model order.
fn groups(ui: &AppWindow) -> Vec<TileGroup> {
    ui.get_tile_groups().iter().collect()
}

/// The group with id `id`.
fn group(ui: &AppWindow, id: i32) -> TileGroup {
    groups(ui)
        .into_iter()
        .find(|group| group.id == id)
        .unwrap_or_else(|| panic!("no group {id}"))
}

/// The id of the focused group.
fn focused_id(ui: &AppWindow) -> i32 {
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

/// The window with a laid out page area, one group and the start page.
fn window() -> App {
    crate::support::init();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    app.ui.show().expect("show the window");
    app.ui.invoke_tile_area_resized(AREA_W, AREA_H);
    app
}

/// A drag of tab `index` of group `source` that is moved to page area point (`x`, `y`) and released there.
fn drag_to(app: &App, source: i32, index: i32, x: f32, y: f32) {
    app.ui.invoke_tile_drag_moved(source, index, x, y);
    app.ui.invoke_tile_drag_dropped(source, index, x, y);
}

/// The tile drag reports the pointer, without a release: the overlay and the ghost show what a drop there would do.
fn hover(app: &App, source: i32, index: i32, x: f32, y: f32) {
    app.ui.invoke_tile_drag_moved(source, index, x, y);
}

/// A tab drag inside one strip reorders the strip: the insertion mark shows the slot, and the drop moves the tab there.
pub fn tab_drag_reorders_within_a_strip() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(START), title_of(routes[0]), title_of(routes[1])],
        "three tabs in the one group"
    );

    // The Start tab is dragged past the last tab (x 400 is past the third 136 px slot's middle): the mark is the end slot.
    hover(&app, 0, 0, 400.0, 10.0);
    assert_eq!(ui.get_drag_mark(), 3, "the mark is after the last tab");
    drag_to(&app, 0, 0, 400.0, 10.0);
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(routes[0]), title_of(routes[1]), title_of(START)],
        "the Start tab moved to the end"
    );
    assert_eq!(ui.get_drag_mark(), -1, "no mark after the drop");
    assert_eq!(focused_id(ui), 0, "the strip keeps its group focused");
}

/// Dragging a tab to the right edge of its group splits the group: the tab becomes the new right group, which takes focus.
pub fn tab_drag_to_an_edge_splits_the_group() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);

    // The pointer in the right 25% of the body (x 990 of 1000, y 300 below the strip): the preview is the right edge.
    hover(&app, 0, 2, 990.0, 300.0);
    assert_eq!(ui.get_drag_zone(), 2, "the right edge is previewed");
    assert!(ui.get_tile_ghost_visible(), "the ghost follows the drag");
    assert_eq!(
        ui.get_tile_ghost_label().as_str(),
        title_of(routes[1]),
        "the ghost is the dragged tab's label"
    );
    assert!(!ui.get_tile_ghost_refused(), "a split is allowed");

    drag_to(&app, 0, 2, 990.0, 300.0);
    assert_eq!(groups(ui).len(), 2, "the split made a second group");
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(START), title_of(routes[0])],
        "the source keeps its other two tabs"
    );
    let right = groups(ui)
        .into_iter()
        .find(|group| group.id != 0)
        .expect("the new group");
    assert_eq!(
        labels(&right),
        [title_of(routes[1])],
        "the new group holds the tab"
    );
    assert_eq!(focused_id(ui), right.id, "the new group takes focus");
    assert_eq!(ui.get_drag_zone(), 0, "the preview is gone after the drop");
    assert!(
        !ui.get_tile_ghost_visible(),
        "the ghost is gone after the drop"
    );
}

/// A drop on another group's strip moves the tab there at the pointer's slot. The emptied source keeps its other tab, and
/// the target group takes focus.
pub fn tab_drag_onto_another_groups_strip_moves_the_tab() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    drag_to(&app, 0, 2, 990.0, 300.0);
    let right = groups(ui)
        .into_iter()
        .find(|group| group.id != 0)
        .expect("the new group")
        .id;

    // The Start tab (group 0, index 0) goes onto the new group's strip, past its only tab.
    let strip_x = group(ui, right).x + 200.0;
    drag_to(&app, 0, 0, strip_x, 10.0);
    assert_eq!(
        labels(&group(ui, right)),
        [title_of(routes[1]), title_of(START)],
        "the Start tab joins the new group's strip after its tab"
    );
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(routes[0])],
        "the source keeps its remaining tab"
    );
    assert_eq!(focused_id(ui), right, "the target group takes focus");
}

/// Edge drops that would break the rules are refused, and change nothing: a group's only tab on its own edge, and the
/// seventh group. The centre of a group's own body is a no-op too.
pub fn tab_drag_refuses_the_only_tab_on_its_own_edge_and_the_cap() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;

    // One tab on its own right edge: refused, the ghost says so, and the layout is the same after the release.
    hover(&app, 0, 0, 990.0, 300.0);
    assert!(
        ui.get_tile_ghost_refused(),
        "the only tab cannot split its group"
    );
    assert!(ui.get_drag_refused(), "the edge shows the refusal");
    drag_to(&app, 0, 0, 990.0, 300.0);
    assert_eq!(groups(ui).len(), 1, "no split");
    assert_eq!(labels(&group(ui, 0)), [title_of(START)], "the tab stays");

    // The centre of its own group does nothing.
    drag_to(&app, 0, 0, 500.0, 300.0);
    assert_eq!(groups(ui).len(), 1, "the own centre is a no-op");
    assert_eq!(labels(&group(ui, 0)), [title_of(START)], "the tab stays");

    // Seven tabs in group 0, then five splits take it to six groups. Each split drops the second tab on the right edge of
    // group 0 (read from the layout, since the group narrows each time).
    for route in &routes[..6] {
        open_in_new_tab(&app, route);
    }
    assert_eq!(group(ui, 0).tabs.row_count(), 7, "seven tabs in group 0");
    for _ in 0..5 {
        let g = group(ui, 0);
        drag_to(&app, 0, 1, g.x + g.w - 2.0, g.y + g.h / 2.0);
    }
    assert_eq!(groups(ui).len(), 6, "six groups, the cap");
    assert_eq!(group(ui, 0).tabs.row_count(), 2, "two tabs left in group 0");

    // A seventh group is refused, with the not-allowed state while hovering.
    let g = group(ui, 0);
    hover(&app, 0, 1, g.x + g.w - 2.0, g.y + g.h / 2.0);
    assert!(ui.get_tile_ghost_refused(), "the cap refuses the split");
    drag_to(&app, 0, 1, g.x + g.w - 2.0, g.y + g.h / 2.0);
    assert_eq!(groups(ui).len(), 6, "the cap holds");
    assert_eq!(group(ui, 0).tabs.row_count(), 2, "the tab stays in group 0");
}

/// Escape cancels a drag: the overlay and the ghost go, and the release that follows does nothing. A release outside every
/// group cancels too.
pub fn tab_drag_escape_and_a_release_outside_cancel() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);

    hover(&app, 0, 1, 990.0, 300.0);
    assert!(ui.get_tile_ghost_visible(), "the drag is running");
    ui.invoke_tile_drag_escaped();
    assert!(!ui.get_tile_ghost_visible(), "Escape removes the ghost");
    assert_eq!(ui.get_drag_zone(), 0, "Escape removes the preview");
    ui.invoke_tile_drag_dropped(0, 1, 990.0, 300.0);
    assert_eq!(groups(ui).len(), 1, "the release after Escape does nothing");
    assert_eq!(labels(&group(ui, 0)).len(), 2, "both tabs stay");

    // A release outside every group (past the right edge of the page area) cancels.
    drag_to(&app, 0, 1, AREA_W + 200.0, 300.0);
    assert_eq!(groups(ui).len(), 1, "a release outside does nothing");
    assert_eq!(labels(&group(ui, 0)).len(), 2, "both tabs stay");
    assert!(!ui.get_tile_ghost_visible(), "the ghost is gone");
}

/// A workspace tab dragged in the strip follows the pointer with its insertion mark, and the drop moves it. The active
/// workspace stays the active one (here it is the moved workspace).
pub fn workspace_strip_drag_reorders_the_workspaces() {
    let app = window();
    let ui = &app.ui;
    ui.invoke_new_workspace_requested();
    ui.invoke_new_workspace_requested();
    assert_eq!(ui.get_workspace_tabs().row_count(), 3, "three workspaces");
    assert_eq!(ui.get_workspace_active(), 2, "the newest is active");

    // The last workspace dragged to the start of the strip: the mark is slot 0, and the drop moves it there.
    ui.invoke_workspace_drag_moved(2, 10.0, 10.0);
    assert_eq!(
        ui.get_workspace_drop_mark(),
        0,
        "the mark is the first slot"
    );
    ui.invoke_workspace_drag_dropped(2, 10.0, 10.0);
    assert_eq!(ui.get_workspace_drop_mark(), -1, "no mark after the drop");
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "the moved workspace is still the active one, now first"
    );
}
