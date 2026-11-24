//! EDID (VESA E-EDID 1.4) and CTA-861 extensions: checksum and header checks, the detailed timing
//! descriptors (the modes a monitor names exactly), the physical size and the monitor name. The
//! KMS core parses EDID too; the driver uses this to pick a mode for its own boot display and to
//! verify what it read over AUX/DDC.

use crate::error::{Error, Result};
use crate::mode::{Timing, FLAG_INTERLACE, FLAG_NHSYNC, FLAG_NVSYNC, FLAG_PHSYNC, FLAG_PVSYNC};
use alloc::string::String;
use alloc::vec::Vec;

/// Size of one EDID block.
pub const BLOCK: usize = 128;
/// Fixed header of the base block.
pub const HEADER: [u8; 8] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];

/// Whether a 128-byte block sums to 0.
pub fn checksum_ok(block: &[u8]) -> bool {
    block.len() == BLOCK && block.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

/// Checks the base block; returns the number of extension blocks that follow.
pub fn check_base(block: &[u8]) -> Result<usize> {
    if block.len() != BLOCK || block[..8] != HEADER {
        return Err(Error::Edid("header"));
    }
    if !checksum_ok(block) {
        return Err(Error::Edid("checksum"));
    }
    Ok(block[126] as usize)
}

/// What the driver uses of an EDID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// Detailed timings in order (the first is the preferred mode on EDID 1.3+).
    pub timings: Vec<Timing>,
    /// Physical size in millimetres (0 = unknown).
    pub size_mm: (u32, u32),
    /// Monitor name, if given.
    pub name: Option<String>,
    /// Three-letter manufacturer id.
    pub vendor: [u8; 3],
}

/// Decodes one 18-byte detailed timing descriptor (`None` for display descriptors).
pub fn detailed_timing(d: &[u8]) -> Option<(Timing, (u32, u32))> {
    let clock10k = u16::from_le_bytes([*d.first()?, *d.get(1)?]) as u32;
    if clock10k == 0 || d.len() < 18 {
        return None;
    }
    let hact = d[2] as u32 | ((d[4] as u32 >> 4) << 8);
    let hblank = d[3] as u32 | ((d[4] as u32 & 0xf) << 8);
    let vact = d[5] as u32 | ((d[7] as u32 >> 4) << 8);
    let vblank = d[6] as u32 | ((d[7] as u32 & 0xf) << 8);
    let hso = d[8] as u32 | ((d[11] as u32 >> 6) << 8);
    let hsw = d[9] as u32 | (((d[11] as u32 >> 4) & 3) << 8);
    let vso = (d[10] as u32 >> 4) | (((d[11] as u32 >> 2) & 3) << 4);
    let vsw = (d[10] as u32 & 0xf) | ((d[11] as u32 & 3) << 4);
    let hmm = d[12] as u32 | ((d[14] as u32 >> 4) << 8);
    let vmm = d[13] as u32 | ((d[14] as u32 & 0xf) << 8);
    let mut flags = if d[17] & 0x80 != 0 { FLAG_INTERLACE } else { 0 };
    if d[17] & 0x18 == 0x18 {
        flags |= if d[17] & 4 != 0 {
            FLAG_PVSYNC
        } else {
            FLAG_NVSYNC
        };
        flags |= if d[17] & 2 != 0 {
            FLAG_PHSYNC
        } else {
            FLAG_NHSYNC
        };
    }
    let t = Timing {
        clock: clock10k * 10,
        hdisplay: hact as u16,
        hsync_start: (hact + hso) as u16,
        hsync_end: (hact + hso + hsw) as u16,
        htotal: (hact + hblank) as u16,
        vdisplay: vact as u16,
        vsync_start: (vact + vso) as u16,
        vsync_end: (vact + vso + vsw) as u16,
        vtotal: (vact + vblank) as u16,
        flags,
    };
    Some((t, (hmm, vmm)))
}

fn descriptor_text(d: &[u8]) -> String {
    d[5..18]
        .iter()
        .take_while(|c| **c != b'\n')
        .filter(|c| c.is_ascii_graphic() || **c == b' ')
        .map(|c| *c as char)
        .collect::<String>()
        .trim_end()
        .into()
}

/// Makes base block `base` claim `n` extension blocks (byte 126), keeping its checksum valid. Used
/// when only the first extensions were read (the others need the E-DDC segment pointer).
pub fn claim_extensions(base: &mut [u8], n: u8) {
    if base.len() < BLOCK {
        return;
    }
    let old = base[126];
    base[126] = n;
    base[127] = base[127].wrapping_add(old).wrapping_sub(n);
}

/// Parses a complete EDID (base block and extensions). Blocks with a bad checksum are refused.
pub fn parse(edid: &[u8]) -> Result<Info> {
    let base = edid.get(..BLOCK).ok_or(Error::Edid("too short"))?;
    let ext = check_base(base)?;
    if edid.len() < BLOCK * (1 + ext) {
        return Err(Error::Edid("extension blocks missing"));
    }
    let id = u16::from_be_bytes([base[8], base[9]]);
    let vendor = [
        b'@' + ((id >> 10) & 0x1f) as u8,
        b'@' + ((id >> 5) & 0x1f) as u8,
        b'@' + (id & 0x1f) as u8,
    ];
    let mut info = Info {
        timings: Vec::new(),
        size_mm: (base[21] as u32 * 10, base[22] as u32 * 10),
        name: None,
        vendor,
    };
    for d in base[54..126].chunks_exact(18) {
        if let Some((t, mm)) = detailed_timing(d) {
            if info.timings.is_empty() && mm.0 > 0 && mm.1 > 0 {
                info.size_mm = mm;
            }
            info.timings.push(t);
        } else if d[3] == 0xfc {
            info.name = Some(descriptor_text(d));
        }
    }
    for b in 1..=ext {
        let blk = &edid[b * BLOCK..(b + 1) * BLOCK];
        if !checksum_ok(blk) {
            return Err(Error::Edid("extension checksum"));
        }
        // CTA-861: d = 0 means "no detailed timings and no data blocks"; d < 4 is invalid
        if blk[0] == 0x02 && blk[2] >= 4 {
            let start = (blk[2] as usize).min(127);
            for d in blk[start..127].chunks_exact(18) {
                match detailed_timing(d) {
                    Some((t, _)) => info.timings.push(t),
                    None => break,
                }
            }
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dp::sink::test_edid;
    use crate::sim::fhd;

    #[test]
    fn parses_the_test_monitor() {
        let e = test_edid();
        assert_eq!(check_base(&e[..128]), Ok(1));
        let i = parse(&e).unwrap();
        assert_eq!(i.timings[0], fhd());
        assert_eq!(i.timings[1].hdisplay, 1280);
        assert_eq!(i.timings[1].clock, 74_250);
        assert_eq!(i.timings[1].htotal, 1650);
        assert_eq!(i.timings[1].vtotal, 750);
        assert_eq!(i.name.as_deref(), Some("ULTAR TEST"));
        assert_eq!(&i.vendor, b"DEL");
        assert_eq!(i.size_mm, (700, 390));
    }

    #[test]
    fn damaged_edid_is_refused() {
        let mut e = test_edid();
        assert_eq!(parse(&e[..100]), Err(Error::Edid("too short")));
        assert_eq!(
            parse(&e[..128]),
            Err(Error::Edid("extension blocks missing"))
        );
        e[130] ^= 1;
        assert_eq!(parse(&e), Err(Error::Edid("extension checksum")));
        e[130] ^= 1;
        e[0] = 1;
        assert_eq!(parse(&e), Err(Error::Edid("header")));
        e[0] = 0;
        e[60] ^= 0x10;
        assert_eq!(parse(&e), Err(Error::Edid("checksum")));
    }

    #[test]
    fn sync_and_interlace_flags() {
        let mut d = [0u8; 18];
        d[0] = 1;
        d[2] = 10;
        d[17] = 0x80 | 0x18;
        let (t, _) = detailed_timing(&d).unwrap();
        assert_eq!(t.flags, FLAG_INTERLACE | FLAG_NVSYNC | FLAG_NHSYNC);
        d[17] = 0x10; // analog/composite: no polarity flags
        assert_eq!(detailed_timing(&d).unwrap().0.flags, 0);
        assert_eq!(detailed_timing(&[0u8; 18]), None);
        assert!(!checksum_ok(&[0u8; 10]));
    }

    #[test]
    fn claiming_fewer_extensions_keeps_the_checksum() {
        let mut e = test_edid();
        e[126] = 3;
        e[127] = e[127].wrapping_sub(2);
        assert_eq!(check_base(&e[..128]), Ok(3));
        claim_extensions(&mut e[..128], 1);
        assert_eq!(check_base(&e[..128]), Ok(1));
        assert!(parse(&e).is_ok());
        let mut short = [0u8; 4];
        claim_extensions(&mut short, 1);
        assert_eq!(short, [0; 4]);
    }
}
