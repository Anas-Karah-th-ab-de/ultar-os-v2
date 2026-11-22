//! Repository walking and file loading.

use crate::error::CheckError;
use crate::scan::SourceFile;
use std::fs;
use std::path::Path;

/// Directory names that are never scanned (besides hidden directories).
const SKIPPED_DIRS: [&str; 3] = ["target", "vendor", "node_modules"];

/// File name of a Cargo manifest.
pub const MANIFEST_NAME: &str = "Cargo.toml";

/// Files of interest found in a repository.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tree {
    /// Rust sources, repository-relative with `/` separators, sorted.
    pub rust_files: Vec<String>,
    /// Cargo manifests, repository-relative with `/` separators, sorted.
    pub manifests: Vec<String>,
}

/// Walks the repository below `root`, skipping build output and hidden directories.
pub fn walk(root: &Path) -> Result<Tree, CheckError> {
    let mut tree = Tree::default();
    walk_dir(root, "", &mut tree)?;
    tree.rust_files.sort();
    tree.manifests.sort();
    Ok(tree)
}

fn walk_dir(dir: &Path, relative: &str, tree: &mut Tree) -> Result<(), CheckError> {
    let entries = fs::read_dir(dir).map_err(|source| io_error(dir, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_error(dir, source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| io_error(&entry.path(), source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        if file_type.is_dir() {
            if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_str()) {
                walk_dir(&entry.path(), &child, tree)?;
            }
        } else if file_type.is_file() {
            if name == MANIFEST_NAME {
                tree.manifests.push(child);
            } else if name.ends_with(".rs") {
                tree.rust_files.push(child);
            }
        }
    }
    Ok(())
}

/// Reads a repository file as text (invalid UTF-8 is replaced).
pub fn read_text(root: &Path, relative: &str) -> Result<String, CheckError> {
    let path = root.join(relative);
    let bytes = fs::read(&path).map_err(|source| io_error(&path, source))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads an optional repository file; `None` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<String>, CheckError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(path, source)),
    }
}

/// Loads and lexes all Rust sources of the repository.
pub fn load_sources(root: &Path) -> Result<Vec<SourceFile>, CheckError> {
    let tree = walk(root)?;
    let mut files = Vec::with_capacity(tree.rust_files.len());
    for relative in &tree.rust_files {
        let text = read_text(root, relative)?;
        files.push(SourceFile::parse(relative, &text));
    }
    Ok(files)
}

/// Wraps an I/O error with the path it happened on.
pub fn io_error(path: &Path, source: std::io::Error) -> CheckError {
    CheckError::Io {
        path: path.display().to_string(),
        source,
    }
}
