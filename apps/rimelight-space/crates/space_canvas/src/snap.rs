//! Snapping a moving rectangle to other rectangles, and snapping points to a grid.

use crate::geom::{Rect, Vec2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    Vertical { x: f64, from: f64, to: f64 },
    Horizontal { y: f64, from: f64, to: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapped {
    pub offset: Vec2,
    pub guides: Vec<Guide>,
}

/// Tolerance for two lines counting as the same line when drawing guides (world units).
const LINE_EPS: f64 = 1e-6;

/// The three lines of a rect on the x axis: left, centre, right.
fn x_lines(r: &Rect) -> [f64; 3] {
    [r.x, r.x + r.w / 2.0, r.x + r.w]
}

/// The three lines of a rect on the y axis: top, centre, bottom.
fn y_lines(r: &Rect) -> [f64; 3] {
    [r.y, r.y + r.h / 2.0, r.y + r.h]
}

/// The closest pair of lines within `threshold`: returns (offset to add to `mine`, the snapped line position).
/// Ties keep the first pair found.
fn best_snap(
    mine: [f64; 3],
    others: impl Iterator<Item = [f64; 3]>,
    threshold: f64,
) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for theirs in others {
        for line in theirs {
            for m in mine {
                let d = line - m;
                if d.abs() <= threshold && best.is_none_or(|(b, _)| d.abs() < b.abs()) {
                    best = Some((d, line));
                }
            }
        }
    }
    best
}

/// Snaps a moving rectangle to the others: its left, centre and right line up with any other's left, centre or right
/// (and likewise top, centre, bottom), when the distance is at most `threshold` world units. Each axis takes the smallest
/// distance found. `offset` is what to add to the moving rect. `guides` are the lines to draw for the snapped axes,
/// spanning from the moving rect to the furthest rect lined up on that line.
pub fn snap_move(moving: Rect, others: &[Rect], threshold: f64) -> Snapped {
    let sx = best_snap(x_lines(&moving), others.iter().map(x_lines), threshold);
    let sy = best_snap(y_lines(&moving), others.iter().map(y_lines), threshold);

    let dx = sx.map_or(0.0, |(d, _)| d);
    let dy = sy.map_or(0.0, |(d, _)| d);
    let moved = Rect {
        x: moving.x + dx,
        y: moving.y + dy,
        w: moving.w,
        h: moving.h,
    };

    let mut guides = Vec::new();

    if let Some((_, line)) = sx {
        let mut from = moved.y;
        let mut to = moved.y + moved.h;
        for r in others {
            if x_lines(r).iter().any(|l| (l - line).abs() <= LINE_EPS) {
                from = from.min(r.y);
                to = to.max(r.y + r.h);
            }
        }
        guides.push(Guide::Vertical { x: line, from, to });
    }

    if let Some((_, line)) = sy {
        let mut from = moved.x;
        let mut to = moved.x + moved.w;
        for r in others {
            if y_lines(r).iter().any(|l| (l - line).abs() <= LINE_EPS) {
                from = from.min(r.x);
                to = to.max(r.x + r.w);
            }
        }
        guides.push(Guide::Horizontal { y: line, from, to });
    }

    Snapped {
        offset: Vec2 { x: dx, y: dy },
        guides,
    }
}

/// Rounds a point to a grid of `size` world units (size <= 0: unchanged).
pub fn snap_to_grid(p: Vec2, size: f64) -> Vec2 {
    if size <= 0.0 {
        return p;
    }
    Vec2 {
        x: (p.x / size).round() * size,
        y: (p.y / size).round() * size,
    }
}

/// The threshold in world units for a screen distance at a zoom (screen_px / zoom).
pub fn world_threshold(screen_px: f64, zoom: f64) -> f64 {
    screen_px / zoom
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-6;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "expected {b}, got {a}");
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn snap_move_snaps_left_to_right_with_guide() {
        let moving = rect(105.0, 0.0, 50.0, 50.0);
        let others = [rect(0.0, 200.0, 100.0, 50.0)];
        let out = snap_move(moving, &others, 8.0);
        close(out.offset.x, -5.0);
        close(out.offset.y, 0.0);
        assert_eq!(out.guides.len(), 1);
        match out.guides[0] {
            Guide::Vertical { x, from, to } => {
                close(x, 100.0);
                close(from, 0.0);
                close(to, 250.0);
            }
            other => panic!("expected a vertical guide, got {other:?}"),
        }
    }

    #[test]
    fn snap_move_beyond_threshold_does_nothing() {
        let moving = rect(110.0, 0.0, 50.0, 50.0);
        let others = [rect(0.0, 200.0, 100.0, 50.0)];
        let out = snap_move(moving, &others, 8.0);
        assert_eq!(out.offset, Vec2 { x: 0.0, y: 0.0 });
        assert!(out.guides.is_empty());
    }

    #[test]
    fn snap_to_grid_rounds() {
        let p = snap_to_grid(Vec2 { x: 13.0, y: -7.0 }, 10.0);
        close(p.x, 10.0);
        close(p.y, -10.0);
        assert_eq!(
            snap_to_grid(Vec2 { x: 13.0, y: -7.0 }, 0.0),
            Vec2 { x: 13.0, y: -7.0 }
        );
    }

    #[test]
    fn world_threshold_divides_by_zoom() {
        close(world_threshold(8.0, 2.0), 4.0);
    }
}
