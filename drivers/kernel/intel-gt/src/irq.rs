//! Interrupts of the GT: the engines report a finished job (`MI_USER_INTERRUPT` at the end of
//! it), the GuC reports a message for the host. With them a program waiting for the GPU is woken
//! when its job is done, instead of the kernel looking every 250 µs.
//!
//! Register offsets, bits and the acknowledge sequence follow the MIT-licensed Linux `xe` driver
//! (`regs/xe_irq_regs.h`, `xe_irq.c`: `dg1_irq_handler`, `gt_irq_handler`, `gt_engine_identity`,
//! `xe_irq_enable_hwe`; `xe_guc.c`: `guc_enable_irq`). Graphics 12.70+ has the tile master
//! register in front of the graphics master register (the "DG1" path in `xe`).

use crate::Hw;

/// `DG1_MSTR_TILE_INTR`: bit 31 lets interrupts out; bits 3:0 which tile has one.
pub const MSTR_TILE_INTR: u32 = 0x19_0008;
const MSTR_IRQ: u32 = 1 << 31;
/// `GFX_MSTR_IRQ`: which group of the tile has an interrupt.
pub const GFX_MSTR_IRQ: u32 = 0x19_0010;
/// `GT_DW_IRQ(0)`, `GT_DW_IRQ(1)`: the two GT banks.
const GT_DW_IRQ: [u32; 2] = [1 << 0, 1 << 1];
/// `GT_INTR_DW(bank)`: which source of the bank has an interrupt.
pub const fn gt_intr_dw(bank: u32) -> u32 {
    0x19_0018 + bank * 4
}
const fn identity_reg(bank: u32) -> u32 {
    0x19_0060 + bank * 4
}
const fn iir_selector(bank: u32) -> u32 {
    0x19_0070 + bank * 4
}
const INTR_DATA_VALID: u32 = 1 << 31;

/// `RENDER_COPY_INTR_ENABLE`: render in bits 31:16, copy in bits 15:0.
pub const RENDER_COPY_INTR_ENABLE: u32 = 0x19_0030;
/// `GUC_SG_INTR_ENABLE`.
pub const GUC_SG_INTR_ENABLE: u32 = 0x19_0038;
/// `CCS_RSVD_INTR_ENABLE`: compute in bits 31:16.
pub const CCS_RSVD_INTR_ENABLE: u32 = 0x19_0048;
const RCS0_RSVD_INTR_MASK: u32 = 0x19_0090;
const BCS_RSVD_INTR_MASK: u32 = 0x19_00a0;
const GUC_SG_INTR_MASK: u32 = 0x19_00e8;
const CCS0_CCS1_INTR_MASK: u32 = 0x19_0100;

/// `GT_MI_USER_INTERRUPT`.
const USER_INTERRUPT: u32 = 1 << 0;
/// `GT_FLUSH_COMPLETE_INTERRUPT`.
const FLUSH_COMPLETE: u32 = 1 << 4;
/// `GUC_INTR_GUC2HOST`.
const GUC_TO_HOST: u32 = 1 << 15;
/// What the engines report (as `xe` with the GuC).
const ENGINE_EVENTS: u32 = USER_INTERRUPT | FLUSH_COMPLETE;

/// Bank 0 bit of the GuC (`INTR_GUC`).
const BIT_GUC: u32 = 25;
/// Engine class "other" in the identity (GuC, GSC, ...).
const CLASS_OTHER: u32 = 4;

/// Lets engine and GuC interrupts through and opens the master registers. Render (instance 0),
/// copy (0) and compute (0 and 1) as this GT has them.
pub fn enable(hw: &mut impl Hw) {
    let both = ENGINE_EVENTS << 16 | ENGINE_EVENTS;
    hw.write(RENDER_COPY_INTR_ENABLE, both);
    hw.write(CCS_RSVD_INTR_ENABLE, ENGINE_EVENTS << 16);
    hw.write(RCS0_RSVD_INTR_MASK, !(ENGINE_EVENTS << 16));
    hw.write(BCS_RSVD_INTR_MASK, !(ENGINE_EVENTS << 16));
    hw.write(CCS0_CCS1_INTR_MASK, !both);
    hw.write(GUC_SG_INTR_ENABLE, GUC_TO_HOST << 16);
    let mask = hw.read(GUC_SG_INTR_MASK);
    hw.write(GUC_SG_INTR_MASK, mask & !(GUC_TO_HOST << 16));
    hw.write(GFX_MSTR_IRQ, 1 << 31);
    hw.write(MSTR_TILE_INTR, MSTR_IRQ);
}

/// Closes the master register: no interrupts leave the device.
pub fn disable(hw: &mut impl Hw) {
    hw.write(MSTR_TILE_INTR, 0);
}

/// What one interrupt reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    /// Engine events (any engine): a job ended.
    pub engines: u32,
    /// The GuC has a message for the host.
    pub guc: bool,
    /// Sources whose identity did not become valid.
    pub lost: u32,
}

/// Takes the interrupt: master off, every source of both GT banks acknowledged, master on again.
/// What the display or other units report is not looked at (they are not enabled).
pub fn handle(hw: &mut impl Hw) -> Events {
    let mut ev = Events::default();
    hw.write(MSTR_TILE_INTR, 0);
    let tiles = hw.read(MSTR_TILE_INTR);
    if tiles == 0 {
        hw.write(MSTR_TILE_INTR, MSTR_IRQ);
        return ev;
    }
    hw.write(MSTR_TILE_INTR, tiles);
    let master = hw.read(GFX_MSTR_IRQ);
    if master == u32::MAX {
        // the device does not answer (reset, gone)
        return ev;
    }
    hw.write(GFX_MSTR_IRQ, master);
    for (bank, flag) in (0u32..).zip(GT_DW_IRQ) {
        if master & flag == 0 {
            continue;
        }
        let dw = hw.read(gt_intr_dw(bank));
        for bit in (0..32).filter(|b| dw & 1 << b != 0) {
            match identity(hw, bank, bit) {
                0 => ev.lost += 1,
                id if (id >> 16) & 7 == CLASS_OTHER || bank == 0 && bit == BIT_GUC => {
                    ev.guc |= id & GUC_TO_HOST != 0;
                }
                id => ev.engines |= id & 0xffff,
            }
        }
        hw.write(gt_intr_dw(bank), dw);
    }
    hw.write(MSTR_TILE_INTR, MSTR_IRQ);
    ev
}

/// Selects source `bit` of `bank`, waits (at most ~100 µs) for its identity and acknowledges it.
fn identity(hw: &mut impl Hw, bank: u32, bit: u32) -> u32 {
    hw.write(iir_selector(bank), 1 << bit);
    let start = hw.now_us();
    loop {
        let id = hw.read(identity_reg(bank));
        if id & INTR_DATA_VALID != 0 {
            hw.write(identity_reg(bank), id);
            return id;
        }
        if hw.now_us() - start > 100 {
            return 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn enable_unmasks_user_interrupts_and_opens_the_master() {
        let mut hw = Sim::new();
        hw.set(GUC_SG_INTR_MASK, u32::MAX);
        enable(&mut hw);
        assert_eq!(hw.get(RENDER_COPY_INTR_ENABLE), 0x0011_0011);
        assert_eq!(hw.get(CCS_RSVD_INTR_ENABLE), 0x0011_0000);
        assert_eq!(hw.get(RCS0_RSVD_INTR_MASK), !0x0011_0000);
        assert_eq!(hw.get(CCS0_CCS1_INTR_MASK), !0x0011_0011);
        assert_eq!(hw.get(GUC_SG_INTR_MASK), !(1 << 31));
        assert_eq!(hw.get(MSTR_TILE_INTR), 1 << 31);
    }

    #[test]
    fn handle_acknowledges_an_engine_and_the_guc() {
        let mut hw = Sim::new();
        // the indications stay when the master is written (level, not storage)
        hw.read_only(MSTR_TILE_INTR);
        hw.set(MSTR_TILE_INTR, 1);
        hw.set(GFX_MSTR_IRQ, 1);
        hw.set(gt_intr_dw(0), 1 << 4 | 1 << BIT_GUC);
        // the identity register answers what was selected last
        hw.set(identity_reg(0), INTR_DATA_VALID | 1 << 16 | USER_INTERRUPT);
        let ev = handle(&mut hw);
        assert_eq!(ev.engines, USER_INTERRUPT);
        assert_eq!(hw.writes_to(iir_selector(0)), [1 << 4, 1 << BIT_GUC]);
        assert_eq!(hw.writes_to(gt_intr_dw(0)), [1 << 4 | 1 << BIT_GUC]);
        assert_eq!(hw.writes_to(MSTR_TILE_INTR), [0, 1, 1 << 31]);
    }

    #[test]
    fn nothing_pending_reopens_the_master() {
        let mut hw = Sim::new();
        let ev = handle(&mut hw);
        assert_eq!(ev, Events::default());
        assert_eq!(hw.writes_to(MSTR_TILE_INTR), [0, 1 << 31]);
    }
}
