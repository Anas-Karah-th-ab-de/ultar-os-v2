//! The service: keys in memory, one request per connection on a local Unix socket.

use crate::audit::{AuditLog, Record};
use crate::error::SignError;
use crate::keys::{self, Role};
use crate::ratelimit::{Limit, RateLimiter};
use crate::request::{Request, MAX_LINE, RELEASE_NAMESPACE};
use crate::sshsig;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signer as _, SigningKey};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

/// Read/write timeout per connection (a slow client cannot hold the service).
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// The signing state: loaded keys, rate limiter, audit log.
pub struct Signer {
    keys: [Option<SigningKey>; 3],
    limiter: RateLimiter,
    audit: AuditLog,
}

fn slot(role: Role) -> usize {
    match role {
        Role::Os => 0,
        Role::System => 1,
        Role::Store => 2,
    }
}

impl Signer {
    /// Loads every key present in `key_dir`. A key with wrong permissions or damaged contents
    /// stops the start; a missing key only disables its role.
    pub fn load(
        key_dir: &Path,
        audit: AuditLog,
        limits: [Limit; 3],
        now: u64,
    ) -> Result<Signer, SignError> {
        let mut keys = [None, None, None];
        for role in Role::ALL {
            match keys::load(key_dir, role) {
                Ok(k) => keys[slot(role)] = Some(k),
                Err(SignError::KeyMissing) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Self::with_keys(keys, audit, limits, now))
    }

    /// A signer with the given keys (tests and [`Signer::load`]).
    pub fn with_keys(
        keys: [Option<SigningKey>; 3],
        audit: AuditLog,
        limits: [Limit; 3],
        now: u64,
    ) -> Signer {
        Signer {
            keys,
            limiter: RateLimiter::new(limits, now),
            audit,
        }
    }

    /// Roles whose key is loaded.
    pub fn roles(&self) -> Vec<Role> {
        Role::ALL
            .into_iter()
            .filter(|r| self.keys[slot(*r)].is_some())
            .collect()
    }

    /// Answers one request line from caller `uid` at time `now` (always one line, with `\n`).
    pub fn handle(&mut self, line: &str, uid: Option<u32>, now: u64) -> String {
        match self.answer(line, uid, now) {
            Ok(text) => format!("ok {text}\n"),
            Err(e) => format!("err {}\n", e.code()),
        }
    }

    fn answer(&mut self, line: &str, uid: Option<u32>, now: u64) -> Result<String, SignError> {
        let req = match Request::parse(line) {
            Ok(r) => r,
            Err(e) => {
                self.record(now, uid, "invalid", "-", "-", "-", e.code())?;
                return Err(e);
            }
        };
        let role = req.role();
        let Some(key) = self.keys[slot(role)].clone() else {
            self.record(
                now,
                uid,
                req.op(),
                role.name(),
                "-",
                &req.digest(),
                "key_missing",
            )?;
            return Err(SignError::KeyMissing);
        };
        let public = key.verifying_key();
        let kid = keys::key_id(&public);
        let Some(message) = req.message() else {
            return Ok(hex::encode(public.as_bytes()));
        };
        if !self.limiter.take(role, now) {
            self.record(
                now,
                uid,
                req.op(),
                role.name(),
                &kid,
                &req.digest(),
                "rate_limited",
            )?;
            return Err(SignError::RateLimited);
        }
        // Audit first: no line, no signature.
        self.record(now, uid, req.op(), role.name(), &kid, &req.digest(), "ok")?;
        let sig = key.sign(&message);
        Ok(match req {
            Request::Release { .. } => format!(
                "{kid} {}",
                B64.encode(sshsig::armor(&public, RELEASE_NAMESPACE, &sig))
            ),
            _ => format!("{kid} {}", B64.encode(sig.to_bytes())),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        time: u64,
        uid: Option<u32>,
        op: &str,
        role: &str,
        key_id: &str,
        digest: &str,
        result: &str,
    ) -> Result<(), SignError> {
        self.audit.write(&Record {
            time,
            uid,
            op,
            role,
            key_id,
            digest,
            result,
        })
    }
}

/// Binds the socket at `path` (a stale socket is replaced, any other file is refused) with
/// mode `0660`: only the service user and the publish group can connect.
pub fn bind(path: &Path) -> Result<UnixListener, SignError> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() {
            return Err(SignError::Io);
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    Ok(listener)
}

/// Reads one request line (at most [`MAX_LINE`] bytes) from a connection.
pub fn read_line(stream: &mut impl Read) -> Result<String, SignError> {
    let mut buf = Vec::with_capacity(160);
    let mut byte = [0u8; 1];
    while buf.len() <= MAX_LINE {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        if byte[0] == b'\n' {
            return String::from_utf8(buf).map_err(|_| SignError::BadRequest);
        }
        buf.push(byte[0]);
    }
    Err(SignError::BadRequest)
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .map(|c| c.uid.as_raw())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Serves one connection: one line in, one line out.
pub fn serve_one(signer: &mut Signer, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let uid = peer_uid(&stream);
    let answer = match read_line(&mut stream) {
        Ok(line) => signer.handle(&line, uid, unix_now()),
        Err(e) => format!("err {}\n", e.code()),
    };
    let _ = stream.write_all(answer.as_bytes());
}

/// Accepts connections one after another, for ever. Refuses to run as root.
pub fn run(listener: UnixListener, mut signer: Signer) -> Result<(), SignError> {
    if rustix::process::geteuid().is_root() {
        return Err(SignError::RunningAsRoot);
    }
    for stream in listener.incoming() {
        match stream {
            Ok(s) => serve_one(&mut signer, s),
            Err(_) => continue,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};

    const H64: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn signer(dir: &Path) -> Signer {
        let k = |b: u8| Some(SigningKey::from_bytes(&[b; 32]));
        Signer::with_keys(
            [k(1), None, k(3)],
            AuditLog::new(dir.join("audit.log")),
            Role::ALL.map(Limit::default_for),
            0,
        )
    }

    #[test]
    fn signs_store_requests_with_the_store_key_and_audits() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = signer(dir.path());
        assert_eq!(s.roles(), vec![Role::Os, Role::Store]);
        let ans = s.handle(&format!("catalog {H64}\n"), Some(42), 7);
        let words: Vec<&str> = ans.trim().split(' ').collect();
        assert_eq!(words[0], "ok");
        let store = SigningKey::from_bytes(&[3; 32]).verifying_key();
        assert_eq!(words[1], keys::key_id(&store));
        let sig = B64.decode(words[2]).unwrap();
        let sig = Signature::from_bytes(&sig.try_into().unwrap());
        let msg = format!("ultar-catalog-v1\n{H64}\n");
        assert!(store.verify(msg.as_bytes(), &sig).is_ok());
        let log = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
        assert!(log.contains("uid=42 op=catalog key=store:"));
        assert!(log.ends_with("result=ok\n"));
    }

    #[test]
    fn missing_key_bad_request_and_rate_limit_are_refused_and_audited() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = signer(dir.path());
        assert_eq!(
            s.handle(&format!("pkg-system {H64} {H64}"), None, 0),
            "err key_missing\n"
        );
        assert_eq!(s.handle("sign everything", None, 0), "err bad_request\n");
        let release = format!("release {H64}{H64}");
        for _ in 0..10 {
            assert!(s.handle(&release, None, 0).starts_with("ok "));
        }
        assert_eq!(s.handle(&release, None, 0), "err rate_limited\n");
        let log = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
        assert_eq!(log.lines().count(), 13);
        assert!(log.contains("result=key_missing"));
        assert!(log.contains("op=invalid"));
        assert!(log.contains("result=rate_limited"));
    }

    #[test]
    fn no_audit_no_signature() {
        let mut s = Signer::with_keys(
            [None, None, Some(SigningKey::from_bytes(&[3; 32]))],
            AuditLog::new("/nonexistent/audit.log"),
            Role::ALL.map(Limit::default_for),
            0,
        );
        assert_eq!(
            s.handle(&format!("catalog {H64}"), None, 0),
            "err audit_failed\n"
        );
    }

    #[test]
    fn pubkey_answers_without_signing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = signer(dir.path());
        let os = SigningKey::from_bytes(&[1; 32]).verifying_key();
        assert_eq!(
            s.handle("pubkey os", None, 0),
            format!("ok {}\n", hex::encode(os.as_bytes()))
        );
    }

    #[test]
    fn read_line_limits_length() {
        let mut ok = std::io::Cursor::new(b"pubkey os\nrest".to_vec());
        assert_eq!(read_line(&mut ok).unwrap(), "pubkey os");
        let mut long = std::io::Cursor::new(vec![b'a'; 2000]);
        assert_eq!(read_line(&mut long), Err(SignError::BadRequest));
        let mut eof = std::io::Cursor::new(b"no newline".to_vec());
        assert_eq!(read_line(&mut eof), Err(SignError::BadRequest));
    }

    #[test]
    fn bind_refuses_regular_files_and_sets_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain");
        std::fs::write(&file, "x").unwrap();
        assert!(bind(&file).is_err());
        let sock = dir.path().join("s.sock");
        drop(bind(&sock).unwrap());
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o660);
        // A stale socket is replaced.
        assert!(bind(&sock).is_ok());
    }
}
