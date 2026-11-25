//! Pointer acceleration (`pointer.acceleration`, `Request::ConfigurePointer`): faster
//! movements travel farther. The profile measures the velocity of the relative motion
//! between two events; a driver that reports no usable event times is passed through
//! unchanged (as with the key repeat).

/// Velocity below which the pointer moves as reported (device units per millisecond).
const SLOW: f64 = 3.0;
/// Velocity at which the pointer moves three times as far (device units per millisecond).
const FAST: f64 = 30.0;
/// The most the profile multiplies a motion by.
const MOST: f64 = 3.0;

/// The acceleration profile and the time of the last motion event.
#[derive(Clone, Debug)]
pub struct Acceleration {
    /// The profile is on (`pointer.acceleration`; the schema's default).
    pub on: bool,
    /// When the last motion event was, in microseconds (0: none yet).
    last_us: u64,
}

impl Default for Acceleration {
    fn default() -> Self {
        Acceleration {
            on: true,
            last_us: 0,
        }
    }
}

impl Acceleration {
    /// The factor a motion of `(dx, dy)` at `time_us` is multiplied by: between 1 (slow)
    /// and [`MOST`] (fast). Without a previous event time, or with one that does not move
    /// forward, the motion passes as reported – a velocity cannot be measured then.
    pub fn factor(&mut self, dx: f32, dy: f32, time_us: u64) -> f64 {
        let previous = self.last_us;
        if time_us > previous {
            self.last_us = time_us;
        }
        if !self.on || previous == 0 || time_us <= previous {
            return 1.0;
        }
        let dt_ms = (time_us - previous) as f64 / 1000.0;
        let velocity = f64::from(dx).hypot(f64::from(dy)) / dt_ms;
        let share = (velocity - SLOW).clamp(0.0, FAST - SLOW) / (FAST - SLOW);
        1.0 + share * (MOST - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A motion of `dx` units at `ms` milliseconds.
    fn at(accel: &mut Acceleration, dx: f32, ms: u64) -> f64 {
        accel.factor(dx, 0.0, ms * 1000)
    }

    #[test]
    fn slow_motion_passes_and_fast_motion_travels_farther() {
        let mut accel = Acceleration::default();
        assert!(accel.on);
        // The first event has no previous time: as reported.
        assert_eq!(at(&mut accel, 300.0, 10), 1.0);
        // 30 units per 10 ms = 3/ms: the slow end, factor 1.
        assert_eq!(at(&mut accel, 30.0, 20), 1.0);
        assert_eq!(at(&mut accel, 30.0, 30), 1.0);
        // 165 units per 10 ms = 16.5/ms: halfway, factor 2.
        assert_eq!(at(&mut accel, 165.0, 40), 2.0);
        assert_eq!(at(&mut accel, 165.0, 50), 2.0);
        // 300 units per 10 ms = 30/ms and beyond: factor 3, capped.
        assert_eq!(at(&mut accel, 300.0, 60), 3.0);
        assert_eq!(at(&mut accel, 3000.0, 70), 3.0);
        // Direction does not matter, only speed.
        assert_eq!(at(&mut accel, -165.0, 80), 2.0);
    }

    #[test]
    fn switched_off_the_motion_passes_as_reported() {
        let mut accel = Acceleration {
            on: false,
            ..Acceleration::default()
        };
        for ms in 1..=4 {
            assert_eq!(at(&mut accel, 300.0, ms * 10), 1.0);
        }
        // Turning it on at runtime measures again at once (the times were kept).
        accel.on = true;
        assert_eq!(at(&mut accel, 300.0, 50), 3.0);
        accel.on = false;
        assert_eq!(at(&mut accel, 300.0, 60), 1.0);
    }

    #[test]
    fn events_without_usable_times_pass_as_reported() {
        let mut accel = Acceleration::default();
        // A driver that stamps every event 0 (tests, a kernel without times).
        for _ in 0..4 {
            assert_eq!(accel.factor(300.0, 0.0, 0), 1.0);
        }
        // Times that stand still or go backwards cannot measure a velocity either.
        let mut accel = Acceleration::default();
        assert_eq!(at(&mut accel, 300.0, 10), 1.0);
        assert_eq!(at(&mut accel, 300.0, 10), 1.0, "same time");
        assert_eq!(at(&mut accel, 300.0, 5), 1.0, "backwards");
        assert_eq!(at(&mut accel, 300.0, 20), 3.0, "forward again");
    }
}
