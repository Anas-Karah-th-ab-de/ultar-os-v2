//! Plain data types of the device daemon.

use crate::fsm::Phase;
use crate::traits::StoreError;
use std::fmt;
use ultar_driverpkg::{Candidate, DeviceDescriptor, DriverClass, Version};

/// Identity of a driver: id and version. A new version is a new driver for quarantine purposes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriverKey {
    /// Driver identifier from the manifest.
    pub id: String,
    /// Driver version.
    pub version: Version,
}

impl DriverKey {
    /// Creates a key.
    pub fn new(id: impl Into<String>, version: Version) -> Self {
        Self {
            id: id.into(),
            version,
        }
    }

    /// The key of a candidate.
    pub fn of(candidate: &Candidate) -> Self {
        Self {
            id: candidate.id.clone(),
            version: candidate.version.clone(),
        }
    }
}

impl fmt::Display for DriverKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.id, self.version)
    }
}

/// A device appeared or disappeared (hot-plug, cold-plug at boot, kernel device events).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// A device was found.
    Added(DeviceDescriptor),
    /// The device at this address is gone.
    Removed(String),
}

macro_rules! coded_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident => $code:literal),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vdoc])* $variant),*
        }

        impl $name {
            /// Every value.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// Stable machine code (stored and sent on the bus; translated by the UI).
            pub fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code),*
                }
            }

            /// Parses a machine code.
            pub fn parse(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

coded_enum! {
    /// Why a driver could not be loaded.
    LoadFailure {
        /// The package file is not on this system (not copied at install time). Not a driver fault.
        PackageMissing => "package-missing",
        /// The package failed signature or integrity verification.
        PackageRejected => "package-rejected",
        /// The module does not fit the running kernel.
        IncompatibleKernel => "incompatible-kernel",
        /// The driver's initialisation reported an error.
        InitFailed => "init-failed",
        /// Loading did not finish in time.
        Timeout => "timeout",
    }
}

coded_enum! {
    /// Why a loaded driver failed its health check.
    HealthFailure {
        /// The device does not answer through the driver.
        NoResponse => "no-response",
        /// The driver's self test failed.
        SelfTestFailed => "self-test-failed",
        /// The check did not finish in time.
        Timeout => "timeout",
        /// The device works only intermittently.
        Unstable => "unstable",
    }
}

/// Why a driver was put into quarantine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Loading failed.
    Load(LoadFailure),
    /// The health check failed.
    Health(HealthFailure),
}

impl FailureReason {
    /// Stable machine code such as `load.init-failed` or `health.no-response`.
    pub fn code(self) -> String {
        match self {
            Self::Load(failure) => format!("load.{}", failure.code()),
            Self::Health(failure) => format!("health.{}", failure.code()),
        }
    }

    /// Parses a machine code produced by [`FailureReason::code`].
    pub fn parse(code: &str) -> Option<Self> {
        let (kind, detail) = code.split_once('.')?;
        match kind {
            "load" => LoadFailure::parse(detail).map(Self::Load),
            "health" => HealthFailure::parse(detail).map(Self::Health),
            _ => None,
        }
    }
}

/// A driver that must not be tried automatically on a device, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineEntry {
    /// [`DeviceDescriptor::identity`] of the device the driver failed on.
    pub device: String,
    /// The failed driver.
    pub driver: DriverKey,
    /// Why it failed.
    pub reason: FailureReason,
}

/// A capability a working device provides, e.g. `display.count = 2` or `modem = present`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Capability {
    /// Capability name.
    pub name: String,
    /// Capability value.
    pub value: String,
}

impl Capability {
    /// Creates a capability.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Whether capabilities appeared or went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A driver became active.
    Gained,
    /// The active driver was unloaded or its device removed.
    Lost,
}

/// A capability announcement for the system bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityChange {
    /// Address of the device.
    pub device: String,
    /// The driver providing the capabilities.
    pub driver: DriverKey,
    /// Class of that driver.
    pub class: DriverClass,
    /// Gained or lost.
    pub kind: ChangeKind,
    /// The capabilities concerned.
    pub capabilities: Vec<Capability>,
}

/// How one load attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// Loaded and healthy.
    Worked,
    /// Failed; the driver was quarantined with this reason.
    Failed(FailureReason),
    /// Not tried further because its package is not on this system (no quarantine).
    Skipped,
    /// The device disappeared during the attempt (no quarantine).
    DeviceGone,
}

/// One load attempt during a probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// The driver that was tried.
    pub driver: DriverKey,
    /// How it ended.
    pub outcome: AttemptOutcome,
}

/// What happened while handling an event or a manual operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Address of the device.
    pub address: String,
    /// Every phase the device entered, in order.
    pub transitions: Vec<Phase>,
    /// Every load attempt, in order.
    pub attempts: Vec<Attempt>,
    /// The driver active at the end, if any.
    pub active: Option<DriverKey>,
    /// `true` when the remembered driver was loaded directly and works.
    pub used_remembered: bool,
    /// `true` when a remembered driver existed but no longer works (the store was corrected).
    pub remembered_failed: bool,
    /// `true` when a manual switch failed and the previous driver was restored.
    pub restored: bool,
    /// Set when the decisions could not be persisted (they still apply until shutdown).
    pub store_error: Option<StoreError>,
}

impl ProbeReport {
    pub(crate) fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
            transitions: Vec::new(),
            attempts: Vec::new(),
            active: None,
            used_remembered: false,
            remembered_failed: false,
            restored: false,
            store_error: None,
        }
    }

    /// The phase the device ended in, `None` when nothing changed.
    pub fn final_phase(&self) -> Option<Phase> {
        self.transitions.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for failure in LoadFailure::ALL {
            assert_eq!(LoadFailure::parse(failure.code()), Some(*failure));
            let reason = FailureReason::Load(*failure);
            assert_eq!(FailureReason::parse(&reason.code()), Some(reason));
        }
        for failure in HealthFailure::ALL {
            assert_eq!(HealthFailure::parse(failure.code()), Some(*failure));
            let reason = FailureReason::Health(*failure);
            assert_eq!(FailureReason::parse(&reason.code()), Some(reason));
        }
        assert_eq!(
            FailureReason::Health(HealthFailure::NoResponse).code(),
            "health.no-response"
        );
        assert_eq!(
            FailureReason::Load(LoadFailure::Timeout).code(),
            "load.timeout"
        );
        // "timeout" exists in both families and stays distinguishable.
        assert_ne!(
            FailureReason::parse("load.timeout"),
            FailureReason::parse("health.timeout")
        );
        for bad in [
            "",
            "timeout",
            "load.",
            "load.nope",
            "other.timeout",
            "health.init-failed",
        ] {
            assert_eq!(FailureReason::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn small_helpers() {
        let key = DriverKey::new("gen-xhci", Version::new(1, 2, 0));
        assert_eq!(key.to_string(), "gen-xhci 1.2.0");
        assert!(key < DriverKey::new("gen-xhci", Version::new(1, 10, 0)));
        assert_eq!(Capability::new("modem", "present").name, "modem");
        let mut report = ProbeReport::new("pci:0");
        assert_eq!(report.final_phase(), None);
        report.transitions.push(Phase::Detected);
        report.transitions.push(Phase::NoDriver);
        assert_eq!(report.final_phase(), Some(Phase::NoDriver));
        assert_eq!(report.address, "pci:0");
    }
}
