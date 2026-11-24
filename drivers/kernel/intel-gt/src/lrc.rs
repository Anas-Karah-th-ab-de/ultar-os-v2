//! The context image ("LRC"): the memory an engine loads its state from when the GuC schedules
//! a context, and saves it to when the context leaves. Layout: one status page, then the
//! engine's register state as a list of register loads the hardware replays.
//!
//! Follows Linux' `xe_lrc.c` and `regs/xe_lrc_layout.h` (MIT): the register lists are its
//! `gen12_xcs_offsets` and `mtl_rcs_offsets`, written out instead of byte-coded.

use crate::ads::Class;
use crate::mi;
use crate::regs::masked_enable;
use crate::{Error, Result};
use alloc::vec;
use alloc::vec::Vec;

/// Size of the status page in front of the register state.
pub const PPHWSP_SIZE: usize = 4096;

/// Word of the register state: context control.
pub const CTX_CONTEXT_CONTROL: usize = 0x03;
/// Word of the register state: ring head.
pub const CTX_RING_HEAD: usize = 0x05;
/// Word of the register state: ring tail.
pub const CTX_RING_TAIL: usize = 0x07;
/// Word of the register state: GGTT address of the ring.
pub const CTX_RING_START: usize = 0x09;
/// Word of the register state: ring size and valid bit.
pub const CTX_RING_CTL: usize = 0x0b;
/// Word of the register state: upper half of the page table root.
pub const CTX_PDP0_UDW: usize = 0x31;
/// Word of the register state: lower half of the page table root.
pub const CTX_PDP0_LDW: usize = 0x33;

const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const RING_VALID: u32 = 1;

const LRC_VALID: u64 = 1 << 0;
const LRC_LEGACY_64B_CONTEXT: u64 = 3 << 3;
const LRC_PRIVILEGE: u64 = 1 << 8;

enum Op {
    /// Leave words as they are.
    Skip(usize),
    /// One register load: engine-relative offsets, and whether the load is posted.
    Load(&'static [u32], bool),
}

const XCS: &[Op] = &[
    Op::Skip(1),
    Op::Load(
        &[
            0x244, 0x034, 0x030, 0x038, 0x03c, 0x168, 0x140, 0x110, 0x1c0, 0x1c4, 0x1c8, 0x180,
            0x2b4,
        ],
        true,
    ),
    Op::Skip(5),
    Op::Load(
        &[
            0x3a8, 0x28c, 0x288, 0x284, 0x280, 0x27c, 0x278, 0x274, 0x270,
        ],
        true,
    ),
];

const MTL_RCS: &[Op] = &[
    Op::Skip(1),
    Op::Load(
        &[
            0x244, 0x034, 0x030, 0x038, 0x03c, 0x168, 0x140, 0x110, 0x1c0, 0x1c4, 0x1c8, 0x180,
            0x2b4, 0x120, 0x124,
        ],
        true,
    ),
    Op::Skip(1),
    Op::Load(
        &[
            0x3a8, 0x28c, 0x288, 0x284, 0x280, 0x27c, 0x278, 0x274, 0x270,
        ],
        true,
    ),
    Op::Skip(2),
    Op::Load(&[0x5a8, 0x5ac], true),
    Op::Skip(6),
    Op::Load(&[0x0c8], false),
];

/// The register state of a context that never ran, for an engine of `class` at `mmio_base` on
/// Xe_LPG: the register loads with zero values, the end marker, and "do not restore the engine
/// state" (there is none yet). One page of words.
pub fn empty_state(class: Class, mmio_base: u32) -> Vec<u32> {
    let mut regs = vec![0u32; PPHWSP_SIZE / 4];
    let table = if class == Class::Render { MTL_RCS } else { XCS };
    let mut at = 0;
    for op in table {
        match op {
            Op::Skip(n) => at += n,
            Op::Load(offsets, posted) => {
                regs[at] = mi::lri(offsets.len() as u32, *posted);
                at += 1;
                for off in *offsets {
                    regs[at] = mmio_base + off;
                    at += 2;
                }
            }
        }
    }
    regs[at] = mi::BATCH_BUFFER_END | 1;
    regs[CTX_CONTEXT_CONTROL] =
        masked_enable(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH | CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    regs
}

/// What a new context is made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    /// The engine's class.
    pub class: Class,
    /// Register base of the engine.
    pub mmio_base: u32,
    /// GGTT address of the ring.
    pub ring_ggtt: u32,
    /// Size of the ring in bytes (a power of two, 4K to 2M).
    pub ring_bytes: u32,
    /// How many bytes of commands wait in the ring (a multiple of 8).
    pub tail: u32,
    /// Root of the context's page tables; `None` for a context that only uses the GGTT.
    pub pml4: Option<u64>,
}

impl Context {
    fn check(&self) -> Result<()> {
        let ring_ok = self.ring_bytes.is_power_of_two()
            && (4096..=2 << 20).contains(&self.ring_bytes)
            && self.ring_ggtt.is_multiple_of(4096);
        if !ring_ok {
            return Err(Error::Invalid("context ring"));
        }
        if self.tail >= self.ring_bytes || !self.tail.is_multiple_of(8) {
            return Err(Error::Invalid("context ring tail"));
        }
        if self.pml4.is_some_and(|p| !p.is_multiple_of(4096)) {
            return Err(Error::Invalid("context page table root"));
        }
        Ok(())
    }

    /// The whole image ([`Class::lrc_size`] bytes): an empty status page, the register state
    /// with ring and page tables filled in, and zeroes for the engine state behind it.
    pub fn image(&self) -> Result<Vec<u8>> {
        self.check()?;
        let mut regs = empty_state(self.class, self.mmio_base);
        regs[CTX_RING_START] = self.ring_ggtt;
        regs[CTX_RING_HEAD] = 0;
        regs[CTX_RING_TAIL] = self.tail;
        regs[CTX_RING_CTL] = (self.ring_bytes - 4096) | RING_VALID;
        if let Some(root) = self.pml4 {
            regs[CTX_PDP0_UDW] = (root >> 32) as u32;
            regs[CTX_PDP0_LDW] = root as u32;
        }
        let mut image = vec![0u8; self.class.lrc_size() as usize];
        for (i, w) in regs.iter().enumerate() {
            let at = PPHWSP_SIZE + i * 4;
            image[at..at + 4].copy_from_slice(&w.to_le_bytes());
        }
        Ok(image)
    }

    /// The descriptor the GuC is given: where the image is mapped in the GGTT and how the
    /// context addresses memory.
    pub fn descriptor(&self, image_ggtt: u32) -> u64 {
        let mode = if self.pml4.is_some() {
            LRC_PRIVILEGE
        } else {
            0
        };
        LRC_VALID | LRC_LEGACY_64B_CONTEXT | mode | image_ggtt as u64
    }
}

/// Word `index` of the register state inside an image (what the engine saved there).
pub fn state_word(image: &[u8], index: usize) -> Option<u32> {
    let at = PPHWSP_SIZE + index * 4;
    let b = image.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BCS: u32 = 0x22000;

    #[test]
    fn copy_engine_state_has_the_registers_where_the_layout_says() {
        let r = empty_state(Class::Blitter, BCS);
        assert_eq!(r[0], 0);
        assert_eq!(r[1], mi::lri(13, true));
        assert_eq!(r[CTX_CONTEXT_CONTROL - 1], BCS + 0x244);
        assert_eq!(r[CTX_CONTEXT_CONTROL], 0x0009_0009);
        assert_eq!(r[CTX_RING_HEAD - 1], BCS + 0x34);
        assert_eq!(r[CTX_RING_TAIL - 1], BCS + 0x30);
        assert_eq!(r[CTX_RING_START - 1], BCS + 0x38);
        assert_eq!(r[CTX_RING_CTL - 1], BCS + 0x3c);
        assert_eq!(r[0x21], mi::lri(9, true));
        assert_eq!(r[0x22], BCS + 0x3a8);
        assert_eq!(r[CTX_PDP0_UDW - 1], BCS + 0x274);
        assert_eq!(r[CTX_PDP0_LDW - 1], BCS + 0x270);
        assert_eq!(r[0x34], mi::BATCH_BUFFER_END | 1);
        assert!(r[0x35..].iter().all(|w| *w == 0));
    }

    #[test]
    fn render_state_has_the_longer_lists() {
        let r = empty_state(Class::Render, 0x2000);
        assert_eq!(r[1], mi::lri(15, true));
        assert_eq!(r[0x21], mi::lri(9, true));
        assert_eq!(r[CTX_PDP0_LDW - 1], 0x2000 + 0x270);
        assert_eq!(r[0x36], mi::lri(2, true));
        assert_eq!(r[0x37], 0x2000 + 0x5a8);
        assert_eq!(r[0x41], mi::lri(1, false));
        assert_eq!(r[0x42], 0x2000 + 0xc8);
        assert_eq!(r[0x44], mi::BATCH_BUFFER_END | 1);
    }

    fn ctx() -> Context {
        Context {
            class: Class::Blitter,
            mmio_base: BCS,
            ring_ggtt: 0x0200_0000,
            ring_bytes: 16384,
            tail: 24,
            pml4: None,
        }
    }

    #[test]
    fn image_carries_ring_and_page_tables() {
        let img = ctx().image().unwrap();
        assert_eq!(img.len(), 2 * 4096);
        assert!(img[..PPHWSP_SIZE].iter().all(|b| *b == 0));
        assert_eq!(state_word(&img, CTX_RING_START), Some(0x0200_0000));
        assert_eq!(state_word(&img, CTX_RING_HEAD), Some(0));
        assert_eq!(state_word(&img, CTX_RING_TAIL), Some(24));
        assert_eq!(state_word(&img, CTX_RING_CTL), Some(0x3001));
        assert_eq!(state_word(&img, CTX_PDP0_LDW), Some(0));

        let with_vm = Context {
            pml4: Some(0x1_2345_6000),
            ..ctx()
        };
        let img = with_vm.image().unwrap();
        assert_eq!(state_word(&img, CTX_PDP0_UDW), Some(1));
        assert_eq!(state_word(&img, CTX_PDP0_LDW), Some(0x2345_6000));
        assert_eq!(state_word(&img, 1 << 20), None);
    }

    #[test]
    fn descriptor_says_where_and_how() {
        assert_eq!(ctx().descriptor(0x0300_0000), 0x0300_0019);
        let with_vm = Context {
            pml4: Some(0x1000),
            ..ctx()
        };
        assert_eq!(with_vm.descriptor(0x0300_0000), 0x0300_0119);
    }

    #[test]
    fn bad_rings_are_refused() {
        for bad in [
            Context {
                ring_bytes: 12288,
                ..ctx()
            },
            Context {
                ring_bytes: 4 << 20,
                ..ctx()
            },
            Context {
                ring_ggtt: 0x0200_0010,
                ..ctx()
            },
            Context { tail: 20, ..ctx() },
            Context {
                tail: 16384,
                ..ctx()
            },
            Context {
                pml4: Some(0x1001),
                ..ctx()
            },
        ] {
            assert!(bad.image().is_err(), "{bad:?}");
        }
    }
}
