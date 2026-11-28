//! Where PDF documents live: the cell binds each granted place of the user at
//! `/areas/<name>`; a `files://<area>/<path>` URI names a file below one of them, and its
//! `#page=N` (or `?page=N`) suffix names the page to open. Nothing outside the areas is
//! reachable (no `..`, no absolute parts). The recent files are a small list in `app:data`.

use std::path::{Path, PathBuf};

/// Longest URI taken.
pub const MAX_URI: usize = 4096;
/// Recent files kept.
pub const MAX_RECENT: usize = 16;
/// File name of the recent list below the data directory.
const RECENT_FILE: &str = "recents.txt";

/// Why a file could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// The system refused (permission, read-only place).
    Denied,
    /// The file or its folder does not exist.
    Missing,
    /// Any other input/output error.
    Io,
}

impl ReadError {
    /// The message key of the reason.
    pub fn key(self) -> &'static str {
        match self {
            ReadError::Denied => "pdf.error.denied",
            ReadError::Missing => "pdf.error.missing",
            ReadError::Io => "pdf.error.io",
        }
    }

    pub fn of(e: &std::io::Error) -> ReadError {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                ReadError::Denied
            }
            std::io::ErrorKind::NotFound => ReadError::Missing,
            _ => ReadError::Io,
        }
    }
}

/// The path of the file a `files://` URI names below `areas`; `None` for anything else.
pub fn path_of(uri: &str, areas: &Path) -> Option<PathBuf> {
    if uri.len() > MAX_URI || uri.chars().any(char::is_control) {
        return None;
    }
    let rest = uri.strip_prefix("files://")?;
    let rest = rest.split(['#', '?']).next().unwrap_or("");
    let mut path = areas.to_path_buf();
    let mut parts = 0;
    for raw in rest.split('/').filter(|p| !p.is_empty()) {
        let part = decode(raw)?;
        if part == "." || part == ".." || part.contains('/') || part.contains('\0') {
            return None;
        }
        path.push(part);
        parts += 1;
    }
    // an area and at least one name in it
    (parts >= 2).then_some(path)
}

/// The (0-based) page a `#page=N` / `?page=N` suffix names; 0 when absent.
pub fn page_of(uri: &str) -> usize {
    for part in uri.split(['#', '?']).skip(1) {
        for kv in part.split('&') {
            if let Some(v) = kv.strip_prefix("page=") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    return n.saturating_sub(1);
                }
            }
        }
    }
    0
}

/// The recent files (URIs or paths), newest first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recents {
    pub entries: Vec<String>,
}

impl Recents {
    /// Reads the saved list below `data` (missing or broken → empty).
    pub fn load(data: &Path) -> Recents {
        let file = data.join(RECENT_FILE);
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let entries = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(MAX_RECENT)
            .map(str::to_owned)
            .collect();
        Recents { entries }
    }

    /// Remembers `entry` at the front, without duplicates, at most [`MAX_RECENT`].
    pub fn push(&mut self, entry: &str) {
        if entry.trim().is_empty() {
            return;
        }
        self.entries.retain(|e| e != entry);
        self.entries.insert(0, entry.to_owned());
        self.entries.truncate(MAX_RECENT);
    }

    /// Writes the list below `data` (atomically via the office library).
    pub fn save(&self, data: &Path) {
        let file = data.join(RECENT_FILE);
        let text = self.entries.join("\n") + "\n";
        let _ = ultar_office::write_atomic(&file.to_string_lossy(), text.as_bytes());
    }
}

fn decode(part: &str) -> Option<String> {
    let raw = part.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = std::str::from_utf8(raw.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_uri_is_a_path_below_the_areas() {
        let a = Path::new("/areas");
        assert_eq!(
            path_of("files://documents/%D9%85%D9%84%D8%A7%D8%AD%D8%B8%D8%A7%D8%AA.pdf", a),
            Some(PathBuf::from("/areas/documents/ملاحظات.pdf"))
        );
        assert_eq!(path_of("files://documents/../x.pdf", a), None);
        assert_eq!(path_of("files://documents", a), None);
        assert_eq!(path_of("file:///etc/passwd", a), None);
    }

    #[test]
    fn the_page_fragment_names_the_page() {
        assert_eq!(page_of("files://documents/a.pdf#page=3"), 2);
        assert_eq!(page_of("files://documents/a.pdf?page=1"), 0);
        assert_eq!(page_of("files://documents/a.pdf"), 0);
        assert_eq!(page_of("files://documents/a.pdf#page=none"), 0);
    }

    #[test]
    fn recents_roundtrip_and_dedupe() {
        let d = std::env::temp_dir().join(format!("ultar-pdf-recents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut r = Recents::load(&d);
        assert!(r.entries.is_empty());
        r.push("files://documents/a.pdf");
        r.push("files://documents/b.pdf");
        r.push("files://documents/a.pdf");
        assert_eq!(r.entries, vec!["files://documents/a.pdf", "files://documents/b.pdf"]);
        r.save(&d);
        assert_eq!(Recents::load(&d).entries, r.entries);
        let _ = std::fs::remove_dir_all(&d);
    }
}