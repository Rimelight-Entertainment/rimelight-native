//! PureRef-style organising for a selection of items (clean-room: behaviour described by the owner's request, no code or
//! assets from other products). Every function takes the selection and returns new rectangles; nothing else changes.
//! Rotation is ignored here: items are arranged by their unrotated rectangles. Results keep the selection's top-left
//! corner where it was, unless a function says otherwise.

use crate::geom::{Rect, Vec2};
use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

/// One selected item as the arrange functions see it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub rect: Rect,
    pub name: String,
    pub added: i64,
    pub hue: Option<f64>,
}

pub type Placed = Vec<(String, Rect)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortKey {
    Name,
    Added,
    Size,
    Hue,
    Random(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalize {
    Height,
    Width,
    Area,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
    CenterX,
    CenterY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// Packs the items into a compact block close to `target_aspect` (width / height), largest first, with `gap` between items.
pub fn pack_optimal(items: &[Entry], gap: f64, target_aspect: f64) -> Placed {
    if items.is_empty() {
        return Vec::new();
    }
    let anchor = anchor_of(items);
    let aspect = sanitize_aspect(target_aspect);
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (items[a].rect, items[b].rect);
        rb.h.total_cmp(&ra.h)
            .then(rb.w.total_cmp(&ra.w))
            .then_with(|| id_cmp(items, a, b))
    });
    let sizes = sized(items, &order);
    let total = total_area(items);
    let wide = widest(items);

    let mut best: Option<(f64, Vec<(usize, Vec2)>)> = None;
    for k in 0..16_u32 {
        let f = 0.5 + 0.1 * f64::from(k);
        let row_width = wide.max((total * aspect).sqrt() * f);
        let layout = rows(&sizes, row_width, gap);
        let Some(b) = layout_bounds(items, &layout) else {
            continue;
        };
        let (w, h) = (b.w, b.h);
        let score = if w > 0.0 && h > 0.0 {
            w * h * (1.0 + (w / h / aspect).ln().abs())
        } else {
            w * h
        };
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, layout));
        }
    }
    let layout = best.map(|(_, l)| l).unwrap_or_default();
    finish(items, &layout, anchor)
}

/// Sorts by `key`, then lays the items out in rows (left to right, wrapping) in that order.
pub fn arrange_sorted(items: &[Entry], key: SortKey, gap: f64, target_aspect: f64) -> Placed {
    if items.is_empty() {
        return Vec::new();
    }
    let anchor = anchor_of(items);
    let aspect = sanitize_aspect(target_aspect);
    let mut order: Vec<usize> = (0..items.len()).collect();
    sort_order(items, key, &mut order);
    let row_width = widest(items).max((total_area(items) * aspect).sqrt());
    let layout = rows(&sized(items, &order), row_width, gap);
    finish(items, &layout, anchor)
}

/// A grid with `columns` columns in the given order; each cell is as large as the largest item; items centred in cells.
pub fn grid(items: &[Entry], columns: usize, gap: f64) -> Placed {
    if items.is_empty() {
        return Vec::new();
    }
    let columns = columns.max(1);
    let anchor = anchor_of(items);
    let (cell_w, cell_h) = (widest(items), tallest(items));
    let layout: Vec<(usize, Vec2)> = (0..items.len())
        .map(|i| {
            let col = to_f64(i % columns);
            let row = to_f64(i / columns);
            let cell = Vec2::new(col * (cell_w + gap), row * (cell_h + gap));
            let r = items[i].rect;
            let centring = Vec2::new((cell_w - r.w) / 2.0, (cell_h - r.h) / 2.0);
            (i, cell + centring)
        })
        .collect();
    finish(items, &layout, anchor)
}

/// Scales every item (keeping its aspect ratio and its top-left corner) so all share the same height, width or area.
/// The target is the mean over the selection, or the reference item's value when `reference` names one of them.
pub fn normalize(items: &[Entry], mode: Normalize, reference: Option<&str>) -> Placed {
    if items.is_empty() {
        return Vec::new();
    }
    let mean = items.iter().map(|e| measure(e.rect, mode)).sum::<f64>() / to_f64(items.len());
    let target = reference
        .and_then(|id| items.iter().find(|e| e.id == id))
        .map_or(mean, |e| measure(e.rect, mode));
    if !(target.is_finite() && target > 0.0) {
        return unchanged(items);
    }
    let rects: Vec<Rect> = items
        .iter()
        .map(|e| {
            let r = e.rect;
            let v = measure(r, mode);
            if v <= 0.0 {
                return r;
            }
            let s = match mode {
                Normalize::Area => (target / v).sqrt(),
                Normalize::Height | Normalize::Width => target / v,
            };
            Rect::new(r.x, r.y, r.w * s, r.h * s)
        })
        .collect();
    with_rects(items, rects)
}

/// Lines items up on one edge or centre line of the selection's bounds.
pub fn align(items: &[Entry], how: Align) -> Placed {
    let Some(b) = Rect::bounds_of(items.iter().map(|e| e.rect)) else {
        return unchanged(items);
    };
    let rects: Vec<Rect> = items
        .iter()
        .map(|e| {
            let r = e.rect;
            let (x, y) = match how {
                Align::Left => (b.left(), r.y),
                Align::Right => (b.right() - r.w, r.y),
                Align::Top => (r.x, b.top()),
                Align::Bottom => (r.x, b.bottom() - r.h),
                Align::CenterX => (b.center().x - r.w / 2.0, r.y),
                Align::CenterY => (r.x, b.center().y - r.h / 2.0),
            };
            Rect::new(x, y, r.w, r.h)
        })
        .collect();
    with_rects(items, rects)
}

/// Spaces items evenly along an axis between the first and last (by position): equal gaps between neighbours. Fewer
/// than 3 items: unchanged.
pub fn distribute(items: &[Entry], axis: Axis) -> Placed {
    let n = items.len();
    if n < 3 {
        return unchanged(items);
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        start_len(items[a].rect, axis)
            .0
            .total_cmp(&start_len(items[b].rect, axis).0)
            .then_with(|| id_cmp(items, a, b))
    });
    let (lo, _) = start_len(items[order[0]].rect, axis);
    let (last_start, last_len) = start_len(items[order[n - 1]].rect, axis);
    let hi = last_start + last_len;
    let total: f64 = items.iter().map(|e| start_len(e.rect, axis).1).sum();
    let gap = ((hi - lo) - total) / to_f64(n - 1);

    let mut rects: Vec<Rect> = items.iter().map(|e| e.rect).collect();
    let mut cursor = lo + start_len(items[order[0]].rect, axis).1 + gap;
    for &i in &order[1..n - 1] {
        rects[i] = moved(items[i].rect, axis, cursor);
        cursor += start_len(items[i].rect, axis).1 + gap;
    }
    with_rects(items, rects)
}

/// Piles items on top of each other at the first item's position, each shifted by `offset` from the one before.
pub fn stack(items: &[Entry], offset: Vec2) -> Placed {
    let Some(first) = items.first() else {
        return Vec::new();
    };
    let origin = first.rect.top_left();
    let rects: Vec<Rect> = items
        .iter()
        .enumerate()
        .map(|(k, e)| {
            let p = origin + offset * to_f64(k);
            Rect::new(p.x, p.y, e.rect.w, e.rect.h)
        })
        .collect();
    with_rects(items, rects)
}

/// Row layout: x starts at 0; an item goes on the current row if it is the first on the row or fits within
/// `row_width`; otherwise a new row starts below the current one (plus `gap`). Items are top-aligned within a row.
fn rows(sizes: &[(usize, Vec2)], row_width: f64, gap: f64) -> Vec<(usize, Vec2)> {
    let mut out = Vec::with_capacity(sizes.len());
    let mut x = 0.0;
    let mut y = 0.0;
    let mut row_h = 0.0_f64;
    for &(idx, size) in sizes {
        if x > 0.0 && x + size.x > row_width {
            y += row_h + gap;
            x = 0.0;
            row_h = 0.0;
        }
        out.push((idx, Vec2::new(x, y)));
        x += size.x + gap;
        row_h = row_h.max(size.y);
    }
    out
}

/// The layout's bounds, using each item's own size from `items`.
fn layout_bounds(items: &[Entry], layout: &[(usize, Vec2)]) -> Option<Rect> {
    Rect::bounds_of(
        layout
            .iter()
            .map(|&(i, p)| Rect::new(p.x, p.y, items[i].rect.w, items[i].rect.h)),
    )
}

/// Places a layout built from (0,0), translated by `anchor`. Items not in the layout keep their rectangle.
fn finish(items: &[Entry], layout: &[(usize, Vec2)], anchor: Vec2) -> Placed {
    let mut rects: Vec<Rect> = items.iter().map(|e| e.rect).collect();
    for &(i, p) in layout {
        let r = items[i].rect;
        rects[i] = Rect::new(p.x, p.y, r.w, r.h).translate(anchor);
    }
    with_rects(items, rects)
}

/// Pairs each input item's id with its new rectangle, in input order.
fn with_rects(items: &[Entry], rects: Vec<Rect>) -> Placed {
    items
        .iter()
        .zip(rects)
        .map(|(e, r)| (e.id.clone(), r))
        .collect()
}

fn unchanged(items: &[Entry]) -> Placed {
    with_rects(items, items.iter().map(|e| e.rect).collect())
}

fn anchor_of(items: &[Entry]) -> Vec2 {
    Rect::bounds_of(items.iter().map(|e| e.rect)).map_or(Vec2::new(0.0, 0.0), |b| b.top_left())
}

fn sanitize_aspect(aspect: f64) -> f64 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    }
}

fn sized(items: &[Entry], order: &[usize]) -> Vec<(usize, Vec2)> {
    order
        .iter()
        .map(|&i| (i, Vec2::new(items[i].rect.w, items[i].rect.h)))
        .collect()
}

fn total_area(items: &[Entry]) -> f64 {
    items.iter().map(|e| e.rect.area()).sum()
}

fn widest(items: &[Entry]) -> f64 {
    items.iter().map(|e| e.rect.w).fold(0.0, f64::max)
}

fn tallest(items: &[Entry]) -> f64 {
    items.iter().map(|e| e.rect.h).fold(0.0, f64::max)
}

/// Exact for any realistic item count (far below 2^32).
fn to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn measure(r: Rect, mode: Normalize) -> f64 {
    match mode {
        Normalize::Height => r.h,
        Normalize::Width => r.w,
        Normalize::Area => r.w * r.h,
    }
}

fn start_len(r: Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x, r.w),
        Axis::Y => (r.y, r.h),
    }
}

fn moved(r: Rect, axis: Axis, start: f64) -> Rect {
    match axis {
        Axis::X => Rect::new(start, r.y, r.w, r.h),
        Axis::Y => Rect::new(r.x, start, r.w, r.h),
    }
}

fn id_cmp(items: &[Entry], a: usize, b: usize) -> Ordering {
    items[a].id.cmp(&items[b].id)
}

fn hue_cmp(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn sort_order(items: &[Entry], key: SortKey, order: &mut [usize]) {
    match key {
        SortKey::Name => order.sort_by(|&a, &b| {
            natural_cmp(&items[a].name, &items[b].name).then_with(|| id_cmp(items, a, b))
        }),
        SortKey::Added => order.sort_by(|&a, &b| {
            items[a]
                .added
                .cmp(&items[b].added)
                .then_with(|| id_cmp(items, a, b))
        }),
        SortKey::Size => order.sort_by(|&a, &b| {
            items[b]
                .rect
                .area()
                .total_cmp(&items[a].rect.area())
                .then_with(|| id_cmp(items, a, b))
        }),
        SortKey::Hue => order.sort_by(|&a, &b| {
            hue_cmp(items[a].hue, items[b].hue).then_with(|| id_cmp(items, a, b))
        }),
        SortKey::Random(seed) => shuffle(order, seed),
    }
}

/// Fisher-Yates over the index list, driven by xorshift64* seeded with `seed | 1`.
fn shuffle(order: &mut [usize], seed: u64) {
    let mut rng = XorShift64Star::new(seed);
    for i in (1..order.len()).rev() {
        let j = rng.below(i + 1);
        order.swap(i, j);
    }
}

struct XorShift64Star(u64);

impl XorShift64Star {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n` (`n` must be at least 1).
    fn below(&mut self, n: usize) -> usize {
        let m = u64::try_from(n).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next_u64() % m).unwrap_or(0)
    }
}

/// Case-insensitive natural order: runs of digits compare as numbers ("img2" < "img10").
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_digits(&mut ai);
                let nb = take_digits(&mut bi);
                let ord = cmp_digit_runs(&na, &nb);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = lower(x).cmp(&lower(y));
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn take_digits(it: &mut Peekable<Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        s.push(c);
    }
    s
}

fn cmp_digit_runs(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, x: f64, y: f64, w: f64, h: f64) -> Entry {
        Entry {
            id: id.to_string(),
            rect: Rect::new(x, y, w, h),
            name: id.to_string(),
            added: 0,
            hue: None,
        }
    }

    fn get(p: &Placed, id: &str) -> Rect {
        p.iter()
            .find(|(i, _)| i.as_str() == id)
            .map(|(_, r)| *r)
            .expect("id is in the output")
    }

    fn ids(p: &Placed) -> Vec<&str> {
        p.iter().map(|(id, _)| id.as_str()).collect()
    }

    fn input_ids(items: &[Entry]) -> Vec<&str> {
        items.iter().map(|e| e.id.as_str()).collect()
    }

    fn coords(p: &Placed) -> Vec<(f64, f64)> {
        p.iter().map(|(_, r)| (r.x, r.y)).collect()
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn overlap(a: &Rect, b: &Rect) -> f64 {
        let w = a.right().min(b.right()) - a.left().max(b.left());
        let h = a.bottom().min(b.bottom()) - a.top().max(b.top());
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }

    fn assert_no_overlap(p: &Placed) {
        for (i, (_, a)) in p.iter().enumerate() {
            for (_, b) in &p[i + 1..] {
                assert!(overlap(a, b) <= 1e-9, "two items overlap");
            }
        }
    }

    #[test]
    fn every_function_keeps_input_order_and_ids() {
        let items = vec![
            item("c", 0.0, 0.0, 30.0, 20.0),
            item("a", 50.0, 10.0, 40.0, 40.0),
            item("b", 5.0, 80.0, 10.0, 60.0),
        ];
        let want = input_ids(&items);
        assert_eq!(ids(&pack_optimal(&items, 2.0, 1.0)), want);
        assert_eq!(ids(&arrange_sorted(&items, SortKey::Name, 2.0, 1.0)), want);
        assert_eq!(ids(&grid(&items, 2, 2.0)), want);
        assert_eq!(ids(&normalize(&items, Normalize::Area, None)), want);
        assert_eq!(ids(&align(&items, Align::Top)), want);
        assert_eq!(ids(&distribute(&items, Axis::X)), want);
        assert_eq!(ids(&stack(&items, Vec2::new(5.0, 5.0))), want);
    }

    #[test]
    fn empty_input_returns_empty() {
        let none: Vec<Entry> = Vec::new();
        assert!(pack_optimal(&none, 1.0, 1.0).is_empty());
        assert!(arrange_sorted(&none, SortKey::Hue, 1.0, 1.0).is_empty());
        assert!(grid(&none, 3, 1.0).is_empty());
        assert!(normalize(&none, Normalize::Width, None).is_empty());
        assert!(align(&none, Align::CenterX).is_empty());
        assert!(distribute(&none, Axis::Y).is_empty());
        assert!(stack(&none, Vec2::new(1.0, 1.0)).is_empty());
    }

    #[test]
    fn pack_four_squares_makes_a_200_block_at_the_anchor() {
        let items = vec![
            item("a", 10.0, 20.0, 100.0, 100.0),
            item("b", 300.0, 200.0, 100.0, 100.0),
            item("c", 50.0, 400.0, 100.0, 100.0),
            item("d", 500.0, 30.0, 100.0, 100.0),
        ];
        let p = pack_optimal(&items, 0.0, 1.0);
        assert_no_overlap(&p);
        let b = Rect::bounds_of(p.iter().map(|(_, r)| *r)).expect("non-empty");
        assert!(approx(b.w, 200.0));
        assert!(approx(b.h, 200.0));
        assert!(approx(b.x, 10.0));
        assert!(approx(b.y, 20.0));
    }

    #[test]
    fn pack_twenty_mixed_sizes_never_overlaps_and_stays_near_square() {
        let sizes = [
            (120.0, 80.0),
            (40.0, 200.0),
            (64.0, 64.0),
            (300.0, 50.0),
            (90.0, 90.0),
            (150.0, 30.0),
            (70.0, 140.0),
            (33.0, 47.0),
            (210.0, 120.0),
            (55.0, 95.0),
            (80.0, 20.0),
            (180.0, 60.0),
            (25.0, 25.0),
            (100.0, 220.0),
            (140.0, 70.0),
            (60.0, 110.0),
            (199.0, 41.0),
            (48.0, 48.0),
            (260.0, 90.0),
            (75.0, 160.0),
        ];
        let items: Vec<Entry> = sizes
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| item(&format!("i{i}"), 0.0, 0.0, w, h))
            .collect();
        let p = pack_optimal(&items, 4.0, 1.0);
        assert_no_overlap(&p);
        let b = Rect::bounds_of(p.iter().map(|(_, r)| *r)).expect("non-empty");
        let aspect = b.w / b.h;
        assert!((0.5..=2.0).contains(&aspect));
    }

    #[test]
    fn name_sort_is_natural() {
        assert_eq!(natural_cmp("img2", "img10"), Ordering::Less);
        assert_eq!(natural_cmp("IMG2", "img10"), Ordering::Less);
        let items = vec![
            item("img10", 0.0, 0.0, 10.0, 10.0),
            item("img2", 0.0, 0.0, 10.0, 10.0),
            item("img1", 0.0, 0.0, 10.0, 10.0),
        ];
        // A huge aspect target makes one row, so x follows the sorted order.
        let p = arrange_sorted(&items, SortKey::Name, 0.0, 1e6);
        let x1 = get(&p, "img1").x;
        let x2 = get(&p, "img2").x;
        let x10 = get(&p, "img10").x;
        assert!(x1 < x2);
        assert!(x2 < x10);
    }

    #[test]
    fn random_sort_is_deterministic_per_seed() {
        let items: Vec<Entry> = (0..10_i32)
            .map(|i| {
                item(
                    &format!("r{i}"),
                    0.0,
                    0.0,
                    10.0 + f64::from(i) * 7.0,
                    20.0 + f64::from((i * 13) % 17) * 3.0,
                )
            })
            .collect();
        let a = arrange_sorted(&items, SortKey::Random(7), 2.0, 1.0);
        let b = arrange_sorted(&items, SortKey::Random(7), 2.0, 1.0);
        let c = arrange_sorted(&items, SortKey::Random(8), 2.0, 1.0);
        assert_eq!(coords(&a), coords(&b));
        assert_ne!(coords(&a), coords(&c));
    }

    #[test]
    fn normalize_height_equalises_and_keeps_aspect() {
        let items = vec![
            item("a", 0.0, 0.0, 100.0, 50.0),
            item("b", 0.0, 0.0, 50.0, 100.0),
            item("c", 0.0, 0.0, 100.0, 200.0),
        ];
        let mean = (50.0 + 100.0 + 200.0) / 3.0;
        let p = normalize(&items, Normalize::Height, None);
        for (src, (_, r)) in items.iter().zip(&p) {
            assert!(approx(r.h, mean));
            assert!(approx(r.w / r.h, src.rect.w / src.rect.h));
        }
    }

    #[test]
    fn normalize_reference_picks_that_items_value() {
        let items = vec![
            item("a", 0.0, 0.0, 100.0, 50.0),
            item("b", 0.0, 0.0, 50.0, 100.0),
            item("c", 0.0, 0.0, 100.0, 200.0),
        ];
        let p = normalize(&items, Normalize::Height, Some("b"));
        assert!(approx(get(&p, "a").h, 100.0));
        assert!(approx(get(&p, "b").h, 100.0));
        assert!(approx(get(&p, "c").h, 100.0));
    }

    #[test]
    fn align_left_gives_equal_x() {
        let items = vec![
            item("a", 10.0, 0.0, 20.0, 20.0),
            item("b", 50.0, 30.0, 20.0, 20.0),
            item("c", 30.0, 60.0, 20.0, 20.0),
        ];
        let p = align(&items, Align::Left);
        for (_, r) in &p {
            assert!(approx(r.x, 10.0));
        }
    }

    #[test]
    fn distribute_x_gives_equal_gaps() {
        let items = vec![
            item("a", 0.0, 0.0, 10.0, 10.0),
            item("b", 40.0, 0.0, 10.0, 10.0),
            item("c", 100.0, 0.0, 10.0, 10.0),
        ];
        let p = distribute(&items, Axis::X);
        let a = get(&p, "a");
        let b = get(&p, "b");
        let c = get(&p, "c");
        assert!(approx(a.x, 0.0));
        assert!(approx(c.x, 100.0));
        assert!(approx(b.x - (a.x + a.w), 40.0));
        assert!(approx(c.x - (b.x + b.w), 40.0));
    }

    #[test]
    fn stack_offsets_each_item() {
        let items = vec![
            item("a", 5.0, 5.0, 10.0, 10.0),
            item("b", 0.0, 0.0, 20.0, 20.0),
            item("c", 90.0, 90.0, 1.0, 1.0),
        ];
        let p = stack(&items, Vec2::new(10.0, 3.0));
        let a = get(&p, "a");
        let b = get(&p, "b");
        let c = get(&p, "c");
        assert!(approx(a.x, 5.0) && approx(a.y, 5.0));
        assert!(approx(b.x, 15.0) && approx(b.y, 8.0));
        assert!(approx(c.x, 25.0) && approx(c.y, 11.0));
    }

    #[test]
    fn grid_centres_small_item_in_big_cell() {
        let items = vec![
            item("big", 0.0, 0.0, 100.0, 100.0),
            item("small", 0.0, 0.0, 10.0, 10.0),
        ];
        let p = grid(&items, 2, 0.0);
        let s = get(&p, "small");
        assert!(approx(s.x, 145.0));
        assert!(approx(s.y, 45.0));
    }
}
