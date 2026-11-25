//! The Slint screens (ui/app.slint), compiled by build.rs. Slint's generated code uses unsafe internally.
#![allow(unsafe_code)]
#![allow(clippy::all)]

slint::include_modules!();

/// The theme setters from rimelight_ui_kit (set_mode, is_dark, set_reduced_motion, set_text_scale) for the shared Tokens
/// global. Only set_mode is called today; the others stay available with the same wiring.
#[allow(dead_code)]
pub mod theme {
    rimelight_ui_kit::wire_theme!(super::AppWindow, super::Tokens, super::ThemeChoice);
}
