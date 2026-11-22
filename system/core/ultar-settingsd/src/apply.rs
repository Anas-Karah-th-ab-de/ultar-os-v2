//! Appliers: the components that put a value into effect.
//!
//! The service calls the applier of a setting's domain after a value was
//! accepted. Real appliers live in the HAL and the system services; this
//! crate ships a recording mock and a logging applier.

use crate::schema::Domain;
use crate::value::Value;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// One value to put into effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change<'a> {
    /// Concrete key.
    pub key: &'a str,
    /// Key template of the declaration (`display.<id>.scale`).
    pub template: &'a str,
    /// The `<id>` segment of the key, if any.
    pub instance: Option<&'a str>,
    /// The value.
    pub value: &'a Value,
    /// `true` while the value still waits for confirmation.
    pub provisional: bool,
}

/// Why a value could not be put into effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyError {
    /// The hardware or service cannot do what the value asks.
    Unsupported,
    /// The component is not ready; trying again later may work.
    Busy,
    /// The component tried and failed.
    Failed,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ApplyError::Unsupported => "value not supported by the device",
            ApplyError::Busy => "component busy",
            ApplyError::Failed => "component failed to apply the value",
        })
    }
}

impl std::error::Error for ApplyError {}

/// Puts the settings of one domain into effect.
pub trait Applier: Send {
    /// Applies one value. Must be idempotent: the service may apply the
    /// value that is already in effect again.
    fn apply(&mut self, change: &Change<'_>) -> Result<(), ApplyError>;
}

/// The appliers of all domains. A domain without applier accepts every
/// value (its consumers follow `settings.changed` on the bus).
#[derive(Default)]
pub struct Appliers {
    by_domain: BTreeMap<Domain, Box<dyn Applier>>,
}

impl fmt::Debug for Appliers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.by_domain.keys()).finish()
    }
}

impl Appliers {
    /// No appliers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the applier of a domain, replacing an earlier one.
    pub fn with(mut self, domain: Domain, applier: Box<dyn Applier>) -> Self {
        self.by_domain.insert(domain, applier);
        self
    }

    /// Registers clones of one applier for every domain.
    pub fn with_all<A: Applier + Clone + 'static>(mut self, applier: &A) -> Self {
        for domain in Domain::ALL {
            self.by_domain.insert(domain, Box::new(applier.clone()));
        }
        self
    }

    /// Applies a value through the applier of `domain`.
    pub fn apply(&mut self, domain: Domain, change: &Change<'_>) -> Result<(), ApplyError> {
        match self.by_domain.get_mut(&domain) {
            Some(applier) => applier.apply(change),
            None => Ok(()),
        }
    }
}

/// One call a [`MockApplier`] received.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedCall {
    /// Concrete key.
    pub key: String,
    /// The value.
    pub value: Value,
    /// Whether it was provisional.
    pub provisional: bool,
}

#[derive(Debug, Default)]
struct MockState {
    calls: VecDeque<AppliedCall>,
    total: u64,
    fail_next: Option<ApplyError>,
    fail_keys: BTreeMap<String, ApplyError>,
}

/// Applier for tests: records calls (the last [`MockApplier::CAPACITY`])
/// and fails on request. Clones share their state.
#[derive(Clone, Debug, Default)]
pub struct MockApplier {
    state: Arc<Mutex<MockState>>,
}

impl MockApplier {
    /// Most calls kept.
    pub const CAPACITY: usize = 4096;

    /// A mock that accepts everything.
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes the next call fail once.
    pub fn fail_next(&self, error: ApplyError) {
        self.state().fail_next = Some(error);
    }

    /// Makes every call for `key` fail until [`Self::clear_failures`].
    pub fn fail_key(&self, key: &str, error: ApplyError) {
        self.state().fail_keys.insert(key.to_owned(), error);
    }

    /// Stops all requested failures.
    pub fn clear_failures(&self) {
        let mut state = self.state();
        state.fail_next = None;
        state.fail_keys.clear();
    }

    /// The recorded calls, oldest first.
    pub fn calls(&self) -> Vec<AppliedCall> {
        self.state().calls.iter().cloned().collect()
    }

    /// Forgets the recorded calls.
    pub fn clear_calls(&self) {
        self.state().calls.clear();
    }

    /// Number of calls received, failed ones included.
    pub fn total(&self) -> u64 {
        self.state().total
    }

    /// The value of the last successful call for `key`.
    pub fn last(&self, key: &str) -> Option<Value> {
        self.state()
            .calls
            .iter()
            .rev()
            .find(|call| call.key == key)
            .map(|call| call.value.clone())
    }
}

impl Applier for MockApplier {
    fn apply(&mut self, change: &Change<'_>) -> Result<(), ApplyError> {
        let mut state = self.state();
        state.total += 1;
        if let Some(error) = state.fail_next.take() {
            return Err(error);
        }
        if let Some(error) = state.fail_keys.get(change.key) {
            return Err(*error);
        }
        if state.calls.len() >= Self::CAPACITY {
            state.calls.pop_front();
        }
        state.calls.push_back(AppliedCall {
            key: change.key.to_owned(),
            value: change.value.clone(),
            provisional: change.provisional,
        });
        Ok(())
    }
}

/// Applier that writes one line per value to a sink (standard error in the
/// daemon) and accepts everything. Clones share the sink.
pub struct LogApplier<W: Write + Send> {
    sink: Arc<Mutex<W>>,
    dropped: Arc<Mutex<u64>>,
}

impl<W: Write + Send> Clone for LogApplier<W> {
    fn clone(&self) -> Self {
        Self {
            sink: Arc::clone(&self.sink),
            dropped: Arc::clone(&self.dropped),
        }
    }
}

impl<W: Write + Send> LogApplier<W> {
    /// Logs to `sink`.
    pub fn new(sink: W) -> Self {
        Self {
            sink: Arc::new(Mutex::new(sink)),
            dropped: Arc::new(Mutex::new(0)),
        }
    }

    /// Lines that could not be written.
    pub fn dropped(&self) -> u64 {
        *self.dropped.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The shared sink.
    pub fn sink(&self) -> Arc<Mutex<W>> {
        Arc::clone(&self.sink)
    }
}

impl<W: Write + Send> Applier for LogApplier<W> {
    fn apply(&mut self, change: &Change<'_>) -> Result<(), ApplyError> {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let line = format!(
            "apply key={} value={:?} provisional={}",
            change.key,
            change.value.to_string(),
            change.provisional
        );
        // A log line that cannot be written must not undo the setting.
        if writeln!(sink, "{line}").is_err() {
            let mut dropped = self.dropped.lock().unwrap_or_else(PoisonError::into_inner);
            *dropped = dropped.saturating_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change<'a>(key: &'a str, value: &'a Value) -> Change<'a> {
        Change {
            key,
            template: key,
            instance: None,
            value,
            provisional: false,
        }
    }

    #[test]
    fn mock_records_and_fails_on_request() {
        let mock = MockApplier::new();
        let mut appliers = Appliers::new().with(Domain::Power, Box::new(mock.clone()));
        let value = Value::Int(3);
        assert_eq!(
            appliers.apply(Domain::Power, &change("power.x", &value)),
            Ok(())
        );
        // No applier for the domain: accepted, nothing recorded.
        assert_eq!(
            appliers.apply(Domain::Time, &change("time.x", &value)),
            Ok(())
        );
        assert_eq!(mock.calls().len(), 1);
        assert_eq!(mock.last("power.x"), Some(Value::Int(3)));
        assert_eq!(mock.last("time.x"), None);

        mock.fail_next(ApplyError::Busy);
        assert_eq!(
            appliers.apply(Domain::Power, &change("power.x", &value)),
            Err(ApplyError::Busy)
        );
        assert_eq!(
            appliers.apply(Domain::Power, &change("power.x", &value)),
            Ok(())
        );
        mock.fail_key("power.y", ApplyError::Unsupported);
        for _ in 0..2 {
            assert_eq!(
                appliers.apply(Domain::Power, &change("power.y", &value)),
                Err(ApplyError::Unsupported)
            );
        }
        mock.clear_failures();
        assert_eq!(
            appliers.apply(Domain::Power, &change("power.y", &value)),
            Ok(())
        );
        assert_eq!(mock.total(), 6);
        assert_eq!(mock.calls().len(), 3);
        mock.clear_calls();
        assert!(mock.calls().is_empty());
        assert!(format!("{appliers:?}").contains("Power"));
    }

    #[test]
    fn mock_keeps_a_bounded_history() {
        let mut mock = MockApplier::new();
        let value = Value::Bool(true);
        for _ in 0..MockApplier::CAPACITY + 10 {
            mock.apply(&change("k", &value)).unwrap();
        }
        assert_eq!(mock.calls().len(), MockApplier::CAPACITY);
        assert_eq!(mock.total(), MockApplier::CAPACITY as u64 + 10);
    }

    #[test]
    fn with_all_covers_every_domain() {
        let mock = MockApplier::new();
        let mut appliers = Appliers::new().with_all(&mock);
        let value = Value::Bool(true);
        for domain in Domain::ALL {
            appliers.apply(domain, &change("k", &value)).unwrap();
        }
        assert_eq!(mock.total(), Domain::ALL.len() as u64);
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn log_applier_writes_lines_and_never_fails_the_setting() {
        let mut log = LogApplier::new(Vec::new());
        let value = Value::text("dark");
        log.apply(&change("appearance.theme", &value)).unwrap();
        let provisional = Change {
            provisional: true,
            ..change("display.mode", &value)
        };
        log.clone().apply(&provisional).unwrap();
        let text = String::from_utf8(log.sink().lock().unwrap().clone()).unwrap();
        assert_eq!(
            text,
            "apply key=appearance.theme value=\"dark\" provisional=false\n\
             apply key=display.mode value=\"dark\" provisional=true\n"
        );
        assert_eq!(log.dropped(), 0);

        let mut broken = LogApplier::new(Broken);
        broken.apply(&change("k", &value)).unwrap();
        assert_eq!(broken.dropped(), 1);
        for error in [
            ApplyError::Unsupported,
            ApplyError::Busy,
            ApplyError::Failed,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }
}
