//! Hit testing, marquee selection, selection handles, resize and rotate, in world units.

use crate::geom::{Rect, RotRect, Vec2};

/// An item as hit testing sees it, in draw order (bottom first).
#[derive(Debug, Clone, PartialEq)]
pub struct HitItem {
    pub id: String,
    pub shape: RotRect,
    pub locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marquee {
    Contain,
    Intersect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
    Rotate,
}

/// The topmost item under `p` (world units), with `tolerance` world units of slack. Locked items are skipped unless
/// `include_locked`.
pub fn hit_test(
    items: &[HitItem],
    p: Vec2,
    tolerance: f64,
    include_locked: bool,
) -> Option<String> {
    items
        .iter()
        .rev()
        .filter(|item| !item.locked || include_locked)
        .find(|item| {
            RotRect {
                rect: item.shape.rect.inflate(tolerance),
                rotation: item.shape.rotation,
            }
            .contains_point(p)
        })
        .map(|item| item.id.clone())
}

/// Ids (in draw order) inside a marquee rectangle: Contain = the item's aabb is inside it; Intersect = any overlap of
/// aabbs. Locked items are skipped.
pub fn marquee(items: &[HitItem], area: Rect, mode: Marquee) -> Vec<String> {
    items
        .iter()
        .filter(|item| !item.locked)
        .filter(|item| {
            let aabb = item.shape.aabb();
            match mode {
                Marquee::Contain => area.contains_rect(&aabb),
                Marquee::Intersect => area.intersects(&aabb),
            }
        })
        .map(|item| item.id.clone())
        .collect()
}

/// Centre of a shape in world units.
fn centre_of(rect: Rect) -> Vec2 {
    Vec2 {
        x: rect.x + rect.w / 2.0,
        y: rect.y + rect.h / 2.0,
    }
}

/// Converts a point given relative to the shape centre in its local frame into world units.
fn local_to_world(shape: RotRect, local: Vec2) -> Vec2 {
    centre_of(shape.rect) + local.rotate(shape.rotation)
}

/// Where each handle sits for a selection shape: the 8 points on corners and edge midpoints of the rotated rect, and Rotate at
/// `rotate_offset` world units above the top edge midpoint (rotated with the shape).
pub fn handle_positions(shape: RotRect, rotate_offset: f64) -> Vec<(Handle, Vec2)> {
    let hw = shape.rect.w / 2.0;
    let hh = shape.rect.h / 2.0;
    let local = [
        (Handle::N, Vec2 { x: 0.0, y: -hh }),
        (Handle::NE, Vec2 { x: hw, y: -hh }),
        (Handle::E, Vec2 { x: hw, y: 0.0 }),
        (Handle::SE, Vec2 { x: hw, y: hh }),
        (Handle::S, Vec2 { x: 0.0, y: hh }),
        (Handle::SW, Vec2 { x: -hw, y: hh }),
        (Handle::W, Vec2 { x: -hw, y: 0.0 }),
        (Handle::NW, Vec2 { x: -hw, y: -hh }),
        (
            Handle::Rotate,
            Vec2 {
                x: 0.0,
                y: -hh - rotate_offset,
            },
        ),
    ];
    local
        .into_iter()
        .map(|(handle, p)| (handle, local_to_world(shape, p)))
        .collect()
}

/// The handle within `radius` world units of `p` (closest wins), if any.
pub fn handle_at(shape: RotRect, p: Vec2, radius: f64, rotate_offset: f64) -> Option<Handle> {
    handle_positions(shape, rotate_offset)
        .into_iter()
        .map(|(handle, pos)| (handle, (pos - p).length()))
        .filter(|(_, dist)| *dist <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(handle, _)| handle)
}

/// Relative change of `new` against `old` (0 means unchanged).
fn relative_change(new: f64, old: f64) -> f64 {
    if old.abs() > f64::EPSILON {
        (new / old - 1.0).abs()
    } else {
        (new - old).abs()
    }
}

/// Resizes a (possibly rotated) shape by dragging `handle` by `delta` (world units). The delta is first taken into the shape's
/// own frame (rotate by -rotation). Edges move with the drag; the opposite edge or corner stays fixed (in world space, so the
/// centre moves accordingly for rotated shapes). `keep_aspect`: corner handles keep w/h (use the larger relative change);
/// `from_center`: both opposite sides move symmetrically and the centre stays put. Width and height never go below `min`.
pub fn resize(
    shape: RotRect,
    handle: Handle,
    delta: Vec2,
    keep_aspect: bool,
    from_center: bool,
    min: f64,
) -> RotRect {
    // Which edges the handle moves: -1 = left/top, 1 = right/bottom, 0 = not on this axis.
    let (sign_x, sign_y): (i32, i32) = match handle {
        Handle::N => (0, -1),
        Handle::NE => (1, -1),
        Handle::E => (1, 0),
        Handle::SE => (1, 1),
        Handle::S => (0, 1),
        Handle::SW => (-1, 1),
        Handle::W => (-1, 0),
        Handle::NW => (-1, -1),
        Handle::Rotate => return shape,
    };
    let r = shape.rect;
    let d = delta.rotate(-shape.rotation);

    // Dragging one side by d moves the width by d (or 2d when mirrored around the centre).
    let mirror = if from_center { 2.0 } else { 1.0 };
    let mut w = r.w + f64::from(sign_x) * d.x * mirror;
    let mut h = r.h + f64::from(sign_y) * d.y * mirror;

    if sign_x != 0 && sign_y != 0 && keep_aspect {
        let ratio = if r.w > 0.0 && r.h > 0.0 {
            r.w / r.h
        } else {
            1.0
        };
        if relative_change(w, r.w) >= relative_change(h, r.h) {
            h = w / ratio;
        } else {
            w = h * ratio;
        }
    }

    w = w.max(min);
    h = h.max(min);

    // New centre in the local frame, relative to the old centre. The side opposite the handle stays fixed.
    let (cx, cy) = if from_center {
        (0.0, 0.0)
    } else {
        let cx = match sign_x {
            1 => -r.w / 2.0 + w / 2.0,
            -1 => r.w / 2.0 - w / 2.0,
            _ => 0.0,
        };
        let cy = match sign_y {
            1 => -r.h / 2.0 + h / 2.0,
            -1 => r.h / 2.0 - h / 2.0,
            _ => 0.0,
        };
        (cx, cy)
    };

    let centre = centre_of(r) + Vec2 { x: cx, y: cy }.rotate(shape.rotation);
    RotRect {
        rect: Rect {
            x: centre.x - w / 2.0,
            y: centre.y - h / 2.0,
            w,
            h,
        },
        rotation: shape.rotation,
    }
}

/// The rotation (degrees) for dragging the rotate handle from `start` to `current` around `center`, added to `initial`.
/// With `snap`, the result is rounded to the nearest 15 degrees. Result normalised to (-180, 180].
pub fn rotate_to(center: Vec2, start: Vec2, current: Vec2, initial: f64, snap: bool) -> f64 {
    let a0 = (start.y - center.y).atan2(start.x - center.x).to_degrees();
    let a1 = (current.y - center.y)
        .atan2(current.x - center.x)
        .to_degrees();
    let mut result = initial + (a1 - a0);
    if snap {
        result = (result / 15.0).round() * 15.0;
    }
    normalise_degrees(result)
}

/// Maps degrees into (-180, 180].
fn normalise_degrees(degrees: f64) -> f64 {
    let m = degrees.rem_euclid(360.0);
    if m > 180.0 { m - 360.0 } else { m }
}

/// Maps each rect from `from` bounds to `to` bounds (multi-selection resize): positions and sizes scale with the box.
pub fn scale_rects(rects: &[Rect], from: Rect, to: Rect) -> Vec<Rect> {
    let sx = if from.w == 0.0 { 1.0 } else { to.w / from.w };
    let sy = if from.h == 0.0 { 1.0 } else { to.h / from.h };
    rects
        .iter()
        .map(|r| Rect {
            x: to.x + (r.x - from.x) * sx,
            y: to.y + (r.y - from.y) * sy,
            w: r.w * sx,
            h: r.h * sy,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-6;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "expected {b}, got {a}");
    }

    fn rr(x: f64, y: f64, w: f64, h: f64, rotation: f64) -> RotRect {
        RotRect {
            rect: Rect { x, y, w, h },
            rotation,
        }
    }

    fn item(id: &str, shape: RotRect, locked: bool) -> HitItem {
        HitItem {
            id: id.to_string(),
            shape,
            locked,
        }
    }

    fn pt(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }

    #[test]
    fn hit_test_returns_top_of_overlapping() {
        let items = [
            item("a", rr(0.0, 0.0, 100.0, 100.0, 0.0), false),
            item("b", rr(50.0, 50.0, 100.0, 100.0, 0.0), false),
        ];
        assert_eq!(
            hit_test(&items, pt(75.0, 75.0), 0.0, false).as_deref(),
            Some("b")
        );
        assert_eq!(
            hit_test(&items, pt(10.0, 10.0), 0.0, false).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn hit_test_skips_locked() {
        let items = [
            item("under", rr(0.0, 0.0, 100.0, 100.0, 0.0), false),
            item("top", rr(0.0, 0.0, 100.0, 100.0, 0.0), true),
        ];
        assert_eq!(
            hit_test(&items, pt(50.0, 50.0), 0.0, false).as_deref(),
            Some("under")
        );
        assert_eq!(
            hit_test(&items, pt(50.0, 50.0), 0.0, true).as_deref(),
            Some("top")
        );
    }

    #[test]
    fn hit_test_tolerance_catches_point_just_outside() {
        let items = [item("a", rr(0.0, 0.0, 100.0, 100.0, 0.0), false)];
        assert_eq!(hit_test(&items, pt(-2.0, 50.0), 0.0, false), None);
        assert_eq!(
            hit_test(&items, pt(-2.0, 50.0), 3.0, false).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn hit_test_rotated_square_ignores_aabb_corner() {
        let items = [item("sq", rr(0.0, 0.0, 100.0, 100.0, 45.0), false)];
        // Inside the aabb but outside the rotated square.
        assert_eq!(hit_test(&items, pt(2.0, 2.0), 0.0, false), None);
        // The centre is inside.
        assert_eq!(
            hit_test(&items, pt(50.0, 50.0), 0.0, false).as_deref(),
            Some("sq")
        );
    }

    #[test]
    fn marquee_contain_vs_intersect() {
        let items = [
            item("a", rr(10.0, 10.0, 20.0, 20.0, 0.0), false),
            item("b", rr(90.0, 90.0, 30.0, 30.0, 0.0), false),
            item("locked", rr(20.0, 20.0, 10.0, 10.0, 0.0), true),
        ];
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        assert_eq!(
            marquee(&items, area, Marquee::Contain),
            vec!["a".to_string()]
        );
        assert_eq!(
            marquee(&items, area, Marquee::Intersect),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn handle_at_finds_corner_and_rotate() {
        let shape = rr(0.0, 0.0, 100.0, 50.0, 0.0);
        assert_eq!(
            handle_at(shape, pt(100.0, 50.0), 5.0, 20.0),
            Some(Handle::SE)
        );
        assert_eq!(
            handle_at(shape, pt(50.0, -20.0), 5.0, 20.0),
            Some(Handle::Rotate)
        );
        assert_eq!(handle_at(shape, pt(50.0, 25.0), 5.0, 20.0), None);
    }

    #[test]
    fn resize_se_grows_from_top_left() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 0.0),
            Handle::SE,
            pt(10.0, 20.0),
            false,
            false,
            1.0,
        );
        close(out.rect.x, 0.0);
        close(out.rect.y, 0.0);
        close(out.rect.w, 110.0);
        close(out.rect.h, 70.0);
    }

    #[test]
    fn resize_w_moves_left_edge() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 0.0),
            Handle::W,
            pt(10.0, 0.0),
            false,
            false,
            1.0,
        );
        close(out.rect.x, 10.0);
        close(out.rect.w, 90.0);
        close(out.rect.h, 50.0);
    }

    #[test]
    fn resize_from_center_keeps_centre() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 0.0),
            Handle::E,
            pt(10.0, 0.0),
            false,
            true,
            1.0,
        );
        close(out.rect.w, 120.0);
        close(out.rect.x + out.rect.w / 2.0, 50.0);
        close(out.rect.y + out.rect.h / 2.0, 25.0);
    }

    #[test]
    fn resize_keep_aspect_keeps_ratio() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 0.0),
            Handle::SE,
            pt(10.0, 20.0),
            true,
            false,
            1.0,
        );
        close(out.rect.w / out.rect.h, 2.0);
        close(out.rect.x, 0.0);
        close(out.rect.y, 0.0);
    }

    #[test]
    fn resize_clamps_to_min() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 0.0),
            Handle::SE,
            pt(-500.0, -500.0),
            false,
            false,
            10.0,
        );
        close(out.rect.w, 10.0);
        close(out.rect.h, 10.0);
        close(out.rect.x, 0.0);
        close(out.rect.y, 0.0);
    }

    #[test]
    fn resize_rotated_e_handle_uses_local_axis() {
        let out = resize(
            rr(0.0, 0.0, 100.0, 50.0, 90.0),
            Handle::E,
            pt(0.0, 10.0),
            false,
            false,
            1.0,
        );
        close(out.rect.w, 110.0);
        close(out.rect.h, 50.0);
    }

    #[test]
    fn rotate_to_quarter_turn_and_snap() {
        let c = pt(0.0, 0.0);
        close(rotate_to(c, pt(1.0, 0.0), pt(0.0, 1.0), 0.0, false), 90.0);
        let a = 52.0_f64.to_radians();
        close(
            rotate_to(c, pt(1.0, 0.0), pt(a.cos(), a.sin()), 0.0, false),
            52.0,
        );
        close(
            rotate_to(c, pt(1.0, 0.0), pt(a.cos(), a.sin()), 0.0, true),
            45.0,
        );
    }

    #[test]
    fn scale_rects_doubles() {
        let from = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        };
        let to = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let out = scale_rects(
            &[Rect {
                x: 10.0,
                y: 10.0,
                w: 10.0,
                h: 10.0,
            }],
            from,
            to,
        );
        close(out[0].x, 20.0);
        close(out[0].y, 20.0);
        close(out[0].w, 20.0);
        close(out[0].h, 20.0);
    }
}
