//! Display buffer (DBUF/DDB) split and level-0 watermarks for a pipe the driver enables itself.
//! One linear primary plane and the cursor per pipe; the formulas are Intel's "method 1/2" as in
//! Linux' `skl_watermark.c` (MIT) for display 11+, level 0 only (higher levels stay disabled,
//! which costs power, never correctness).

use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::regs::{self, plane};

/// `MTL_LATENCY_LP0_LP1`: memory latency of watermark levels 0/1 in µs (level 0 in 12:0).
pub const MTL_LATENCY_LP0_LP1: u32 = 0x45780;
/// `CUR_BUF_CFG` of pipe A (pipe stride 0x1000).
pub const CUR_BUF_CFG_A: u32 = 0x7017c;
/// `CUR_WM` level 0 of pipe A.
pub const CUR_WM0_A: u32 = 0x70140;
/// `PIPE_WM_LINETIME` of pipe A (pipe stride 4).
pub const LINETIME_A: u32 = 0x45270;
/// `PIPE_MBUS_DBOX_CTL` of pipe A (pipe stride 0x1000).
pub const MBUS_DBOX_A: u32 = 0x7003c;
/// MBUS DBOX credits of a display-14 pipe that is alone on its DBUF bank (Linux'
/// `pipe_mbus_dbox_ctl`: I credit 2, B2B 16/delay 1/regulated, A credit 8, B credit 10, BW 8 credits).
pub const MBUS_DBOX_MTL: u32 =
    (16 << 20) | (1 << 17) | (1 << 16) | (3 << 14) | (0xa << 8) | (2 << 5) | 8;
/// Watermark enable.
pub const WM_ENABLE: u32 = 1 << 31;
/// DBUF blocks given to one pipe (one slice of display 14).
pub const PIPE_BLOCKS: u32 = 2048;
/// DBUF blocks of the cursor.
pub const CURSOR_BLOCKS: u32 = 39;

const BLOCK: u64 = 512;
const ONE: u64 = 1 << 16;

/// Level-0 watermark of a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level0 {
    /// Blocks.
    pub blocks: u32,
    /// Lines.
    pub lines: u32,
    /// Smallest DDB allocation that keeps this level valid.
    pub min_ddb: u32,
}

impl Level0 {
    /// The `PLANE_WM` / `CUR_WM` register value.
    pub fn reg(&self) -> u32 {
        WM_ENABLE | (self.lines & 0x1fff) << 14 | (self.blocks & 0x1fff)
    }
}

fn ceil16(v: u64) -> u64 {
    v.div_ceil(ONE)
}

/// Level-0 watermark of a linear plane `width` pixels wide with `cpp` bytes per pixel on a pipe
/// with `pixel_khz` and `htotal`, for a memory latency of `latency_us`.
pub fn level0(
    pixel_khz: u32,
    htotal: u32,
    width: u32,
    cpp: u32,
    latency_us: u32,
) -> Result<Level0> {
    if pixel_khz == 0 || htotal == 0 || latency_us == 0 || width == 0 || cpp == 0 {
        return Err(Error::Invalid("watermark inputs"));
    }
    let (rate, ht, lat) = (pixel_khz as u64, htotal as u64, latency_us as u64);
    let bytes_per_line = width as u64 * cpp as u64;
    let blocks_per_line = (bytes_per_line.div_ceil(BLOCK) + 1) * ONE;
    let method1 = lat * rate * cpp as u64 * ONE / (1000 * BLOCK) + ONE;
    let method2 = (lat * rate).div_ceil(ht * 1000) * blocks_per_line;
    let linetime_us = (ht * 1000).div_ceil(rate);
    let selected =
        if cpp as u64 * ht / BLOCK < 1 && bytes_per_line / BLOCK < 1 || lat >= linetime_us {
            method2
        } else {
            method1
        };
    let blocks = (ceil16(selected) + 1).max(ceil16(blocks_per_line));
    let lines = (selected * ONE).div_ceil(blocks_per_line).div_ceil(ONE);
    if lines > 0x1fff || blocks > 0x1fff {
        return Err(Error::Invalid("watermark too large"));
    }
    Ok(Level0 {
        blocks: blocks as u32,
        lines: lines as u32,
        min_ddb: (blocks + blocks.div_ceil(10)) as u32,
    })
}

/// Level-0 latency from the hardware; 0 means "read latency only" (3 µs on display 12+).
pub fn read_latency<H: Hw>(hw: &mut H) -> u32 {
    match hw.read(MTL_LATENCY_LP0_LP1) & 0x1fff {
        0 => 3,
        l => l,
    }
}

/// Line time in 1/8 µs (`PIPE_WM_LINETIME`).
pub fn linetime(pixel_khz: u32, htotal: u32) -> u32 {
    ((htotal as u64 * 8000).div_ceil(pixel_khz.max(1) as u64) as u32).min(0x1ff)
}

/// `PLANE_BUF_CFG` value of a DDB range `[start, end)`.
pub fn buf_cfg(start: u32, end: u32) -> u32 {
    if end == 0 {
        0
    } else {
        ((end - 1) & 0x1fff) << 16 | (start & 0x1fff)
    }
}

/// Programs DDB split and level-0 watermarks of pipe `p` (plane 1 + cursor), DDB from `base`.
pub fn program<H: Hw>(
    hw: &mut H,
    p: u8,
    base: u32,
    plane_wm: &Level0,
    cursor_wm: &Level0,
    pixel_khz: u32,
    htotal: u32,
) -> Result<()> {
    let plane_end = base + PIPE_BLOCKS - CURSOR_BLOCKS;
    if plane_wm.min_ddb > PIPE_BLOCKS - CURSOR_BLOCKS || cursor_wm.min_ddb > CURSOR_BLOCKS {
        return Err(Error::NoSpace);
    }
    let pipe_off = p as u32 * 0x1000;
    hw.write(plane(p, 0, regs::PLANE_BUF_CFG), buf_cfg(base, plane_end));
    hw.write(plane(p, 0, regs::PLANE_WM0), plane_wm.reg());
    for level in 1..8 {
        hw.write(plane(p, 0, regs::PLANE_WM0) + 4 * level, 0);
    }
    hw.write(plane(p, 0, 0xe8), 0); // PLANE_WM_TRANS
    hw.write(
        CUR_BUF_CFG_A + pipe_off,
        buf_cfg(plane_end, base + PIPE_BLOCKS),
    );
    hw.write(CUR_WM0_A + pipe_off, cursor_wm.reg());
    hw.write(LINETIME_A + 4 * p as u32, linetime(pixel_khz, htotal));
    hw.write(MBUS_DBOX_A + pipe_off, MBUS_DBOX_MTL);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn fhd_plane_level0() {
        let wm = level0(148_500, 2200, 1920, 4, 3).unwrap();
        // 1920 * 4 / 512 = 15 blocks per line + 1 -> 16; latency 3 us < line time 15 us: method 1
        assert_eq!(wm.blocks, 16);
        assert_eq!(wm.lines, 1);
        assert_eq!(wm.min_ddb, 18);
        assert_eq!(wm.reg(), WM_ENABLE | 1 << 14 | 16);
        let long = level0(148_500, 2200, 1920, 4, 30).unwrap();
        assert!(long.lines >= 2 && long.blocks >= 32);
        let cur = level0(148_500, 2200, 64, 4, 3).unwrap();
        assert!(cur.min_ddb <= CURSOR_BLOCKS);
        assert_eq!(
            level0(0, 2200, 1920, 4, 3),
            Err(Error::Invalid("watermark inputs"))
        );
        assert_eq!(
            level0(1_000_000, 100, 5120, 4, 8000),
            Err(Error::Invalid("watermark too large"))
        );
    }

    #[test]
    fn linetime_and_buffers() {
        assert_eq!(linetime(148_500, 2200), 0x77); // PIPE_WM_LINETIME_A as captured
        assert_eq!(buf_cfg(0, 4057), 4056 << 16);
        assert_eq!(buf_cfg(4057, 4096), (4095 << 16) | 4057);
        assert_eq!(buf_cfg(0, 0), 0);
    }

    #[test]
    fn program_pipe_a() {
        let mut s = Sim::new();
        s.set(MTL_LATENCY_LP0_LP1, 0);
        let lat = read_latency(&mut s);
        assert_eq!(lat, 3);
        let p = level0(148_500, 2200, 1920, 4, lat).unwrap();
        let c = level0(148_500, 2200, 64, 4, lat).unwrap();
        program(&mut s, 0, 0, &p, &c, 148_500, 2200).unwrap();
        assert_eq!(s.get(0x7027c), buf_cfg(0, 2009));
        assert_eq!(s.get(0x70240), p.reg());
        assert_eq!(s.get(0x70244), 0);
        assert_eq!(s.get(0x7017c), buf_cfg(2009, 2048));
        assert_eq!(s.get(0x45270), 0x77);
        assert_eq!(s.get(0x7003c), 0x0103_ca48);
        let huge = Level0 {
            blocks: 3000,
            lines: 1,
            min_ddb: 3300,
        };
        assert_eq!(
            program(&mut s, 1, 0, &huge, &c, 148_500, 2200),
            Err(Error::NoSpace)
        );
        s.set(MTL_LATENCY_LP0_LP1, 0x0004_0005);
        assert_eq!(read_latency(&mut s), 5);
    }
}
