//! Copy text to the clipboard. The window's `copy-text` function (ui/app.slint) drives rimelight_ui's hidden
//! ClipboardWriter; rimelight_ui_kit's `wire_clipboard!` emits `copy_text(ui, text)` here, which calls it.

use crate::generated::AppWindow;

rimelight_ui_kit::wire_clipboard!(AppWindow);
