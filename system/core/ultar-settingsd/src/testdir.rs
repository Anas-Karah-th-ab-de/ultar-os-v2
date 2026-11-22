//! A unique temporary directory for tests that is removed on drop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A directory below the system temp directory, unique per process and call.
#[derive(Debug)]
pub struct TestDir(PathBuf);

impl TestDir {
    /// Creates the directory; `tag` makes it recognisable.
    ///
    /// If it cannot be created the path is still returned and the test that
    /// uses it fails on its first file operation.
    pub fn new(tag: &str) -> Self {
        let number = NEXT.fetch_add(1, Ordering::SeqCst);
        let name = format!("usettings-{}-{number}-{tag}", std::process::id());
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self(path)
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
