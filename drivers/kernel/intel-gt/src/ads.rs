//! The ADS ("additional data struct"): the block of tables the GuC firmware reads while it starts.
//! Layout follows `struct guc_ads`, `guc_policies`, `guc_gt_system_info`, `guc_engine_usage` and
//! `guc_um_init_params` of the Linux `xe` driver (`xe_guc_fwif.h`, MIT); the minimal content is
//! what its `xe_guc_ads_populate_minimal` writes.
//!
//! ```text
//! | guc_ads | policies | system info | engine usage | um init | pad to 4K | private data |
//! ```
//! Stage G1b uses the **minimal** block: scheduling policies, an engine mapping table that maps
//! nothing, the doorbell count and a private work area. It lets the firmware become ready and
//! answer queries; engines, register lists and golden contexts come with the full block (G3).

use crate::{Error, Result};
use alloc::vec;
use alloc::vec::Vec;

/// Engine classes the firmware's tables have room for.
pub const MAX_ENGINE_CLASSES: usize = 16;
/// Instances per engine class the firmware's tables have room for.
pub const MAX_INSTANCES_PER_CLASS: usize = 32;

// struct guc_ads
const REG_STATE_LIST_SIZE: usize = MAX_ENGINE_CLASSES * MAX_INSTANCES_PER_CLASS * 8;
const ADS_SCHEDULER_POLICIES: usize = REG_STATE_LIST_SIZE + 4;
const ADS_GT_SYSTEM_INFO: usize = REG_STATE_LIST_SIZE + 8;
const ADS_PRIVATE_DATA: usize = REG_STATE_LIST_SIZE + 20 + 2 * MAX_ENGINE_CLASSES * 4;
/// Size of `struct guc_ads`: register lists, 5 words, golden context and state size tables,
/// 2 words, capture tables (2 × 16, 2 × 16, 2), 3 workaround words, 11 reserved words.
const ADS_SIZE: usize = ADS_PRIVATE_DATA + 8 + (2 * 16 + 2 * 16 + 2) * 4 + 3 * 4 + 11 * 4;

// struct guc_policies
const POLICIES: usize = ADS_SIZE;
const POLICIES_DPC_PROMOTE_TIME: usize = POLICIES + MAX_ENGINE_CLASSES * 4;
const POLICIES_IS_VALID: usize = POLICIES_DPC_PROMOTE_TIME + 4;
const POLICIES_MAX_NUM_WORK_ITEMS: usize = POLICIES_DPC_PROMOTE_TIME + 8;
const POLICIES_SIZE: usize = MAX_ENGINE_CLASSES * 4 + 4 * 4 + 4 * 4;

// struct guc_gt_system_info
const SYSTEM_INFO: usize = POLICIES + POLICIES_SIZE;
const MAPPING_TABLE_SIZE: usize = MAX_ENGINE_CLASSES * MAX_INSTANCES_PER_CLASS;
const GENERIC_SYSINFO: usize = SYSTEM_INFO + MAPPING_TABLE_SIZE + MAX_ENGINE_CLASSES * 4;
const SYSINFO_DOORBELL_COUNT_PER_SQIDI: usize = 2;
const SYSTEM_INFO_SIZE: usize = MAPPING_TABLE_SIZE + MAX_ENGINE_CLASSES * 4 + 16 * 4;

// struct guc_engine_usage (32 bytes per engine) and guc_um_init_params (3 queues)
const ENGINE_USAGE_SIZE: usize = MAX_ENGINE_CLASSES * MAX_INSTANCES_PER_CLASS * 32;
const UM_INIT_PARAMS_SIZE: usize = 8 + 6 * 4 + 3 * 32;

/// Size of the fixed part of the block.
pub const STATIC_SIZE: usize =
    SYSTEM_INFO + SYSTEM_INFO_SIZE + ENGINE_USAGE_SIZE + UM_INIT_PARAMS_SIZE;

/// Time after which queued work is promoted (µs).
const DPC_PROMOTE_TIME_US: u32 = 500_000;
/// Work items handled per call.
const MAX_NUM_WORK_ITEMS: u32 = 15;

const fn page_align(n: usize) -> usize {
    (n + 4095) & !4095
}

/// Where the firmware's private work area starts in the minimal block.
pub const fn private_data_offset() -> usize {
    page_align(STATIC_SIZE)
}

/// Size of the minimal block for a firmware that wants `private_data_size` bytes of work area
/// (from the firmware's CSS header).
pub fn minimal_size(private_data_size: u32) -> usize {
    private_data_offset() + page_align(private_data_size as usize)
}

fn put(blob: &mut [u8], off: usize, val: u32) {
    blob[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Builds the minimal block for the GGTT address `ads_ggtt` it will be mapped at.
/// `dist_dbs_populated` is the value of the register of that name (doorbells per SQIDI).
pub fn minimal(ads_ggtt: u32, private_data_size: u32, dist_dbs_populated: u32) -> Result<Vec<u8>> {
    let size = minimal_size(private_data_size);
    if !ads_ggtt.is_multiple_of(4096) {
        return Err(Error::Invalid("ADS not page aligned"));
    }
    let end = u32::try_from(size)
        .ok()
        .and_then(|s| ads_ggtt.checked_add(s));
    if end.is_none() {
        return Err(Error::Invalid("ADS does not fit below 4 GiB"));
    }
    let mut blob = vec![0u8; size];
    put(&mut blob, POLICIES_DPC_PROMOTE_TIME, DPC_PROMOTE_TIME_US);
    put(&mut blob, POLICIES_MAX_NUM_WORK_ITEMS, MAX_NUM_WORK_ITEMS);
    put(&mut blob, POLICIES_IS_VALID, 1);
    // no engine is mapped yet: every entry holds the "invalid instance" value
    blob[SYSTEM_INFO..SYSTEM_INFO + MAPPING_TABLE_SIZE].fill(MAX_INSTANCES_PER_CLASS as u8);
    let doorbells = ((dist_dbs_populated >> 16) & 0xff) + 1;
    put(
        &mut blob,
        GENERIC_SYSINFO + SYSINFO_DOORBELL_COUNT_PER_SQIDI * 4,
        doorbells,
    );
    put(
        &mut blob,
        ADS_SCHEDULER_POLICIES,
        ads_ggtt + POLICIES as u32,
    );
    put(&mut blob, ADS_GT_SYSTEM_INFO, ads_ggtt + SYSTEM_INFO as u32);
    put(
        &mut blob,
        ADS_PRIVATE_DATA,
        ads_ggtt + private_data_offset() as u32,
    );
    Ok(blob)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(blob: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]])
    }

    #[test]
    fn layout_matches_the_firmware_interface() {
        // sizes of the structures as the xe driver's headers define them
        assert_eq!(ADS_SIZE, 4572);
        assert_eq!(POLICIES_SIZE, 96);
        assert_eq!(SYSTEM_INFO_SIZE, 640);
        assert_eq!(ENGINE_USAGE_SIZE, 16384);
        assert_eq!(UM_INIT_PARAMS_SIZE, 128);
        assert_eq!(STATIC_SIZE, 21820);
        assert_eq!(
            (ADS_SCHEDULER_POLICIES, ADS_GT_SYSTEM_INFO, ADS_PRIVATE_DATA),
            (4100, 4104, 4244)
        );
        assert_eq!(private_data_offset(), 24576);
        assert_eq!(minimal_size(0x4_0000), 24576 + 0x4_0000);
        assert_eq!(minimal_size(1), 24576 + 4096);
    }

    #[test]
    fn minimal_block_content() {
        let base = 0x0120_0000;
        let b = minimal(base, 0x2000, 0x000f_0001).unwrap();
        assert_eq!(b.len(), 24576 + 0x2000);
        assert_eq!(get(&b, 4100), base + 4572);
        assert_eq!(get(&b, 4104), base + 4668);
        assert_eq!(get(&b, 4244), base + 24576);
        assert_eq!(get(&b, 4572 + 64), 500_000);
        assert_eq!(get(&b, 4572 + 68), 1);
        assert_eq!(get(&b, 4572 + 72), 15);
        assert!(b[4668..4668 + 512].iter().all(|v| *v == 32));
        // no engine enabled, 16 doorbells per SQIDI
        assert!(b[4668 + 512..4668 + 576].iter().all(|v| *v == 0));
        assert_eq!(get(&b, 4668 + 576 + 8), 16);
        // everything else stays zero: register lists, golden contexts, capture lists
        assert!(b[..4100].iter().all(|v| *v == 0));
        assert!(b[4108..4244].iter().all(|v| *v == 0));
        assert!(b[24576..].iter().all(|v| *v == 0));
    }

    #[test]
    fn refuses_bad_addresses() {
        assert!(minimal(0x1234, 0, 0).is_err());
        assert!(minimal(0xffff_f000, 0x2000, 0).is_err());
    }
}
