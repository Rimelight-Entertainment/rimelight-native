//! Z order is a text rank per item; items draw in ascending byte order of `z` (Postgres `COLLATE "C"`). A new rank
//! always fits between two existing ones, so moving one item never renumbers the others.

use std::collections::HashSet;

/// The 62 digits in ascending byte order.
pub const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Upper limit on the digit positions one rank may use. Valid input never reaches it.
const MAX_DIGITS: usize = 1000;

/// The index of a digit character in [`DIGITS`]. A character that is not a digit counts as 0.
fn digit_value(c: u8) -> usize {
    DIGITS.iter().position(|&d| d == c).unwrap_or(0)
}

/// A rank strictly between `lo` and `hi` (None = no bound). Never panics; if lo >= hi it returns rank_between(Some(lo), None).
pub fn rank_between(lo: Option<&str>, hi: Option<&str>) -> String {
    let hi = match (lo, hi) {
        (Some(l), Some(h)) if l >= h => None,
        _ => hi,
    };
    let lo_d: Vec<usize> = lo.unwrap_or("").bytes().map(digit_value).collect();
    let hi_d: Option<Vec<usize>> = hi.map(|h| h.bytes().map(digit_value).collect());
    let mut out: Vec<usize> = Vec::new();
    let mut open = hi_d.is_none();
    let mut i = 0usize;
    while i <= MAX_DIGITS {
        let a = lo_d.get(i).copied().unwrap_or(0);
        let b = if open {
            62
        } else {
            hi_d.as_ref().and_then(|h| h.get(i)).copied().unwrap_or(0)
        };
        if !open && a == b {
            out.push(a);
            i += 1;
            continue;
        }
        if b > a + 1 {
            out.push((a + b) / 2);
            break;
        }
        // b == a + 1 (or defensively b <= a): keep a and leave the upper bound open from here on.
        out.push(a);
        i += 1;
        open = true;
    }
    out.into_iter().map(|d| char::from(DIGITS[d])).collect()
}

/// `n` ranks in ascending order, all strictly between lo and hi.
pub fn ranks_between(lo: Option<&str>, hi: Option<&str>, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(n);
    for _ in 0..n {
        let next = rank_between(out.last().map(|s| s.as_str()).or(lo), hi);
        out.push(next);
    }
    out
}

/// `n` ranks in ascending order, all strictly below `hi`.
fn ranks_below(hi: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(n);
    for _ in 0..n {
        let next = rank_between(None, Some(out.last().map_or(hi, |s| s.as_str())));
        out.push(next);
    }
    // Generated from the top down, so reverse to ascend.
    out.reverse();
    out
}

/// The ids in `selected` as a set of borrowed strings.
fn id_set(selected: &[String]) -> HashSet<&str> {
    selected.iter().map(|s| s.as_str()).collect()
}

/// A copy of `all` sorted by (z, id).
fn sorted_pairs(all: &[(String, String)]) -> Vec<(String, String)> {
    let mut v = all.to_vec();
    v.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    v
}

/// The items of `work` (already sorted) whose id is selected, as (id, z).
fn selected_items(work: &[(String, String)], sel: &HashSet<&str>) -> Vec<(String, String)> {
    work.iter()
        .filter(|(id, _)| sel.contains(id.as_str()))
        .cloned()
        .collect()
}

/// New ranks for `selected` ids moved to the top, keeping their relative order. `all` is every item as (id, z) in any
/// order. Returns only the selected items' new (id, z).
pub fn to_front(all: &[(String, String)], selected: &[String]) -> Vec<(String, String)> {
    let sel = id_set(selected);
    let work = sorted_pairs(all);
    let chosen = selected_items(&work, &sel);
    let ranks = match work.last() {
        Some((_, max)) => ranks_between(Some(max.as_str()), None, chosen.len()),
        None => ranks_between(None, None, chosen.len()),
    };
    chosen
        .into_iter()
        .zip(ranks)
        .map(|((id, _), z)| (id, z))
        .collect()
}

/// New ranks for `selected` ids moved to the bottom, keeping their relative order. `all` is every item as (id, z) in
/// any order. Returns only the selected items' new (id, z).
pub fn to_back(all: &[(String, String)], selected: &[String]) -> Vec<(String, String)> {
    let sel = id_set(selected);
    let work = sorted_pairs(all);
    let chosen = selected_items(&work, &sel);
    let ranks = match work.first() {
        Some((_, min)) => ranks_below(min, chosen.len()),
        None => ranks_between(None, None, chosen.len()),
    };
    chosen
        .into_iter()
        .zip(ranks)
        .map(|((id, _), z)| (id, z))
        .collect()
}

/// Moves each selected item one step up past the next unselected item above it (if any).
pub fn forward(all: &[(String, String)], selected: &[String]) -> Vec<(String, String)> {
    let sel = id_set(selected);
    let mut work = sorted_pairs(all);
    // Top-down: the highest selected item moves first.
    let ids: Vec<String> = work
        .iter()
        .rev()
        .filter(|(id, _)| sel.contains(id.as_str()))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &ids {
        let Some(pos) = work.iter().position(|(i, _)| i == id) else {
            continue;
        };
        let Some(up) = (pos + 1..work.len()).find(|&j| !sel.contains(work[j].0.as_str())) else {
            continue;
        };
        let next_z = work.get(up + 1).map(|(_, z)| z.as_str());
        let z = rank_between(Some(work[up].1.as_str()), next_z);
        work[pos].1 = z;
        work.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    }
    selected_items(&work, &sel)
}

/// Moves each selected item one step down past the next unselected item below it (if any).
pub fn backward(all: &[(String, String)], selected: &[String]) -> Vec<(String, String)> {
    let sel = id_set(selected);
    let mut work = sorted_pairs(all);
    // Bottom-up: the lowest selected item moves first.
    let ids: Vec<String> = work
        .iter()
        .filter(|(id, _)| sel.contains(id.as_str()))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &ids {
        let Some(pos) = work.iter().position(|(i, _)| i == id) else {
            continue;
        };
        let Some(down) = (0..pos).rev().find(|&j| !sel.contains(work[j].0.as_str())) else {
            continue;
        };
        let prev_z = if down > 0 {
            Some(work[down - 1].1.as_str())
        } else {
            None
        };
        let z = rank_between(prev_z, Some(work[down].1.as_str()));
        work[pos].1 = z;
        work.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    }
    selected_items(&work, &sel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> String {
        x.to_string()
    }

    fn items() -> Vec<(String, String)> {
        vec![(s("a"), s("a0")), (s("b"), s("a1")), (s("c"), s("a2"))]
    }

    fn z_of(out: &[(String, String)], id: &str) -> String {
        out.iter()
            .find(|(i, _)| i == id)
            .map(|(_, z)| z.clone())
            .expect("id present")
    }

    #[test]
    fn digits_are_in_ascending_byte_order() {
        assert!(DIGITS.windows(2).all(|w| w[0] < w[1]));
        assert!("Z" < "a");
        assert!("9" < "A");
    }

    #[test]
    fn rank_between_unbounded() {
        assert_eq!(rank_between(None, None), "V");
    }

    #[test]
    fn rank_between_simple_pair() {
        let r = rank_between(Some("a"), Some("b"));
        assert!("a" < r.as_str() && r.as_str() < "b");
    }

    #[test]
    fn rank_between_prefix_neighbour() {
        let r = rank_between(Some("a"), Some("a1"));
        assert!("a" < r.as_str() && r.as_str() < "a1", "got {r}");
    }

    #[test]
    fn a_thousand_front_inserts_decrease() {
        let mut prev = rank_between(None, None);
        for _ in 0..1000 {
            let r = rank_between(None, Some(prev.as_str()));
            assert!(!r.is_empty());
            assert!(r < prev);
            prev = r;
        }
    }

    #[test]
    fn a_thousand_end_inserts_increase() {
        let mut prev = rank_between(None, None);
        for _ in 0..1000 {
            let r = rank_between(Some(prev.as_str()), None);
            assert!(!r.is_empty());
            assert!(r > prev);
            prev = r;
        }
    }

    #[test]
    fn a_thousand_midpoints_stay_between_neighbours() {
        let mut lo = s("a");
        for _ in 0..1000 {
            let r = rank_between(Some(lo.as_str()), Some("b"));
            assert!(lo.as_str() < r.as_str() && r.as_str() < "b");
            lo = r;
        }
    }

    #[test]
    fn to_front_puts_selected_above_everything() {
        let out = to_front(&items(), &[s("a")]);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.as_str() > "a2");
    }

    #[test]
    fn to_back_puts_selected_below_everything() {
        let out = to_back(&items(), &[s("c")]);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.as_str() < "a0");
    }

    #[test]
    fn forward_moves_past_next_unselected() {
        let out = forward(&items(), &[s("a")]);
        let z = z_of(&out, "a");
        assert!("a1" < z.as_str() && z.as_str() < "a2", "got {z}");
    }

    #[test]
    fn backward_moves_below_next_unselected() {
        let out = backward(&items(), &[s("c")]);
        let z = z_of(&out, "c");
        assert!("a0" < z.as_str() && z.as_str() < "a1", "got {z}");
    }
}
