//! Points, rectangles, rotated rectangles and the camera (pan and zoom) for the canvas.
//!
//! Screen and world y grow downwards. Angles are in degrees, clockwise on screen (positive is clockwise).

use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Neg, Sub};

/// A point or vector in world or screen space.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    /// Horizontal component (grows to the right).
    pub x: f64,
    /// Vertical component (grows downwards).
    pub y: f64,
}

impl Vec2 {
    /// The zero vector, the origin.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    /// Builds a vector from its two components.
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }

    /// The length of the vector.
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Rotates around the origin by `degrees` (clockwise on screen): x' = x cos - y sin, y' = x sin + y cos.
    pub fn rotate(self, degrees: f64) -> Vec2 {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Vec2::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }
}

impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;

    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f64) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f64> for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: f64) -> Vec2 {
        Vec2::new(self.x / rhs, self.y / rhs)
    }
}

/// An axis-aligned rectangle: top-left corner `(x, y)` plus width and height.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width (not negative for a valid rectangle).
    pub w: f64,
    /// Height (not negative for a valid rectangle).
    pub h: f64,
}

impl Rect {
    /// Builds a rectangle from its top-left corner and size.
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    /// The rectangle spanned by two corners in any order (w and h are never negative).
    pub fn from_points(a: Vec2, b: Vec2) -> Rect {
        Rect::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        )
    }

    /// The x of the left edge.
    pub fn left(&self) -> f64 {
        self.x
    }

    /// The x of the right edge.
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    /// The y of the top edge.
    pub fn top(&self) -> f64 {
        self.y
    }

    /// The y of the bottom edge.
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// The middle point of the rectangle.
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// The top-left corner as a point.
    pub fn top_left(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// The width and height as a vector.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    /// The area (width times height).
    pub fn area(&self) -> f64 {
        self.w * self.h
    }

    /// True when the width or the height is zero or less.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// True when the point is inside the rectangle; points on the edges count as inside.
    pub fn contains_point(&self, p: Vec2) -> bool {
        p.x >= self.left() && p.x <= self.right() && p.y >= self.top() && p.y <= self.bottom()
    }

    /// True when the other rectangle lies entirely inside this one (edges may touch).
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.left() >= self.left()
            && other.right() <= self.right()
            && other.top() >= self.top()
            && other.bottom() <= self.bottom()
    }

    /// True when the rectangles overlap. Touching edges count as intersecting.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.left() <= other.right()
            && other.left() <= self.right()
            && self.top() <= other.bottom()
            && other.top() <= self.bottom()
    }

    /// The smallest rectangle that contains both this one and the other.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect::from_points(
            Vec2::new(self.left().min(other.left()), self.top().min(other.top())),
            Vec2::new(
                self.right().max(other.right()),
                self.bottom().max(other.bottom()),
            ),
        )
    }

    /// Grows every side by `d` (a negative `d` shrinks it). Width and height never go below zero.
    pub fn inflate(&self, d: f64) -> Rect {
        Rect::new(
            self.x - d,
            self.y - d,
            (self.w + 2.0 * d).max(0.0),
            (self.h + 2.0 * d).max(0.0),
        )
    }

    /// The same rectangle moved by the vector `v`.
    pub fn translate(&self, v: Vec2) -> Rect {
        Rect::new(self.x + v.x, self.y + v.y, self.w, self.h)
    }

    /// The smallest rectangle around all of them, or None for an empty iterator.
    pub fn bounds_of<I: IntoIterator<Item = Rect>>(rects: I) -> Option<Rect> {
        rects.into_iter().reduce(|a, b| a.union(&b))
    }
}

/// A rectangle rotated by `rotation` degrees around its own centre.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RotRect {
    /// The unrotated rectangle, in world space.
    pub rect: Rect,
    /// Rotation in degrees, clockwise on screen, around the rectangle's centre.
    pub rotation: f64,
}

impl RotRect {
    /// Builds a rotated rectangle.
    pub const fn new(rect: Rect, rotation: f64) -> Self {
        RotRect { rect, rotation }
    }

    /// Corners in order top-left, top-right, bottom-right, bottom-left of the unrotated rect, each rotated around the centre.
    pub fn corners(&self) -> [Vec2; 4] {
        let c = self.rect.center();
        let hw = self.rect.w / 2.0;
        let hh = self.rect.h / 2.0;
        [
            Vec2::new(-hw, -hh),
            Vec2::new(hw, -hh),
            Vec2::new(hw, hh),
            Vec2::new(-hw, hh),
        ]
        .map(|v| c + v.rotate(self.rotation))
    }

    /// The axis-aligned box around the corners.
    pub fn aabb(&self) -> Rect {
        let pts = self.corners();
        let mut min = pts[0];
        let mut max = pts[0];
        for p in pts {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        Rect::from_points(min, max)
    }

    /// True when the point is inside the rotated rectangle (edges inclusive).
    pub fn contains_point(&self, p: Vec2) -> bool {
        self.rect.contains_point(self.to_local(p))
    }

    /// The point `p` mapped into the rectangle's own unrotated frame: it is turned by -rotation around the centre,
    /// so `Rect::contains_point` on the unrotated rect answers for the rotated shape. The result is in world coordinates.
    pub fn to_local(&self, p: Vec2) -> Vec2 {
        let c = self.rect.center();
        c + (p - c).rotate(-self.rotation)
    }
}

/// The view: `pan` is where the world origin appears on screen, `zoom` is screen pixels per world unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// Screen position of the world origin.
    pub pan: Vec2,
    /// Screen pixels per world unit.
    pub zoom: f64,
}

impl Default for Camera {
    /// No pan and a zoom of 1.0.
    fn default() -> Self {
        Camera {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// The smallest allowed zoom.
    pub const MIN_ZOOM: f64 = 0.02;
    /// The largest allowed zoom.
    pub const MAX_ZOOM: f64 = 64.0;

    /// Converts a world point to screen pixels.
    pub fn world_to_screen(&self, p: Vec2) -> Vec2 {
        p * self.zoom + self.pan
    }

    /// Converts a screen point to world coordinates.
    pub fn screen_to_world(&self, p: Vec2) -> Vec2 {
        (p - self.pan) / self.zoom
    }

    /// Zooms by `factor` keeping the world point under `screen` fixed; the zoom is clamped to MIN..MAX.
    pub fn zoom_at(&self, screen: Vec2, factor: f64) -> Camera {
        let anchor = self.screen_to_world(screen);
        let zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        Camera {
            pan: screen - anchor * zoom,
            zoom,
        }
    }

    /// Pans by a screen-space delta.
    pub fn pan_by(&self, delta: Vec2) -> Camera {
        Camera {
            pan: self.pan + delta,
            zoom: self.zoom,
        }
    }

    /// A camera showing `world` centred in a viewport of `viewport` pixels with `margin` pixels on every side.
    /// Zoom = min((vw-2m)/w, (vh-2m)/h), clamped to MIN..MAX. An empty `world` keeps zoom 1.0 and centres its top-left.
    /// A viewport smaller than 2*margin uses margin 0.
    pub fn fit(world: Rect, viewport: Vec2, margin: f64) -> Camera {
        let m = if viewport.x < 2.0 * margin || viewport.y < 2.0 * margin {
            0.0
        } else {
            margin
        };
        let half = viewport / 2.0;
        if world.is_empty() {
            return Camera {
                pan: half - world.top_left(),
                zoom: 1.0,
            };
        }
        let avail = Vec2::new(viewport.x - 2.0 * m, viewport.y - 2.0 * m);
        let zoom = (avail.x / world.w)
            .min(avail.y / world.h)
            .clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        Camera {
            pan: half - world.center() * zoom,
            zoom,
        }
    }

    /// The world rectangle the viewport shows.
    pub fn visible_world(&self, viewport: Vec2) -> Rect {
        Rect::from_points(
            self.screen_to_world(Vec2::ZERO),
            self.screen_to_world(viewport),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn close_v(a: Vec2, b: Vec2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn rotate_quarter_turn_is_clockwise_on_screen() {
        assert!(close_v(
            Vec2::new(1.0, 0.0).rotate(90.0),
            Vec2::new(0.0, 1.0)
        ));
    }

    #[test]
    fn from_points_accepts_corners_in_any_order() {
        let a = Rect::from_points(Vec2::new(10.0, 8.0), Vec2::new(2.0, 1.0));
        assert_eq!(a, Rect::new(2.0, 1.0, 8.0, 7.0));
    }

    #[test]
    fn union_intersects_and_contains() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 10.0, 5.0, 5.0);
        let far = Rect::new(20.0, 20.0, 1.0, 1.0);
        assert!(a.intersects(&b), "touching corners count as intersecting");
        assert!(!a.intersects(&far));
        assert_eq!(a.union(&far), Rect::new(0.0, 0.0, 21.0, 21.0));
        assert!(
            a.contains_point(Vec2::new(10.0, 10.0)),
            "edges are inclusive"
        );
        assert!(!a.contains_point(Vec2::new(10.5, 5.0)));
        assert!(a.contains_rect(&Rect::new(1.0, 1.0, 2.0, 2.0)));
        assert!(!a.contains_rect(&b));
    }

    #[test]
    fn inflate_negative_clamps_size_at_zero() {
        let r = Rect::new(0.0, 0.0, 4.0, 2.0).inflate(-5.0);
        assert!(close(r.w, 0.0));
        assert!(close(r.h, 0.0));
        assert!(r.is_empty());
    }

    #[test]
    fn bounds_of_empty_is_none() {
        assert_eq!(Rect::bounds_of(Vec::<Rect>::new()), None);
        let b = Rect::bounds_of([Rect::new(0.0, 0.0, 1.0, 1.0), Rect::new(4.0, 4.0, 1.0, 1.0)]);
        assert_eq!(b, Some(Rect::new(0.0, 0.0, 5.0, 5.0)));
    }

    #[test]
    fn rot_rect_quarter_turn() {
        let r = RotRect::new(Rect::new(0.0, 0.0, 4.0, 2.0), 90.0);
        let aabb = r.aabb();
        assert!(close(aabb.x, 1.0));
        assert!(close(aabb.y, -1.0));
        assert!(close(aabb.w, 2.0));
        assert!(close(aabb.h, 4.0));
        assert!(r.contains_point(Vec2::new(2.0, 1.0)), "centre is inside");
        assert!(
            !r.contains_point(Vec2::new(0.1, 0.1)),
            "unrotated corner area is outside"
        );
    }

    #[test]
    fn camera_round_trip() {
        let cam = Camera {
            pan: Vec2::new(10.0, -5.0),
            zoom: 2.5,
        };
        let p = Vec2::new(3.0, 7.0);
        assert!(close_v(cam.screen_to_world(cam.world_to_screen(p)), p));
        assert!(close_v(cam.world_to_screen(cam.screen_to_world(p)), p));
    }

    #[test]
    fn zoom_at_keeps_point_fixed() {
        let cam = Camera {
            pan: Vec2::new(30.0, 20.0),
            zoom: 1.5,
        };
        let screen = Vec2::new(100.0, 50.0);
        let before = cam.screen_to_world(screen);
        let after = cam.zoom_at(screen, 2.0).screen_to_world(screen);
        assert!(close_v(before, after));
    }

    #[test]
    fn zoom_at_clamps_to_max() {
        let cam = Camera::default().zoom_at(Vec2::ZERO, 1e6);
        assert!(close(cam.zoom, Camera::MAX_ZOOM));
    }

    #[test]
    fn fit_centres_world_and_scales_to_viewport() {
        let cam = Camera::fit(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Vec2::new(800.0, 600.0),
            0.0,
        );
        assert!(close(cam.zoom, 8.0));
        assert!(close_v(
            cam.world_to_screen(Vec2::new(50.0, 25.0)),
            Vec2::new(400.0, 300.0)
        ));
    }

    #[test]
    fn visible_world_of_default_camera() {
        let r = Camera::default().visible_world(Vec2::new(800.0, 600.0));
        assert_eq!(r, Rect::new(0.0, 0.0, 800.0, 600.0));
    }
}
