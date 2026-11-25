//! The device keys seen for other numbers (trust on first use). A number whose key changes is
//! still called, but the phone says so: the person compares the safety number before trusting
//! the call. Kept in the call service's own folder (`0600`), never sent anywhere.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// Most numbers kept.
const MAX_PINS: usize = 4096;

/// What a key means for a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pin {
    /// First call with this number: the key is kept from now on.
    New,
    /// The key seen before.
    Same,
    /// Another key than before (a new device of the person, or someone in between).
    Changed,
}

/// The kept keys, optionally in a file.
#[derive(Default)]
pub struct Pins {
    keys: BTreeMap<String, [u8; 32]>,
    file: Option<PathBuf>,
}

impl Pins {
    /// Keys from `file` (absent or unreadable: none yet).
    pub fn open(file: PathBuf) -> Self {
        let keys = std::fs::read_to_string(&file)
            .ok()
            .map(|t| {
                t.lines()
                    .filter_map(|l| {
                        let (n, k) = l.split_once(' ')?;
                        let k: [u8; 32] = B64.decode(k).ok()?.try_into().ok()?;
                        Some((n.to_owned(), k))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            keys,
            file: Some(file),
        }
    }

    /// Checks `key` for `number` and keeps it (the newest key is kept after a change).
    pub fn check(&mut self, number: &str, key: &[u8; 32]) -> Pin {
        let pin = match self.keys.get(number) {
            None => Pin::New,
            Some(k) if k == key => return Pin::Same,
            Some(_) => Pin::Changed,
        };
        if self.keys.len() < MAX_PINS || self.keys.contains_key(number) {
            self.keys.insert(number.to_owned(), *key);
            self.save();
        }
        pin
    }

    fn save(&self) {
        let Some(path) = &self.file else { return };
        let text: String = self
            .keys
            .iter()
            .map(|(n, k)| format!("{n} {}\n", B64.encode(k)))
            .collect();
        let tmp = path.with_extension("new");
        let ok = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .and_then(|()| std::fs::rename(&tmp, path));
        if ok.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_use_then_same_then_changed_and_kept_on_disk() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pins");
        let mut p = Pins::open(f.clone());
        assert_eq!(p.check("UOS-AAAA-0001", &[1; 32]), Pin::New);
        assert_eq!(p.check("UOS-AAAA-0001", &[1; 32]), Pin::Same);
        let mut again = Pins::open(f.clone());
        assert_eq!(again.check("UOS-AAAA-0001", &[1; 32]), Pin::Same);
        assert_eq!(again.check("UOS-AAAA-0001", &[2; 32]), Pin::Changed);
        assert_eq!(again.check("UOS-AAAA-0001", &[2; 32]), Pin::Same);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(Pins::default().check("x", &[0; 32]), Pin::New);
    }
}
