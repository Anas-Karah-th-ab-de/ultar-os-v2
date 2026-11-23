//! The apply journal: which phase of an installation the device is in.
//!
//! One line, replaced atomically: `idle`, `writing <slot>` or `trial <slot> <previous version>`.
//! The journal is a hint for recovery and reporting; safety never depends on it alone (the
//! boot control's slot flags decide what can start). An unreadable journal counts as `idle`.

use crate::bootctl::Slot;
use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use crate::version::Version;
use std::path::Path;

/// Phase of an installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Journal {
    /// Nothing in progress.
    Idle,
    /// The slot is being written; it is unbootable.
    Writing {
        /// Slot being written.
        slot: Slot,
    },
    /// The slot is complete and is (to be) started once.
    Trial {
        /// Slot under trial.
        slot: Slot,
        /// Version that was running when the installation was made.
        previous: Version,
    },
}

impl Journal {
    fn encode(&self) -> String {
        match self {
            Journal::Idle => "idle\n".to_string(),
            Journal::Writing { slot } => format!("writing {}\n", slot.letter()),
            Journal::Trial { slot, previous } => {
                format!("trial {} {previous}\n", slot.letter())
            }
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let mut words = text.strip_suffix('\n')?.split(' ');
        let journal = match words.next()? {
            "idle" => Journal::Idle,
            "writing" => Journal::Writing {
                slot: Slot::from_letter(words.next()?)?,
            },
            "trial" => Journal::Trial {
                slot: Slot::from_letter(words.next()?)?,
                previous: Version::parse(words.next()?)?,
            },
            _ => return None,
        };
        words.next().is_none().then_some(journal)
    }

    /// Reads the journal. The flag is true when a file was there but unreadable (it then
    /// counts as [`Journal::Idle`]); a missing file is a normal idle state.
    pub fn load(path: &Path) -> Result<(Self, bool), UpdateError> {
        match read_bounded(path, 64) {
            Ok(None) => Ok((Journal::Idle, false)),
            Ok(Some(bytes)) => {
                let parsed = std::str::from_utf8(&bytes).ok().and_then(Self::decode);
                Ok((parsed.unwrap_or(Journal::Idle), parsed.is_none()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Ok((Journal::Idle, true)),
            Err(e) => Err(e.into()),
        }
    }

    /// Stores the journal durably.
    pub fn store(&self, path: &Path) -> Result<(), UpdateError> {
        Ok(write_atomic(path, self.encode().as_bytes())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_of_every_phase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal");
        assert_eq!(Journal::load(&path).unwrap(), (Journal::Idle, false));
        for journal in [
            Journal::Idle,
            Journal::Writing { slot: Slot::B },
            Journal::Trial {
                slot: Slot::A,
                previous: Version::new(0, 7, 0),
            },
        ] {
            journal.store(&path).unwrap();
            assert_eq!(Journal::load(&path).unwrap(), (journal, false));
        }
    }

    #[test]
    fn damaged_journal_counts_as_idle_and_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal");
        for bad in [
            &b""[..],
            b"idle",
            b"writing c\n",
            b"writing a b\n",
            b"trial a\n",
            b"trial a 1.2\n",
            b"\xff\n",
            &[b'x'; 200],
        ] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(
                Journal::load(&path).unwrap(),
                (Journal::Idle, true),
                "{bad:?}"
            );
        }
    }
}
