//! Crash recovery: while a document has unsaved changes the app writes a copy next to it
//! (`.<name>.autosave.docx`; a document without a name uses `.untitled.autosave.docx` in
//! the Documents area). Saving, discarding or closing removes it. When the app opens and finds
//! a copy newer than the file, it offers to bring the changes back - the file itself is never
//! touched until the person saves.

use crate::place::NEW_IN;
use crate::state::{write_replacing, Doc, DocError};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How often a changed document is copied.
pub const INTERVAL: Duration = Duration::from_secs(20);

/// Where the copy of a document lives.
pub fn autosave_path(doc: Option<&Path>, areas: &Path) -> PathBuf {
    match doc.and_then(|p| Some((p.parent()?, p.file_name()?.to_string_lossy().into_owned()))) {
        Some((dir, name)) => dir.join(format!(".{name}.autosave.docx")),
        None => areas.join(NEW_IN).join(".untitled.autosave.docx"),
    }
}

/// Writes the copy of `doc` (atomically).
pub fn write(doc: &Doc, areas: &Path) -> Result<PathBuf, DocError> {
    let path = autosave_path(doc.path.as_deref(), areas);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    write_replacing(&path, &doc.docx_bytes()?)?;
    Ok(path)
}

/// Removes the copy of the document (if there is one).
pub fn remove(doc: Option<&Path>, areas: &Path) {
    let _ = std::fs::remove_file(autosave_path(doc, areas));
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The copy to offer for `doc` (the document's file, or `None` for an untitled document):
/// there is one and it is newer than the file (or the file is gone).
pub fn pending(doc: Option<&Path>, areas: &Path) -> Option<PathBuf> {
    let copy = autosave_path(doc, areas);
    let at = modified(&copy)?;
    match doc.and_then(modified) {
        Some(file) if file >= at => {
            let _ = std::fs::remove_file(&copy);
            None
        }
        _ => Some(copy),
    }
}

/// Replaces the content of `doc` with the copy; the document stays unsaved and keeps its name.
pub fn restore(doc: &mut Doc, copy: &Path) -> Result<(), DocError> {
    let bytes = std::fs::read(copy).map_err(|e| DocError::of(&e))?;
    let document = ultar_office::docx::read(&bytes).map_err(|_| DocError::Unsupported)?;
    let (path, zoom, page_view) = (doc.path.clone(), doc.zoom, doc.page_view);
    *doc = Doc::from_document(document);
    doc.path = path;
    doc.zoom = zoom;
    doc.page_view = page_view;
    doc.dirty = true;
    let _ = std::fs::remove_file(copy);
    Ok(())
}

/// Decides when to write the copy: the document changed since the last one and the
/// interval has passed.
#[derive(Debug, Default)]
pub struct Timer {
    saved_rev: Option<u64>,
    last: Option<Duration>,
}

impl Timer {
    /// `now` is any steady clock reading. Returns `true` when a copy is due (and marks it
    /// as written).
    pub fn due(&mut self, doc: &Doc, now: Duration) -> bool {
        if !doc.dirty() || self.saved_rev == Some(doc.revision()) {
            return false;
        }
        if self.last.is_some_and(|t| now.saturating_sub(t) < INTERVAL) {
            return false;
        }
        self.saved_rev = Some(doc.revision());
        self.last = Some(now);
        true
    }

    /// The document was saved, replaced or closed: start over.
    pub fn reset(&mut self) {
        *self = Timer::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-doc-rec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(d.join(NEW_IN));
        d
    }

    #[test]
    fn a_named_document_gets_a_hidden_copy_beside_it() {
        let p = autosave_path(
            Some(Path::new("/areas/documents/a.docx")),
            Path::new("/areas"),
        );
        assert_eq!(p, PathBuf::from("/areas/documents/.a.docx.autosave.docx"));
        let u = autosave_path(None, Path::new("/areas"));
        assert_eq!(u, PathBuf::from("/areas/documents/.untitled.autosave.docx"));
    }

    #[test]
    fn the_timer_waits_for_a_change_and_for_the_interval() {
        let mut d = Doc::new();
        let mut t = Timer::default();
        assert!(!t.due(&d, Duration::from_secs(0)), "nothing changed");
        d.insert("x");
        assert!(t.due(&d, Duration::from_secs(1)));
        assert!(!t.due(&d, Duration::from_secs(2)), "same revision");
        d.insert("y");
        assert!(!t.due(&d, Duration::from_secs(5)), "interval not over");
        assert!(t.due(&d, Duration::from_secs(30)));
    }

    #[test]
    fn a_crash_leaves_a_copy_that_brings_the_changes_back() {
        let areas = dir("crash");
        let file = areas.join(NEW_IN).join("letter.docx");
        let mut d = Doc::new();
        d.insert("saved text");
        assert!(d.save_as(&file).is_ok());
        std::thread::sleep(Duration::from_millis(30));
        d.insert(" and more typed after");
        let copy = write(&d, &areas).unwrap_or_default();
        assert!(copy.exists());
        // "crash": a new app instance opens the file.
        let mut again = Doc::open(&file).unwrap_or_default();
        assert_eq!(again.item_text(0, None).as_deref(), Some("saved text"));
        let found = pending(Some(&file), &areas);
        assert_eq!(found.as_deref(), Some(copy.as_path()));
        assert!(restore(&mut again, &copy).is_ok());
        assert_eq!(
            again.item_text(0, None).as_deref(),
            Some("saved text and more typed after")
        );
        assert!(again.dirty() && again.path.as_deref() == Some(file.as_path()));
        assert!(!copy.exists(), "the copy is removed once restored");
        let _ = std::fs::remove_dir_all(&areas);
    }

    #[test]
    fn an_older_copy_is_dropped_and_saving_removes_it() {
        let areas = dir("old");
        let file = areas.join(NEW_IN).join("a.docx");
        let mut d = Doc::new();
        d.insert("x");
        let _ = write(&d, &areas);
        std::thread::sleep(Duration::from_millis(30));
        assert!(d.save_as(&file).is_ok());
        assert!(
            pending(Some(&file), &areas).is_none(),
            "the file is newer than a stale copy"
        );
        let untitled = Doc::new();
        let _ = write(&untitled, &areas);
        assert!(pending(None, &areas).is_some());
        remove(None, &areas);
        assert!(pending(None, &areas).is_none());
        let _ = std::fs::remove_dir_all(&areas);
    }
}
