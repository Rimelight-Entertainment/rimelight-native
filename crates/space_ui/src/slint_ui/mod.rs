//! Slint foundations (S1, decision 0007). The Slint markup is in `ui/`; this module is the Rust
//! side. See the crate README, section "Slint port".
//!
//! - [`assets`]: tests that the shared `icons.slint` names exactly the icons the Rust lists map. Always compiled.
//! - The Slint types (`Tokens`, `Type`, `Icons`, `AboutScreen`, `AppShell`, ...) are compiled from `ui/main.slint` by
//!   `build.rs`. The `Tokens` global comes from the shared `rimelight_design` markup.
//! - The shared rules (toast, color, table, tree, display, select, menu, theme) are in `rimelight_ui_kit`. [`glue`]
//!   wires them to the types above.

#[cfg(test)]
mod assets;

// The Slint types are generated in crate space_ui_slint (decision 0013), which holds the unsafe allow.
pub use space_ui_slint::*;

// Wiring of the shared rimelight_ui_kit rules to the generated types above (the kit's macros expand here).
pub mod glue;
// The Content area's window glue (CMS.6): the wire_content! expansion, the editor binder and the route dispatch (src/slint_ui/content.rs).
pub mod content;
// The native web views of the editor's web-only blocks (CMS.6): the WebViewHost adapter (src/slint_ui/webviews.rs).
pub mod webviews;

// Whole-shell captures (review captures, not kept): the real AppShell driven into named states. Headless builds only.
#[cfg(feature = "headless")]
pub mod shell_capture;

// Rail and sidebar (S9.1): the row models from the space_nav manifest (src/slint_ui/shell.rs).
pub mod shell;

// Sign-in and session rules (SA.4): the sign-in error text (src/slint_ui/session.rs).
pub mod session;

// Sign-in driver (SA.4 desktop, SA.6 web): runs the client and sets the window (src/slint_ui/session_driver.rs).
pub mod session_driver;

// Chat sidebar tree (SF.4 scaffold): the Teams-style rows of the Chat area, mock data and collapse state.
pub mod chat_sidebar;

// App shell (S9.4): the window apps/desktop opens, its wiring and the toast timer (src/slint_ui/app_shell.rs).
pub mod app_shell;

// Top-level workspaces (planned feature B) and their text persistence (src/slint_ui/workspace.rs). Always compiled.
pub mod workspace;

// Tile groups (planned feature B, tiles): the plain-Rust tree of page-tab groups in each workspace (src/slint_ui/tiles.rs).
pub mod tiles;

// Tab drag rules (planned feature B, tiles, T2b): the hit test and the drop plan for dragged page tabs (src/slint_ui/tile_drop.rs).
pub mod tile_drop;

// Back and forward history of one page tab (planned feature A): the route rules over the shared History (src/slint_ui/history.rs).
pub mod history;

// Space routes for the shared shell models (ST2.5b): the rail areas, Home and the page titles (src/slint_ui/routes.rs).
pub mod routes;

// Copy link (gaps D2): the clipboard path for a link (src/slint_ui/clipboard.rs). The web build registers its writer here.
pub mod clipboard;

// Command palette (S9.3): the page list and the filter and highlight rules (src/slint_ui/palette.rs). Status bar (S9.3):
// the timer text (src/slint_ui/status.rs). Both need the Slint types.
pub mod palette;

pub mod status;

// Plain-Rust rules of the P0.17 batch 6 date components (date arithmetic, month grid, range selection). Always compiled.
