//! DisplayPort configuration data (DPCD) of the sink: the addresses this driver uses (VESA DP 1.4
//! §2.9.3) and parsing of the receiver capabilities and the lane status.

use crate::error::{Error, Result};

/// `DPCD_REV`.
pub const DPCD_REV: u32 = 0x000;
/// `LINK_BW_SET`.
pub const LINK_BW_SET: u32 = 0x100;
/// `LANE_COUNT_SET` (bit 7: enhanced framing).
pub const LANE_COUNT_SET: u32 = 0x101;
/// `TRAINING_PATTERN_SET`.
pub const TRAINING_PATTERN_SET: u32 = 0x102;
/// `TRAINING_LANE0_SET` (…3 follow).
pub const TRAINING_LANE0_SET: u32 = 0x103;
/// `DOWNSPREAD_CTRL`.
pub const DOWNSPREAD_CTRL: u32 = 0x107;
/// `MAIN_LINK_CHANNEL_CODING_SET` (1 = 8b/10b).
pub const CHANNEL_CODING_SET: u32 = 0x108;
/// `SINK_COUNT`.
pub const SINK_COUNT: u32 = 0x200;
/// `LANE0_1_STATUS` (…`LANE2_3_STATUS`, `LANE_ALIGN_STATUS_UPDATED`, `SINK_STATUS`, `ADJUST_REQUEST_*` follow).
pub const LANE0_1_STATUS: u32 = 0x202;
/// `SET_POWER` (1 = D0, 2 = D3).
pub const SET_POWER: u32 = 0x600;
/// Extended receiver capability field (DPCD 1.4: a copy of 0x000.. with the real values).
pub const EXTENDED_CAPS: u32 = 0x2200;

/// Pattern value for `TRAINING_PATTERN_SET`: off.
pub const TPS_OFF: u8 = 0;
/// TPS1.
pub const TPS1: u8 = 1;
/// TPS2.
pub const TPS2: u8 = 2;
/// TPS3.
pub const TPS3: u8 = 3;
/// TPS4 (no scrambling-disable bit with it).
pub const TPS4: u8 = 7;
/// Scrambling off (with TPS1..3).
pub const SCRAMBLING_DISABLE: u8 = 1 << 5;

/// Link rate of a `MAX_LINK_RATE`/`LINK_BW_SET` code in kHz of port clock (code × 27 000).
pub fn rate_from_code(code: u8) -> Option<u32> {
    matches!(code, 0x06 | 0x0a | 0x14 | 0x1e).then_some(code as u32 * 27_000)
}

/// The `LINK_BW_SET` code of a port clock in kHz.
pub fn code_from_rate(rate: u32) -> Option<u8> {
    match rate {
        162_000 => Some(0x06),
        270_000 => Some(0x0a),
        540_000 => Some(0x14),
        810_000 => Some(0x1e),
        _ => None,
    }
}

/// What the sink can do (from the first 16 bytes of DPCD).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// DPCD revision (0x12 = 1.2, 0x14 = 1.4).
    pub rev: u8,
    /// Highest link rate in kHz.
    pub max_rate: u32,
    /// Most lanes (1, 2, 4).
    pub max_lanes: u8,
    /// Enhanced framing.
    pub enhanced_framing: bool,
    /// TPS3 supported.
    pub tps3: bool,
    /// TPS4 supported.
    pub tps4: bool,
    /// Clock-recovery wait in µs and channel-equalisation wait in µs.
    pub aux_rd_interval_us: (u32, u32),
}

impl Caps {
    /// Parses receiver capabilities.
    pub fn parse(d: &[u8; 16]) -> Result<Caps> {
        if d[0] == 0 || d[0] > 0x14 && d[0] != 0x20 {
            return Err(Error::Invalid("DPCD revision"));
        }
        let max_rate = rate_from_code(d[1]).ok_or(Error::Invalid("DPCD link rate"))?;
        let max_lanes = d[2] & 0x1f;
        if !matches!(max_lanes, 1 | 2 | 4) {
            return Err(Error::Invalid("DPCD lane count"));
        }
        let interval = d[0x0e] & 0x7f;
        let eq = match interval {
            0 => 400,
            n => (n as u32 * 4000).min(16_000),
        };
        Ok(Caps {
            rev: d[0],
            max_rate,
            max_lanes,
            enhanced_framing: d[2] & 0x80 != 0,
            tps3: d[2] & 0x40 != 0,
            tps4: d[3] & 0x80 != 0,
            aux_rd_interval_us: (
                if d[0] >= 0x14 || interval == 0 {
                    100
                } else {
                    eq
                },
                eq,
            ),
        })
    }

    /// Whether byte 0x0e says that the extended capability field at 0x2200 holds the real values.
    pub fn has_extended(d: &[u8; 16]) -> bool {
        d[0x0e] & 0x80 != 0
    }
}

/// Lane status bits of one lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneStatus {
    /// Clock recovery done.
    pub cr_done: bool,
    /// Channel equalisation done.
    pub eq_done: bool,
    /// Symbol lock.
    pub symbol_locked: bool,
}

/// Link status: bytes 0x202..=0x207.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkStatus {
    /// Lane status of lanes 0..4.
    pub lanes: [LaneStatus; 4],
    /// Inter-lane alignment done.
    pub aligned: bool,
    /// Requested (voltage swing, pre-emphasis) of lanes 0..4.
    pub adjust: [(u8, u8); 4],
}

impl LinkStatus {
    /// Parses the six status bytes from 0x202.
    pub fn parse(b: &[u8; 6]) -> LinkStatus {
        let mut s = LinkStatus {
            aligned: b[2] & 1 != 0,
            ..Default::default()
        };
        for l in 0..4 {
            let nib = b[l / 2] >> (4 * (l % 2));
            s.lanes[l] = LaneStatus {
                cr_done: nib & 1 != 0,
                eq_done: nib & 2 != 0,
                symbol_locked: nib & 4 != 0,
            };
            let adj = b[4 + l / 2] >> (4 * (l % 2));
            s.adjust[l] = (adj & 3, (adj >> 2) & 3);
        }
        s
    }

    /// Clock recovery done on the first `lanes` lanes.
    pub fn cr_ok(&self, lanes: u8) -> bool {
        self.lanes[..lanes as usize].iter().all(|l| l.cr_done)
    }

    /// Channel equalisation, symbol lock and alignment done on the first `lanes` lanes.
    pub fn eq_ok(&self, lanes: u8) -> bool {
        self.aligned
            && self.lanes[..lanes as usize]
                .iter()
                .all(|l| l.cr_done && l.eq_done && l.symbol_locked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beelink_monitor_caps() {
        // DPCD 1.2, HBR2, 4 lanes + enhanced framing + TPS3
        let mut d = [0u8; 16];
        d[..4].copy_from_slice(&[0x12, 0x14, 0xc4, 0x01]);
        let c = Caps::parse(&d).unwrap();
        assert_eq!(
            c,
            Caps {
                rev: 0x12,
                max_rate: 540_000,
                max_lanes: 4,
                enhanced_framing: true,
                tps3: true,
                tps4: false,
                aux_rd_interval_us: (100, 400)
            }
        );
        d[0x0e] = 0x82;
        assert!(Caps::has_extended(&d));
        assert_eq!(Caps::parse(&d).unwrap().aux_rd_interval_us, (8000, 8000));
        d[0] = 0x14;
        d[3] = 0x81;
        let c4 = Caps::parse(&d).unwrap();
        assert!(c4.tps4);
        assert_eq!(c4.aux_rd_interval_us.0, 100);
    }

    #[test]
    fn bad_caps() {
        let mut d = [0u8; 16];
        assert_eq!(Caps::parse(&d), Err(Error::Invalid("DPCD revision")));
        d[0] = 0x12;
        d[1] = 0x07;
        assert_eq!(Caps::parse(&d), Err(Error::Invalid("DPCD link rate")));
        d[1] = 0x06;
        d[2] = 3;
        assert_eq!(Caps::parse(&d), Err(Error::Invalid("DPCD lane count")));
    }

    #[test]
    fn rates() {
        assert_eq!(rate_from_code(0x06), Some(162_000));
        assert_eq!(rate_from_code(0x1e), Some(810_000));
        assert_eq!(rate_from_code(0x07), None);
        for r in [162_000, 270_000, 540_000, 810_000] {
            assert_eq!(rate_from_code(code_from_rate(r).unwrap()), Some(r));
        }
        assert_eq!(code_from_rate(1), None);
    }

    #[test]
    fn link_status() {
        // lanes 0,1 fully trained, lane 2 CR only, lane 3 nothing; aligned; lane1 asks swing 2 pre 1
        let s = LinkStatus::parse(&[0x77, 0x01, 0x01, 0x00, 0x60, 0x00]);
        assert!(s.lanes[0].eq_done && s.lanes[1].symbol_locked);
        assert!(s.cr_ok(3) && !s.cr_ok(4));
        assert!(s.eq_ok(2) && !s.eq_ok(3));
        assert_eq!(s.adjust[1], (2, 1));
        assert_eq!(s.adjust[0], (0, 0));
        let unaligned = LinkStatus::parse(&[0x77, 0x77, 0x00, 0, 0, 0]);
        assert!(!unaligned.eq_ok(4));
    }
}
