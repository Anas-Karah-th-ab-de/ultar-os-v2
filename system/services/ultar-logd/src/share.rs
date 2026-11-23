//! Fair admission to the engine's queue: every source may have at most a
//! fixed number of records waiting in the queue all sources share.
//!
//! Review follow-up to round 3: a source that floods inside its rate limit
//! filled the shared queue whenever the engine thread was slower (load, slow
//! disk), and the records of every other source were dropped. Now a record
//! takes a [`Ticket`] of its source before it enters the queue; the ticket
//! is given back when the engine has handled the record (it is dropped with
//! the delivery). A source at its share waits on its own connection – its
//! peer's socket buffer fills, the logging API there drops and counts – and
//! never takes a slot another source needs: the shares of all sources
//! together fit the queue (`IngestLimits::fits`).

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How long a reader waits for its source's share before it looks again
/// whether the service stops.
const WAIT_STEP: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Count {
    waiting: Mutex<usize>,
    freed: Condvar,
}

/// The records of one source that wait in the queue.
#[derive(Clone, Default)]
pub struct Share(Arc<Count>);

impl Share {
    /// Records of this source in the queue now.
    pub fn waiting(&self) -> usize {
        *self
            .0
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes a ticket once fewer than `limit` records of the source wait.
    /// `None` when `stop()` turns true while it waits.
    pub fn take(&self, limit: usize, stop: impl Fn() -> bool) -> Option<Ticket> {
        let mut waiting = self
            .0
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while *waiting >= limit.max(1) {
            if stop() {
                return None;
            }
            waiting = self
                .0
                .freed
                .wait_timeout(waiting, WAIT_STEP)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *waiting += 1;
        Some(Ticket(self.0.clone()))
    }
}

/// One record's place in its source's share; given back when dropped.
pub struct Ticket(Arc<Count>);

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut waiting = self
            .0
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *waiting = waiting.saturating_sub(1);
        self.0.freed.notify_one();
    }
}

impl fmt::Debug for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ticket")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_waits_at_its_share_and_continues_when_a_ticket_comes_back() {
        let share = Share::default();
        let first = share.take(2, || false).unwrap();
        let _second = share.take(2, || false).unwrap();
        assert_eq!(share.waiting(), 2);
        // At the share a stopping reader gives up instead of waiting.
        assert!(share.take(2, || true).is_none());
        let waiter = {
            let share = share.clone();
            std::thread::spawn(move || share.take(2, || false))
        };
        drop(first);
        let third = waiter.join().unwrap();
        assert!(third.is_some());
        assert_eq!(share.waiting(), 2);
        drop(third);
        assert_eq!(share.waiting(), 1);
    }
}
