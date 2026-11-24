//! WOPCM: the write-once protected memory the GuC (and HuC) firmware runs in. The driver divides
//! it and writes the GuC's part into two registers that lock until the next reset of the
//! machine. Follows `xe_wopcm.c` (MIT).
//!
//! ```text
//! | reserved 16K | HuC image | GuC part: [stack 8K][GuC image][reserved 16K] ... | context 36K |
//! 0              16K         guc_base (16K-aligned)                              size - 36K
//! ```

use crate::regs::*;
use crate::{Error, Hw, Result};

/// Size of the protected memory on Meteor Lake / Arrow Lake.
pub const MTL_WOPCM_SIZE: u32 = 4 << 20;
/// Largest protected memory of the platforms whose registers a firmware may have locked.
const MAX_WOPCM_SIZE: u32 = 8 << 20;
/// Reserved at the start.
const WOPCM_RESERVED: u32 = 16 << 10;
/// Reserved inside the GuC's part.
const GUC_RESERVED: u32 = 16 << 10;
/// The GuC's stack below its image.
const GUC_STACK_RESERVED: u32 = 8 << 10;
/// Reserved at the end for the hardware contexts.
const CTX_RESERVED: u32 = (32 << 10) + (4 << 10);
/// The GuC's part starts on a multiple of this.
const GUC_OFFSET_ALIGN: u32 = 1 << 14;

/// The GuC's part of the protected memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Size of the whole protected memory.
    pub size: u32,
    /// Where the GuC's part starts.
    pub guc_base: u32,
    /// Size of the GuC's part.
    pub guc_size: u32,
}

fn check(l: &Layout, guc_fw: u32, huc_fw: u32) -> Result<()> {
    let usable = l
        .size
        .checked_sub(CTX_RESERVED)
        .ok_or(Error::Wopcm("smaller than the context area"))?;
    if l.guc_base >= usable || l.guc_size > usable - l.guc_base {
        return Err(Error::Wopcm("GuC part outside the protected memory"));
    }
    let need = guc_fw
        .checked_add(GUC_RESERVED + GUC_STACK_RESERVED)
        .ok_or(Error::Wopcm("GuC image too large"))?;
    if l.guc_size < need {
        return Err(Error::Wopcm("no space for the GuC image"));
    }
    let need = huc_fw
        .checked_add(WOPCM_RESERVED)
        .ok_or(Error::Wopcm("HuC image too large"))?;
    if l.guc_base < need {
        return Err(Error::Wopcm("no space for the HuC image"));
    }
    Ok(())
}

/// The layout the registers hold if they are locked already (by an earlier driver since the last
/// reset of the machine).
pub fn locked(hw: &mut impl Hw) -> Option<(u32, u32)> {
    let base = hw.read(DMA_GUC_WOPCM_OFFSET);
    let size = hw.read(GUC_WOPCM_SIZE);
    if base == u32::MAX || size == u32::MAX {
        return None;
    }
    (size & GUC_WOPCM_SIZE_LOCKED != 0 && base & GUC_WOPCM_OFFSET_VALID != 0)
        .then_some((base & GUC_WOPCM_OFFSET_MASK, size & GUC_WOPCM_SIZE_MASK))
}

/// Divides a protected memory of `size` bytes for a GuC image of `guc_fw` and a HuC image of
/// `huc_fw` bytes (upload sizes; 0 without HuC).
pub fn compute(size: u32, guc_fw: u32, huc_fw: u32) -> Result<Layout> {
    if guc_fw == 0 {
        return Err(Error::Wopcm("no GuC image"));
    }
    let usable = size
        .checked_sub(CTX_RESERVED)
        .ok_or(Error::Wopcm("smaller than the context area"))?;
    let base = huc_fw
        .checked_add(WOPCM_RESERVED)
        .and_then(|b| b.checked_next_multiple_of(GUC_OFFSET_ALIGN))
        .ok_or(Error::Wopcm("HuC image too large"))?
        .min(usable);
    let l = Layout {
        size,
        guc_base: base,
        guc_size: (usable - base) & GUC_WOPCM_SIZE_MASK,
    };
    check(&l, guc_fw, huc_fw)?;
    Ok(l)
}

fn write_and_verify(hw: &mut impl Hw, reg: u32, val: u32, mask: u32, want: u32) -> Result<()> {
    hw.write(reg, val);
    let got = hw.read(reg);
    if got & mask == want {
        Ok(())
    } else {
        Err(Error::Wopcm("register did not take the layout"))
    }
}

/// Makes the layout for the images and brings the registers to it: a layout that is locked
/// already is kept if the images fit it (the registers cannot change any more); otherwise the
/// computed layout is written and locked. Needs the `Gt` domain awake.
pub fn init(hw: &mut impl Hw, size: u32, guc_fw: u32, huc_fw: u32) -> Result<Layout> {
    if let Some((guc_base, guc_size)) = locked(hw) {
        let l = Layout {
            size: MAX_WOPCM_SIZE,
            guc_base,
            guc_size,
        };
        check(&l, guc_fw, huc_fw)?;
        return Ok(l);
    }
    let l = compute(size, guc_fw, huc_fw)?;
    let agent = if huc_fw != 0 {
        HUC_LOADING_AGENT_GUC
    } else {
        0
    };
    write_and_verify(
        hw,
        GUC_WOPCM_SIZE,
        l.guc_size,
        GUC_WOPCM_SIZE_MASK | GUC_WOPCM_SIZE_LOCKED,
        l.guc_size | GUC_WOPCM_SIZE_LOCKED,
    )?;
    write_and_verify(
        hw,
        DMA_GUC_WOPCM_OFFSET,
        l.guc_base | agent,
        GUC_WOPCM_OFFSET_MASK | GUC_WOPCM_OFFSET_VALID | agent,
        l.guc_base | agent | GUC_WOPCM_OFFSET_VALID,
    )?;
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use alloc::boxed::Box;

    /// The registers lock themselves on the first write, as the hardware does.
    fn locking(mut s: Sim) -> Sim {
        s.on_write(
            GUC_WOPCM_SIZE,
            Box::new(|r, reg, v| {
                r.insert(reg, v | GUC_WOPCM_SIZE_LOCKED);
            }),
        );
        s.on_write(
            DMA_GUC_WOPCM_OFFSET,
            Box::new(|r, reg, v| {
                r.insert(reg, v | GUC_WOPCM_OFFSET_VALID);
            }),
        );
        s
    }

    #[test]
    fn layout_without_huc() {
        let l = compute(MTL_WOPCM_SIZE, 0x5_0080, 0).unwrap();
        assert_eq!(l.guc_base, 0x4000);
        // 4 MiB - 36 KiB - 16 KiB, already a multiple of 4 KiB
        assert_eq!(l.guc_size, 0x40_0000 - 0x9000 - 0x4000);
        assert_eq!(l.guc_size & !GUC_WOPCM_SIZE_MASK, 0);
        assert_eq!(l.guc_base & !GUC_WOPCM_OFFSET_MASK, 0);
    }

    #[test]
    fn layout_with_huc_is_aligned() {
        let l = compute(MTL_WOPCM_SIZE, 0x5_0080, 0x9_3210).unwrap();
        assert_eq!(l.guc_base, 0x9_8000);
        assert!(l.guc_base >= 0x9_3210 + 0x4000);
    }

    #[test]
    fn refuses_what_does_not_fit() {
        assert_eq!(
            compute(MTL_WOPCM_SIZE, 0, 0),
            Err(Error::Wopcm("no GuC image"))
        );
        assert_eq!(
            compute(MTL_WOPCM_SIZE, 4 << 20, 0),
            Err(Error::Wopcm("no space for the GuC image"))
        );
        assert!(compute(MTL_WOPCM_SIZE, 0x5_0080, 4 << 20).is_err());
        assert!(compute(0x1000, 0x100, 0).is_err());
        assert!(compute(MTL_WOPCM_SIZE, u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn writes_and_locks_the_registers() {
        let mut s = locking(Sim::new());
        let l = init(&mut s, MTL_WOPCM_SIZE, 0x5_0080, 0).unwrap();
        assert_eq!(
            s.writes,
            [(GUC_WOPCM_SIZE, l.guc_size), (DMA_GUC_WOPCM_OFFSET, 0x4000)]
        );
        assert_eq!(locked(&mut s), Some((0x4000, l.guc_size)));
    }

    #[test]
    fn keeps_a_locked_layout_that_fits() {
        let mut s = Sim::new();
        s.set(GUC_WOPCM_SIZE, 0x1f_0000 | GUC_WOPCM_SIZE_LOCKED);
        s.set(
            DMA_GUC_WOPCM_OFFSET,
            0x10_0000 | GUC_WOPCM_OFFSET_VALID | HUC_LOADING_AGENT_GUC,
        );
        let l = init(&mut s, MTL_WOPCM_SIZE, 0x5_0080, 0).unwrap();
        assert_eq!((l.guc_base, l.guc_size), (0x10_0000, 0x1f_0000));
        assert!(s.writes.is_empty());
        // an image that does not fit the locked part cannot be loaded until the machine resets
        assert_eq!(
            init(&mut s, MTL_WOPCM_SIZE, 0x20_0000, 0),
            Err(Error::Wopcm("no space for the GuC image"))
        );
    }

    #[test]
    fn a_register_that_does_not_lock_is_an_error() {
        let mut s = Sim::new();
        assert_eq!(
            init(&mut s, MTL_WOPCM_SIZE, 0x5_0080, 0),
            Err(Error::Wopcm("register did not take the layout"))
        );
    }
}
