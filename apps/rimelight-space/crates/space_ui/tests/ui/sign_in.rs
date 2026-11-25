//! SA.4 sign-in screen and signed-in shell. The window is built with the desktop wiring (`app_shell::build`) on the headless
//! testing backend. The screen's states, its button and Cancel, the avatar menu's injected user, Sign out, and the status
//! bar's Connected and Offline words are read through the window's test state and hooks.

use std::cell::Cell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::SharedString;
use slint::platform::{Key, WindowEvent};
use space_ui::slint_ui::{AppShell, SessionPhase, app_shell};
use space_ui::theme::ThemeMode;

const PROVISION_TEXT: &str = "Your account does not have access to Rimelight Space. Ask an admin.";

fn window() -> space_ui::slint_ui::app_shell::App {
    app_shell::build("/home", ThemeMode::Light).expect("window builds")
}

pub fn sign_in_screen_states_render() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;

    // A window built with the desktop wiring defaults to the shell: the screen is not placed.
    assert_eq!(ui.get_session_phase(), SessionPhase::SignedIn);
    assert!(!ui.get_sign_in_placed(), "signed-in shows the shell");

    ui.set_session_phase(SessionPhase::Loading);
    assert!(ui.get_sign_in_placed());
    assert!(ui.get_sign_in_loading_shown(), "loading shows the spinner");
    assert!(!ui.get_sign_in_button_shown(), "no button while loading");
    assert_eq!(ui.get_sign_in_status_text(), "Checking your session...");

    ui.set_session_phase(SessionPhase::SignedOut);
    assert!(
        ui.get_sign_in_button_shown(),
        "signed-out shows the primary button"
    );
    assert!(
        !ui.get_sign_in_error_shown(),
        "no error line without an error"
    );

    ui.set_sign_in_error(PROVISION_TEXT.into());
    assert!(
        ui.get_sign_in_error_shown(),
        "the error line shows with an error"
    );
    assert!(
        ui.get_sign_in_button_shown(),
        "the button stays for a retry"
    );

    ui.set_session_phase(SessionPhase::Waiting);
    assert!(
        ui.get_sign_in_waiting_shown(),
        "waiting shows the browser message"
    );
    assert!(
        !ui.get_sign_in_button_shown(),
        "no primary button while waiting"
    );
    assert_eq!(
        ui.get_sign_in_status_text(),
        "Finish signing in in your browser"
    );

    ui.set_session_phase(SessionPhase::SignedIn);
    assert!(!ui.get_sign_in_placed(), "signed in hides the screen");
}

pub fn sign_in_button_invokes_callback_only_when_enabled() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;
    let signed_in_clicks = Rc::new(Cell::new(0u32));
    let cancels = Rc::new(Cell::new(0u32));
    let clicks = signed_in_clicks.clone();
    ui.on_sign_in_clicked(move || clicks.set(clicks.get() + 1));
    let cancelled = cancels.clone();
    ui.on_sign_in_cancelled(move || cancelled.set(cancelled.get() + 1));

    ui.set_session_phase(SessionPhase::SignedOut);
    ui.invoke_test_click_sign_in();
    assert_eq!(signed_in_clicks.get(), 1, "the button starts a sign-in");

    ui.set_session_phase(SessionPhase::Waiting);
    ui.invoke_test_click_sign_in();
    assert_eq!(
        signed_in_clicks.get(),
        1,
        "the button is disabled while waiting"
    );
    ui.invoke_test_click_sign_in_cancel();
    assert_eq!(cancels.get(), 1, "Cancel ends the wait");
}

/// The profile menu on its own. The window's profile popover is a popup, and the window cannot read into it, so the menu's
/// text and its Sign out row are checked here, with the same bindings the window gives it.
mod profile_form {
    pub use space_ui_test_slint::sign_in::*;
}

pub fn avatar_menu_shows_the_injected_user() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;
    ui.set_user_name("Ana Souza".into());
    ui.set_user_initials("AS".into());
    ui.set_user_email("ana.souza@example.com".into());
    slint::platform::update_timers_and_animations();

    // The top bar's avatar draws the injected initials.
    assert_eq!(
        ui.get_avatar_initials(),
        "AS",
        "the top bar avatar shows the user's initials"
    );

    // The profile menu shows the injected name and email, through the window's own binding.
    let form = profile_form::ProfileForm::new().expect("profile form builds");
    form.set_user_name("Ana Souza".into());
    form.set_user_initials("AS".into());
    slint::platform::update_timers_and_animations();
    assert_eq!(
        form.get_shown_name(),
        "Ana Souza",
        "the menu shows the display name"
    );
}

pub fn sign_out_calls_back_and_returns_to_sign_in() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;
    let signouts = Rc::new(Cell::new(0u32));
    let count = signouts.clone();
    ui.on_sign_out_clicked(move || count.set(count.get() + 1));

    // Sign out in the profile menu calls the window's sign-out callback (the menu is wired to it in the window).
    let form = profile_form::ProfileForm::new().expect("profile form builds");
    let count = signouts.clone();
    form.on_sign_out_clicked(move || {
        count.set(count.get() + 1);
    });
    form.invoke_test_click_sign_out();
    assert_eq!(signouts.get(), 1, "Sign out fires its callback");

    // The window's own callback is still a plain callback: the host answers it by showing the sign-in screen.
    ui.invoke_sign_out_clicked();
    assert_eq!(signouts.get(), 2, "the window forwards the callback");

    // The host clears the session and shows the sign-in screen again.
    ui.set_session_phase(SessionPhase::SignedOut);
    assert!(
        ui.get_sign_in_placed(),
        "after sign-out the sign-in screen is shown"
    );
    assert!(ui.get_sign_in_button_shown());
}

/// Control, a key, and Control released, as an OS key event would deliver them.
fn press_ctrl(ui: &AppShell, key: &str) {
    let control: SharedString = Key::Control.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: control.clone(),
    });
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: control });
}

/// While the sign-in screen covers the shell, Ctrl+K does not open the command palette. Once signed in, it does.
pub fn shell_shortcuts_are_off_while_signed_out() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;

    ui.set_session_phase(SessionPhase::SignedOut);
    assert!(ui.get_sign_in_placed());
    press_ctrl(ui, "k");
    assert!(
        !ui.get_palette_open(),
        "Ctrl+K must not reach the shell while signed out"
    );

    ui.set_session_phase(SessionPhase::SignedIn);
    press_ctrl(ui, "k");
    assert!(
        ui.get_palette_open(),
        "Ctrl+K opens the palette once signed in"
    );
    ui.invoke_palette_closed();
}

/// SA.8: when start-up cannot reach the server, the window shows "Can't reach Rimelight Space" with Retry, and no sign-in
/// button. Retry asks the host again; once the host shows the shell, the screen is gone.
pub fn start_up_unreachable_shows_the_screen_and_retry_recovers() {
    crate::support::init();
    let app = window();
    let ui: &AppShell = &app.ui;
    let retries = Rc::new(Cell::new(0u32));
    let count = retries.clone();
    ui.on_sign_in_retry_clicked(move || count.set(count.get() + 1));

    ui.set_session_phase(SessionPhase::Unreachable);
    assert!(
        ui.get_sign_in_placed(),
        "the unreachable screen covers the shell"
    );
    assert!(
        ui.get_sign_in_unreachable_shown(),
        "the Can't reach screen is drawn"
    );
    assert!(
        !ui.get_sign_in_button_shown(),
        "no sign-in button on the unreachable screen"
    );
    assert!(!ui.get_sign_in_loading_shown(), "no spinner there either");

    ui.invoke_test_click_sign_in_retry();
    assert_eq!(retries.get(), 1, "Retry asks the host to check again");

    // The host reached the server and found the session: the shell shows.
    ui.set_session_phase(SessionPhase::SignedIn);
    assert!(
        !ui.get_sign_in_placed(),
        "a successful retry shows the shell"
    );
    assert!(!ui.get_sign_in_unreachable_shown());
}
