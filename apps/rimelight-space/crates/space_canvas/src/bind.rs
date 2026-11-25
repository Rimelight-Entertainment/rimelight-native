//! Arrow and line binding: the ends of a connector can attach to other items.
//!
//! A connector is a Shape whose outline is a line or an arrow. Its two ends are the first and last point of `pts`,
//! measured from the top-left of its rectangle. When an end is bound, the end sits on the edge of the item it is
//! bound to, a small gap away, pointing toward the other end. Everything here is pure geometry.

use crate::geom::{Rect, RotRect, Vec2};
use crate::model::{Item, ItemData, ShapeKind};

/// The gap, in canvas units, between a bound end and the edge of its item.
pub const BIND_GAP: f64 = 6.0;

/// True for items an arrow end can attach to: images, videos, text, frames and shapes other than lines and arrows.
/// Strokes no.
pub fn bindable(item: &Item) -> bool {
    match &item.data {
        ItemData::Image { .. }
        | ItemData::Video { .. }
        | ItemData::Text { .. }
        | ItemData::Frame { .. } => true,
        ItemData::Shape { shape, .. } => !matches!(shape, ShapeKind::Line | ShapeKind::Arrow),
        ItemData::Stroke { .. } => false,
    }
}

/// True for a Shape whose shape is Line or Arrow.
pub fn is_connector(item: &Item) -> bool {
    matches!(
        &item.data,
        ItemData::Shape {
            shape: ShapeKind::Line | ShapeKind::Arrow,
            ..
        }
    )
}

/// The topmost bindable item (last in `items`, which is in draw order) whose rotated rectangle, inflated by
/// BIND_GAP, contains `p`. Items whose id is in `exclude` are skipped.
pub fn target_at(items: &[&Item], p: Vec2, exclude: &[&str]) -> Option<String> {
    items
        .iter()
        .rev()
        .find(|item| {
            bindable(item)
                && !exclude.contains(&item.id.as_str())
                && RotRect::new(item.rect.inflate(BIND_GAP), item.rot).contains_point(p)
        })
        .map(|item| item.id.clone())
}

/// Where the line from the item's centre toward `toward` leaves the item, moved BIND_GAP further out.
///
/// The work is done in the item's own unrotated frame, then turned back. When `toward` is the centre, the centre
/// itself is returned.
pub fn anchor(item: &Item, toward: Vec2) -> Vec2 {
    let c = item.rect.center();
    let d = (toward - c).rotate(-item.rot);
    let len = d.length();
    if len < 1e-9 {
        return c;
    }
    let hw = item.rect.w / 2.0;
    let hh = item.rect.h / 2.0;
    // Scale the direction until it reaches the edge: the smaller of the two ratios wins.
    let mut t = f64::INFINITY;
    if d.x.abs() > 1e-12 {
        t = t.min(hw / d.x.abs());
    }
    if d.y.abs() > 1e-12 {
        t = t.min(hh / d.y.abs());
    }
    let local = d * t + d / len * BIND_GAP;
    c + local.rotate(item.rot)
}

/// The connector's world endpoints, or None when it is not a connector or has fewer than 2 points.
pub fn endpoints(item: &Item) -> Option<(Vec2, Vec2)> {
    let ItemData::Shape { pts, .. } = &item.data else {
        return None;
    };
    if !is_connector(item) || pts.len() < 2 {
        return None;
    }
    let origin = item.rect.top_left();
    Some((origin + pts[0], origin + pts[pts.len() - 1]))
}

/// Sets a connector's endpoints in world units. The rectangle spans both points, with width and height at least 1.
/// Does nothing for a non-connector.
pub fn set_endpoints(item: &mut Item, start: Vec2, end: Vec2) {
    if !is_connector(item) {
        return;
    }
    let mut rect = Rect::from_points(start, end);
    rect.w = rect.w.max(1.0);
    rect.h = rect.h.max(1.0);
    let origin = rect.top_left();
    item.rect = rect;
    if let ItemData::Shape { pts, .. } = &mut item.data {
        *pts = vec![start - origin, end - origin];
    }
}

/// Re-routes a connector to the items its ends are bound to.
///
/// `start` and `end` are the bound items, None when that end is free or its item is gone. A bound end moves to its
/// item's anchor point. The reference point is the other end's bound item's centre, or the other end's current
/// point when that end is free. Free ends do not move.
pub fn route(item: &mut Item, start: Option<&Item>, end: Option<&Item>) {
    let Some((cur_start, cur_end)) = endpoints(item) else {
        return;
    };
    let new_start = start.map_or(cur_start, |s| {
        anchor(s, end.map_or(cur_end, |e| e.rect.center()))
    });
    let new_end = end.map_or(cur_end, |e| {
        anchor(e, start.map_or(cur_start, |s| s.rect.center()))
    });
    set_endpoints(item, new_start, new_end);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn close_v(a: Vec2, b: Vec2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    /// A plain frame (bindable) at the given rectangle.
    fn block(id: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item::new(
            id.to_string(),
            "a0".to_string(),
            Rect::new(x, y, w, h),
            ItemData::Frame {
                title: String::new(),
            },
        )
    }

    /// An arrow whose rectangle is `rect` and whose points are placed by `set_endpoints` in the tests.
    fn connector(id: &str, rect: Rect) -> Item {
        Item::new(
            id.to_string(),
            "a9".to_string(),
            rect,
            ItemData::Shape {
                shape: ShapeKind::Arrow,
                stroke: "ink".to_string(),
                fill: None,
                sw: 2.0,
                pts: vec![Vec2::ZERO, Vec2::new(rect.w, rect.h)],
                start_bind: None,
                end_bind: None,
                head: true,
            },
        )
    }

    fn stroke(id: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item::new(
            id.to_string(),
            "a0".to_string(),
            Rect::new(x, y, w, h),
            ItemData::Stroke {
                pts: Vec::new(),
                color: "ink".to_string(),
                sw: 3.0,
            },
        )
    }

    #[test]
    fn anchor_on_axis_aligned_rect_leaves_the_edge_plus_gap() {
        let item = block("a", 0.0, 0.0, 100.0, 50.0);
        assert!(close_v(
            anchor(&item, Vec2::new(500.0, 25.0)),
            Vec2::new(106.0, 25.0)
        ));
    }

    #[test]
    fn anchor_toward_the_centre_returns_the_centre() {
        let item = block("a", 0.0, 0.0, 100.0, 50.0);
        assert!(close_v(
            anchor(&item, Vec2::new(50.0, 25.0)),
            Vec2::new(50.0, 25.0)
        ));
    }

    #[test]
    fn anchor_on_a_quarter_turned_item_follows_the_rotation() {
        // Rotated 90 degrees the item is 50 wide and 100 tall around (50, 25), so its right edge is at x = 75.
        let mut item = block("a", 0.0, 0.0, 100.0, 50.0);
        item.rot = 90.0;
        assert!(close_v(
            anchor(&item, Vec2::new(500.0, 25.0)),
            Vec2::new(81.0, 25.0)
        ));
    }

    #[test]
    fn target_at_picks_the_topmost_and_honours_exclude() {
        let bottom = block("bottom", 0.0, 0.0, 100.0, 100.0);
        let top = block("top", 0.0, 0.0, 100.0, 100.0);
        let items: Vec<&Item> = vec![&bottom, &top];
        let p = Vec2::new(50.0, 50.0);
        assert_eq!(target_at(&items, p, &[]), Some("top".to_string()));
        assert_eq!(target_at(&items, p, &["top"]), Some("bottom".to_string()));
    }

    #[test]
    fn target_at_skips_strokes_and_connectors() {
        let bottom = block("bottom", 0.0, 0.0, 100.0, 100.0);
        let pen = stroke("pen", 0.0, 0.0, 100.0, 100.0);
        let arrow = connector("arrow", Rect::new(0.0, 0.0, 100.0, 100.0));
        let p = Vec2::new(50.0, 50.0);
        let items: Vec<&Item> = vec![&bottom, &pen, &arrow];
        assert_eq!(target_at(&items, p, &[]), Some("bottom".to_string()));
        let only_pen: Vec<&Item> = vec![&pen];
        assert_eq!(target_at(&only_pen, p, &[]), None);
    }

    #[test]
    fn target_at_includes_the_gap_around_the_item() {
        let item = block("a", 0.0, 0.0, 100.0, 100.0);
        let items: Vec<&Item> = vec![&item];
        assert_eq!(
            target_at(&items, Vec2::new(104.0, 50.0), &[]),
            Some("a".to_string())
        );
        assert_eq!(target_at(&items, Vec2::new(107.0, 50.0), &[]), None);
    }

    #[test]
    fn set_endpoints_keeps_size_at_least_one() {
        let mut conn = connector("c", Rect::new(0.0, 0.0, 1.0, 1.0));
        set_endpoints(&mut conn, Vec2::new(10.0, 10.0), Vec2::new(10.0, 10.0));
        assert!(conn.rect.w >= 1.0);
        assert!(conn.rect.h >= 1.0);
        let (s, e) = endpoints(&conn).expect("connector");
        assert!(close_v(s, Vec2::new(10.0, 10.0)));
        assert!(close_v(e, Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn set_endpoints_does_nothing_for_a_non_connector() {
        let mut frame = block("f", 1.0, 2.0, 3.0, 4.0);
        set_endpoints(&mut frame, Vec2::new(0.0, 0.0), Vec2::new(50.0, 50.0));
        assert_eq!(frame.rect, Rect::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(endpoints(&frame), None);
    }

    #[test]
    fn route_moves_only_the_bound_end() {
        let a = block("a", 0.0, 0.0, 100.0, 50.0);
        let mut conn = connector("c", Rect::new(0.0, 0.0, 1.0, 1.0));
        set_endpoints(&mut conn, Vec2::new(200.0, 25.0), Vec2::new(300.0, 25.0));
        route(&mut conn, Some(&a), None);
        let (s, e) = endpoints(&conn).expect("connector");
        assert!(close_v(s, Vec2::new(106.0, 25.0)));
        assert!(close_v(e, Vec2::new(300.0, 25.0)));
    }

    #[test]
    fn route_with_both_ends_bound_uses_the_other_centre() {
        let a = block("a", 0.0, 0.0, 100.0, 50.0);
        let b = block("b", 400.0, 0.0, 100.0, 50.0);
        let mut conn = connector("c", Rect::new(0.0, 0.0, 1.0, 1.0));
        set_endpoints(&mut conn, Vec2::new(200.0, 25.0), Vec2::new(300.0, 25.0));
        route(&mut conn, Some(&a), Some(&b));
        let (s, e) = endpoints(&conn).expect("connector");
        assert!(close_v(s, Vec2::new(106.0, 25.0)));
        assert!(close_v(e, Vec2::new(394.0, 25.0)));
    }

    #[test]
    fn route_with_no_bound_items_changes_nothing() {
        let mut conn = connector("c", Rect::new(0.0, 0.0, 1.0, 1.0));
        set_endpoints(&mut conn, Vec2::new(5.0, 5.0), Vec2::new(60.0, 40.0));
        let before = conn.clone();
        route(&mut conn, None, None);
        assert_eq!(conn, before);
    }
}
