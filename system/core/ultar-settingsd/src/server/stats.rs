//! Counters of a running server.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Counters of a running server.
#[derive(Debug, Default)]
pub struct ServerStats {
    pub(super) lost_events: AtomicU64,
    pub(super) requests: AtomicU64,
    capability_reports: Mutex<u64>,
    capability_seen: Condvar,
}

impl ServerStats {
    /// Events that could not be published (bus down or publication denied).
    pub fn lost_events(&self) -> u64 {
        self.lost_events.load(Ordering::Relaxed)
    }

    /// Requests handled.
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// Capability reports of the device service taken over so far.
    pub fn capability_reports(&self) -> u64 {
        *self
            .capability_reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Blocks until at least `count` capability reports were taken over;
    /// `false` when that did not happen within `timeout`.
    pub fn wait_capability_reports(&self, count: u64, timeout: Duration) -> bool {
        let seen = self
            .capability_reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (seen, _) = self
            .capability_seen
            .wait_timeout_while(seen, timeout, |seen| *seen < count)
            .unwrap_or_else(PoisonError::into_inner);
        *seen >= count
    }

    pub(super) fn capability_report_taken(&self) {
        let mut seen = self
            .capability_reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *seen += 1;
        self.capability_seen.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_start_at_zero_and_reports_wake_waiters() {
        let stats = ServerStats::default();
        assert_eq!(
            (
                stats.lost_events(),
                stats.requests(),
                stats.capability_reports()
            ),
            (0, 0, 0)
        );
        assert!(stats.wait_capability_reports(0, Duration::ZERO));
        assert!(!stats.wait_capability_reports(1, Duration::ZERO));
        stats.capability_report_taken();
        assert!(stats.wait_capability_reports(1, Duration::ZERO));
        stats.lost_events.fetch_add(2, Ordering::Relaxed);
        stats.requests.fetch_add(3, Ordering::Relaxed);
        assert_eq!(
            (
                stats.lost_events(),
                stats.requests(),
                stats.capability_reports()
            ),
            (2, 3, 1)
        );
    }
}
