//! What the canvas pop-out window needs from the platform (CV.12). The desktop app implements [`PopoutPlatform`] and installs
//! it at start-up; where nothing is installed the pop-out features are disabled. The system-wide click-through hotkey
//! reaches the canvas page through [`on_hotkey`] and [`hotkey_pressed`]. All of this runs on the UI thread.

use std::cell::RefCell;

/// What the canvas pop-out window needs from the platform. Each method returns whether it worked, where that matters.
pub trait PopoutPlatform {
    fn set_click_through(&self, window: &slint::Window, on: bool) -> bool;
    fn drag_window(&self, window: &slint::Window);
    fn set_minimized(&self, window: &slint::Window, on: bool);
    fn set_maximized(&self, window: &slint::Window, on: bool);
    fn set_resizable(&self, window: &slint::Window, on: bool);
    fn set_always_on_top(&self, window: &slint::Window, on: bool);
    fn set_always_on_bottom(&self, window: &slint::Window, on: bool) -> bool;
    fn set_alpha(&self, window: &slint::Window, alpha: u8) -> bool;
    fn list_windows(&self, window: &slint::Window) -> Vec<(isize, String)>;
    fn set_owner(&self, window: &slint::Window, owner: isize) -> bool;
}

type Platform = Box<dyn PopoutPlatform>;
type HotkeyHandler = Box<dyn Fn()>;

thread_local! {
    static PLATFORM: RefCell<Option<Platform>> = const { RefCell::new(None) };
    static HOTKEY: RefCell<Option<HotkeyHandler>> = const { RefCell::new(None) };
}

/// Installs the platform hooks. Called once by the desktop app at start-up.
pub fn install(platform: Box<dyn PopoutPlatform>) {
    PLATFORM.with(|slot| *slot.borrow_mut() = Some(platform));
}

/// Runs `f` with the installed platform, or returns None when there is none.
pub fn with<R>(f: impl FnOnce(&dyn PopoutPlatform) -> R) -> Option<R> {
    PLATFORM.with(|slot| slot.borrow().as_deref().map(f))
}

/// Registers the handler for the system-wide click-through hotkey (Ctrl+Alt+Shift+M). The canvas page registers it.
pub fn on_hotkey(f: Box<dyn Fn()>) {
    HOTKEY.with(|slot| *slot.borrow_mut() = Some(f));
}

/// Called by the desktop app when the hotkey is pressed: runs the registered handler, if any.
pub fn hotkey_pressed() {
    HOTKEY.with(|slot| {
        if let Ok(handler) = slot.try_borrow()
            && let Some(handler) = handler.as_ref()
        {
            handler();
        }
    });
}
