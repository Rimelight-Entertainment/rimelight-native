//! The Rimelight Personal app shell: the one Slint window (`ui/app.slint`) that the desktop and web apps open, the nav routes
//! from `rimelight_personal_nav`, and the glue that connects each page's controller to its markup.
//!
//! Each page owns three files (crates/features/README.md, "Wiring into rimelight_personal_ui_slint"): its markup under
//! `ui/pages/<page>.slint`, its glue under `src/pages/<page>.rs`, and its controller in the feature crate. The shell
//! itself (frame, sidebar, route switch, theme, sign-in) is in [`shell`], [`theme`] and [`session`].

pub mod clipboard;
pub mod pages;
#[cfg(not(target_arch = "wasm32"))]
pub mod platform;
pub mod session;
pub mod shell;
pub mod theme;

// Slint's generated code contains unsafe vtable statics. Only this module may allow it (the crate denies unsafe).
#[allow(unsafe_code)]
mod generated {
    slint::include_modules!();
}

pub use generated::{AppWindow, SidebarItem, SidebarKind, ThemeChoice, Tokens};
