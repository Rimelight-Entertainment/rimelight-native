//! Tab drag rules (planned feature B, tiles, T2b). The model is in `rimelight_shell::tile_drop` (ST2.5b). This module keeps
//! the old paths: the drop functions are generic over the layout's routes, and Space's layouts are [`super::tiles::TileLayout`].

pub use rimelight_shell::tile_drop::{
    DropGroup, EDGE_FRACTION, Hit, Plan, STRIP_HEIGHT, STRIP_PINNED_WIDTH, STRIP_TAB_WIDTH, Zone,
    drop_groups, hit_test, insertion_slot, pinned_flags, plan, reorder, slot_x, zone_slot,
};
