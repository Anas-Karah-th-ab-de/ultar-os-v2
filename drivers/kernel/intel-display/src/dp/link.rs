//! Link configuration for DisplayPort single stream with 8b/10b coding: choosing rate and lane count
//! for a mode, and the data/link M/N values of the transcoder (the formula of Linux'
//! `intel_link_compute_m_n`, MIT, with fixed N values as the firmware uses them).

use crate::error::{Error, Result};

/// Link rates (kHz port clock) the driver trains, ascending.
pub const RATES: [u32; 4] = [162_000, 270_000, 540_000, 810_000];

/// Rate and width of a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkConfig {
    /// Port clock in kHz (162 000 = RBR).
    pub rate: u32,
    /// Lanes (1, 2, 4).
    pub lanes: u8,
}

/// The four M/N values and the transfer unit size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MN {
    /// Transfer unit size (64).
    pub tu: u32,
    /// Data M.
    pub data_m: u32,
    /// Data N.
    pub data_n: u32,
    /// Link M.
    pub link_m: u32,
    /// Link N.
    pub link_n: u32,
}

const MN_MAX: u64 = 0xff_ffff;

fn reduce(mut m: u64, mut n: u64) -> (u32, u32) {
    while m > MN_MAX || n > MN_MAX {
        m >>= 1;
        n >>= 1;
    }
    (m as u32, n as u32)
}

fn ratio(m: u64, n: u64, constant_n: u64) -> (u32, u32) {
    reduce(m * constant_n / n.max(1), constant_n)
}

/// Payload bytes per millisecond of a mode (`pixel_khz` × bits per pixel / 8, rounded up).
pub fn data_rate(pixel_khz: u32, bpp: u32) -> u64 {
    (pixel_khz as u64 * bpp as u64).div_ceil(8)
}

/// Capacity of a link in payload bytes per millisecond (8b/10b: one byte per symbol and lane).
pub fn capacity(cfg: LinkConfig) -> u64 {
    cfg.rate as u64 * cfg.lanes as u64
}

/// M/N values of a mode on a link.
pub fn compute_mn(pixel_khz: u32, bpp: u32, cfg: LinkConfig) -> MN {
    let (data_m, data_n) = ratio(data_rate(pixel_khz, bpp), capacity(cfg), 0x800_0000);
    let (link_m, link_n) = ratio(pixel_khz as u64, cfg.rate as u64, 0x8_0000);
    MN {
        tu: 64,
        data_m,
        data_n,
        link_m,
        link_n,
    }
}

/// The slowest link (lowest rate first, then fewest lanes – the order Linux uses) that carries the
/// mode within the sink's and the source's limits, with 0.5 % reserve.
pub fn choose(pixel_khz: u32, bpp: u32, max_rate: u32, max_lanes: u8) -> Result<LinkConfig> {
    let need = data_rate(pixel_khz, bpp) * 1000;
    for rate in RATES.iter().copied().filter(|r| *r <= max_rate) {
        for lanes in [1u8, 2, 4].into_iter().filter(|l| *l <= max_lanes) {
            let cfg = LinkConfig { rate, lanes };
            if need <= capacity(cfg) * 995 {
                return Ok(cfg);
            }
        }
    }
    Err(Error::Invalid(
        "mode needs more bandwidth than the link has",
    ))
}

/// Limits for choosing a link: highest rate and most lanes allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Highest rate (kHz).
    pub max_rate: u32,
    /// Most lanes.
    pub max_lanes: u8,
}

/// The limits to retry with after `failed` did not train (DP 1.4 §3.5.1.2.2 / Linux'
/// `intel_dp_get_link_train_fallback_values`): first lower the rate below the failed one at the
/// same lane limit; at the lowest rate halve the lanes and allow the sink's highest rate again.
pub fn fallback_limits(failed: LinkConfig, cur: Limits, sink_max_rate: u32) -> Option<Limits> {
    if let Some(lower) = RATES.iter().rev().copied().find(|r| *r < failed.rate) {
        return Some(Limits {
            max_rate: lower,
            max_lanes: cur.max_lanes.min(failed.lanes),
        });
    }
    (failed.lanes > 1).then_some(Limits {
        max_rate: sink_max_rate,
        max_lanes: failed.lanes / 2,
    })
}

/// The next config to train after `failed`, under fallback limits; `None` when nothing is left.
/// Updates `limits`.
pub fn next_after_failure(
    failed: LinkConfig,
    limits: &mut Limits,
    sink_max_rate: u32,
    pixel_khz: u32,
    bpp: u32,
) -> Option<LinkConfig> {
    let mut f = failed;
    loop {
        *limits = fallback_limits(f, *limits, sink_max_rate)?;
        match choose(pixel_khz, bpp, limits.max_rate, limits.max_lanes) {
            Ok(c) => return Some(c),
            // nothing fits under these limits: fall back further as if the slowest one had failed
            Err(_) => {
                f = LinkConfig {
                    rate: RATES[0],
                    lanes: limits.max_lanes,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beelink_mode_gives_the_captured_values() {
        let cfg = choose(148_500, 24, 540_000, 4).unwrap();
        assert_eq!(
            cfg,
            LinkConfig {
                rate: 162_000,
                lanes: 4
            }
        );
        let mn = compute_mn(148_500, 24, cfg);
        // PIPEA_DATA_M1 0x7e580000 (TU 64), DATA_N1 0x800000, LINK_M1 0x75555, LINK_N1 0x80000
        assert_eq!(
            mn,
            MN {
                tu: 64,
                data_m: 0x58_0000,
                data_n: 0x80_0000,
                link_m: 0x7_5555,
                link_n: 0x8_0000
            }
        );
    }

    #[test]
    fn choice_respects_limits() {
        // 4K60 (594 MHz) needs HBR2 x4
        assert_eq!(
            choose(594_000, 24, 810_000, 4).unwrap(),
            LinkConfig {
                rate: 540_000,
                lanes: 4
            }
        );
        // a two-lane sink at RBR cannot carry 1080p60
        assert!(choose(148_500, 24, 162_000, 2).is_err());
        // 640x480 fits one RBR lane
        assert_eq!(
            choose(25_175, 24, 162_000, 4).unwrap(),
            LinkConfig {
                rate: 162_000,
                lanes: 1
            }
        );
    }

    fn cfg(rate: u32, lanes: u8) -> LinkConfig {
        LinkConfig { rate, lanes }
    }

    fn lim(max_rate: u32, max_lanes: u8) -> Limits {
        Limits {
            max_rate,
            max_lanes,
        }
    }

    #[test]
    fn fallback_follows_the_dp_order() {
        // 1080p60 on a HBR2 x4 sink: RBR x4 first, then 2 lanes (HBR x2), then 1 lane (HBR2 x1)
        let sink = 540_000;
        let mut l = lim(sink, 4);
        let first = choose(148_500, 24, l.max_rate, l.max_lanes).unwrap();
        assert_eq!(first, cfg(162_000, 4));
        let second = next_after_failure(first, &mut l, sink, 148_500, 24).unwrap();
        assert_eq!(second, cfg(270_000, 2));
        assert_eq!(l, lim(sink, 2));
        let third = next_after_failure(second, &mut l, sink, 148_500, 24).unwrap();
        assert_eq!(third, cfg(540_000, 1));
        assert_eq!(next_after_failure(third, &mut l, sink, 148_500, 24), None);
    }

    #[test]
    fn fallback_limits_and_their_end() {
        // 4K60 at HBR2 x4: nothing slower carries it
        let sink = 540_000;
        let mut l4 = lim(sink, 4);
        let big = choose(594_000, 24, sink, 4).unwrap();
        assert_eq!(next_after_failure(big, &mut l4, sink, 594_000, 24), None);
        assert_eq!(
            fallback_limits(cfg(270_000, 4), lim(270_000, 4), 270_000),
            Some(lim(162_000, 4))
        );
        assert_eq!(
            fallback_limits(cfg(162_000, 1), lim(162_000, 1), 540_000),
            None
        );
    }

    #[test]
    fn m_n_stays_in_24_bits() {
        for &rate in &RATES {
            let mn = compute_mn(1_000_000, 30, LinkConfig { rate, lanes: 4 });
            assert!(mn.data_m <= 0xff_ffff && mn.data_n <= 0xff_ffff);
            assert!(mn.link_m <= 0xff_ffff && mn.link_n <= 0xff_ffff);
        }
    }
}
