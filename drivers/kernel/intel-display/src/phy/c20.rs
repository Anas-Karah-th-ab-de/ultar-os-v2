//! C20 PHY (Type-C ports of display 14): PLL configuration tables for the DisplayPort 1.4 link
//! rates, the port clock they produce, the vendor-register values and the drive-level table.
//!
//! Licence note: the four `DP_*` tables, the SRAM context addresses, the vendor register layout and
//! `C20_DP14_TRANS` are taken from Linux' `intel_cx0_phy.c`, `intel_cx0_phy_regs.h` and
//! `intel_ddi_buf_trans.c` (Copyright © 2023 Intel Corporation, MIT licence). The values are
//! hardware programming data published by Intel.

/// One PLL configuration: TX, common and MPLL-B context words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C20Pll {
    /// TX context configuration 0..2.
    pub tx: [u16; 3],
    /// Common context configuration 0..3.
    pub cmn: [u16; 4],
    /// MPLL-B configuration 0..10.
    pub mpllb: [u16; 11],
}

/// RBR, 1.62 Gbit/s.
pub const DP_RBR: C20Pll = C20Pll {
    tx: [0xbe88, 0x5800, 0x0000],
    cmn: [0x0500, 0x0005, 0x0000, 0x0000],
    mpllb: [
        0x50a8, 0x2120, 0xcd9a, 0xbfc1, 0x5ab8, 0x4c34, 0x2000, 0x0001, 0x6000, 0x0000, 0x0000,
    ],
};
/// HBR, 2.7 Gbit/s.
pub const DP_HBR1: C20Pll = C20Pll {
    tx: [0xbe88, 0x4800, 0x0000],
    cmn: [0x0500, 0x0005, 0x0000, 0x0000],
    mpllb: [
        0x308c, 0x2110, 0xcc9c, 0xbfc1, 0x4b9a, 0x3f81, 0x2000, 0x0001, 0x5000, 0x0000, 0x0000,
    ],
};
/// HBR2, 5.4 Gbit/s.
pub const DP_HBR2: C20Pll = C20Pll {
    tx: [0xbe88, 0x4800, 0x0000],
    cmn: [0x0500, 0x0005, 0x0000, 0x0000],
    mpllb: [
        0x108c, 0x2108, 0xcc9c, 0xbfc1, 0x4b9a, 0x3f81, 0x2000, 0x0001, 0x5000, 0x0000, 0x0000,
    ],
};
/// HBR3, 8.1 Gbit/s.
pub const DP_HBR3: C20Pll = C20Pll {
    tx: [0xbe88, 0x4800, 0x0000],
    cmn: [0x0500, 0x0005, 0x0000, 0x0000],
    mpllb: [
        0x10d2, 0x2108, 0x8d98, 0xbfc1, 0x7166, 0x5f42, 0x2000, 0x0001, 0x7800, 0x0000, 0x0000,
    ],
};

/// The table for a DP link rate (kHz).
pub fn dp_table(rate: u32) -> Option<&'static C20Pll> {
    match rate {
        162_000 => Some(&DP_RBR),
        270_000 => Some(&DP_HBR1),
        540_000 => Some(&DP_HBR2),
        810_000 => Some(&DP_HBR3),
        _ => None,
    }
}

/// `C20_PHY_USE_MPLLB` in TX config 0.
pub const USE_MPLLB: u16 = 1 << 7;

fn div_round(a: u64, b: u64) -> u64 {
    (a + b / 2) / b
}

/// Port clock (kHz) a configuration produces (MPLL-B; reference 38.4 MHz).
pub fn port_clock(p: &C20Pll) -> u32 {
    let tx_rate = (p.tx[0] & 7) as u64;
    let den = p.mpllb[7].max(1) as u64;
    let frac = if p.mpllb[6] & (1 << 13) != 0 {
        p.mpllb[8] as u64 + div_round(p.mpllb[9] as u64, den)
    } else {
        0
    };
    let multiplier = (p.mpllb[0] & 0xfff) as u64;
    let tx_clk_div = (p.mpllb[0] >> 13) as u64;
    let ref_div = ((p.mpllb[6] >> 10) & 7) as u64;
    let reference = div_round(38_400 * 2, 1 << ref_div);
    let vco = div_round((reference * ((multiplier << 15) + frac)) >> 17, 10);
    ((vco << 1) >> tx_clk_div >> tx_rate) as u32
}

/// `PHY_C20_VDR_CUSTOM_SERDES_RATE`.
pub const VDR_SERDES_RATE: u16 = 0xd00;
/// `PHY_C20_VDR_HDMI_RATE`.
pub const VDR_HDMI_RATE: u16 = 0xd01;
/// `PHY_C20_VDR_CUSTOM_WIDTH`.
pub const VDR_CUSTOM_WIDTH: u16 = 0xd02;
/// Serdes rate: the context toggle (selects context A or B).
pub const CONTEXT_TOGGLE: u8 = 1;
/// Serdes rate: DisplayPort.
pub const IS_DP: u8 = 1 << 6;
/// Serdes rate: HDMI FRL.
pub const IS_HDMI_FRL: u8 = 1 << 7;
/// Serdes rate: the DP rate field.
pub const DP_RATE_MASK: u8 = 0b1_1110;
/// All serdes rate bits a PLL program sets.
pub const SERDES_MASK: u8 = IS_DP | DP_RATE_MASK | IS_HDMI_FRL;

/// The DP rate code of the serdes register.
pub fn dp_rate_code(rate: u32) -> Option<u8> {
    match rate {
        162_000 => Some(0),
        270_000 => Some(1),
        540_000 => Some(2),
        810_000 => Some(3),
        _ => None,
    }
}

/// Serdes rate register value for DP at `rate`.
pub fn serdes_dp(rate: u32) -> Option<u8> {
    dp_rate_code(rate).map(|c| IS_DP | (c << 1))
}

/// SRAM address of TX context word `i` (`b` = context B).
pub fn tx_cfg(b: bool, i: usize) -> u16 {
    (if b { 0xcf2a } else { 0xcf2e }) - i as u16
}
/// SRAM address of common context word `i`.
pub fn cmn_cfg(b: bool, i: usize) -> u16 {
    (if b { 0xcda5 } else { 0xcdaa }) - i as u16
}
/// SRAM address of MPLL-B context word `i`.
pub fn mpllb_cfg(b: bool, i: usize) -> u16 {
    (if b { 0xcb4e } else { 0xcb5a }) - i as u16
}

/// SRAM indirect access registers.
pub mod sram {
    /// Write address high byte.
    pub const WR_ADDR_H: u16 = 0xc03;
    /// Write address low byte.
    pub const WR_ADDR_L: u16 = 0xc02;
    /// Write data high byte.
    pub const WR_DATA_H: u16 = 0xc05;
    /// Write data low byte (committed write triggers the SRAM write).
    pub const WR_DATA_L: u16 = 0xc04;
    /// Read address high byte.
    pub const RD_ADDR_H: u16 = 0xc07;
    /// Read address low byte.
    pub const RD_ADDR_L: u16 = 0xc06;
    /// Read data high byte.
    pub const RD_DATA_H: u16 = 0xc09;
    /// Read data low byte.
    pub const RD_DATA_L: u16 = 0xc08;
}

/// Drive levels (vswing, pre-cursor, post-cursor) of DP 1.4 rates, indexed as
/// [`crate::dp::training::level_index`].
pub const C20_DP14_TRANS: [(u8, u8, u8); 10] = [
    (20, 0, 0),
    (24, 0, 4),
    (30, 0, 9),
    (34, 0, 14),
    (29, 0, 0),
    (34, 0, 5),
    (38, 0, 10),
    (36, 0, 0),
    (40, 0, 6),
    (48, 0, 0),
];

/// PHY register of the drive override `control` (0 pre-cursor, 1 vswing, 2 post-cursor) of
/// `tx` (0, 1) on PHY lane `lane` (0, 1).
pub fn vdrovrd(lane: u8, tx: u8, control: u16) -> u16 {
    0xd80 + ((lane ^ tx) as u16) * 0x10 + control
}
/// Override enable register (TX1 | TX2).
pub const VDR_OVRD: u16 = 0xd71;
/// `PHY_CX0_TX_CONTROL(tx, 2)`: lane transmitter control (bit 6 = disable this TX).
pub fn tx_control2(tx: u8) -> u16 {
    0x400 + (tx as u16 - 1) * 0x200 + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_produce_their_rates() {
        for rate in [162_000, 270_000, 540_000, 810_000] {
            let t = dp_table(rate).unwrap();
            assert_eq!(port_clock(t), rate, "rate {rate}");
            assert!(t.tx[0] & USE_MPLLB != 0);
        }
        assert!(dp_table(100).is_none());
    }

    #[test]
    fn captured_pll_state_is_rbr() {
        // i915_shared_dplls_info of the Beelink: TC PLL 4, tx 0xbe88/0x5800, mpllb[0] 0x50a8, serdes 0x40
        assert_eq!(DP_RBR.tx[..2], [0xbe88, 0x5800]);
        assert_eq!(DP_RBR.mpllb[0], 0x50a8);
        assert_eq!(serdes_dp(162_000), Some(0x40));
        assert_eq!(serdes_dp(810_000), Some(0x46));
        assert_eq!(serdes_dp(1), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(tx_cfg(false, 0), 0xcf2e);
        assert_eq!(tx_cfg(true, 2), 0xcf28);
        assert_eq!(cmn_cfg(false, 3), 0xcda7);
        assert_eq!(mpllb_cfg(true, 10), 0xcb44);
        assert_eq!(vdrovrd(0, 0, 1), 0xd81);
        assert_eq!(vdrovrd(1, 0, 2), 0xd92);
        assert_eq!(vdrovrd(1, 1, 0), 0xd80);
        assert_eq!(tx_control2(1), 0x402);
        assert_eq!(tx_control2(2), 0x602);
    }
}
