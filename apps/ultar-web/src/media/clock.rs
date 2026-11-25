//! The clock sound and pictures follow: it runs while playing and stands while paused.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A clock that can be paused.
#[derive(Debug, Default)]
pub struct Clock {
    inner: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Time played before the current run.
    before: Duration,
    /// Start of the current run (`None`: paused).
    since: Option<Instant>,
}

impl Clock {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Time played so far.
    pub fn now(&self) -> Duration {
        let s = self.state();
        s.before + s.since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    /// `true` while it runs.
    pub fn running(&self) -> bool {
        self.state().since.is_some()
    }

    /// Runs (again).
    pub fn start(&self) {
        let mut s = self.state();
        if s.since.is_none() {
            s.since = Some(Instant::now());
        }
    }

    /// Stands.
    pub fn pause(&self) {
        let mut s = self.state();
        if let Some(t) = s.since.take() {
            s.before += t.elapsed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_stands() {
        let c = Clock::default();
        assert_eq!(c.now(), Duration::ZERO);
        c.start();
        std::thread::sleep(Duration::from_millis(20));
        c.pause();
        let t = c.now();
        assert!(t >= Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(c.now(), t, "paused");
        assert!(!c.running());
    }
}
