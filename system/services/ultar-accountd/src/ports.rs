//! What the account logic needs from outside, as traits, and their real
//! implementations: the credential store (`auth.*` of `ultar-authd`, the
//! single store of password hashes), the system authentication dialog
//! (`sysdialog.authenticate`) and the home folders on disk.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::auth::{Method, Outcome, Secret, SetOutcome, SetSecret, Users, Verify};
use ultar_bus::messages::auth_admin::{ManageOutcome, RemoveUser, UpdateUser};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, CALL_TIMEOUT_MS};
use ultar_bus::{Client, Text};

/// How long a credential call may take (Argon2id is slow on purpose).
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of a password check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// Right.
    Granted,
    /// Wrong; the next try after this many milliseconds.
    Denied(u64),
    /// Not checked: tried again too early.
    Throttled(u64),
    /// The credential store cannot be used.
    Unavailable,
}

/// Why a credential change failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredError {
    /// The store refused (bad password, unknown account, last administrator).
    Rejected,
    /// The store is not there or damaged.
    Unavailable,
}

/// The credential store.
pub trait Credentials {
    /// It answers and its accounts are usable.
    fn available(&self) -> bool;
    /// Checks a password.
    fn verify(&self, user: &str, password: &Secret) -> Check;
    /// Sets a password (creates the account there with `display` if new).
    fn set_password(&self, user: &str, display: &str, password: &Secret) -> Result<(), CredError>;
    /// Changes the shown name and/or the administrator flag.
    fn update(
        &self,
        user: &str,
        display: Option<&str>,
        admin: Option<bool>,
    ) -> Result<(), CredError>;
    /// Removes the account and its secrets.
    fn remove(&self, user: &str) -> Result<(), CredError>;
}

/// The system's authentication dialog.
pub trait Dialog {
    /// Asks for an administrator's password; names the asking app and the action.
    fn authenticate(&self, caller: &str, action: Text) -> AuthResult;
}

/// Home folders.
pub trait Homes {
    /// Makes an empty home owned by the new user (mode 0700).
    fn create(&self, name: &str, uid: u32) -> io::Result<()>;
    /// Moves a removed user's home where only administrators reach it.
    fn retire(&self, name: &str, uid: u32) -> io::Result<()>;
    /// Deletes a removed user's home (links are not followed).
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// The credential store on the bus.
#[derive(Clone)]
pub struct BusCredentials(pub Client);

fn manage(outcome: ManageOutcome) -> Result<(), CredError> {
    match outcome {
        ManageOutcome::Done => Ok(()),
        ManageOutcome::Unknown | ManageOutcome::Rejected => Err(CredError::Rejected),
        ManageOutcome::Unavailable => Err(CredError::Unavailable),
    }
}

impl Credentials for BusCredentials {
    fn available(&self) -> bool {
        self.0.call(&Users {}, AUTH_TIMEOUT).is_ok()
    }

    fn verify(&self, user: &str, password: &Secret) -> Check {
        let call = Verify {
            user: user.to_owned(),
            method: Method::Password,
            secret: password.clone(),
        };
        match self.0.call(&call, AUTH_TIMEOUT).map(|r| r.outcome) {
            Ok(Outcome::Granted) => Check::Granted,
            Ok(Outcome::Denied { retry_after_ms }) => Check::Denied(retry_after_ms),
            Ok(Outcome::Throttled { retry_after_ms }) => Check::Throttled(retry_after_ms),
            Ok(Outcome::Unavailable) | Err(_) => Check::Unavailable,
        }
    }

    fn set_password(&self, user: &str, display: &str, password: &Secret) -> Result<(), CredError> {
        let call = SetSecret {
            user: user.to_owned(),
            display_name: display.to_owned(),
            method: Method::Password,
            current: None,
            new: password.clone(),
        };
        match self.0.call(&call, AUTH_TIMEOUT).map(|r| r.outcome) {
            Ok(SetOutcome::Done) => Ok(()),
            Ok(SetOutcome::Rejected) | Ok(SetOutcome::Denied { .. }) => Err(CredError::Rejected),
            Ok(SetOutcome::Unavailable) | Err(_) => Err(CredError::Unavailable),
        }
    }

    fn update(
        &self,
        user: &str,
        display: Option<&str>,
        admin: Option<bool>,
    ) -> Result<(), CredError> {
        let call = UpdateUser {
            user: user.to_owned(),
            display_name: display.map(str::to_owned),
            admin,
        };
        let reply = self
            .0
            .call(&call, AUTH_TIMEOUT)
            .map_err(|_| CredError::Unavailable)?;
        manage(reply.outcome)
    }

    fn remove(&self, user: &str) -> Result<(), CredError> {
        let call = RemoveUser {
            user: user.to_owned(),
        };
        let reply = self
            .0
            .call(&call, AUTH_TIMEOUT)
            .map_err(|_| CredError::Unavailable)?;
        manage(reply.outcome)
    }
}

/// The system dialog on the bus.
#[derive(Clone)]
pub struct BusDialog(pub Client);

impl Dialog for BusDialog {
    fn authenticate(&self, caller: &str, action: Text) -> AuthResult {
        let call = Authenticate {
            app_id: caller.to_owned(),
            app_name: caller.to_owned(),
            action,
        };
        match self.0.call(&call, Duration::from_millis(CALL_TIMEOUT_MS)) {
            Ok(reply) => reply.result,
            // No dialog service, no answer: nobody was authorised (fail closed).
            Err(_) => AuthResult::Unavailable,
        }
    }
}

/// Home folders below one directory (`/home`).
#[derive(Clone, Debug)]
pub struct FsHomes {
    /// The directory.
    pub root: PathBuf,
}

/// Folder of kept homes of removed users (below the home directory).
pub const REMOVED: &str = ".removed";

impl Homes for FsHomes {
    fn create(&self, name: &str, uid: u32) -> io::Result<()> {
        let path = self.root.join(name);
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        if let Err(e) = std::os::unix::fs::chown(&path, Some(uid), Some(uid)) {
            let _ = fs::remove_dir(&path);
            return Err(e);
        }
        Ok(())
    }

    fn retire(&self, name: &str, uid: u32) -> io::Result<()> {
        let removed = self.root.join(REMOVED);
        match fs::DirBuilder::new().mode(0o700).create(&removed) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        fs::set_permissions(&removed, fs::Permissions::from_mode(0o700))?;
        let from = self.root.join(name);
        if !from.exists() {
            return Ok(());
        }
        fs::rename(from, removed.join(format!("{name}-{uid}")))
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        let path = self.root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => fs::remove_file(path),
            Ok(_) => fs::remove_dir_all(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn homes_are_made_kept_and_deleted_without_following_links() {
        let root = std::env::temp_dir().join(format!("ultar-homes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let homes = FsHomes { root: root.clone() };
        let me = fs::metadata(&root).unwrap().uid();
        homes.create("leila", me).unwrap();
        let m = fs::metadata(root.join("leila")).unwrap();
        assert_eq!(m.mode() & 0o777, 0o700);
        assert!(
            homes.create("leila", me).is_err(),
            "never over an existing home"
        );
        fs::write(root.join("leila/note"), b"x").unwrap();
        homes.retire("leila", 1001).unwrap();
        assert!(root.join(".removed/leila-1001/note").exists());
        assert_eq!(
            fs::metadata(root.join(".removed")).unwrap().mode() & 0o777,
            0o700
        );
        homes.retire("ghost", 1).unwrap();
        // A home that is a link to elsewhere: only the link goes.
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("sami")).unwrap();
        homes.delete("sami").unwrap();
        assert!(outside.join("keep").exists());
        homes.create("omar", me).unwrap();
        fs::write(root.join("omar/f"), b"x").unwrap();
        homes.delete("omar").unwrap();
        assert!(!root.join("omar").exists());
        homes.delete("omar").unwrap();
        let _ = fs::remove_dir_all(&root);
    }
}
