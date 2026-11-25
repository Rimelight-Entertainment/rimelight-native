//! A stable presence colour per user (CMS.6). The user id is hashed (FNV-1a, then a 64-bit finaliser) into one fixed palette, so
//! the same person has the same colour in every editor, on every device, and in every session. The hash is written out here
//! rather than taken from `std::hash`, whose output is not guaranteed across Rust releases.

/// The eight presence colours. Each reaches a 4.5:1 contrast ratio against white text (checked by a test).
pub const PRESENCE_PALETTE: [(u8, u8, u8); 8] = [
    (0x00, 0x72, 0xB2), // blue
    (0xA8, 0x43, 0x00), // burnt orange
    (0x00, 0x7A, 0x59), // green
    (0xA4, 0x49, 0x7E), // plum
    (0x5B, 0x3F, 0xA8), // violet
    (0x5E, 0x6B, 0x00), // olive
    (0xB3, 0x12, 0x3A), // crimson
    (0x4B, 0x55, 0x63), // slate
];

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The 64-bit finaliser of MurmurHash3: spreads the bits so the low bits used for the palette index are well mixed.
fn finalise(mut hash: u64) -> u64 {
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^= hash >> 33;
    hash
}

/// The palette index of a user id.
pub fn palette_index(user_id: &str) -> usize {
    (finalise(fnv1a(user_id.as_bytes())) % PRESENCE_PALETTE.len() as u64) as usize
}

/// The presence colour of a user id, as RGB.
pub fn colour_for(user_id: &str) -> (u8, u8, u8) {
    PRESENCE_PALETTE[palette_index(user_id)]
}

/// The presence colour of a user id, as `#rrggbb` (the form `PageDeps::color` and the sync session take).
pub fn colour_hex(user_id: &str) -> String {
    let (r, g, b) = colour_for(user_id);
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an sRGB colour.
    fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
        fn linear(c: u8) -> f64 {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }

    #[test]
    fn fnv1a_matches_the_published_vector() {
        // FNV-1a 64 of "a" (the reference test vector).
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn colour_is_stable_for_a_user() {
        let first = colour_for("user-42");
        for _ in 0..100 {
            assert_eq!(colour_for("user-42"), first);
        }
        assert_eq!(colour_hex("user-42"), colour_hex("user-42"));
        let hex = colour_hex("user-42");
        assert_eq!(hex.len(), 7);
        assert!(hex.starts_with('#'));
    }

    #[test]
    fn the_caller_and_others_use_the_same_rule() {
        // The caller's colour is the colour of the caller's id; no separate path exists for "me".
        let me = "caller-id";
        assert_eq!(colour_for(me), PRESENCE_PALETTE[palette_index(me)]);
    }

    #[test]
    fn palette_entries_are_distinct() {
        for (i, a) in PRESENCE_PALETTE.iter().enumerate() {
            for b in PRESENCE_PALETTE.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn every_palette_colour_has_4_5_to_1_contrast_with_white() {
        for colour in PRESENCE_PALETTE {
            let contrast = 1.05 / (luminance(colour) + 0.05);
            assert!(contrast >= 4.5, "{colour:?} has contrast {contrast:.2}");
        }
    }

    #[test]
    fn ids_spread_over_the_palette() {
        // UNSURE: the 20% band is a guess at what a good hash gives for 8,000 ids; check it at the build.
        const IDS: usize = 8_000;
        let mut counts = [0usize; 8];
        for n in 0..IDS {
            counts[palette_index(&format!("user-{n}"))] += 1;
        }
        let expected = IDS / PRESENCE_PALETTE.len();
        for (slot, count) in counts.iter().enumerate() {
            assert!(
                *count >= expected * 4 / 5 && *count <= expected * 6 / 5,
                "slot {slot} got {count}, expected about {expected}"
            );
        }
    }
}
