//! Quick action (SF.4; IA section 4, OQ14, OQ16, OQ17, OQ25). The button is the bottom-right floating control in the window
//! on desktop and phone. Its menu lists the `space_nav::quick` categories, Personal then Chat. A pick records its id.
//! Ctrl+Alt+N opens the menu. On a phone the button is removed while a text field has focus.
//!
//! The headless backend has no pointer or keyboard device, so the shortcut and the clicks are sent as window events and
//! mock clicks, the same way the other UI tests drive the window.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model as _, SharedString};
use space_nav::quick;
use space_ui::slint_ui::{AppShell, app_shell};
use space_ui::theme::ThemeMode;

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

/// Whether the quick action button is drawn (the window's own condition for its visibility).
fn quick_button_present(ui: &AppShell) -> bool {
    ui.get_quick_button_shown()
}

pub fn quick_action_button_menu_shortcut_and_phone_hiding() {
    crate::support::init();

    // The model the menu is fed from is the nav category model: Personal first, then Chat, in the nav order.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let categories = ui.get_quick_categories();
    assert_eq!(
        categories.row_count(),
        3,
        "three categories: Personal, Chat and Projects"
    );
    let mut titles = Vec::new();
    for (index, category) in quick::categories().iter().enumerate() {
        let row = categories.row_data(index).expect("category row");
        assert_eq!(row.title.as_str(), category.title);
        titles.push(row.title.to_string());
    }
    assert_eq!(titles, ["Personal", "Chat", "Projects"]);
    let labels = |row: usize| -> Vec<String> {
        let row = categories.row_data(row).expect("category row");
        (0..row.items.row_count())
            .map(|i| row.items.row_data(i).expect("item").label.to_string())
            .collect()
    };
    assert_eq!(labels(0), ["New Mail", "New Event", "New Task", "New Note"]);
    assert_eq!(
        labels(1),
        ["New Channel", "New Chat", "New Room", "New Group"]
    );
    assert_eq!(labels(2), ["New issue"]);

    // Desktop: the button is present, and a focused field does not hide it (OQ25 is a phone rule).
    assert!(quick_button_present(ui), "the quick action is on desktop");
    ui.set_input_focused(true);
    assert!(
        quick_button_present(ui),
        "desktop keeps the button while a field has focus"
    );
    ui.set_input_focused(false);

    // A pick: the button opens the menu, and clicking an item records its id.
    ui.invoke_test_tap_quick_action();
    slint::platform::update_timers_and_animations();
    assert!(
        ui.get_quick_menu_open(),
        "the Chat category is in the open menu"
    );
    ui.invoke_test_pick_quick_item("new_chat".into());
    assert_eq!(
        app.last_quick_pick.borrow().as_deref(),
        Some("new_chat"),
        "picking New Chat fires the callback with its id"
    );
    drop(app);

    // Ctrl+Alt+N (Cmd+Option+N on macOS) opens the menu. The menu items are only in the tree while it is open.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    assert!(
        !ui.get_quick_menu_open(),
        "the menu is closed before the shortcut"
    );
    hold(ui, modifier);
    hold(ui, Key::Alt);
    press(ui, "n");
    release(ui, Key::Alt);
    release(ui, modifier);
    assert!(ui.get_quick_menu_open(), "the shortcut opens the menu");
    drop(app);

    // Phone: the button is shown on every page, and removed while input-focused is true.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");
    assert!(quick_button_present(ui), "the phone shows the quick action");
    ui.set_input_focused(true);
    slint::platform::update_timers_and_animations();
    assert!(
        !quick_button_present(ui),
        "the phone hides the quick action while a composer or form has focus"
    );
    ui.set_input_focused(false);
    slint::platform::update_timers_and_animations();
    assert!(
        quick_button_present(ui),
        "the button returns when focus leaves"
    );
}

/// The text an OS key event carries for N and K on a layout that uses AltGr (US-International on Windows). Ctrl+Alt is
/// AltGr there, so AltGr+N types "ñ" and AltGr+Shift+N "Ñ"; the control character is what Ctrl+N sends. The headless
/// test used to send only "n", which is why the real app's shortcut passed here and did nothing on the owner's machine.
/// macOS Option+N is a dead key that types "˜".
pub fn quick_action_shortcut_accepts_each_layout_text() {
    crate::support::init();
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };

    for text in ["n", "N", "ñ", "Ñ", "\u{000e}", "˜"] {
        let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
        let ui = &app.ui;
        assert!(
            !ui.get_quick_menu_open(),
            "the menu is closed before the shortcut ({text:?})"
        );
        hold(ui, modifier);
        hold(ui, Key::Alt);
        press(ui, text);
        release(ui, Key::Alt);
        release(ui, modifier);
        assert!(
            ui.get_quick_menu_open(),
            "Ctrl+Alt+N (Cmd+Option+N on macOS) with text {text:?} opens the menu"
        );
    }

    // Ctrl+K opens the palette with the letter, either case, or the Ctrl+K control character.
    for text in ["k", "K", "\u{000b}"] {
        let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
        let ui = &app.ui;
        assert!(!ui.get_palette_open(), "the palette is closed at start");
        hold(ui, Key::Control);
        press(ui, text);
        release(ui, Key::Control);
        assert!(
            ui.get_palette_open(),
            "Ctrl+K with text {text:?} opens the palette"
        );
    }
}
