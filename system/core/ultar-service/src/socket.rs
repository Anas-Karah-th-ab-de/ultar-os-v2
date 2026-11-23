//! Binding a Unix socket the careful way, for services with an own socket.

use std::fmt;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

/// Why a socket could not be bound.
#[derive(Debug)]
pub enum BindError {
    /// Another process is listening on the path.
    InUse,
    /// Something that is not a socket lies at the path; it is left alone.
    NotASocket,
    /// The operating system refused.
    Io(std::io::Error),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InUse => f.write_str("socket path is served by another process"),
            Self::NotASocket => {
                f.write_str("socket path is occupied by something that is not a socket")
            }
            Self::Io(error) => write!(f, "socket: {error}"),
        }
    }
}

impl std::error::Error for BindError {}

/// Binds `path`. A socket file nobody listens on any more (left behind by
/// a killed instance) is replaced; a live socket is [`BindError::InUse`];
/// a regular file, directory or symbolic link at the path is **never**
/// removed. With `mode` the permission bits of the socket file are set;
/// without it the process's umask decides (077 after hardening: private).
pub fn bind_listener(path: &Path, mode: Option<u32>) -> Result<UnixListener, BindError> {
    bind_listener_for(path, mode, None)
}

/// Like [`bind_listener`], and hands the socket file to the group `group`
/// before its mode is set (so it is never open to more than the final
/// setting allows). A process may give a file it owns to a group it is a
/// member of; anything else needs the right to change owners.
pub fn bind_listener_for(
    path: &Path,
    mode: Option<u32>,
    group: Option<u32>,
) -> Result<UnixListener, BindError> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            let meta = std::fs::symlink_metadata(path).map_err(BindError::Io)?;
            if !meta.file_type().is_socket() {
                return Err(BindError::NotASocket);
            }
            if UnixStream::connect(path).is_ok() {
                return Err(BindError::InUse);
            }
            std::fs::remove_file(path).map_err(BindError::Io)?;
            UnixListener::bind(path).map_err(BindError::Io)?
        }
        Err(error) => return Err(BindError::Io(error)),
    };
    if group.is_some() {
        std::os::unix::fs::chown(path, None, group).map_err(BindError::Io)?;
    }
    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))
            .map_err(BindError::Io)?;
    }
    Ok(listener)
}

/// Makes sure the directory a socket is to live in exists. A directory
/// this call has to create gets `mode` (e.g. `0o755`: everybody may pass
/// through, the socket's own mode decides who connects); an existing
/// directory is left exactly as it is.
pub fn ensure_parent(socket: &Path, mode: u32) -> std::io::Result<()> {
    let Some(parent) = socket
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if parent.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(mode & 0o777))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_parent_is_created_with_the_mode_and_an_existing_one_untouched() {
        let dir = std::env::temp_dir().join(format!("ultar-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        ensure_parent(&dir.join("s.sock"), 0o755).unwrap();
        assert_eq!(mode(&dir), 0o700);
        ensure_parent(&dir.join("bus/s.sock"), 0o755).unwrap();
        assert_eq!(mode(&dir.join("bus")), 0o755);
        ensure_parent(Path::new("s.sock"), 0o755).unwrap();
        std::fs::write(dir.join("file"), b"x").unwrap();
        assert!(ensure_parent(&dir.join("file/s.sock"), 0o755).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_sockets_are_replaced_live_ones_and_files_are_not() {
        let dir = std::env::temp_dir().join(format!("ultar-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let first = bind_listener(&path, Some(0o600)).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(matches!(bind_listener(&path, None), Err(BindError::InUse)));
        drop(first);
        // The file is still there, nobody listens: replaced.
        assert!(path.exists());
        let second = bind_listener(&path, Some(0o666)).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o666
        );
        drop(second);
        let file = dir.join("file");
        std::fs::write(&file, b"keep").unwrap();
        assert!(matches!(
            bind_listener(&file, None),
            Err(BindError::NotASocket)
        ));
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(matches!(
            bind_listener(&link, None),
            Err(BindError::NotASocket)
        ));
        assert!(matches!(
            bind_listener(&dir.join("no/such/dir.sock"), None),
            Err(BindError::Io(_))
        ));
        for error in [
            BindError::InUse,
            BindError::NotASocket,
            BindError::Io(ErrorKind::Other.into()),
        ] {
            assert!(!error.to_string().is_empty());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
