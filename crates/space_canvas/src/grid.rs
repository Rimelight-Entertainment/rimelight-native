//! The canvas grid: the mode, snapping positions and sizes to grid cells, and the grid lines to draw at a zoom.
//! Everything is in world units. The grid is a per-user view setting, not stored on the canvas.

use crate::geom::Rect;

/// What the grid draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridMode {
    #[default]
    None,
    Lines,
    Dots,
}

/// The grid cell in world units.
pub const CELL: f64 = 32.0;

/// Smallest screen distance, in pixels, between two drawn grid lines or dots.
const MIN_SCREEN_STEP: f64 = 8.0;

/// Rounds to the nearest multiple of `cell`. A cell that is not positive leaves `v` as it is.
pub fn snap(v: f64, cell: f64) -> f64 {
    if cell <= 0.0 {
        return v;
    }
    (v / cell).round() * cell
}

/// Snaps a rect's top-left to the grid, keeping its size.
pub fn snap_rect_position(r: Rect, cell: f64) -> Rect {
    Rect::new(snap(r.x, cell), snap(r.y, cell), r.w, r.h)
}

/// Snaps a rect's edges for a resize: left/top/right/bottom each to the grid, keeping at least one cell of size.
pub fn snap_rect_edges(r: Rect, cell: f64) -> Rect {
    if cell <= 0.0 {
        return r;
    }
    let left = snap(r.left(), cell);
    let top = snap(r.top(), cell);
    let mut right = snap(r.right(), cell);
    let mut bottom = snap(r.bottom(), cell);
    if right - left < cell {
        right = left + cell;
    }
    if bottom - top < cell {
        bottom = top + cell;
    }
    Rect::new(left, top, right - left, bottom - top)
}

/// The grid step to draw at a zoom: CELL, doubled until it is at least 8 screen px apart.
pub fn visible_step(zoom: f64) -> f64 {
    let mut step = CELL;
    // Bounded: 32 doublings reach any finite zoom, and a zero or NaN zoom stops here too.
    for _ in 0..32 {
        if step * zoom >= MIN_SCREEN_STEP {
            break;
        }
        step *= 2.0;
    }
    step
}

/// The world positions of one axis' grid lines from `from` to `to`, every `step`, at most `max` of them.
fn axis(from: f64, to: f64, step: f64, max: usize) -> Vec<f64> {
    let mut out = Vec::new();
    let mut v = (from / step).ceil() * step;
    while v <= to && out.len() < max {
        out.push(v);
        v += step;
    }
    out
}

/// World x and y positions of the grid lines inside `visible` (world rect) with `step`, at most `max` per axis.
/// A step that is not finite and positive gives no lines.
pub fn lines(visible: Rect, step: f64, max: usize) -> (Vec<f64>, Vec<f64>) {
    if !step.is_finite() || step <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    (
        axis(visible.left(), visible.right(), step, max),
        axis(visible.top(), visible.bottom(), step, max),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_rounds_to_nearest_cell() {
        assert_eq!(snap(50.0, CELL), 64.0);
        assert_eq!(snap(40.0, CELL), 32.0);
        assert_eq!(snap(-40.0, CELL), -32.0);
    }

    #[test]
    fn snap_rect_position_keeps_size() {
        let r = snap_rect_position(Rect::new(40.0, 70.0, 10.0, 12.0), CELL);
        assert_eq!(r, Rect::new(32.0, 64.0, 10.0, 12.0));
    }

    #[test]
    fn snap_rect_edges_keeps_one_cell() {
        let r = snap_rect_edges(Rect::new(0.0, 0.0, 5.0, 5.0), CELL);
        assert_eq!(r, Rect::new(0.0, 0.0, CELL, CELL));
    }

    #[test]
    fn visible_step_grows_when_zoomed_out() {
        assert!(visible_step(0.1) >= 80.0);
        assert_eq!(visible_step(1.0), CELL);
    }

    #[test]
    fn lines_are_bounded_by_max() {
        let (xs, ys) = lines(Rect::new(0.0, 0.0, 10000.0, 10000.0), CELL, 5);
        assert_eq!(xs.len(), 5);
        assert_eq!(ys.len(), 5);
    }

    #[test]
    fn lines_start_at_first_grid_line_inside() {
        let (xs, ys) = lines(Rect::new(10.0, 10.0, 100.0, 100.0), CELL, 100);
        assert_eq!(xs, vec![32.0, 64.0, 96.0]);
        assert_eq!(ys, vec![32.0, 64.0, 96.0]);
    }
}
