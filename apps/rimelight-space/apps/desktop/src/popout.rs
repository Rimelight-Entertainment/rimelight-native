//! Desktop side of the canvas pop-out window (CV.12): winit window calls, the Win32 calls from `rimelight_platform` on
//! Windows, and the system-wide click-through hotkey (Ctrl+Alt+Shift+M).

#![cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]

use std::time::Duration;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use slint::TimerMode;
use slint::winit_030::{WinitWindowAccessor, winit};
use space_area_canvas::popout_platform::{self, PopoutPlatform};
#[cfg(windows)]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::WindowLevel;

/// The winit-backed pop-out platform, installed by `main`.
pub struct DesktopPopout;

/// The Win32 window handle of the window, when it has one.
#[cfg(windows)]
fn hwnd_of(window: &slint::Window) -> Option<isize> {
    window
        .with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        })
        .flatten()
}

impl PopoutPlatform for DesktopPopout {
    fn set_click_through(&self, window: &slint::Window, on: bool) -> bool {
        window
            .with_winit_window(|w| w.set_cursor_hittest(!on).is_ok())
            .unwrap_or(false)
    }

    fn drag_window(&self, window: &slint::Window) {
        let _ = window.with_winit_window(|w| w.drag_window());
    }

    fn set_minimized(&self, window: &slint::Window, on: bool) {
        let _ = window.with_winit_window(|w| w.set_minimized(on));
    }

    fn set_maximized(&self, window: &slint::Window, on: bool) {
        let _ = window.with_winit_window(|w| w.set_maximized(on));
    }

    fn set_resizable(&self, window: &slint::Window, on: bool) {
        let _ = window.with_winit_window(|w| w.set_resizable(on));
    }

    fn set_always_on_top(&self, window: &slint::Window, on: bool) {
        let level = if on {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        let _ = window.with_winit_window(|w| w.set_window_level(level));
    }

    fn set_always_on_bottom(&self, window: &slint::Window, on: bool) -> bool {
        #[cfg(windows)]
        {
            hwnd_of(window)
                .is_some_and(|hwnd| rimelight_platform::set_always_on_bottom(hwnd, on).is_ok())
        }
        #[cfg(not(windows))]
        {
            let _ = (window, on);
            false
        }
    }

    fn set_alpha(&self, window: &slint::Window, alpha: u8) -> bool {
        #[cfg(windows)]
        {
            hwnd_of(window)
                .is_some_and(|hwnd| rimelight_platform::set_window_alpha(hwnd, alpha).is_ok())
        }
        #[cfg(not(windows))]
        {
            let _ = (window, alpha);
            false
        }
    }

    fn list_windows(&self, window: &slint::Window) -> Vec<(isize, String)> {
        #[cfg(windows)]
        {
            hwnd_of(window)
                .map(|except| {
                    let list = rimelight_platform::WindowList::popout_owners(except);
                    rimelight_platform::list_windows(list)
                        .into_iter()
                        .map(|top| (top.hwnd, top.title))
                        .collect::<Vec<(isize, String)>>()
                })
                .unwrap_or_default()
        }
        #[cfg(not(windows))]
        {
            let _ = window;
            Vec::new()
        }
    }

    fn set_owner(&self, window: &slint::Window, owner: isize) -> bool {
        #[cfg(windows)]
        {
            hwnd_of(window).is_some_and(|hwnd| rimelight_platform::set_owner(hwnd, owner).is_ok())
        }
        #[cfg(not(windows))]
        {
            let _ = (window, owner);
            false
        }
    }
}

/// Registers Ctrl+Alt+Shift+M and starts a 50 ms poll that forwards presses to the canvas page. The caller keeps both
/// returned values alive for as long as the hotkey should work. None when the hotkey cannot be registered.
pub fn start_hotkey() -> Option<(GlobalHotKeyManager, slint::Timer)> {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(e) => {
            eprintln!("rimelight_desktop: the pop-out hotkey is off: {e}");
            return None;
        }
    };
    let hotkey = HotKey::new(
        Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT),
        Code::KeyM,
    );
    if let Err(e) = manager.register(hotkey) {
        eprintln!(
            "rimelight_desktop: the pop-out hotkey (Ctrl+Alt+Shift+M) is not registered: {e}"
        );
        return None;
    }
    let timer = slint::Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), || {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if matches!(event.state, HotKeyState::Pressed) {
                popout_platform::hotkey_pressed();
            }
        }
    });
    Some((manager, timer))
}
