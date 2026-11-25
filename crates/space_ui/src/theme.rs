//! Themes: [`Theme`] (one resolved appearance) and [`ThemeMode`] (what the user chose). Plain values, no UI state.
//! The colours are the shared design tokens in `rimelight_design`; the Slint app reads the mode and keeps the `Tokens`
//! global in step with it (`slint_ui::glue::theme`, which wraps `rimelight_ui_kit::theme`).

/// What the user chose. `System` resolves against the OS appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeMode {
    /// Whether the mode is dark. `System` uses `system_dark`; the others ignore it.
    pub fn resolve(self, system_dark: bool) -> bool {
        match self {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => system_dark,
        }
    }
}

/// One resolved appearance (light or dark). Built by [`Theme::light`] and [`Theme::dark`]. The colour values are in
/// `rimelight_design::tokens::color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
}

impl Theme {
    /// The light theme.
    pub fn light() -> Self {
        Self::resolved(false)
    }

    /// The dark theme.
    pub fn dark() -> Self {
        Self::resolved(true)
    }

    fn resolved(dark: bool) -> Self {
        Self { dark }
    }

    /// `light` or `dark`, for labels and file names.
    pub fn name(&self) -> &'static str {
        if self.dark { "dark" } else { "light" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_are_different_and_named() {
        assert!(!Theme::light().dark);
        assert!(Theme::dark().dark);
        assert_eq!(Theme::light().name(), "light");
        assert_eq!(Theme::dark().name(), "dark");
    }

    #[test]
    fn explicit_modes_ignore_the_system() {
        for system_dark in [false, true] {
            assert!(!ThemeMode::Light.resolve(system_dark));
            assert!(ThemeMode::Dark.resolve(system_dark));
        }
    }

    #[test]
    fn system_follows_the_system() {
        assert!(!ThemeMode::System.resolve(false));
        assert!(ThemeMode::System.resolve(true));
    }

    #[test]
    fn default_mode_is_system() {
        assert_eq!(ThemeMode::default(), ThemeMode::System);
    }
}
