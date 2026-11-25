//! space_canvas: canvas logic with no UI (CV.5, decision 0045).
//!
//! The canvas page (CV.6 onward) draws what this crate computes. Everything here is plain data and functions, so it
//! is unit tested without a window:
//! - [`geom`]: points, rectangles, rotated rectangles and the camera (pan and zoom).
//! - [`zorder`]: fractional z-order ranks (base 62 text that sorts with byte order, as Postgres `COLLATE "C"` does).
//! - [`model`]: the typed item model over the live room's `props`, and the document.
//! - [`ops`]: turning edits into live-room ops.
//! - [`undo`]: per-user undo and redo.
//! - [`arrange`]: PureRef-style organising (optimal pack, sort, normalise, align, distribute, stack, grid).
//! - [`bind`]: arrow and line ends attached to items: targets, anchor points and re-routing.
//! - [`hit`]: hit testing, marquee selection, selection handles, resize and rotate.
//! - [`snap`]: snapping to other items and to a grid.
//! - [`draw`]: Slint path commands and stroke geometry for shapes and pen strokes, in item-local units.
//! - [`grid`]: the grid mode, snapping to grid cells and the grid lines to draw at a zoom.
//! - [`crop`]: non-destructive image crop rectangles (fractions of the full image) and their drags.

pub mod arrange;
pub mod bind;
pub mod crop;
pub mod draw;
pub mod geom;
pub mod grid;
pub mod hit;
pub mod model;
pub mod ops;
pub mod snap;
pub mod undo;
pub mod zorder;
