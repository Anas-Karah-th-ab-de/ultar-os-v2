//! The update policy the user or administrator sets, and the decisions derived from it.

use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use crate::manifest::valid_ident;
use crate::stage::Pacer;
use std::path::Path;
use std::time::Duration;

/// Minutes in a day.
pub const DAY_MINUTES: u16 = 24 * 60;
/// Largest rate limit (KiB/s) that can be set; 0 means unlimited.
pub const MAX_RATE_KIB: u32 = 1024 * 1024;

/// What the device may do by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Search, download and prepare updates without being asked. A restart is never forced.
    pub automatic: bool,
    /// Daily window for automatic work as minutes of the day in UTC (`start`, `end`); the
    /// window may cross midnight; `None` or equal values mean "any time".
    pub window: Option<(u16, u16)>,
    /// Update channel.
    pub channel: String,
    /// Automatic downloads may use a metered connection.
    pub allow_metered: bool,
    /// Download rate limit in KiB per second; 0 = unlimited.
    pub rate_limit_kib: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            automatic: true,
            window: None,
            channel: "stable".to_string(),
            allow_metered: false,
            rate_limit_kib: 0,
        }
    }
}

/// Why automatic work does not run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBlock {
    /// Automatic updates are switched off.
    Disabled,
    /// The time is outside the window.
    OutsideWindow,
    /// The connection is metered, or its kind is unknown.
    Metered,
}

impl Policy {
    /// Checks all values.
    pub fn validate(&self) -> Result<(), UpdateError> {
        let window_ok = self
            .window
            .is_none_or(|(start, end)| start < DAY_MINUTES && end < DAY_MINUTES);
        if window_ok && valid_ident(&self.channel) && self.rate_limit_kib <= MAX_RATE_KIB {
            Ok(())
        } else {
            Err(UpdateError::InvalidArgument)
        }
    }

    /// True when `now` (seconds since 1970) lies inside the window.
    pub fn in_window(&self, now: u64) -> bool {
        let minute = ((now / 60) % u64::from(DAY_MINUTES)) as u16;
        match self.window {
            None => true,
            Some((start, end)) if start == end => true,
            Some((start, end)) if start < end => (start..end).contains(&minute),
            Some((start, end)) => minute >= start || minute < end,
        }
    }

    /// May automatic work run now? `metered`: what the network service reported
    /// (`None` = unknown, treated as metered).
    pub fn automatic_allowed(&self, now: u64, metered: Option<bool>) -> Result<(), AutoBlock> {
        if !self.automatic {
            return Err(AutoBlock::Disabled);
        }
        if !self.in_window(now) {
            return Err(AutoBlock::OutsideWindow);
        }
        if metered != Some(false) && !self.allow_metered {
            return Err(AutoBlock::Metered);
        }
        Ok(())
    }

    fn encode(&self) -> String {
        let window = self
            .window
            .map_or_else(|| "-".to_string(), |(s, e)| format!("{s}-{e}"));
        format!(
            "automatic={}\nwindow={window}\nchannel={}\nallow_metered={}\nrate_limit_kib={}\n",
            u8::from(self.automatic),
            self.channel,
            u8::from(self.allow_metered),
            self.rate_limit_kib
        )
    }

    fn decode(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let mut value = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix('=');
        let flag = |text: &str| match text {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let automatic = flag(value("automatic")?)?;
        let window = match value("window")? {
            "-" => None,
            range => {
                let (start, end) = range.split_once('-')?;
                Some((start.parse().ok()?, end.parse().ok()?))
            }
        };
        let channel = value("channel")?.to_string();
        let allow_metered = flag(value("allow_metered")?)?;
        let rate_limit_kib = value("rate_limit_kib")?.parse().ok()?;
        let policy = Self {
            automatic,
            window,
            channel,
            allow_metered,
            rate_limit_kib,
        };
        (lines.next().is_none() && policy.validate().is_ok()).then_some(policy)
    }

    /// Loads the policy. A missing file gives the default. A damaged file gives the default
    /// with automatic updates **off** (the device does nothing by itself until someone sets a
    /// policy again); the flag reports that.
    pub fn load(path: &Path) -> (Self, bool) {
        match read_bounded(path, 512) {
            Ok(None) => (Self::default(), false),
            Ok(Some(bytes)) => match std::str::from_utf8(&bytes).ok().and_then(Self::decode) {
                Some(policy) => (policy, false),
                None => (Self::manual(), true),
            },
            Err(_) => (Self::manual(), true),
        }
    }

    fn manual() -> Self {
        Self {
            automatic: false,
            ..Self::default()
        }
    }

    /// Validates and stores the policy durably.
    pub fn store(&self, path: &Path) -> Result<(), UpdateError> {
        self.validate()?;
        Ok(write_atomic(path, self.encode().as_bytes())?)
    }
}

/// A pacer that keeps the average rate at or below a limit.
///
/// Time comes from the two closures, so the behaviour is testable without waiting:
/// `elapsed` returns the time since the transfer started, `sleep` waits.
pub struct RatePacer<E: FnMut() -> Duration, S: FnMut(Duration)> {
    bytes_per_second: u64,
    sent: u64,
    elapsed: E,
    sleep: S,
}

impl<E: FnMut() -> Duration, S: FnMut(Duration)> RatePacer<E, S> {
    /// A pacer for `kib_per_second` (0 = unlimited).
    pub fn new(kib_per_second: u32, elapsed: E, sleep: S) -> Self {
        Self {
            bytes_per_second: u64::from(kib_per_second) * 1024,
            sent: 0,
            elapsed,
            sleep,
        }
    }
}

impl<E: FnMut() -> Duration, S: FnMut(Duration)> Pacer for RatePacer<E, S> {
    fn consumed(&mut self, bytes: u64) {
        if self.bytes_per_second == 0 {
            return;
        }
        self.sent = self.sent.saturating_add(bytes);
        let due_ms = self.sent.saturating_mul(1000) / self.bytes_per_second;
        let due = Duration::from_millis(due_ms);
        let now = (self.elapsed)();
        if due > now {
            (self.sleep)(due - now);
        }
    }
}

/// A [`RatePacer`] on the real monotonic clock.
pub fn real_pacer(kib_per_second: u32) -> impl Pacer {
    let start = std::time::Instant::now();
    RatePacer::new(kib_per_second, move || start.elapsed(), std::thread::sleep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn default_is_automatic_on_unmetered_networks_only() {
        let p = Policy::default();
        assert_eq!(p.automatic_allowed(0, Some(false)), Ok(()));
        assert_eq!(p.automatic_allowed(0, Some(true)), Err(AutoBlock::Metered));
        assert_eq!(p.automatic_allowed(0, None), Err(AutoBlock::Metered));
        let metered_ok = Policy {
            allow_metered: true,
            ..Policy::default()
        };
        assert_eq!(metered_ok.automatic_allowed(0, None), Ok(()));
        let off = Policy {
            automatic: false,
            ..Policy::default()
        };
        assert_eq!(
            off.automatic_allowed(0, Some(false)),
            Err(AutoBlock::Disabled)
        );
    }

    #[test]
    fn window_including_midnight_crossing() {
        let at = |h: u64, m: u64| 86_400 * 3 + h * 3600 + m * 60;
        let night = Policy {
            window: Some((23 * 60, 5 * 60)),
            ..Policy::default()
        };
        assert!(night.in_window(at(23, 0)));
        assert!(night.in_window(at(0, 30)));
        assert!(night.in_window(at(4, 59)));
        assert!(!night.in_window(at(5, 0)));
        assert!(!night.in_window(at(12, 0)));
        assert_eq!(
            night.automatic_allowed(at(12, 0), Some(false)),
            Err(AutoBlock::OutsideWindow)
        );
        let day = Policy {
            window: Some((60, 120)),
            ..Policy::default()
        };
        assert!(!day.in_window(at(0, 59)));
        assert!(day.in_window(at(1, 0)));
        assert!(!day.in_window(at(2, 0)));
        let same = Policy {
            window: Some((7, 7)),
            ..Policy::default()
        };
        assert!(same.in_window(at(15, 0)));
    }

    #[test]
    fn store_load_round_trip_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy");
        assert_eq!(Policy::load(&path), (Policy::default(), false));
        let p = Policy {
            automatic: false,
            window: Some((1380, 300)),
            channel: "beta".into(),
            allow_metered: true,
            rate_limit_kib: 512,
        };
        p.store(&path).unwrap();
        assert_eq!(Policy::load(&path), (p.clone(), false));
        for bad in [
            Policy {
                window: Some((1440, 0)),
                ..p.clone()
            },
            Policy {
                window: Some((0, 2000)),
                ..p.clone()
            },
            Policy {
                channel: "../x".into(),
                ..p.clone()
            },
            Policy {
                channel: String::new(),
                ..p.clone()
            },
            Policy {
                rate_limit_kib: MAX_RATE_KIB + 1,
                ..p.clone()
            },
        ] {
            assert!(matches!(
                bad.store(&path),
                Err(UpdateError::InvalidArgument)
            ));
        }
        assert_eq!(
            Policy::load(&path).0,
            p,
            "a refused policy does not replace the stored one"
        );
    }

    #[test]
    fn damaged_policy_switches_automatic_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy");
        for bad in [
            "",
            "automatic=1\n",
            "automatic=2\nwindow=-\nchannel=stable\nallow_metered=0\nrate_limit_kib=0\n",
            "automatic=1\nwindow=9-99999\nchannel=stable\nallow_metered=0\nrate_limit_kib=0\n",
            "automatic=1\nwindow=-\nchannel=st able\nallow_metered=0\nrate_limit_kib=0\n",
            "automatic=1\nwindow=-\nchannel=stable\nallow_metered=0\nrate_limit_kib=0\nextra=1\n",
        ] {
            std::fs::write(&path, bad).unwrap();
            let (policy, damaged) = Policy::load(&path);
            assert!(damaged, "{bad:?}");
            assert!(!policy.automatic);
            assert_eq!(policy.channel, "stable");
        }
    }

    #[test]
    fn pacer_sleeps_exactly_what_the_limit_demands() {
        let now = RefCell::new(Duration::ZERO);
        let slept = RefCell::new(Vec::new());
        let mut pacer = RatePacer::new(
            100,
            || *now.borrow(),
            |d| {
                slept.borrow_mut().push(d);
                *now.borrow_mut() += d;
            },
        );
        pacer.consumed(100 * 1024); // one second's worth at once
        pacer.consumed(50 * 1024);
        *now.borrow_mut() += Duration::from_secs(10); // the network was slow by itself
        pacer.consumed(1024);
        assert_eq!(
            *slept.borrow(),
            vec![Duration::from_secs(1), Duration::from_millis(500)]
        );
        let mut unlimited = RatePacer::new(0, || Duration::ZERO, |_| panic!("must not sleep"));
        unlimited.consumed(u64::MAX);
        real_pacer(0).consumed(1);
    }
}
