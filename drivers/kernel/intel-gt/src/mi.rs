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
/// Continues execution in a batch (length field to be added).
pub const BATCH_BUFFER_START: u32 = instr(0x31);
/// The batch lies in the context's own address space, not in the GGTT.
pub const BATCH_PPGTT: u32 = 1 << 8;
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

/// `MI_FLUSH_DW` (copy engine): waits for the engine's writes to reach memory.
const FLUSH_DW: u32 = instr(0x26);
const FLUSH_DW_OP_STOREDW: u32 = 1 << 14;
const FLUSH_DW_USE_GTT: u32 = 1 << 2;
/// `PIPE_CONTROL` of six words (render and compute engine).
const PIPE_CONTROL: u32 = (3 << 29) | (3 << 27) | (2 << 24) | (6 - 2);
const PC0_HDC_PIPELINE_FLUSH: u32 = 1 << 9;
const PC_DEPTH_CACHE_FLUSH: u32 = 1 << 0;
const PC_DC_FLUSH: u32 = 1 << 5;
const PC_FLUSH_ENABLE: u32 = 1 << 7;
const PC_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
const PC_QW_WRITE: u32 = 1 << 14;
const PC_CS_STALL: u32 = 1 << 20;
const PC_GLOBAL_GTT: u32 = 1 << 24;
const PC_TILE_CACHE_FLUSH: u32 = 1 << 28;

/// End of a job on the copy engine: once its writes reached memory, `value` is stored at the
/// GGTT address `addr` (as Linux' `emit_flush_imm_ggtt`).
pub const fn flush_store_ggtt_copy(addr: u32, value: u32) -> [u32; 4] {
    [FLUSH_DW | FLUSH_DW_OP_STOREDW | (4 - 2), addr | FLUSH_DW_USE_GTT, 0, value]
}

/// End of a job on the render or compute engine: the engine stalls until the job is done, its
/// caches are written to memory, then `value` is stored (8 bytes, upper half zero) at the GGTT
/// address `addr`. Without this a program could see the job's number before its data. Flags as
/// Linux' `emit_render_cache_flush` and `emit_pipe_imm_ggtt`; the compute engine leaves out the
/// flushes of the 3D caches.
pub const fn flush_store_ggtt_render(addr: u32, value: u32, compute: bool) -> [u32; 6] {
    let three_d = if compute { 0 } else { PC_RENDER_TARGET_CACHE_FLUSH | PC_DEPTH_CACHE_FLUSH | PC_TILE_CACHE_FLUSH };
    let flags = PC_CS_STALL | PC_DC_FLUSH | PC_FLUSH_ENABLE | PC_QW_WRITE | PC_GLOBAL_GTT | three_d;
    [PIPE_CONTROL | PC0_HDC_PIPELINE_FLUSH, flags, addr, 0, value, 0]
}

/// Stores `value` at the address `addr` of the context's own address space.
pub const fn store_imm(addr: u64, value: u32) -> [u32; 4] {
    [STORE_DATA_IMM | 2, addr as u32, (addr >> 32) as u32, value]
}

/// Continues in the batch at `addr`: an address of the context's own address space, or of the
/// GGTT when `ggtt` is set.
pub const fn batch_start(addr: u64, ggtt: bool) -> [u32; 3] {
    let space = if ggtt { 0 } else { BATCH_PPGTT };
    [
        BATCH_BUFFER_START | space | 1,
        addr as u32,
        (addr >> 32) as u32,
    ]
}

/// A batch that stores `value` at `addr` of the context's address space and ends.
pub fn store_batch(addr: u64, value: u32) -> Vec<u32> {
    let mut batch = store_imm(addr, value).to_vec();
    batch.push(BATCH_BUFFER_END);
    batch.push(NOOP);
    batch
}

/// Ring contents that run the batch at `addr` of the context's address space; a multiple of
/// 8 bytes.
pub fn batch_job(addr: u64) -> Vec<u32> {
    let mut ring = alloc::vec![ARB_ON];
    ring.extend_from_slice(&batch_start(addr, false));
    ring.push(ARB_CHECK);
    ring.push(NOOP);
    ring
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
    fn batches_in_the_context_address_space() {
        assert_eq!(
            batch_start(0x1_0010_0000, false),
            [0x1880_0101, 0x0010_0000, 1]
        );
        assert_eq!(batch_start(0x5000, true)[0], 0x1880_0001);
        assert_eq!(
            store_batch(0x2_0020_0000, 9),
            [0x1000_0002, 0x0020_0000, 2, 9, BATCH_BUFFER_END, NOOP]
        );
        let ring = batch_job(0x10_0000);
        assert_eq!(ring.len() % 2, 0);
        assert_eq!(ring[1..4], batch_start(0x10_0000, false));
    }

    #[test]
    fn the_store_job_ends_on_a_qword() {
        let job = store_job(0x10_0000, 0xc0de);
        assert_eq!(job.len() % 2, 0);
        assert_eq!(job[0], ARB_ON);
        assert_eq!(job[4], 0xc0de);
    }

    #[test]
    fn job_ends_flush_before_they_store() {
        // MI_FLUSH_DW, store dword, length 2; address with the GGTT bit
        assert_eq!(flush_store_ggtt_copy(0x1000, 5), [0x1300_4002, 0x1004, 0, 5]);
        // PIPE_CONTROL (0x7a00_0004) with the HDC flush; CS stall, DC flush, flush enable,
        // immediate write, GGTT; the render engine also flushes the 3D caches
        let compute = flush_store_ggtt_render(0x2000, 9, true);
        assert_eq!(compute[0], 0x7a00_0204);
        assert_eq!(compute[1], 0x0110_40a0);
        assert_eq!(compute[2..], [0x2000, 0, 9, 0]);
        assert_eq!(flush_store_ggtt_render(0x2000, 9, false)[1], 0x1110_50a1);
    }
}
