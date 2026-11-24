//! DisplayPort link training (VESA DP 1.4 §3.5.1.2): clock recovery with TPS1, then channel
//! equalisation with TPS2/3/4, adjusting voltage swing and pre-emphasis as the sink asks. The
//! algorithm is pure; the source side and the DPCD access come through [`Trainer`].

use super::dpcd::{self, Caps, LinkStatus};
use super::link::LinkConfig;
use crate::error::{Error, Result};
use crate::regs::ddi::TrainPattern;

/// Voltage swing and pre-emphasis level of a lane (each 0..=3, sum ≤ 3).
pub type Level = (u8, u8);

/// What link training needs from the hardware.
pub trait Trainer {
    /// Sets the pattern the source sends.
    fn source_pattern(&mut self, p: TrainPattern) -> Result<()>;
    /// Sets the source's drive level (one level for all lanes).
    fn source_level(&mut self, level: Level) -> Result<()>;
    /// Writes DPCD.
    fn dpcd_write(&mut self, addr: u32, data: &[u8]) -> Result<()>;
    /// Reads DPCD.
    fn dpcd_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()>;
    /// Waits.
    fn delay_us(&mut self, us: u32);
}

/// Clamps a requested level to what DP allows (pre-emphasis ≤ 3 − swing).
pub fn clamp(level: Level) -> Level {
    let v = level.0.min(3);
    (v, level.1.min(3 - v))
}

/// `TRAINING_LANEx_SET` byte of a level, with the "maximum reached" flags.
pub fn lane_set(level: Level) -> u8 {
    let (v, p) = clamp(level);
    let mut b = v | (p << 3);
    if v == 3 {
        b |= 1 << 2;
    }
    if v + p == 3 {
        b |= 1 << 5;
    }
    b
}

/// Index of a level in the 10-entry buffer translation tables (swing-major order).
pub fn level_index(level: Level) -> usize {
    let (v, p) = clamp(level);
    const BASE: [usize; 4] = [0, 4, 7, 9];
    BASE[v as usize] + p as usize
}

/// The highest level any lane asks for.
fn requested(st: &LinkStatus, lanes: u8) -> Level {
    let l = st.adjust[..lanes as usize]
        .iter()
        .fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
    clamp(l)
}

fn read_status<T: Trainer>(t: &mut T) -> Result<LinkStatus> {
    let mut b = [0u8; 6];
    t.dpcd_read(dpcd::LANE0_1_STATUS, &mut b)?;
    Ok(LinkStatus::parse(&b))
}

fn set_level<T: Trainer>(t: &mut T, level: Level, lanes: u8) -> Result<()> {
    t.source_level(level)?;
    let b = [lane_set(level); 4];
    t.dpcd_write(dpcd::TRAINING_LANE0_SET, &b[..lanes as usize])
}

/// Clock recovery; returns the level it ended with.
fn clock_recovery<T: Trainer>(t: &mut T, cfg: LinkConfig, caps: &Caps) -> Result<Level> {
    let mut level = (0, 0);
    t.source_pattern(TrainPattern::Tps1)?;
    t.source_level(level)?;
    let mut set = [dpcd::TPS1 | dpcd::SCRAMBLING_DISABLE, 0, 0, 0, 0];
    set[1..=cfg.lanes as usize].fill(lane_set(level));
    t.dpcd_write(dpcd::TRAINING_PATTERN_SET, &set[..1 + cfg.lanes as usize])?;
    let mut same = 0;
    for _ in 0..10 {
        t.delay_us(caps.aux_rd_interval_us.0);
        let st = read_status(t)?;
        if st.cr_ok(cfg.lanes) {
            return Ok(level);
        }
        let want = requested(&st, cfg.lanes);
        if want == level {
            same += 1;
            if same >= 5 {
                return Err(Error::LinkTraining(
                    "clock recovery: no progress at one level",
                ));
            }
        } else {
            if level.0 == 3 {
                return Err(Error::LinkTraining("clock recovery: maximum swing reached"));
            }
            same = 0;
            level = want;
            set_level(t, level, cfg.lanes)?;
        }
    }
    Err(Error::LinkTraining("clock recovery: too many tries"))
}

/// Channel equalisation from `level` on.
fn equalise<T: Trainer>(
    t: &mut T,
    cfg: LinkConfig,
    caps: &Caps,
    mut level: Level,
) -> Result<Level> {
    let (src, sink) = if caps.tps4 {
        (TrainPattern::Tps4, dpcd::TPS4)
    } else if caps.tps3 {
        (TrainPattern::Tps3, dpcd::TPS3 | dpcd::SCRAMBLING_DISABLE)
    } else {
        (TrainPattern::Tps2, dpcd::TPS2 | dpcd::SCRAMBLING_DISABLE)
    };
    t.source_pattern(src)?;
    t.dpcd_write(dpcd::TRAINING_PATTERN_SET, &[sink])?;
    for _ in 0..5 {
        t.delay_us(caps.aux_rd_interval_us.1);
        let st = read_status(t)?;
        if !st.cr_ok(cfg.lanes) {
            return Err(Error::LinkTraining(
                "channel equalisation: clock recovery lost",
            ));
        }
        if st.eq_ok(cfg.lanes) {
            return Ok(level);
        }
        let want = requested(&st, cfg.lanes);
        if want != level {
            level = want;
            set_level(t, level, cfg.lanes)?;
        }
    }
    Err(Error::LinkTraining("channel equalisation: too many tries"))
}

/// Configures the sink for `cfg` and trains the link. On success the sink's training pattern is
/// off and the source sends the idle pattern; the caller switches the source to normal data once
/// the transcoder runs. Returns the final drive level.
pub fn train<T: Trainer>(t: &mut T, cfg: LinkConfig, caps: &Caps) -> Result<Level> {
    let code = dpcd::code_from_rate(cfg.rate).ok_or(Error::Invalid("link rate"))?;
    if cfg.rate > caps.max_rate || cfg.lanes > caps.max_lanes || !matches!(cfg.lanes, 1 | 2 | 4) {
        return Err(Error::Invalid("link config beyond the sink"));
    }
    let lanes = cfg.lanes | if caps.enhanced_framing { 0x80 } else { 0 };
    t.dpcd_write(dpcd::LINK_BW_SET, &[code, lanes])?;
    t.dpcd_write(dpcd::DOWNSPREAD_CTRL, &[0, 1])?;
    let result = clock_recovery(t, cfg, caps).and_then(|l| equalise(t, cfg, caps, l));
    // always leave the sink out of training, also after a failure
    let off = t.dpcd_write(dpcd::TRAINING_PATTERN_SET, &[dpcd::TPS_OFF]);
    let level = result?;
    off?;
    t.source_pattern(TrainPattern::Idle)?;
    Ok(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dp::sink::Scripted;

    fn caps(tps3: bool, tps4: bool) -> Caps {
        Caps {
            rev: 0x12,
            max_rate: 540_000,
            max_lanes: 4,
            enhanced_framing: true,
            tps3,
            tps4,
            aux_rd_interval_us: (100, 400),
        }
    }

    const RBR4: LinkConfig = LinkConfig {
        rate: 162_000,
        lanes: 4,
    };

    #[test]
    fn levels() {
        assert_eq!(clamp((2, 3)), (2, 1));
        assert_eq!(clamp((5, 0)), (3, 0));
        assert_eq!(lane_set((0, 0)), 0);
        assert_eq!(lane_set((3, 0)), 3 | 4 | 0x20);
        assert_eq!(lane_set((1, 2)), 1 | (2 << 3) | 0x20);
        assert_eq!(lane_set((1, 1)), 1 | (1 << 3));
        let idx: alloc::vec::Vec<usize> = [(0, 0), (0, 3), (1, 0), (1, 2), (2, 0), (2, 1), (3, 0)]
            .iter()
            .map(|l| level_index(*l))
            .collect();
        assert_eq!(idx, [0, 3, 4, 6, 7, 8, 9]);
    }

    #[test]
    fn trains_at_once() {
        let mut s = Scripted::new(0, 0);
        let lvl = train(&mut s, RBR4, &caps(true, false)).unwrap();
        assert_eq!(lvl, (0, 0));
        assert_eq!(s.dpcd[0x100], 0x06);
        assert_eq!(s.dpcd[0x101], 0x84);
        assert_eq!(s.dpcd[0x102], 0); // training off at the end
        assert_eq!(
            s.patterns,
            [TrainPattern::Tps1, TrainPattern::Tps3, TrainPattern::Idle]
        );
    }

    #[test]
    fn follows_adjust_requests() {
        // sink wants swing 2 for clock recovery, then pre-emphasis 1 for equalisation
        let mut s = Scripted::new(2, 1);
        let lvl = train(&mut s, RBR4, &caps(false, false)).unwrap();
        assert_eq!(lvl, (2, 1));
        assert_eq!(s.levels.last(), Some(&(2, 1)));
        assert!(s.patterns.contains(&TrainPattern::Tps2));
        assert_eq!(s.dpcd[0x103], lane_set((2, 1)));
    }

    #[test]
    fn tps4_when_offered() {
        let mut s = Scripted::new(0, 0);
        train(&mut s, RBR4, &caps(true, true)).unwrap();
        assert!(s.patterns.contains(&TrainPattern::Tps4));
    }

    #[test]
    fn failures() {
        let mut never = Scripted::new(0, 0);
        never.cr_never = true;
        assert_eq!(
            train(&mut never, RBR4, &caps(true, false)),
            Err(Error::LinkTraining(
                "clock recovery: no progress at one level"
            ))
        );
        assert_eq!(never.dpcd[0x102], 0);
        let mut eq = Scripted::new(0, 0);
        eq.eq_never = true;
        assert_eq!(
            train(&mut eq, RBR4, &caps(true, false)),
            Err(Error::LinkTraining("channel equalisation: too many tries"))
        );
        let mut lost = Scripted::new(0, 0);
        lost.cr_lost_in_eq = true;
        assert_eq!(
            train(&mut lost, RBR4, &caps(true, false)),
            Err(Error::LinkTraining(
                "channel equalisation: clock recovery lost"
            ))
        );
        let mut wide = Scripted::new(0, 0);
        assert_eq!(
            train(
                &mut wide,
                LinkConfig {
                    rate: 810_000,
                    lanes: 4
                },
                &caps(true, false)
            ),
            Err(Error::Invalid("link config beyond the sink"))
        );
        assert_eq!(
            train(
                &mut wide,
                LinkConfig {
                    rate: 100,
                    lanes: 4
                },
                &caps(true, false)
            ),
            Err(Error::Invalid("link rate"))
        );
    }
}
