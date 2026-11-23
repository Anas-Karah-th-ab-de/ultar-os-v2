//! Whether the kernel enforces who reaches a fixed-identity socket
//! (review round 3, R3-K1, runtime side).
//!
//! A `--listen PATH=IDENTITY` socket is only an identity if the socket
//! file's owner and mode decide who may connect. The broker reads the file
//! back after binding: a kernel that creates no file, or ignores `chmod` and
//! `chown` (the Ultar kernel today), is detected, not assumed, and every
//! such listener is reported with a warning record
//! `bus.identity.unenforced`. Once the kernel keeps the file and its mode,
//! the warning goes away by itself. (Whether `connect` checks the mode
//! cannot be read back; with a socket file present this follows the
//! platform's rules.)

use crate::args::{IdentitySource, Listener};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use ultar_bus::SocketAccess;

/// Whether the file at `path` is a socket with the owner, group and mode
/// `access` asks for.
pub fn access_holds(path: &Path, access: &SocketAccess) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    meta.file_type().is_socket()
        && access.mode.is_none_or(|mode| meta.mode() & 0o777 == mode)
        && access.owner.is_none_or(|owner| meta.uid() == owner)
        && access.group.is_none_or(|group| meta.gid() == group)
}

/// Listener numbers (from 1, the broker's order) of the fixed-identity
/// sockets whose access the kernel does not show.
pub fn unenforced(listeners: &[Listener]) -> Vec<usize> {
    listeners
        .iter()
        .enumerate()
        .filter(|(_, listener)| matches!(listener.source, IdentitySource::Fixed(_)))
        .filter(|(_, listener)| !access_holds(&listener.path, &listener.access))
        .map(|(index, _)| index + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    fn listener(path: &Path, source: IdentitySource, mode: u32) -> Listener {
        Listener {
            path: path.to_owned(),
            source,
            access: SocketAccess::mode(mode),
        }
    }

    #[test]
    fn a_socket_file_with_the_asked_mode_is_enforced_anything_else_is_not() {
        let dir = std::env::temp_dir().join(format!("ultar-busd-enf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let _bound = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let fixed = || IdentitySource::Fixed("x".to_owned());
        let file = dir.join("plain");
        std::fs::write(&file, "").unwrap();
        let listeners = [
            listener(&path, fixed(), 0o600),
            listener(&path, fixed(), 0o660),
            listener(&dir.join("missing.sock"), fixed(), 0o600),
            listener(&file, fixed(), 0o600),
            // Not a fixed identity: not judged by the file.
            listener(&dir.join("missing.sock"), IdentitySource::Token, 0o600),
        ];
        assert_eq!(unenforced(&listeners), vec![2, 3, 4]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
