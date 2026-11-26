//! Global keys: Print Screen (snip an area), Alt+Print Screen (snip a window), Ctrl+Print Screen (whole screen at
//! once), Shift+Print Screen (start or stop a screen recording). Polled from a Slint timer, as rimelight-space does
//! for its pop-out key.

use std::time::Duration;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    SnipRegion,
    SnipWindow,
    SnipScreen,
    ToggleRecording,
}

pub struct Hotkeys {
    _manager: GlobalHotKeyManager,
    _timer: slint::Timer,
    pub status: String,
}

pub fn start(on_action: impl Fn(Action) + 'static) -> Option<Hotkeys> {
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rimelight-media: global keys are off: {e}");
            return None;
        }
    };
    let keys = [
        (
            HotKey::new(None, Code::PrintScreen),
            Action::SnipRegion,
            "Print Screen",
        ),
        (
            HotKey::new(Some(Modifiers::ALT), Code::PrintScreen),
            Action::SnipWindow,
            "Alt + Print Screen",
        ),
        (
            HotKey::new(Some(Modifiers::CONTROL), Code::PrintScreen),
            Action::SnipScreen,
            "Ctrl + Print Screen",
        ),
        (
            HotKey::new(Some(Modifiers::SHIFT), Code::PrintScreen),
            Action::ToggleRecording,
            "Shift + Print Screen",
        ),
    ];
    let mut registered = Vec::new();
    let mut failed = Vec::new();
    for (key, action, name) in keys {
        match manager.register(key) {
            Ok(()) => registered.push((key.id(), action)),
            Err(_) => failed.push(name),
        }
    }
    let status = if failed.is_empty() {
        "All keys are active.".to_string()
    } else {
        format!(
            "Not available: {}. Another app (or another Rimelight Media window, or Windows' own \"Use the Print screen key to open screen capture\" setting) is using them.",
            failed.join(", ")
        )
    };
    // Win+Shift+S: the low-level hook swallows it, and Windows' own snipping overlay (if it starts anyway) is caught by a
    // window-event hook. Both set the flag that is read here.
    rimelight_platform::start_win_shift_s_hook();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            if rimelight_platform::take_win_shift_s() {
                on_action(Action::SnipRegion);
            }
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
                if let Some((_, action)) = registered.iter().find(|(id, _)| *id == event.id) {
                    on_action(*action);
                }
            }
        },
    );
    Some(Hotkeys {
        _manager: manager,
        _timer: timer,
        status,
    })
}
