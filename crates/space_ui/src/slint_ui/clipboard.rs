//! Copy link (gaps D2): the one way the shell writes a link to the clipboard.
//!
//! Desktop and Android: the AppShell's `copy-link` function (`ui/components/clipboard-writer.slint`) selects its hidden
//! TextInput's text and calls the TextInput's `copy()`, which Slint hands to the platform clipboard. That path is synchronous,
//! so its outcome is known at once.
//!
//! Web: Slint's `copy()` only works inside a browser copy event, so it does nothing from a menu pick. The web app registers
//! its own writer (`apps/web`, a `navigator.clipboard.writeText` call). The browser answers later, from a promise, so the
//! writer takes a completion callback and calls it with `true` (written) or `false` (refused). A writer is registered once per
//! thread, before the window is built.

use rimelight_ui_kit::toast::Tone;

// The writer lives in space_ui_slint (the area glue macros take it by path); the web app registers its browser writer here.
pub use space_ui_slint::clipboard::{set_browser_writer, write_link};

/// The toast for a Copy link outcome: "Link copied" in the success tone when the text was written, and "Could not copy the
/// link" in the danger (error) tone when it was refused.
pub(crate) fn copy_toast(copied: bool) -> (Tone, &'static str) {
    if copied {
        (Tone::Success, "Link copied")
    } else {
        (Tone::Danger, "Could not copy the link")
    }
}

#[cfg(test)]
mod tests {
    use super::copy_toast;
    use rimelight_ui_kit::toast::Tone;

    #[test]
    fn copy_outcome_picks_the_success_or_error_toast() {
        assert_eq!(copy_toast(true), (Tone::Success, "Link copied"));
        assert_eq!(copy_toast(false), (Tone::Danger, "Could not copy the link"));
    }
}
