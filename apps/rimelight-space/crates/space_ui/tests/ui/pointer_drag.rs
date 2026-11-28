//! Page tab and workspace drags driven by REAL pointer events (`WindowEvent::PointerPressed`, `PointerMoved`,
//! `PointerReleased`, `KeyPressed`) through the testing backend, as shell_capture.rs does for a press. Nothing here invokes
//! a `tile-drag-*` or `workspace-drag-*` callback: every step is what an OS mouse sends, so the TabStrip's own press, move
//! and release handling is part of what is tested. Positions come from the laid out elements, not from the model.
//!
//! Targets are read through the `test-*` geometry hooks of AppWindow (app.slint, components/tabs.slint and
//! components/quick-action.slint), not from the accessibility tree. Labels come from the `tile-groups` and `workspace-tabs`
//! models.

use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle as _, LogicalPosition, Model as _, SharedString};
use space_nav::{Area, pages_in, parse};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::slint_ui::{AppWindow, TileGroup};
use space_ui::theme::ThemeMode;

const START: &str = "/home";

fn home_routes() -> Vec<&'static str> {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .filter(|route| *route != START)
        .collect()
}

fn row_of(route: &str) -> i32 {
    sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .expect("the route is a Home sidebar row") as i32
}

fn title_of(route: &str) -> String {
    parse(route).expect("a known route").page.title.to_owned()
}

fn open_in_new_tab(app: &App, route: &str) {
    app.ui.invoke_sidebar_opened_in_new_tab(row_of(route));
}

/// The shown window, laid out: a snapshot forces the first layout, so the page area reports its real size to the groups.
fn window() -> App {
    crate::support::init();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    app.ui.show().expect("show the window");
    let _ = app.ui.window().take_snapshot();
    slint::platform::update_timers_and_animations();
    app
}

fn groups(ui: &AppWindow) -> Vec<TileGroup> {
    ui.get_tile_groups().iter().collect()
}

fn group(ui: &AppWindow, id: i32) -> TileGroup {
    groups(ui)
        .into_iter()
        .find(|group| group.id == id)
        .unwrap_or_else(|| panic!("no group {id}"))
}

fn focused_group(ui: &AppWindow) -> i32 {
    groups(ui)
        .iter()
        .find(|g| g.focused)
        .expect("one focused group")
        .id
}

fn labels(group: &TileGroup) -> Vec<String> {
    group.tabs.iter().map(|tab| tab.label.to_string()).collect()
}

/// A window rectangle in logical px, from the test hooks.
#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn centre_of(rect: Rect) -> LogicalPosition {
    LogicalPosition::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

/// The page tab labelled `label`, as (row in `tile-groups`, tab index). The labels are read from the group models.
fn page_tab_at(ui: &AppWindow, label: &str) -> (i32, i32) {
    for (row, group) in groups(ui).iter().enumerate() {
        if let Some(index) = group
            .tabs
            .iter()
            .position(|tab| tab.label.as_str() == label)
        {
            return (row as i32, index as i32);
        }
    }
    panic!("no page tab labelled {label:?}");
}

/// The rectangle of the page tab labelled `label`.
fn page_tab(ui: &AppWindow, label: &str) -> Rect {
    let (row, index) = page_tab_at(ui, label);
    Rect {
        x: ui.invoke_test_page_tab_x(row, index),
        y: ui.invoke_test_page_strip_y(row),
        w: ui.invoke_test_page_tab_w(row, index),
        h: ui.invoke_test_page_strip_h(),
    }
}

/// The workspace tab `index` (left to right, as the `workspace-tabs` model).
fn workspace_tab(ui: &AppWindow, index: i32) -> Rect {
    Rect {
        x: ui.invoke_test_ws_tab_x(index),
        y: ui.invoke_test_ws_y(),
        w: ui.invoke_test_ws_tab_w(index),
        h: ui.invoke_test_ws_h(),
    }
}

/// The workspace tabs, left to right: one rectangle per entry of the `workspace-tabs` model.
fn workspace_tabs(ui: &AppWindow) -> Vec<Rect> {
    (0..ui.get_workspace_tabs().row_count())
        .map(|index| workspace_tab(ui, index as i32))
        .collect()
}

/// The index in `tile-groups` of the group with `id`.
fn group_row(ui: &AppWindow, id: i32) -> i32 {
    groups(ui)
        .iter()
        .position(|group| group.id == id)
        .unwrap_or_else(|| panic!("no group {id}")) as i32
}

/// The window position of the top left corner of group `id`: the corner of its first tab (slot 0 starts at the strip's x).
fn group_origin(ui: &AppWindow, id: i32) -> LogicalPosition {
    let row = group_row(ui, id);
    LogicalPosition::new(
        ui.invoke_test_page_tab_x(row, 0),
        ui.invoke_test_page_strip_y(row),
    )
}

/// A point inside group `id`, `dx` and `dy` from its top left corner in window px.
fn in_group(ui: &AppWindow, id: i32, dx: f32, dy: f32) -> LogicalPosition {
    let o = group_origin(ui, id);
    LogicalPosition::new(o.x + dx, o.y + dy)
}

/// The right edge zone of group `id`: 2 px inside its right edge, halfway down the body.
fn right_edge(ui: &AppWindow, id: i32) -> LogicalPosition {
    let g = group(ui, id);
    in_group(ui, id, g.w - 2.0, g.h / 2.0)
}

// Real pointer events, as the OS delivers them to the window.

fn pointer_move(ui: &AppWindow, at: LogicalPosition) {
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position: at });
}

fn pointer_press(ui: &AppWindow, at: LogicalPosition) {
    pointer_move(ui, at);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
}

fn pointer_release(ui: &AppWindow, at: LogicalPosition) {
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

fn key(ui: &AppWindow, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Presses the page tab `label`, then moves 10 px (past the 4 px threshold) and on to `target`, without a release.
fn drag_tab(ui: &AppWindow, label: &str, target: LogicalPosition) {
    let from = centre_of(page_tab(ui, label));
    pointer_press(ui, from);
    pointer_move(ui, LogicalPosition::new(from.x + 10.0, from.y));
    pointer_move(ui, target);
}

/// Splits group 0 by dragging its tab `label` onto its right edge, with real pointer events, and releases.
fn split_right_with_pointer(ui: &AppWindow, label: &str) {
    let edge = right_edge(ui, 0);
    drag_tab(ui, label, edge);
    pointer_release(ui, edge);
}

/// (a) and (d): press the second tab, move past the threshold, move over the right edge zone, and the ghost and the overlay
/// show it; release: the group splits and the tab moves to the new right group.
pub fn press_drag_to_the_edge_splits_with_pointer_events() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    let moved = title_of(routes[0]);
    assert_eq!(labels(&group(ui, 0)).len(), 3, "three tabs in group 0");

    let edge = right_edge(ui, 0);
    drag_tab(ui, &moved, edge);
    assert!(
        ui.get_tile_ghost_visible(),
        "(d) the ghost is visible during the drag"
    );
    assert_eq!(
        ui.get_tile_ghost_label().as_str(),
        moved,
        "(d) the ghost shows the dragged tab"
    );
    assert_eq!(ui.get_drag_zone(), 2, "(d) the right edge overlay is shown");
    assert!(!ui.get_tile_ghost_refused(), "a split is allowed here");

    pointer_release(ui, edge);
    assert_eq!(groups(ui).len(), 2, "(a) the edge drop split the group");
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(START), title_of(routes[1])],
        "(a) the source keeps its other tabs"
    );
    let right = groups(ui)
        .into_iter()
        .find(|g| g.id != 0)
        .expect("the new group");
    assert_eq!(
        labels(&right),
        std::slice::from_ref(&moved),
        "(a) the new group holds the tab"
    );
    assert!(
        !ui.get_tile_ghost_visible(),
        "the ghost is gone after the drop"
    );
    assert_eq!(ui.get_drag_zone(), 0, "the overlay is gone after the drop");
}

/// (b) Press a tab and move it within its strip: the mark shows the slot, and the drop reorders the strip.
pub fn press_drag_within_a_strip_reorders_with_pointer_events() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    let start = title_of(START);
    let last = centre_of(page_tab(ui, &title_of(routes[1])));

    // Past the centre of the last tab: the insertion mark is the end slot.
    let target = LogicalPosition::new(last.x + 30.0, last.y);
    drag_tab(ui, &start, target);
    assert_eq!(ui.get_drag_mark(), 3, "(b) the mark is after the last tab");
    pointer_release(ui, target);
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(routes[0]), title_of(routes[1]), start],
        "(b) the pressed tab moved to the end"
    );
    assert_eq!(ui.get_drag_mark(), -1, "no mark after the drop");
}

/// (c) Press a tab and drop it on another group's strip: it joins that strip.
pub fn press_drag_onto_another_groups_strip_with_pointer_events() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    open_in_new_tab(&app, routes[1]);
    split_right_with_pointer(ui, &title_of(routes[1]));
    let right = groups(ui)
        .into_iter()
        .find(|g| g.id != 0)
        .expect("the new group")
        .id;

    let target = in_group(ui, right, 200.0, 10.0);
    drag_tab(ui, &title_of(START), target);
    pointer_release(ui, target);
    assert_eq!(
        labels(&group(ui, right)),
        [title_of(routes[1]), title_of(START)],
        "(c) the tab joined the other group's strip"
    );
    assert_eq!(
        labels(&group(ui, 0)),
        [title_of(routes[0])],
        "(c) the source keeps its tab"
    );
    assert_eq!(focused_group(ui), right, "(c) the target group takes focus");
}

/// (e) Escape during a drag cancels it: the release that follows changes nothing.
pub fn escape_during_a_pointer_drag_cancels_it() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    let edge = right_edge(ui, 0);
    drag_tab(ui, &title_of(routes[0]), edge);
    assert!(ui.get_tile_ghost_visible(), "the drag is running");
    key(ui, Key::Escape);
    assert!(!ui.get_tile_ghost_visible(), "(e) Escape removes the ghost");
    assert_eq!(ui.get_drag_zone(), 0, "(e) Escape removes the overlay");
    pointer_release(ui, edge);
    assert_eq!(
        groups(ui).len(),
        1,
        "(e) the release after Escape does nothing"
    );
    assert_eq!(labels(&group(ui, 0)).len(), 2, "(e) both tabs stay");
}

/// (f) A release outside every group (above the page area, on the tab bar and workspace row) cancels.
pub fn release_outside_every_group_cancels_a_pointer_drag() {
    let routes = home_routes();
    let app = window();
    let ui = &app.ui;
    open_in_new_tab(&app, routes[0]);
    let outside = LogicalPosition::new(centre_of(page_tab(ui, &title_of(START))).x + 300.0, 2.0);
    drag_tab(ui, &title_of(routes[0]), outside);
    pointer_release(ui, outside);
    assert_eq!(groups(ui).len(), 1, "(f) a release outside does nothing");
    assert_eq!(labels(&group(ui, 0)).len(), 2, "(f) both tabs stay");
    assert!(!ui.get_tile_ghost_visible(), "(f) the ghost is gone");
}

/// (g) A workspace tab dragged in the workspace strip with pointer events moves to the first slot.
pub fn press_drag_reorders_the_workspace_strip_with_pointer_events() {
    let app = window();
    let ui = &app.ui;
    ui.invoke_new_workspace_requested();
    ui.invoke_new_workspace_requested();
    assert_eq!(workspace_tabs(ui).len(), 3, "three workspace tabs");
    assert_eq!(ui.get_workspace_active(), 2, "the newest is active");

    let first = centre_of(workspace_tabs(ui)[0]);
    let from = centre_of(workspace_tabs(ui)[2]);
    pointer_press(ui, from);
    pointer_move(ui, LogicalPosition::new(from.x - 10.0, from.y));
    let target = LogicalPosition::new(first.x - 20.0, first.y);
    pointer_move(ui, target);
    assert_eq!(
        ui.get_workspace_drop_mark(),
        0,
        "(g) the mark is the first slot"
    );
    pointer_release(ui, target);
    assert_eq!(
        ui.get_workspace_drop_mark(),
        -1,
        "(g) no mark after the drop"
    );
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "(g) the moved workspace is first and active"
    );
}

// Tooltips obey one central rule (TooltipHub, ui/components/tooltip.slint). These tests use real pointer and key events.

fn advance(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(ms));
}

/// The quick action button's window rectangle (test hooks of QuickAction, forwarded by AppWindow).
fn quick_button(ui: &AppWindow) -> Rect {
    Rect {
        x: ui.get_test_quick_x(),
        y: ui.get_test_quick_y(),
        w: ui.get_test_quick_w(),
        h: ui.get_test_quick_h(),
    }
}

fn pinned_tab_title(ui: &AppWindow, app: &App) -> String {
    let routes = home_routes();
    open_in_new_tab(app, routes[0]);
    open_in_new_tab(app, routes[1]);
    ui.invoke_open_tab_menu_picked(1, "pin".into());
    let first = ui.get_open_tabs().row_data(0).expect("a first tab");
    assert!(first.pinned, "the tab moved to the front is pinned");
    first.label.to_string()
}

/// Bug 1: a hover tooltip on a pinned tab goes when a press and drag start, and stays gone during and after the drop.
pub fn pinned_tab_tooltip_goes_on_press_and_drag_and_stays_gone() {
    let app = window();
    let ui = &app.ui;
    let title = pinned_tab_title(ui, &app);
    let centre = centre_of(page_tab(ui, &title));
    let away = LogicalPosition::new(centre.x + 400.0, centre.y + 300.0);

    pointer_move(ui, centre);
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        title,
        "hovering a pinned tab shows its title"
    );
    pointer_press(ui, centre);
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        "",
        "a press hides the tooltip"
    );
    pointer_move(ui, LogicalPosition::new(centre.x + 10.0, centre.y));
    pointer_move(ui, LogicalPosition::new(centre.x + 80.0, centre.y));
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        "",
        "it stays gone during the drag"
    );
    pointer_release(ui, LogicalPosition::new(centre.x + 80.0, centre.y));
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        "",
        "it stays gone after the drop"
    );

    // Leave and re-enter: it shows again.
    pointer_move(ui, away);
    let title = ui
        .get_open_tabs()
        .iter()
        .find(|tab| tab.pinned)
        .expect("the pinned tab")
        .label
        .to_string();
    let centre = centre_of(page_tab(ui, &title));
    pointer_move(ui, centre);
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        title,
        "re-entering the tab shows the tooltip again"
    );

    // A right press opens the tab menu and no tooltip shows with it.
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: centre,
        button: PointerEventButton::Right,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: centre,
        button: PointerEventButton::Right,
    });
    assert!(ui.get_tab_menu_open(), "the context menu is open");
    assert_eq!(
        ui.get_pinned_tip_text().as_str(),
        "",
        "no tooltip with an open context menu"
    );
}

/// Bug 2: the quick action tooltip shows on hover, goes on click, and does not come back when the menu closes and gives
/// the focus back to the button.
pub fn quick_action_tooltip_does_not_return_after_a_menu_pick() {
    let app = window();
    let ui = &app.ui;
    let centre = centre_of(quick_button(ui));

    pointer_move(ui, centre);
    advance(500);
    assert!(ui.get_quick_tooltip_shown(), "hover shows the tooltip");
    pointer_press(ui, centre);
    pointer_release(ui, centre);
    advance(500);
    assert!(!ui.get_quick_tooltip_shown(), "a click hides the tooltip");

    // The click opened the menu (its items are in the tree then).
    assert!(ui.get_quick_menu_open(), "the menu is open");
    // The "New Chat" item is inside the menu's popup, whose items have popup-local positions, so the pick runs the item's
    // own handler through the test hook (fire the pick, close the menu, focus the button) rather than a pointer event.
    ui.invoke_test_pick_quick_item("new_chat".into());
    advance(1000);
    assert_eq!(
        app.last_quick_pick.borrow().as_deref(),
        Some("new_chat"),
        "the pick fired"
    );
    assert!(!ui.get_quick_menu_open(), "the menu closed");
    assert!(
        !ui.get_quick_tooltip_shown(),
        "the tooltip stays hidden after the menu closes and the focus returns to the button"
    );

    // Leaving and re-entering the button shows it again.
    pointer_move(ui, LogicalPosition::new(5.0, 400.0));
    pointer_move(ui, centre);
    advance(500);
    assert!(
        ui.get_quick_tooltip_shown(),
        "re-entering the button shows the tooltip"
    );

    // Keyboard navigation: Tab onto the button shows its tooltip.
    pointer_move(ui, LogicalPosition::new(5.0, 400.0));
    advance(100);
    assert!(!ui.get_quick_tooltip_shown(), "the pointer left");
    let mut shown = false;
    for _ in 0..80 {
        key(ui, Key::Tab);
        if ui.get_quick_tooltip_shown() {
            shown = true;
            break;
        }
    }
    assert!(shown, "Tab focus on the button shows its tooltip");
    key(ui, Key::Return);
    advance(100);
    key(ui, Key::Escape);
    assert!(
        !ui.get_quick_tooltip_shown(),
        "keys hide the tooltip once the menu is closed again"
    );
}

/// The "New workspace" button's window rectangle (its x and width are read; the row gives its y and height).
fn plus_button(ui: &AppWindow) -> Rect {
    Rect {
        x: ui.get_test_plus_x(),
        y: ui.invoke_test_ws_y(),
        w: ui.get_test_plus_w(),
        h: ui.invoke_test_ws_h(),
    }
}

/// Step C: the "+" sits right after the last workspace tab, and stays pinned at the right edge only on overflow.
pub fn workspace_plus_button_follows_the_last_tab_and_pins_right_on_overflow() {
    let app = window();
    let ui = &app.ui;
    let window_w = ui.window().size().width as f32 / ui.window().scale_factor();
    for count in [1usize, 3, 10] {
        let have = workspace_tabs(ui).len();
        for _ in have..count {
            ui.invoke_new_workspace_requested();
        }
        let _ = ui.window().take_snapshot();
        slint::platform::update_timers_and_animations();
        let plus = plus_button(ui);
        let plus_right = plus.x + plus.w;
        assert!(
            plus_right <= window_w + 0.5,
            "{count} workspaces: the + stays inside the window"
        );
        if count < 10 {
            let tabs = workspace_tabs(ui);
            assert_eq!(tabs.len(), count);
            let last = tabs.last().expect("a tab");
            let gap = plus.x - (last.x + last.w);
            assert!(
                (0.0..=2.0).contains(&gap),
                "{count} workspaces: the + is right after the last tab (gap {gap}, window {window_w})"
            );
        } else {
            // Overflow: the tabs fill the row (the rest are clipped) and the + is pinned at the right edge.
            assert!(
                window_w - plus_right <= 1.0,
                "{count} workspaces overflow the strip: the + sits at the right edge (right {plus_right}, window {window_w})"
            );
        }
    }
}

/// The centre of handle `index` in window px (the handle rows are in page area px, whose origin is group 0's corner).
fn handle_centre(ui: &AppWindow, index: usize) -> LogicalPosition {
    let o = group_origin(ui, 0);
    let h = ui.get_tile_handles().row_data(index).expect("a handle");
    LogicalPosition::new(o.x + h.x + h.w / 2.0, o.y + h.y + h.h / 2.0)
}

fn two_groups(ui: &AppWindow) -> (TileGroup, TileGroup) {
    let all = groups(ui);
    assert_eq!(all.len(), 2, "two groups");
    (all[0].clone(), all[1].clone())
}

/// Step D: dragging a split handle with real pointer events resizes the groups live, in several moves, to the clamp.
pub fn split_handle_drag_resizes_the_groups_live_with_pointer_events() {
    let routes = home_routes();
    for horizontal in [true, false] {
        let app = window();
        let ui = &app.ui;
        open_in_new_tab(&app, routes[0]);
        // Side by side (right edge) or stacked (bottom edge).
        let g = group(ui, 0);
        let target = if horizontal {
            right_edge(ui, 0)
        } else {
            in_group(ui, 0, g.w / 2.0, g.h - 2.0)
        };
        let label = title_of(routes[0]);
        let from = centre_of(page_tab(ui, &label));
        pointer_press(ui, from);
        pointer_move(ui, LogicalPosition::new(from.x + 10.0, from.y));
        pointer_move(ui, target);
        pointer_release(ui, target);
        assert_eq!(groups(ui).len(), 2, "the drop split the group");

        let size = |g: &TileGroup| if horizontal { g.w } else { g.h };
        let (a0, b0) = two_groups(ui);
        let (first0, second0) = (size(&a0), size(&b0));
        let start = handle_centre(ui, 0);
        pointer_press(ui, start);
        let mut at = start;
        let mut last_first = first0;
        for _ in 0..4 {
            at = if horizontal {
                LogicalPosition::new(at.x + 50.0, at.y)
            } else {
                LogicalPosition::new(at.x, at.y + 50.0)
            };
            pointer_move(ui, at);
            let (a, b) = two_groups(ui);
            assert!(
                size(&a) > last_first + 40.0,
                "{horizontal}: the first group grows live with each move ({} then {})",
                last_first,
                size(&a)
            );
            assert!(
                (size(&a) + size(&b) - (first0 + second0)).abs() < 1.5,
                "{horizontal}: the groups keep the total"
            );
            last_first = size(&a);
        }
        pointer_release(ui, at);
        let (a, b) = two_groups(ui);
        assert!(
            (size(&a) - (first0 + 200.0)).abs() < 3.0,
            "{horizontal}: dragged 200 px, the first group grew by 200 ({} to {})",
            first0,
            size(&a)
        );
        assert!(
            size(&b) < second0 - 190.0,
            "{horizontal}: the second shrank"
        );

        // Far past the end: clamped at 0.9. Far back: clamped at 0.1.
        let pos = handle_centre(ui, 0);
        pointer_press(ui, pos);
        let far = if horizontal {
            LogicalPosition::new(pos.x + 5000.0, pos.y)
        } else {
            LogicalPosition::new(pos.x, pos.y + 5000.0)
        };
        pointer_move(ui, LogicalPosition::new(pos.x + 5.0, pos.y + 5.0));
        pointer_move(ui, far);
        pointer_release(ui, far);
        let (a, b) = two_groups(ui);
        let ratio = size(&a) / (size(&a) + size(&b));
        assert!(
            (ratio - 0.9).abs() < 0.01,
            "{horizontal}: clamped at 0.9, got {ratio}"
        );
        let pos = handle_centre(ui, 0);
        pointer_press(ui, pos);
        let near = if horizontal {
            LogicalPosition::new(pos.x - 5000.0, pos.y)
        } else {
            LogicalPosition::new(pos.x, pos.y - 5000.0)
        };
        pointer_move(ui, LogicalPosition::new(pos.x - 5.0, pos.y - 5.0));
        pointer_move(ui, near);
        pointer_release(ui, near);
        let (a, b) = two_groups(ui);
        let ratio = size(&a) / (size(&a) + size(&b));
        assert!(
            (ratio - 0.1).abs() < 0.01,
            "{horizontal}: clamped at 0.1, got {ratio}"
        );
    }
}
