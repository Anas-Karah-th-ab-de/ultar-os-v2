//! Which NPU a PCI device is, and the address ranges its firmware expects.

use crate::{Error, Result};

/// PCI vendor.
pub const VENDOR: u16 = 0x8086;
/// NPU of Meteor Lake and Arrow Lake-H/U.
pub const DEVICE_MTL: u16 = 0x7d1d;
/// NPU of Arrow Lake-S.
pub const DEVICE_ARL: u16 = 0xad1d;
/// NPU of Lunar Lake.
pub const DEVICE_LNL: u16 = 0x643e;
/// NPU of Panther Lake.
pub const DEVICE_PTL: u16 = 0xb03e;

/// A range of NPU addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// First address.
    pub start: u64,
    /// One past the last address.
    pub end: u64,
}

impl Range {
    const fn new(start: u64, size: u64) -> Range {
        Range {
            start,
            end: start + size,
        }
    }

    /// Whether `size` bytes at `addr` lie inside the range.
    pub fn holds(&self, addr: u64, size: u64) -> bool {
        addr >= self.start && addr.checked_add(size).is_some_and(|end| end <= self.end)
    }
}

/// An NPU model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    /// Marketing/engineering name.
    pub name: &'static str,
    /// IP generation (37 = NPU 3720).
    pub ip_gen: u32,
    /// What the powered IP block reports in the generation and SoC fields of its hardware
    /// version register, as one number (`generation << 8 | soc`). Read on the Beelink GTi15
    /// Ultra: `0x3630`, the two-tile SKU value Linux assumes for this generation.
    pub sku: u32,
    /// Firmware file name (linux-firmware, `intel/vpu/`).
    pub firmware: &'static str,
    /// Where the firmware image and its run-time data live.
    pub runtime: Range,
    /// Memory shared between driver and firmware (IPC, logs).
    pub global: Range,
    /// Buffers of user contexts.
    pub user: Range,
}

const NPU_3720: Model = Model {
    name: "NPU 3720",
    ip_gen: 37,
    sku: 0x3630,
    firmware: "vpu_37xx_v1.bin",
    runtime: Range::new(0x8480_0000, 64 << 20),
    global: Range::new(0x9000_0000, 256 << 20),
    user: Range::new(0xa000_0000, 511 << 20),
};

/// The model for a PCI device id. Later generations (Lunar Lake and newer) have another buttress
/// and another power sequence; they are known but not supported yet.
pub fn lookup(device: u16) -> Result<Model> {
    match device {
        DEVICE_MTL | DEVICE_ARL => Ok(NPU_3720),
        DEVICE_LNL | DEVICE_PTL => Err(Error::NotSupported("NPU generation 40xx and later")),
        d => Err(Error::UnknownDevice(d)),
    }
}

/// Whether [`lookup`] knows the id (supported or not), for the driver's match.
pub fn known(device: u16) -> bool {
    !matches!(lookup(device), Err(Error::UnknownDevice(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beelink_npu() {
        let m = lookup(0x7d1d).unwrap();
        assert_eq!((m.ip_gen, m.firmware), (37, "vpu_37xx_v1.bin"));
        assert_eq!(
            m.runtime,
            Range {
                start: 0x8480_0000,
                end: 0x8880_0000
            }
        );
        assert!(m.runtime.holds(0x8480_0000, 64 << 20));
        assert!(!m.runtime.holds(0x8480_0000, (64 << 20) + 1));
        assert!(!m.runtime.holds(0x8000_0000, 4096));
        assert!(!m.runtime.holds(u64::MAX, 2));
    }

    #[test]
    fn other_ids() {
        assert!(known(DEVICE_LNL) && lookup(DEVICE_LNL).is_err());
        assert!(known(DEVICE_ARL) && lookup(DEVICE_ARL).is_ok());
        assert_eq!(lookup(0x1234), Err(Error::UnknownDevice(0x1234)));
        assert!(!known(0x1234));
    }
}
