//! SA.9 connection-lost overlay. While signed in and the connection is lost, one full-window overlay ("Connection lost",
//! "Reconnecting…") blocks the whole app: a click on a shell control underneath reaches nothing, and the open tabs stay as
//! they were. Clicks go through `Window::dispatch_event`, as in `dialog.rs`, at the quick action button's centre; the quick
//! menu opening is the sentinel for "the click reached a control". No element lookup, so no Slint debug info (decision 0034 (c)).

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use space_ui::slint_ui::{AppWindow, ConnectionState, app_shell};
use space_ui::theme::ThemeMode;

/// Sets the window size as a resize from the OS would, then runs the update that sets `phone` from the width.
fn resize(ui: &impl ComponentHandle, width: f32, height: f32) {
    let size = LogicalSize::new(width, height);
    ui.window().set_size(size);
    ui.window().dispatch_event(WindowEvent::Resized { size });
    slint::platform::update_timers_and_animations();
}

fn set_state(ui: &AppWindow, state: ConnectionState) {
    ui.set_connection(state);
    slint::platform::update_timers_and_animations();
}

/// The overlay (with its "Connection lost" text) is drawn: visible and with a width.
fn overlay_text_visible(ui: &AppWindow) -> bool {
    ui.get_test_lost_overlay_drawn()
}

/// Clicks the centre of the quick action button: a press and a release, as a pointer does.
fn click_quick_action(ui: &AppWindow) {
    let at = LogicalPosition::new(
        ui.get_test_quick_x() + ui.get_test_quick_w() / 2.0,
        ui.get_test_quick_y() + ui.get_test_quick_h() / 2.0,
    );
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

/// SA.9: losing the connection shows the full-window overlay, and a click on a control underneath sends nothing.
pub fn disconnect_shows_the_overlay_and_blocks_clicks_underneath() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    resize(ui, 1280.0, 800.0);
    assert!(!ui.get_connection_lost_shown(), "connected: no overlay");

    set_state(ui, ConnectionState::Offline);
    assert!(
        ui.get_connection_lost_shown(),
        "the overlay shows when offline"
    );
    assert!(
        overlay_text_visible(ui),
        "the overlay's \"Connection lost\" text is visible"
    );

    click_quick_action(ui);
    assert!(
        !ui.get_quick_menu_open(),
        "a click on a control underneath the overlay reaches nothing"
    );
}

/// SA.9: when the connection returns the overlay goes, and the controls underneath work again.
pub fn reconnect_hides_the_overlay_and_restores_clicks() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    resize(ui, 1280.0, 800.0);

    set_state(ui, ConnectionState::Offline);
    assert!(ui.get_connection_lost_shown());
    set_state(ui, ConnectionState::Connected);
    assert!(
        !ui.get_connection_lost_shown(),
        "the overlay goes when the connection returns"
    );
    assert!(
        !overlay_text_visible(ui),
        "no \"Connection lost\" text once reconnected"
    );

    click_quick_action(ui);
    assert!(
        ui.get_quick_menu_open(),
        "a click on a control reaches it again once reconnected"
    );
}
