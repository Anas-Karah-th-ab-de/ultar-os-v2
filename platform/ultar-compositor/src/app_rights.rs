//! Rights of app user ids, from the file the app launcher writes
//! (`ultar-launcher`, `docs/integration/ultar-launcher.md`).
//!
//! Every app runs as its own user id from the launcher's range, so the
//! per-uid limits of the compositor are per app (findings N7/K9), and the
//! `ui.*` permissions the user granted an app reach the compositor as lines
//! `20003 = "ui.capture"` of this file. The file changes while the
//! compositor runs (installs, grants, revokes): it is looked at again when a
//! client says hello, and read again only when it changed.
//!
//! Trusted only when it is a regular file of the configured owner (the
//! launcher) that nobody else may write, at most [`MAX_BYTES`] long, every
//! line valid and every id inside the app range. Anything else gives no app
//! any right (fail closed); ids outside the range never get a right from
//! here – the static rights file is the only source for those.

use crate::config::RightsTable;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use ultar_surface::proto::Rights;

/// Largest file read.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Identity of the file's content as the kernel reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
}

/// What became of the last look at the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRightsState {
    /// Not looked at yet, or there is no file (no app has a right).
    Absent,
    /// Read and in use.
    Loaded,
    /// Refused: wrong owner or mode, too large, a bad line or an id
    /// outside the range. No app has a right.
    Refused,
}

/// The app rights file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRights {
    path: PathBuf,
    first: u32,
    last: u32,
    owner: u32,
    stamp: Option<Stamp>,
    table: RightsTable,
    state: AppRightsState,
}

impl AppRights {
    /// The file at `path` for the ids `first..=last`, written by `owner`.
    pub fn new(path: PathBuf, first: u32, last: u32, owner: u32) -> Self {
        Self {
            path,
            first,
            last,
            owner,
            stamp: None,
            table: RightsTable::default(),
            state: AppRightsState::Absent,
        }
    }

    /// What the last look found.
    pub fn state(&self) -> AppRightsState {
        self.state
    }

    /// Reads the file again if it changed since the last look.
    pub fn refresh(&mut self) {
        let meta = match std::fs::symlink_metadata(&self.path) {
            Ok(meta) => meta,
            Err(_) => {
                self.set(None, RightsTable::default(), AppRightsState::Absent);
                return;
            }
        };
        let stamp = Stamp {
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            mtime: meta.mtime(),
            mtime_nsec: meta.mtime_nsec(),
        };
        if self.stamp == Some(stamp) {
            return;
        }
        let trusted = meta.file_type().is_file()
            && meta.uid() == self.owner
            && meta.mode() & 0o022 == 0
            && meta.len() <= MAX_BYTES;
        let table = trusted
            .then(|| std::fs::read_to_string(&self.path).ok())
            .flatten()
            .and_then(|text| RightsTable::parse(&text).ok())
            .filter(|table| {
                table
                    .uids()
                    .all(|uid| (self.first..=self.last).contains(&uid))
            });
        match table {
            Some(table) => self.set(Some(stamp), table, AppRightsState::Loaded),
            None => self.set(Some(stamp), RightsTable::default(), AppRightsState::Refused),
        }
    }

    fn set(&mut self, stamp: Option<Stamp>, table: RightsTable, state: AppRightsState) {
        self.stamp = stamp;
        self.table = table;
        self.state = state;
    }

    /// The rights of `uid` (none outside the app range).
    pub fn rights_of(&self, uid: u32) -> Rights {
        if (self.first..=self.last).contains(&uid) {
            self.table.rights_of(uid)
        } else {
            Rights::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_launchers_file_is_followed_and_checked() {
        let dir = std::env::temp_dir().join(format!("app-rights-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("apps.toml");
        let me = std::fs::metadata(&dir).unwrap().uid();
        let mut apps = AppRights::new(file.clone(), 20000, 20009, me);
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Absent);
        let write = |text: &str| {
            std::fs::write(&file, text).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        };
        write("# launcher\n20003 = \"ui.capture,ui.overlay\"\n");
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Loaded);
        assert_eq!(
            apps.rights_of(20003),
            Rights::CAPTURE.union(Rights::OVERLAY)
        );
        assert_eq!(apps.rights_of(20004), Rights::NONE);
        assert_eq!(apps.rights_of(811), Rights::NONE);
        // changed (other length): read again
        write("20004 = \"ui.overlay\"\n");
        apps.refresh();
        assert_eq!(apps.rights_of(20003), Rights::NONE);
        assert_eq!(apps.rights_of(20004), Rights::OVERLAY);
        // an id outside the range or a bad line: no app has a right
        write("20004 = \"ui.overlay\"\n811 = \"ui.shell\"\n");
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Refused);
        assert_eq!(apps.rights_of(20004), Rights::NONE);
        write("20004 = \"ui.root\"\n");
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Refused);
        // writable by others or owned by someone else: refused
        write("20004 = \"ui.overlay\"\n");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Refused);
        let mut stranger = AppRights::new(file.clone(), 20000, 20009, me.wrapping_add(1));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        stranger.refresh();
        assert_eq!(stranger.state(), AppRightsState::Refused);
        std::fs::remove_file(&file).unwrap();
        apps.refresh();
        assert_eq!(apps.state(), AppRightsState::Absent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_static_table_wins_and_app_ids_come_from_the_file() {
        let dir = std::env::temp_dir().join(format!("app-rights-table-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("apps.toml");
        std::fs::write(&file, "20001 = \"ui.capture\"\n20002 = \"ui.overlay\"\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let me = std::fs::metadata(&file).unwrap().uid();
        let mut table =
            RightsTable::parse("1001 = \"ui.shell\"\n20002 = \"ui.lock-screen\"\n").unwrap();
        table.set_apps(AppRights::new(file, 20000, 20009, me));
        assert_eq!(
            table.rights_of(20001),
            Rights::NONE,
            "not read before the first hello"
        );
        table.refresh_apps();
        assert_eq!(table.rights_of(1001), Rights::SHELL);
        assert_eq!(table.rights_of(20001), Rights::CAPTURE);
        assert_eq!(
            table.rights_of(20002),
            Rights::LOCK_SCREEN,
            "the static file wins"
        );
        assert_eq!(table.rights_of(20005), Rights::NONE);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
