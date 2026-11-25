//! Non-destructive image cropping. A crop is a rect of fractions 0..1 of the full image (x, y, w, h); the item shows
//! only that part. These functions map between the full image's rect and the item's rect, and drag crop handles.

use crate::geom::{Rect, Vec2};
use crate::hit::Handle;

/// The smallest crop side, as a fraction of the full image.
pub const MIN_FRAC: f64 = 0.02;

/// The whole image as a crop.
pub const WHOLE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1.0,
    h: 1.0,
};

/// Clamps `v` into `lo..=hi`; when `hi` is below `lo`, the result is `lo` (never panics).
fn clamp_to(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

/// The full image's rect in world units for an item showing `crop` inside `item_rect` (same scale): the inverse of cropping.
pub fn full_rect(item_rect: Rect, crop: Rect) -> Rect {
    let w = item_rect.w / crop.w.max(f64::EPSILON);
    let h = item_rect.h / crop.h.max(f64::EPSILON);
    Rect::new(item_rect.x - crop.x * w, item_rect.y - crop.y * h, w, h)
}

/// The item rect for showing `crop` of an image whose full rect is `full` (keeps scale and position of the visible part).
pub fn item_rect(full: Rect, crop: Rect) -> Rect {
    Rect::new(
        full.x + crop.x * full.w,
        full.y + crop.y * full.h,
        crop.w * full.w,
        crop.h * full.h,
    )
}

/// Drags a crop handle by a world delta, in the full image's frame (unrotated): returns the new crop, clamped to 0..1 with
/// at least MIN_FRAC size. Handle::Rotate leaves it unchanged; a drag with no handle (None) moves the crop window.
pub fn drag_crop(full: Rect, crop: Rect, handle: Option<Handle>, delta: Vec2) -> Rect {
    if full.is_empty() || handle == Some(Handle::Rotate) {
        return crop;
    }
    let dx = delta.x / full.w;
    let dy = delta.y / full.h;
    let (left, top, right, bottom) = (crop.x, crop.y, crop.x + crop.w, crop.y + crop.h);
    match handle {
        None => {
            let x = clamp_to(left + dx, 0.0, 1.0 - crop.w);
            let y = clamp_to(top + dy, 0.0, 1.0 - crop.h);
            Rect::new(x, y, crop.w, crop.h)
        }
        Some(handle) => {
            let (mut l, mut t, mut r, mut b) = (left, top, right, bottom);
            if matches!(handle, Handle::NW | Handle::W | Handle::SW) {
                l = clamp_to(left + dx, 0.0, right - MIN_FRAC);
            }
            if matches!(handle, Handle::NE | Handle::E | Handle::SE) {
                r = clamp_to(right + dx, left + MIN_FRAC, 1.0);
            }
            if matches!(handle, Handle::NW | Handle::N | Handle::NE) {
                t = clamp_to(top + dy, 0.0, bottom - MIN_FRAC);
            }
            if matches!(handle, Handle::SW | Handle::S | Handle::SE) {
                b = clamp_to(bottom + dy, top + MIN_FRAC, 1.0);
            }
            Rect::new(l, t, r - l, b - t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    fn close_rect(a: Rect, b: Rect) {
        close(a.x, b.x);
        close(a.y, b.y);
        close(a.w, b.w);
        close(a.h, b.h);
    }

    #[test]
    fn full_and_item_rect_round_trip() {
        let full = Rect::new(10.0, 20.0, 200.0, 100.0);
        let crop = Rect::new(0.25, 0.5, 0.5, 0.25);
        let item = item_rect(full, crop);
        close_rect(item, Rect::new(60.0, 70.0, 100.0, 25.0));
        close_rect(full_rect(item, crop), full);
    }

    #[test]
    fn drag_edge_clamps_to_image_and_min_size() {
        let full = Rect::new(0.0, 0.0, 100.0, 100.0);
        let crop = WHOLE;
        let out = drag_crop(full, crop, Some(Handle::E), Vec2::new(-500.0, 0.0));
        assert!(out.w >= MIN_FRAC - 1e-9, "keeps the minimum size");
        close(out.x, 0.0);
        let out = drag_crop(full, crop, Some(Handle::NW), Vec2::new(500.0, 500.0));
        assert!(out.w >= MIN_FRAC - 1e-9);
        close(out.x + out.w, 1.0);
        close(out.y + out.h, 1.0);
    }

    #[test]
    fn drag_past_the_image_stays_inside() {
        let full = Rect::new(0.0, 0.0, 100.0, 100.0);
        let out = drag_crop(full, WHOLE, Some(Handle::SE), Vec2::new(900.0, 900.0));
        close(out.x + out.w, 1.0);
        close(out.y + out.h, 1.0);
    }

    #[test]
    fn rotate_handle_leaves_crop_unchanged() {
        let full = Rect::new(0.0, 0.0, 100.0, 100.0);
        let crop = Rect::new(0.1, 0.1, 0.5, 0.5);
        assert_eq!(
            drag_crop(full, crop, Some(Handle::Rotate), Vec2::new(10.0, 10.0)),
            crop
        );
    }

    #[test]
    fn moving_the_window_keeps_its_size() {
        let full = Rect::new(0.0, 0.0, 100.0, 100.0);
        let crop = Rect::new(0.2, 0.2, 0.3, 0.3);
        let out = drag_crop(full, crop, None, Vec2::new(50.0, 0.0));
        close(out.x, 0.7);
        close(out.w, 0.3);
        close(out.y, 0.2);
        let out = drag_crop(full, crop, None, Vec2::new(-5000.0, 5000.0));
        close(out.x, 0.0);
        close(out.y, 0.7);
        close(out.h, 0.3);
    }
}
