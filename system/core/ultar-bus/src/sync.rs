//! Small helpers around `std::sync` so that a poisoned lock never panics.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};
use std::time::Duration;

/// Locks a mutex; a poisoned lock is used as it is.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read-locks an `RwLock`; a poisoned lock is used as it is.
pub(crate) fn read<T>(rwlock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    rwlock.read().unwrap_or_else(PoisonError::into_inner)
}

/// Replaces the value behind an `RwLock`.
pub(crate) fn replace<T>(rwlock: &RwLock<T>, value: T) {
    *rwlock.write().unwrap_or_else(PoisonError::into_inner) = value;
}

/// Waits on a condition variable for at most `timeout`.
pub(crate) fn wait_timeout<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    timeout: Duration,
) -> MutexGuard<'a, T> {
    match condvar.wait_timeout(guard, timeout) {
        Ok((guard, _)) => guard,
        Err(poisoned) => poisoned.into_inner().0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn poisoned_locks_stay_usable() {
        let mutex = Arc::new(Mutex::new(1));
        let rwlock = Arc::new(RwLock::new(1));
        let (m, r) = (mutex.clone(), rwlock.clone());
        let _ = std::thread::spawn(move || {
            let _guard = m.lock().unwrap();
            let _write = r.write().unwrap();
            panic!("poison both locks");
        })
        .join();
        assert!(mutex.is_poisoned());
        *lock(&mutex) += 1;
        assert_eq!(*lock(&mutex), 2);
        replace(&rwlock, 5);
        assert_eq!(*read(&rwlock), 5);
    }

    #[test]
    fn wait_timeout_returns_after_the_timeout() {
        let mutex = Mutex::new(0);
        let condvar = Condvar::new();
        let guard = wait_timeout(&condvar, lock(&mutex), Duration::from_millis(5));
        assert_eq!(*guard, 0);
    }
}
