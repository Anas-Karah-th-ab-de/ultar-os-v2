//! After the copy: the data file system grows to its partition (`ultar-ext4grow`), through a
//! view of the partition – plain, or decrypted behind the LUKS2 metadata.

use super::device::BlockDevice;
use super::EngineError;
use ultar_ext4grow::{Disk, Outcome};

fn io(e: EngineError) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// The partition from `base` on, plain.
struct Plain<'a> {
    dev: &'a mut dyn BlockDevice,
    base: u64,
}

impl Disk for Plain<'_> {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.dev.read_at(self.base + off, buf).map_err(io)
    }
    fn write_at(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        self.dev.write_at(self.base + off, buf).map_err(io)
    }
}

/// The encrypted data segment from `base` on, decrypted (sector numbers from its start).
struct Sealed<'a> {
    dev: &'a mut dyn BlockDevice,
    base: u64,
    xts: &'a ultar_luks::Sectors,
}

impl Disk for Sealed<'_> {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.dev.read_at(self.base + off, buf).map_err(io)?;
        self.xts.decrypt(off / 512, buf);
        Ok(())
    }
    fn write_at(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let mut sealed = zeroize::Zeroizing::new(buf.to_vec());
        self.xts.encrypt(off / 512, &mut sealed);
        self.dev.write_at(self.base + off, &sealed).map_err(io)
    }
}

/// Grows the file system at `base` (`bytes` long) on `dst`; `xts`: it lies in a LUKS2 data
/// segment. A layout the growth does not handle stays as it is.
pub fn data(
    dst: &mut dyn BlockDevice,
    base: u64,
    bytes: u64,
    xts: Option<&ultar_luks::Sectors>,
) -> Result<Outcome, EngineError> {
    let r = match xts {
        None => ultar_ext4grow::grow(&mut Plain { dev: dst, base }, bytes),
        Some(xts) => ultar_ext4grow::grow(
            &mut Sealed {
                dev: dst,
                base,
                xts,
            },
            bytes,
        ),
    };
    r.map_err(|e| EngineError::Io("grow", e.0.kind()))
}
