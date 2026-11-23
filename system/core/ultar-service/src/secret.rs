//! Secrets (bus tokens, chain keys): where they may come from and a type
//! that cannot be printed.
//!
//! A secret is read from a **file** whose owner and permission bits are
//! checked on the opened descriptor, or from an **inherited descriptor**.
//! It is never taken from the command line (visible in the process list)
//! or from the environment (inherited by children, readable in
//! `/proc/<pid>/environ`).
//!
//! [`Secret`] implements neither `Display` nor `Clone`; its `Debug` output
//! is the fixed text `Secret(<redacted>)`. The bytes are overwritten when
//! the value is dropped.
//!
//! ```compile_fail
//! let secret = ultar_service::Secret::from_bytes(b"token".to_vec()).unwrap();
//! println!("{secret}"); // no Display
//! ```
//!
//! ```compile_fail
//! let secret = ultar_service::Secret::from_bytes(b"token".to_vec()).unwrap();
//! let copy = secret.clone(); // no Clone
//! ```

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use ultar_sys as sys;

/// Longest accepted secret in bytes (a token, a key).
pub const MAX_SECRET_LEN: usize = 4096;
/// Longest accepted secret *table* in bytes (e.g. the broker's token file:
/// one line per client).
pub const MAX_SECRET_TABLE_LEN: usize = 256 * 1024;

/// `O_NOFOLLOW`: the last path component must not be a symbolic link.
#[cfg(target_arch = "x86_64")]
const O_NOFOLLOW: i32 = 0o400_000;
/// `O_NOFOLLOW`: the last path component must not be a symbolic link.
#[cfg(target_arch = "aarch64")]
const O_NOFOLLOW: i32 = 0o100_000;

/// A value that must not appear in logs, error messages or panics.
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wraps `bytes`. One trailing line feed (as editors and `echo` leave
    /// it) is removed; an empty or oversized secret is refused.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, SecretError> {
        Self::from_bytes_limited(bytes, MAX_SECRET_LEN)
    }

    /// Like [`Self::from_bytes`] with an own upper bound (for tables).
    pub fn from_bytes_limited(mut bytes: Vec<u8>, max: usize) -> Result<Self, SecretError> {
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        let secret = Self(bytes);
        if secret.0.is_empty() {
            return Err(SecretError::Empty);
        }
        if secret.0.len() > max {
            return Err(SecretError::TooLong);
        }
        Ok(secret)
    }

    /// A second copy, for a secret read once from a descriptor and used
    /// again (each copy is wiped when dropped).
    pub fn duplicate(&self) -> Self {
        Self(self.0.clone())
    }

    /// The bytes. Whoever calls this is responsible for where they go.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The secret as text, if it is UTF-8.
    pub fn expose_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`: an empty secret cannot be constructed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Compares with `other` without stopping at the first difference.
    pub fn ct_eq(&self, other: &[u8]) -> bool {
        constant_time_eq(&self.0, other)
    }
}

/// Compares two byte strings in time that depends only on their lengths.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for (index, byte) in left.iter().enumerate() {
        let other = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(byte ^ other);
    }
    std::hint::black_box(difference) == 0
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            *byte = 0;
        }
        // Keeps the optimiser from removing the loop as a dead store.
        std::hint::black_box(&mut self.0);
    }
}

/// Why a secret was refused. No variant carries any part of the secret.
#[derive(Debug)]
pub enum SecretError {
    /// The file or descriptor could not be opened or read.
    Io(std::io::ErrorKind),
    /// Not a regular file.
    NotAFile,
    /// The file belongs to a user other than the service's or root.
    WrongOwner,
    /// Group or others have access to the file.
    TooPermissive,
    /// The secret is empty.
    Empty,
    /// The secret is longer than [`MAX_SECRET_LEN`].
    TooLong,
    /// The descriptor number is unusable.
    BadDescriptor,
}

impl SecretError {
    /// Stable key for logs.
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Io(_) => "secret.io",
            Self::NotAFile => "secret.not_a_file",
            Self::WrongOwner => "secret.wrong_owner",
            Self::TooPermissive => "secret.too_permissive",
            Self::Empty => "secret.empty",
            Self::TooLong => "secret.too_long",
            Self::BadDescriptor => "secret.bad_descriptor",
        }
    }
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "{}: {kind}", self.key()),
            _ => f.write_str(self.key()),
        }
    }
}

impl std::error::Error for SecretError {}

/// Who may own a secret file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecretFilePolicy {
    /// The user the file must belong to (normally the service's own).
    pub owner: u32,
    /// Also accept files owned by root (uid 0), as the service manager
    /// writes them.
    pub allow_root_owner: bool,
}

impl SecretFilePolicy {
    /// Files owned by the calling process's user or by root.
    pub fn for_current_user() -> Self {
        Self {
            owner: sys::euid(),
            allow_root_owner: true,
        }
    }
}

fn read_bounded(mut file: File, max: usize) -> Result<Secret, SecretError> {
    // The buffer never grows beyond its first allocation, so no unwiped
    // copy of the secret is left behind by a reallocation, and it lives in
    // a `Secret` from the start, so it is wiped on every path.
    let mut holder = Secret(Vec::with_capacity(max + 2));
    let limit = (max + 2) as u64;
    (&mut file)
        .take(limit)
        .read_to_end(&mut holder.0)
        .map_err(|error| SecretError::Io(error.kind()))?;
    Secret::from_bytes_limited(std::mem::take(&mut holder.0), max)
}

/// Reads a secret from `path`.
///
/// The file is opened without following a final symbolic link; owner, type
/// and mode are then checked **on the opened descriptor**, so the checked
/// file is the file that is read. Any permission bit for group or others
/// (`mode & 0o077`) refuses the file.
pub fn read_secret_file(path: &Path, policy: &SecretFilePolicy) -> Result<Secret, SecretError> {
    read_secret_file_limited(path, policy, MAX_SECRET_LEN)
}

/// Like [`read_secret_file`] with an own upper bound in bytes (for tables
/// such as the broker's token file, at most [`MAX_SECRET_TABLE_LEN`]).
pub fn read_secret_file_limited(
    path: &Path,
    policy: &SecretFilePolicy,
    max: usize,
) -> Result<Secret, SecretError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|error| SecretError::Io(error.kind()))?;
    let meta = file
        .metadata()
        .map_err(|error| SecretError::Io(error.kind()))?;
    if !meta.is_file() {
        return Err(SecretError::NotAFile);
    }
    let owner_ok = meta.uid() == policy.owner || (policy.allow_root_owner && meta.uid() == 0);
    if !owner_ok {
        return Err(SecretError::WrongOwner);
    }
    if meta.mode() & 0o077 != 0 {
        return Err(SecretError::TooPermissive);
    }
    read_bounded(file, max.min(MAX_SECRET_TABLE_LEN))
}

/// Reads a secret of at most `max` bytes from an inherited descriptor
/// (a pipe or a file) and closes it. The descriptor comes from
/// `Runtime::take_inherited` (daemons) or [`take_inherited_secret`]
/// (tools); a bare number is never trusted.
pub fn read_secret_owned(fd: OwnedFd, max: usize) -> Result<Secret, SecretError> {
    read_bounded(File::from(fd), max.min(MAX_SECRET_TABLE_LEN))
}

/// For tools without the service runtime: takes over the inherited
/// descriptor `fd` (a pipe or a regular file, checked) and reads a secret
/// of at most `max` bytes from it. Works once per process and must come
/// before the tool opens anything.
pub fn take_inherited_secret(fd: i32, max: usize) -> Result<Secret, SecretError> {
    let mut inherited =
        sys::take_inherited(&[fd], false, 0).map_err(|_| SecretError::BadDescriptor)?;
    let owned = inherited.take(fd).ok_or(SecretError::BadDescriptor)?;
    read_secret_owned(owned, max)
}

/// For tools without the service runtime: reads a secret of at most `max`
/// bytes from a checked file or an inherited descriptor (see
/// [`take_inherited_secret`]: one descriptor per process).
pub fn read_secret_source(source: &SecretSource, max: usize) -> Result<Secret, SecretError> {
    match source {
        SecretSource::File(path) => {
            read_secret_file_limited(path, &SecretFilePolicy::for_current_user(), max)
        }
        SecretSource::Fd(fd) => take_inherited_secret(*fd, max),
    }
}

/// Where a daemon finds a secret: a checked file or an inherited
/// descriptor. Never the command line or the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretSource {
    /// A file owned by the service's user (or root) without access for
    /// group and others.
    File(std::path::PathBuf),
    /// A descriptor the supervisor passed under this number; it must be
    /// declared in `ServiceSpec::inherited`.
    Fd(i32),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ultar-secret-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &[u8], mode: u32) {
        std::fs::write(path, content).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_private_file_is_read_and_the_line_feed_removed() {
        let dir = temp("ok");
        let path = dir.join("token");
        write(&path, b"s3cret\n", 0o600);
        let secret = read_secret_file(&path, &SecretFilePolicy::for_current_user()).unwrap();
        assert_eq!(secret.expose(), b"s3cret");
        assert_eq!(secret.expose_str(), Some("s3cret"));
        assert_eq!((secret.len(), secret.is_empty()), (6, false));
        write(&path, b"s3cret", 0o400);
        assert!(read_secret_file(&path, &SecretFilePolicy::for_current_user()).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn files_open_to_others_are_refused_bit_by_bit() {
        let dir = temp("mode");
        let path = dir.join("token");
        for bit in [0o040, 0o020, 0o010, 0o004, 0o002, 0o001] {
            write(&path, b"x", 0o600 | bit);
            assert!(
                matches!(
                    read_secret_file(&path, &SecretFilePolicy::for_current_user()),
                    Err(SecretError::TooPermissive)
                ),
                "{bit:o}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wrong_owner_links_directories_and_bad_content_are_refused() {
        let dir = temp("bad");
        let path = dir.join("token");
        write(&path, b"x", 0o600);
        let stranger = SecretFilePolicy {
            owner: sys::euid().wrapping_add(1).max(1),
            allow_root_owner: false,
        };
        assert!(matches!(
            read_secret_file(&path, &stranger),
            Err(SecretError::WrongOwner)
        ));
        let own = SecretFilePolicy::for_current_user();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(matches!(
            read_secret_file(&link, &own),
            Err(SecretError::Io(_))
        ));
        assert!(matches!(
            read_secret_file(&dir.join("missing"), &own),
            Err(SecretError::Io(_))
        ));
        assert!(read_secret_file(&dir, &own).is_err());
        write(&path, b"", 0o600);
        assert!(matches!(
            read_secret_file(&path, &own),
            Err(SecretError::Empty)
        ));
        write(&path, b"\n", 0o600);
        assert!(matches!(
            read_secret_file(&path, &own),
            Err(SecretError::Empty)
        ));
        write(&path, &vec![b'a'; MAX_SECRET_LEN + 1], 0o600);
        assert!(matches!(
            read_secret_file(&path, &own),
            Err(SecretError::TooLong)
        ));
        write(&path, &vec![b'a'; MAX_SECRET_LEN], 0o600);
        assert_eq!(read_secret_file(&path, &own).unwrap().len(), MAX_SECRET_LEN);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_secret_never_shows_in_debug_or_errors() {
        let secret = Secret::from_bytes(b"hunter2".to_vec()).unwrap();
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(Secret(<redacted>))");
        for error in [
            SecretError::Io(std::io::ErrorKind::NotFound),
            SecretError::NotAFile,
            SecretError::WrongOwner,
            SecretError::TooPermissive,
            SecretError::Empty,
            SecretError::TooLong,
            SecretError::BadDescriptor,
        ] {
            let text = format!("{error} {error:?}");
            assert!(text.contains("secret.") && !text.contains("hunter2"));
        }
    }

    #[test]
    fn comparison_is_correct() {
        let secret = Secret::from_bytes(b"abc".to_vec()).unwrap();
        assert!(secret.ct_eq(b"abc"));
        for other in [&b"abd"[..], b"ab", b"abcd", b""] {
            assert!(!secret.ct_eq(other));
        }
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn a_secret_is_read_from_an_owned_pipe_with_its_own_bound() {
        use std::io::Write;
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer.write_all(b"pipe-secret\n").unwrap();
        drop(writer);
        let secret = read_secret_owned(OwnedFd::from(reader), 64).unwrap();
        assert_eq!(secret.expose(), b"pipe-secret");
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer.write_all(&[b'a'; 65]).unwrap();
        drop(writer);
        assert!(matches!(
            read_secret_owned(OwnedFd::from(reader), 64),
            Err(SecretError::TooLong)
        ));
        let table = vec![b't'; MAX_SECRET_LEN * 3];
        assert_eq!(
            Secret::from_bytes_limited(table, MAX_SECRET_TABLE_LEN)
                .unwrap()
                .len(),
            MAX_SECRET_LEN * 3
        );
    }
}
