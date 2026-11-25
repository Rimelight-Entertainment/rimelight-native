//! S9.4 app shell test. The window is built with the same wiring as `apps/desktop`, on the
//! headless testing backend, and driven through its callbacks: an unknown start route falls back to Home's first page (Tasks); the palette
//! opens, filters and opens a page; About and the theme switch update the window.
//!
//! One test function: the testing backend can be initialised once per process, so the steps run in order here.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model as _, SharedString};
use space_nav::Area;
use space_ui::slint_ui::app_shell;
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::theme::ThemeMode;

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &impl ComponentHandle, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

pub fn app_shell_palette_about_and_theme_work_headless() {
    crate::support::init();

    let fallback = app_shell::build("/no-such-page", ThemeMode::System).expect("window builds");
    assert_eq!(fallback.ui.get_page_route().as_str(), "/home");
    drop(fallback);

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    assert!(!ui.get_palette_open());

    ui.invoke_palette_requested();
    assert!(ui.get_palette_open());
    assert!(
        ui.get_palette_rows().row_count() > 0,
        "every page is listed"
    );

    ui.invoke_palette_query_changed("calendar".into());
    let expected = ui
        .get_palette_rows()
        .row_data(0)
        .expect("a calendar page matches")
        .route
        .to_string();

    ui.invoke_palette_picked(0);
    assert!(!ui.get_palette_open(), "a pick closes the palette");
    assert_eq!(ui.get_page_route().as_str(), expected);

    // Ctrl+K with a control focused: Tab moves focus from the window into the shell, then Control+K still opens the
    // palette (the shell takes it in the capture phase).
    press(ui, Key::Tab);
    let control: SharedString = Key::Control.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: control.clone(),
    });
    press(ui, "k");
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: control });
    assert!(
        ui.get_palette_open(),
        "Ctrl+K opens the palette while a control has focus"
    );
    ui.invoke_palette_closed();
    assert!(!ui.get_palette_open());

    ui.invoke_about_clicked();
    assert!(ui.get_about_open());

    assert_eq!(ui.get_theme_label().as_str(), "Theme: light");
    ui.invoke_theme_clicked();
    assert_eq!(ui.get_theme_label().as_str(), "Theme: dark");
    assert_eq!(
        ui.get_toasts().row_count(),
        0,
        "the theme switch shows no toast"
    );

    // Phone width (PLAN 6.2): the rail and sidebar give way to the bottom tabs and the drawer.
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    // The `changed width` handler that sets `phone` runs on the next update, so run one before reading it.
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");
    assert!(
        !ui.get_rail_shown(),
        "the rail is hidden on a phone (Calendar is a rail item only)"
    );
    assert!(
        ui.get_tab_bar_shown(),
        "the bottom tab bar shows the Chat tab"
    );

    // The phone top bar has a search button that opens the command palette, the same as Ctrl+K.
    assert!(!ui.get_palette_open());
    ui.invoke_palette_requested();
    assert!(ui.get_palette_open(), "Search opens the command palette");
    ui.invoke_palette_closed();
    assert!(!ui.get_palette_open());

    // Tapping a tab changes the area.
    ui.invoke_tab_activated(1);
    assert_eq!(ui.get_shell_area(), 1, "Chat is area index 1");
    assert_eq!(ui.get_tab_selected(), 1, "the Chat tab is marked active");
    assert!(!ui.get_drawer_open());

    // More opens the drawer with the area's rows; picking a row navigates and closes it.
    ui.invoke_tab_activated(4);
    assert!(ui.get_drawer_open(), "More opens the drawer");

    // The index of the Chat page row in the sidebar, as the sidebar callback takes it.
    let (row, route) = sidebar_entries(Area::Chat)
        .into_iter()
        .enumerate()
        .find_map(|(at, entry)| entry.route.map(|r| (at as i32, r)))
        .expect("Chat has a page row");
    // The drawer's row pick: the drawer closes, then the row fires (the hook does what the drawer's own handler does).
    ui.invoke_test_pick_drawer_row(row);
    assert_eq!(ui.get_page_route().as_str(), route, "the row navigates");
    assert!(!ui.get_drawer_open(), "a row pick closes the drawer");
}
