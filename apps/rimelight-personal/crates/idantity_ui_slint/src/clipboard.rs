//! Copy text to the clipboard. The window's `copy-text` function (ui/main.slint) drives rimelight_ui's hidden
//! ClipboardWriter; rimelight_ui_kit's `wire_clipboard!` emits `copy_text(ui, text)` here, which calls it.

use crate::generated::IdantityShell;

rimelight_ui_kit::wire_clipboard!(IdantityShell);
