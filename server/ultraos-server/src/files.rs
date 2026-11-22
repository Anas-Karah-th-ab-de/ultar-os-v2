//! Content-addressed file store and HTTP delivery of files.
//!
//! Files live in one flat directory and are named by the lower-case SHA-256 of their content.
//! The name always comes from the database (looked up by a validated identifier), never from the
//! client, and is validated again before it is joined to the directory; a request can therefore
//! not reach outside the store.
//!
//! The store only transports bytes. Packages and system images are signed by the publisher and
//! verified on the device; the server is not a trust anchor.

use crate::conditional::{self, ByteRange};
use crate::error::ApiError;
use crate::search::valid_sha256;
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const CHUNK: usize = 64 * 1024;

/// Errors of the file store.
#[derive(Debug)]
pub enum FileError {
    /// The name is not a lower-case SHA-256 digest.
    InvalidName,
    /// Reading or writing failed.
    Io(io::Error),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::InvalidName => f.write_str("file name is not a sha-256 digest"),
            FileError::Io(e) => write!(f, "file store i/o: {e}"),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FileError::InvalidName => None,
            FileError::Io(e) => Some(e),
        }
    }
}

impl From<io::Error> for FileError {
    fn from(e: io::Error) -> Self {
        FileError::Io(e)
    }
}

/// A file that was added to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    /// Lower-case hexadecimal SHA-256 of the content (also the file name).
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
}

/// What a download handler found in the database for the requested item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    /// SHA-256 recorded for the file.
    pub sha256: String,
    /// Size recorded for the file.
    pub size: u64,
    /// True when the file is in the server's store.
    pub stored: bool,
    /// External location of items that predate the file store (may be empty).
    pub legacy_url: String,
    /// True when access depended on an entitlement (controls caching, forbids redirects).
    pub gated: bool,
}

/// The directory of stored files.
#[derive(Debug, Clone)]
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    /// Uses `dir` as the store. The directory is created on first import.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The path of the file with the given digest, or [`FileError::InvalidName`].
    pub fn path_for(&self, sha256: &str) -> Result<PathBuf, FileError> {
        if !valid_sha256(sha256) {
            return Err(FileError::InvalidName);
        }
        Ok(self.dir.join(sha256))
    }

    /// Copies `source` into the store: written to a temporary file, flushed to disk, then renamed
    /// to its digest, so a crash never leaves a partial file under a valid name.
    pub fn import(&self, source: &Path) -> Result<StoredFile, FileError> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self
            .dir
            .join(format!(".tmp-{:032x}", rand::thread_rng().gen::<u128>()));
        let result = self.import_via(source, &tmp);
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    fn import_via(&self, source: &Path, tmp: &Path) -> Result<StoredFile, FileError> {
        let mut input = std::fs::File::open(source)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            output.write_all(&buf[..n])?;
            size += n as u64;
        }
        output.sync_all()?;
        drop(output);
        let sha256 = hex::encode(hasher.finalize());
        std::fs::rename(tmp, self.path_for(&sha256)?)?;
        std::fs::File::open(&self.dir)?.sync_all()?;
        Ok(StoredFile { sha256, size })
    }

    /// Answers a download request for `target`: the stored file (with `ETag`, `Range`,
    /// `If-None-Match`, `If-Range`), a redirect for an open pre-store item, or `not_found`.
    pub async fn deliver(
        &self,
        headers: &HeaderMap,
        target: &FileTarget,
    ) -> Result<Response, ApiError> {
        if target.stored {
            return self.serve(headers, target).await;
        }
        let url = &target.legacy_url;
        let external =
            url.starts_with("https://") || url.starts_with("http://") || url.starts_with('/');
        if target.gated || !external {
            return Err(ApiError::NotFound);
        }
        let location = HeaderValue::from_str(&target.legacy_url).map_err(|_| ApiError::NotFound)?;
        Ok((
            StatusCode::TEMPORARY_REDIRECT,
            [(header::LOCATION, location)],
        )
            .into_response())
    }

    async fn serve(&self, headers: &HeaderMap, target: &FileTarget) -> Result<Response, ApiError> {
        let path = self
            .path_for(&target.sha256)
            .map_err(|_| ApiError::NotFound)?;
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(event = "stored_file_missing", sha256 = %target.sha256, error = %e);
                return Err(ApiError::NotFound);
            }
        };
        let len = file.metadata().await.map_err(|_| ApiError::Internal)?.len();
        if len != target.size {
            tracing::error!(event = "stored_file_size_mismatch", sha256 = %target.sha256);
            return Err(ApiError::Internal);
        }
        let etag = format!("\"{}\"", target.sha256);
        let mut common = HeaderMap::new();
        common.insert(
            header::ETAG,
            HeaderValue::from_str(&etag).map_err(|_| ApiError::Internal)?,
        );
        common.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        let cache = if target.gated {
            conditional::CACHE_PRIVATE
        } else {
            conditional::CACHE_REVALIDATE
        };
        common.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        common.insert(header::VARY, HeaderValue::from_static("authorization"));
        if conditional::none_match_hits(headers, &etag) {
            return Ok((StatusCode::NOT_MODIFIED, common).into_response());
        }
        let (status, start, end) = match conditional::requested_range(headers, &etag, len) {
            ByteRange::Unsatisfiable => return Err(ApiError::RangeNotSatisfiable { size: len }),
            ByteRange::Full => (StatusCode::OK, 0, len.saturating_sub(1)),
            ByteRange::Partial { start, end } => (StatusCode::PARTIAL_CONTENT, start, end),
        };
        let count = if len == 0 { 0 } else { end - start + 1 };
        if status == StatusCode::PARTIAL_CONTENT {
            let value = format!("bytes {start}-{end}/{len}");
            common.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&value).map_err(|_| ApiError::Internal)?,
            );
            file.seek(io::SeekFrom::Start(start))
                .await
                .map_err(|_| ApiError::Internal)?;
        }
        common.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        common.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
        Ok((status, common, stream_body(file, count)).into_response())
    }
}

/// Streams exactly `count` bytes from `file` in bounded chunks (constant memory per download).
fn stream_body(file: tokio::fs::File, count: u64) -> Body {
    let stream = futures_util::stream::unfold((file, count), |(mut file, left)| async move {
        if left == 0 {
            return None;
        }
        let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
        let mut buf = vec![0u8; want];
        match file.read(&mut buf).await {
            Ok(0) => Some((
                Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                (file, 0),
            )),
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), (file, left - n as u64)))
            }
            Err(e) => Some((Err(e), (file, 0))),
        }
    });
    Body::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_for_accepts_only_digests() {
        let store = FileStore::new("/srv/files");
        let good = "ab".repeat(32);
        assert_eq!(
            store.path_for(&good).unwrap(),
            Path::new("/srv/files").join(&good)
        );
        for bad in [
            "",
            "..",
            "../../etc/passwd",
            "/etc/passwd",
            &"AB".repeat(32),
            &"g".repeat(64),
        ] {
            assert!(
                matches!(store.path_for(bad), Err(FileError::InvalidName)),
                "{bad}"
            );
        }
    }

    #[test]
    fn import_names_by_digest_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"hello").unwrap();
        let store = FileStore::new(dir.path().join("store"));
        let stored = store.import(&src).unwrap();
        assert_eq!(stored.size, 5);
        assert_eq!(
            stored.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            std::fs::read(store.path_for(&stored.sha256).unwrap()).unwrap(),
            b"hello"
        );
        assert_eq!(
            store.import(&src).unwrap(),
            stored,
            "importing twice is harmless"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path().join("store"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn import_of_missing_source_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("store"));
        let err = store.import(&dir.path().join("absent")).unwrap_err();
        assert!(matches!(err, FileError::Io(_)));
        assert!(err.to_string().contains("i/o"));
        assert!(std::error::Error::source(&err).is_some());
        assert_eq!(
            std::fs::read_dir(dir.path().join("store")).unwrap().count(),
            0
        );
        assert!(std::error::Error::source(&FileError::InvalidName).is_none());
    }
}
