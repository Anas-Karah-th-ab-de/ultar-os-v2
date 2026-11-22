//! Shared plumbing of the mock devices: locked state plus the "fail next call" switch.

use crate::error::{HalError, HalResult};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// State of one mock device behind a lock, with an optional error for the next call.
#[derive(Debug, Default)]
pub(crate) struct MockCell<T> {
    state: Mutex<T>,
    failure: Mutex<Option<HalError>>,
}

impl<T> MockCell<T> {
    /// Wraps the initial state.
    pub(crate) fn new(state: T) -> Self {
        MockCell {
            state: Mutex::new(state),
            failure: Mutex::new(None),
        }
    }

    /// Direct access for scripting; never fails (a poisoned lock is taken over).
    pub(crate) fn state(&self) -> MutexGuard<'_, T> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Arms the error returned by the next trait call.
    pub(crate) fn fail_next(&self, error: HalError) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
    }

    /// Runs a trait call: returns the armed error once, otherwise runs the action.
    pub(crate) fn call<R>(&self, action: impl FnOnce(&mut T) -> HalResult<R>) -> HalResult<R> {
        let armed = self
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match armed {
            Some(error) => Err(error),
            None => action(&mut self.state()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_error_fires_exactly_once() {
        let cell = MockCell::new(1u32);
        cell.fail_next(HalError::Busy);
        assert_eq!(cell.call(|value| Ok(*value)), Err(HalError::Busy));
        assert_eq!(cell.call(|value| Ok(*value)), Ok(1));
        *cell.state() = 5;
        assert_eq!(cell.call(|value| Ok(*value)), Ok(5));
    }

    #[test]
    fn poisoned_lock_is_taken_over() {
        let cell = std::sync::Arc::new(MockCell::new(7u32));
        let clone = cell.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.state();
            panic!("poison the lock");
        })
        .join();
        assert_eq!(*cell.state(), 7);
    }
}
