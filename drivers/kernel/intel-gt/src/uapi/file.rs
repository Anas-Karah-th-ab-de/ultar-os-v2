//! What one open of the render node owns: buffers, address spaces, exec queues and the fences
//! of jobs that still run – and the requests that work on them (stages G4c to G4e). The
//! hardware side is behind [`Backend`]; everything here is checked on the host.
//!
//! Follows the request semantics of Linux' `xe` driver (`xe_bo.c`, `xe_vm.c`,
//! `xe_exec_queue.c`, `xe_exec.c`, `xe_wait_user_fence.c`; MIT). Simpler than Linux in these
//! points, each refused with an error instead of done wrongly: a job that waits for a sync
//! object makes its exec wait (Linux queues it), no sparse or user-pointer bindings, a binding can only be replaced or removed whole,
//! one batch per exec, one engine per queue.

use super::drm::User;
use super::sync::{self, Point, Sync, SyncObj};
use super::{EngineClass, Errno};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// The GPU as the requests need it.
pub trait Backend {
    /// A buffer: memory pages that stay where they are while a clone of this lives.
    type Buffer: Clone;
    /// A zeroed buffer of `size` bytes (a multiple of the page size).
    fn buffer_new(&mut self, size: u64) -> Result<Self::Buffer, Errno>;
    /// The buffer's memory pages, in order.
    fn buffer_pages(&self, b: &Self::Buffer) -> Vec<u64>;
    /// A new address space; returns its number.
    fn vm_new(&mut self) -> Result<u32, Errno>;
    /// Maps memory pages at `va` of address space `vm`.
    fn vm_map(&mut self, vm: u32, va: u64, pages: &[u64]) -> Result<(), Errno>;
    /// Removes `pages` pages at `va` and makes the GPU forget them.
    fn vm_unmap(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno>;
    /// Maps `pages` pages at `va` of address space `vm` to nothing (writes vanish, reads give 0).
    fn vm_map_null(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno>;
    /// Ends an address space (no queue uses it any more).
    fn vm_free(&mut self, vm: u32);
    /// A new queue on an engine of `class` running in address space `vm`; returns its number.
    fn queue_new(&mut self, vm: u32, class: EngineClass) -> Result<u32, Errno>;
    /// Runs the batch at `batch_va`; returns the job's sequence number (they count up from 1).
    fn queue_run(&mut self, queue: u32, batch_va: u64) -> Result<u64, Errno>;
    /// Sequence number of the last job of the queue that finished (0: none yet); `u64::MAX`
    /// once the queue was stopped because a job hung (nothing of it will finish any more).
    fn queue_done(&mut self, queue: u32) -> u64;
    /// Whether the queue was stopped because a job of it hung.
    fn queue_banned(&mut self, queue: u32) -> bool;
    /// Ends a queue.
    fn queue_free(&mut self, queue: u32);
    /// Monotonic time in ns.
    fn now_ns(&mut self) -> u64;
}

const PAGE: u64 = 4096;
const MAX_BUFFER_BYTES: u64 = 1 << 30;
const MAX_BUFFERS: usize = 4096;
const MAX_VMS: usize = 16;
const MAX_QUEUES: usize = 64;
const MAX_BINDS: usize = 16384;
const MAX_BIND_OPS: u32 = 256;
const MAX_FENCES: usize = 1024;
const VA_END: u64 = 1 << 48;

struct Bind<B: Backend> {
    va: u64,
    pages: u64,
    handle: u32,
    /// Keeps the memory alive while the GPU can reach it; `None`: the range maps to nothing.
    buffer: Option<B::Buffer>,
}

struct Vm<B: Backend> {
    backend: u32,
    binds: Vec<Bind<B>>,
}

struct Queue {
    backend: u32,
    vm: u32,
}

/// A value to store in the program's memory once a job finished.
struct Fence {
    queue: u32,
    seqno: u64,
    addr: u64,
    value: u64,
}

/// The state behind one open of the render node.
pub struct File<B: Backend> {
    buffers: BTreeMap<u32, (B::Buffer, u64, u32)>,
    vms: BTreeMap<u32, Vm<B>>,
    queues: BTreeMap<u32, Queue>,
    pub(super) syncs: BTreeMap<u32, SyncObj>,
    fences: Vec<Fence>,
    /// A wait in progress: address, value and the time it ends.
    waiting: Option<(u64, u64, u64)>,
    next_id: u32,
}

impl<B: Backend> Default for File<B> {
    fn default() -> Self {
        File {
            buffers: BTreeMap::new(),
            vms: BTreeMap::new(),
            queues: BTreeMap::new(),
            syncs: BTreeMap::new(),
            fences: Vec::new(),
            waiting: None,
            next_id: 1,
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u32_at(b, at) as u64 | (u32_at(b, at + 4) as u64) << 32
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// The first `len` bytes of a request structure; every byte from `zero_from` on must be zero
/// (reserved fields), as must the extension pointer at the start when `no_ext`.
fn fields(arg: &[u8], len: usize, zero_from: usize, no_ext: bool) -> Result<&[u8], Errno> {
    let b = arg.get(..len).ok_or(Errno::Inval)?;
    if b[zero_from..].iter().any(|x| *x != 0) || (no_ext && u64_at(b, 0) != 0) {
        return Err(Errno::Inval);
    }
    Ok(b)
}

impl<B: Backend> File<B> {
    /// The backend's number of queue `id`, if the file has it.
    pub(super) fn queue_backend(&self, id: u32) -> Option<u32> {
        self.queues.get(&id).map(|q| q.backend)
    }

    /// A new handle or id.
    pub(super) fn new_id(&mut self) -> u32 {
        self.id()
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// The buffer a mapping offset from [`File::gem_mmap_offset`] names, and its size.
    pub fn buffer_at(&self, offset: u64) -> Option<(&B::Buffer, u64)> {
        if offset & 0xffff_ffff != 0 {
            return None;
        }
        let (b, size, _) = self.buffers.get(&((offset >> 32) as u32))?;
        Some((b, *size))
    }

    /// Stores the values of fences whose jobs finished. Called before every request.
    pub fn reap(&mut self, be: &mut B, user: &mut impl User) {
        let mut done: BTreeMap<u32, u64> = BTreeMap::new();
        for f in &self.fences {
            let queues = &self.queues;
            done.entry(f.queue)
                .or_insert_with(|| match queues.get(&f.queue) {
                    Some(q) => be.queue_done(q.backend),
                    // the queue is gone: its jobs will not finish any more
                    None => u64::MAX,
                });
        }
        self.fences.retain(|f| {
            let finished = done.get(&f.queue).is_some_and(|d| *d >= f.seqno);
            if finished {
                // a fence address that is no longer the program's memory is the program's loss
                let _ = user.write(f.addr, &f.value.to_le_bytes());
            }
            !finished
        });
    }

    /// `DRM_IOCTL_XE_GEM_CREATE`.
    pub fn gem_create(&mut self, be: &mut B, arg: &mut [u8]) -> Result<(), Errno> {
        let b = fields(arg, 56, 34, true)?;
        let (size, placement, flags) = (u64_at(b, 8), u32_at(b, 16), u32_at(b, 20));
        let (vm, caching) = (u32_at(b, 24), u16_at(b, 32));
        // placement: system memory is region 0; caching: 1 = write-back, 2 = write-combining
        let bad = size == 0
            || size % PAGE != 0
            || size > MAX_BUFFER_BYTES
            || placement != 1
            || flags & !0xf != 0
            || !(1..=2).contains(&caching)
            || (vm != 0 && !self.vms.contains_key(&vm));
        if bad {
            return Err(Errno::Inval);
        }
        if self.buffers.len() >= MAX_BUFFERS {
            return Err(Errno::NoMem);
        }
        let buffer = be.buffer_new(size)?;
        let handle = self.id();
        self.buffers.insert(handle, (buffer, size, vm));
        put32(arg, 28, handle);
        Ok(())
    }

    /// `DRM_IOCTL_GEM_CLOSE`. Memory that is still bound stays until it is unbound.
    pub fn gem_close(&mut self, arg: &[u8]) -> Result<(), Errno> {
        let b = fields(arg, 8, 4, false)?;
        self.buffers
            .remove(&u32_at(b, 0))
            .map(|_| ())
            .ok_or(Errno::Inval)
    }

    /// `DRM_IOCTL_XE_GEM_MMAP_OFFSET`: the offset to pass to `mmap` on the node.
    pub fn gem_mmap_offset(&mut self, arg: &mut [u8]) -> Result<(), Errno> {
        let b = fields(arg, 40, 24, true)?;
        let (handle, flags) = (u32_at(b, 8), u32_at(b, 12));
        if flags != 0 {
            return Err(Errno::Inval);
        }
        if !self.buffers.contains_key(&handle) {
            return Err(Errno::NoEnt);
        }
        put64(arg, 16, (handle as u64) << 32);
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_CREATE`. Scratch-page and long-running mode are accepted; fault mode
    /// is refused.
    pub fn vm_create(&mut self, be: &mut B, arg: &mut [u8]) -> Result<(), Errno> {
        let b = fields(arg, 32, 16, true)?;
        if u32_at(b, 8) & !0b11 != 0 {
            return Err(Errno::Inval);
        }
        if self.vms.len() >= MAX_VMS {
            return Err(Errno::NoMem);
        }
        let backend = be.vm_new()?;
        let id = self.id();
        self.vms.insert(
            id,
            Vm {
                backend,
                binds: Vec::new(),
            },
        );
        put32(arg, 12, id);
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_DESTROY`. Refused while a queue runs in the address space.
    pub fn vm_destroy(&mut self, be: &mut B, arg: &[u8]) -> Result<(), Errno> {
        let b = fields(arg, 24, 4, false)?;
        let id = u32_at(b, 0);
        if self.queues.values().any(|q| q.vm == id) {
            return Err(Errno::Busy);
        }
        let vm = self.vms.remove(&id).ok_or(Errno::NoEnt)?;
        be.vm_free(vm.backend);
        Ok(())
    }

    /// Everything the request's syncs signal is done right now.
    fn finish_now(&mut self, syncs: &[Sync], user: &mut impl User) -> Result<(), Errno> {
        self.syncs_signal(syncs, Point::Signaled);
        for s in syncs {
            if let Sync::UserFence { addr, value } = s {
                if !user.write(*addr, &value.to_le_bytes()) {
                    return Err(Errno::Fault);
                }
            }
        }
        Ok(())
    }

    /// The property extensions of a queue: priority (up to normal) and time slice are taken.
    fn queue_extensions(user: &mut impl User, mut next: u64) -> Result<(), Errno> {
        for _ in 0..16 {
            if next == 0 {
                return Ok(());
            }
            let e = user.read(next, 48).ok_or(Errno::Fault)?;
            let (name, property, value) = (u32_at(&e, 8), u32_at(&e, 16), u64_at(&e, 24));
            let ok = name == 0 && matches!((property, value), (0, 0..=1) | (1, _));
            if !ok || u32_at(&e, 12) != 0 || u32_at(&e, 20) != 0 {
                return Err(Errno::Inval);
            }
            next = u64_at(&e, 0);
        }
        Err(Errno::Inval)
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_CREATE`: one engine, named by class and instance.
    pub fn queue_create(
        &mut self,
        be: &mut B,
        arg: &mut [u8],
        user: &mut impl User,
    ) -> Result<(), Errno> {
        let b = fields(arg, 48, 32, false)?;
        let (width, placements, vm_id, flags) =
            (u16_at(b, 8), u16_at(b, 10), u32_at(b, 12), u32_at(b, 16));
        if width != 1 || placements != 1 || flags & !1 != 0 {
            return Err(Errno::Inval);
        }
        Self::queue_extensions(user, u64_at(b, 0))?;
        let inst = user.read(u64_at(b, 24), 8).ok_or(Errno::Fault)?;
        let class = match u16_at(&inst, 0) {
            0 => EngineClass::Render,
            1 => EngineClass::Copy,
            4 => EngineClass::Compute,
            _ => return Err(Errno::Inval),
        };
        // instance, GT and padding: there is one engine of each class on GT 0
        if inst[2..].iter().any(|x| *x != 0) {
            return Err(Errno::Inval);
        }
        let vm = self.vms.get(&vm_id).ok_or(Errno::NoEnt)?.backend;
        if self.queues.len() >= MAX_QUEUES {
            return Err(Errno::NoMem);
        }
        let backend = be.queue_new(vm, class)?;
        let id = self.id();
        self.queues.insert(id, Queue { backend, vm: vm_id });
        put32(arg, 20, id);
        Ok(())
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_DESTROY`. Fences of its jobs are stored as if they finished.
    pub fn queue_destroy(&mut self, be: &mut B, arg: &[u8]) -> Result<(), Errno> {
        let b = fields(arg, 24, 4, false)?;
        let q = self.queues.remove(&u32_at(b, 0)).ok_or(Errno::NoEnt)?;
        be.queue_free(q.backend);
        Ok(())
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY`: whether the queue was stopped because a job hung.
    pub fn queue_get_property(&mut self, be: &mut B, arg: &mut [u8]) -> Result<(), Errno> {
        let b = fields(arg, 40, 24, true)?;
        if u32_at(b, 12) != 0 {
            return Err(Errno::Inval);
        }
        let q = self.queues.get(&u32_at(b, 8)).ok_or(Errno::NoEnt)?;
        put64(arg, 16, be.queue_banned(q.backend) as u64);
        Ok(())
    }

    /// `DRM_IOCTL_XE_EXEC`: runs one batch; its user fences are stored when it finished.
    /// Without a batch only the fences are stored.
    pub fn exec(&mut self, be: &mut B, arg: &[u8], user: &mut impl User) -> Result<(), Errno> {
        let b = fields(arg, 56, 34, true)?;
        let (id, batches) = (u32_at(b, 8), u16_at(b, 32));
        let q = self.queues.get(&id).ok_or(Errno::NoEnt)?;
        if batches > 1 {
            return Err(Errno::Inval);
        }
        if be.queue_banned(q.backend) {
            return Err(Errno::Canceled);
        }
        let (backend, batch) = (q.backend, u64_at(b, 24));
        let syncs = sync::read(user, u64_at(b, 16), u32_at(b, 12))?;
        self.syncs_ready(be, &syncs)?;
        if self.fences.len() + syncs.len() > MAX_FENCES {
            return Err(Errno::Again);
        }
        if batches == 0 {
            return self.finish_now(&syncs, user);
        }
        let seqno = be.queue_run(backend, batch)?;
        self.syncs_signal(&syncs, Point::Job { queue: id, seqno });
        for s in &syncs {
            if let Sync::UserFence { addr, value } = s {
                self.fences.push(Fence {
                    queue: id,
                    seqno,
                    addr: *addr,
                    value: *value,
                });
            }
        }
        Ok(())
    }

    /// `DRM_IOCTL_XE_WAIT_USER_FENCE`: compares the value at an address of the program with a
    /// wanted one. [`Errno::Again`]: not yet – the caller lets the program sleep a moment and
    /// asks again; [`Errno::Time`]: the time is over.
    pub fn wait_user_fence(
        &mut self,
        be: &mut B,
        arg: &[u8],
        user: &mut impl User,
    ) -> Result<(), Errno> {
        let b = fields(arg, 72, 56, true)?;
        let (addr, op, flags) = (u64_at(b, 8), u16_at(b, 16), u16_at(b, 18));
        let (value, mask, timeout) = (u64_at(b, 24), u64_at(b, 32), u64_at(b, 40) as i64);
        if op > 5 || flags & !1 != 0 || u32_at(b, 20) != 0 || u32_at(b, 52) != 0 || addr % 8 != 0 {
            return Err(Errno::Inval);
        }
        if timeout < 0 {
            return Err(Errno::Inval);
        }
        let now = be.now_ns();
        let deadline = match (self.waiting, flags & 1 != 0) {
            (_, true) => timeout as u64,
            (Some((a, v, end)), false) if a == addr && v == value => end,
            _ => now.saturating_add(timeout as u64),
        };
        let seen = user.read(addr, 8).ok_or(Errno::Fault)?;
        let (have, want) = (u64_at(&seen, 0) & mask, value & mask);
        let passed = match op {
            0 => have == want,
            1 => have != want,
            2 => have > want,
            3 => have >= want,
            4 => have < want,
            _ => have <= want,
        };
        if passed || now >= deadline {
            self.waiting = None;
            return if passed { Ok(()) } else { Err(Errno::Time) };
        }
        self.waiting = Some((addr, value, deadline));
        Err(Errno::Again)
    }

    /// Ends everything the file owns (the node was closed).
    pub fn close(&mut self, be: &mut B) {
        for (_, q) in core::mem::take(&mut self.queues) {
            be.queue_free(q.backend);
        }
        for (_, vm) in core::mem::take(&mut self.vms) {
            be.vm_free(vm.backend);
        }
        self.buffers.clear();
        self.syncs.clear();
        self.fences.clear();
    }
}

mod bind;
#[cfg(test)]
pub(crate) mod tests;
