//! Interrupt registers of the display engine. The Ultar kernel has no MSI routing for PCI devices
//! yet, so the driver unmasks the sources it needs (so that they latch in the identity registers)
//! and reads and clears the identity registers from its polling path; the master enables stay off.

/// `GEN8_DE_PIPE_ISR` (live status).
pub const fn pipe_isr(pipe: u8) -> u32 {
    0x44400 + 0x10 * pipe as u32
}
/// `GEN8_DE_PIPE_IMR` (mask: 1 = not latched).
pub const fn pipe_imr(pipe: u8) -> u32 {
    0x44404 + 0x10 * pipe as u32
}
/// `GEN8_DE_PIPE_IIR` (latched identity, write 1 to clear).
pub const fn pipe_iir(pipe: u8) -> u32 {
    0x44408 + 0x10 * pipe as u32
}
/// `GEN8_DE_PIPE_IER` (enable towards the master interrupt).
pub const fn pipe_ier(pipe: u8) -> u32 {
    0x4440c + 0x10 * pipe as u32
}

/// Pipe: FIFO underrun.
pub const PIPE_FIFO_UNDERRUN: u32 = 1 << 31;
/// Pipe: CRC done.
pub const PIPE_CRC_DONE: u32 = 1 << 28;
/// Pipe: plane 1 flip done (display 9+).
pub const PIPE_PLANE1_FLIP_DONE: u32 = 1 << 3;
/// Pipe: vertical sync.
pub const PIPE_VSYNC: u32 = 1 << 1;
/// Pipe: vertical blank.
pub const PIPE_VBLANK: u32 = 1 << 0;
/// The sources this driver latches.
pub const PIPE_SOURCES: u32 =
    PIPE_FIFO_UNDERRUN | PIPE_CRC_DONE | PIPE_PLANE1_FLIP_DONE | PIPE_VBLANK;

/// Display 11+: `GEN11_DISPLAY_INT_CTL` (display master); display 9: `GEN8_MASTER_IRQ`.
pub const DISPLAY_INT_CTL: u32 = 0x44200;

/// Display 14: `PICAINTERRUPT_ISR` (Type-C live status: DP-alt and Thunderbolt hot-plug).
pub const PICA_ISR: u32 = 0x16fe50;
/// `PICAINTERRUPT_IMR`.
pub const PICA_IMR: u32 = 0x16fe54;
/// `PICAINTERRUPT_IIR`.
pub const PICA_IIR: u32 = 0x16fe58;
/// `PICAINTERRUPT_IER`.
pub const PICA_IER: u32 = 0x16fe5c;
/// PICA: DP-alt hot-plug of Type-C index `tc`.
pub const fn pica_dp_alt(tc: u8) -> u32 {
    1 << (16 + tc as u32)
}
/// PICA: Thunderbolt hot-plug of Type-C index `tc`.
pub const fn pica_tbt(tc: u8) -> u32 {
    1 << tc as u32
}

/// South display engine `SDEISR` (live status of the PCH hot-plug pins).
pub const SDE_ISR: u32 = 0xc4000;
/// `SDEIMR`.
pub const SDE_IMR: u32 = 0xc4004;
/// `SDEIIR`.
pub const SDE_IIR: u32 = 0xc4008;
/// `SDEIER`.
pub const SDE_IER: u32 = 0xc400c;
/// SDE: legacy Type-C connector hot-plug (ICP+; a Type-C PHY wired to a fixed DP/HDMI connector).
pub const fn sde_tc(tc: u8) -> u32 {
    1 << (24 + tc as u32)
}
/// SDE: combo port hot-plug (ICP+) of port A/B.
pub const fn sde_ddi(port: u8) -> u32 {
    1 << (16 + port as u32)
}
/// `SHOTPLUG_CTL_DDI` (ICP+): hot-plug detection enable for ports A/B.
pub const SHOTPLUG_CTL_DDI: u32 = 0xc4030;
/// `SHOTPLUG_CTL_TC` (ICP+): hot-plug detection enable for legacy Type-C connectors.
pub const SHOTPLUG_CTL_TC: u32 = 0xc4034;
/// SHOTPLUG_CTL_DDI enable bit of port A/B.
pub const fn shotplug_ddi_enable(port: u8) -> u32 {
    0x8 << (4 * port as u32)
}
/// SHOTPLUG_CTL_TC enable bit of Type-C index `tc`.
pub const fn shotplug_tc_enable(tc: u8) -> u32 {
    0x8 << (4 * tc as u32)
}
/// Display 14 `XELPDP_PORT_HOTPLUG_CTL`: DP-alt and Thunderbolt detection enable.
pub const XELPDP_HOTPLUG_ENABLE: u32 = (1 << 6) | (1 << 2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_and_bits() {
        assert_eq!(pipe_isr(0), 0x44400);
        assert_eq!(pipe_iir(1), 0x44418);
        assert_eq!(pipe_imr(3), 0x44434);
        assert_eq!(pipe_ier(2), 0x4442c);
        assert_eq!(pica_dp_alt(3), 1 << 19);
        assert_eq!(pica_tbt(3), 1 << 3);
        // the captured SDEISR 0x0b003006 has the legacy hot-plug bit of TC4 (bit 27) set
        assert!(0x0b00_3006 & sde_tc(3) != 0);
        assert_eq!(sde_ddi(1), 1 << 17);
        assert_eq!(shotplug_tc_enable(3), 0x8000);
        assert_eq!(shotplug_ddi_enable(0), 0x8);
    }
}
