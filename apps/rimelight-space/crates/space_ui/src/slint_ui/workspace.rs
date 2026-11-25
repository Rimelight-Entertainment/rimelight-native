//! Top-level workspaces (planned feature B) and their text persistence. The model is in `rimelight_shell::workspace`
//! (ST2.5b). This module keeps the old paths and names Space's routes: [`Workspaces`] is the shared model over
//! [`SpaceRoutes`], and the saved file lives in the Space app-data folder ([`APP_FOLDER`]).

use std::rc::Rc;

use super::routes::SpaceRoutes;

pub use rimelight_shell::workspace::{
    EMPTY_WORKSPACE_NAME, FileStore, MemoryStore, Saver, StartText, Store, WORKSPACE_CAP,
    WORKSPACE_CLOSED_CAP, WORKSPACE_FORMAT_VERSION, choose_start_text,
};

/// The app-data folder Space saves its workspaces in.
pub const APP_FOLDER: &str = "rimelight-space";

/// One top-level Space workspace (see `rimelight_shell::workspace::WorkspaceTab`).
pub type WorkspaceTab = rimelight_shell::workspace::WorkspaceTab<SpaceRoutes>;

/// A workspace on the Space closed stack.
pub type ClosedWorkspace = rimelight_shell::workspace::ClosedWorkspace<SpaceRoutes>;

/// The top-level workspaces of Space.
pub type Workspaces = rimelight_shell::workspace::Workspaces<SpaceRoutes>;

/// The workspaces the app starts with, and the [`Saver`] that writes them back. See `rimelight_shell::workspace::startup`.
pub fn startup(store: Rc<dyn Store>) -> (Workspaces, Saver, bool) {
    rimelight_shell::workspace::startup::<SpaceRoutes>(store)
}
