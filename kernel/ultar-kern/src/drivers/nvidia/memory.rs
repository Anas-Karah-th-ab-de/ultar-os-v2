//! VRAM ranges and system-memory DMA buffers.

use super::{Error, Result};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramAllocation {
    pub offset: u64,
    pub size: u64,
}

pub struct VramHeap {
    free: Vec<(u64, u64)>,
    allocated: Vec<VramAllocation>,
}

impl VramHeap {
    pub fn new(base: u64, size: u64) -> Result<Self> {
        if size == 0 || base.checked_add(size).is_none() {
            return Err(Error::InvalidArgument);
        }
        Ok(Self {
            free: alloc::vec![(base, size)],
            allocated: Vec::new(),
        })
    }

    pub fn free_bytes(&self) -> u64 {
        self.free.iter().map(|(_, size)| *size).sum()
    }

    pub fn allocate(&mut self, size: u64, align: u64) -> Result<VramAllocation> {
        if size == 0 || !align.is_power_of_two() {
            return Err(Error::InvalidArgument);
        }
        for i in 0..self.free.len() {
            let (base, available) = self.free[i];
            let Some(aligned) = base.checked_add(align - 1).map(|v| v & !(align - 1)) else {
                continue;
            };
            let prefix = aligned.saturating_sub(base);
            if prefix <= available && size <= available - prefix {
                let suffix = available - prefix - size;
                let mut replacement = Vec::new();
                if prefix != 0 {
                    replacement.push((base, prefix));
                }
                if suffix != 0 {
                    replacement.push((aligned + size, suffix));
                }
                self.free.splice(i..=i, replacement);
                let allocation = VramAllocation {
                    offset: aligned,
                    size,
                };
                self.allocated.push(allocation);
                return Ok(allocation);
            }
        }
        Err(Error::NoMemory)
    }

    pub fn release(&mut self, allocation: VramAllocation) -> Result<()> {
        let Some(index) = self.allocated.iter().position(|a| *a == allocation) else {
            return Err(Error::InvalidArgument);
        };
        self.allocated.swap_remove(index);
        self.free.push((allocation.offset, allocation.size));
        self.free.sort_unstable_by_key(|(base, _)| *base);
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.free.len());
        for (base, size) in self.free.drain(..) {
            if let Some((last_base, last_size)) = merged.last_mut() {
                if last_base.checked_add(*last_size) == Some(base) {
                    *last_size += size;
                    continue;
                }
            }
            merged.push((base, size));
        }
        self.free = merged;
        Ok(())
    }
}

/// Contiguous, zeroed system memory suitable for a DMA descriptor or command buffer.
pub struct DmaBuffer {
    physical: u64,
    bytes: usize,
    pages: usize,
}

impl DmaBuffer {
    pub fn allocate(bytes: usize) -> Result<Self> {
        if bytes == 0 {
            return Err(Error::InvalidArgument);
        }
        let pages = bytes.div_ceil(crate::mem::PAGE as usize);
        let physical = crate::mem::alloc_pages(pages).ok_or(Error::NoMemory)?;
        Ok(Self {
            physical,
            bytes,
            pages,
        })
    }

    pub fn physical(&self) -> u64 {
        self.physical
    }

    pub fn len(&self) -> usize {
        self.bytes
    }
}

impl Drop for DmaBuffer {
    fn drop(&mut self) {
        crate::mem::free_run(self.physical, self.pages as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_allocations_split_and_merge() {
        let mut heap = VramHeap::new(0x1000, 0x4000).unwrap();
        let a = heap.allocate(0x1000, 0x1000).unwrap();
        let b = heap.allocate(0x1000, 0x1000).unwrap();
        assert_eq!(heap.free_bytes(), 0x2000);
        heap.release(a).unwrap();
        heap.release(b).unwrap();
        assert_eq!(heap.free_bytes(), 0x4000);
    }

    #[test]
    fn rejects_invalid_ranges() {
        assert!(VramHeap::new(0, 0).is_err());
        let mut heap = VramHeap::new(0x1000, 0x1000).unwrap();
        assert!(heap.allocate(1, 3).is_err());
        assert!(heap.allocate(0x2000, 0x1000).is_err());
    }
}
