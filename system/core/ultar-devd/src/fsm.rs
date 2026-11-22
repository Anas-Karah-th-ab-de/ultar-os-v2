//! The explicit per-device state machine.
//!
//! ```text
//!                 CandidatesFound            ProbeSucceeded
//!   Detected ───────────────────▶ Probing ─────────────────▶ Active
//!      │  ▲                          │  ▲                      │
//!      │  │ Rescan                   │  └── SwitchRequested ───┘
//!      │  ├──────────── Failed ◀─────┘ CandidatesExhausted
//!      │  │
//!      │  └──────────── NoDriver
//!      └───────────────────▲ NoCandidates
//!
//!   any state except Removed ── Unplugged ──▶ Removed (terminal)
//! ```

use std::fmt;

/// The life-cycle phase of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// The device was seen; no driver decision yet.
    Detected,
    /// A driver is being loaded and checked.
    Probing,
    /// A driver is loaded and passed its health check.
    Active,
    /// The depot has no driver for this device.
    NoDriver,
    /// Drivers exist but none of them works.
    Failed,
    /// The device is gone.
    Removed,
}

impl Phase {
    /// Every phase.
    pub const ALL: [Self; 6] = [
        Self::Detected,
        Self::Probing,
        Self::Active,
        Self::NoDriver,
        Self::Failed,
        Self::Removed,
    ];

    /// Stable machine name of the phase (for the bus and logs).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Probing => "probing",
            Self::Active => "active",
            Self::NoDriver => "no-driver",
            Self::Failed => "failed",
            Self::Removed => "removed",
        }
    }
}

/// What happened to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// At least one candidate driver exists; probing starts.
    CandidatesFound,
    /// The depot has no candidate.
    NoCandidates,
    /// A candidate was loaded and is healthy.
    ProbeSucceeded,
    /// Every candidate was tried without success.
    CandidatesExhausted,
    /// The device disappeared.
    Unplugged,
    /// A new search was requested (new depot index, cleared quarantine, manual request).
    Rescan,
    /// The user asked for a different driver.
    SwitchRequested,
}

impl Signal {
    /// Every signal.
    pub const ALL: [Self; 7] = [
        Self::CandidatesFound,
        Self::NoCandidates,
        Self::ProbeSucceeded,
        Self::CandidatesExhausted,
        Self::Unplugged,
        Self::Rescan,
        Self::SwitchRequested,
    ];
}

/// A signal that is not allowed in the current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    /// The phase the device was in.
    pub from: Phase,
    /// The signal that does not apply there.
    pub signal: Signal,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "devd.invalid-transition: {} + {:?}",
            self.from.as_str(),
            self.signal
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// The transition function: the next phase, or an error when the signal does not apply.
pub fn transition(from: Phase, signal: Signal) -> Result<Phase, InvalidTransition> {
    use Phase::{Active, Detected, Failed, NoDriver, Probing, Removed};
    match (from, signal) {
        (Detected, Signal::CandidatesFound) => Ok(Probing),
        (Detected, Signal::NoCandidates) => Ok(NoDriver),
        (Probing, Signal::ProbeSucceeded) => Ok(Active),
        (Probing, Signal::CandidatesExhausted) => Ok(Failed),
        (Active, Signal::SwitchRequested) => Ok(Probing),
        (NoDriver | Failed, Signal::Rescan) => Ok(Detected),
        (Detected | Probing | Active | NoDriver | Failed, Signal::Unplugged) => Ok(Removed),
        _ => Err(InvalidTransition { from, signal }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::{Active, Detected, Failed, NoDriver, Probing, Removed};

    /// The complete table: every allowed (phase, signal) pair with its target.
    const ALLOWED: [(Phase, Signal, Phase); 12] = [
        (Detected, Signal::CandidatesFound, Probing),
        (Detected, Signal::NoCandidates, NoDriver),
        (Detected, Signal::Unplugged, Removed),
        (Probing, Signal::ProbeSucceeded, Active),
        (Probing, Signal::CandidatesExhausted, Failed),
        (Probing, Signal::Unplugged, Removed),
        (Active, Signal::SwitchRequested, Probing),
        (Active, Signal::Unplugged, Removed),
        (NoDriver, Signal::Rescan, Detected),
        (NoDriver, Signal::Unplugged, Removed),
        (Failed, Signal::Rescan, Detected),
        (Failed, Signal::Unplugged, Removed),
    ];

    #[test]
    fn every_pair_is_decided_exactly_as_in_the_table() {
        let mut allowed = 0;
        for from in Phase::ALL {
            for signal in Signal::ALL {
                let expected = ALLOWED
                    .iter()
                    .find(|(f, s, _)| *f == from && *s == signal)
                    .map(|(_, _, to)| *to);
                match expected {
                    Some(to) => {
                        allowed += 1;
                        assert_eq!(transition(from, signal), Ok(to), "{from:?} + {signal:?}");
                    }
                    None => {
                        assert_eq!(
                            transition(from, signal),
                            Err(InvalidTransition { from, signal })
                        );
                    }
                }
            }
        }
        assert_eq!(allowed, ALLOWED.len());
        assert_eq!(Phase::ALL.len() * Signal::ALL.len(), 42);
    }

    #[test]
    fn removed_is_terminal_and_every_phase_can_be_unplugged() {
        for signal in Signal::ALL {
            assert!(transition(Removed, signal).is_err());
        }
        for from in Phase::ALL.into_iter().filter(|p| *p != Removed) {
            assert_eq!(transition(from, Signal::Unplugged), Ok(Removed));
        }
    }

    #[test]
    fn every_phase_is_reachable_from_detected() {
        let mut reached = vec![Detected];
        let mut grew = true;
        while grew {
            grew = false;
            for from in reached.clone() {
                for signal in Signal::ALL {
                    if let Ok(to) = transition(from, signal) {
                        if !reached.contains(&to) {
                            reached.push(to);
                            grew = true;
                        }
                    }
                }
            }
        }
        reached.sort();
        assert_eq!(reached, Phase::ALL);
    }

    #[test]
    fn names_and_errors() {
        let names: Vec<&str> = Phase::ALL.iter().map(|p| p.as_str()).collect();
        assert_eq!(
            names,
            [
                "detected",
                "probing",
                "active",
                "no-driver",
                "failed",
                "removed"
            ]
        );
        let err = transition(Active, Signal::Rescan).unwrap_err();
        assert_eq!(err.to_string(), "devd.invalid-transition: active + Rescan");
    }
}
