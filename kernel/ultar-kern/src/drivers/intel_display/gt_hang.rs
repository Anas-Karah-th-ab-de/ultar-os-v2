//! What a hung job was doing, read from its memory after the GuC stopped it: the commands
//! around where the engine stands and, for the last compute dispatch before it, the buffers
//! its binding table names (base address, whether the GPU maps it). For finding a dispatch
//! that never ends; the layouts follow Mesa's `genxml/gen125.xml` (MIT).

use super::gt_run::Runtime;
use alloc::string::String;

/// Reads the word at GPU address `va` through the programs' address spaces.
fn word(rt: &mut Runtime, va: u64) -> Option<u32> {
    let tables = &mut rt.mem.tables;
    let page = rt
        .programs
        .vms
        .values_mut()
        .find_map(|vm| vm.lookup(tables, va & !0xfff))?;
    // SAFETY: memory bound to the GPU is mapped one to one in the kernel.
    Some(unsafe { core::ptr::read_volatile((page + (va & 0xfff)) as *const u32) })
}

/// Whether the GPU maps `va` to memory (not nothing).
fn mapped(rt: &mut Runtime, va: u64) -> bool {
    let tables = &mut rt.mem.tables;
    rt.programs
        .vms
        .values_mut()
        .any(|vm| vm.lookup(tables, va & !0xfff).is_some())
}

/// The nearest command before `va` (within `range` bytes) whose header matches `mask`/`want`.
fn find_back(rt: &mut Runtime, va: u64, range: u64, mask: u32, want: u32) -> Option<u64> {
    (1..range / 4)
        .map(|k| va - k * 4)
        .find(|a| word(rt, *a).is_some_and(|w| w & mask == want))
}

/// Prints the commands around `va` and the buffers of the dispatch before it.
pub(super) fn dump(rt: &mut Runtime, va: u64) {
    let start = va.saturating_sub(96 * 4);
    for line in 0..13u64 {
        let at = start + line * 32;
        let mut text = String::new();
        for k in 0..8u64 {
            match word(rt, at + k * 4) {
                Some(w) => text.push_str(&alloc::format!(" {w:08x}")),
                None => text.push_str(" --------"),
            }
        }
        let mark = if (at..at + 32).contains(&va) {
            " <"
        } else {
            ""
        };
        crate::kprintln!("[intel-gt]   {at:#x}:{text}{mark}");
    }
    buffers(rt, va);
}

/// COMPUTE_WALKER (gen12.5): binding table pointer in its interface descriptor; binding table
/// pool from 3DSTATE_BINDING_TABLE_POOL_ALLOC; surface state base from STATE_BASE_ADDRESS.
fn buffers(rt: &mut Runtime, va: u64) {
    const RANGE: u64 = 64 * 1024;
    let Some(walker) = find_back(rt, va, 2048, 0xffff_0000, 0x7208_0000) else {
        crate::kprintln!("[intel-gt] no compute dispatch before the hang");
        return;
    };
    let bt = u64::from(word(rt, walker + 22 * 4).unwrap_or(0) & 0x001f_ffe0);
    let pool = find_back(rt, walker, RANGE, 0xffff_00ff, 0x7919_0002).map(|a| {
        let lo = u64::from(word(rt, a + 4).unwrap_or(0) & !0xfff);
        lo | u64::from(word(rt, a + 8).unwrap_or(0)) << 32
    });
    let ssba = find_back(rt, walker, RANGE, 0xffff_0000, 0x6101_0000).map(|a| {
        let lo = u64::from(word(rt, a + 4 * 4).unwrap_or(0) & !0xfff);
        lo | u64::from(word(rt, a + 5 * 4).unwrap_or(0)) << 32
    });
    crate::kprintln!("[intel-gt] dispatch at {walker:#x}: binding table {bt:#x}, pool {pool:x?}, surface state base {ssba:x?}");
    let (Some(pool), Some(ssba)) = (pool, ssba) else {
        return;
    };
    for i in 0..4u64 {
        let Some(entry) = word(rt, pool + bt + i * 4) else {
            break;
        };
        let ss = ssba + u64::from(entry & !0x3f);
        let s: alloc::vec::Vec<u32> = (0..16)
            .map(|k| word(rt, ss + k * 4).unwrap_or(0xdead_beef))
            .collect();
        let base = u64::from(s[8]) | u64::from(s[9]) << 32;
        // buffer size: width 6:0 + height 20:0 << 7 + depth 10:0 << 21, plus one
        let size = (u64::from(s[2] & 0x7f)
            | u64::from((s[2] >> 16) & 0x3fff) << 7
            | u64::from(s[3] >> 21) << 21)
            + 1;
        let (first, last) = (mapped(rt, base), mapped(rt, base + size.saturating_sub(1)));
        crate::kprintln!(
            "[intel-gt]   binding {i}: entry {entry:#x}, surface {ss:#x}: base {base:#x}, size {size:#x}, mapped first/last {first}/{last}, words {:08x?}",
            &s[..4]
        );
    }
}
