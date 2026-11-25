//! Status bar data (S9.3; PLAN 6.1). The running timer text. The bar's controls are drawn by
//! `@rimelight_shell/components/status-bar.slint`; no call runs here (placeholders).

/// The running timer as text: `m:ss`, or `h:mm:ss` from one hour on.
pub fn format_elapsed(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}
