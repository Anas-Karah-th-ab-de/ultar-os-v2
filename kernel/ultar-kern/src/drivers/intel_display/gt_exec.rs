//! The GPU for programs: the hardware side of the requests on the render node (stages G4c to
//! G4e). Buffers are kernel memory objects, address spaces are page tables of their own, a
//! queue is a context on one engine that stays registered with the GuC and gets one job per
//! exec: "run the batch at this address, then store the job's number". Logic of the requests:
//! `ultar_intel_gt::uapi::file`; context, commands and messages: `lrc`, `mi`, `submit`.
//!
//! Limits: jobs are found finished by looking (no interrupt), a hung batch is not detected
//! (the GuC's reset notice is not handled), and waits for the GuC keep the kernel busy.

use super::device::Device;
use super::gt_mem::GgttBuf;
use super::gt_run::{Runtime, RUNTIME};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::MutexGuard;
use ultar_intel_gt::ads::Class;
use ultar_intel_gt::lrc::{self, Context};
use ultar_intel_gt::mi;
use ultar_intel_gt::ppgtt::Ppgtt;
use ultar_intel_gt::submit::{self, Policy, TlbScope};
use ultar_intel_gt::uapi::file::Backend;
use ultar_intel_gt::uapi::{EngineClass, Errno};

const RING_BYTES: u32 = 16384;
/// Bytes of one job in a ring.
const JOB_BYTES: u32 = 32;
/// Jobs that may wait in one ring (the ring holds 512).
const MAX_PENDING: u64 = 400;
const STEP_TIMEOUT_US: u32 = 500_000;

/// A context of a program on one engine.
struct Queue {
    /// Context id at the GuC.
    id: u32,
    ring: GgttBuf,
    image: GgttBuf,
    /// The page the engine stores finished job numbers in.
    seqno: GgttBuf,
    /// Where the next job goes in the ring.
    tail: u32,
    /// Number of the last job handed over.
    submitted: u64,
}

/// What programs created.
#[derive(Default)]
pub(super) struct Programs {
    vms: BTreeMap<u32, Ppgtt>,
    queues: BTreeMap<u32, Queue>,
    next: u32,
    tlb_seqno: u32,
}

/// The GPU in operation, held for one request.
pub struct Gpu {
    dev: MutexGuard<'static, Option<Device>>,
    rt: MutexGuard<'static, Option<Runtime>>,
}

impl Gpu {
    /// Takes the device and the GT; `None` while the GT is not in operation.
    pub fn lock() -> Option<Gpu> {
        let dev = super::DEVICE.lock();
        let rt = RUNTIME.lock();
        (dev.is_some() && rt.is_some()).then_some(Gpu { dev, rt })
    }

    fn parts(&mut self) -> Result<(&mut Device, &mut Runtime), Errno> {
        match (self.dev.as_mut(), self.rt.as_mut()) {
            (Some(d), Some(r)) => Ok((d, r)),
            _ => Err(Errno::NoDev),
        }
    }
}

fn log(what: &str, e: impl core::fmt::Display) -> Errno {
    crate::kprintln!("[intel-gt] {what}: {e}");
    Errno::NoDev
}

/// Makes the GuC and the engines forget cached address translations.
fn invalidate(dev: &mut Device, rt: &mut Runtime) -> Result<(), Errno> {
    for scope in [TlbScope::Guc, TlbScope::Full] {
        rt.programs.tlb_seqno = rt.programs.tlb_seqno.wrapping_add(1).max(1);
        let seqno = 0x4000_0000 | rt.programs.tlb_seqno;
        let hw = &mut dev.mmio;
        rt.ct
            .post(hw, &mut rt.words, &submit::tlb_invalidate(seqno, scope))
            .map_err(|e| log("TLB invalidation", e))?;
        let event = submit::EVENT_TLB_INVALIDATION_DONE;
        rt.ct
            .wait_event(hw, &mut rt.words, event, STEP_TIMEOUT_US)
            .map_err(|e| log("TLB invalidation not confirmed", e))?;
    }
    Ok(())
}

fn engine(class: EngineClass) -> Option<(Class, u32)> {
    match class {
        EngineClass::Render => Some((Class::Render, 0x2000)),
        EngineClass::Copy => Some((Class::Blitter, 0x22000)),
        EngineClass::Compute => Some((Class::Compute, 0x1a000)),
        _ => None,
    }
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Ring, context image (from the class's golden image) and job number page of a new queue.
fn build_queue(
    dev: &mut Device,
    rt: &mut Runtime,
    root: u64,
    class: Class,
    base: u32,
) -> Result<(Queue, u64), String> {
    let golden = rt.goldens.iter().find(|(c, _)| *c == class);
    let golden = golden.map(|(_, g)| g.clone()).ok_or("no golden image")?;
    let mut ring = GgttBuf::new(dev, RING_BYTES as u64)?;
    // the first slot: arbitration on, the rest of the slot empty
    let mut first = alloc::vec![mi::NOOP; JOB_BYTES as usize / 4];
    first[0] = mi::ARB_ON;
    ring.fill(&words_to_bytes(&first));
    let ctx = Context {
        class,
        mmio_base: base,
        ring_ggtt: ring.gtt,
        ring_bytes: RING_BYTES,
        tail: JOB_BYTES,
        pml4: Some(root),
    };
    let made = ctx.image_from(&golden).map_err(|e| format!("{e}"));
    let image = made.and_then(|bytes| {
        let mut buf = GgttBuf::new(dev, bytes.len() as u64)?;
        buf.fill(&bytes);
        Ok(buf)
    });
    let pair = image.and_then(|image| match GgttBuf::new(dev, 4096) {
        Ok(seqno) => Ok((image, seqno)),
        Err(e) => {
            image.release(dev);
            Err(e)
        }
    });
    let (image, seqno) = match pair {
        Ok(p) => p,
        Err(e) => {
            ring.release(dev);
            return Err(e);
        }
    };
    let descriptor = ctx.descriptor(image.gtt);
    let id = rt.next_id;
    rt.next_id += 1;
    let queue = Queue {
        id,
        ring,
        image,
        seqno,
        tail: JOB_BYTES,
        submitted: 0,
    };
    Ok((queue, descriptor))
}

/// Registers the context with the GuC and enables it.
fn register(
    dev: &mut Device,
    rt: &mut Runtime,
    q: &Queue,
    class: Class,
    desc: u64,
) -> Result<(), String> {
    let (hw, words) = (&mut dev.mmio, &mut rt.words);
    rt.ct
        .post(hw, words, &submit::register(q.id, class, 1, desc))
        .map_err(|e| format!("register: {e}"))?;
    rt.ct
        .post(hw, words, &submit::policies(q.id, Policy::DEFAULT))
        .map_err(|e| format!("policies: {e}"))?;
    rt.ct
        .post(hw, words, &submit::schedule(q.id, true))
        .map_err(|e| format!("enable: {e}"))?;
    let event = submit::EVENT_SCHED_CONTEXT_MODE_DONE;
    rt.ct
        .wait_event(hw, words, event, STEP_TIMEOUT_US)
        .map_err(|e| format!("enable not confirmed: {e}"))?;
    Ok(())
}

/// Disables and deregisters the context and gives its buffers back.
fn retire(dev: &mut Device, rt: &mut Runtime, q: Queue) {
    let (hw, words) = (&mut dev.mmio, &mut rt.words);
    for (action, event) in [
        (
            &submit::schedule(q.id, false)[..],
            submit::EVENT_SCHED_CONTEXT_MODE_DONE,
        ),
        (
            &submit::deregister(q.id)[..],
            submit::EVENT_DEREGISTER_CONTEXT_DONE,
        ),
    ] {
        let sent = rt.ct.post(hw, words, action).map(|_| ());
        let done = sent.and_then(|_| {
            rt.ct
                .wait_event(hw, words, event, STEP_TIMEOUT_US)
                .map(|_| ())
        });
        if let Err(e) = done {
            // the GuC may still use the buffers: they are kept rather than freed under it
            crate::kprintln!(
                "[intel-gt] context {} not retired ({e}); its buffers are kept",
                q.id
            );
            core::mem::forget(q);
            return;
        }
    }
    for b in [q.ring, q.image, q.seqno] {
        b.release(dev);
    }
    let _ = invalidate(dev, rt);
}

/// Number of the last finished job of `q`.
fn done(q: &Queue) -> u64 {
    let b = q.seqno.bytes(4);
    let seen = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    q.submitted - (q.submitted as u32).wrapping_sub(seen) as u64
}

impl Backend for Gpu {
    type Buffer = Arc<crate::memfd::MemObj>;

    fn buffer_new(&mut self, size: u64) -> Result<Self::Buffer, Errno> {
        let obj = crate::memfd::MemObj::new(false);
        obj.set_size(size).map_err(|_| Errno::NoMem)?;
        Ok(Arc::new(obj))
    }

    fn buffer_pages(&self, b: &Self::Buffer) -> Vec<u64> {
        b.frames(0, b.size()).unwrap_or_default()
    }

    fn vm_new(&mut self) -> Result<u32, Errno> {
        let (_, rt) = self.parts()?;
        let space = Ppgtt::new(&mut rt.mem.tables).map_err(|_| Errno::NoMem)?;
        rt.programs.next += 1;
        rt.programs.vms.insert(rt.programs.next, space);
        Ok(rt.programs.next)
    }

    fn vm_map(&mut self, vm: u32, va: u64, pages: &[u64]) -> Result<(), Errno> {
        let (_, rt) = self.parts()?;
        let space = rt.programs.vms.get_mut(&vm).ok_or(Errno::NoEnt)?;
        space
            .map(&mut rt.mem.tables, va, pages)
            .map_err(|_| Errno::NoMem)
    }

    fn vm_unmap(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno> {
        let (dev, rt) = self.parts()?;
        let space = rt.programs.vms.get_mut(&vm).ok_or(Errno::NoEnt)?;
        space
            .unmap(&mut rt.mem.tables, va, pages)
            .map_err(|_| Errno::Inval)?;
        invalidate(dev, rt)
    }

    fn vm_free(&mut self, vm: u32) {
        let Ok((dev, rt)) = self.parts() else { return };
        if let Some(space) = rt.programs.vms.remove(&vm) {
            // nothing may still translate through these tables when their pages are reused
            let _ = invalidate(dev, rt);
            rt.mem.tables.free(space.table_pages());
        }
    }

    fn queue_new(&mut self, vm: u32, class: EngineClass) -> Result<u32, Errno> {
        let (dev, rt) = self.parts()?;
        let (class, base) = engine(class).ok_or(Errno::Inval)?;
        let root = rt.programs.vms.get(&vm).ok_or(Errno::NoEnt)?.root();
        let (queue, desc) = build_queue(dev, rt, root, class, base).map_err(|e| log("queue", e))?;
        invalidate(dev, rt)?;
        if let Err(e) = register(dev, rt, &queue, class, desc) {
            crate::kprintln!("[intel-gt] queue: {e}");
            // the GuC may know the context: keep its buffers
            core::mem::forget(queue);
            return Err(Errno::NoDev);
        }
        rt.programs.next += 1;
        rt.programs.queues.insert(rt.programs.next, queue);
        Ok(rt.programs.next)
    }

    fn queue_run(&mut self, queue: u32, batch_va: u64) -> Result<u64, Errno> {
        let (dev, rt) = self.parts()?;
        let q = rt.programs.queues.get_mut(&queue).ok_or(Errno::NoEnt)?;
        if q.submitted - done(q) >= MAX_PENDING {
            return Err(Errno::Again);
        }
        let seqno = q.submitted + 1;
        let mut job = mi::batch_start(batch_va, false).to_vec();
        job.extend_from_slice(&mi::store_imm_ggtt(q.seqno.gtt, seqno as u32));
        job.push(mi::ARB_CHECK);
        let tail = (q.tail + JOB_BYTES) % RING_BYTES;
        let tail_at = lrc::PPHWSP_SIZE + lrc::CTX_RING_TAIL * 4;
        let written = q.ring.write_at(q.tail as usize, &words_to_bytes(&job))
            && q.image.write_at(tail_at, &tail.to_le_bytes());
        if !written {
            return Err(Errno::NoMem);
        }
        rt.ct
            .post(&mut dev.mmio, &mut rt.words, &submit::submit(q.id))
            .map_err(|e| log("submit", e))?;
        q.tail = tail;
        q.submitted = seqno;
        Ok(seqno)
    }

    fn queue_done(&mut self, queue: u32) -> u64 {
        let found = self
            .rt
            .as_ref()
            .and_then(|rt| rt.programs.queues.get(&queue));
        found.map(done).unwrap_or(u64::MAX)
    }

    fn queue_free(&mut self, queue: u32) {
        let Ok((dev, rt)) = self.parts() else { return };
        if let Some(q) = rt.programs.queues.remove(&queue) {
            retire(dev, rt, q);
        }
    }

    fn now_ns(&mut self) -> u64 {
        crate::arch::uptime_ns()
    }
}
