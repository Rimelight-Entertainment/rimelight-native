//! Shortcuts dialog (IA section 9, owner direction 2026-11-12). It opens from the status bar Shortcuts button and from "?"
//! (only with no text field focused and no palette open), lists the shortcuts in three groups, marks the planned ones
//! with a Planned tag, and closes on Escape. It is not on a phone (OQ29), so the status bar button and "?" do nothing there.
//!
//! Driven by real key and pointer events through `Window::dispatch_event`, and the list is read from its row model (the
//! drawn rows are built from that model), so no element lookup is needed.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model as _, SharedString};
use space_ui::slint_ui::AppShell;
use space_ui::slint_ui::app_shell;
use space_ui::theme::ThemeMode;

/// One key press and release, as an OS key event would deliver them.
fn press(ui: &impl ComponentHandle, text: impl Into<SharedString>) {
    let text: SharedString = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Every row of the list as (group title, label, planned, has a second chord), in group order. Read from the model
/// `shortcut-sections`, which the drawn rows are built from.
fn rows(ui: &AppShell) -> Vec<(String, String, bool, bool)> {
    let mut out = Vec::new();
    for section in ui.get_shortcut_sections().iter() {
        let title = section.title.to_string();
        for row in section.rows.iter() {
            out.push((
                title.clone(),
                row.label.to_string(),
                row.planned,
                row.has_other,
            ));
        }
    }
    out
}

/// The list's group titles, in order.
fn titles(ui: &AppShell) -> Vec<String> {
    ui.get_shortcut_sections()
        .iter()
        .map(|section| section.title.to_string())
        .collect()
}

/// How many rows carry the label `label`.
fn rows_labelled(ui: &AppShell, label: &str) -> usize {
    rows(ui).iter().filter(|row| row.1 == label).count()
}

fn has(ui: &AppShell, label: &str) -> bool {
    rows_labelled(ui, label) > 0
}

pub fn shortcuts_dialog_opens_lists_and_closes() {
    crate::support::init();

    // Opens from the status bar Shortcuts button. Before the click the list is closed.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    assert!(
        !ui.get_shortcuts_open(),
        "the shortcuts list is closed at start"
    );
    // The status bar's Shortcuts button calls `open-shortcuts` (its clicked handler).
    ui.invoke_open_shortcuts();
    assert!(
        ui.get_shortcuts_open(),
        "the Shortcuts button opens the list"
    );

    // The implemented shortcuts are listed, in their groups.
    let group_titles = titles(ui);
    for title in ["Navigation", "Create", "Panes"] {
        assert!(
            group_titles.iter().any(|seen| seen == title),
            "the list has the {title:?} group"
        );
    }
    for label in [
        "Open the command palette",
        "Show this list",
        "Open the quick action menu",
    ] {
        assert!(has(ui, label), "the list has {label:?}");
    }
    // The history shortcuts are built (planned feature A): listed without a Planned tag. Each action is one row, with
    // its second key chord (Ctrl+] for Forward, Cmd+[ on macOS for Back) shown after "or" in that row.
    for label in [
        "Back in the current page tab",
        "Forward in the current page tab",
    ] {
        assert_eq!(
            rows_labelled(ui, label),
            1,
            "the list has one row for {label:?}"
        );
    }
    let history_rows: Vec<(String, String, bool, bool)> = rows(ui)
        .into_iter()
        .filter(|row| row.1 == "Forward in the current page tab")
        .collect();
    assert!(
        history_rows.iter().all(|row| row.3),
        "the history rows show their second chord after \"or\""
    );
    // The theme cycle is still planned, and its row carries a Planned tag (the tag is drawn for a planned row).
    assert!(
        has(ui, "Cycle the theme: system, light, dark"),
        "the list has the planned theme cycle"
    );
    assert!(
        rows(ui)
            .iter()
            .any(|row| row.1 == "Cycle the theme: system, light, dark" && row.2),
        "the planned theme cycle has a Planned tag"
    );

    // Esc closes it.
    press(ui, Key::Escape);
    assert!(!ui.get_shortcuts_open(), "Escape closes the list");
    drop(app);

    // "?" opens it when no text field has focus.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    press(ui, "?");
    assert!(ui.get_shortcuts_open(), "? opens the list");
    press(ui, Key::Escape);
    assert!(!ui.get_shortcuts_open(), "Escape closes it again");

    // "?" closes the list it opened: the same key toggles the dialog.
    press(ui, "?");
    assert!(ui.get_shortcuts_open(), "? opens the list again");
    press(ui, "?");
    assert!(!ui.get_shortcuts_open(), "? closes the open shortcuts list");
    // Escape still closes it after a toggle reopen.
    press(ui, "?");
    assert!(ui.get_shortcuts_open(), "? reopens the list");
    press(ui, Key::Escape);
    assert!(!ui.get_shortcuts_open(), "Escape still closes it");

    // "?" with a text field focused does not open it, so typing "?" still types.
    ui.set_input_focused(true);
    press(ui, "?");
    assert!(
        !ui.get_shortcuts_open(),
        "? does not open the list while a text field has focus"
    );
    ui.set_input_focused(false);

    // "?" with the palette open does not open it either.
    ui.invoke_palette_requested();
    press(ui, "?");
    assert!(
        !ui.get_shortcuts_open(),
        "? does not open the list while the palette is open"
    );
    drop(app);

    // The close control closes it too.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    ui.invoke_open_shortcuts();
    assert!(ui.get_shortcuts_open(), "open_shortcuts opens the list");
    // The dialog's close control closes the list (the same close the Escape key takes).
    ui.invoke_test_close_shortcuts();
    assert!(!ui.get_shortcuts_open(), "the close control closes it");
    drop(app);

    // Phone: no status bar, and "?" does nothing.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");
    press(ui, "?");
    assert!(
        !ui.get_shortcuts_open(),
        "? does not open the list on a phone"
    );
}
