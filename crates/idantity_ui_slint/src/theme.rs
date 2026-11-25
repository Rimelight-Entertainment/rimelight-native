//! Theme setters for the shell window, from rimelight-rs's shared rules (`rimelight_ui_kit::theme`). Expanded here
//! because the macro names this crate's generated types (the pattern of rimelight-space `crates/space_ui/src/slint_ui/glue.rs`).

use crate::generated::{IdantityShell, ThemeChoice, Tokens};

rimelight_ui_kit::wire_theme!(IdantityShell, Tokens, ThemeChoice);
