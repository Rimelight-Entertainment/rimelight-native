//! Motion accessor (P0.14). Every animated component takes its durations and easings from [`Motion`], never from
//! literals. When reduced motion is on, every duration is zero. The Slint app passes the reduced-motion setting here.

use rimelight_design::tokens::motion as tokens;

/// Resolved durations (milliseconds) and easings for the current motion setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub fast_ms: u32,
    pub base_ms: u32,
    pub slow_ms: u32,
    /// Period of looping animations (spinner, skeleton pulse, indeterminate progress), in milliseconds.
    pub loop_ms: u32,
    pub standard: [f32; 4],
    pub enter: [f32; 4],
    pub reduced: bool,
}

impl Motion {
    /// The tokens as written, or every duration zero when `reduced`. Easings stay (they have no duration).
    pub fn resolve(reduced: bool) -> Self {
        let scale = |ms: u32| if reduced { 0 } else { ms };
        Self {
            fast_ms: scale(tokens::FAST_MS),
            base_ms: scale(tokens::BASE_MS),
            slow_ms: scale(tokens::SLOW_MS),
            loop_ms: scale(tokens::LOOP_MS),
            standard: tokens::EASE_STANDARD,
            enter: tokens::EASE_ENTER,
            reduced,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_motion_keeps_the_token_durations() {
        let m = Motion::resolve(false);
        assert_eq!((m.fast_ms, m.base_ms, m.slow_ms), (120, 200, 320));
        assert_eq!(m.loop_ms, 1200);
        assert!(!m.reduced);
    }

    #[test]
    fn reduced_motion_resolves_every_duration_to_zero() {
        let m = Motion::resolve(true);
        assert_eq!((m.fast_ms, m.base_ms, m.slow_ms, m.loop_ms), (0, 0, 0, 0));
        assert!(m.reduced);
        assert_eq!(m.standard, tokens::EASE_STANDARD);
    }
}
