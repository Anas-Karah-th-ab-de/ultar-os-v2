//! How a held key repeats (`Request::ConfigureKeyRepeat`, settings `keyboard.repeat.*`).

use ultar_hal::KeyState;

/// How a held key repeats: the settings `keyboard.repeat.*`, and the key being held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRepeat {
    /// `false`: repeats of the driver are dropped.
    pub enabled: bool,
    /// Time before the first repeat in milliseconds (0 = as the driver reports).
    pub delay_ms: u16,
    /// Repeats per second (0 = as the driver reports).
    pub per_second: u8,
    /// The key last pressed, when it was pressed and when its next repeat is due
    /// (microseconds).
    held: Option<(u16, u64, u64)>,
}

impl Default for KeyRepeat {
    fn default() -> Self {
        KeyRepeat {
            enabled: true,
            delay_ms: 0,
            per_second: 0,
            held: None,
        }
    }
}

impl KeyRepeat {
    /// Takes a key event at `time_us`; `false` when it is a repeat the settings drop. The
    /// driver's repeats are thinned out, never multiplied: a repeat passes when it is due,
    /// and the next one is due one period later (so the average rate is the set one as long
    /// as the driver repeats at least that fast).
    pub fn passes(&mut self, code: u16, state: KeyState, time_us: u64) -> bool {
        match state {
            KeyState::Pressed => {
                let due = time_us.saturating_add(u64::from(self.delay_ms) * 1000);
                self.held = Some((code, time_us, due));
                true
            }
            KeyState::Released => {
                if self.held.is_some_and(|(held, _, _)| held == code) {
                    self.held = None;
                }
                true
            }
            KeyState::Repeated if !self.enabled => false,
            KeyState::Repeated => match &mut self.held {
                Some((held, pressed, due)) if *held == code => {
                    // A driver without usable event times cannot be measured: as reported.
                    if time_us <= *pressed {
                        return true;
                    }
                    if time_us < *due {
                        return false;
                    }
                    let period = match self.per_second {
                        0 => 0,
                        n => 1_000_000 / u64::from(n),
                    };
                    *due = due.saturating_add(period).max(time_us);
                    true
                }
                // A repeat of another key than the last pressed one: as the driver reports.
                _ => true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(repeat: &mut KeyRepeat, times_ms: &[u64]) -> Vec<u64> {
        times_ms
            .iter()
            .copied()
            .filter(|t| repeat.passes(30, KeyState::Repeated, t * 1000))
            .collect()
    }

    /// The driver's repeats: the first after 250 ms, then every 33 ms.
    fn driver(count: u64) -> Vec<u64> {
        (0..count).map(|n| 250 + n * 33).collect()
    }

    #[test]
    fn unset_values_pass_what_the_driver_reports() {
        let mut r = KeyRepeat::default();
        assert!(r.passes(30, KeyState::Pressed, 0));
        assert_eq!(held(&mut r, &driver(10)).len(), 10);
        assert!(r.passes(30, KeyState::Released, 600_000));
    }

    #[test]
    fn switched_off_a_held_key_gives_one_press_only() {
        let mut r = KeyRepeat {
            enabled: false,
            ..KeyRepeat::default()
        };
        assert!(r.passes(30, KeyState::Pressed, 0));
        assert!(held(&mut r, &driver(10)).is_empty());
        assert!(r.passes(30, KeyState::Released, 600_000));
    }

    #[test]
    fn the_delay_and_the_rate_thin_out_the_repeats() {
        let mut r = KeyRepeat {
            delay_ms: 500,
            per_second: 10,
            ..KeyRepeat::default()
        };
        assert!(r.passes(30, KeyState::Pressed, 0));
        // Two seconds of driver repeats: nothing before 500 ms, then ten per second.
        let passed = held(&mut r, &driver(54));
        assert!(passed.iter().all(|t| *t >= 500), "{passed:?}");
        let second: Vec<_> = passed
            .iter()
            .filter(|t| (1000..2000).contains(*t))
            .collect();
        assert_eq!(second.len(), 10, "{passed:?}");
        // A new press starts the delay again.
        assert!(r.passes(30, KeyState::Released, 2_100_000));
        assert!(r.passes(31, KeyState::Pressed, 2_200_000));
        assert!(!r.passes(31, KeyState::Repeated, 2_450_000));
        assert!(r.passes(31, KeyState::Repeated, 2_700_000));
    }

    #[test]
    fn repeats_without_event_times_pass_as_reported() {
        let mut r = KeyRepeat {
            delay_ms: 500,
            per_second: 5,
            ..KeyRepeat::default()
        };
        assert!(r.passes(30, KeyState::Pressed, 0));
        assert_eq!(held(&mut r, &[0, 0, 0]).len(), 3);
    }

    #[test]
    fn a_rate_above_the_drivers_passes_every_repeat_without_a_burst() {
        let mut r = KeyRepeat {
            per_second: 60,
            ..KeyRepeat::default()
        };
        assert!(r.passes(30, KeyState::Pressed, 0));
        assert_eq!(held(&mut r, &driver(20)).len(), 20);
    }
}
