//! Registers of the NPU 3720 (Meteor Lake / Arrow Lake): the buttress of that generation and the
//! IP block. Offsets and bit meanings as the Linux `ivpu` driver describes them
//! (`ivpu_hw_btrs_reg.h`, `ivpu_hw_ip_reg.h`).

use crate::hw::{genmask, Reg};

// --- buttress (BAR4) ---

/// Work point request: lowest and highest PLL ratio.
pub const WP_REQ_PAYLOAD0: Reg = Reg::b(0x08);
/// Lowest ratio.
pub const WP_MIN_RATIO: u32 = genmask(15, 0);
/// Highest ratio.
pub const WP_MAX_RATIO: u32 = genmask(31, 16);
/// Work point request: target ratio and energy/performance preference.
pub const WP_REQ_PAYLOAD1: Reg = Reg::b(0x0c);
/// Target ratio.
pub const WP_TARGET_RATIO: u32 = genmask(15, 0);
/// Energy/performance preference.
pub const WP_EPP: u32 = genmask(31, 16);
/// Work point request: configuration (tiles and PLL divider).
pub const WP_REQ_PAYLOAD2: Reg = Reg::b(0x10);
/// Configuration.
pub const WP_CONFIG: u32 = genmask(15, 0);
/// Work point request: command.
pub const WP_REQ_CMD: Reg = Reg::b(0x14);
/// Send the request; reads 0 again when the power unit took it.
pub const WP_REQ_CMD_SEND: u32 = 1 << 0;
/// The PLL ratio in use.
pub const CURRENT_PLL: Reg = Reg::b(0x1c);
/// Ratio.
pub const CURRENT_PLL_RATIO: u32 = genmask(15, 0);
/// Frequency fuse: lowest and nominal ratio.
pub const FMIN_FUSE: Reg = Reg::b(0x24);
/// Lowest ratio.
pub const FMIN_FUSE_MIN_RATIO: u32 = genmask(7, 0);
/// Nominal ("PN") ratio.
pub const FMIN_FUSE_PN_RATIO: u32 = genmask(15, 8);
/// Frequency fuse: highest ratio.
pub const FMAX_FUSE: Reg = Reg::b(0x28);
/// Highest ratio.
pub const FMAX_FUSE_MAX_RATIO: u32 = genmask(7, 0);
/// Tile fuse.
pub const TILE_FUSE: Reg = Reg::b(0x2c);
/// PLL status.
pub const PLL_STATUS: Reg = Reg::b(0x40);
/// The PLL is locked.
pub const PLL_STATUS_LOCK: u32 = 1 << 1;
/// NPU status.
pub const VPU_STATUS: Reg = Reg::b(0x44);
/// Powered and clocked.
pub const VPU_STATUS_READY: u32 = 1 << 0;
/// Nothing running.
pub const VPU_STATUS_IDLE: u32 = 1 << 1;
/// Reset of the IP.
pub const VPU_IP_RESET: Reg = Reg::b(0x50);
/// Start the reset; reads 0 again when it is done.
pub const VPU_IP_RESET_TRIGGER: u32 = 1 << 0;
/// Low-power state D0i3.
pub const VPU_D0I3_CONTROL: Reg = Reg::b(0x60);
/// A change of the state is in progress.
pub const VPU_D0I3_INPROGRESS: u32 = 1 << 0;
/// In (or asked into) D0i3.
pub const VPU_D0I3_I3: u32 = 1 << 2;

// --- IP block (BAR0) ---

/// Host subsystem clocks: set.
pub const HOST_SS_CPR_CLK_SET: Reg = Reg::ip(0x84);
/// Host subsystem resets: set (take out of reset).
pub const HOST_SS_CPR_RST_SET: Reg = Reg::ip(0x94);
/// Host subsystem resets: clear (put into reset).
pub const HOST_SS_CPR_RST_CLR: Reg = Reg::ip(0x98);
/// Always-on part; reads 0 once the IP block answers.
pub const CPR_AON: u32 = 1 << 0;
/// Top interconnect.
pub const CPR_TOP_NOC: u32 = 1 << 1;
/// Display/inference subsystem master.
pub const CPR_DSS_MAS: u32 = 1 << 10;
/// Processor subsystem master.
pub const CPR_MSS_MAS: u32 = 1 << 11;
/// Hardware version.
pub const HOST_SS_HW_VERSION: Reg = Reg::ip(0x108);
/// SoC revision.
pub const HW_VERSION_SOC_REVISION: u32 = genmask(7, 0);
/// SoC number.
pub const HW_VERSION_SOC_NUMBER: u32 = genmask(15, 8);
/// NPU generation.
pub const HW_VERSION_VPU_GENERATION: u32 = genmask(23, 16);
/// Host interconnect: request to leave the quiescent state.
pub const HOST_SS_NOC_QREQN: Reg = Reg::ip(0x154);
/// Host interconnect: accepted.
pub const HOST_SS_NOC_QACCEPTN: Reg = Reg::ip(0x158);
/// Host interconnect: denied.
pub const HOST_SS_NOC_QDENY: Reg = Reg::ip(0x15c);
/// The SoC MMIO port of the host interconnect.
pub const NOC_TOP_SOCMMIO: u32 = 1 << 0;
/// Top interconnect: request.
pub const TOP_NOC_QREQN: Reg = Reg::ip(0x160);
/// Top interconnect: accepted.
pub const TOP_NOC_QACCEPTN: Reg = Reg::ip(0x164);
/// Top interconnect: denied.
pub const TOP_NOC_QDENY: Reg = Reg::ip(0x168);
/// Processor control port.
pub const TOP_NOC_CPU_CTRL: u32 = 1 << 0;
/// Host interface L2 cache port.
pub const TOP_NOC_HOSTIF_L2CACHE: u32 = 1 << 1;
/// Power island isolation.
pub const AON_PWR_ISO_EN0: Reg = Reg::ip(0x3_0020);
/// Power island enable.
pub const AON_PWR_ISLAND_EN0: Reg = Reg::ip(0x3_0024);
/// Power island trickle (pre-charge) enable.
pub const AON_PWR_ISLAND_TRICKLE_EN0: Reg = Reg::ip(0x3_0028);
/// Power island status.
pub const AON_PWR_ISLAND_STATUS0: Reg = Reg::ip(0x3_002c);
/// The island of the NPU's processor.
pub const PWR_MSS_CPU: u32 = 1 << 3;
/// Idle generator.
pub const AON_VPU_IDLE_GEN: Reg = Reg::ip(0x3_0200);
/// Idle generator on.
pub const VPU_IDLE_GEN_EN: u32 = 1 << 0;
/// Marks the inference subsystem active.
pub const AON_DPU_ACTIVE: Reg = Reg::ip(0x3_0204);
/// Active.
pub const DPU_ACTIVE: u32 = 1 << 0;
