//! The staging area: where a bundle is collected, resumably, until it is complete and verified.
//!
//! Files in `<state>/staging`: `bundle.part` (in progress), `bundle.pos` (length known to be on
//! the medium), `bundle.uup` (complete) and `bundle.ok` (completion marker: manifest digest,
//! size, downgrade permission). Only a bundle with a matching marker is ever applied.
//! `bundle.part` and `bundle.uup` are segmented streams ([`crate::segfile`]): the Ultar kernel
//! refuses written files above 64 MiB, a bundle is several hundred megabytes.

use crate::bundle::Header;
use crate::error::{NetworkError, UpdateError};
use crate::fault::{Checkpoint, Step};
use crate::fsutil::{create_private_dir, read_bounded, remove_if_present, sync_dir, write_atomic};
use crate::manifest::{valid_ident, Manifest, Role};
use crate::segfile::{self, SegFile};
use crate::source::{Remote, UpdateSource};
use crate::verify::VerifiedManifest;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Size of one copy step; a checkpoint is reached before each.
pub const CHUNK: usize = 64 * 1024;
/// The download is flushed and its length recorded after this many bytes.
pub const FLUSH_EVERY: u64 = 4 * 1024 * 1024;

/// Slows a transfer down (rate limit policy).
pub trait Pacer {
    /// Called after `bytes` more bytes were stored; may block.
    fn consumed(&mut self, bytes: u64);
}

/// No rate limit.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoPacing;

impl Pacer for NoPacing {
    fn consumed(&mut self, _bytes: u64) {}
}

/// Checks payload hashes while the payload part of a bundle streams by.
#[derive(Debug)]
pub struct PayloadCheck {
    parts: Vec<(Role, u64, [u8; 32])>,
    index: usize,
    seen: u64,
    hasher: Sha256,
}

impl PayloadCheck {
    /// A checker for the payloads of `manifest`, in manifest order.
    pub fn new(manifest: &Manifest) -> Self {
        Self {
            parts: manifest
                .payloads
                .iter()
                .map(|p| (p.role, p.size, p.sha256))
                .collect(),
            index: 0,
            seen: 0,
            hasher: Sha256::new(),
        }
    }

    /// Feeds the next bytes. [`UpdateError::HashMismatch`] as soon as a payload is complete
    /// and wrong; [`UpdateError::SizeExceeded`] for bytes after the last payload.
    pub fn feed(&mut self, mut data: &[u8]) -> Result<(), UpdateError> {
        while !data.is_empty() {
            let Some((role, size, expected)) = self.parts.get(self.index).copied() else {
                return Err(UpdateError::SizeExceeded);
            };
            let take = usize::try_from(size - self.seen).map_or(data.len(), |r| r.min(data.len()));
            self.hasher.update(&data[..take]);
            self.seen += take as u64;
            data = &data[take..];
            if self.seen == size {
                let digest: [u8; 32] = std::mem::take(&mut self.hasher).finalize().into();
                if digest != expected {
                    return Err(UpdateError::HashMismatch(role));
                }
                self.index += 1;
                self.seen = 0;
            }
        }
        Ok(())
    }

    /// True when every payload has been seen completely and matched.
    pub fn complete(&self) -> bool {
        self.index == self.parts.len()
    }
}

/// A complete, verified bundle in the staging area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedInfo {
    /// SHA-256 of the signed manifest envelope.
    pub digest: [u8; 32],
    /// Size of the bundle file.
    pub size: u64,
    /// An administrator allowed this bundle although it is older than the running version.
    pub allow_downgrade: bool,
    /// The channel it was fetched for; `None` for an offline bundle (chosen by an
    /// administrator, any channel).
    pub channel: Option<String>,
}

/// What a fetch needs from its surroundings.
pub struct FetchEnv<'a> {
    /// Stop points (cancel, fault injection).
    pub control: &'a dyn Checkpoint,
    /// Rate limit.
    pub pacer: &'a mut dyn Pacer,
    /// Called with (bytes present, total bytes).
    pub progress: &'a mut dyn FnMut(u64, u64),
    /// Recorded in the marker: the bundle was admitted with a downgrade permission.
    pub allow_downgrade: bool,
    /// Recorded in the marker: the channel that was asked for (`None`: offline bundle).
    pub channel: Option<&'a str>,
}

/// The staging directory.
#[derive(Debug, Clone)]
pub struct Staging {
    dir: PathBuf,
}

fn marker_text(info: &StagedInfo) -> String {
    let downgrade = u8::from(info.allow_downgrade);
    let channel = info.channel.as_deref().unwrap_or("-");
    let digest = hex::encode(info.digest);
    format!("{digest} {} {downgrade} {channel}\n", info.size)
}

fn parse_marker(bytes: &[u8]) -> Option<StagedInfo> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut parts = text.strip_suffix('\n')?.split(' ');
    let mut digest = [0u8; 32];
    hex::decode_to_slice(parts.next()?, &mut digest).ok()?;
    let size = parts.next()?.parse().ok()?;
    let allow_downgrade = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let channel = match parts.next()? {
        "-" => None,
        name if valid_ident(name) => Some(name.to_string()),
        _ => return None,
    };
    let info = StagedInfo {
        digest,
        size,
        allow_downgrade,
        channel,
    };
    (parts.next().is_none() && marker_text(&info) == text).then_some(info)
}

impl Staging {
    /// The staging area in `dir` (created on first use).
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn part(&self) -> PathBuf {
        self.dir.join("bundle.part")
    }

    fn pos(&self) -> PathBuf {
        self.dir.join("bundle.pos")
    }

    fn done(&self) -> PathBuf {
        self.dir.join("bundle.uup")
    }

    fn marker(&self) -> PathBuf {
        self.dir.join("bundle.ok")
    }

    /// The complete bundle, if there is one whose marker matches its size.
    pub fn staged(&self) -> Result<Option<StagedInfo>, UpdateError> {
        let Some(bytes) = read_bounded(&self.marker(), 256).ok().flatten() else {
            return Ok(None);
        };
        let size = match segfile::len_of(&self.done()) {
            Ok(len) => len,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(parse_marker(&bytes).filter(|info| info.size == size))
    }

    /// Opens the complete bundle for reading from its start.
    pub fn open_staged(&self) -> Result<Option<(SegFile, StagedInfo)>, UpdateError> {
        match self.staged()? {
            Some(info) => Ok(Some((SegFile::open(&self.done())?, info))),
            None => Ok(None),
        }
    }

    /// Bytes of an unfinished download that are known to be on the medium.
    pub fn partial_bytes(&self) -> u64 {
        let recorded = read_bounded(&self.pos(), 32)
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| t.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let present = segfile::len_of(&self.part()).unwrap_or(0);
        recorded.min(present)
    }

    /// Removes everything in the staging area (also a single-file bundle of an older updater).
    pub fn purge(&self) -> Result<(), UpdateError> {
        for path in [self.marker(), self.done(), self.pos(), self.part()] {
            remove_if_present(&path)?;
        }
        segfile::remove(&self.done())?;
        segfile::remove(&self.part())?;
        if self.dir.exists() {
            sync_dir(&self.dir)?;
        }
        Ok(())
    }

    /// A finished file without marker (power loss between rename and marker) becomes the
    /// partial file again, so its bytes are verified once more instead of fetched once more.
    fn adopt_orphan(&self) -> Result<(), UpdateError> {
        remove_if_present(&self.marker())?;
        let Ok(len) = segfile::len_of(&self.done()) else {
            return Ok(());
        };
        if segfile::exists(&self.part()) {
            segfile::remove(&self.done())?;
        } else {
            write_atomic(&self.pos(), len.to_string().as_bytes())?;
            segfile::rename(&self.done(), &self.part())?;
        }
        sync_dir(&self.dir)?;
        Ok(())
    }

    /// Prepares `bundle.part` and returns it positioned at the offset to continue from.
    fn resume(&self, header: &Header, total: u64) -> Result<(SegFile, u64), UpdateError> {
        remove_if_present(&self.part())?; // a single-file part of an older updater
        let mut file = SegFile::open_rw(&self.part())?;
        let keep = self.partial_bytes().min(total);
        let mut same_header = keep >= header.len();
        if same_header {
            let mut present = vec![0u8; header.raw.len()];
            file.read_exact(&mut present)?;
            same_header = present == header.raw;
        }
        if !same_header {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&header.raw)?;
            file.sync_all()?;
            sync_dir(&self.dir)?;
            write_atomic(&self.pos(), header.len().to_string().as_bytes())?;
            return Ok((file, header.len()));
        }
        file.set_len(keep)?;
        Ok((file, keep))
    }

    fn record(&self, file: &mut SegFile, offset: u64) -> Result<(), UpdateError> {
        file.sync_data()?;
        write_atomic(&self.pos(), offset.to_string().as_bytes())?;
        Ok(())
    }

    /// Collects the bundle described by `header` / `verified` from `source` into the staging
    /// area, continuing an earlier attempt when its bytes still fit. On success the bundle is
    /// complete, every payload hash matched, and the completion marker is on the medium.
    ///
    /// Errors leave resumable state behind, except hash and size violations, which discard
    /// the staging area. No clean-up runs for [`UpdateError::Cancelled`] or
    /// [`UpdateError::Interrupted`].
    pub fn fetch(
        &self,
        header: &Header,
        verified: &VerifiedManifest,
        source: &mut dyn UpdateSource,
        remote: &Remote,
        env: &mut FetchEnv<'_>,
    ) -> Result<StagedInfo, UpdateError> {
        create_private_dir(&self.dir)?;
        let wanted = StagedInfo {
            digest: verified.digest(),
            size: header.len() + verified.manifest().payload_bytes(),
            allow_downgrade: env.allow_downgrade,
            channel: env.channel.map(str::to_string),
        };
        match self.staged()? {
            Some(info) if info == wanted => return Ok(info),
            Some(info) if info.digest == wanted.digest && info.size == wanted.size => {
                // Same verified bundle, admitted now under other conditions (channel,
                // downgrade permission): only the marker changes.
                write_atomic(&self.marker(), marker_text(&wanted).as_bytes())?;
                return Ok(wanted);
            }
            Some(_) => self.purge()?,
            None => self.adopt_orphan()?,
        }
        let outcome = self.transfer(header, verified, source, remote, env, &wanted);
        if matches!(
            outcome,
            Err(UpdateError::HashMismatch(_) | UpdateError::SizeExceeded)
        ) {
            self.purge()?;
        }
        outcome?;
        env.control.reached(Step::StageRename)?;
        segfile::rename(&self.part(), &self.done())?;
        sync_dir(&self.dir)?;
        env.control.reached(Step::StageMarker)?;
        write_atomic(&self.marker(), marker_text(&wanted).as_bytes())?;
        remove_if_present(&self.pos())?;
        Ok(wanted)
    }

    fn transfer(
        &self,
        header: &Header,
        verified: &VerifiedManifest,
        source: &mut dyn UpdateSource,
        remote: &Remote,
        env: &mut FetchEnv<'_>,
        wanted: &StagedInfo,
    ) -> Result<(), UpdateError> {
        let (mut file, mut offset) = self.resume(header, wanted.size)?;
        let mut check = PayloadCheck::new(verified.manifest());
        rehash(&mut file, header.len(), offset, &mut check)?;
        if offset == wanted.size {
            // Everything is here already (power loss just before the rename).
            return if check.complete() {
                Ok(file.sync_all()?)
            } else {
                Err(UpdateError::SizeExceeded)
            };
        }
        let (actual, mut reader) = source.open(remote, offset, None)?;
        if actual != offset {
            if actual != 0 {
                return Err(UpdateError::Network(NetworkError::Protocol));
            }
            // The source cannot resume: start again, but only for the same release.
            let mut again = vec![0u8; header.raw.len()];
            read_source(&mut *reader, &mut again, source.is_network())?;
            if again != header.raw {
                return Err(UpdateError::Network(NetworkError::Protocol));
            }
            offset = header.len();
            file.set_len(offset)?;
            check = PayloadCheck::new(verified.manifest());
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut copy = Copy {
            staging: self,
            file,
            offset,
            total: wanted.size,
            network: source.is_network(),
        };
        copy.run(&mut *reader, &mut check, env)
    }
}

fn read_source(reader: &mut dyn Read, buf: &mut [u8], network: bool) -> Result<(), UpdateError> {
    reader.read_exact(buf).map_err(|e| source_error(e, network))
}

fn source_error(error: io::Error, network: bool) -> UpdateError {
    if network {
        UpdateError::Network(NetworkError::Transport)
    } else {
        UpdateError::Io(error)
    }
}

/// Feeds the payload bytes already on disk (`from..to`) through the checker.
fn rehash(
    file: &mut SegFile,
    from: u64,
    to: u64,
    check: &mut PayloadCheck,
) -> Result<(), UpdateError> {
    file.seek(SeekFrom::Start(from))?;
    let mut left = to - from;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
        file.read_exact(&mut buf[..want])?;
        check.feed(&buf[..want])?;
        left -= want as u64;
    }
    Ok(())
}

struct Copy<'a> {
    staging: &'a Staging,
    file: SegFile,
    offset: u64,
    total: u64,
    network: bool,
}

impl Copy<'_> {
    fn run(
        &mut self,
        reader: &mut dyn Read,
        check: &mut PayloadCheck,
        env: &mut FetchEnv<'_>,
    ) -> Result<(), UpdateError> {
        let mut buf = vec![0u8; CHUNK];
        let mut unflushed = 0u64;
        (env.progress)(self.offset, self.total);
        while self.offset < self.total {
            env.control.reached(Step::DownloadChunk)?;
            let left = self.total - self.offset;
            let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
            let n = match reader.read(&mut buf[..want]) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.staging.record(&mut self.file, self.offset)?;
                    return Err(source_error(e, self.network));
                }
            };
            if n == 0 {
                self.staging.record(&mut self.file, self.offset)?;
                return Err(UpdateError::Truncated);
            }
            check.feed(&buf[..n])?;
            self.file.write_all(&buf[..n])?;
            self.offset += n as u64;
            unflushed += n as u64;
            env.pacer.consumed(n as u64);
            (env.progress)(self.offset, self.total);
            if unflushed >= FLUSH_EVERY {
                env.control.reached(Step::DownloadFlushed)?;
                self.staging.record(&mut self.file, self.offset)?;
                unflushed = 0;
            }
        }
        // The signed size is reached; a single further byte means the file is not the bundle
        // the manifest describes. (A read failure here is not held against the bundle: every
        // signed byte is present and matched.)
        let mut extra = [0u8; 1];
        if !check.complete() || matches!(reader.read(&mut extra), Ok(n) if n > 0) {
            return Err(UpdateError::SizeExceeded);
        }
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "stage_tests.rs"]
mod tests;
