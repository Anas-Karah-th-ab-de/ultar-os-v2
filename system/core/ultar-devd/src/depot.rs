//! Reads what the daemon needs from disk besides hardware: the trusted keys and the
//! signed depot index. Failures never stop the daemon; they become notices and an empty
//! index (every device is then reported as driverless until a rescan succeeds).

use std::io::Read;
use std::path::Path;
use ultar_bus::{Text, TextArg};
use ultar_driverpkg::depot::load_signed_index;
use ultar_driverpkg::{Coded, DriverIndex, TrustedKeys};

/// Extension of trusted key files (one hexadecimal public key per line, `#` comments).
pub const KEY_EXTENSION: &str = "keys";
/// Most key files read from the key directory.
pub const MAX_KEY_FILES: usize = 64;
/// Largest accepted key file in bytes.
pub const MAX_KEY_FILE: u64 = 64 * 1024;

/// Keys, index and what went wrong while reading them.
#[derive(Debug, Clone)]
pub struct DepotState {
    /// Keys allowed to sign driver packages and the index.
    pub trusted: TrustedKeys,
    /// The verified index, limited to the packages present in the depot directory.
    pub index: DriverIndex,
    /// Notices for the device list.
    pub notices: Vec<Text>,
}

fn key_files(dir: &Path) -> Option<Vec<std::path::PathBuf>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|x| x == KEY_EXTENSION))
        .collect();
    files.sort();
    files.truncate(MAX_KEY_FILES);
    Some(files)
}

fn read_key_file(path: &Path) -> Option<TrustedKeys> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_KEY_FILE + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_KEY_FILE {
        return None;
    }
    TrustedKeys::parse(&text).ok()
}

/// Reads every `*.keys` file of `dir`. A file that cannot be read or parsed is skipped
/// as a whole and reported; it never adds a key.
pub fn load_trusted_keys(dir: &Path) -> (TrustedKeys, Vec<Text>) {
    let mut trusted = TrustedKeys::new();
    let mut notices = Vec::new();
    let Some(files) = key_files(dir) else {
        return (trusted, vec![Text::key("devd.notice.keys-missing")]);
    };
    for path in &files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match read_key_file(path) {
            Some(keys) => trusted.extend(keys),
            None => {
                notices.push(Text::key("devd.notice.keys-unreadable").with(TextArg::Verbatim(name)))
            }
        }
    }
    if trusted.is_empty() {
        notices.push(Text::key("devd.notice.keys-missing"));
    }
    (trusted, notices)
}

fn present_packages(depot: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(depot) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// File in the state directory with the highest index serial seen.
pub const SERIAL_FILE: &str = "index-serial";

/// Whether `index` is not older than the newest one this device accepted (kept in `state`);
/// a newer serial becomes the floor. An index without a serial (0) only passes where none
/// with a serial was ever seen.
fn fresh(index: &DriverIndex, state: &Path) -> bool {
    let file = state.join(SERIAL_FILE);
    let floor: u64 = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    if index.serial() < floor {
        return false;
    }
    if index.serial() > floor {
        // written atomically: a torn file would lower the floor to 0
        let temp = state.join(format!(".{SERIAL_FILE}.tmp"));
        let _ = std::fs::write(&temp, format!("{}\n", index.serial()))
            .and_then(|()| std::fs::rename(&temp, &file));
    }
    true
}

/// Loads keys and index. The index must carry a valid signature of a trusted key and must not
/// be older than one this device already accepted (`state`: the daemon's state directory).
pub fn load(depot: &Path, keys: &Path, state: &Path) -> DepotState {
    let (trusted, mut notices) = load_trusted_keys(keys);
    let index = match load_signed_index(depot, &trusted) {
        Ok(index) if !fresh(&index, state) => {
            notices.push(
                Text::key("devd.notice.index-older")
                    .with(TextArg::Verbatim(index.serial().to_string())),
            );
            DriverIndex::empty()
        }
        Ok(index) => index.retain_files(&present_packages(depot)),
        Err(error) => {
            notices.push(
                Text::key("devd.notice.depot-unreadable")
                    .with(TextArg::Key(error.code().to_string())),
            );
            DriverIndex::empty()
        }
    };
    DepotState {
        trusted,
        index,
        notices,
    }
}

#[cfg(test)]
mod serial_tests {
    use super::*;

    /// An old, validly signed index cannot be replayed once a newer one was used.
    #[test]
    fn an_older_index_is_refused_after_a_newer_one() {
        let dir = std::env::temp_dir().join(format!("devd-serial-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let index = |n| DriverIndex::empty().with_serial(n);
        assert!(
            fresh(&index(0), &dir),
            "nothing seen yet: an index without serial passes"
        );
        assert!(fresh(&index(100), &dir));
        assert!(fresh(&index(100), &dir), "the same one again");
        assert!(!fresh(&index(99), &dir), "older");
        assert!(!fresh(&index(0), &dir), "no serial after one with a serial");
        assert!(fresh(&index(200), &dir));
        assert_eq!(
            std::fs::read_to_string(dir.join(SERIAL_FILE)).unwrap(),
            "200\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
