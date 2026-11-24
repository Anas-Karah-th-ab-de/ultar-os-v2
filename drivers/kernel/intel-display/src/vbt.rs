//! Video BIOS Table (VBT): which outputs the board actually wires, as the firmware describes them.
//! Only the child-device list of the "general definitions" block is read – enough to know the
//! connectors (DP, HDMI, both), their port, AUX channel and DDC pin.
//!
//! Layout as in Linux' `intel_vbt_defs.h` (MIT); every length is checked, a damaged table gives an
//! error, never a guess.

use crate::error::{Error, Result};
use crate::regs::port::PORT_TC1;
use alloc::vec::Vec;

/// One wired output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// Port index (0 = A, 1 = B, 3..=6 = TC1..TC4).
    pub port: u8,
    /// DisplayPort signalling.
    pub dp: bool,
    /// HDMI/DVI (TMDS) signalling.
    pub hdmi: bool,
    /// GMBUS pin for DDC (0 = none).
    pub ddc_pin: u8,
}

const DEVICE_TYPE_DISPLAYPORT_OUTPUT: u16 = 1 << 2;
const DEVICE_TYPE_TMDS_DVI_SIGNALING: u16 = 1 << 4;
const DEVICE_TYPE_NOT_HDMI_OUTPUT: u16 = 1 << 11;

fn u16_at(d: &[u8], at: usize) -> Result<u16> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Invalid("VBT truncated"))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Invalid("VBT truncated"))
}

/// Port of a VBT "DVO port" value on display 13+ (`None` for ports this driver does not drive).
pub fn dvo_port_to_port(dvo: u8) -> Option<u8> {
    match dvo {
        0 | 10 => Some(0),             // HDMI A / DP A
        1 | 7 => Some(1),              // HDMI B / DP B
        14 | 13 => Some(PORT_TC1),     // HDMI F / DP F = TC1
        16 | 15 => Some(PORT_TC1 + 1), // TC2
        18 | 17 => Some(PORT_TC1 + 2), // TC3
        20 | 19 => Some(PORT_TC1 + 3), // TC4
        _ => None,
    }
}

/// The general-definitions block (id 2) of a VBT image.
fn general_definitions(vbt: &[u8]) -> Result<&[u8]> {
    if vbt.get(0..4) != Some(b"$VBT") {
        return Err(Error::Invalid("VBT signature"));
    }
    let bdb_off = u32_at(vbt, 28)? as usize;
    let bdb = vbt.get(bdb_off..).ok_or(Error::Invalid("VBT truncated"))?;
    if bdb.get(0..16) != Some(b"BIOS_DATA_BLOCK ") {
        return Err(Error::Invalid("BDB signature"));
    }
    let header = u16_at(bdb, 18)? as usize;
    let size = (u16_at(bdb, 20)? as usize).min(bdb.len());
    let mut p = header;
    while p + 3 <= size {
        let id = bdb[p];
        let len = u16_at(bdb, p + 1)? as usize;
        let data = bdb
            .get(p + 3..p + 3 + len)
            .ok_or(Error::Invalid("VBT block truncated"))?;
        if id == 2 {
            return Ok(data);
        }
        p += 3 + len;
    }
    Err(Error::Invalid("VBT without general definitions"))
}

/// The wired outputs of a VBT image (display 13+ port mapping).
pub fn outputs(vbt: &[u8]) -> Result<Vec<Output>> {
    let gd = general_definitions(vbt)?;
    let child_size = *gd.get(4).ok_or(Error::Invalid("VBT truncated"))? as usize;
    if child_size < 28 {
        return Err(Error::Invalid("VBT child size"));
    }
    let mut out: Vec<Output> = Vec::new();
    for child in gd[5..].chunks_exact(child_size) {
        let device_type = u16_at(child, 2)?;
        if device_type == 0 {
            continue;
        }
        let Some(port) = dvo_port_to_port(child[16]) else {
            continue;
        };
        let dp = device_type & DEVICE_TYPE_DISPLAYPORT_OUTPUT != 0;
        let hdmi = device_type & DEVICE_TYPE_TMDS_DVI_SIGNALING != 0
            && device_type & DEVICE_TYPE_NOT_HDMI_OUTPUT == 0;
        if !dp && !hdmi || out.iter().any(|o| o.port == port) {
            continue;
        }
        out.push(Output {
            port,
            dp,
            hdmi,
            ddc_pin: child[19],
        });
    }
    Ok(out)
}

/// Largest raw VBT accepted.
pub const RAW_VBT_MAX: u32 = 64 * 1024;
/// How far from the OpRegion a raw VBT may lie.
const RAW_VBT_REACH: u64 = 16 << 20;
/// Highest physical address taken from firmware fields (46-bit physical address width).
const PHYS_LIMIT: u64 = 1 << 46;

/// Physical address and size of the raw VBT that mailbox 3 of the OpRegion at `asls` names
/// (`rvda`, `rvds`; OpRegion `version` 2.1+ relative to the OpRegion, 2.0 absolute). Firmware
/// values are checked: the image must lie within 16 MiB of the OpRegion, below the physical
/// address limit, and be at most [`RAW_VBT_MAX`] bytes; no arithmetic can overflow.
pub fn raw_vbt_range(asls: u64, version: u16, rvda: u64, rvds: u32) -> Option<(u64, usize)> {
    if rvda == 0 || rvds == 0 || rvds > RAW_VBT_MAX {
        return None;
    }
    let addr = if version >= 0x0201 {
        asls.checked_add(rvda)?
    } else {
        rvda
    };
    let end = addr.checked_add(rvds as u64)?;
    if end > PHYS_LIMIT || addr.abs_diff(asls) > RAW_VBT_REACH {
        return None;
    }
    Some((addr, rvds as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// A VBT image with the Beelink GTi15 Ultra's four children (as captured under Linux).
    pub(crate) fn beelink_vbt() -> Vec<u8> {
        let children: [[u8; 40]; 4] = [
            child(0x40, 0x68c6, 13, 3, 0x60),
            child(0x20, 0x68c6, 15, 4, 0x70),
            child(0x10, 0x60d2, 18, 5, 0x00),
            child(0x02, 0x60d6, 19, 6, 0x90),
        ];
        let mut gd = vec![0x02, 0x04, 0x00, 0x00, 40];
        gd.extend_from_slice(&[0u8; 120]); // three empty children first, as on the machine
        for c in children {
            gd.extend_from_slice(&c);
        }
        let mut bdb = b"BIOS_DATA_BLOCK ".to_vec();
        bdb.extend_from_slice(&258u16.to_le_bytes());
        bdb.extend_from_slice(&22u16.to_le_bytes());
        let total = 22 + 3 + 4 + 3 + gd.len();
        bdb.extend_from_slice(&(total as u16).to_le_bytes());
        bdb.push(1); // block 1, 4 bytes
        bdb.extend_from_slice(&4u16.to_le_bytes());
        bdb.extend_from_slice(&[0; 4]);
        bdb.push(2);
        bdb.extend_from_slice(&(gd.len() as u16).to_le_bytes());
        bdb.extend_from_slice(&gd);
        let mut vbt = b"$VBT METEORLAKE     ".to_vec();
        vbt.resize(28, 0);
        vbt.extend_from_slice(&48u32.to_le_bytes());
        vbt.resize(48, 0);
        vbt.extend_from_slice(&bdb);
        vbt
    }

    fn child(handle: u16, ty: u16, dvo: u8, ddc: u8, aux: u8) -> [u8; 40] {
        let mut c = [0u8; 40];
        c[0..2].copy_from_slice(&handle.to_le_bytes());
        c[2..4].copy_from_slice(&ty.to_le_bytes());
        c[16] = dvo;
        c[19] = ddc;
        c[25] = aux;
        c
    }

    #[test]
    fn beelink_outputs() {
        let o = outputs(&beelink_vbt()).unwrap();
        assert_eq!(
            o,
            vec![
                Output {
                    port: 3,
                    dp: true,
                    hdmi: false,
                    ddc_pin: 3
                },
                Output {
                    port: 4,
                    dp: true,
                    hdmi: false,
                    ddc_pin: 4
                },
                Output {
                    port: 5,
                    dp: false,
                    hdmi: true,
                    ddc_pin: 5
                },
                Output {
                    port: 6,
                    dp: true,
                    hdmi: true,
                    ddc_pin: 6
                },
            ]
        );
    }

    #[test]
    fn damaged_tables_are_refused() {
        let v = beelink_vbt();
        assert_eq!(outputs(&v[..30]), Err(Error::Invalid("VBT truncated")));
        assert_eq!(outputs(b"$XYZ"), Err(Error::Invalid("VBT signature")));
        let mut bad = v.clone();
        bad[48] = b'X';
        assert_eq!(outputs(&bad), Err(Error::Invalid("BDB signature")));
        let cut = &v[..v.len() - 50];
        assert_eq!(outputs(cut), Err(Error::Invalid("VBT block truncated")));
        let mut small = v.clone();
        let gd_start = 48 + 22 + 3 + 4 + 3;
        small[gd_start + 4] = 8;
        assert_eq!(outputs(&small), Err(Error::Invalid("VBT child size")));
    }

    #[test]
    fn port_mapping() {
        assert_eq!(dvo_port_to_port(10), Some(0));
        assert_eq!(dvo_port_to_port(7), Some(1));
        assert_eq!(dvo_port_to_port(19), Some(6));
        assert_eq!(dvo_port_to_port(21), None);
    }

    #[test]
    fn raw_vbt_range_is_checked() {
        let asls = 0x7a00_0000;
        assert_eq!(
            raw_vbt_range(asls, 0x0201, 0x2000, 0x1800),
            Some((0x7a00_2000, 0x1800))
        );
        assert_eq!(
            raw_vbt_range(asls, 0x0200, 0x7a10_0000, 0x1800),
            Some((0x7a10_0000, 0x1800))
        );
        assert_eq!(raw_vbt_range(asls, 0x0201, u64::MAX, 0x1800), None, "wraps");
        assert_eq!(
            raw_vbt_range(asls, 0x0200, u64::MAX - 4, 0x1800),
            None,
            "end overflows"
        );
        assert_eq!(
            raw_vbt_range(asls, 0x0201, 1 << 40, 0x1800),
            None,
            "far from the OpRegion"
        );
        assert_eq!(
            raw_vbt_range(asls, 0x0200, 0x1000, 0x1800),
            None,
            "far below"
        );
        assert_eq!(raw_vbt_range(asls, 0x0201, 0x2000, RAW_VBT_MAX + 1), None);
        assert_eq!(raw_vbt_range(asls, 0x0201, 0, 0x1800), None);
    }
}
