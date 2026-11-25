//! The GuC's own power management (SLPC): it chooses the GT's frequency from the load, up to the
//! highest the fuses allow, and lets the GT sleep (RC6) when it is idle. The kernel starts it with
//! a shared page and a reset request, then sets the limits and the strategy for compute work.
//!
//! Message layout, event and parameter numbers and the shared data layout follow Linux'
//! MIT-licensed `abi/guc_actions_slpc_abi.h` and `xe_guc_pc.c` (`xe_guc_pc_start`).

/// `GUC_ACTION_HOST2GUC_PC_SLPC_REQUEST`.
pub const ACTION_SLPC_REQUEST: u32 = 0x3003;
/// `GUC_ACTION_HOST2GUC_SETUP_PC_GUCRC`.
pub const ACTION_SETUP_GUCRC: u32 = 0x3004;
/// The GuC decides about RC6 itself.
pub const GUCRC_FIRMWARE_CONTROL: u32 = 1;

const EVENT_RESET: u32 = 0;
const EVENT_QUERY_TASK_STATE: u32 = 5;
const EVENT_PARAMETER_SET: u32 = 6;

/// Parameter: lowest frequency in MHz.
pub const PARAM_MIN_FREQ_MHZ: u32 = 6;
/// Parameter: highest frequency in MHz.
pub const PARAM_MAX_FREQ_MHZ: u32 = 7;
/// Parameter: strategies (bit 0: optimized for compute).
pub const PARAM_STRATEGIES: u32 = 26;
/// `SLPC_OPTIMIZED_STRATEGY_COMPUTE`.
pub const STRATEGY_COMPUTE: u32 = 1;

/// Size of the shared data (`struct slpc_shared_data`, two pages).
pub const SHARED_BYTES: usize = 8192;
/// Byte offset of `header.size`.
const OFF_SIZE: usize = 0;
/// Byte offset of `header.global_state`.
pub const OFF_GLOBAL_STATE: usize = 4;
/// Byte offset of `task_state_data.freq`.
const OFF_TASK_FREQ: usize = 132;

/// `SLPC_GLOBAL_STATE_RUNNING`.
pub const STATE_RUNNING: u32 = 3;

fn event(id: u32, args: u32) -> u32 {
    (id & 0xff) << 8 | (args & 0xff)
}

/// The shared page as it is handed to the GuC: zero, with its size in the header.
pub fn shared_init() -> [u8; 8] {
    let mut head = [0u8; 8];
    head[OFF_SIZE..OFF_SIZE + 4].copy_from_slice(&(SHARED_BYTES as u32).to_le_bytes());
    head
}

/// Starts (or restarts) SLPC with the shared data at GGTT address `shared`.
pub fn reset(shared: u32) -> [u32; 4] {
    [ACTION_SLPC_REQUEST, event(EVENT_RESET, 2), shared, 0]
}

/// Asks SLPC to write its current state (limits) into the shared data.
pub fn query_task_state(shared: u32) -> [u32; 4] {
    [
        ACTION_SLPC_REQUEST,
        event(EVENT_QUERY_TASK_STATE, 2),
        shared,
        0,
    ]
}

/// Sets parameter `id` to `value`.
pub fn set_param(id: u32, value: u32) -> [u32; 4] {
    [
        ACTION_SLPC_REQUEST,
        event(EVENT_PARAMETER_SET, 2),
        id,
        value,
    ]
}

/// Lets the GuC put the GT to sleep when it is idle.
pub fn setup_gucrc() -> [u32; 2] {
    [ACTION_SETUP_GUCRC, GUCRC_FIRMWARE_CONTROL]
}

fn word(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// `header.global_state` of the shared data.
pub fn global_state(data: &[u8]) -> u32 {
    word(data, OFF_GLOBAL_STATE)
}

/// The limits SLPC reports after [`query_task_state`]: lowest and highest frequency in MHz.
pub fn limits_mhz(data: &[u8]) -> (u32, u32) {
    let f = word(data, OFF_TASK_FREQ);
    let mhz = |ratio: u32| (ratio * 50 + 1) / 3;
    (mhz((f >> 8) & 0xff), mhz(f & 0xff))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_as_linux_sends_them() {
        assert_eq!(reset(0x1234_5000), [0x3003, 0x0002, 0x1234_5000, 0]);
        assert_eq!(query_task_state(0x5000), [0x3003, 0x0502, 0x5000, 0]);
        assert_eq!(
            set_param(PARAM_MAX_FREQ_MHZ, 2350),
            [0x3003, 0x0602, 7, 2350]
        );
        assert_eq!(
            set_param(PARAM_STRATEGIES, STRATEGY_COMPUTE),
            [0x3003, 0x0602, 26, 1]
        );
        assert_eq!(setup_gucrc(), [0x3004, 1]);
    }

    #[test]
    fn shared_data_fields() {
        assert_eq!(shared_init(), [0, 0x20, 0, 0, 0, 0, 0, 0]);
        let mut data = alloc::vec![0u8; SHARED_BYTES];
        data[4] = 3;
        // max ratio 141 (2350 MHz) in bits 7:0, min ratio 48 (800 MHz) in bits 15:8
        data[132] = 141;
        data[133] = 48;
        assert_eq!(global_state(&data), STATE_RUNNING);
        assert_eq!(limits_mhz(&data), (800, 2350));
    }
}
