//! Register offsets and bits of the GT (byte offsets in BAR0). Names and values follow the Linux
//! `xe` driver's register headers (`regs/xe_gt_regs.h`, `regs/xe_guc_regs.h`, MIT).

use crate::genmask;

/// The media GT sees the GT register range `0..MEDIA_GSI_LENGTH` moved up by this offset.
pub const MEDIA_GSI_OFFSET: u32 = 0x38_0000;
/// Length of the register range that moves for the media GT.
pub const MEDIA_GSI_LENGTH: u32 = 0x4_0000;

/// IP identification: architecture, release, revision.
pub const GMD_ID: u32 = 0xd8c;
/// Architecture (major version).
pub const GMD_ID_ARCH: u32 = genmask(31, 22);
/// Release (minor version).
pub const GMD_ID_RELEASE: u32 = genmask(21, 14);
/// Stepping.
pub const GMD_ID_REVID: u32 = genmask(5, 0);

/// Forcewake request, GT domain.
pub const FORCEWAKE_GT: u32 = 0xa188;
/// Forcewake request, render domain.
pub const FORCEWAKE_RENDER: u32 = 0xa278;
/// Forcewake request, GSC domain.
pub const FORCEWAKE_GSC: u32 = 0xa618;
/// Forcewake acknowledge, GT domain (graphics 12.70 and later).
pub const FORCEWAKE_ACK_GT_MTL: u32 = 0xdfc;
/// Forcewake acknowledge, render domain.
pub const FORCEWAKE_ACK_RENDER: u32 = 0xd84;
/// Forcewake acknowledge, GSC domain.
pub const FORCEWAKE_ACK_GSC: u32 = 0xdf8;
/// Forcewake request of video decode box `n` (0..8).
pub const fn forcewake_media_vdbox(n: u32) -> u32 {
    0xa540 + n * 4
}
/// Forcewake request of video enhancement box `n` (0..4).
pub const fn forcewake_media_vebox(n: u32) -> u32 {
    0xa560 + n * 4
}
/// Forcewake acknowledge of video decode box `n`.
pub const fn forcewake_ack_media_vdbox(n: u32) -> u32 {
    0xd50 + n * 4
}
/// Forcewake acknowledge of video enhancement box `n`.
pub const fn forcewake_ack_media_vebox(n: u32) -> u32 {
    0xd70 + n * 4
}
/// The kernel's bit in the request and acknowledge registers.
pub const FORCEWAKE_KERNEL: u32 = 1 << 0;
/// Write-enable mask of the kernel's bit (masked register: the upper half enables the lower).
pub const FORCEWAKE_KERNEL_MASK: u32 = 1 << 16;

/// Media fuses: which video boxes exist.
pub const GT_VEBOX_VDBOX_DISABLE: u32 = 0x9140;
/// Video enhancement boxes present (graphics 12.50 and later: 1 = present).
pub const GT_VEBOX_MASK: u32 = genmask(19, 16);
/// Video decode boxes present.
pub const GT_VDBOX_MASK: u32 = genmask(7, 0);

/// Reset requests.
pub const GDRST: u32 = 0x941c;
/// Reset the GuC.
pub const GRDOM_GUC: u32 = 1 << 3;

/// Power-management interrupt mask.
pub const PMINTRMSK: u32 = 0xa168;
/// ARAT timer expiry masked (must be clear so the GuC gets its timer interrupts).
pub const ARAT_EXPIRED_INTRMSK: u32 = 1 << 9;

/// GuC status.
pub const GUC_STATUS: u32 = 0xc000;
/// Signature check result.
pub const GS_AUTH_STATUS: u32 = genmask(31, 30);
/// State of the GuC's processor core.
pub const GS_MIA: u32 = genmask(18, 16);
/// Progress of the firmware's own start code.
pub const GS_UKERNEL: u32 = genmask(15, 8);
/// Progress of the boot ROM.
pub const GS_BOOTROM: u32 = genmask(7, 1);
/// The GuC's processor core is held in reset.
pub const GS_MIA_IN_RESET: u32 = 1 << 0;

/// Size of the GuC's part of the protected memory.
pub const GUC_WOPCM_SIZE: u32 = 0xc050;
/// The size (4 KiB units in place).
pub const GUC_WOPCM_SIZE_MASK: u32 = genmask(31, 12);
/// The size is locked until the next reset of the machine.
pub const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;

/// Control of the GuC's shim.
pub const GUC_SHIM_CONTROL: u32 = 0xc064;
/// Clock gating of the processor core.
pub const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
/// Read cache for data in the protected memory.
pub const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
/// Read cache for data in the GuC's RAM.
pub const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
/// Read cache logic.
pub const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;

/// Scratch register `n` (0..16): boot parameters and the MMIO message channel.
pub const fn soft_scratch(n: u32) -> u32 {
    0xc180 + n * 4
}
/// Number of scratch registers.
pub const SOFT_SCRATCH_COUNT: u32 = 16;

/// The HuC was loaded.
pub const HUC_KERNEL_LOAD_INFO: u32 = 0xc1dc;

/// RSA signature register `i` (0..64).
pub const fn uos_rsa_scratch(i: u32) -> u32 {
    0xc200 + i * 4
}
/// Number of RSA signature registers (256 bytes).
pub const UOS_RSA_SCRATCH_COUNT: u32 = 64;

/// DMA source address, low half.
pub const DMA_ADDR_0_LOW: u32 = 0xc300;
/// DMA source address, high half and address space.
pub const DMA_ADDR_0_HIGH: u32 = 0xc304;
/// DMA destination address, low half.
pub const DMA_ADDR_1_LOW: u32 = 0xc308;
/// DMA destination address, high half and address space.
pub const DMA_ADDR_1_HIGH: u32 = 0xc30c;
/// Address space: the protected memory.
pub const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
/// Address space: the global GTT.
pub const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
/// Number of bytes to copy.
pub const DMA_COPY_SIZE: u32 = 0xc310;
/// DMA control (masked register).
pub const DMA_CTRL: u32 = 0xc314;
/// The copy is the GuC firmware ("uOS move").
pub const UOS_MOVE: u32 = 1 << 4;
/// Start the copy; reads 0 again when it is done.
pub const START_DMA: u32 = 1 << 0;

/// Where the GuC's part of the protected memory starts.
pub const DMA_GUC_WOPCM_OFFSET: u32 = 0xc340;
/// The offset (16 KiB units in place).
pub const GUC_WOPCM_OFFSET_MASK: u32 = genmask(31, 14);
/// The HuC is loaded by the GuC.
pub const HUC_LOADING_AGENT_GUC: u32 = 1 << 1;
/// The offset is set (and locked).
pub const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;

/// GT power-management configuration.
pub const GT_PM_CONFIG: u32 = 0x13_816c;
/// Doorbells enabled.
pub const GT_DOORBELL_ENABLE: u32 = 1 << 0;

/// Value that enables `bits` of a masked register.
pub const fn masked_enable(bits: u32) -> u32 {
    (bits << 16) | bits
}
/// Value that disables `bits` of a masked register.
pub const fn masked_disable(bits: u32) -> u32 {
    bits << 16
}

/// How many doorbells exist per SQIDI.
pub const DIST_DBS_POPULATED: u32 = 0xd08;

/// Message register `n` (0..4) of the primary GT's GuC: requests and replies.
pub const fn vf_sw_flag(n: u32) -> u32 {
    0x19_0240 + n * 4
}
/// Writing here tells the primary GT's GuC that a message waits.
pub const GUC_HOST_INTERRUPT: u32 = 0x19_01f0;

/// Engine register (offset from the engine's base): ring tail.
pub const RING_TAIL: u32 = 0x30;
/// Engine register: ring head.
pub const RING_HEAD: u32 = 0x34;
/// Engine register: GGTT address of the ring.
pub const RING_START: u32 = 0x38;
/// Engine register: ring size and valid bit.
pub const RING_CTL: u32 = 0x3c;
/// Engine register: the command that was executing when an error was raised.
pub const RING_IPEHR: u32 = 0x68;
/// Engine register: address of the command being executed.
pub const RING_ACTHD: u32 = 0x74;
/// Engine register: GGTT address of the engine's status page.
pub const RING_HWS_PGA: u32 = 0x80;
/// Engine register: which interrupts do not update the status page.
pub const RING_HWSTAM: u32 = 0x98;
/// Engine register: error identity.
pub const RING_EIR: u32 = 0xb0;
/// Engine register: error status.
pub const RING_ESR: u32 = 0xb8;
/// Engine register: execution status (lower half).
pub const RING_EXECLIST_STATUS_LO: u32 = 0x234;
/// Engine register: the context image the engine runs.
pub const RING_CURRENT_LRCA: u32 = 0x240;
/// Engine register (masked): mode of the engine.
pub const GFX_MODE: u32 = 0x29c;
/// The engine runs contexts, not the old ring mode.
pub const GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
