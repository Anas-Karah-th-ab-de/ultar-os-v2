//! A small, bounded ZIP reader for APK files (stored and deflated entries only).
//!
//! Every size is checked before anything is allocated or read: the central directory,
//! names, entries and the decompressed data each have their own limit, and nothing
//! claimed by the file is trusted beyond the file's own length.

use std::io::{Read, Seek, SeekFrom};

/// Largest central directory read.
const MAX_CENTRAL: u64 = 16 << 20;
/// Most entries of one archive.
const MAX_ENTRIES: usize = 100_000;
/// Longest entry name.
const MAX_NAME: usize = 1024;
/// Longest EOCD comment the reader looks behind.
const MAX_COMMENT: u64 = 0xffff;
/// Size of the end-of-central-directory record without its comment.
pub const EOCD_SIZE: u64 = 22;

/// Why an archive or an entry cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipError {
    /// Reading the file failed.
    Io,
    /// Not a ZIP file, or its structure is inconsistent.
    Format,
    /// A feature this reader does not take (ZIP64, encryption, other compression).
    Unsupported,
    /// Larger than the caller's limit.
    TooLarge,
}

impl From<std::io::Error> for ZipError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

/// One entry of the central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name (`/`-separated path inside the archive).
    pub name: String,
    method: u16,
    flags: u16,
    compressed: u64,
    /// Size after decompression (as the directory says).
    pub size: u64,
    local: u64,
}

/// An opened archive: where its parts are and what it holds.
#[derive(Debug)]
pub struct Archive<R> {
    file: R,
    /// Length of the whole file.
    pub len: u64,
    /// Offset of the central directory.
    pub central: u64,
    /// Offset of the end-of-central-directory record.
    pub eocd: u64,
    /// The entries.
    pub entries: Vec<Entry>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, ZipError> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(ZipError::Format)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ZipError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(ZipError::Format)
}

/// Reads exactly `len` bytes at `at`.
pub fn read_at<R: Read + Seek>(file: &mut R, at: u64, len: usize) -> Result<Vec<u8>, ZipError> {
    file.seek(SeekFrom::Start(at))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Finds the end-of-central-directory record: the last signature whose comment ends
/// exactly at the end of the file.
fn find_eocd<R: Read + Seek>(file: &mut R, len: u64) -> Result<u64, ZipError> {
    if len < EOCD_SIZE {
        return Err(ZipError::Format);
    }
    let tail = (EOCD_SIZE + MAX_COMMENT).min(len);
    let start = len - tail;
    let buf = read_at(file, start, tail as usize)?;
    let last = buf.len() - EOCD_SIZE as usize;
    for i in (0..=last).rev() {
        if buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment = u16_at(&buf, i + 20)? as usize;
            if i + EOCD_SIZE as usize + comment == buf.len() {
                return Ok(start + i as u64);
            }
        }
    }
    Err(ZipError::Format)
}

fn parse_entry(cd: &[u8], at: usize) -> Result<(Entry, usize), ZipError> {
    if u32_at(cd, at)? != 0x0201_4b50 {
        return Err(ZipError::Format);
    }
    let flags = u16_at(cd, at + 8)?;
    let method = u16_at(cd, at + 10)?;
    let compressed = u32_at(cd, at + 20)?;
    let size = u32_at(cd, at + 24)?;
    let name_len = u16_at(cd, at + 28)? as usize;
    let extra = u16_at(cd, at + 30)? as usize;
    let comment = u16_at(cd, at + 32)? as usize;
    let local = u32_at(cd, at + 42)?;
    if [compressed, size, local].contains(&u32::MAX) {
        return Err(ZipError::Unsupported); // ZIP64
    }
    if name_len == 0 || name_len > MAX_NAME {
        return Err(ZipError::Format);
    }
    let name_bytes = cd
        .get(at + 46..at + 46 + name_len)
        .ok_or(ZipError::Format)?;
    let name = std::str::from_utf8(name_bytes).map_err(|_| ZipError::Format)?;
    if name.contains('\0') {
        return Err(ZipError::Format);
    }
    let next = at + 46 + name_len + extra + comment;
    if next > cd.len() {
        return Err(ZipError::Format);
    }
    let entry = Entry {
        name: name.to_owned(),
        method,
        flags,
        compressed: compressed.into(),
        size: size.into(),
        local: local.into(),
    };
    Ok((entry, next))
}

impl<R: Read + Seek> Archive<R> {
    /// Opens `file`: finds its directory and reads every entry's header.
    pub fn open(mut file: R) -> Result<Self, ZipError> {
        let len = file.seek(SeekFrom::End(0))?;
        let eocd = find_eocd(&mut file, len)?;
        let rec = read_at(&mut file, eocd, EOCD_SIZE as usize)?;
        let (disk, cd_disk) = (u16_at(&rec, 4)?, u16_at(&rec, 6)?);
        let (here, total) = (u16_at(&rec, 8)?, u16_at(&rec, 10)?);
        let cd_size = u64::from(u32_at(&rec, 12)?);
        let central = u64::from(u32_at(&rec, 16)?);
        if disk != 0 || cd_disk != 0 || here != total {
            return Err(ZipError::Unsupported);
        }
        if central == u64::from(u32::MAX) || total == u16::MAX {
            return Err(ZipError::Unsupported); // ZIP64
        }
        if cd_size > MAX_CENTRAL || central.checked_add(cd_size) != Some(eocd) {
            return Err(ZipError::Format);
        }
        let cd = read_at(&mut file, central, cd_size as usize)?;
        let mut entries = Vec::with_capacity(usize::from(total).min(MAX_ENTRIES));
        let mut at = 0;
        while at < cd.len() {
            if entries.len() >= MAX_ENTRIES {
                return Err(ZipError::TooLarge);
            }
            let (entry, next) = parse_entry(&cd, at)?;
            if entry.local >= central {
                return Err(ZipError::Format);
            }
            entries.push(entry);
            at = next;
        }
        if entries.len() != usize::from(total) {
            return Err(ZipError::Format);
        }
        Ok(Self {
            file,
            len,
            central,
            eocd,
            entries,
        })
    }

    /// The entry named `name`.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Reads `len` raw bytes of the file at `at` (signing block, directory).
    pub fn raw(&mut self, at: u64, len: usize) -> Result<Vec<u8>, ZipError> {
        if at.checked_add(len as u64).is_none_or(|end| end > self.len) {
            return Err(ZipError::Format);
        }
        read_at(&mut self.file, at, len)
    }

    /// The decompressed data of `entry`, refused when larger than `max`.
    pub fn read(&mut self, entry: &Entry, max: usize) -> Result<Vec<u8>, ZipError> {
        if entry.flags & 1 != 0 {
            return Err(ZipError::Unsupported); // encrypted
        }
        if entry.size > max as u64 || entry.compressed > self.central {
            return Err(ZipError::TooLarge);
        }
        let head = self.raw(entry.local, 30)?;
        if u32_at(&head, 0)? != 0x0403_4b50 {
            return Err(ZipError::Format);
        }
        let data_at =
            entry.local + 30 + u64::from(u16_at(&head, 26)?) + u64::from(u16_at(&head, 28)?);
        if data_at
            .checked_add(entry.compressed)
            .is_none_or(|end| end > self.central)
        {
            return Err(ZipError::Format);
        }
        let raw = self.raw(data_at, entry.compressed as usize)?;
        let data = match entry.method {
            0 => raw,
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, max)
                .map_err(|_| ZipError::Format)?,
            _ => return Err(ZipError::Unsupported),
        };
        if data.len() as u64 != entry.size {
            return Err(ZipError::Format);
        }
        Ok(data)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A ZIP of `files` (stored when `deflate` is false), as a test fixture.
    pub fn build(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, data) in files {
            let (method, body): (u16, Vec<u8>) = if deflate {
                (8, miniz_oxide::deflate::compress_to_vec(data, 6))
            } else {
                (0, data.to_vec())
            };
            let local = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&body);
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&[0; 8]);
            cd.extend_from_slice(&(body.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&local.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let central = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&central.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn stored_and_deflated_entries_are_read_within_their_limits() {
        for deflate in [false, true] {
            let zip = build(
                &[("a.txt", b"hello"), ("lib/x86_64/l.so", &[7u8; 3000])],
                deflate,
            );
            let mut a = Archive::open(Cursor::new(zip)).unwrap();
            assert_eq!(a.entries.len(), 2);
            let e = a.entry("lib/x86_64/l.so").unwrap().clone();
            assert_eq!(a.read(&e, 4096).unwrap(), vec![7u8; 3000]);
            assert_eq!(a.read(&e, 100), Err(ZipError::TooLarge));
            let t = a.entry("a.txt").unwrap().clone();
            assert_eq!(a.read(&t, 10).unwrap(), b"hello");
        }
    }

    #[test]
    fn broken_archives_are_refused() {
        assert!(Archive::open(Cursor::new(b"not a zip at all".to_vec())).is_err());
        let zip = build(&[("a", b"x")], false);
        // directory offset pointing elsewhere
        let mut bad = zip.clone();
        let n = bad.len();
        bad[n - 6] ^= 1;
        assert!(Archive::open(Cursor::new(bad)).is_err());
        // a trailing byte after the record (the comment length does not match)
        let mut tail = zip.clone();
        tail.push(0);
        assert!(Archive::open(Cursor::new(tail)).is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let zip = build(&[("AndroidManifest.xml", &[1u8; 200]), ("b", b"yy")], true);
        let mut seed = 0x1234_5678_u64;
        for _ in 0..3000 {
            let mut copy = zip.clone();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let at = (seed as usize) % copy.len();
                copy[at] = (seed >> 32) as u8;
            }
            if let Ok(mut a) = Archive::open(Cursor::new(copy)) {
                let entries = a.entries.clone();
                for e in &entries {
                    let _ = a.read(e, 4096);
                }
            }
        }
    }
}
