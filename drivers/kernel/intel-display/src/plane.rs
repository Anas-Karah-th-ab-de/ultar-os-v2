//! Universal planes: pixel formats, the register values of a plane and the flip (a write of
//! `PLANE_SURF` arms all double-buffered plane registers for the next vertical blank).

use crate::error::{Error, Result};
use crate::hw::{field, genmask, Hw};
use crate::regs::{self, plane};

/// DRM fourcc `XR24` (XRGB8888).
pub const XRGB8888: u32 = 0x3432_5258;
/// DRM fourcc `AR24` (ARGB8888).
pub const ARGB8888: u32 = 0x3432_5241;
/// DRM fourcc `XB24` (XBGR8888).
pub const XBGR8888: u32 = 0x3432_4258;
/// DRM fourcc `AB24` (ABGR8888).
pub const ABGR8888: u32 = 0x3432_4241;
/// DRM fourcc `RG16` (RGB565).
pub const RGB565: u32 = 0x3631_4752;

/// Formats the primary plane takes (alpha is ignored on the primary plane).
pub const FORMATS: &[u32] = &[XRGB8888, ARGB8888, XBGR8888, ABGR8888, RGB565];

/// `PLANE_COLOR_CTL`: plane gamma off (display 10+).
pub const COLOR_CTL_GAMMA_DISABLE: u32 = 1 << 13;

/// Bytes per pixel of a supported format.
pub fn bytes_per_pixel(fourcc: u32) -> Option<u32> {
    match fourcc {
        XRGB8888 | ARGB8888 | XBGR8888 | ABGR8888 => Some(4),
        RGB565 => Some(2),
        _ => None,
    }
}

/// `PLANE_CTL` format and order bits of `fourcc`.
pub fn format_bits(fourcc: u32) -> Option<u32> {
    match fourcc {
        XRGB8888 | ARGB8888 => Some(4 << 24),
        XBGR8888 | ABGR8888 => Some((4 << 24) | regs::PLANE_CTL_ORDER_RGBX),
        RGB565 => Some(14 << 24),
        _ => None,
    }
}

/// The fourcc of `PLANE_CTL` format bits (ARGB/ABGR come back as their X variants).
pub fn format_from_ctl(ctl: u32) -> Option<u32> {
    let order = ctl & regs::PLANE_CTL_ORDER_RGBX != 0;
    if ctl & (1 << 23) != 0 {
        // display 11+ YUV formats use bit 23
        return None;
    }
    match (field(ctl, genmask(27, 24)), order) {
        (4, false) => Some(XRGB8888),
        (4, true) => Some(XBGR8888),
        (14, false) => Some(RGB565),
        _ => None,
    }
}

/// What a plane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaneConfig {
    /// GGTT address of the first byte (4 KiB aligned).
    pub surf: u32,
    /// Bytes per row (multiple of 64).
    pub pitch: u32,
    /// Size in pixels.
    pub width: u32,
    /// Size in pixels.
    pub height: u32,
    /// DRM fourcc.
    pub format: u32,
}

impl PlaneConfig {
    /// Checks size, pitch and alignment.
    pub fn validate(&self) -> Result<()> {
        let bpp = bytes_per_pixel(self.format).ok_or(Error::Invalid("pixel format"))?;
        if self.surf & 0xfff != 0 {
            return Err(Error::Invalid("surface not 4 KiB aligned"));
        }
        // size first: the pitch check multiplies the width
        if self.width == 0 || self.height == 0 || self.width > 5120 || self.height > 4096 {
            return Err(Error::Invalid("plane size"));
        }
        if !self.pitch.is_multiple_of(64)
            || self.pitch / 64 > 0xfff
            || (self.pitch as u64) < self.width as u64 * bpp as u64
        {
            return Err(Error::Invalid("pitch"));
        }
        Ok(())
    }

    /// Bytes the hardware reads.
    pub fn bytes(&self) -> u64 {
        self.pitch as u64 * self.height as u64
    }
}

/// Writes all registers of `pipe`'s plane `pl` and enables it; `PLANE_SURF` last (arms the update).
pub fn enable<H: Hw>(
    hw: &mut H,
    pipe: u8,
    pl: u8,
    cfg: &PlaneConfig,
    display_ver: u8,
) -> Result<()> {
    cfg.validate()?;
    let fmt = format_bits(cfg.format).ok_or(Error::Invalid("pixel format"))?;
    if display_ver >= 10 {
        hw.write(
            plane(pipe, pl, regs::PLANE_COLOR_CTL),
            COLOR_CTL_GAMMA_DISABLE,
        );
    }
    hw.rmw(
        plane(pipe, pl, regs::PLANE_STRIDE),
        genmask(11, 0),
        cfg.pitch / 64,
    );
    hw.write(plane(pipe, pl, regs::PLANE_POS), 0);
    hw.write(plane(pipe, pl, regs::PLANE_OFFSET), 0);
    hw.write(
        plane(pipe, pl, regs::PLANE_SIZE),
        ((cfg.height - 1) << 16) | (cfg.width - 1),
    );
    hw.write(
        plane(pipe, pl, regs::PLANE_CTL),
        regs::PLANE_CTL_ENABLE | fmt,
    );
    hw.write(plane(pipe, pl, regs::PLANE_SURF), cfg.surf);
    Ok(())
}

/// Turns the plane off (latched at the next blank).
pub fn disable<H: Hw>(hw: &mut H, pipe: u8, pl: u8) {
    hw.write(plane(pipe, pl, regs::PLANE_CTL), 0);
    hw.write(plane(pipe, pl, regs::PLANE_SURF), 0);
}

/// Shows `surf` from the next vertical blank on; pitch and format as now. Returns the armed value.
pub fn flip<H: Hw>(hw: &mut H, pipe: u8, pl: u8, surf: u32) -> Result<u32> {
    if surf & 0xfff != 0 {
        return Err(Error::Invalid("surface not 4 KiB aligned"));
    }
    hw.write(plane(pipe, pl, regs::PLANE_SURF), surf);
    Ok(surf)
}

/// Whether the hardware scans out `surf` now (the flip was latched).
pub fn latched<H: Hw>(hw: &mut H, pipe: u8, pl: u8, surf: u32) -> bool {
    hw.read(plane(pipe, pl, regs::PLANE_SURFLIVE)) & !0xfff == surf & !0xfff
}

/// The firmware's plane as found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FwPlane {
    /// `PLANE_CTL`.
    pub ctl: u32,
    /// What it shows.
    pub cfg: PlaneConfig,
}

/// Reads an enabled plane and checks that the driver can take it over: linear, not rotated, not
/// compressed, a supported RGB format.
pub fn read_fw<H: Hw>(hw: &mut H, pipe: u8, pl: u8) -> Result<Option<FwPlane>> {
    let ctl = hw.read(plane(pipe, pl, regs::PLANE_CTL));
    if ctl & regs::PLANE_CTL_ENABLE == 0 {
        return Ok(None);
    }
    let bad = regs::PLANE_CTL_TILED_MASK
        | regs::PLANE_CTL_ROTATE_MASK
        | regs::PLANE_CTL_DECOMPRESSION
        | regs::PLANE_CTL_FLIP_HORIZONTAL;
    let format = format_from_ctl(ctl).ok_or(Error::UnsupportedPlane(ctl))?;
    if ctl & bad != 0 {
        return Err(Error::UnsupportedPlane(ctl));
    }
    let size = hw.read(plane(pipe, pl, regs::PLANE_SIZE));
    let stride = hw.read(plane(pipe, pl, regs::PLANE_STRIDE)) & genmask(11, 0);
    let surf = hw.read(plane(pipe, pl, regs::PLANE_SURF)) & !0xfff;
    let cfg = PlaneConfig {
        surf,
        pitch: stride * 64,
        width: (size & 0xffff) + 1,
        height: (size >> 16) + 1,
        format,
    };
    cfg.validate().map_err(|_| Error::UnsupportedPlane(ctl))?;
    Ok(Some(FwPlane { ctl, cfg }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    fn beelink_plane(s: &mut Sim) {
        s.set(0x70180, 0x8400_0000);
        s.set(0x70188, 0x12ac_0078);
        s.set(0x70190, (1079 << 16) | 1919);
        s.set(0x7019c, 0x0175_b000);
    }

    #[test]
    fn reads_the_captured_plane() {
        let mut s = Sim::new();
        beelink_plane(&mut s);
        let p = read_fw(&mut s, 0, 0).unwrap().unwrap();
        assert_eq!(
            p.cfg,
            PlaneConfig {
                surf: 0x0175_b000,
                pitch: 7680,
                width: 1920,
                height: 1080,
                format: XRGB8888
            }
        );
        assert_eq!(p.cfg.bytes(), 7680 * 1080);
        assert_eq!(read_fw(&mut s, 1, 0), Ok(None));
    }

    #[test]
    fn refuses_tiled_and_rotated_planes() {
        let mut s = Sim::new();
        beelink_plane(&mut s);
        s.set(0x70180, 0x8400_0000 | (1 << 10));
        assert_eq!(
            read_fw(&mut s, 0, 0),
            Err(Error::UnsupportedPlane(0x8400_0400))
        );
        s.set(0x70180, 0x8400_0002);
        assert!(read_fw(&mut s, 0, 0).is_err());
        s.set(0x70180, 0x8200_0000);
        assert!(read_fw(&mut s, 0, 0).is_err());
        s.set(0x70180, 0x8480_0000); // Y410-like value with bit 23
        assert!(read_fw(&mut s, 0, 0).is_err());
    }

    #[test]
    fn enable_writes_surf_last() {
        let mut s = Sim::new();
        s.set(0x70188, 0x12ac_0000);
        let cfg = PlaneConfig {
            surf: 0x0100_0000,
            pitch: 7680,
            width: 1920,
            height: 1080,
            format: XRGB8888,
        };
        enable(&mut s, 0, 0, &cfg, 14).unwrap();
        assert_eq!(s.writes.last(), Some(&(0x7019c, 0x0100_0000)));
        assert_eq!(s.get(0x70180), 0x8400_0000);
        assert_eq!(s.get(0x70188), 0x12ac_0078);
        assert_eq!(s.get(0x701cc), COLOR_CTL_GAMMA_DISABLE);
        let mut s9 = Sim::new();
        enable(
            &mut s9,
            0,
            0,
            &PlaneConfig {
                format: ABGR8888,
                ..cfg
            },
            9,
        )
        .unwrap();
        assert_eq!(s9.get(0x70180), 0x8400_0000 | regs::PLANE_CTL_ORDER_RGBX);
        assert!(s9.writes_to(0x701cc).is_empty());
        disable(&mut s9, 0, 0);
        assert_eq!(s9.get(0x70180), 0);
    }

    #[test]
    fn flip_and_latch() {
        let mut s = Sim::new();
        assert_eq!(flip(&mut s, 0, 0, 0x2000_0000), Ok(0x2000_0000));
        assert_eq!(
            flip(&mut s, 0, 0, 0x2000_0010),
            Err(Error::Invalid("surface not 4 KiB aligned"))
        );
        assert!(!latched(&mut s, 0, 0, 0x2000_0000));
        s.set(0x701ac, 0x2000_0000);
        assert!(latched(&mut s, 0, 0, 0x2000_0000));
    }

    #[test]
    fn formats() {
        for f in FORMATS {
            assert!(bytes_per_pixel(*f).is_some());
            let bits = format_bits(*f).unwrap();
            let back = format_from_ctl(bits).unwrap();
            assert_eq!(bytes_per_pixel(back), bytes_per_pixel(*f));
        }
        assert_eq!(bytes_per_pixel(0x1234), None);
        assert_eq!(format_bits(0x1234), None);
        let bad = PlaneConfig {
            surf: 0,
            pitch: 100,
            width: 10,
            height: 10,
            format: XRGB8888,
        };
        assert_eq!(bad.validate(), Err(Error::Invalid("pitch")));
        let small = PlaneConfig {
            surf: 0,
            pitch: 64,
            width: 32,
            height: 10,
            format: XRGB8888,
        };
        assert_eq!(small.validate(), Err(Error::Invalid("pitch")));
        let zero = PlaneConfig {
            surf: 0,
            pitch: 64,
            width: 0,
            height: 10,
            format: XRGB8888,
        };
        assert_eq!(zero.validate(), Err(Error::Invalid("plane size")));
        let fmt = PlaneConfig {
            surf: 0,
            pitch: 64,
            width: 1,
            height: 1,
            format: 7,
        };
        assert_eq!(fmt.validate(), Err(Error::Invalid("pixel format")));
    }
}
