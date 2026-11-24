//! Call log and blocked numbers of the call service, both bounded
//! ([`MAX_LOG`] entries, oldest dropped; [`MAX_BLOCKED`] numbers).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::VecDeque;
use ultar_bus::messages::call::{
    check_number, CallError, CallInfo, Direction, End, LogEntry, Phase, MAX_BLOCKED, MAX_LOG,
};

/// Log of finished calls, newest last inside.
#[derive(Debug, Default)]
pub struct CallLog {
    entries: VecDeque<LogEntry>,
    next_id: u32,
    blocked: Vec<String>,
    store: Option<std::path::PathBuf>,
    store_errors: u32,
}

/// Numbers compare without spaces and dashes.
fn canonical(number: &str) -> String {
    number.chars().filter(|c| !matches!(c, ' ' | '-')).collect()
}

impl CallLog {
    /// A log kept in the file at `path` (see [`crate::logstore`]); what the file holds is read
    /// now. Returns the log and the number of lines that could not be read (a corrupt file
    /// counts as one and is replaced on the next save).
    pub fn open(path: &std::path::Path) -> (CallLog, usize) {
        let (stored, bad) = match crate::logstore::load(path) {
            Ok(s) => {
                let bad = s.bad_lines;
                (s, bad)
            }
            Err(_) => (crate::logstore::Stored::default(), 1),
        };
        let next_id = stored.entries.iter().map(|e| e.id).max().unwrap_or(0);
        let log = CallLog {
            entries: stored.entries.into(),
            next_id,
            blocked: stored.blocked,
            store: Some(path.to_path_buf()),
            store_errors: 0,
        };
        (log, bad)
    }

    /// Writes the log to its file (if it has one); a failure is counted, the log stays.
    fn save(&mut self) {
        if let Some(path) = &self.store {
            let entries: Vec<LogEntry> = self.entries.iter().cloned().collect();
            if crate::logstore::save(path, &entries, &self.blocked).is_err() {
                self.store_errors = self.store_errors.saturating_add(1);
            }
        }
    }

    /// Saves that failed since start.
    pub fn store_errors(&self) -> u32 {
        self.store_errors
    }

    /// Deletes every entry (blocked numbers stay).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    /// Records a finished call that started at `when_s`.
    pub fn record(&mut self, call: &CallInfo, when_s: i64) {
        let missed = matches!(call.phase, Phase::Ended(End::Missed));
        if matches!(call.phase, Phase::Ended(End::Blocked)) {
            return;
        }
        if self.entries.len() == MAX_LOG {
            self.entries.pop_front();
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.entries.push_back(LogEntry {
            id: self.next_id,
            way: call.way,
            direction: call.direction,
            missed: missed && call.direction == Direction::Incoming,
            number: call.number.clone(),
            name: call.name.clone(),
            when_s,
        });
        self.save();
    }

    /// Entries, newest first.
    pub fn list(&self, missed_only: bool) -> Vec<LogEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| !missed_only || e.missed)
            .cloned()
            .collect()
    }

    /// Deletes one entry.
    pub fn forget(&mut self, id: u32) -> Result<(), CallError> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == before {
            return Err(CallError::UnknownCall);
        }
        self.save();
        Ok(())
    }

    /// Blocks or unblocks a number.
    pub fn block(&mut self, number: &str, on: bool) -> Result<(), CallError> {
        check_number(number)?;
        let key = canonical(number);
        let known = self.blocked.contains(&key);
        if on && !known {
            if self.blocked.len() >= MAX_BLOCKED {
                return Err(CallError::Full);
            }
            self.blocked.push(key);
        } else if !on {
            self.blocked.retain(|n| *n != key);
        }
        self.save();
        Ok(())
    }

    /// True when calls from this number are refused.
    pub fn is_blocked(&self, number: &str) -> bool {
        self.blocked.contains(&canonical(number))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_log_survives_a_restart_and_can_be_cleared() {
        let dir = std::env::temp_dir().join(format!("calllog-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("calls.log");
        let (mut log, bad) = CallLog::open(&path);
        assert_eq!(bad, 0);
        log.record(&call(Direction::Incoming, End::Missed), 5);
        log.block("0955", true).unwrap();
        let (mut again, _) = CallLog::open(&path);
        assert_eq!(again.list(false).len(), 1);
        assert!(again.is_blocked("0955"));
        again.record(&call(Direction::Outgoing, End::Local), 6);
        assert_eq!(
            again.list(false)[0].id,
            2,
            "identifiers continue after a restart"
        );
        again.clear();
        assert!(CallLog::open(&path).0.list(false).is_empty());
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(CallLog::open(&path).1, 1, "a corrupt file is reported");
        assert_eq!(again.store_errors(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::*;
    use ultar_bus::messages::call::{Media, Way};

    fn call(direction: Direction, end: End) -> CallInfo {
        CallInfo {
            id: 1,
            way: Way::Ultar,
            direction,
            number: "UOS-1".into(),
            name: None,
            phase: Phase::Ended(end),
            media: Media::default(),
            encrypted: true,
            active_since_ms: None,
        }
    }

    #[test]
    fn log_is_bounded_newest_first_and_filters_missed() {
        let mut log = CallLog::default();
        for i in 0..(MAX_LOG + 5) {
            log.record(&call(Direction::Outgoing, End::Local), i as i64);
        }
        log.record(&call(Direction::Incoming, End::Missed), 9999);
        log.record(&call(Direction::Incoming, End::Blocked), 10000);
        let all = log.list(false);
        assert_eq!(all.len(), MAX_LOG);
        assert_eq!(all[0].when_s, 9999);
        assert!(all[0].missed);
        assert_eq!(log.list(true).len(), 1);
        let id = all[1].id;
        log.forget(id).unwrap();
        assert_eq!(log.forget(id), Err(CallError::UnknownCall));
    }

    #[test]
    fn blocking_is_canonical_bounded_and_checked() {
        let mut log = CallLog::default();
        log.block("+963 11-555", true).unwrap();
        assert!(log.is_blocked("+96311555"));
        log.block("+96311555", false).unwrap();
        assert!(!log.is_blocked("+963 11-555"));
        assert_eq!(log.block("x", true), Err(CallError::BadNumber));
        for i in 0..MAX_BLOCKED {
            log.block(&format!("{}", 1000 + i), true).unwrap();
        }
        log.block("1000", true).unwrap();
        assert_eq!(log.block("99999", true), Err(CallError::Full));
    }
}
