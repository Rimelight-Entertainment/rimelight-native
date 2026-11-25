//! Drawing geometry for shapes and pen strokes: Slint path commands and stroke simplification, in item-local units
//! (0..w, 0..h). The UI feeds these strings to a Slint `Path` with `viewbox-width/height` set to the item's size.

use crate::geom::{Rect, Vec2};
use crate::model::ShapeKind;

/// Longest arrow head, in local units.
const HEAD_MAX: f64 = 26.0;
/// Arrow head length as a share of the line length.
const HEAD_SHARE: f64 = 0.35;
/// Arrow head angle on each side of the line, in degrees.
const HEAD_ANGLE: f64 = 28.0;

/// A point as the two SVG numbers `x y`, at most 2 decimals.
fn at(p: Vec2) -> String {
    let Vec2 { x, y } = p;
    format!("{x:.2} {y:.2}")
}

/// Path commands (SVG syntax, which Slint's Path `commands` accepts) for a shape drawn in a w x h box.
/// Rect: the box outline; Ellipse: two arcs (A commands) through the box; Diamond: the four edge midpoints;
/// Line/Arrow: from pts[0] to pts[1] (pts are local; default (0,0)-(w,h)); Arrow with `head`: plus two short head lines
/// at the end (length min(16, 0.3 * line length), 28 degrees each side) as separate "M .. L .." subpaths.
pub fn shape_path(shape: ShapeKind, w: f64, h: f64, pts: &[Vec2], head: bool) -> String {
    let (ws, hs) = (format!("{w:.2}"), format!("{h:.2}"));
    match shape {
        ShapeKind::Rect => format!("M0 0 L{ws} 0 L{ws} {hs} L0 {hs} Z"),
        ShapeKind::Ellipse => {
            let (rx, ry) = (format!("{:.2}", w / 2.0), format!("{:.2}", h / 2.0));
            let mid = format!("{:.2}", h / 2.0);
            format!("M0 {mid} A{rx} {ry} 0 0 1 {ws} {mid} A{rx} {ry} 0 0 1 0 {mid} Z")
        }
        ShapeKind::Diamond => {
            let (mx, my) = (format!("{:.2}", w / 2.0), format!("{:.2}", h / 2.0));
            format!("M{mx} 0 L{ws} {my} L{mx} {hs} L0 {my} Z")
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let p0 = pts.first().copied().unwrap_or(Vec2::ZERO);
            let p1 = pts.get(1).copied().unwrap_or(Vec2::new(w, h));
            let mut out = format!("M{} L{}", at(p0), at(p1));
            let d = p1 - p0;
            let len = d.length();
            if shape == ShapeKind::Arrow && head && len > 0.0 {
                let back = -(d / len);
                let head_len = (HEAD_SHARE * len).min(HEAD_MAX);
                for angle in [HEAD_ANGLE, -HEAD_ANGLE] {
                    let end = p1 + back.rotate(angle) * head_len;
                    out.push_str(&format!(" M{} L{}", at(p1), at(end)));
                }
            }
            out
        }
    }
}

/// A pen stroke as a smooth path: M p0, then quadratic curves through midpoints (Q p_i mid(p_i, p_i+1)), ending at the last
/// point. One point gives a tiny line so it is visible. Empty gives "".
pub fn stroke_path(pts: &[Vec2]) -> String {
    match pts {
        [] => String::new(),
        [p] => format!("M{} L{}", at(*p), at(*p + Vec2::new(0.1, 0.0))),
        [first, .., last] => {
            let mut out = format!("M{}", at(*first));
            for pair in pts.windows(2).skip(1) {
                let (control, next) = (pair[0], pair[1]);
                let mid = (control + next) / 2.0;
                out.push_str(&format!(" Q{} {}", at(control), at(mid)));
            }
            out.push_str(&format!(" L{}", at(*last)));
            out
        }
    }
}

/// Distance from `p` to the segment `a`-`b`.
fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let ap = p - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 <= f64::EPSILON {
        return ap.length();
    }
    let t = ((ap.x * ab.x + ap.y * ab.y) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Ramer-Douglas-Peucker simplification (keeps first and last).
pub fn simplify(pts: &[Vec2], tolerance: f64) -> Vec<Vec2> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let first = pts[0];
    let last = pts[pts.len() - 1];
    let mut worst = 0.0;
    let mut index = 0;
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = distance_to_segment(*p, first, last);
        if d > worst {
            worst = d;
            index = i;
        }
    }
    if worst > tolerance {
        let mut left = simplify(&pts[..=index], tolerance);
        let right = simplify(&pts[index..], tolerance);
        // Both halves hold the split point; keep it once.
        left.pop();
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

/// World points of a finished stroke (or line) to (the item's rect, local points relative to that rect's top-left),
/// padded by sw/2 on every side so the stroke is not clipped; a rect of zero width or height gets 1.
pub fn normalize_points(world: &[Vec2], sw: f64) -> (Rect, Vec<Vec2>) {
    let Some(first) = world.first() else {
        return (Rect::new(0.0, 0.0, 1.0, 1.0), Vec::new());
    };
    let (mut min, mut max) = (*first, *first);
    for p in world {
        min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
        max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
    }
    let pad = sw / 2.0;
    let mut w = max.x - min.x + 2.0 * pad;
    let mut h = max.y - min.y + 2.0 * pad;
    if w <= 0.0 {
        w = 1.0;
    }
    if h <= 0.0 {
        h = 1.0;
    }
    let origin = Vec2::new(min.x - pad, min.y - pad);
    let local = world.iter().map(|p| *p - origin).collect();
    (Rect::new(origin.x, origin.y, w, h), local)
}

/// True when `p` (local) is within `tolerance` of the polyline.
pub fn hits_polyline(pts: &[Vec2], p: Vec2, tolerance: f64) -> bool {
    match pts {
        [] => false,
        [only] => (*only - p).length() <= tolerance,
        _ => pts
            .windows(2)
            .any(|s| distance_to_segment(p, s[0], s[1]) <= tolerance),
    }
}

/// A square box from a drag: the side is the larger drag distance, and the box extends in the drag's direction.
pub fn square_drag(start: Vec2, current: Vec2) -> Rect {
    let d = current - start;
    let side = d.x.abs().max(d.y.abs());
    let x = if d.x < 0.0 { start.x - side } else { start.x };
    let y = if d.y < 0.0 { start.y - side } else { start.y };
    Rect::new(x, y, side, side)
}

/// A line end snapped to 45 degrees around `start`: the angle rounds to the nearest 45, the length is kept.
pub fn snap_45(start: Vec2, current: Vec2) -> Vec2 {
    let d = current - start;
    let len = d.length();
    if len <= f64::EPSILON {
        return start;
    }
    let angle = (d.y.atan2(d.x).to_degrees() / 45.0).round() * 45.0;
    let (sin, cos) = angle.to_radians().sin_cos();
    start + Vec2::new(cos, sin) * len
}

/// An item belongs to a frame when its centre is inside the frame's rect.
pub fn in_frame(frame: Rect, item: Rect) -> bool {
    frame.contains_point(item.center())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(s: &str, c: char) -> usize {
        s.chars().filter(|x| *x == c).count()
    }

    #[test]
    fn rect_path_is_a_closed_box() {
        let s = shape_path(ShapeKind::Rect, 10.0, 5.0, &[], false);
        assert!(s.starts_with('M'));
        assert_eq!(count(&s, 'L') + count(&s, 'Z'), 4);
    }

    #[test]
    fn ellipse_uses_arcs() {
        let s = shape_path(ShapeKind::Ellipse, 10.0, 5.0, &[], false);
        assert!(s.contains('A'));
    }

    #[test]
    fn arrow_with_head_has_three_subpaths() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0)];
        let s = shape_path(ShapeKind::Arrow, 40.0, 0.0, &pts, true);
        assert_eq!(count(&s, 'M'), 3);
        let plain = shape_path(ShapeKind::Arrow, 40.0, 0.0, &pts, false);
        assert_eq!(count(&plain, 'M'), 1);
    }

    #[test]
    fn stroke_of_three_points_has_quadratic() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(10.0, 0.0),
        ];
        let s = stroke_path(&pts);
        assert!(s.contains('Q'));
        assert_eq!(stroke_path(&[]), "");
    }

    #[test]
    fn simplify_keeps_endpoints_and_drops_collinear_middle() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(10.0, 0.0),
        ];
        assert_eq!(
            simplify(&pts, 0.5),
            vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]
        );
    }

    #[test]
    fn normalize_points_pads_by_half_stroke() {
        let world = [Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0)];
        let (rect, local) = normalize_points(&world, 4.0);
        assert_eq!(rect.x, 8.0);
        assert_eq!(rect.y, 8.0);
        assert_eq!(rect.w, 14.0);
        assert_eq!(rect.h, 14.0);
        assert_eq!(local[0], Vec2::new(2.0, 2.0));
    }

    #[test]
    fn hits_polyline_near_and_far() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        assert!(hits_polyline(&pts, Vec2::new(5.0, 2.0), 3.0));
        assert!(!hits_polyline(&pts, Vec2::new(5.0, 10.0), 3.0));
    }

    #[test]
    fn square_drag_is_square() {
        let r = square_drag(Vec2::new(0.0, 0.0), Vec2::new(30.0, 12.0));
        assert_eq!(r.w, r.h);
        assert_eq!(r.w, 30.0);
        let r = square_drag(Vec2::new(10.0, 10.0), Vec2::new(4.0, 13.0));
        assert_eq!(r, Rect::new(4.0, 10.0, 6.0, 6.0));
    }

    #[test]
    fn snap_45_snaps_thirty_degrees_to_diagonal() {
        let a = 30.0_f64.to_radians();
        let out = snap_45(Vec2::ZERO, Vec2::new(10.0 * a.cos(), 10.0 * a.sin()));
        assert!((out.x - out.y).abs() < 1e-9, "45 degrees has equal parts");
        assert!((out.length() - 10.0).abs() < 1e-9, "length kept");
    }

    #[test]
    fn in_frame_uses_centre() {
        let frame = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(in_frame(frame, Rect::new(40.0, 40.0, 20.0, 20.0)));
        assert!(!in_frame(frame, Rect::new(90.0, 90.0, 40.0, 40.0)));
    }
}
