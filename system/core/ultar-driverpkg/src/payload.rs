//! The package payload (`payload.tar.gz`): deterministic building, safe listing and unpacking.
//!
//! Unpacking never trusts the archive: absolute paths, `..` components, links, device
//! nodes, duplicates, oversized files and decompression bombs are all rejected before a
//! single byte is written.

use crate::package::{Limits, PackageError, Part};
use flate2::read::GzDecoder;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::Path;
use std::rc::Rc;

/// What is wrong with a path inside a payload or manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    /// The path is empty or only consists of `.` components.
    Empty,
    /// The path starts at the file-system root.
    Absolute,
    /// The path contains a `..` component.
    Traversal,
    /// The path contains a backslash, a control character or is too long.
    Invalid,
}

/// Checks a relative payload path and returns it in normal form (`a/b/c`, no `.` parts).
pub fn normalize_path(path: &str) -> Result<String, PathProblem> {
    if path.starts_with('/') {
        return Err(PathProblem::Absolute);
    }
    if path.len() > 1024 || path.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(PathProblem::Invalid);
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(PathProblem::Traversal),
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(PathProblem::Empty);
    }
    Ok(parts.join("/"))
}

/// One file or directory inside a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadEntry {
    /// Normalized relative path.
    pub path: String,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// `true` for a directory.
    pub is_dir: bool,
}

fn io_error(path: &Path, error: &io::Error) -> PackageError {
    PackageError::Io {
        path: path.display().to_string(),
        kind: error.kind(),
    }
}

fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), PackageError> {
    let mut children = Vec::new();
    for child in std::fs::read_dir(dir).map_err(|e| io_error(dir, &e))? {
        children.push(child.map_err(|e| io_error(dir, &e))?.path());
    }
    children.sort();
    for child in children {
        let meta = std::fs::symlink_metadata(&child).map_err(|e| io_error(&child, &e))?;
        let relative = child
            .strip_prefix(root)
            .unwrap_or(&child)
            .to_string_lossy()
            .replace('\\', "/");
        if meta.is_dir() {
            collect_files(root, &child, out)?;
        } else if meta.is_file() {
            out.push((
                relative,
                std::fs::read(&child).map_err(|e| io_error(&child, &e))?,
            ));
        } else {
            return Err(PackageError::UnsafeEntryType(relative));
        }
    }
    Ok(())
}

/// Builds a payload from the regular files below `dir` (sorted, fixed metadata – reproducible).
pub fn build_payload(dir: &Path) -> Result<Vec<u8>, PackageError> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    build_payload_from(&files)
}

/// Builds a payload from in-memory `(relative path, content)` pairs.
pub fn build_payload_from(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, PackageError> {
    let mem = |e: io::Error| PackageError::Io {
        path: String::new(),
        kind: e.kind(),
    };
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let mut seen = BTreeSet::new();
    for (path, content) in files {
        let normal = normalize_path(path).map_err(|problem| path_error(path, problem))?;
        if !seen.insert(normal.clone()) {
            return Err(PackageError::DuplicateEntry(normal));
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(&mut header, &normal, content.as_slice())
            .map_err(mem)?;
    }
    builder.into_inner().map_err(mem)?.finish().map_err(mem)
}

fn path_error(path: &str, problem: PathProblem) -> PackageError {
    match problem {
        PathProblem::Absolute => PackageError::AbsolutePath(path.to_string()),
        PathProblem::Traversal => PackageError::PathTraversal(path.to_string()),
        PathProblem::Empty | PathProblem::Invalid => PackageError::BadPath(path.to_string()),
    }
}

/// A reader that refuses to deliver more than a fixed number of bytes (decompression-bomb guard).
struct Capped<R> {
    inner: R,
    left: u64,
    tripped: Rc<Cell<bool>>,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 && !buf.is_empty() {
            let mut probe = [0u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            self.tripped.set(true);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "payload exceeds unpack limit",
            ));
        }
        let want = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let got = self.inner.read(&mut buf[..want])?;
        self.left -= got as u64;
        Ok(got)
    }
}

struct Scan<'a> {
    limits: &'a Limits,
    tripped: Rc<Cell<bool>>,
    total: u64,
    seen: BTreeSet<String>,
    entries: Vec<PayloadEntry>,
}

impl Scan<'_> {
    fn corrupt(&self) -> PackageError {
        if self.tripped.get() {
            PackageError::TooLarge {
                part: Part::UnpackedTotal,
                limit: self.limits.max_unpacked_total,
            }
        } else {
            PackageError::PayloadCorrupt
        }
    }

    /// Validates one archive entry and returns its normalized description.
    fn admit<R: Read>(&mut self, entry: &tar::Entry<'_, R>) -> Result<PayloadEntry, PackageError> {
        let raw = entry.path_bytes();
        let shown = String::from_utf8_lossy(&raw).into_owned();
        let text = std::str::from_utf8(&raw).map_err(|_| PackageError::BadPath(shown.clone()))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(PackageError::UnsafeEntryType(shown));
        }
        let path = normalize_path(text).map_err(|problem| path_error(text, problem))?;
        if self.entries.len() as u64 >= self.limits.max_entries {
            return Err(PackageError::TooManyEntries {
                limit: self.limits.max_entries,
            });
        }
        let size = if kind.is_dir() { 0 } else { entry.size() };
        if size > self.limits.max_unpacked_file {
            return Err(PackageError::TooLarge {
                part: Part::UnpackedFile,
                limit: self.limits.max_unpacked_file,
            });
        }
        self.total = self.total.saturating_add(size);
        if self.total > self.limits.max_unpacked_total {
            return Err(PackageError::TooLarge {
                part: Part::UnpackedTotal,
                limit: self.limits.max_unpacked_total,
            });
        }
        if !self.seen.insert(path.clone()) && !kind.is_dir() {
            return Err(PackageError::DuplicateEntry(path));
        }
        Ok(PayloadEntry {
            path,
            size,
            is_dir: kind.is_dir(),
        })
    }
}

/// Walks the payload, validating every entry; `visit` receives each admitted entry and its data.
fn walk(
    payload: &[u8],
    limits: &Limits,
    mut visit: impl FnMut(&PayloadEntry, &mut dyn Read) -> Result<(), PackageError>,
) -> Result<Vec<PayloadEntry>, PackageError> {
    let tripped = Rc::new(Cell::new(false));
    let overhead = limits.max_entries.saturating_add(4).saturating_mul(2048);
    let capped = Capped {
        inner: GzDecoder::new(payload),
        left: limits.max_unpacked_total.saturating_add(overhead),
        tripped: Rc::clone(&tripped),
    };
    let mut archive = tar::Archive::new(capped);
    let mut scan = Scan {
        limits,
        tripped,
        total: 0,
        seen: BTreeSet::new(),
        entries: Vec::new(),
    };
    let iter = archive.entries().map_err(|_| scan.corrupt())?;
    for entry in iter {
        let mut entry = entry.map_err(|_| scan.corrupt())?;
        let admitted = scan.admit(&entry)?;
        visit(&admitted, &mut entry)?;
        scan.entries.push(admitted);
    }
    Ok(scan.entries)
}

/// Lists and validates the payload without writing anything.
pub fn list_payload(payload: &[u8], limits: &Limits) -> Result<Vec<PayloadEntry>, PackageError> {
    let mut sink = io::sink();
    walk(payload, limits, |_, data| {
        io::copy(data, &mut sink)
            .map(|_| ())
            .map_err(|_| PackageError::PayloadCorrupt)
    })
}

fn refuse_links(dest: &Path, relative: &str) -> Result<(), PackageError> {
    let mut current = dest.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(PackageError::UnsafeEntryType(relative.to_string()));
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_entry(dest: &Path, entry: &PayloadEntry, data: &mut dyn Read) -> Result<(), PackageError> {
    refuse_links(dest, &entry.path)?;
    let target = dest.join(&entry.path);
    if entry.is_dir {
        return std::fs::create_dir_all(&target).map_err(|e| io_error(&target, &e));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
    }
    let mut file = std::fs::File::create(&target).map_err(|e| io_error(&target, &e))?;
    let written =
        io::copy(&mut data.take(entry.size), &mut file).map_err(|e| io_error(&target, &e))?;
    if written != entry.size {
        return Err(PackageError::PayloadCorrupt);
    }
    Ok(())
}

/// Validates the whole payload first and only then writes it below `dest`.
pub fn unpack_payload(
    payload: &[u8],
    dest: &Path,
    limits: &Limits,
) -> Result<Vec<PayloadEntry>, PackageError> {
    list_payload(payload, limits)?;
    std::fs::create_dir_all(dest).map_err(|e| io_error(dest, &e))?;
    walk(payload, limits, |entry, data| {
        write_entry(dest, entry, data)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize_path("drivers/a.uko"), Ok("drivers/a.uko".into()));
        assert_eq!(
            normalize_path("./drivers//a.uko"),
            Ok("drivers/a.uko".into())
        );
        assert_eq!(normalize_path("/etc/passwd"), Err(PathProblem::Absolute));
        assert_eq!(normalize_path("a/../../b"), Err(PathProblem::Traversal));
        assert_eq!(normalize_path(".."), Err(PathProblem::Traversal));
        assert_eq!(normalize_path(""), Err(PathProblem::Empty));
        assert_eq!(normalize_path("./."), Err(PathProblem::Empty));
        assert_eq!(normalize_path("a\\b"), Err(PathProblem::Invalid));
        assert_eq!(normalize_path("a\nb"), Err(PathProblem::Invalid));
        assert_eq!(normalize_path(&"a/".repeat(600)), Err(PathProblem::Invalid));
    }

    #[test]
    fn builds_deterministically_and_lists() {
        let files = vec![
            ("b/two.bin".to_string(), vec![2u8; 10]),
            ("one.txt".to_string(), b"1".to_vec()),
        ];
        let first = build_payload_from(&files).unwrap();
        assert_eq!(first, build_payload_from(&files).unwrap());
        let listed = list_payload(&first, &Limits::default()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[0],
            PayloadEntry {
                path: "b/two.bin".into(),
                size: 10,
                is_dir: false
            }
        );
    }

    #[test]
    fn builder_rejects_bad_input() {
        let one = |path: &str| build_payload_from(&[(path.to_string(), Vec::new())]);
        assert_eq!(one("/abs"), Err(PackageError::AbsolutePath("/abs".into())));
        assert_eq!(
            one("../up"),
            Err(PackageError::PathTraversal("../up".into()))
        );
        assert_eq!(one(""), Err(PackageError::BadPath(String::new())));
        let twice = vec![
            ("a".to_string(), Vec::new()),
            ("./a".to_string(), Vec::new()),
        ];
        assert_eq!(
            build_payload_from(&twice),
            Err(PackageError::DuplicateEntry("a".into()))
        );
    }

    #[test]
    fn garbage_is_corrupt() {
        assert_eq!(
            list_payload(b"not gzip at all", &Limits::default()),
            Err(PackageError::PayloadCorrupt)
        );
    }
}
