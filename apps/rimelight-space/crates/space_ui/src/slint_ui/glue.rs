//! Space's wiring of the shared rules in `rimelight_ui_kit` to the Slint types this crate generates. Each module here
//! expands one of the kit's `wire_*!` macros in this crate, where the generated types are in scope, so the kit never
//! names them (rimelight-rs AGENTS.md, layering 2). The shared gallery in rimelight-rs has its own wiring of the kit's
//! rules; these modules are Space's, used by the app windows only.
//!
//! Callers use the emitted functions by path, for example `glue::select::wire_nav(&ui.global::<SelectNav>())`.

/// The Select rules on the generated `SelectOption` and `SelectNav`: `entries`, `filter_options` and `wire_nav`.
pub mod select {
    use crate::slint_ui::{SelectNav, SelectOption};
    rimelight_ui_kit::wire_select!(SelectOption, SelectNav);
}

/// The Menu and ContextMenu rules on the generated `MenuItem`, `MenuRow`, `MenuNav`, `MenuKind` and `MenuChecked`.
pub mod menu {
    use crate::slint_ui::{MenuChecked, MenuItem, MenuKind, MenuNav, MenuRow};
    rimelight_ui_kit::wire_menu!(MenuItem, MenuRow, MenuNav, MenuKind, MenuChecked);
}

/// The ColorPicker's Hex field on the generated `ColorHex` global: `wire_hex`.
pub mod color {
    use crate::slint_ui::ColorHex;
    rimelight_ui_kit::wire_color_hex!(ColorHex);
}

/// The theme setters on the generated `Tokens` global of the root window: `set_mode`, `is_dark`, `set_reduced_motion`
/// and `set_text_scale`.
pub mod theme {
    use crate::slint_ui::{AppShell, ThemeChoice, Tokens};
    rimelight_ui_kit::wire_theme!(AppShell, Tokens, ThemeChoice);
}

impl From<crate::theme::ThemeMode> for rimelight_ui_kit::theme::Mode {
    /// Space's user choice (`crate::theme::ThemeMode`) as the kit's mode. `System` keeps following the OS.
    fn from(mode: crate::theme::ThemeMode) -> Self {
        match mode {
            crate::theme::ThemeMode::Light => Self::Light,
            crate::theme::ThemeMode::Dark => Self::Dark,
            crate::theme::ThemeMode::System => Self::System,
        }
    }
}
pub mod calendar {
    use crate::slint_ui::CalendarDay;
    space_page_kit::calendar_glue!(CalendarDay);
}
