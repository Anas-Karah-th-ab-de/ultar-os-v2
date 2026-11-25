//! Boot parameters of the GuC (`SOFT_SCRATCH(1..=14)`): where its log and its ADS block are, which
//! device it runs on and which features and workarounds it shall use. Values follow `xe_guc.c`
//! and `xe_guc_fwif.h` of the Linux `xe` driver (MIT).

use crate::fw::Version;
use crate::guc::{LOG_CRASH_DUMP_SIZE, LOG_EVENT_DATA_SIZE, LOG_STATE_CAPTURE_SIZE};
use crate::{genmask, prep, Error, Result};

/// Number of boot parameter words.
pub const PARAM_WORDS: usize = 14;

/// Which load the parameters are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// First load with the minimal ADS: no features, no workarounds. Enough for the GuC to
    /// become ready and answer the hardware configuration query.
    Minimal,
    /// Load for submission: power management by the GuC, lite restore, workarounds.
    Full,
}

/// What the driver tells the firmware at boot (`SOFT_SCRATCH(1..=14)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// GGTT address of the log buffer ([`LOG_BUFFER_SIZE`] bytes, page aligned).
    pub log_ggtt: u32,
    /// GGTT address of the ADS block (page aligned).
    pub ads_ggtt: u32,
    /// PCI device id.
    pub devid: u16,
    /// PCI revision.
    pub revid: u8,
    /// Release of the firmware being loaded.
    pub release: Version,
}

const CTL_LOG_PARAMS: usize = 0;
const CTL_WA: usize = 1;
const CTL_FEATURE: usize = 2;
const CTL_DEBUG: usize = 3;
const CTL_ADS: usize = 4;
const CTL_DEVID: usize = 5;

const LOG_VALID: u32 = 1 << 0;
const LOG_NOTIFY_ON_HALF_FULL: u32 = 1 << 1;
const LOG_CAPTURE_ALLOC_UNITS: u32 = 1 << 2;
const LOG_CRASH_DUMP: u32 = genmask(5, 4);
const LOG_EVENT_DATA: u32 = genmask(9, 6);
const LOG_STATE_CAPTURE: u32 = genmask(11, 10);
const LOG_BUF_ADDR: u32 = genmask(31, 12);
const WA_DUAL_QUEUE: u32 = 1 << 11;
const WA_ENABLE_TSC_CHECK_ON_RC6: u32 = 1 << 22;
const CTL_ENABLE_LITE_RESTORE: u32 = 1 << 4;
const CTL_DISABLE_MULTI_QUEUE: u32 = 1 << 24;
const DEBUG_LOG_DISABLED: u32 = 1 << 6;
const ADS_ADDR: u32 = genmask(21, 1);

impl Params {
    /// The parameter words for graphics 12.70–12.74 (Xe_LPG) as Linux sets them: log sections in
    /// 4 KiB units (capture in 1 MiB units), dual-queue and RC6 time-stamp workarounds, lite
    /// restore, verbose logging off. Unlike Linux without power management by the GuC (SLPC).
    pub fn words(&self, stage: Stage) -> Result<[u32; PARAM_WORDS]> {
        if !self.log_ggtt.is_multiple_of(4096) || !self.ads_ggtt.is_multiple_of(4096) {
            return Err(Error::Invalid("GuC buffer not page aligned"));
        }
        let mut p = [0u32; PARAM_WORDS];
        p[CTL_LOG_PARAMS] = LOG_VALID
            | LOG_NOTIFY_ON_HALF_FULL
            | LOG_CAPTURE_ALLOC_UNITS
            | prep(LOG_CRASH_DUMP, LOG_CRASH_DUMP_SIZE / 4096 - 1)
            | prep(LOG_EVENT_DATA, LOG_EVENT_DATA_SIZE / 4096 - 1)
            | prep(LOG_STATE_CAPTURE, LOG_STATE_CAPTURE_SIZE / (1 << 20) - 1)
            | prep(LOG_BUF_ADDR, self.log_ggtt >> 12);
        if stage == Stage::Full {
            p[CTL_WA] = WA_DUAL_QUEUE | WA_ENABLE_TSC_CHECK_ON_RC6;
            // power management stays with the kernel: the GuC's own (SLPC) only runs after a
            // start request with its shared data, which the kernel does not send yet; enabled but
            // not started it held the GT at 1800 of 2350 MHz under load
            p[CTL_FEATURE] = CTL_ENABLE_LITE_RESTORE;
            let multi_queue = Version {
                major: 70,
                minor: 66,
                patch: 0,
            };
            if self.release < multi_queue {
                p[CTL_FEATURE] |= CTL_DISABLE_MULTI_QUEUE;
            }
        }
        p[CTL_DEBUG] = DEBUG_LOG_DISABLED;
        p[CTL_ADS] = prep(ADS_ADDR, self.ads_ggtt >> 12);
        p[CTL_DEVID] = (self.devid as u32) << 16 | self.revid as u32;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            log_ggtt: 0x0100_0000,
            ads_ggtt: 0x0120_0000,
            devid: 0x7d51,
            revid: 3,
            release: Version {
                major: 70,
                minor: 53,
                patch: 0,
            },
        }
    }

    #[test]
    fn full_parameter_words() {
        let w = params().words(Stage::Full).unwrap();
        // valid | notify | capture units; crash 16K -> 3, events 64K -> 15, capture 1M -> 0
        assert_eq!(w[0], 0x0100_0000 | 0b0111 | 3 << 4 | 15 << 6);
        assert_eq!(w[1], 1 << 11 | 1 << 22);
        assert_eq!(w[2], 1 << 4 | 1 << 24, "lite restore, single queue; no SLPC");
        assert_eq!(w[3], 1 << 6);
        assert_eq!(w[4], 0x1200 << 1);
        assert_eq!(w[5], 0x7d51_0003);
        assert!(w[6..].iter().all(|v| *v == 0));
        let newer = Params {
            release: Version {
                major: 70,
                minor: 66,
                patch: 0,
            },
            ..params()
        };
        assert_eq!(newer.words(Stage::Full).unwrap()[2], 1 << 4 | 1 << 2);
    }

    #[test]
    fn minimal_load_has_no_features_and_workarounds() {
        let full = params().words(Stage::Full).unwrap();
        let min = params().words(Stage::Minimal).unwrap();
        assert_eq!((min[1], min[2]), (0, 0));
        assert_eq!(
            (min[0], min[3], min[4], min[5]),
            (full[0], full[3], full[4], full[5])
        );
    }

    #[test]
    fn buffers_must_be_page_aligned() {
        assert!(Params {
            log_ggtt: 0x100,
            ..params()
        }
        .words(Stage::Minimal)
        .is_err());
        assert!(Params {
            ads_ggtt: 0x1234,
            ..params()
        }
        .words(Stage::Full)
        .is_err());
    }
}
