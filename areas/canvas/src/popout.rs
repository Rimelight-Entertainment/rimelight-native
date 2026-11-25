//! The canvas pop-out window (CV.12). The canvas moves into its own frameless window; the main page shows a placeholder
//! while it is open. The window's bar (`CanvasPopout` in `canvas-popout.slint`) and the Mode / Window menu commands end up
//! here. Platform-only features go through [`popout_platform`]; where none is installed they toast "Not available".
//!
//! The open window lives in a thread-local slot (the UI thread only). Closing takes it out of the slot and drops it on the
//! next event-loop turn, so a window is never dropped from inside its own callback.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use std::rc::Rc;

use slint::LogicalSize;

use crate::controller::WindowCommand;
use crate::page::CanvasPage;
use crate::popout_platform;
use crate::window::{PopoutDyn, PopoutWindow, WindowItemView};
use rimelight_ui_kit::toast::{self, Tone};

/// Size of a new pop-out window, in logical px.
const START_W: f32 = 900.0;
const START_H: f32 = 640.0;
/// Height of the app-drawn top bar, added to the window height when sizing to a selection.
const BAR_PX: f64 = 32.0;
/// Opacity limits of the pop-out window.
const MIN_OPACITY: f32 = 0.2;
const MAX_OPACITY: f32 = 1.0;
/// Opacity step of Increase / Decrease opacity.
const OPACITY_STEP: f32 = 0.1;

/// The open pop-out and the state its commands keep between calls.
struct Popout {
    window: Rc<dyn PopoutDyn>,
    maximized: Cell<bool>,
    bottom: Cell<bool>,
    /// Size and pinned state before overlay selection was turned on, restored when it is turned off.
    saved_overlay: RefCell<Option<(f32, f32, bool)>>,
}

thread_local! {
    static POPOUT: RefCell<Option<Popout>> = const { RefCell::new(None) };
}

/// Runs `f` with the open pop-out, or returns None when none is open.
fn with_popout<R>(f: impl FnOnce(&Popout) -> R) -> Option<R> {
    POPOUT.with(|slot| slot.borrow().as_ref().map(f))
}

/// A clone of the open pop-out window (a handle, so no borrow is held while Slint or the platform runs).
fn live() -> Option<Rc<dyn PopoutDyn>> {
    with_popout(|p| p.window.clone())
}

/// The open pop-out window, for the page to read its `CanvasState`. None when no pop-out is open.
pub fn window() -> Option<Rc<dyn PopoutDyn>> {
    live()
}

/// Whether the pop-out window is open.
pub fn is_open() -> bool {
    with_popout(|_| ()).is_some()
}

/// Whether overlay selection is on in the open pop-out (the page hides the unselected items while it is).
pub fn overlay_active() -> bool {
    live().is_some_and(|w| w.get_overlay_mode())
}

/// Shows the "not available" toast for a command that needs the platform layer.
fn unavailable() {
    toast::announce(Tone::Neutral, "Not available on this system.");
}

thread_local! {
    // Opens the pop-out of the window type the page was wired with (set by [`register`])...
    static OPENER: Cell<Option<fn(&CanvasPage)>> = const { Cell::new(None) };
}

/// Tells the module which pop-out window type [`open`] creates. Called by the page's `wire`.
pub fn register<P: PopoutWindow>() {
    OPENER.with(|slot| slot.set(Some(open_with::<P>)));
}

/// Opens the pop-out window of the type the page was wired with, see [`open_with`].
pub fn open(page: &CanvasPage) {
    if let Some(opener) = OPENER.with(Cell::get) {
        opener(page);
    }
}

/// Opens the pop-out window: creates it, wires its bar, puts the canvas page into pop-out mode and shows it. When it is
/// already open, the window is brought forward instead.
fn open_with<P: PopoutWindow>(page: &CanvasPage) {
    if let Some(win) = live() {
        let _ = win.show_window();
        return;
    }
    let window = match P::create() {
        Ok(window) => window,
        Err(err) => {
            toast::announce(Tone::Danger, format!("Could not open the window: {err}"));
            return;
        }
    };
    window.window().set_size(LogicalSize::new(START_W, START_H));
    POPOUT.with(|slot| {
        *slot.borrow_mut() = Some(Popout {
            window: Rc::new(window.clone_strong()),
            maximized: Cell::new(false),
            bottom: Cell::new(false),
            saved_overlay: RefCell::new(None),
        });
    });
    wire(&window, page);
    page.pop_out(&window);
    popout_platform::on_hotkey(Box::new(click_through_off));
    let _ = window.show();
}

/// Closes the pop-out: hides it, drops it on the next event-loop turn, and brings the canvas back to the main page.
pub fn close(page: &CanvasPage) {
    let Some(popout) = POPOUT.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    let _ = popout.window.hide_window();
    page.bring_back();
    slint::Timer::single_shot(Duration::ZERO, move || drop(popout));
}

/// Runs a Mode or Window command on the open pop-out. New window opens the pop-out when none is open.
pub fn command(page: &CanvasPage, cmd: WindowCommand) {
    if matches!(cmd, WindowCommand::NewWindow) {
        match live() {
            Some(win) => {
                let _ = win.show_window();
            }
            None => open(page),
        }
        return;
    }
    let Some(win) = live() else {
        return;
    };
    match cmd {
        WindowCommand::Transparent => toggle_click_through(&*win),
        WindowCommand::OverlaySelection => toggle_overlay(&*win),
        WindowCommand::OnTop => toggle_on_top(&*win),
        WindowCommand::OnTopOf => list_for_picker(&*win),
        WindowCommand::OnBottom => toggle_on_bottom(&*win),
        WindowCommand::Maximize => toggle_maximized(&*win),
        WindowCommand::Minimize => minimize(&*win),
        WindowCommand::ResizeToSelection { w, h } => {
            resize_to(&*win, w, h + BAR_PX);
        }
        WindowCommand::Lock => toggle_locked(&*win),
        WindowCommand::NewWindow => {}
        WindowCommand::OpacityUp => set_opacity(&*win, win.get_window_opacity() + OPACITY_STEP),
        WindowCommand::OpacityDown => set_opacity(&*win, win.get_window_opacity() - OPACITY_STEP),
    }
}

/// The window picker's choice (Always on top of...): `handle` is the HWND as a decimal string. Makes the pop-out an owned
/// window of that one, and closes the picker.
pub fn set_owner_from_picker(handle: &str) {
    let Some(win) = live() else {
        return;
    };
    win.set_windows_open(false);
    let Ok(owner) = handle.parse::<isize>() else {
        return;
    };
    if popout_platform::with(|p| p.set_owner(win.slint_window(), owner)).is_none() {
        unavailable();
    }
}

/// Connects the pop-out's bar and its window close request to the page. The canvas page wires its own CanvasState.
fn wire<P: PopoutWindow>(window: &P, page: &CanvasPage) {
    let p = page.clone();
    window.on_bar_drag(move || {
        if let Some(win) = live()
            && !win.get_locked()
        {
            // No platform: nothing moves the window (the bar still shows).
            let _ = popout_platform::with(|pl| pl.drag_window(win.slint_window()));
        }
    });
    let p2 = p.clone();
    window.on_bar_toggle_pin(move || command(&p2, WindowCommand::OnTop));
    let p2 = p.clone();
    window.on_bar_toggle_click_through(move || command(&p2, WindowCommand::Transparent));
    let p2 = p.clone();
    window.on_bar_minimize(move || command(&p2, WindowCommand::Minimize));
    let p2 = p.clone();
    window.on_bar_maximize(move || command(&p2, WindowCommand::Maximize));
    let p2 = p.clone();
    window.on_bar_close(move || close(&p2));
    window.on_bar_opacity(|v| {
        if let Some(win) = live() {
            set_opacity(&*win, v);
        }
    });
    let p2 = p.clone();
    window.window().on_close_requested(move || {
        close(&p2);
        slint::CloseRequestResponse::HideWindow
    });
}

/// The system-wide click-through hotkey: turns click-through off so the window takes the mouse again.
fn click_through_off() {
    let Some(win) = live() else {
        return;
    };
    if win.get_click_through() {
        win.set_click_through(false);
        let _ = popout_platform::with(|p| p.set_click_through(win.slint_window(), false));
    }
}

fn toggle_click_through(win: &dyn PopoutDyn) {
    let on = !win.get_click_through();
    match popout_platform::with(|p| p.set_click_through(win.slint_window(), on)) {
        None => unavailable(),
        Some(false) => toast::announce(Tone::Warning, "Could not change mouse pass-through."),
        Some(true) => win.set_click_through(on),
    }
}

fn toggle_on_top(win: &dyn PopoutDyn) {
    let on = !win.get_pinned();
    // The pinned property drives always-on-top in Slint; the platform call makes it stick on Windows.
    win.set_pinned(on);
    let _ = popout_platform::with(|p| p.set_always_on_top(win.slint_window(), on));
}

fn toggle_on_bottom(win: &dyn PopoutDyn) {
    let on = !with_popout(|p| p.bottom.get()).unwrap_or(false);
    match popout_platform::with(|p| p.set_always_on_bottom(win.slint_window(), on)) {
        None => unavailable(),
        Some(false) => toast::announce(Tone::Warning, "Could not change the window order."),
        Some(true) => {
            let _ = with_popout(|p| p.bottom.set(on));
        }
    }
}

fn list_for_picker(win: &dyn PopoutDyn) {
    match popout_platform::with(|p| p.list_windows(win.slint_window())) {
        None => unavailable(),
        Some(list) => {
            let items: Vec<WindowItemView> = list
                .into_iter()
                .map(|(handle, title)| WindowItemView {
                    handle: handle.to_string(),
                    title,
                })
                .collect();
            win.set_window_items(items);
            win.set_windows_open(true);
        }
    }
}

fn toggle_maximized(win: &dyn PopoutDyn) {
    let Some(on) = with_popout(|p| !p.maximized.get()) else {
        return;
    };
    match popout_platform::with(|p| p.set_maximized(win.slint_window(), on)) {
        None => unavailable(),
        Some(()) => {
            let _ = with_popout(|p| p.maximized.set(on));
        }
    }
}

fn minimize(win: &dyn PopoutDyn) {
    if popout_platform::with(|p| p.set_minimized(win.slint_window(), true)).is_none() {
        unavailable();
    }
}

fn toggle_locked(win: &dyn PopoutDyn) {
    // Slint drops the resize border while locked; the platform stops the window from moving and resizing.
    let on = !win.get_locked();
    win.set_locked(on);
    let _ = popout_platform::with(|p| p.set_resizable(win.slint_window(), !on));
}

/// Overlay selection: the window shows only the selected images on a transparent, frameless, topmost background. Turning it
/// on hides the bar, pins the window and sizes it to the selection at the current zoom; turning it off restores both.
fn toggle_overlay(win: &dyn PopoutDyn) {
    let on = !win.get_overlay_mode();
    if on {
        let size = win
            .slint_window()
            .size()
            .to_logical(win.slint_window().scale_factor());
        let _ = with_popout(|p| {
            *p.saved_overlay.borrow_mut() = Some((size.width, size.height, win.get_pinned()));
        });
        win.set_overlay_mode(true);
        win.set_pinned(true);
        let _ = popout_platform::with(|p| p.set_always_on_top(win.slint_window(), true));
        if win.get_has_selection() {
            let zoom = f64::from(win.get_zoom());
            let w = f64::from(win.get_sel_w()) * zoom;
            let h = f64::from(win.get_sel_h()) * zoom;
            // The bar is hidden in overlay mode, so the height is the selection's own.
            resize_to(win, w, h);
        }
    } else {
        win.set_overlay_mode(false);
        let saved = with_popout(|p| p.saved_overlay.borrow_mut().take()).flatten();
        if let Some((w, h, pinned)) = saved {
            win.slint_window().set_size(LogicalSize::new(w, h));
            win.set_pinned(pinned);
            let _ = popout_platform::with(|p| p.set_always_on_top(win.slint_window(), pinned));
        }
    }
}

/// Sizes the window to `w` x `h` logical px (the height already includes the bar when it is shown), clamped to sane limits.
/// A locked window is not resized.
fn resize_to(win: &dyn PopoutDyn, w: f64, h: f64) {
    if win.get_locked() {
        return;
    }
    let w = w.clamp(200.0, 4000.0) as f32;
    let h = h.clamp(150.0, 3000.0) as f32;
    win.slint_window().set_size(LogicalSize::new(w, h));
}

/// Sets the whole window's opacity (clamped to 20 % .. 100 %, in steps of 1 %) and the platform's layered alpha.
fn set_opacity(win: &dyn PopoutDyn, value: f32) {
    let value = (value.clamp(MIN_OPACITY, MAX_OPACITY) * 100.0).round() / 100.0;
    win.set_window_opacity(value);
    let alpha = (value * 255.0).round() as u8;
    if popout_platform::with(|p| p.set_alpha(win.slint_window(), alpha)).is_none() {
        unavailable();
    }
}
