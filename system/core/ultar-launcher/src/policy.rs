//! What happens after an instance ended: pure rules with the time passed
//! in, so they are tested without a clock.
//!
//! * A window (`Role::Ui`) is never restarted: a crash is shown
//!   (`Crashed`), the user starts it again.
//! * A background service follows its manifest's `restart`; failures back
//!   off 200 ms · 2ⁿ up to 30 s; [`LOOP_FAILURES`] failed runs within
//!   [`LOOP_WINDOW`] give it up (`Failed`) until it is started on request.
//! * A requested stop is never followed by a start.

use crate::manifest::Restart;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{Exit, InstanceState, Role};

/// Failed runs in [`LOOP_WINDOW`] after which a service is given up.
pub const LOOP_FAILURES: usize = 5;
/// The window of the crash-loop rule.
pub const LOOP_WINDOW: Duration = Duration::from_secs(60);
/// First pause before a restart.
pub const FIRST_BACKOFF: Duration = Duration::from_millis(200);
/// Longest pause before a restart.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Whether an end is a failure (anything but code 0, or a stop that was
/// asked for).
pub fn is_failure(exit: Exit, requested: bool) -> bool {
    !requested && exit != Exit::Code(0)
}

/// Pause before the restart after `failures` failures in a row.
pub fn backoff(failures: u32) -> Duration {
    let factor = 1u32 << failures.min(8);
    FIRST_BACKOFF.saturating_mul(factor).min(MAX_BACKOFF)
}

/// The decision after an end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    /// Stays ended in this state.
    Stay(InstanceState),
    /// Started again at this time.
    RestartAt(Instant),
}

/// The failures an instance had recently (bounded).
#[derive(Clone, Debug, Default)]
pub struct History {
    failures: VecDeque<Instant>,
    in_a_row: u32,
}

impl History {
    /// Records an end at `now` and decides what comes next.
    pub fn after_end(
        &mut self,
        role: Role,
        restart: Option<Restart>,
        exit: Exit,
        requested: bool,
        now: Instant,
    ) -> Next {
        let failure = is_failure(exit, requested);
        if failure {
            self.failures.push_back(now);
            while self.failures.len() > LOOP_FAILURES {
                self.failures.pop_front();
            }
            self.in_a_row = self.in_a_row.saturating_add(1);
        } else {
            self.in_a_row = 0;
        }
        let ended = if failure {
            InstanceState::Crashed
        } else {
            InstanceState::Stopped
        };
        let policy = match (role, restart) {
            (Role::Service, Some(policy)) if !requested => policy,
            _ => return Next::Stay(ended),
        };
        let wanted = match policy {
            Restart::Never => false,
            Restart::OnFailure => failure,
            Restart::Always => true,
        };
        if !wanted {
            return Next::Stay(ended);
        }
        let looping = self.failures.len() >= LOOP_FAILURES
            && self
                .failures
                .front()
                .is_some_and(|first| now.saturating_duration_since(*first) < LOOP_WINDOW);
        if failure && looping {
            return Next::Stay(InstanceState::Failed);
        }
        Next::RestartAt(now + backoff(self.in_a_row.saturating_sub(1)))
    }

    /// Forgets the failures (a start on request after `Failed`).
    pub fn reset(&mut self) {
        self.failures.clear();
        self.in_a_row = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_never_restarted() {
        let mut h = History::default();
        let now = Instant::now();
        assert_eq!(
            h.after_end(Role::Ui, None, Exit::Signal(11), false, now),
            Next::Stay(InstanceState::Crashed)
        );
        assert_eq!(
            h.after_end(Role::Ui, Some(Restart::Always), Exit::Code(0), false, now),
            Next::Stay(InstanceState::Stopped)
        );
    }

    #[test]
    fn services_back_off_and_give_up_in_a_crash_loop() {
        let mut h = History::default();
        let start = Instant::now();
        let mut now = start;
        let mut pauses = Vec::new();
        for _ in 0..LOOP_FAILURES - 1 {
            match h.after_end(
                Role::Service,
                Some(Restart::OnFailure),
                Exit::Code(3),
                false,
                now,
            ) {
                Next::RestartAt(at) => pauses.push(at - now),
                other => panic!("{other:?}"),
            }
            now += Duration::from_secs(1);
        }
        assert_eq!(pauses[0], FIRST_BACKOFF);
        assert!(pauses.windows(2).all(|w| w[1] == w[0] * 2));
        assert_eq!(
            h.after_end(
                Role::Service,
                Some(Restart::OnFailure),
                Exit::Signal(9),
                false,
                now
            ),
            Next::Stay(InstanceState::Failed)
        );
        // the same number of failures spread over more than the window: restarted
        let mut slow = History::default();
        let mut now = start;
        for _ in 0..LOOP_FAILURES + 2 {
            now += LOOP_WINDOW / 4;
            let next = slow.after_end(
                Role::Service,
                Some(Restart::Always),
                Exit::Code(1),
                false,
                now,
            );
            assert!(matches!(next, Next::RestartAt(_)), "{next:?}");
        }
        slow.reset();
        assert_eq!(
            slow.after_end(
                Role::Service,
                Some(Restart::OnFailure),
                Exit::Code(1),
                false,
                now
            ),
            Next::RestartAt(now + FIRST_BACKOFF)
        );
    }

    #[test]
    fn policies_and_requested_stops() {
        let now = Instant::now();
        let mut h = History::default();
        let svc = |h: &mut History, r, e, req| h.after_end(Role::Service, Some(r), e, req, now);
        assert_eq!(
            svc(&mut h, Restart::OnFailure, Exit::Code(0), false),
            Next::Stay(InstanceState::Stopped)
        );
        assert_eq!(
            svc(&mut h, Restart::Always, Exit::Code(0), false),
            Next::RestartAt(now + FIRST_BACKOFF)
        );
        assert_eq!(
            svc(&mut h, Restart::Never, Exit::Signal(6), false),
            Next::Stay(InstanceState::Crashed)
        );
        assert_eq!(
            svc(&mut h, Restart::Always, Exit::Signal(15), true),
            Next::Stay(InstanceState::Stopped)
        );
        assert!(!is_failure(Exit::Signal(9), true));
        assert!(is_failure(Exit::Setup(113), false));
        assert_eq!(backoff(40), MAX_BACKOFF);
    }
}
