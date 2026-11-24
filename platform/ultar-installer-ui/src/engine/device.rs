//! Block devices: a disk, loop device or image file, opened read-only unless the installation
//! runs.

use super::EngineError;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// A device read and written at byte offsets.
pub trait BlockDevice {
    /// Size in bytes.
    fn len(&self) -> u64;
    /// `true` when the device has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Reads `buf.len()` bytes at `offset`.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), EngineError>;
    /// Writes `buf` at `offset`.
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> Result<(), EngineError>;
    /// Makes the written data durable.
    fn flush(&mut self) -> Result<(), EngineError>;
}

/// A device backed by a file descriptor.
#[derive(Debug)]
pub struct FileDevice {
    file: File,
    len: u64,
    writable: bool,
}

fn io(op: &'static str) -> impl Fn(std::io::Error) -> EngineError {
    move |e| EngineError::Io(op, e.kind())
}

impl FileDevice {
    fn open(path: &Path, writable: bool) -> Result<FileDevice, EngineError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(writable)
            .open(path)
            .map_err(io("open"))?;
        let len = file.seek(SeekFrom::End(0)).map_err(io("size"))?;
        Ok(FileDevice {
            file,
            len,
            writable,
        })
    }

    /// Opens for reading only (inspection, the medium).
    pub fn open_read(path: &Path) -> Result<FileDevice, EngineError> {
        FileDevice::open(path, false)
    }

    /// Opens for writing – only [`super::write::execute`] calls this.
    pub(crate) fn open_write(path: &Path) -> Result<FileDevice, EngineError> {
        FileDevice::open(path, true)
    }
}

impl BlockDevice for FileDevice {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), EngineError> {
        if offset.saturating_add(buf.len() as u64) > self.len {
            return Err(EngineError::Io("read", ErrorKind::UnexpectedEof));
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(io("seek"))?;
        self.file.read_exact(buf).map_err(io("read"))
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> Result<(), EngineError> {
        if !self.writable {
            return Err(EngineError::Io("write", ErrorKind::PermissionDenied));
        }
        if offset.saturating_add(buf.len() as u64) > self.len {
            return Err(EngineError::Io("write", ErrorKind::UnexpectedEof));
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(io("seek"))?;
        self.file.write_all(buf).map_err(io("write"))
    }

    fn flush(&mut self) -> Result<(), EngineError> {
        self.file.sync_all().map_err(io("flush"))
    }
}
