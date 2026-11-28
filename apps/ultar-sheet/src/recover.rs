//! A copy of unsaved changes for crash recovery. While there are unsaved changes the window
//! writes the workbook, now and then, to a file in the recovery folder; a save or a closed
//! window without changes removes it. On the next start of the same file (or of a new
//! workbook) the window finds the copy and offers it.

use crate::book::Book;
use crate::file::{self, DocError};
use std::path::{Path, PathBuf};

/// The recovery folder and the name this window's copy has in it.
#[derive(Debug, Clone)]
pub struct Recovery {
    dir: PathBuf,
    id: String,
    last: u64,
}

/// A copy found at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The recovery file.
    pub file: PathBuf,
    /// The workbook it belongs to (none: a new, never saved workbook).
    pub of: Option<PathBuf>,
}

/// 64-bit FNV-1a of a path, in hex: the name of a copy.
fn id_of(path: Option<&Path>) -> String {
    match path {
        None => "untitled".to_owned(),
        Some(p) => {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in p.to_string_lossy().bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            format!("{h:016x}")
        }
    }
}

impl Recovery {
    /// The copy of `path` (or of a new workbook) in `dir`.
    pub fn new(dir: &Path, path: Option<&Path>) -> Recovery {
        Recovery { dir: dir.to_owned(), id: id_of(path), last: 0 }
    }

    /// The recovery folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn data(&self) -> PathBuf {
        self.dir.join(format!("{}.xlsx", self.id))
    }

    fn meta(&self) -> PathBuf {
        self.dir.join(format!("{}.path", self.id))
    }

    /// A copy left by an earlier run (a crash), when it is newer than the saved file.
    pub fn find(&self, of: Option<&Path>) -> Option<Found> {
        let data = self.data();
        let copy = std::fs::metadata(&data).ok()?;
        if copy.len() == 0 {
            return None;
        }
        if let Some(saved) = of.and_then(|p| std::fs::metadata(p).ok()) {
            if let (Ok(a), Ok(b)) = (copy.modified(), saved.modified()) {
                if a <= b {
                    return None;
                }
            }
        }
        Some(Found { file: data, of: of.map(Path::to_owned) })
    }

    /// Writes the copy when the workbook changed since the last copy.
    pub fn save(&mut self, book: &Book, of: Option<&Path>) -> Result<bool, DocError> {
        if !book.dirty() || book.version == self.last {
            return Ok(false);
        }
        std::fs::create_dir_all(&self.dir).map_err(|_| DocError::Io)?;
        let (bytes, _) = file::write_bytes(&book.wb, file::Format::Xlsx, book.active)?;
        file::write_atomic(&self.data(), &bytes)?;
        if let Some(p) = of {
            let _ = std::fs::write(self.meta(), p.to_string_lossy().as_bytes());
        }
        self.last = book.version;
        Ok(true)
    }

    /// Removes the copy (the workbook was saved, or the changes were given up).
    pub fn clear(&mut self) {
        let _ = std::fs::remove_file(self.data());
        let _ = std::fs::remove_file(self.meta());
        self.last = 0;
    }

    /// Reads a found copy.
    pub fn load(found: &Found) -> Result<ultar_office::sheet::Workbook, DocError> {
        Ok(file::open(&found.file)?.book)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-sheet-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_crash_leaves_a_copy_that_is_found_and_read() {
        let d = dir("crash");
        let doc = d.join("a.xlsx");
        let mut r = Recovery::new(&d.join("rec"), Some(&doc));
        let mut b = Book::new();
        // nothing changed: nothing written
        assert!(!r.save(&b, Some(&doc)).unwrap());
        b.set(0, 0, "unsaved");
        b.set(1, 0, "=1+2");
        assert!(r.save(&b, Some(&doc)).unwrap());
        // no change since: nothing again
        assert!(!r.save(&b, Some(&doc)).unwrap());
        // "restart": a new Recovery of the same file finds it
        let again = Recovery::new(&d.join("rec"), Some(&doc));
        let found = again.find(Some(&doc)).expect("copy");
        let wb = Recovery::load(&found).unwrap();
        assert_eq!(wb.sheets[0].get(0, 0), "unsaved");
        assert_eq!(wb.sheets[0].get(1, 0), "=1+2");
        // another file has its own copy
        assert!(Recovery::new(&d.join("rec"), Some(&d.join("b.xlsx"))).find(None).is_none());
        // saving removes it
        r.clear();
        assert!(again.find(Some(&doc)).is_none());
    }

    #[test]
    fn a_copy_older_than_the_saved_file_is_not_offered() {
        let d = dir("old");
        std::fs::create_dir_all(&d).unwrap();
        let doc = d.join("a.xlsx");
        let mut r = Recovery::new(&d.join("rec"), Some(&doc));
        let mut b = Book::new();
        b.set(0, 0, "x");
        r.save(&b, Some(&doc)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&doc, b"saved later").unwrap();
        assert!(r.find(Some(&doc)).is_none());
    }

    #[test]
    fn a_new_workbook_has_a_copy_too() {
        let d = dir("new");
        let mut r = Recovery::new(&d, None);
        let mut b = Book::new();
        b.set(2, 2, "kept");
        r.save(&b, None).unwrap();
        let f = Recovery::new(&d, None).find(None).unwrap();
        assert_eq!(f.of, None);
        assert_eq!(Recovery::load(&f).unwrap().sheets[0].get(2, 2), "kept");
    }
}
