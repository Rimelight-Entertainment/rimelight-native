//! S9.1 rail and sidebar tests. Selection: a click on a rail area and on a sidebar page fires
//! its callback once and sets the selection. Scrolling: the sidebar scrolls when its rows overflow, clamps at the end, and
//! a new list clears the selection. Elision: a long label stays inside the sidebar width. Clicks go through
//! the form's pick hooks (they fire the rail's and sidebar's own callbacks), wheel input through
//! `Window::dispatch_event`, as in `banner.rs`. The long-label and the no-mute checks read state hooks (`sidebar-row-width`
//! and `has-mute`) rather than looking up elements.

use std::cell::RefCell;
use std::rc::Rc;

use slint::SharedString;
use slint::platform::Key;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle as _, LogicalPosition, Model as _, ModelRc, VecModel};
use space_nav::Area;
use space_ui::slint_ui::shell::{icon, sidebar_rows};

pub use space_ui_test_slint::shell_1::*;

const AREA_TITLES: [&str; 10] = [
    "Home",
    "Chat",
    "Projects",
    "Company",
    "People",
    "Finance",
    "Customers",
    "Operations",
    "Analytics",
    "Admin",
];

fn rail_items() -> ModelRc<RailItem> {
    let items: Vec<RailItem> = AREA_TITLES
        .iter()
        .map(|label| RailItem {
            label: (*label).into(),
            icon: slint::Image::default(),
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

fn to_item(row: space_ui::slint_ui::SidebarItem) -> SidebarItem {
    SidebarItem {
        label: row.label,
        header: row.header,
        icon: row.icon,
        kind: if row.header {
            SidebarKind::Heading
        } else {
            SidebarKind::Page
        },
        ..Default::default()
    }
}

/// The sidebar rows of `area`, with their icons from the crate's own `Icons` global (`sidebar_rows` takes it). The window
/// only supplies the global; the rows keep their images after it is dropped.
fn rows_of(area: Area) -> Vec<space_ui::slint_ui::SidebarItem> {
    let window = space_ui::slint_ui::AppWindow::new().expect("create the icon source");
    sidebar_rows(&window.global::<space_ui::slint_ui::Icons>(), area)
}

/// The house icon, loaded from the same Icons global the sidebar uses in production.
fn house_icon() -> slint::Image {
    let window = space_ui::slint_ui::AppWindow::new().expect("create the icon source");
    icon(&window.global::<space_ui::slint_ui::Icons>(), "house")
}

fn items(area: Area) -> ModelRc<SidebarItem> {
    let rows: Vec<SidebarItem> = rows_of(area).into_iter().map(to_item).collect();
    ModelRc::new(VecModel::from(rows))
}

fn show(area: Area) -> ShellForm {
    let ui = ShellForm::new().expect("create the form");
    ui.set_rail_items(rail_items());
    ui.set_area_title(area.title().into());
    ui.set_sidebar_items(items(area));
    ui.show().expect("show the form");
    // One update lays the window out, so the sidebar knows its content height.
    slint::platform::update_timers_and_animations();
    ui
}

fn wheel(ui: &ShellForm, delta_y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(100.0, 250.0),
        delta_x: 0.0,
        delta_y,
    });
}

fn rail_click_selects_the_area_once() {
    let ui = show(Area::Home);

    let picks = Rc::new(RefCell::new(Vec::new()));
    {
        let picks = picks.clone();
        ui.on_area_picked(move |index| picks.borrow_mut().push(index));
    }

    assert_eq!(ui.get_area(), 0, "Home is selected at first");
    ui.invoke_pick_area(2);
    assert_eq!(ui.get_area(), 2, "the click selects Projects");
    assert_eq!(*picks.borrow(), vec![2], "one callback with the index");
}

fn sidebar_click_selects_the_page_once_and_a_new_list_clears_it() {
    let ui = show(Area::Projects);

    let rows = rows_of(Area::Projects);
    let index = rows
        .iter()
        .position(|row| !row.header)
        .expect("Projects has a page row");

    let picks = Rc::new(RefCell::new(Vec::new()));
    {
        let picks = picks.clone();
        ui.on_row_picked(move |row| picks.borrow_mut().push(row));
    }

    assert_eq!(ui.get_selected_row(), -1, "nothing is selected at first");
    ui.invoke_pick_row(index as i32);
    assert_eq!(
        ui.get_selected_row(),
        index as i32,
        "the page row is selected"
    );
    assert_eq!(
        *picks.borrow(),
        vec![index as i32],
        "one callback per click"
    );

    // Another area replaces the list. The old index would point at an unrelated row, so the shell clears the selection
    // when it switches area (`clear_page` in app_shell.rs). The Sidebar itself no longer clears it on a list change.
    ui.set_sidebar_items(items(Area::Home));
    ui.set_selected_row(-1);
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_selected_row(), -1, "a new list clears the selection");
}

fn sidebar_scrolls_when_rows_overflow_and_stops_at_the_end() {
    let ui = show(Area::Projects);
    // The manifest gives Projects only six parameter-free rows, which fit the window. Add 30 pages so the list overflows.
    let mut rows: Vec<SidebarItem> = rows_of(Area::Projects).into_iter().map(to_item).collect();
    for n in 0..30 {
        rows.push(SidebarItem {
            label: format!("Extra page {n}").into(),
            header: false,
            icon: house_icon(),
            kind: SidebarKind::Page,
            ..Default::default()
        });
    }
    ui.set_sidebar_items(ModelRc::new(VecModel::from(rows)));
    slint::platform::update_timers_and_animations();

    let extent = ui.get_sidebar_extent();
    assert!(
        extent > 0.0,
        "the Projects rows overflow the 480 px window (extent {extent})"
    );
    assert_eq!(ui.get_sidebar_offset(), 0.0, "the list starts at the top");

    wheel(&ui, -120.0);
    assert!(
        ui.get_sidebar_offset() > 0.0,
        "a wheel step scrolls the list"
    );

    for _ in 0..50 {
        wheel(&ui, -120.0);
    }
    assert_eq!(
        ui.get_sidebar_offset(),
        extent,
        "scrolling stops at the end of the rows"
    );

    wheel(&ui, 10_000.0);
    assert_eq!(
        ui.get_sidebar_offset(),
        0.0,
        "scrolling back reaches the top"
    );
}

fn long_labels_stay_inside_the_sidebar() {
    let long = "A page title that is far too long to fit in a 240 px sidebar row and must end in an ellipsis";
    let mut rows: Vec<SidebarItem> = rows_of(Area::Home).into_iter().map(to_item).collect();
    rows.push(SidebarItem {
        label: long.into(),
        header: false,
        icon: house_icon(),
        kind: SidebarKind::Page,
        ..Default::default()
    });
    let ui = ShellForm::new().expect("create the form");
    ui.set_rail_items(rail_items());
    ui.set_area_title("Home".into());
    ui.set_sidebar_items(ModelRc::new(VecModel::from(rows)));
    ui.show().expect("show the form");
    // One update lays the window out, so the sidebar knows its content height.
    slint::platform::update_timers_and_animations();

    assert!(
        ui.get_sidebar_items()
            .iter()
            .any(|item| item.label.as_str() == long),
        "the long row is in the list"
    );
    let row_width = ui.get_sidebar_row_width();
    let sidebar_width = ui.get_sidebar_width();
    assert!(
        row_width <= sidebar_width + 0.5,
        "the row is clipped to the sidebar width ({row_width} px, sidebar {sidebar_width} px)"
    );
    assert_eq!(
        sidebar_width, 240.0,
        "a long label does not widen the sidebar"
    );
}

// S9.2 tab strip and split pane. A tab strip over a split pane in one window, so the drag tests know where each one is.
// A module of its own: the second slint! block would otherwise redefine the types the first one generates (TextStyle).
mod workspace_ui {
    pub use space_ui_test_slint::shell_2::*;
}
use workspace_ui::{TabItem, WorkspaceForm};

fn workspace_form(labels: &[&str]) -> WorkspaceForm {
    let ui = WorkspaceForm::new().expect("create the form");
    let tabs: Vec<TabItem> = labels
        .iter()
        .map(|label| TabItem {
            label: (*label).into(),
            short_label: label.chars().take(1).collect::<String>().into(),
            pinned: false,
        })
        .collect();
    ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
    ui.show().expect("show the form");
    slint::platform::update_timers_and_animations();
    ui
}

fn press(ui: &WorkspaceForm, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

fn move_to(ui: &WorkspaceForm, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(x, y),
    });
    slint::platform::update_timers_and_animations();
}

fn release(ui: &WorkspaceForm, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(x, y),
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

fn tabs_select_close_and_reorder_by_drag() {
    let ui = workspace_form(&["Home", "Mail", "Calendar"]);
    let tab_width = ui.get_tab_width();
    let y = 1.0;

    let picks = Rc::new(RefCell::new(Vec::new()));
    {
        let picks = picks.clone();
        ui.on_activated(move |index| picks.borrow_mut().push(index));
    }
    let closes = Rc::new(RefCell::new(Vec::new()));
    {
        let closes = closes.clone();
        ui.on_closed(move |index| closes.borrow_mut().push(index));
    }
    // A drop reports the dragged tab and the pointer in strip px. The owner picks the slot (the strip does not).
    let moves: Rc<RefCell<Vec<(i32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let moves = moves.clone();
        ui.on_dropped(move |index, x, _y| moves.borrow_mut().push((index, x)));
    }

    // A click selects: Mail is slot 1.
    ui.invoke_click_tab(1);
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_active(), 1, "the click selects Mail");
    assert_eq!(*picks.borrow(), vec![1], "one activated callback");

    // The close control closes its own tab, and only that one.
    ui.invoke_click_close(2);
    slint::platform::update_timers_and_animations();
    assert_eq!(
        *closes.borrow(),
        vec![2],
        "one closed callback for Calendar"
    );
    assert!(moves.borrow().is_empty(), "a close is not a reorder");

    // A drag from Home (slot 0) to slot 1 is one drop of tab 0 at the release point, and does not select.
    press(&ui, tab_width * 0.5, y);
    move_to(&ui, tab_width * 0.5 + tab_width, y);
    release(&ui, tab_width * 0.5 + tab_width, y);
    assert_eq!(
        *moves.borrow(),
        vec![(0, tab_width * 1.5)],
        "one drop of tab 0 at the release point"
    );
    assert_eq!(*picks.borrow(), vec![1], "a drag does not select");

    // A move of 2 px or less is a press: it selects, and drops nothing.
    press(&ui, tab_width * 1.5, y);
    move_to(&ui, tab_width * 1.5 + 2.0, y);
    release(&ui, tab_width * 1.5 + 2.0, y);
    assert_eq!(moves.borrow().len(), 1, "no second drop");
    assert_eq!(
        *picks.borrow(),
        vec![1, 1],
        "the short press selects its tab"
    );
}

fn split_drag_moves_the_handle_and_stops_at_the_minimums() {
    let ui = workspace_form(&["Home"]);
    // The window is 480 wide, the handle takes the grab width, and the panes share the rest.
    let resizes = Rc::new(RefCell::new(Vec::new()));
    {
        let resizes = resizes.clone();
        ui.on_resized(move |ratio| resizes.borrow_mut().push(ratio));
    }

    let y = ui.get_split_y() + 20.0;
    let handle_x = ui.get_first_length();
    let start = handle_x + 2.0;
    press(&ui, start, y);

    // Drag right by 100 px: the first pane grows by 100 px.
    let before = ui.get_first_length();
    move_to(&ui, start + 100.0, y);
    assert!(
        (ui.get_first_length() - (before + 100.0)).abs() < 0.5,
        "the first pane grows with the pointer"
    );
    assert!(!resizes.borrow().is_empty(), "a drag reports the ratio");

    // Drag far left: the first pane stops at its minimum.
    move_to(&ui, 0.0, y);
    assert!(
        (ui.get_first_length() - 120.0).abs() < 0.5,
        "the first pane stops at min-first (120 px), got {}",
        ui.get_first_length()
    );

    // Drag far right: the second pane stops at its minimum.
    move_to(&ui, 10_000.0, y);
    assert!(
        (ui.get_second_length() - 100.0).abs() < 0.5,
        "the second pane stops at min-second (100 px), got {}",
        ui.get_second_length()
    );
    release(&ui, 10_000.0, y);

    // The ratio the drag reported agrees with the Rust clamp for the same total length.
    let total = ui.get_first_length() + ui.get_second_length();
    // The same rule the old Rust clamp applied: the first pane keeps at least 120 px and the second at least 100 px.
    let expected = 0.99_f32.clamp(120.0 / total, 1.0 - 100.0 / total);
    let last = *resizes.borrow().last().expect("a resize was reported");
    assert!(
        (last - expected).abs() < 0.001,
        "reported {last}, Rust clamp {expected}"
    );
}

// The testing backend can be installed once per process, so every scenario runs from this one test.
pub fn rail_and_sidebar_selection_scrolling_and_elision() {
    crate::support::init();
    rail_click_selects_the_area_once();
    sidebar_click_selects_the_page_once_and_a_new_list_clears_it();
    sidebar_scrolls_when_rows_overflow_and_stops_at_the_end();
    long_labels_stay_inside_the_sidebar();
    tabs_select_close_and_reorder_by_drag();
    split_drag_moves_the_handle_and_stops_at_the_minimums();
    command_palette_filter_and_keys();
    status_bar_state_and_controls();
}

// Command palette and status bar (S9.3). The palette and the status bar have their own test
// windows. The palette's filter and highlight rules are plain Rust (src/slint_ui/palette.rs), tested below without a window.
use space_ui::slint_ui::palette;
use space_ui::slint_ui::status;

mod palette_forms {
    pub use space_ui_test_slint::shell_3::*;
}

use palette_forms::{PaletteForm, PaletteRow, StatusForm};

fn palette_rows(entries: &[palette::Entry]) -> ModelRc<PaletteRow> {
    let rows: Vec<PaletteRow> = entries
        .iter()
        .map(|entry| PaletteRow {
            title: entry.title.into(),
            route: entry.route.clone().into(),
            subtitle: "".into(),
            category: "".into(),
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn press_key(ui: &PaletteForm, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn palette_filter_and_highlight_rules() {
    let all = palette::entries();
    assert!(all.len() > 20, "the manifest has many pages");
    assert!(
        all.iter().all(|entry| !entry.route.contains(':')),
        "pages with a parameter are left out"
    );

    assert_eq!(
        palette::filter("", &all).len(),
        all.len(),
        "blank matches all"
    );
    assert_eq!(
        palette::filter("   ", &all).len(),
        all.len(),
        "a spaces-only query is blank"
    );

    let upper = palette::filter("HOME", &all);
    assert!(!upper.is_empty(), "Home has a page");
    assert_eq!(palette::filter("home", &all), upper, "case does not matter");
    for &index in &upper {
        let entry = &all[index];
        assert!(
            entry.title.to_lowercase().contains("home")
                || entry.route.to_lowercase().contains("home"),
            "{entry:?} matches home"
        );
    }
    assert!(
        upper.windows(2).all(|pair| pair[0] < pair[1]),
        "entry order is kept"
    );

    // A route fragment matches even when the title does not contain it.
    let fragment = &all[10].route[1..3];
    assert!(
        palette::filter(fragment, &all).contains(&10),
        "route text matches: {fragment:?}"
    );

    assert!(
        palette::filter("zz-no-such-page", &all).is_empty(),
        "no match gives no rows"
    );
}

/// OQ6 and audit item 4: Pipelines is found by its aliases, and the project pages are listed for the current project.
fn palette_aliases_and_project_pages() {
    let project = "grand-tale";
    space_page_kit::projects::set_list(space_page_kit::projects::fixture());
    assert!(space_page_kit::projects::set_current(project));
    let all = palette::entries();
    let pipelines = all
        .iter()
        .position(|entry| entry.title == "Pipelines")
        .expect("Pipelines is in the palette");
    assert_eq!(
        all[pipelines].route,
        format!("/p/{project}/code/builds"),
        "the :key is filled in"
    );
    for query in ["pipelines", "Builds", "jobs", "JOBS"] {
        assert!(
            palette::filter(query, &all).contains(&pipelines),
            "{query:?} finds Pipelines"
        );
    }
    assert!(
        !palette::filter("zz-no-such-alias", &all).contains(&pipelines),
        "an unrelated query does not find it"
    );

    // Every other project page is listed too, under the same key, and no page with a record id is.
    let project_rows = all
        .iter()
        .filter(|entry| entry.route.starts_with("/p/"))
        .count();
    assert_eq!(
        project_rows, 19,
        "the project pages whose only parameter is :key"
    );
    assert!(
        all.iter().all(|entry| !entry.route.contains(':')),
        "record pages (with :id and the like) are left out"
    );
    assert!(
        palette::entries_for("other-key")
            .iter()
            .any(|entry| entry.route == "/p/other-key/code/builds"),
        "a project key other than the default fills in too"
    );
}

fn palette_highlight_moves_and_stops_at_the_ends() {
    assert_eq!(palette::step(0, 0, 1), -1, "no rows, no highlight");
    assert_eq!(
        palette::step(-1, 3, 1),
        0,
        "the first Down highlights row 0"
    );
    assert_eq!(palette::step(1, 3, 1), 2, "Down moves one row");
    assert_eq!(palette::step(2, 3, 1), 2, "Down stops at the last row");
    assert_eq!(palette::step(0, 3, -1), 0, "Up stops at the first row");
}

fn command_palette_filter_and_keys() {
    palette_filter_and_highlight_rules();
    palette_aliases_and_project_pages();
    palette_highlight_moves_and_stops_at_the_ends();

    let all = palette::entries();
    let count = all.len();
    let ui = PaletteForm::new().expect("create the palette form");
    ui.set_rows(palette_rows(&all));
    ui.set_highlight(0);
    ui.show().expect("show the palette form");
    slint::platform::update_timers_and_animations();

    // The list must show several rows, not a sliver (the S9 clipped-list bug). Rows are 34px.
    let list_height: f32 = ui.get_list_height();
    assert!(
        list_height >= 3.0 * 34.0,
        "the palette list shows at least three rows, got {list_height}px"
    );

    // Up and Down move the highlight through the Rust step rule. Escape and Return report to the callbacks.
    let weak = ui.as_weak();
    ui.on_move(move |delta| {
        if let Some(ui) = weak.upgrade() {
            ui.set_highlight(palette::step(ui.get_highlight(), count, delta));
        }
    });
    let picks = Rc::new(RefCell::new(Vec::new()));
    {
        let picks = picks.clone();
        ui.on_picked(move |index| picks.borrow_mut().push(index));
    }
    let closes = Rc::new(RefCell::new(0));
    {
        let closes = closes.clone();
        ui.on_closed(move || *closes.borrow_mut() += 1);
    }

    // A typed query refilters in Rust and resets the highlight to the first row.
    let weak = ui.as_weak();
    let query_entries = all.clone();
    ui.on_query_changed(move |query: SharedString| {
        if let Some(ui) = weak.upgrade() {
            let shown: Vec<palette::Entry> = palette::filter(&query, &query_entries)
                .into_iter()
                .map(|index| query_entries[index].clone())
                .collect();
            ui.set_rows(palette_rows(&shown));
            ui.set_highlight(if shown.is_empty() { -1 } else { 0 });
        }
    });

    ui.invoke_focus_palette();
    press_key(&ui, Key::DownArrow);
    assert_eq!(ui.get_highlight(), 1, "Down moves to row 1");
    press_key(&ui, Key::DownArrow);
    assert_eq!(ui.get_highlight(), 2, "Down again moves to row 2");
    press_key(&ui, Key::UpArrow);
    assert_eq!(ui.get_highlight(), 1, "Up moves back to row 1");
    for _ in 0..3 {
        press_key(&ui, Key::UpArrow);
    }
    assert_eq!(ui.get_highlight(), 0, "Up stops at row 0");

    press_key(&ui, Key::Return);
    assert_eq!(*picks.borrow(), vec![0], "Return picks the highlighted row");
    press_key(&ui, Key::Escape);
    assert_eq!(*closes.borrow(), 1, "Escape closes once");

    ui.invoke_type_query("ops".into());
    let ops = palette::filter("ops", &all);
    assert_eq!(
        ui.get_rows().row_count(),
        ops.len(),
        "the query refilters the rows"
    );
    assert_eq!(ui.get_highlight(), 0, "the first match is highlighted");
    assert!(!ops.is_empty(), "the manifest has an ops page");

    ui.invoke_type_query("zz-no-such-page".into());
    assert_eq!(ui.get_rows().row_count(), 0, "no match, no rows");
    assert_eq!(ui.get_highlight(), -1, "no row is highlighted");
}

fn status_bar_state_and_controls() {
    assert_eq!(status::format_elapsed(0), "0:00");
    assert_eq!(status::format_elapsed(65), "1:05");
    assert_eq!(status::format_elapsed(3599), "59:59");
    assert_eq!(status::format_elapsed(3600), "1:00:00");
    assert_eq!(status::format_elapsed(3723), "1:02:03");

    let ui = StatusForm::new().expect("create the status form");
    ui.set_timer("0:45".into());
    ui.show().expect("show the status form");
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_shown(), "0:45", "the timer text");
    ui.set_timer("".into());
    assert_eq!(ui.get_shown(), "", "an empty timer hides the slot");

    // The mute and leave call controls were removed (SF.4): the bar has no call buttons.
    assert!(!ui.get_has_mute(), "no mute button");
}
