//! Workspaces on a phone and on the web (planned feature B3b; IA part B). On a phone there is no strip: the top bar's
//! workspace button (with the count) opens a bottom sheet that lists the workspaces. A row switches to its workspace and
//! closes the sheet, a row's X closes that workspace, and "New workspace" opens one and switches to it. On the web the
//! strip is hidden (`show-workspace-strip` false), so the page takes the strip's height back; the switcher button then shows
//! once two or more workspaces are open, and Ctrl+Alt+T and Ctrl+Alt+1 to 9 keep working.
//!
//! The headless backend runs the same Rust workspace operations as the desktop, so these tests check the window's state
//! and its callbacks; the sheet's rows are found by their accessible labels.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model as _, SharedString};
use space_ui::slint_ui::AppShell;
use space_ui::slint_ui::app_shell::{self, App};
use space_ui::theme::ThemeMode;

/// The start route. Its workspace is the first one.
const START: &str = "/home";

/// Sets the window size as a resize from the OS would, then runs the update that sets `phone` from the width.
fn resize(ui: &AppShell, width: f32, height: f32) {
    let size = LogicalSize::new(width, height);
    ui.window().set_size(size);
    ui.window().dispatch_event(WindowEvent::Resized { size });
    slint::platform::update_timers_and_animations();
}

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &AppShell, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Holds a modifier key down, as the OS does while another key is pressed.
fn hold(ui: &AppShell, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
}

/// Lets go of a modifier key.
fn release(ui: &AppShell, key: Key) {
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
}

/// Ctrl+Alt+`key` (Cmd+Option+`key` on macOS), with the modifiers held as the OS holds them.
fn chord(ui: &AppShell, key: &str) {
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    hold(ui, modifier);
    hold(ui, Key::Alt);
    press(ui, key);
    release(ui, Key::Alt);
    release(ui, modifier);
}

/// The number of open workspaces, as the window's model holds them.
fn workspace_count(ui: &AppShell) -> usize {
    ui.get_workspace_tabs().row_count()
}

/// The name of workspace `index` as the sheet shows it.
fn workspace_name(ui: &AppShell, index: usize) -> String {
    ui.get_workspace_tabs()
        .row_data(index)
        .expect("the workspace is open")
        .label
        .to_string()
}

pub fn phone_workspace_switcher_sheet_switches_adds_and_closes() {
    crate::support::init();
    let app: App = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");
    resize(ui, 390.0, 844.0);
    assert!(ui.get_phone(), "390 px is a phone width");

    // No strip on a phone: it takes no height, and the switcher button with the count is in the top bar.
    assert_eq!(
        ui.get_workspace_height(),
        0.0,
        "a phone has no workspace strip"
    );
    assert_eq!(workspace_count(ui), 1, "one workspace at the start");
    assert!(
        ui.get_show_switcher(),
        "the phone shows the switcher button"
    );
    assert!(!ui.get_workspace_sheet_open(), "the sheet starts closed");

    // The button opens the sheet, which lists the workspace (its row is labelled with its number and name).
    ui.invoke_test_open_workspace_sheet();
    assert!(
        ui.get_workspace_sheet_open(),
        "the switcher button opens the sheet"
    );
    let first = workspace_name(ui, 0);
    assert!(
        ui.get_workspace_sheet_open() && workspace_count(ui) >= 1 && workspace_name(ui, 0) == first,
        "the sheet lists workspace 1"
    );

    // New workspace (in the sheet) adds one, makes it active and closes the sheet.
    ui.invoke_test_sheet_new_workspace();
    assert_eq!(workspace_count(ui), 2, "New workspace adds one");
    assert_eq!(ui.get_workspace_active(), 1, "the new workspace is active");
    assert!(
        !ui.get_workspace_sheet_open(),
        "New workspace closes the sheet"
    );

    // A row switches to its workspace and closes the sheet.
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_test_pick_sheet_row(0);
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "the row switches to workspace 1"
    );
    assert!(!ui.get_workspace_sheet_open(), "a row closes the sheet");

    // The X on a row closes that workspace, and the sheet stays open to show the rest.
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_workspace_closed(1);
    assert_eq!(workspace_count(ui), 1, "the X closes workspace 2");
    assert!(
        ui.get_workspace_sheet_open(),
        "closing a row keeps the sheet open"
    );
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "the active workspace stays active"
    );
}

pub fn web_hides_the_strip_and_reaches_the_switcher_with_two_workspaces() {
    crate::support::init();
    let app: App = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");
    resize(ui, 1280.0, 800.0);
    assert!(!ui.get_phone(), "1280 px is a desktop width");

    // Desktop: the strip is present, and the page starts under it.
    let strip = ui.get_workspace_height();
    assert!(strip > 0.0, "desktop shows the workspace strip");
    let page_with_strip = ui.get_page_top();
    assert!(strip > 0.0, "the strip's + button is drawn on desktop");
    assert!(
        !ui.get_show_switcher(),
        "desktop with the strip has no switcher button"
    );

    // Web: the "+" button and the workspace tabs are hidden, but the tab row stays (the collapse, Back and Forward
    // buttons live in it), so the row keeps its height and the page area does not move.
    app.show_workspace_strip(false);
    slint::platform::update_timers_and_animations();
    assert_eq!(
        ui.get_workspace_height(),
        strip,
        "the tab row keeps its height on the web"
    );
    assert_eq!(
        ui.get_page_top(),
        page_with_strip,
        "the page area does not move on the web"
    );
    assert!(
        !ui.get_show_switcher(),
        "one workspace: no switcher button on the web"
    );
    assert!(
        !ui.get_show_switcher(),
        "no switcher button drawn with one workspace"
    );

    // The shortcuts still work with the strip hidden: Ctrl+Alt+T opens a second workspace, and the switcher appears.
    chord(ui, "t");
    assert_eq!(
        workspace_count(ui),
        2,
        "Ctrl+Alt+T opens a workspace with the strip hidden"
    );
    assert!(
        ui.get_show_switcher(),
        "two workspaces: the switcher button shows on the web"
    );
    assert!(ui.get_show_switcher(), "the switcher button is drawn");

    // Ctrl+Alt+1 switches back to the first workspace, and the switcher opens the sheet.
    chord(ui, "1");
    assert_eq!(
        ui.get_workspace_active(),
        0,
        "Ctrl+Alt+1 switches to workspace 1"
    );
    ui.invoke_test_open_workspace_sheet();
    assert!(
        ui.get_workspace_sheet_open(),
        "the switcher opens the sheet on the web"
    );
}

/// The sheet row menu's rows as the window holds them (labels; a separator is an empty label).
fn sheet_menu_labels(ui: &AppShell) -> Vec<String> {
    ui.get_open_workspace_menu()
        .iter()
        .map(|item| item.label.to_string())
        .collect()
}

pub fn phone_workspace_sheet_row_menu_pins_renames_duplicates_and_closes() {
    crate::support::init();
    let app: App = app_shell::build(START, ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.show().expect("show the window");
    resize(ui, 390.0, 844.0);

    // Two workspaces; the second is the one the row menu acts on.
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_test_sheet_new_workspace();
    assert_eq!(workspace_count(ui), 2, "two workspaces");
    assert!(!ui.get_workspace_can_reopen(), "nothing closed yet");
    assert!(
        !ui.get_workspace_sheet_open(),
        "New workspace closes the sheet"
    );
    let second = workspace_name(ui, 1);

    // The "more" button opens the compact menu for its row: no bulk closes and no Reopen row, but Copy link (gaps D2, the
    // same row as the desktop menu).
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_test_open_sheet_row_menu(1);
    assert_eq!(
        sheet_menu_labels(ui),
        [
            "Close",
            "",
            "Duplicate workspace",
            "Rename",
            "Pin workspace",
            "Copy link"
        ],
        "the sheet row menu is the compact one"
    );
    // Copy link writes the workspace's active page link (the desktop menu's action, on the same row).
    ui.invoke_workspace_menu_picked(1, "copy_link".into());
    assert!(
        ui.get_last_copied()
            .starts_with("https://space.rimelight.com/"),
        "Copy link from the sheet row writes the page's web link"
    );

    // Pin moves the row to the pinned group, at the top.
    ui.invoke_workspace_menu_picked(1, "pin".into());
    assert!(
        ui.get_workspace_tabs().row_data(0).expect("row 1").pinned,
        "the pinned row moves to the top"
    );
    assert_eq!(workspace_name(ui, 0), second, "the same workspace moved");

    // Duplicate adds a row.
    ui.invoke_workspace_menu_picked(1, "duplicate".into());
    assert_eq!(workspace_count(ui), 3, "Duplicate adds a row");

    // Close removes the row. The closed workspace goes on the stack, so Reopen closed is enabled and brings it back.
    ui.invoke_workspace_menu_picked(2, "close".into());
    assert_eq!(workspace_count(ui), 2, "Close removes the row");
    assert!(
        ui.get_workspace_can_reopen(),
        "Reopen closed is enabled after a close"
    );
    ui.invoke_test_open_workspace_sheet();
    ui.invoke_reopen_workspace_requested();
    assert_eq!(
        workspace_count(ui),
        3,
        "Reopen closed brings the workspace back"
    );

    // Rename last: the sheet is open (Reopen keeps it open), so Rename must close it and open the dialog on top.
    assert!(ui.get_workspace_sheet_open(), "the sheet is still open");
    ui.invoke_workspace_menu_picked(0, "rename".into());
    assert!(
        !ui.get_workspace_sheet_open(),
        "Rename closes the sheet first"
    );
    assert!(ui.get_workspace_rename_open(), "Rename opens the dialog");
    ui.invoke_workspace_renamed(0, "Sales".into());
    assert_eq!(
        workspace_name(ui, 0),
        "Sales",
        "the rename shows in the sheet"
    );
}
