//! Back and forward in the page tabs (planned feature A; IA sections 6 and 9). The top bar Back and Forward buttons, Alt+Left and
//! Alt+Right, Ctrl+[ and Ctrl+]. A plain sidebar click replaces the active tab's route and pushes history; Ctrl+click
//! (Cmd+click on macOS) and a middle-button click open the row in a new tab.
//!
//! The buttons and the sidebar rows call the window's own callbacks (`back-requested`, `sidebar-activated`, and so on),
//! so the test needs no element lookup. The keys are sent as window events, as the other UI tests do.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model as _, SharedString};
use space_nav::{Area, pages_in};
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::slint_ui::shell::sidebar_entries;
use space_ui::theme::ThemeMode;

/// The start route of every test.
const START: &str = "/home";

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &impl ComponentHandle, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Holds a modifier key down, as the OS does while the other key is pressed.
fn hold(ui: &impl ComponentHandle, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
}

/// Lets go of a modifier key.
fn release(ui: &impl ComponentHandle, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}

/// The index of the Home sidebar row for `route`, as the sidebar callbacks take it.
fn home_row(route: &str) -> i32 {
    sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .unwrap_or_else(|| panic!("no Home sidebar row for {route:?}")) as i32
}

/// Two Home pages other than the start route, as (sidebar label, route). The labels are unique.
fn home_pages() -> Vec<(String, &'static str)> {
    let mut pages: Vec<(String, &'static str)> = Vec::new();
    for page in pages_in(Area::Home).filter(|page| !page.has_params()) {
        let label = page.title.to_string();
        if page.route != START && !pages.iter().any(|(seen, _)| *seen == label) {
            pages.push((label, page.route));
        }
    }
    assert!(pages.len() >= 2, "Home has at least two other pages");
    pages.truncate(2);
    pages
}

/// Cmd (macOS) or Ctrl (elsewhere): the modifier of the history shortcuts for `[` and `]`.
fn command_key() -> Key {
    if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    }
}

fn tab_count(app: &App) -> usize {
    app.ui.get_open_tabs().row_count()
}

/// The Back and Forward buttons, and the shortcuts: Alt+Left and Alt+Right, Ctrl+[ and Ctrl+] (Cmd on macOS).
fn buttons_and_shortcuts(app: &App, pages: &[(String, &'static str)]) {
    let ui = &app.ui;
    let (first, second) = (pages[0].1, pages[1].1);

    // At the start there is no history either way: both buttons are disabled, and a press does nothing.
    assert!(!ui.get_can_back(), "Back is disabled at the start");
    assert!(!ui.get_can_forward(), "Forward is disabled at the start");
    ui.invoke_back_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        START,
        "a disabled Back changes nothing"
    );

    // A plain sidebar click replaces the route. Back is enabled, Forward is not.
    ui.invoke_sidebar_activated(home_row(first));
    assert_eq!(ui.get_page_route().as_str(), first);
    assert!(ui.get_can_back(), "Back is enabled after navigating");
    assert!(
        !ui.get_can_forward(),
        "Forward stays disabled with nothing to go to"
    );

    // The Back button goes to the start route, and enables Forward.
    ui.invoke_back_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        START,
        "Back changes the active route"
    );
    assert!(!ui.get_can_back());
    assert!(ui.get_can_forward(), "Forward is enabled after going back");

    // The Forward button returns to the first page.
    ui.invoke_forward_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        first,
        "Forward changes the active route"
    );

    // Alt+Left and Alt+Right.
    hold(ui, Key::Alt);
    press(ui, Key::LeftArrow);
    release(ui, Key::Alt);
    assert_eq!(ui.get_page_route().as_str(), START, "Alt+Left goes back");
    hold(ui, Key::Alt);
    press(ui, Key::RightArrow);
    release(ui, Key::Alt);
    assert_eq!(
        ui.get_page_route().as_str(),
        first,
        "Alt+Right goes forward"
    );

    // Ctrl+[ and Ctrl+] (Cmd on macOS).
    let command = command_key();
    hold(ui, command);
    press(ui, "[");
    release(ui, command);
    assert_eq!(ui.get_page_route().as_str(), START, "Ctrl+[ goes back");
    hold(ui, command);
    press(ui, "]");
    release(ui, command);
    assert_eq!(ui.get_page_route().as_str(), first, "Ctrl+] goes forward");

    // A new route after going back drops the forward entries.
    ui.invoke_back_requested();
    ui.invoke_sidebar_activated(home_row(second));
    assert_eq!(ui.get_page_route().as_str(), second);
    assert!(
        !ui.get_can_forward(),
        "a new route after going back drops the forward entries"
    );
}

/// A plain click replaces the active tab's route (no new tab, the history grows). Ctrl+click, or a middle-button click, opens
/// the row in a new tab that becomes the active one.
fn sidebar_click_replaces_and_ctrl_click_opens_a_tab(app: &App, pages: &[(String, &'static str)]) {
    let ui = &app.ui;
    let (first, second) = (pages[0].1, pages[1].1);
    assert_eq!(tab_count(app), 1, "one tab at the start");

    // Plain clicks replace the route of the one tab: the count stays at one and the history grows.
    ui.invoke_sidebar_activated(home_row(first));
    ui.invoke_sidebar_activated(home_row(second));
    assert_eq!(tab_count(app), 1, "a plain click does not open a tab");
    assert_eq!(ui.get_page_route().as_str(), second);
    // Three entries (start, first, second): two Back steps reach the start.
    ui.invoke_back_requested();
    ui.invoke_back_requested();
    assert_eq!(
        ui.get_page_route().as_str(),
        START,
        "the history holds the start route"
    );
    assert!(!ui.get_can_back(), "no step back past the start");
    assert_eq!(tab_count(app), 1);

    // Ctrl+click (Cmd+click on macOS) opens the row in a new tab, which is active and has its own history.
    let command = command_key();
    hold(ui, command);
    ui.invoke_sidebar_opened_in_new_tab(home_row(first));
    release(ui, command);
    assert_eq!(tab_count(app), 2, "Ctrl+click adds one tab");
    assert_eq!(ui.get_open_active(), 1, "the new tab is active");
    assert_eq!(ui.get_page_route().as_str(), first);
    assert!(!ui.get_can_back(), "the new tab has no history yet");

    // A middle-button click opens another new tab.
    ui.invoke_sidebar_opened_in_new_tab(home_row(second));
    assert_eq!(tab_count(app), 3, "a middle-button click adds one tab");
    assert_eq!(ui.get_open_active(), 2, "the middle-click tab is active");
    assert_eq!(ui.get_page_route().as_str(), second);
}

pub fn back_forward_buttons_and_shortcuts() {
    crate::support::init();
    let pages = home_pages();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    buttons_and_shortcuts(&app, &pages);
}

pub fn back_forward_sidebar_click_replaces_and_ctrl_click_opens_a_tab() {
    crate::support::init();
    let pages = home_pages();
    let app = app_shell::build(START, ThemeMode::Light).expect("window builds");
    sidebar_click_replaces_and_ctrl_click_opens_a_tab(&app, &pages);
}
