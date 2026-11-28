//! How often the screen is looked at: never more than 30 times a second, less when
//! nothing changes, and slower while the link cannot keep up.

use std::time::Duration;

/// Shortest time between two looks (30 frames a second at most).
pub const MIN: Duration = Duration::from_millis(33);
/// Longest time between two looks while the screen is quiet.
pub const IDLE_MAX: Duration = Duration::from_millis(250);
/// Longest time between two looks while the link is slow.
pub const SLOW_MAX: Duration = Duration::from_millis(500);

/// The adaptive frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pacer {
    interval: Duration,
}

impl Default for Pacer {
    fn default() -> Self {
        Self { interval: MIN }
    }
}

impl Pacer {
    /// Time to wait before the next look.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// The screen changed and the viewer took everything: full speed.
    pub fn busy(&mut self) {
        self.interval = MIN;
    }

    /// Nothing changed: wait longer, up to [`IDLE_MAX`].
    pub fn idle(&mut self) {
        self.interval = (self.interval * 2).clamp(MIN, IDLE_MAX);
    }

    /// The viewer did not take a rectangle (link too slow): wait longer, up to [`SLOW_MAX`].
    pub fn slow(&mut self) {
        self.interval = (self.interval * 2).clamp(MIN, SLOW_MAX);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn never_faster_than_30_frames_a_second() {
        let mut p = Pacer::default();
        assert!(Duration::from_secs(1).as_millis() / p.interval().as_millis() <= 30);
        for _ in 0..10 {
            p.busy();
            assert!(p.interval() >= MIN);
            p.idle();
            p.slow();
            assert!(p.interval() >= MIN);
        }
    }

    #[test]
    fn a_quiet_screen_backs_off_and_activity_brings_full_speed_back() {
        let mut p = Pacer::default();
        let mut seen = Vec::new();
        for _ in 0..6 {
            p.idle();
            seen.push(p.interval());
        }
        assert_eq!(seen[0], Duration::from_millis(66));
        assert_eq!(seen[seen.len() - 1], IDLE_MAX);
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        p.busy();
        assert_eq!(p.interval(), MIN);
    }

    #[test]
    fn a_slow_link_backs_off_further_than_a_quiet_screen() {
        let mut p = Pacer::default();
        for _ in 0..8 {
            p.slow();
        }
        assert_eq!(p.interval(), SLOW_MAX);
        p.idle();
        assert_eq!(
            p.interval(),
            IDLE_MAX,
            "idle never waits longer than its own maximum"
        );
    }
}
