//! A file of any length kept as numbered segments of at most [`SEGMENT`] bytes
//! (`<base>.000`, `<base>.001`, …).
//!
//! The Ultar kernel holds a file that is being written in memory until it is stored and
//! refuses writable files above 64 MiB (`EFBIG`); a system update bundle is several hundred
//! megabytes. Kept in segments, no single file ever comes near that limit, and every
//! segment is stored (`fsync`) before the next one is begun. The segments form one byte
//! stream: only a contiguous run from `.000` counts, and every segment before the last is
//! exactly [`SEGMENT`] bytes long.

use crate::fsutil::{remove_if_present, sync_dir};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Size of one segment (far below the kernel's 64 MiB limit for a written file).
pub const SEGMENT: u64 = 16 * 1024 * 1024;
/// Most segments of one stream (three digits: 16 GiB).
const MAX_SEGMENTS: usize = 1000;

/// The path of segment `index` of `base`.
fn segment(base: &Path, index: usize) -> PathBuf {
    let mut name = base.as_os_str().to_owned();
    name.push(format!(".{index:03}"));
    PathBuf::from(name)
}

/// Lengths of the contiguous segments of `base` (a short segment ends the stream).
fn lengths(base: &Path) -> io::Result<Vec<u64>> {
    let mut out = Vec::new();
    for index in 0..MAX_SEGMENTS {
        let len = match fs::metadata(segment(base, index)) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        };
        if len > SEGMENT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "segment too long",
            ));
        }
        out.push(len);
        if len < SEGMENT {
            break;
        }
    }
    Ok(out)
}

/// Length of the stream `base`; `NotFound` when it has no first segment.
pub fn len_of(base: &Path) -> io::Result<u64> {
    if !segment(base, 0).exists() {
        return Err(io::ErrorKind::NotFound.into());
    }
    Ok(lengths(base)?.iter().sum())
}

/// True when `base` has a first segment.
pub fn exists(base: &Path) -> bool {
    segment(base, 0).exists()
}

/// Removes every segment of `base` (also ones beyond a gap).
pub fn remove(base: &Path) -> io::Result<()> {
    for index in 0..MAX_SEGMENTS {
        let path = segment(base, index);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Renames the stream `from` to `to` (segment by segment, the last one first: a power loss in
/// between leaves `from` with its first segments, which [`len_of`] still reads as a shorter
/// stream).
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    remove(to)?;
    let count = lengths(from)?.len();
    for index in (0..count).rev() {
        fs::rename(segment(from, index), segment(to, index))?;
    }
    remove(from)
}

/// An open segmented stream.
#[derive(Debug)]
pub struct SegFile {
    base: PathBuf,
    writable: bool,
    len: u64,
    pos: u64,
    /// The segment that is open, and whether it was written since it was stored.
    current: Option<(usize, File, bool)>,
}

impl SegFile {
    /// Opens the stream `base` for reading; `NotFound` when it has no first segment.
    pub fn open(base: &Path) -> io::Result<Self> {
        let len = len_of(base)?;
        Ok(Self::new(base, false, len))
    }

    /// Opens (or begins) the stream `base` for reading and writing, without truncating it.
    /// Segments after the contiguous run are removed.
    pub fn open_rw(base: &Path) -> io::Result<Self> {
        let lens = lengths(base)?;
        for index in lens.len()..MAX_SEGMENTS {
            remove_if_present(&segment(base, index))?;
        }
        Ok(Self::new(base, true, lens.iter().sum()))
    }

    fn new(base: &Path, writable: bool, len: u64) -> Self {
        Self {
            base: base.to_path_buf(),
            writable,
            len,
            pos: 0,
            current: None,
        }
    }

    /// The stream's length.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// True when the stream holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Stores the open segment if it was written.
    fn store(&mut self) -> io::Result<()> {
        if let Some((_, file, dirty)) = &mut self.current {
            if *dirty {
                file.sync_data()?;
                *dirty = false;
            }
        }
        Ok(())
    }

    /// The open segment `index` (stores and closes another one first).
    fn at(&mut self, index: usize) -> io::Result<&mut File> {
        if !matches!(&self.current, Some((open, ..)) if *open == index) {
            self.store()?;
            self.current = None;
            let path = segment(&self.base, index);
            let fresh = self.writable && !path.exists();
            let file = if self.writable {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&path)?
            } else {
                File::open(&path)?
            };
            if fresh {
                if let Some(dir) = self.base.parent() {
                    sync_dir(dir)?;
                }
            }
            self.current = Some((index, file, false));
        }
        match &mut self.current {
            Some((_, file, _)) => Ok(file),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    /// Shortens the stream to `len` bytes (it never grows here).
    pub fn set_len(&mut self, len: u64) -> io::Result<()> {
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if len > self.len {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a stream only shrinks",
            ));
        }
        self.store()?;
        self.current = None;
        let keep = usize::try_from(len.div_ceil(SEGMENT)).unwrap_or(MAX_SEGMENTS);
        for index in keep..MAX_SEGMENTS {
            remove_if_present(&segment(&self.base, index))?;
        }
        if keep > 0 {
            let last = (keep - 1) as u64;
            let file = OpenOptions::new()
                .write(true)
                .open(segment(&self.base, keep - 1))?;
            file.set_len(len - last * SEGMENT)?;
            file.sync_all()?;
        }
        if let Some(dir) = self.base.parent() {
            sync_dir(dir)?;
        }
        self.len = len;
        Ok(())
    }

    /// Stores what was written (the open segment; earlier ones were stored when left).
    pub fn sync_data(&mut self) -> io::Result<()> {
        self.store()
    }

    /// Same as [`SegFile::sync_data`]: segments carry no metadata of their own that matters.
    pub fn sync_all(&mut self) -> io::Result<()> {
        self.store()
    }

    fn split(&self) -> (usize, u64) {
        let index = usize::try_from(self.pos / SEGMENT).unwrap_or(usize::MAX);
        (index, self.pos % SEGMENT)
    }
}

impl Read for SegFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let (index, within) = self.split();
        let room = (SEGMENT - within).min(self.len - self.pos);
        let want = usize::try_from(room).map_or(buf.len(), |r| r.min(buf.len()));
        let file = self.at(index)?;
        file.seek(SeekFrom::Start(within))?;
        let n = file.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Write for SegFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if self.pos > self.len {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no holes in a stream",
            ));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let (index, within) = self.split();
        if index >= MAX_SEGMENTS {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "stream too long",
            ));
        }
        let want = usize::try_from(SEGMENT - within).map_or(buf.len(), |r| r.min(buf.len()));
        let file = self.at(index)?;
        file.seek(SeekFrom::Start(within))?;
        let n = file.write(&buf[..want])?;
        if let Some((_, _, dirty)) = &mut self.current {
            *dirty = true;
        }
        self.pos += n as u64;
        self.len = self.len.max(self.pos);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for SegFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = pos.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

impl Drop for SegFile {
    fn drop(&mut self) {
        let _ = self.store();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-segfile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn a_long_stream_is_kept_in_segments_none_above_the_limit() {
        let d = dir("long");
        let base = d.join("bundle.part");
        let data = pattern(2 * SEGMENT as usize + 12_345);
        let mut f = SegFile::open_rw(&base).unwrap();
        for chunk in data.chunks(64 * 1024 + 3) {
            f.write_all(chunk).unwrap();
        }
        f.sync_all().unwrap();
        drop(f);
        assert_eq!(len_of(&base).unwrap(), data.len() as u64);
        for index in 0..3 {
            let len = fs::metadata(segment(&base, index)).unwrap().len();
            assert!(len <= SEGMENT);
        }
        assert!(!segment(&base, 3).exists());
        let mut back = Vec::new();
        SegFile::open(&base)
            .unwrap()
            .read_to_end(&mut back)
            .unwrap();
        assert_eq!(back, data);
        // reading across a segment border from the middle
        let mut f = SegFile::open(&base).unwrap();
        f.seek(SeekFrom::Start(SEGMENT - 5)).unwrap();
        let mut ten = [0u8; 10];
        f.read_exact(&mut ten).unwrap();
        assert_eq!(&ten[..], &data[SEGMENT as usize - 5..SEGMENT as usize + 5]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn shortening_removes_later_segments_and_writing_continues_at_the_end() {
        let d = dir("short");
        let base = d.join("s");
        let data = pattern(SEGMENT as usize + 100);
        let mut f = SegFile::open_rw(&base).unwrap();
        f.write_all(&data).unwrap();
        f.set_len(SEGMENT).unwrap();
        assert!(!segment(&base, 1).exists());
        assert_eq!(f.len(), SEGMENT);
        f.set_len(10).unwrap();
        f.seek(SeekFrom::End(0)).unwrap();
        f.write_all(b"xyz").unwrap();
        drop(f);
        let mut back = Vec::new();
        SegFile::open(&base)
            .unwrap()
            .read_to_end(&mut back)
            .unwrap();
        assert_eq!(&back[..10], &data[..10]);
        assert_eq!(&back[10..], b"xyz");
        let mut f = SegFile::open_rw(&base).unwrap();
        f.set_len(0).unwrap();
        assert!(!exists(&base));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rename_moves_every_segment_and_a_gap_ends_the_stream() {
        let d = dir("rename");
        let (a, b) = (d.join("a"), d.join("b"));
        let data = pattern(SEGMENT as usize + 7);
        let mut f = SegFile::open_rw(&a).unwrap();
        f.write_all(&data).unwrap();
        drop(f);
        fs::write(segment(&b, 4), b"stale").unwrap();
        rename(&a, &b).unwrap();
        assert!(!exists(&a));
        assert!(!segment(&b, 4).exists());
        assert_eq!(len_of(&b).unwrap(), data.len() as u64);
        // a stray segment after a gap is not part of the stream and goes on opening for writing
        fs::write(segment(&b, 5), b"stray").unwrap();
        assert_eq!(len_of(&b).unwrap(), data.len() as u64);
        drop(SegFile::open_rw(&b).unwrap());
        assert!(!segment(&b, 5).exists());
        assert!(SegFile::open(&d.join("none")).is_err());
        remove(&b).unwrap();
        assert!(!exists(&b));
        let _ = fs::remove_dir_all(&d);
    }
}
