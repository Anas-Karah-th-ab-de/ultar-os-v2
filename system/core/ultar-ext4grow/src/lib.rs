//! # ultar-ext4grow – grow an unmounted ext4 to its partition
//!
//! The installer copies the medium's data file system (a few hundred MiB) into a data
//! partition that takes the rest of the disk; this crate then grows the file system to the
//! partition, offline, the way `resize2fs` does: the last group is filled, new block groups
//! are added (bitmaps and inode table inside each new group, `BLOCK_UNINIT`/`INODE_UNINIT`,
//! so nothing has to be zeroed), reserved descriptor blocks become descriptor blocks, the
//! resize inode is rebuilt, superblock and descriptors are written to every backup group.
//!
//! It only grows layouts it fully understands (what `mke2fs -t ext4` writes: sparse_super,
//! resize_inode, metadata_csum, flex_bg optional, 32 or 64 bit); anything else is left as it
//! is ([`Outcome::Unchanged`]) – never half grown. The growth is bounded by the reserved
//! descriptor blocks (1024 times the original size with mke2fs defaults).
//!
//! The device is a [`Disk`]: plain partition or the decrypted view of a LUKS2 volume.

mod grow;
mod layout;

pub use grow::grow;

/// Byte access to the file system (offsets from its start; always whole 512-byte sectors).
pub trait Disk {
    /// Reads `buf.len()` bytes at `off`.
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()>;
    /// Writes `buf` at `off`.
    fn write_at(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()>;
}

/// What [`grow`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Grown from `from` to `to` blocks.
    Grown { from: u64, to: u64 },
    /// Left as it is (why).
    Unchanged(&'static str),
}

/// Reading or writing the device failed (the file system may be partly grown only when a
/// write failed; the caller treats the installation as failed).
#[derive(Debug)]
pub struct Error(pub std::io::Error);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ext4 grow: {}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error(e)
    }
}

/// A file as [`Disk`] (images, tests).
impl Disk for std::fs::File {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, off)
    }
    fn write_at(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        std::os::unix::fs::FileExt::write_all_at(self, buf, off)
    }
}

/// CRC32C (Castagnoli) without final inversion, as ext4 uses it.
pub(crate) fn crc32c(mut crc: u32, data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0x82F6_3B78
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    for &b in data {
        crc = t[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc
}

pub(crate) fn le16(b: &[u8], o: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[o], b[o + 1]]))
}

pub(crate) fn le32(b: &[u8], o: usize) -> u64 {
    u64::from(u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]))
}

pub(crate) fn put16(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
}

pub(crate) fn put32(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 4].copy_from_slice(&(v as u32).to_le_bytes());
}
