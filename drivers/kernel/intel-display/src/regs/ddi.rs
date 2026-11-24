//! Transcoder-to-port routing (`TRANS_DDI_FUNC_CTL`), port buffers (`DDI_BUF_CTL`), DisplayPort
//! transport control (`DP_TP_CTL`) and the transcoder clock select.

use super::trans;
use crate::hw::{field, genmask, prep};

/// `TRANS_DDI_FUNC_CTL` of a transcoder.
pub const fn func_ctl(t: u8) -> u32 {
    trans(t, 0x400)
}
/// `TRANS_MSA_MISC` of a transcoder.
pub const fn msa_misc(t: u8) -> u32 {
    trans(t, 0x410)
}
/// `DP_TP_CTL` of a transcoder (display 12+).
pub const fn dp_tp_ctl(t: u8) -> u32 {
    trans(t, 0x540)
}
/// `DP_TP_STATUS` of a transcoder (display 12+).
pub const fn dp_tp_status(t: u8) -> u32 {
    trans(t, 0x544)
}
/// `TRANS_CLK_SEL` of a transcoder.
pub const fn clk_sel(t: u8) -> u32 {
    0x46140 + t as u32 * 4
}
/// `DDI_BUF_CTL` (MTL: `DDI_CTL_DE`) of a port.
pub const fn buf_ctl(port: u8) -> u32 {
    0x64000 + port as u32 * 0x100
}
/// `DDI_CLK_VALFREQ` of a port (display 14).
pub const fn clk_valfreq(port: u8) -> u32 {
    0x64030 + port as u32 * 0x100
}

/// Function enable.
pub const FUNC_ENABLE: u32 = 1 << 31;
/// Port select, display 12+: port + 1 in 30:27.
pub const FUNC_PORT_MASK_TGL: u32 = genmask(30, 27);
/// Port select, display 9: port in 30:28.
pub const FUNC_PORT_MASK_SKL: u32 = genmask(30, 28);
/// Mode select.
pub const FUNC_MODE_MASK: u32 = genmask(26, 24);
/// Bits per colour.
pub const FUNC_BPC_MASK: u32 = genmask(22, 20);
/// Positive vertical sync.
pub const FUNC_PVSYNC: u32 = 1 << 17;
/// Positive horizontal sync.
pub const FUNC_PHSYNC: u32 = 1 << 16;
/// Lane count − 1.
pub const FUNC_WIDTH_MASK: u32 = genmask(3, 1);

/// What a transcoder sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdiMode {
    /// HDMI (TMDS with data islands).
    Hdmi,
    /// DVI (TMDS).
    Dvi,
    /// DisplayPort single stream.
    DpSst,
    /// DisplayPort multi stream.
    DpMst,
    /// FDI or 128b/132b (not used by this driver).
    Other,
}

impl DdiMode {
    /// From the mode field.
    pub fn from_bits(v: u32) -> DdiMode {
        match field(v, FUNC_MODE_MASK) {
            0 => DdiMode::Hdmi,
            1 => DdiMode::Dvi,
            2 => DdiMode::DpSst,
            3 => DdiMode::DpMst,
            _ => DdiMode::Other,
        }
    }
    /// The mode field.
    pub fn bits(self) -> u32 {
        prep(
            FUNC_MODE_MASK,
            match self {
                DdiMode::Hdmi => 0,
                DdiMode::Dvi => 1,
                DdiMode::DpSst => 2,
                DdiMode::DpMst => 3,
                DdiMode::Other => 4,
            },
        )
    }
}

/// Bits per colour component from the BPC field (8 when the field is unknown).
pub fn bpc_from_bits(v: u32) -> u8 {
    match field(v, FUNC_BPC_MASK) {
        1 => 10,
        2 => 6,
        3 => 12,
        _ => 8,
    }
}

/// The BPC field for `bpc` (6, 8, 10, 12).
pub fn bpc_bits(bpc: u8) -> Option<u32> {
    let v = match bpc {
        8 => 0,
        10 => 1,
        6 => 2,
        12 => 3,
        _ => return None,
    };
    Some(prep(FUNC_BPC_MASK, v))
}

/// The port a transcoder feeds (`None`: no port selected).
pub fn port_from_bits(v: u32, display_ver: u8) -> Option<u8> {
    if display_ver >= 12 {
        field(v, FUNC_PORT_MASK_TGL).checked_sub(1).map(|p| p as u8)
    } else {
        Some(field(v, FUNC_PORT_MASK_SKL) as u8)
    }
}

/// The port select field for `port` on display 12+.
pub fn port_bits_tgl(port: u8) -> u32 {
    prep(FUNC_PORT_MASK_TGL, port as u32 + 1)
}

/// `DDI_BUF_CTL`: buffer enable.
pub const BUF_ENABLE: u32 = 1 << 31;
/// `DDI_BUF_CTL`: port data width (display 14: 0 = 10 bit, 2 = 40 bit).
pub const BUF_DATA_MASK: u32 = genmask(19, 18);
/// `DDI_BUF_CTL`: lane reversal.
pub const BUF_REVERSAL: u32 = 1 << 16;
/// `DDI_BUF_CTL`: buffer is idle.
pub const BUF_IS_IDLE: u32 = 1 << 7;
/// `DDI_BUF_CTL`: port width.
pub const BUF_WIDTH_MASK: u32 = genmask(3, 1);

/// The `DDI_BUF_CTL` port width field for `lanes` (1, 2, 4).
pub fn buf_width_bits(lanes: u8) -> u32 {
    prep(
        BUF_WIDTH_MASK,
        if lanes == 3 {
            4
        } else {
            lanes.max(1) as u32 - 1
        },
    )
}

/// `DP_TP_CTL`: enable.
pub const TP_ENABLE: u32 = 1 << 31;
/// `DP_TP_CTL`: enhanced framing.
pub const TP_ENHANCED_FRAMING: u32 = 1 << 18;
/// `DP_TP_CTL`: link training field.
pub const TP_TRAIN_MASK: u32 = genmask(10, 8);
/// `DP_TP_STATUS`: idle patterns were sent.
pub const TP_STATUS_IDLE_DONE: u32 = 1 << 25;

/// Pattern the transport sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainPattern {
    /// TPS1 (clock recovery).
    Tps1,
    /// TPS2 (channel equalisation).
    Tps2,
    /// TPS3.
    Tps3,
    /// TPS4.
    Tps4,
    /// Idle pattern.
    Idle,
    /// Normal pixel data.
    Normal,
}

impl TrainPattern {
    /// The `DP_TP_CTL` training field.
    pub fn bits(self) -> u32 {
        prep(
            TP_TRAIN_MASK,
            match self {
                TrainPattern::Tps1 => 0,
                TrainPattern::Tps2 => 1,
                TrainPattern::Idle => 2,
                TrainPattern::Normal => 3,
                TrainPattern::Tps3 => 4,
                TrainPattern::Tps4 => 5,
            },
        )
    }
}

/// `TRANS_CLK_SEL` value for PHY `phy` on display 12+ (0 = off).
pub fn clk_sel_bits(phy: u8) -> u32 {
    (phy as u32 + 1) << 28
}

/// `TRANS_MSA_MISC` for RGB with `bpc` bits per colour: synchronous clock + colour depth.
pub fn msa_misc_bits(bpc: u8) -> u32 {
    let depth = match bpc {
        6 => 0,
        10 => 2,
        12 => 3,
        _ => 1,
    };
    1 | (depth << 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_beelink_transcoder() {
        let v = 0xba03_0006;
        assert!(v & FUNC_ENABLE != 0);
        assert_eq!(port_from_bits(v, 14), Some(6));
        assert_eq!(DdiMode::from_bits(v), DdiMode::DpSst);
        assert_eq!(bpc_from_bits(v), 8);
        assert_eq!(field(v, FUNC_WIDTH_MASK) + 1, 4);
        assert_eq!(v & (FUNC_PVSYNC | FUNC_PHSYNC), FUNC_PVSYNC | FUNC_PHSYNC);
        // re-encoding gives the same bits
        let enc = FUNC_ENABLE
            | port_bits_tgl(6)
            | DdiMode::DpSst.bits()
            | bpc_bits(8).unwrap()
            | FUNC_PVSYNC
            | FUNC_PHSYNC
            | prep(FUNC_WIDTH_MASK, 3);
        assert_eq!(enc, v);
    }

    #[test]
    fn port_select_and_modes() {
        assert_eq!(port_from_bits(0, 14), None);
        assert_eq!(port_from_bits(3 << 28, 9), Some(3));
        for m in [
            DdiMode::Hdmi,
            DdiMode::Dvi,
            DdiMode::DpSst,
            DdiMode::DpMst,
            DdiMode::Other,
        ] {
            assert_eq!(DdiMode::from_bits(m.bits()), m);
        }
        for b in [6, 8, 10, 12] {
            assert_eq!(bpc_from_bits(bpc_bits(b).unwrap()), b);
        }
        assert_eq!(bpc_bits(7), None);
    }

    #[test]
    fn offsets_and_fields() {
        assert_eq!(func_ctl(0), 0x60400);
        assert_eq!(dp_tp_ctl(1), 0x61540);
        assert_eq!(dp_tp_status(0), 0x60544);
        assert_eq!(clk_sel(0), 0x46140);
        assert_eq!(buf_ctl(6), 0x64600);
        assert_eq!(clk_valfreq(6), 0x64630);
        assert_eq!(msa_misc(0), 0x60410);
        // TC4 is PHY I (8): the captured TRANS_CLK_SEL_A was 0x90000000
        assert_eq!(clk_sel_bits(8), 0x9000_0000);
        assert_eq!(buf_width_bits(4), 3 << 1);
        assert_eq!(buf_width_bits(1), 0);
        assert_eq!(buf_width_bits(3), 4 << 1);
        assert_eq!(TrainPattern::Normal.bits(), 3 << 8);
        assert_eq!(TrainPattern::Tps4.bits(), 5 << 8);
        assert_eq!(TrainPattern::Tps1.bits(), 0);
        assert_eq!(TrainPattern::Tps2.bits(), 1 << 8);
        assert_eq!(TrainPattern::Tps3.bits(), 4 << 8);
        assert_eq!(TrainPattern::Idle.bits(), 2 << 8);
        assert_eq!(msa_misc_bits(8), 0x21);
        assert_eq!(msa_misc_bits(10), 0x41);
        assert_eq!(msa_misc_bits(6), 0x01);
        assert_eq!(msa_misc_bits(12), 0x61);
    }
}
