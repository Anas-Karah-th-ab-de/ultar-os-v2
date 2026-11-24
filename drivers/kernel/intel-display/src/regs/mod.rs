//! Register layout of the display engine (byte offsets in BAR0). Offsets and bit positions follow
//! Intel's public PRMs ("Display Engine Registers", volumes 2c) as also used by the MIT-licensed
//! Linux i915 headers (`intel_display_regs.h`, `skl_universal_plane_regs.h`).
//!
//! Index conventions: `pipe`/`trans` 0 = A … 3 = D (on these platforms transcoder n feeds pipe n);
//! `plane` 0 = plane 1 (primary) … 4 = plane 5; `port` 0 = DDI A, 1 = DDI B, 3..=6 = TC1..TC4.

pub mod ddi;
pub mod irq;
pub mod port;

use crate::hw::genmask;

/// Transcoder timing and link registers: `0x60000 + trans * 0x1000 + off`.
pub const fn trans(trans: u8, off: u32) -> u32 {
    0x60000 + trans as u32 * 0x1000 + off
}

/// Pipe registers in the 0x70000 block: `0x70000 + pipe * 0x1000 + off`.
pub const fn pipe(pipe: u8, off: u32) -> u32 {
    0x70000 + pipe as u32 * 0x1000 + off
}

/// Universal plane registers: `0x70180 + pipe * 0x1000 + plane * 0x100 + off`.
pub const fn plane(pipe: u8, plane: u8, off: u32) -> u32 {
    0x70180 + pipe as u32 * 0x1000 + plane as u32 * 0x100 + off
}

/// `HTOTAL`: active − 1 in 15:0, total − 1 in 31:16 (same for HBLANK, HSYNC, VTOTAL, VBLANK, VSYNC).
pub const HTOTAL: u32 = 0x00;
/// `HBLANK`: blank start − 1, blank end − 1.
pub const HBLANK: u32 = 0x04;
/// `HSYNC`: sync start − 1, sync end − 1.
pub const HSYNC: u32 = 0x08;
/// `VTOTAL`.
pub const VTOTAL: u32 = 0x0c;
/// `VBLANK`.
pub const VBLANK: u32 = 0x10;
/// `VSYNC`.
pub const VSYNC: u32 = 0x14;
/// `PIPESRC`: (width − 1) << 16 | (height − 1).
pub const PIPESRC: u32 = 0x1c;
/// `VSYNCSHIFT` (interlaced modes only).
pub const VSYNCSHIFT: u32 = 0x28;
/// `PIPE_DATA_M1`: TU size − 1 in 30:25, data M in 23:0.
pub const DATA_M1: u32 = 0x30;
/// `PIPE_DATA_N1`.
pub const DATA_N1: u32 = 0x34;
/// `PIPE_LINK_M1`.
pub const LINK_M1: u32 = 0x40;
/// `PIPE_LINK_N1`.
pub const LINK_N1: u32 = 0x44;
/// `PIPE_CRC_CTL`.
pub const CRC_CTL: u32 = 0x50;
/// `PIPE_CRC_RES` (Haswell and later: one 32-bit result).
pub const CRC_RES: u32 = 0x64;

/// Mask of the TU size field in `DATA_M1`.
pub const TU_SIZE_MASK: u32 = genmask(30, 25);
/// Mask of M and N values.
pub const MN_MASK: u32 = genmask(23, 0);

/// `TRANSCONF` / `PIPECONF` (pipe block 0x70008).
pub const TRANSCONF: u32 = 0x08;
/// Enable request.
pub const TRANSCONF_ENABLE: u32 = 1 << 31;
/// The transcoder actually runs.
pub const TRANSCONF_STATE: u32 = 1 << 30;
/// `PIPE_MISC` (pipe block 0x70030).
pub const PIPE_MISC: u32 = 0x30;
/// `PIPEDSL`: current scan line in 12:0 (pipe block 0x70000).
pub const PIPEDSL: u32 = 0x00;
/// `PIPE_FRMCOUNT`: 32-bit frame counter (pipe block 0x70040).
pub const FRMCOUNT: u32 = 0x40;
/// `PIPE_FLIPCOUNT` (pipe block 0x70044).
pub const FLIPCOUNT: u32 = 0x44;
/// `PIPE_FRMTMSTMP`: value of [`TIMESTAMP_CTR`] at the start of the last vertical blank.
pub const FRMTMSTMP: u32 = 0x48;
/// `PIPE_FLIPTMSTMP`.
pub const FLIPTMSTMP: u32 = 0x4c;
/// Free-running display time stamp counter (1 MHz).
pub const TIMESTAMP_CTR: u32 = 0x44070;

/// `PLANE_CTL`.
pub const PLANE_CTL: u32 = 0x00;
/// `PLANE_STRIDE` (64-byte units for linear and X-tiled).
pub const PLANE_STRIDE: u32 = 0x08;
/// `PLANE_POS`.
pub const PLANE_POS: u32 = 0x0c;
/// `PLANE_SIZE`: (height − 1) << 16 | (width − 1).
pub const PLANE_SIZE: u32 = 0x10;
/// `PLANE_SURF`: GGTT address (4 KiB aligned); writing it arms the update at the next blank.
pub const PLANE_SURF: u32 = 0x1c;
/// `PLANE_OFFSET`.
pub const PLANE_OFFSET: u32 = 0x24;
/// `PLANE_SURFLIVE`: the surface address the hardware scans out right now.
pub const PLANE_SURFLIVE: u32 = 0x2c;
/// `PLANE_COLOR_CTL` (Gemini Lake and later).
pub const PLANE_COLOR_CTL: u32 = 0x4c;
/// `PLANE_BUF_CFG` (0x7027c: plane 1 at 0x70180 + 0xfc).
pub const PLANE_BUF_CFG: u32 = 0xfc;
/// `PLANE_WM` level 0 (0x70240).
pub const PLANE_WM0: u32 = 0xc0;

/// Plane enable.
pub const PLANE_CTL_ENABLE: u32 = 1 << 31;
/// Pixel format field (display 11+: 27:23; display 9: 27:24, same values for RGB).
pub const PLANE_CTL_FORMAT_MASK: u32 = genmask(27, 23);
/// RGB order XBGR instead of XRGB.
pub const PLANE_CTL_ORDER_RGBX: u32 = 1 << 20;
/// Tiling field.
pub const PLANE_CTL_TILED_MASK: u32 = genmask(12, 10);
/// Rotation field.
pub const PLANE_CTL_ROTATE_MASK: u32 = genmask(1, 0);
/// Render/media decompression and other bits the takeover does not support.
pub const PLANE_CTL_DECOMPRESSION: u32 = (1 << 15) | (1 << 4);
/// Horizontal flip.
pub const PLANE_CTL_FLIP_HORIZONTAL: u32 = 1 << 8;
/// Async flip.
pub const PLANE_CTL_ASYNC_FLIP: u32 = 1 << 9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_match_the_captured_dump() {
        assert_eq!(trans(0, HTOTAL), 0x60000);
        assert_eq!(trans(1, VSYNC), 0x61014);
        assert_eq!(trans(0, PIPESRC), 0x6001c);
        assert_eq!(pipe(0, TRANSCONF), 0x70008);
        assert_eq!(pipe(2, TRANSCONF), 0x72008);
        assert_eq!(plane(0, 0, PLANE_CTL), 0x70180);
        assert_eq!(plane(0, 0, PLANE_SURF), 0x7019c);
        assert_eq!(plane(1, 1, PLANE_CTL), 0x71280);
        assert_eq!(plane(0, 0, PLANE_BUF_CFG), 0x7027c);
        assert_eq!(plane(0, 0, PLANE_WM0), 0x70240);
        assert_eq!(plane(0, 0, PLANE_COLOR_CTL), 0x701cc);
        assert_eq!(plane(0, 0, PLANE_SURFLIVE), 0x701ac);
        assert_eq!(trans(0, CRC_CTL), 0x60050);
        assert_eq!(trans(1, CRC_RES), 0x61064);
        assert_eq!(pipe(0, FRMTMSTMP), 0x70048);
    }
}
