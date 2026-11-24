//! Small, bounded readers of kernel attribute files.

use std::io::Read;
use std::path::Path;

/// Longest attribute read.
const MAX_ATTR: u64 = 4096;

/// The trimmed text of an attribute file (`None`: missing or unreadable).
pub fn text(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut out = String::new();
    file.take(MAX_ATTR).read_to_string(&mut out).ok()?;
    Some(out.trim().to_owned())
}

/// An attribute as a number.
pub fn number(path: &Path) -> Option<i64> {
    text(path)?.parse().ok()
}

/// The entries of a directory, sorted by name (at most `limit`).
pub fn entries(dir: &Path, limit: usize) -> Vec<std::path::PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = read.flatten().take(limit).map(|e| e.path()).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_are_bounded_and_tolerant() {
        let dir = std::env::temp_dir().join(format!("hal-sysfs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), " 42\n").unwrap();
        std::fs::write(dir.join("b"), "x".repeat(10_000)).unwrap();
        assert_eq!(number(&dir.join("a")), Some(42));
        assert_eq!(text(&dir.join("b")).unwrap().len(), MAX_ATTR as usize);
        assert_eq!(text(&dir.join("missing")), None);
        assert_eq!(number(&dir.join("b")), None);
        assert_eq!(entries(&dir, 1).len(), 1);
        assert!(entries(&dir.join("missing"), 9).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
