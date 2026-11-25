//! Tile groups (planned feature B, tiles). The model is in `rimelight_shell::tiles` (ST2.5b). This module keeps the old
//! paths and names Space's routes: [`TileLayout`] and [`Group`] are the shared models over [`SpaceRoutes`].

use super::routes::SpaceRoutes;

pub use rimelight_shell::tiles::{
    Edge, GROUP_CAP, GroupId, GroupRect, HandleRect, MoveError, Orientation, RATIO_MAX, RATIO_MIN,
    Side, SplitError, clamp_ratio,
};

/// The tile groups of one Space workspace.
pub type TileLayout = rimelight_shell::tiles::TileLayout<SpaceRoutes>;

/// One tile group of a Space workspace.
pub type Group = rimelight_shell::tiles::Group<SpaceRoutes>;
