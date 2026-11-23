//! Where bundles come from: the trait the engine downloads through, and the file source used
//! for offline updates (USB stick, folder).

use crate::bundle::BUNDLE_SUFFIX;
use crate::error::UpdateError;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most bundle files looked at in one folder.
pub const MAX_FOLDER_BUNDLES: usize = 64;

/// What the device asks a source for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query<'a> {
    /// Update channel.
    pub channel: &'a str,
    /// Processor architecture.
    pub arch: &'a str,
    /// Edition.
    pub edition: &'a str,
}

/// A bundle a source offers. Nothing in it is trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// How the source finds the bundle again (URL path, file path).
    pub locator: String,
    /// Size the source announced (0 = unknown). Information only.
    pub size_hint: u64,
}

/// A place bundles can be fetched from.
pub trait UpdateSource {
    /// The newest bundle for this device, or `None`.
    fn latest(&mut self, query: &Query<'_>) -> Result<Option<Remote>, UpdateError>;

    /// Opens the bundle for reading from byte `offset`; with `limit`, at most that many bytes
    /// are wanted (the header probe), so the source need not start sending the whole file.
    /// Returns the offset the stream really starts at: `offset`, or `0` when the source
    /// cannot resume.
    fn open(
        &mut self,
        remote: &Remote,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<(u64, Box<dyn Read + Send>), UpdateError>;

    /// True when read failures of the stream are network failures (reported as such and
    /// worth a retry), false for local files.
    fn is_network(&self) -> bool;
}

/// One bundle file on a local medium.
#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
}

impl FileSource {
    /// A source for the bundle at `path`.
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl UpdateSource for FileSource {
    fn latest(&mut self, _query: &Query<'_>) -> Result<Option<Remote>, UpdateError> {
        let size_hint = fs::metadata(&self.path)?.len();
        Ok(Some(Remote {
            locator: self.path.to_string_lossy().into_owned(),
            size_hint,
        }))
    }

    fn open(
        &mut self,
        _remote: &Remote,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<(u64, Box<dyn Read + Send>), UpdateError> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;
        Ok((offset, Box::new(file.take(limit.unwrap_or(u64::MAX)))))
    }

    fn is_network(&self) -> bool {
        false
    }
}

/// The bundle files an offline location offers: the file itself, or the regular files ending
/// in `.uup` directly inside a folder (sorted by name, not recursive, symbolic links are not
/// followed, at most [`MAX_FOLDER_BUNDLES`]).
pub fn bundles_in(location: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let meta = fs::symlink_metadata(location)?;
    if meta.is_file() {
        return Ok(vec![location.to_path_buf()]);
    }
    if !meta.is_dir() {
        return Err(UpdateError::InvalidArgument);
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(location)? {
        let entry = entry?;
        let name = entry.file_name();
        let is_bundle = name.to_str().is_some_and(|n| n.ends_with(BUNDLE_SUFFIX));
        if is_bundle && entry.file_type()?.is_file() {
            found.push(entry.path());
        }
    }
    found.sort();
    found.truncate(MAX_FOLDER_BUNDLES);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_resumes_at_any_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.uup");
        fs::write(&path, b"0123456789").unwrap();
        let mut source = FileSource::new(&path);
        let query = Query {
            channel: "stable",
            arch: "any",
            edition: "any",
        };
        let remote = source.latest(&query).unwrap().unwrap();
        assert_eq!(remote.size_hint, 10);
        let (at, mut reader) = source.open(&remote, 7, None).unwrap();
        let mut rest = String::new();
        reader.read_to_string(&mut rest).unwrap();
        assert_eq!((at, rest.as_str()), (7, "789"));
        let (_, mut reader) = source.open(&remote, 2, Some(3)).unwrap();
        rest.clear();
        reader.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "234");
        assert!(!source.is_network());
        assert!(FileSource::new(&dir.path().join("none"))
            .latest(&query)
            .is_err());
    }

    #[test]
    fn folder_listing_is_flat_sorted_and_ignores_other_things() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.uup"), b"").unwrap();
        fs::write(dir.path().join("a.uup"), b"").unwrap();
        fs::write(dir.path().join("notes.txt"), b"").unwrap();
        fs::create_dir(dir.path().join("dir.uup")).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/c.uup"), b"").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hostname", dir.path().join("link.uup")).unwrap();
        let names: Vec<String> = bundles_in(dir.path())
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.uup", "b.uup"]);
        let single = dir.path().join("notes.txt");
        assert_eq!(bundles_in(&single).unwrap(), vec![single]);
        assert!(bundles_in(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn folder_listing_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_FOLDER_BUNDLES + 5 {
            fs::write(dir.path().join(format!("{i:03}.uup")), b"").unwrap();
        }
        assert_eq!(bundles_in(dir.path()).unwrap().len(), MAX_FOLDER_BUNDLES);
    }
}
