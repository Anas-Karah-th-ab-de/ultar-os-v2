//! Append-only audit log of every request: time, caller uid, operation, key id, digest, result.
//!
//! Written **before** the signature is handed out; if the line cannot be written, the request
//! fails (`audit_failed`) and nothing is signed. The file is rotated once it exceeds
//! [`MAX_LOG_BYTES`] (one previous file `<name>.1` is kept), so it cannot fill the disk.

use crate::error::SignError;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Size at which the log is rotated.
pub const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;

/// One audit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<'a> {
    /// Unix time in seconds.
    pub time: u64,
    /// Caller's uid from the socket credentials (`None` when unknown).
    pub uid: Option<u32>,
    /// Operation (`pkg-store`, `release`, …) or `invalid`.
    pub op: &'a str,
    /// Key role name.
    pub role: &'a str,
    /// Key id or `-`.
    pub key_id: &'a str,
    /// Digest(s) or `-`.
    pub digest: &'a str,
    /// `ok` or an error code.
    pub result: &'a str,
}

impl Record<'_> {
    /// The single line written for this record (fields never contain spaces or newlines:
    /// every field is a fixed word, a hex digest or an error code).
    pub fn line(&self) -> String {
        let clean = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_ascii_graphic() { c } else { '?' })
                .take(300)
                .collect()
        };
        format!(
            "{} uid={} op={} key={}:{} digest={} result={}\n",
            self.time,
            self.uid.map_or_else(|| "-".to_string(), |u| u.to_string()),
            clean(self.op),
            clean(self.role),
            clean(self.key_id),
            clean(self.digest),
            clean(self.result)
        )
    }
}

/// The audit log file.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
    max: u64,
}

impl AuditLog {
    /// A log at `path` with the default rotation size.
    pub fn new(path: impl Into<PathBuf>) -> AuditLog {
        AuditLog {
            path: path.into(),
            max: MAX_LOG_BYTES,
        }
    }

    /// Same with another rotation size (tests).
    pub fn with_max(mut self, max: u64) -> AuditLog {
        self.max = max;
        self
    }

    /// Where the log lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one record and syncs it to disk.
    pub fn write(&self, record: &Record<'_>) -> Result<(), SignError> {
        use std::os::unix::fs::OpenOptionsExt;
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() >= self.max) {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            std::fs::rename(&self.path, old).map_err(|_| SignError::AuditFailed)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(|_| SignError::AuditFailed)?;
        file.write_all(record.line().as_bytes())
            .and_then(|()| file.sync_data())
            .map_err(|_| SignError::AuditFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(result: &str) -> Record<'_> {
        Record {
            time: 5,
            uid: Some(1000),
            op: "catalog",
            role: "store",
            key_id: "abcd",
            digest: "ff",
            result,
        }
    }

    #[test]
    fn appends_lines_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.log")).with_max(100);
        log.write(&rec("ok")).unwrap();
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(
            text,
            "5 uid=1000 op=catalog key=store:abcd digest=ff result=ok\n"
        );
        log.write(&rec("rate_limited")).unwrap();
        log.write(&rec("ok")).unwrap(); // > 100 bytes: rotated before this line
        assert!(dir.path().join("audit.log.1").exists());
        assert_eq!(
            std::fs::read_to_string(log.path()).unwrap().lines().count(),
            1
        );
    }

    #[test]
    fn hostile_fields_cannot_forge_lines() {
        let r = Record {
            op: "x\n0 uid=0 op=release",
            uid: None,
            ..rec("ok")
        };
        let line = r.line();
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.contains("uid=-"));
    }

    #[test]
    fn unwritable_log_fails_closed() {
        let log = AuditLog::new("/nonexistent-dir/audit.log");
        assert_eq!(log.write(&rec("ok")), Err(SignError::AuditFailed));
    }
}
