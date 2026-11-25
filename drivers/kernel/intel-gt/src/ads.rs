//! The ADS ("additional data struct"): the block of tables the GuC firmware reads while it starts.
//! Layout follows `struct guc_ads`, `guc_policies`, `guc_gt_system_info`, `guc_engine_usage` and
//! `guc_um_init_params` of the Linux `xe` driver (`xe_guc_fwif.h`, MIT); the minimal content is
//! what its `xe_guc_ads_populate_minimal` writes.
//!
//! ```text
//! | guc_ads | policies | system info | engine usage | um init | pad to 4K | private data |
//! ```
//! The **minimal** block (first load): scheduling policies, an engine mapping table that maps
//! nothing, the doorbell count and a private work area. It lets the firmware become ready and
//! answer queries.
//!
//! The **full** block (second load) adds the engines: which exist, how the GuC's numbering maps
//! to the hardware's, per engine the registers the GuC saves and restores around an engine
//! reset, per class where the golden context image lies and how large it is, and an empty error
//! capture list.
//!
//! ```text
//! | fixed part | pad | register lists 4K | golden contexts | capture list 4K | private data |
//! ```

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

/// Checks the address and makes a zeroed block of `size` bytes with what every block has:
/// policies, a mapping table without engines, the doorbell count and the three addresses.
fn base(
    ads_ggtt: u32,
    size: usize,
    private_offset: usize,
    dist_dbs_populated: u32,
) -> Result<Vec<u8>> {
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
        ads_ggtt + private_offset as u32,
    );
    Ok(blob)
}

/// Builds the minimal block for the GGTT address `ads_ggtt` it will be mapped at.
/// `dist_dbs_populated` is the value of the register of that name (doorbells per SQIDI).
pub fn minimal(ads_ggtt: u32, private_data_size: u32, dist_dbs_populated: u32) -> Result<Vec<u8>> {
    let size = minimal_size(private_data_size);
    base(ads_ggtt, size, private_data_offset(), dist_dbs_populated)
}

/// The GuC's engine classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Render engine.
    Render = 0,
    /// Video decode.
    Video = 1,
    /// Video enhancement.
    VideoEnhance = 2,
    /// Copy engine.
    Blitter = 3,
    /// Compute engine.
    Compute = 4,
}

impl Class {
    /// Size of a context image of this class on Xe_LPG: one status page and the engine's state
    /// (13 pages with the render state for render and compute, one page otherwise).
    pub const fn lrc_size(self) -> u32 {
        match self {
            Class::Render | Class::Compute => 14 * 4096,
            _ => 2 * 4096,
        }
    }
}

/// An engine as the GuC needs to know it (one instance per class on Xe_LPG's primary GT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Engine {
    /// The GuC's class.
    pub class: Class,
    /// Instance within the class (hardware and logical numbering agree here).
    pub instance: u8,
    /// Where the engine's registers start.
    pub mmio_base: u32,
}

/// The engines of Xe_LPG's primary GT: render, copy, compute.
pub const XE_LPG_ENGINES: [Engine; 3] = [
    Engine {
        class: Class::Render,
        instance: 0,
        mmio_base: 0x2000,
    },
    Engine {
        class: Class::Blitter,
        instance: 0,
        mmio_base: 0x2_2000,
    },
    Engine {
        class: Class::Compute,
        instance: 0,
        mmio_base: 0x1_a000,
    },
];

/// Registers of the context image that are not part of the engine state (96 words).
const LRC_REG_SIZE: u32 = 96 * 4;
const ADS_GOLDEN_CONTEXT_LRCA: usize = REG_STATE_LIST_SIZE + 20;
const ADS_ENG_STATE_SIZE: usize = ADS_GOLDEN_CONTEXT_LRCA + MAX_ENGINE_CLASSES * 4;
const ADS_CAPTURE: usize = ADS_PRIVATE_DATA + 8;
const CAPTURE_WORDS: usize = 2 * 16 + 2 * 16 + 2;
const ENGINE_ENABLED_MASKS: usize = SYSTEM_INFO + MAPPING_TABLE_SIZE;
const REGSET_SIZE: usize = 4096;
/// What the GuC restores after an engine reset, per engine (offset, flags); see [`regset`].
const REGSET_PER_ENGINE: [(u32, u32); 4] =
    [(0x98, 0), (0x29c, REGSET_MASKED), (0x80, 0), (0xa8, 0)];
/// The register is written with a mask in its upper half.
const REGSET_MASKED: u32 = 1 << 0;
/// The switch of the compute engines: lost with a reset of the render engine's domain.
const RCU_MODE: u32 = 0x1_4800;

/// The registers the GuC saves before it resets engine `e` and writes back afterwards
/// (address, flags): interrupt handling, context mode (without it the engine would come back
/// in the old ring mode and run nothing), status page, interrupt mask – and with the render
/// engine the switch of the compute engines.
fn regset(e: &Engine) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = REGSET_PER_ENGINE
        .iter()
        .map(|(off, flags)| (e.mmio_base + off, *flags))
        .collect();
    if e.class == Class::Render {
        list.push((RCU_MODE, REGSET_MASKED));
    }
    list
}

fn regset_offset() -> usize {
    page_align(STATIC_SIZE)
}

fn golden_size(engines: &[Engine]) -> usize {
    engines
        .iter()
        .map(|e| page_align(e.class.lrc_size() as usize))
        .sum()
}

/// Size of the full block for these engines and a firmware that wants `private_data_size`
/// bytes of work area.
pub fn full_size(private_data_size: u32, engines: &[Engine]) -> usize {
    regset_offset()
        + REGSET_SIZE
        + golden_size(engines)
        + 4096
        + page_align(private_data_size as usize)
}

/// Where the golden context image of `class` lies in the full block built for `engines`: byte
/// offset and size. `None` when no engine of that class is in the list. The image a context of
/// the class saved after its first run is copied there while the GuC runs (Linux does the same
/// after the load).
pub fn golden_slot(engines: &[Engine], class: Class) -> Option<(usize, usize)> {
    let mut at = regset_offset() + REGSET_SIZE;
    let mut seen: Vec<Class> = Vec::new();
    for e in engines {
        if seen.contains(&e.class) {
            continue;
        }
        let size = e.class.lrc_size() as usize;
        if e.class == class {
            return Some((at, size));
        }
        seen.push(e.class);
        at += page_align(size);
    }
    None
}

/// Builds the full block. The golden context images inside it start as zeroes; they are filled
/// once a context of each class has run (stage G3).
pub fn full(
    ads_ggtt: u32,
    private_data_size: u32,
    dist_dbs_populated: u32,
    engines: &[Engine],
) -> Result<Vec<u8>> {
    if engines.len() * (REGSET_PER_ENGINE.len() + 1) * 16 > REGSET_SIZE {
        return Err(Error::Invalid(
            "too many engines for the register list page",
        ));
    }
    let golden = regset_offset() + REGSET_SIZE;
    let capture = golden + golden_size(engines);
    let private = capture + 4096;
    let size = full_size(private_data_size, engines);
    let mut blob = base(ads_ggtt, size, private, dist_dbs_populated)?;
    let (mut regs, mut lrc) = (regset_offset(), golden);
    for e in engines {
        let (class, inst) = (e.class as usize, e.instance as usize);
        if inst >= MAX_INSTANCES_PER_CLASS {
            return Err(Error::Invalid("engine instance"));
        }
        let mask_at = ENGINE_ENABLED_MASKS + class * 4;
        let mask = word(&blob, mask_at);
        let first_of_class = mask == 0;
        put(&mut blob, mask_at, mask | 1 << inst);
        blob[SYSTEM_INFO + class * MAX_INSTANCES_PER_CLASS + inst] = e.instance;
        // register list of this engine: address, then the 16-bit count
        let list = (class * MAX_INSTANCES_PER_CLASS + inst) * 8;
        put(&mut blob, list, ads_ggtt + regs as u32);
        let saved = regset(e);
        put(&mut blob, list + 4, saved.len() as u32);
        for (addr, flags) in saved {
            put(&mut blob, regs, addr);
            put(&mut blob, regs + 8, flags);
            regs += 16;
        }
        if first_of_class {
            let lrc_size = e.class.lrc_size();
            put(
                &mut blob,
                ADS_ENG_STATE_SIZE + class * 4,
                lrc_size - 4096 - LRC_REG_SIZE,
            );
            put(
                &mut blob,
                ADS_GOLDEN_CONTEXT_LRCA + class * 4,
                ads_ggtt + lrc as u32,
            );
            lrc += page_align(lrc_size as usize);
        }
    }
    // every error capture list is the empty one (a header that announces no registers)
    for i in 0..CAPTURE_WORDS {
        put(&mut blob, ADS_CAPTURE + i * 4, ads_ggtt + capture as u32);
    }
    Ok(blob)
}

fn word(blob: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]])
}

#[cfg(test)]
mod tests;
