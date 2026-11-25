//! SF.4 shell chrome: the top bar, the status bar's right side, the profile menu's rows and the sidebar's collapse. The window is
//! built with the desktop wiring (`app_shell::build`) on the headless testing backend and driven by real pointer events.
//! The profile menu's presence picker is tested on its own form, because a Popover's panel is a separate popup window.
//! One test function: the testing backend is installed once per process.

use slint::Model;
use space_ui::slint_ui::app_shell;
use space_ui::theme::ThemeMode;

pub use space_ui_test_slint::shell_chrome::*;

pub fn sf4_top_bar_status_bar_and_sidebar_sections_headless() {
    crate::support::init();

    // The presence picker, on its own: a click on a row sets the local presence. A person sets Online, Away or Busy
    // only; Offline is shown to others but is not in the picker.
    let form = ProfileForm::new().expect("profile form builds");
    assert_eq!(form.get_presence(), UserPresence::Online);
    form.invoke_pick_presence(UserPresence::Busy);
    assert_eq!(form.get_presence(), UserPresence::Busy);
    form.invoke_pick_presence(UserPresence::Away);
    assert_eq!(form.get_presence(), UserPresence::Away);
    form.invoke_pick_presence(UserPresence::Online);
    assert_eq!(form.get_presence(), UserPresence::Online);
    // The picker has exactly these three rows: no Offline row and no Do not disturb row.
    let presence: Vec<String> = form
        .get_test_presence_labels()
        .iter()
        .map(|label| label.to_string())
        .collect();
    assert_eq!(
        presence,
        ["Online", "Away", "Busy"],
        "the picker has three rows: no Offline row and no Do not disturb row"
    );
    // The actions the skeleton shows, each with its full label.
    let actions: Vec<String> = form
        .get_test_action_labels()
        .iter()
        .map(|label| label.to_string())
        .collect();
    assert_eq!(
        actions,
        [
            "Profile",
            "About",
            "Give Feedback",
            "Report Issue",
            "Settings",
            "Sign out"
        ],
        "the profile menu has the Profile, About, Give Feedback, Report Issue, Settings and Sign out rows"
    );
    // The profile popover is 400px tall with 8px of padding on each side (see app-shell.slint), so the menu must fit in 384px.
    assert!(
        form.get_content_height() <= 384.0,
        "the profile menu content ({}px) fits the 400px popover",
        form.get_content_height()
    );
    drop(form);

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;

    // Top bar: notifications and the avatar exist; the theme and About buttons no longer sit in the top bar.
    assert!(
        ui.get_test_has_notifications(),
        "the top bar has a notifications button"
    );
    assert!(ui.get_test_has_avatar(), "the top bar has the avatar");
    assert!(
        ui.get_test_has_collapse(),
        "the tab row has the collapse button"
    );
    assert!(
        !ui.get_show_sidebar_shown(),
        "no reopen button while the sidebar is open"
    );

    // The profile menu's About, Report Issue and Settings rows call the shell's callbacks; each opens its own shell
    // screen, and only one is open at a time.
    ui.invoke_report_issue_clicked();
    assert!(ui.get_report_open(), "Report Issue opens its screen");
    ui.invoke_settings_clicked();
    assert!(ui.get_settings_open(), "Settings opens its screen");
    assert!(
        !ui.get_report_open() && !ui.get_about_open(),
        "opening Settings closes the other shell screens"
    );
    ui.invoke_about_clicked();
    assert!(ui.get_about_open(), "About opens the About view");
    assert!(!ui.get_settings_open(), "About closes Settings");
    ui.invoke_submit_feedback_clicked();
    assert!(ui.get_feedback_open(), "Give Feedback opens its screen");
    assert!(!ui.get_about_open(), "Give Feedback closes About");

    // Collapse hides the sidebar; the top bar offers a button to bring it back.
    ui.set_about_open(false);
    ui.invoke_test_collapse_sidebar();
    assert!(ui.get_sidebar_collapsed(), "collapse closes the sidebar");
    assert!(
        ui.get_show_sidebar_shown(),
        "a collapsed sidebar can be reopened from the top bar"
    );
    ui.invoke_test_show_sidebar();
    assert!(
        !ui.get_sidebar_collapsed(),
        "Show sidebar reopens the sidebar"
    );

    // Status bar right side: Shortcuts, Full screen and the theme button, which shows the current mode.
    assert!(ui.get_test_has_shortcuts(), "the status bar has Shortcuts");
    assert!(
        ui.get_test_has_fullscreen(),
        "the status bar has Full screen"
    );
    assert_eq!(
        ui.get_theme_label().as_str(),
        "Theme: light",
        "the theme button shows the current mode"
    );
    ui.invoke_theme_clicked();
    assert_eq!(
        ui.get_theme_label().as_str(),
        "Theme: dark",
        "the theme button cycles to dark"
    );
    ui.invoke_theme_clicked();
    assert_eq!(
        ui.get_theme_label().as_str(),
        "Theme: system",
        "the theme button cycles to system after dark"
    );
}
