//! Who is on the other end of a Unix socket: kernel credentials
//! (`SO_PEERCRED`) and the file that maps user ids to identities.
//!
//! [`KernelCredentials`] is the real [`ultar_bus::CredentialSource`] the
//! bus was waiting for. With a [`UidMap`] it becomes a
//! [`ultar_bus::CredentialIdentity`]: a connecting process is the identity
//! its **user id** is mapped to, and a user id that is not in the map is
//! refused (default deny). The process cannot influence the answer; the
//! kernel recorded its credentials at `connect`.
//!
//! # Kernel support
//!
//! * Linux: supported.
//! * Ultar kernel (state 23.11.2026): **not supported.**
//!   `sys_getsockopt` (`kernel/ultar-kern/src/syscall.rs`) knows `SO_ERROR`
//!   and `SO_TYPE` only and answers every other option with four zero
//!   bytes; the kernel also has no user ids (`getuid` is always 0).
//!   [`peer_credentials`] recognises that answer and fails, so a listener
//!   that uses credentials refuses every peer there (fail closed), and
//!   [`probe`] lets a daemon find out at start. Needed kernel change:
//!   record `{pid, uid, gid}` of the connecting thread group in the Unix
//!   connection at `connect`/`socketpair` and return it as the 12-byte
//!   `struct ucred` for `getsockopt(SOL_SOCKET, SO_PEERCRED)`; before that
//!   is useful the kernel needs an identity per cell (a user id or a cell
//!   id). Until then the bus identities of the Ultar kernel are the fixed
//!   identity per socket (one socket per cell, mounted only into that cell)
//!   and the token identity.

use std::collections::BTreeMap;
use std::fmt;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use ultar_bus::{BusError, CredentialIdentity, CredentialSource, Identity, PeerCredentials};
use ultar_sys as sys;

/// Most entries of a uid map.
pub const MAX_MAP_ENTRIES: usize = 1024;
/// Largest uid map file in bytes.
pub const MAX_MAP_BYTES: u64 = 64 * 1024;

/// Reads the kernel credentials of the peer of `stream`.
pub fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials, PeerCredError> {
    sys::peer_credentials(stream.as_fd())
        .map(|cred| PeerCredentials {
            uid: cred.uid,
            gid: cred.gid,
            pid: Some(cred.pid),
        })
        .map_err(|_| PeerCredError::Unsupported)
}

/// Whether the running kernel reports peer credentials: a socket pair of
/// this process must report this process.
pub fn probe() -> bool {
    let Ok((left, _right)) = UnixStream::pair() else {
        return false;
    };
    peer_credentials(&left)
        .is_ok_and(|cred| cred.pid == Some(std::process::id()) && cred.uid == sys::euid())
}

/// The kernel as source of peer credentials.
#[derive(Clone, Copy, Debug, Default)]
pub struct KernelCredentials;

impl CredentialSource for KernelCredentials {
    fn credentials(&self, stream: &UnixStream) -> Result<PeerCredentials, BusError> {
        peer_credentials(stream).map_err(|_| BusError::AuthFailed)
    }
}

/// Why credentials or a uid map could not be used.
#[derive(Debug)]
pub enum PeerCredError {
    /// The kernel does not report peer credentials for this socket.
    Unsupported,
    /// The map file could not be read.
    Io(std::io::ErrorKind),
    /// The map file is writable by group or others, or owned by a stranger.
    UnsafeFile,
    /// The map file is larger than [`MAX_MAP_BYTES`].
    TooLarge,
    /// A line is not `UID = "identity"`.
    Syntax {
        /// 1-based line number.
        line: usize,
    },
    /// A user id appears twice.
    Duplicate {
        /// 1-based line number of the second appearance.
        line: usize,
    },
    /// More than [`MAX_MAP_ENTRIES`] entries.
    TooManyEntries,
}

impl fmt::Display for PeerCredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("kernel reports no peer credentials"),
            Self::Io(kind) => write!(f, "uid map unreadable: {kind}"),
            Self::UnsafeFile => f.write_str("uid map is writable by others or has a foreign owner"),
            Self::TooLarge => f.write_str("uid map too large"),
            Self::Syntax { line } => write!(f, "uid map line {line}: expected UID = \"identity\""),
            Self::Duplicate { line } => write!(f, "uid map line {line}: user id mapped twice"),
            Self::TooManyEntries => f.write_str("uid map has too many entries"),
        }
    }
}

impl std::error::Error for PeerCredError {}

/// Maps user ids to identities. A user id without entry has no identity.
///
/// File format (a TOML subset, one entry per line, `#` starts a comment):
///
/// ```toml
/// 0    = "system"
/// 1001 = "system.settingsd"
/// 2001 = "app.phone"
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UidMap {
    entries: BTreeMap<u32, Identity>,
}

fn parse_line(line: &str) -> Option<Option<(u32, Identity)>> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return Some(None);
    }
    let (uid, name) = line.split_once('=')?;
    let uid = uid.trim();
    if uid.is_empty() || uid.len() > 10 || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let name = name.trim().strip_prefix('"')?.strip_suffix('"')?;
    let identity = Identity::new(name).ok()?;
    Some(Some((uid.parse().ok()?, identity)))
}

impl UidMap {
    /// A map without entries: everybody is refused.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses the text of a map file. Any faulty line refuses the whole
    /// file: a half-read map would silently lock out or mis-name peers.
    pub fn parse(text: &str) -> Result<Self, PeerCredError> {
        let mut entries = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            let line_number = index + 1;
            let Some(entry) = parse_line(line) else {
                return Err(PeerCredError::Syntax { line: line_number });
            };
            let Some((uid, identity)) = entry else {
                continue;
            };
            if entries.len() >= MAX_MAP_ENTRIES {
                return Err(PeerCredError::TooManyEntries);
            }
            if entries.insert(uid, identity).is_some() {
                return Err(PeerCredError::Duplicate { line: line_number });
            }
        }
        Ok(Self { entries })
    }

    /// Reads a map file. The file must belong to root or to the calling
    /// user and must not be writable by group or others – whoever can
    /// write it can become any identity.
    pub fn load(path: &Path) -> Result<Self, PeerCredError> {
        let io = |error: std::io::Error| PeerCredError::Io(error.kind());
        let file = std::fs::File::open(path).map_err(io)?;
        let meta = file.metadata().map_err(io)?;
        if !meta.is_file() || (meta.uid() != 0 && meta.uid() != sys::euid()) {
            return Err(PeerCredError::UnsafeFile);
        }
        if meta.mode() & 0o022 != 0 {
            return Err(PeerCredError::UnsafeFile);
        }
        if meta.len() > MAX_MAP_BYTES {
            return Err(PeerCredError::TooLarge);
        }
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, MAX_MAP_BYTES + 1), &mut text)
            .map_err(io)?;
        if text.len() as u64 > MAX_MAP_BYTES {
            return Err(PeerCredError::TooLarge);
        }
        Self::parse(&text)
    }

    /// Adds or replaces one entry.
    pub fn with(mut self, uid: u32, identity: Identity) -> Self {
        self.entries.insert(uid, identity);
        self
    }

    /// The identity of `uid`, if it has one.
    pub fn identity(&self, uid: u32) -> Option<&Identity> {
        self.entries.get(&uid)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map refuses everybody.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bus authenticator for this map: kernel credentials, default deny.
    pub fn into_peer_identity(self) -> CredentialIdentity<KernelCredentials> {
        let mut identity = CredentialIdentity::new(KernelCredentials);
        for (uid, name) in self.entries {
            identity = identity.with_uid(uid, name);
        }
        identity
    }

    /// The identity of the peer of `stream`: kernel credentials looked up
    /// in the map. Used by services with an own socket (the log service).
    pub fn identify(
        &self,
        stream: &UnixStream,
    ) -> Result<(Identity, PeerCredentials), PeerCredError> {
        let credentials = peer_credentials(stream)?;
        self.identity(credentials.uid)
            .cloned()
            .map(|identity| (identity, credentials))
            .ok_or(PeerCredError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use ultar_bus::{PeerIdentity, PeerInfo};

    fn id(name: &str) -> Identity {
        Identity::new(name).unwrap()
    }

    #[test]
    fn the_kernel_reports_this_process_for_a_socket_pair() {
        assert!(probe());
        let (left, right) = UnixStream::pair().unwrap();
        for stream in [&left, &right] {
            let cred = peer_credentials(stream).unwrap();
            assert_eq!(cred.uid, sys::euid());
            assert_eq!(cred.gid, sys::gid());
            assert_eq!(cred.pid, Some(std::process::id()));
        }
        let cred = KernelCredentials.credentials(&left).unwrap();
        assert_eq!(cred.uid, sys::euid());
    }

    #[test]
    fn mapped_user_is_identified_and_unmapped_user_is_refused() {
        let (left, _right) = UnixStream::pair().unwrap();
        let peer = PeerInfo {
            stream: &left,
            client_name: "claims-to-be-root",
            token: Some("ignored"),
        };
        let mapped = UidMap::new()
            .with(sys::euid(), id("system.test"))
            .into_peer_identity();
        assert_eq!(mapped.identify(&peer).unwrap(), id("system.test"));
        let unmapped = UidMap::new()
            .with(sys::euid().wrapping_add(1), id("system.other"))
            .into_peer_identity();
        assert!(matches!(
            unmapped.identify(&peer),
            Err(BusError::AuthFailed)
        ));
        assert!(matches!(
            UidMap::new().into_peer_identity().identify(&peer),
            Err(BusError::AuthFailed)
        ));
        let map = UidMap::new().with(sys::euid(), id("log.me"));
        assert_eq!(map.identify(&left).unwrap().0, id("log.me"));
        assert!(UidMap::new().identify(&left).is_err());
    }

    #[test]
    fn map_text_is_parsed_strictly() {
        let map = UidMap::parse(
            "# services\n0 = \"system\"\n\n  1001   =   \"system.settingsd\"  # settings\n2001=\"app.phone\"\n",
        )
        .unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.identity(1001), Some(&id("system.settingsd")));
        assert_eq!(map.identity(7), None);
        assert!(UidMap::parse("").unwrap().is_empty());
        for (text, line) in [
            ("1001 = system", 1),
            ("\nx = \"a\"", 2),
            ("-1 = \"a\"", 1),
            ("1 = \"Not Valid\"", 1),
            ("1 = \"app.*\"", 1),
            ("1 \"a\"", 1),
            ("99999999999 = \"a\"", 1),
            ("4294967296 = \"a\"", 1),
            ("1 = \"a\" trailing", 1),
        ] {
            assert!(
                matches!(UidMap::parse(text), Err(PeerCredError::Syntax { line: l }) if l == line),
                "{text:?}"
            );
        }
        assert!(matches!(
            UidMap::parse("5 = \"a\"\n5 = \"b\""),
            Err(PeerCredError::Duplicate { line: 2 })
        ));
        let many: String = (0..=MAX_MAP_ENTRIES)
            .map(|uid| format!("{uid} = \"a\"\n"))
            .collect();
        assert!(matches!(
            UidMap::parse(&many),
            Err(PeerCredError::TooManyEntries)
        ));
    }

    #[test]
    fn map_files_writable_by_others_are_refused() {
        let dir = std::env::temp_dir().join(format!("ultar-uidmap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("uids.toml");
        std::fs::write(&path, "0 = \"system\"\n").unwrap();
        let chmod = |mode| {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        chmod(0o644);
        assert_eq!(UidMap::load(&path).unwrap().len(), 1);
        for mode in [0o664, 0o646, 0o666] {
            chmod(mode);
            assert!(matches!(
                UidMap::load(&path),
                Err(PeerCredError::UnsafeFile)
            ));
        }
        chmod(0o600);
        std::fs::write(&path, vec![b'#'; MAX_MAP_BYTES as usize + 1]).unwrap();
        assert!(matches!(UidMap::load(&path), Err(PeerCredError::TooLarge)));
        assert!(matches!(
            UidMap::load(&dir.join("missing")),
            Err(PeerCredError::Io(_))
        ));
        assert!(matches!(
            UidMap::load(&dir),
            Err(PeerCredError::UnsafeFile) | Err(PeerCredError::Io(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
        for error in [
            PeerCredError::Unsupported,
            PeerCredError::Io(std::io::ErrorKind::NotFound),
            PeerCredError::UnsafeFile,
            PeerCredError::TooLarge,
            PeerCredError::Syntax { line: 1 },
            PeerCredError::Duplicate { line: 2 },
            PeerCredError::TooManyEntries,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }
}
