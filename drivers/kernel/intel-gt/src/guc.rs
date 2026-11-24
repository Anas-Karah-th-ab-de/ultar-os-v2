//! The GuC: the micro-controller of the GT that schedules work on the engines. On Meteor Lake
//! and later nothing renders or computes without it. This module resets it, hands over boot
//! parameters, the RSA signature and the firmware image, and follows the load status.
//! Follows `xe_guc.c`, `xe_uc_fw.c` and `abi/guc_errors_abi.h` of the Linux `xe` driver (MIT).
//!
//! Not here yet (stage G1b, `docs/integration/intel-gt.md`): the ADS block the firmware reads
//! while it initialises and the command buffers (CTB). Without a valid ADS a GuC that passed the
//! signature check stops with "init data invalid"; [`Params::ads_ggtt`] takes the ADS address.

use crate::fw::GucImage;
pub use crate::params::{Params, Stage, PARAM_WORDS};
use crate::regs::*;
use crate::{field, Error, Hw, Result};
use core::fmt;

/// Where the boot ROM expects the image inside the GuC's protected memory (below it: stack).
pub const WOPCM_LOAD_OFFSET: u32 = 0x2000;
/// GGTT addresses from here on do not reach the GuC through the GGTT.
pub const GUC_GGTT_TOP: u64 = 0xfee0_0000;
/// How long the DMA engine may take for the image (100 ms, as Linux).
pub const DMA_TIMEOUT_US: u32 = 100_000;
/// How long the firmware may take to become ready (3 s, as Linux).
pub const LOAD_TIMEOUT_US: u32 = 3_000_000;

/// Size of the log's crash dump section.
pub const LOG_CRASH_DUMP_SIZE: u32 = 16 << 10;
/// Size of the log's event section.
pub const LOG_EVENT_DATA_SIZE: u32 = 64 << 10;
/// Size of the log's state capture section.
pub const LOG_STATE_CAPTURE_SIZE: u32 = 1 << 20;
/// Size of the whole log buffer (a 4 KiB header page and the three sections).
pub const LOG_BUFFER_SIZE: u32 =
    4096 + LOG_EVENT_DATA_SIZE + LOG_CRASH_DUMP_SIZE + LOG_STATE_CAPTURE_SIZE;

const BOOTROM_NO_KEY_FOUND: u32 = 0x13;
const BOOTROM_AES_PROD_KEY_FOUND: u32 = 0x1a;
const BOOTROM_PROD_KEY_CHECK_FAILURE: u32 = 0x2b;
const BOOTROM_RSA_FAILED: u32 = 0x50;
const BOOTROM_PAVPC_FAILED: u32 = 0x73;
const BOOTROM_WOPCM_FAILED: u32 = 0x74;
const BOOTROM_LOADLOC_FAILED: u32 = 0x75;
/// Boot ROM value: signature accepted, jumped into the firmware.
pub const BOOTROM_JUMP_PASSED: u32 = 0x76;
const BOOTROM_JUMP_FAILED: u32 = 0x77;
const BOOTROM_RC6CTXCONFIG_FAILED: u32 = 0x79;
const BOOTROM_MPUMAP_INCORRECT: u32 = 0x7a;
const BOOTROM_EXCEPTION: u32 = 0x7b;

const UK_ERROR_DEVID_BUILD_MISMATCH: u32 = 0x02;
const UK_GUC_PREPROD_BUILD_MISMATCH: u32 = 0x03;
const UK_ERROR_DEVID_INVALID_GUCTYPE: u32 = 0x04;
const UK_HWCONFIG_ERROR: u32 = 0x07;
const UK_BOOTROM_VERSION_MISMATCH: u32 = 0x08;
const UK_DPC_ERROR: u32 = 0x60;
const UK_EXCEPTION: u32 = 0x70;
/// Firmware start value: the ADS (init data) was refused.
pub const UK_INIT_DATA_INVALID: u32 = 0x71;
const UK_MPU_DATA_INVALID: u32 = 0x73;
const UK_INIT_MMIO_SAVE_RESTORE_INVALID: u32 = 0x74;
const UK_KLV_WORKAROUND_INIT_ERROR: u32 = 0x75;
const UK_INVALID_FTR_FLAG: u32 = 0x76;
/// Firmware start value: running and ready for messages.
pub const UK_READY: u32 = 0xf0;

/// Where a load stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// The firmware runs.
    Ready,
    /// The boot ROM or the firmware reported a failure.
    Failed,
    /// Still loading.
    Running,
}

/// A decoded `GUC_STATUS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// The register value.
    pub raw: u32,
    /// The processor core is held in reset.
    pub in_reset: bool,
    /// Progress of the boot ROM.
    pub bootrom: u32,
    /// Progress of the firmware's start code.
    pub ukernel: u32,
    /// Signature check: 0 not done, 1 bad, 2 good.
    pub auth: u32,
}

impl Status {
    /// Decodes a register value.
    pub fn decode(raw: u32) -> Status {
        Status {
            raw,
            in_reset: raw & GS_MIA_IN_RESET != 0,
            bootrom: field(raw, GS_BOOTROM),
            ukernel: field(raw, GS_UKERNEL),
            auth: field(raw, GS_AUTH_STATUS),
        }
    }

    /// Reads and decodes the register. Needs the `Gt` domain awake.
    pub fn read(hw: &mut impl Hw) -> Status {
        Status::decode(hw.read(GUC_STATUS))
    }

    /// Where the load stands (the rule of Linux' `guc_load_done`).
    pub fn load(&self) -> Load {
        if self.raw == u32::MAX {
            return Load::Failed;
        }
        match self.ukernel {
            UK_READY => return Load::Ready,
            UK_ERROR_DEVID_BUILD_MISMATCH
            | UK_GUC_PREPROD_BUILD_MISMATCH
            | UK_ERROR_DEVID_INVALID_GUCTYPE
            | UK_HWCONFIG_ERROR
            | UK_BOOTROM_VERSION_MISMATCH
            | UK_DPC_ERROR
            | UK_EXCEPTION
            | UK_INIT_DATA_INVALID
            | UK_MPU_DATA_INVALID
            | UK_INIT_MMIO_SAVE_RESTORE_INVALID
            | UK_KLV_WORKAROUND_INIT_ERROR
            | UK_INVALID_FTR_FLAG => return Load::Failed,
            _ => {}
        }
        match self.bootrom {
            BOOTROM_NO_KEY_FOUND
            | BOOTROM_RSA_FAILED
            | BOOTROM_PAVPC_FAILED
            | BOOTROM_WOPCM_FAILED
            | BOOTROM_LOADLOC_FAILED
            | BOOTROM_JUMP_FAILED
            | BOOTROM_RC6CTXCONFIG_FAILED
            | BOOTROM_MPUMAP_INCORRECT
            | BOOTROM_EXCEPTION
            | BOOTROM_PROD_KEY_CHECK_FAILURE => Load::Failed,
            _ => Load::Running,
        }
    }

    /// The boot ROM accepted the signature and started the firmware.
    pub fn authenticated(&self) -> bool {
        self.raw != u32::MAX && (self.auth == 2 || self.bootrom == BOOTROM_JUMP_PASSED)
    }

    fn bootrom_text(&self) -> &'static str {
        match self.bootrom {
            0 => "idle",
            BOOTROM_NO_KEY_FOUND => "no key found",
            BOOTROM_AES_PROD_KEY_FOUND => "production key found",
            BOOTROM_PROD_KEY_CHECK_FAILURE => "production key check failed",
            BOOTROM_RSA_FAILED => "RSA check failed",
            BOOTROM_PAVPC_FAILED => "PAVPC failed",
            BOOTROM_WOPCM_FAILED => "WOPCM failed",
            BOOTROM_LOADLOC_FAILED => "load location failed",
            BOOTROM_JUMP_PASSED => "jump passed",
            BOOTROM_JUMP_FAILED => "jump failed",
            BOOTROM_RC6CTXCONFIG_FAILED => "RC6 context config failed",
            BOOTROM_MPUMAP_INCORRECT => "MPU map incorrect",
            BOOTROM_EXCEPTION => "exception",
            _ => "in progress",
        }
    }

    fn ukernel_text(&self) -> &'static str {
        match self.ukernel {
            0 => "not started",
            UK_READY => "ready",
            UK_ERROR_DEVID_BUILD_MISMATCH => "device id / build mismatch",
            UK_GUC_PREPROD_BUILD_MISMATCH => "pre-production build mismatch",
            UK_ERROR_DEVID_INVALID_GUCTYPE => "device id: wrong GuC type",
            UK_HWCONFIG_ERROR => "hardware config error",
            UK_BOOTROM_VERSION_MISMATCH => "boot ROM version mismatch",
            UK_DPC_ERROR => "DPC error",
            UK_EXCEPTION => "exception",
            UK_INIT_DATA_INVALID => "init data invalid",
            UK_MPU_DATA_INVALID => "MPU data invalid",
            UK_INIT_MMIO_SAVE_RESTORE_INVALID => "MMIO save/restore list invalid",
            UK_KLV_WORKAROUND_INIT_ERROR => "workaround KLV error",
            UK_INVALID_FTR_FLAG => "invalid feature flag",
            _ => "starting",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.raw == u32::MAX {
            return f.write_str("registers read all ones");
        }
        write!(
            f,
            "boot ROM {:#04x} {}, firmware {:#04x} {}, signature {}{}",
            self.bootrom,
            self.bootrom_text(),
            self.ukernel,
            self.ukernel_text(),
            match self.auth {
                0 => "not checked",
                1 => "bad",
                2 => "good",
                _ => "?",
            },
            if self.in_reset { ", core in reset" } else { "" }
        )
    }
}

/// Puts the GuC into reset. Needs the `Gt` domain awake.
pub fn reset(hw: &mut impl Hw) -> Result<()> {
    hw.write(GDRST, GRDOM_GUC);
    hw.wait_clear(GDRST, GRDOM_GUC, 5000)?;
    let status = hw.read(GUC_STATUS);
    if status == u32::MAX {
        return Err(Error::DeviceGone);
    }
    if status & GS_MIA_IN_RESET == 0 {
        return Err(Error::GucReset { status });
    }
    Ok(())
}

/// Writes the boot parameters (word 0 of the scratch registers is cleared, as Linux).
pub fn write_params(hw: &mut impl Hw, params: &[u32; PARAM_WORDS]) {
    hw.write(soft_scratch(0), 0);
    for (i, p) in params.iter().enumerate() {
        hw.write(soft_scratch(1 + i as u32), *p);
    }
}

/// Programs what must be set before the DMA transfer: shim caches, doorbells, ARAT interrupts.
pub fn prepare_xfer(hw: &mut impl Hw) {
    hw.write(
        GUC_SHIM_CONTROL,
        GUC_ENABLE_READ_CACHE_LOGIC
            | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA
            | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA
            | GUC_ENABLE_MIA_CLOCK_GATING,
    );
    hw.write(GT_PM_CONFIG, GT_DOORBELL_ENABLE);
    hw.rmw(PMINTRMSK, ARAT_EXPIRED_INTRMSK, 0);
}

/// Checks that `len` bytes at GGTT address `ggtt` are reachable for the GuC: above the protected
/// memory (lower addresses are shadowed by it) and below [`GUC_GGTT_TOP`].
pub fn check_ggtt(ggtt: u64, len: u64, wopcm_size: u32) -> Result<()> {
    if ggtt < wopcm_size as u64 || ggtt.checked_add(len).is_none_or(|end| end > GUC_GGTT_TOP) {
        return Err(Error::Invalid("GGTT address outside the GuC's range"));
    }
    Ok(())
}

/// Hands over the RSA signature: keys of more than 256 bytes (Meteor Lake and later) by the GGTT
/// address of the key inside the mapped firmware file, smaller ones through 64 registers.
pub fn xfer_rsa(hw: &mut impl Hw, img: &GucImage, file: &[u8], fw_ggtt: u64) -> Result<()> {
    if img.rsa_size > 256 {
        let addr = fw_ggtt + img.rsa_offset() as u64;
        let addr = u32::try_from(addr).map_err(|_| Error::Invalid("RSA key above 4 GiB"))?;
        hw.write(uos_rsa_scratch(0), addr);
        return Ok(());
    }
    for (i, w) in img.rsa_words(file)?.iter().enumerate() {
        hw.write(uos_rsa_scratch(i as u32), *w);
    }
    Ok(())
}

/// Copies header and code from the GGTT into the protected memory with the GuC's DMA engine.
pub fn xfer(hw: &mut impl Hw, img: &GucImage, fw_ggtt: u64) -> Result<()> {
    hw.write(DMA_ADDR_0_LOW, fw_ggtt as u32);
    hw.write(
        DMA_ADDR_0_HIGH,
        (fw_ggtt >> 32) as u32 | DMA_ADDRESS_SPACE_GGTT,
    );
    hw.write(DMA_ADDR_1_LOW, WOPCM_LOAD_OFFSET);
    hw.write(DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM);
    hw.write(DMA_COPY_SIZE, img.upload_size());
    hw.write(DMA_CTRL, masked_enable(UOS_MOVE | START_DMA));
    let done = hw.wait_clear(DMA_CTRL, START_DMA, DMA_TIMEOUT_US);
    hw.write(DMA_CTRL, masked_disable(UOS_MOVE));
    match done {
        Ok(u32::MAX) => Err(Error::DeviceGone),
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Waits until the firmware is ready (`Ok`), or fails with the last status.
pub fn wait_ready(hw: &mut impl Hw, timeout_us: u32) -> Result<Status> {
    let start = hw.now_us();
    loop {
        let st = Status::read(hw);
        match st.load() {
            Load::Ready => return Ok(st),
            Load::Failed => return Err(Error::GucLoad { status: st.raw }),
            Load::Running => {}
        }
        if hw.now_us().saturating_sub(start) >= timeout_us as u64 {
            return Err(Error::GucLoad { status: st.raw });
        }
        hw.delay_us(10_000);
    }
}

/// The whole upload after [`reset`] and [`crate::wopcm::init`]: parameters, shim, RSA, DMA,
/// wait. `file` is the firmware file, mapped at GGTT address `fw_ggtt`. Needs the `Gt` domain
/// awake.
pub fn upload(
    hw: &mut impl Hw,
    img: &GucImage,
    file: &[u8],
    fw_ggtt: u64,
    wopcm_size: u32,
    params: &[u32; PARAM_WORDS],
) -> Result<Status> {
    check_ggtt(fw_ggtt, file.len() as u64, wopcm_size)?;
    write_params(hw, params);
    prepare_xfer(hw);
    xfer_rsa(hw, img, file, fw_ggtt)?;
    xfer(hw, img, fw_ggtt)?;
    wait_ready(hw, LOAD_TIMEOUT_US)
}

#[cfg(test)]
mod tests;
