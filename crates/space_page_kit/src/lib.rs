//! Plain-Rust page rules shared by Space's area crates (`areas/*`), and the macros an area's window glue
//! expands. Nothing in this crate names a type generated from Slint markup: the window-typed parts are generic over
//! `slint::ComponentHandle`, and the generated row and state types are named by the macros an app expands
//! (`table_glue!`, `load_glue!`, `picker_glue!`) in its own crate, where those types are in scope.
//!
//! - [`page_data`]: the API source, the UI-thread hop and the picture cache of the page data pattern.
//! - [`task`]: `spawn_task`, a client task off the UI thread (native thread or web local task).
//! - [`person`]: avatar initials, the photo media type, and the admin role check.
//! - [`picker`]: the plain choice rows of the pickers, and the date shape check.
//! - [`lock`]: locking a page's request state.
//! - [`table`] and [`load`]: the plain table rows and load states, with the glue macros that convert them.

pub mod calendar;
pub mod dates;
pub mod load;
pub mod lock;
pub mod page_data;
pub mod person;
pub mod picker;
pub mod projects;
pub mod table;
pub mod task;

/// Re-exported so the macros in this crate name slint through `$crate` in any app crate.
#[doc(hidden)]
pub use slint as __slint;
