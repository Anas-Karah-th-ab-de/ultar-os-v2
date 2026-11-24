//! Commands every engine understands (the "MI" group): what a ring or a batch is made of.
//! Encodings follow Linux' `instructions/xe_mi_commands.h` (MIT).

use alloc::vec::Vec;

const fn instr(opcode: u32) -> u32 {
    opcode << 23
}

/// Does nothing; fills a ring up to the next 8 bytes.
pub const NOOP: u32 = instr(0x0);
/// Raises the engine's user interrupt.
pub const USER_INTERRUPT: u32 = instr(0x2);
/// A point where the engine may be preempted.
pub const ARB_CHECK: u32 = instr(0x5);
/// Preemption off.
pub const ARB_OFF: u32 = instr(0x8);
/// Preemption on.
pub const ARB_ON: u32 = instr(0x8) | 1;
/// End of a batch.
pub const BATCH_BUFFER_END: u32 = instr(0xa);
/// Stores immediate words in memory (length field to be added).
pub const STORE_DATA_IMM: u32 = instr(0x20);
/// The address of a store is a GGTT address.
pub const SDI_GGTT: u32 = 1 << 22;
/// Loads registers with immediate values (length field to be added).
pub const LOAD_REGISTER_IMM: u32 = instr(0x22);
/// The registers of a load are waited for.
pub const LRI_FORCE_POSTED: u32 = 1 << 12;
/// Register offsets of a load are relative to the engine.
pub const LRI_CS_MMIO: u32 = 1 << 19;

/// Header of a register load of `count` registers (address and value follow for each).
pub const fn lri(count: u32, posted: bool) -> u32 {
    LOAD_REGISTER_IMM | LRI_CS_MMIO | (2 * count - 1) | if posted { LRI_FORCE_POSTED } else { 0 }
}

/// Stores `value` at the GGTT address `addr`.
pub const fn store_imm_ggtt(addr: u32, value: u32) -> [u32; 4] {
    [STORE_DATA_IMM | SDI_GGTT | 2, addr, 0, value]
}

/// The smallest job that proves an engine runs: it stores `value` at the GGTT address `addr`.
/// The words go to the start of a context's ring; the length is a multiple of 8 bytes as the
/// ring's tail must be.
pub fn store_job(addr: u32, value: u32) -> Vec<u32> {
    let mut ring = Vec::with_capacity(8);
    ring.push(ARB_ON);
    ring.extend_from_slice(&store_imm_ggtt(addr, value));
    ring.push(ARB_CHECK);
    while ring.len() % 2 != 0 {
        ring.push(NOOP);
    }
    ring
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_match_the_hardware_documentation() {
        assert_eq!(ARB_ON, 0x0400_0001);
        assert_eq!(BATCH_BUFFER_END, 0x0500_0000);
        assert_eq!(lri(13, true), 0x1108_1019);
        assert_eq!(lri(1, false), 0x1108_0001);
        assert_eq!(
            store_imm_ggtt(0x1234_5000, 7),
            [0x1040_0002, 0x1234_5000, 0, 7]
        );
    }

    #[test]
    fn the_store_job_ends_on_a_qword() {
        let job = store_job(0x10_0000, 0xc0de);
        assert_eq!(job.len() % 2, 0);
        assert_eq!(job[0], ARB_ON);
        assert_eq!(job[4], 0xc0de);
    }
}
