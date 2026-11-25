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
use core::sync::atomic::{AtomicU64, Ordering};
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
const JOB_BYTES: u32 = 64;
/// Jobs that may wait in one ring (the ring holds 256).
const MAX_PENDING: u64 = 200;
/// 2 MiB entries the GT got for buffers (for the log).
pub static HUGE_ENTRIES: AtomicU64 = AtomicU64::new(0);
/// Pages bound per cache attribute the programs asked for (for the log).
pub static PAT_ASKED: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

/// A queue that ran at least this many jobs gets a line in the log when it ends.
const USAGE_LOG_JOBS: u64 = 50;
const STEP_TIMEOUT_US: u32 = 500_000;
/// How long the GuC gets to stop a context (it waits 640 ms for a hung one, then resets).
const STOP_TIMEOUT_US: u32 = 3_000_000;
/// How long one job may take before its queue is stopped (Linux: 5 s).
const JOB_TIMEOUT_NS: u64 = 5_000_000_000;

/// What a queue did, for the log when it ends: how many jobs, how long the engine worked for it
/// (from a job handed to an idle queue until the queue was seen idle again) and the range of
/// frequencies the GT was seen at while it worked.
#[derive(Default)]
struct Usage {
    busy_ns: u64,
    busy_from: u64,
    /// Looks at the frequency while busy: below 1000, below 1700, below 2200 MHz, above.
    mhz: [u64; 4],
    /// Of those looks, how many saw the GT held back by heat, by the power budget, by other
    /// limits.
    limited: [u64; 3],
}

impl Usage {
    fn working(&mut self, mhz: u32, reasons: u32) {
        use ultar_intel_gt::freq::{LIMIT_HEAT, LIMIT_OTHER, LIMIT_POWER};
        let bucket = [1000, 1700, 2200].iter().take_while(|b| mhz >= **b).count();
        self.mhz[bucket] += 1;
        for (i, bits) in [LIMIT_HEAT, LIMIT_POWER, LIMIT_OTHER].iter().enumerate() {
            self.limited[i] += u64::from(reasons & bits != 0);
        }
    }

    fn idle(&mut self, now: u64) {
        if self.busy_from != 0 {
            self.busy_ns += now - self.busy_from;
            self.busy_from = 0;
        }
    }
}

/// A context of a program on one engine.
struct Queue {
    usage: Usage,
    /// Engine class: how a job ends (flush and store).
    class: Class,
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
    /// Number of the last job seen finished, and when that or the hand-over of the job that
    /// waits longest was (ns): what the hang check measures from.
    seen: u64,
    since_ns: u64,
    /// Stopped because a job hung: the GuC no longer knows the context.
    banned: bool,
    /// The GuC did not confirm that it let go of the context: its buffers stay.
    kept: bool,
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
    rt.next_id += 1;
    let queue = Queue {
        id: rt.next_id - 1,
        ring,
        image,
        seqno,
        tail: JOB_BYTES,
        submitted: 0,
        class,
        usage: Usage::default(),
        seen: 0,
        since_ns: 0,
        banned: false,
        kept: false,
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

/// Takes the context away from the GuC: disable, then deregister. A hung job makes the first
/// step take as long as the GuC gives the context to stop (then it resets the engine).
fn unregister(dev: &mut Device, rt: &mut Runtime, id: u32) -> Result<(), String> {
    let (hw, words) = (&mut dev.mmio, &mut rt.words);
    for (what, action, event) in [
        (
            "disable",
            &submit::schedule(id, false)[..],
            submit::EVENT_SCHED_CONTEXT_MODE_DONE,
        ),
        (
            "deregister",
            &submit::deregister(id)[..],
            submit::EVENT_DEREGISTER_CONTEXT_DONE,
        ),
    ] {
        rt.ct
            .post(hw, words, action)
            .map_err(|e| format!("{what}: {e}"))?;
        rt.ct
            .wait_event(hw, words, event, STOP_TIMEOUT_US)
            .map_err(|e| format!("{what} not confirmed: {e}"))?;
    }
    Ok(())
}

/// Prints the commands of a hung batch around `va` (where the engine stands): 96 words before,
/// 8 after, looked up in the programs' address spaces. To read which dispatch did not end.
fn dump_batch(rt: &mut Runtime, va: u64) {
    let tables = &mut rt.mem.tables;
    let start = va.saturating_sub(96 * 4);
    for line in 0..13u64 {
        let at = start + line * 32;
        let mut text = alloc::string::String::new();
        for k in 0..8u64 {
            let a = at + k * 4;
            let phys = rt.programs.vms.values_mut().find_map(|vm| vm.lookup(tables, a & !0xfff));
            match phys {
                // SAFETY: memory bound to the GPU is mapped one to one in the kernel.
                Some(p) => {
                    let w = unsafe { core::ptr::read_volatile((p + (a & 0xfff)) as *const u32) };
                    text.push_str(&alloc::format!(" {w:08x}"));
                }
                None => text.push_str(" --------"),
            }
        }
        let mark = if (at..at + 32).contains(&va) { " <" } else { "" };
        crate::kprintln!("[intel-gt]   {at:#x}:{text}{mark}");
    }
}

/// A job of queue `key` did not finish in [`JOB_TIMEOUT_NS`]: the queue is stopped. The GuC
/// resets the engine when the context does not give way; the queue takes no more jobs.
fn ban(dev: &mut Device, rt: &mut Runtime, key: u32) {
    let Some((id, class, seen, submitted)) = rt.programs.queues.get(&key).map(|q| (q.id, q.class, q.seen, q.submitted)) else {
        return;
    };
    // where the engine stands, read before the GuC resets it (Linux: RING_ACTHD, RING_IPEHR,
    // RING_BBADDR); offsets from the engine's base
    let base = match class {
        Class::Render => 0x2000,
        Class::Blitter => 0x22000,
        _ => 0x1a000,
    };
    let mut reg = |off: u32| ultar_intel_gt::Hw::read(&mut dev.mmio, base + off);
    let (acthd, ipehr, bbaddr, bbaddr_hi) = (reg(0x74), reg(0x68), reg(0x140), reg(0x168));
    crate::kprintln!(
        "[intel-gt] hung job on {class:?}: jobs {seen}/{submitted} done, ACTHD {acthd:#x}, IPEHR {ipehr:#x}, batch {bbaddr_hi:#x}{bbaddr:08x}"
    );
    dump_batch(rt, (bbaddr_hi as u64) << 32 | (acthd & !3) as u64);
    let result = unregister(dev, rt, id);
    let events = rt.ct.take_backlog();
    let reset = events
        .iter()
        .any(|(a, p)| *a == submit::EVENT_CONTEXT_RESET && p.first() == Some(&id));
    crate::kprintln!(
        "[intel-gt] context {id} stopped: a job hung{}{}",
        if reset {
            ", the GuC reset its engine"
        } else {
            ""
        },
        match &result {
            Ok(()) => String::new(),
            Err(e) => format!("; {e}"),
        }
    );
    if let Some(q) = rt.programs.queues.get_mut(&key) {
        q.banned = true;
        // without the GuC's confirmation the buffers are never given back
        q.kept = result.is_err();
    }
}

/// Gives the buffers of a queue back (and takes it from the GuC first if it still knows it).
fn retire(dev: &mut Device, rt: &mut Runtime, q: Queue) {
    let mut kept = q.kept;
    if !q.banned {
        if let Err(e) = unregister(dev, rt, q.id) {
            crate::kprintln!("[intel-gt] context {} not retired ({e})", q.id);
            kept = true;
        }
    }
    if kept {
        // the GuC may still use the buffers: they are kept rather than freed under it
        core::mem::forget(q);
        return;
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

    fn buffer_new(&mut self, size: u64, write_combining: bool) -> Result<Self::Buffer, Errno> {
        let obj = crate::memfd::MemObj::new_gpu(write_combining);
        obj.set_size(size).map_err(|_| Errno::NoMem)?;
        Ok(Arc::new(obj))
    }

    fn buffer_pages(&self, b: &Self::Buffer) -> Vec<u64> {
        b.frames(0, b.size()).unwrap_or_default()
    }

    fn vm_new(&mut self, scratch: bool) -> Result<u32, Errno> {
        let (_, rt) = self.parts()?;
        let tables = &mut rt.mem.tables;
        let space = if scratch { Ppgtt::new_scratch(tables) } else { Ppgtt::new(tables) };
        let space = space.map_err(|_| Errno::NoMem)?;
        rt.programs.next += 1;
        rt.programs.vms.insert(rt.programs.next, space);
        Ok(rt.programs.next)
    }

    fn vm_map(&mut self, vm: u32, va: u64, pages: &[u64], pat: u8) -> Result<(), Errno> {
        let (dev, rt) = self.parts()?;
        let space = rt.programs.vms.get_mut(&vm).ok_or(Errno::NoEnt)?;
        if let Some(n) = PAT_ASKED.get(pat as usize) {
            n.fetch_add(pages.len() as u64, Ordering::Relaxed);
        }
        // what the program asked for (the logic refused attributes that do not fit how the CPU
        // maps the buffer)
        let huge = space
            .map_cached(&mut rt.mem.tables, va, pages, pat)
            .map_err(|_| Errno::NoMem)?;
        HUGE_ENTRIES.fetch_add(huge as u64, Ordering::Relaxed);
        // in a scratch space the engines may hold the translation to nothing that was there
        // before (Linux invalidates after every bind of such a space)
        if space.is_scratch() {
            invalidate(dev, rt)?;
        }
        Ok(())
    }

    fn vm_map_null(&mut self, vm: u32, va: u64, pages: u64) -> Result<(), Errno> {
        let (dev, rt) = self.parts()?;
        let space = rt.programs.vms.get_mut(&vm).ok_or(Errno::NoEnt)?;
        space
            .map_null(&mut rt.mem.tables, va, pages)
            .map_err(|_| Errno::NoMem)?;
        if space.is_scratch() {
            invalidate(dev, rt)?;
        }
        Ok(())
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
        super::gt_slpc::boost(dev, rt, true);
        Ok(rt.programs.next)
    }

    fn queue_run(&mut self, queue: u32, batch_va: u64) -> Result<u64, Errno> {
        let (dev, rt) = self.parts()?;
        let q = rt.programs.queues.get_mut(&queue).ok_or(Errno::NoEnt)?;
        if q.banned {
            return Err(Errno::Canceled);
        }
        let finished = done(q);
        if q.submitted - finished >= MAX_PENDING {
            return Err(Errno::Again);
        }
        if q.submitted == finished {
            // the queue was idle: the hang check measures this job from now
            q.since_ns = crate::arch::uptime_ns();
            q.usage.busy_from = q.since_ns;
        }
        let seqno = q.submitted + 1;
        // the job's number is stored once its writes reached memory: a program that sees the
        // number sees the data
        let mut job = mi::batch_start(batch_va, false).to_vec();
        match q.class {
            Class::Blitter => job.extend_from_slice(&mi::flush_store_ggtt_copy(q.seqno.gtt, seqno as u32)),
            class => job.extend_from_slice(&mi::flush_store_ggtt_render(q.seqno.gtt, seqno as u32, class == Class::Compute)),
        }
        job.push(mi::USER_INTERRUPT);
        job.push(mi::ARB_CHECK);
        job.resize(JOB_BYTES as usize / 4, mi::NOOP);
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
        let Ok((dev, rt)) = self.parts() else {
            return u64::MAX;
        };
        let Some(q) = rt.programs.queues.get_mut(&queue) else {
            return u64::MAX;
        };
        if q.banned {
            return u64::MAX;
        }
        let (finished, now) = (done(q), crate::arch::uptime_ns());
        if finished != q.seen {
            (q.seen, q.since_ns) = (finished, now);
        }
        if finished == q.submitted {
            q.usage.idle(now);
        } else {
            let mhz = ultar_intel_gt::freq::current(&mut dev.mmio).mhz();
            q.usage.working(mhz, ultar_intel_gt::freq::limit_reasons(&mut dev.mmio));
        }
        if finished < q.submitted && now.saturating_sub(q.since_ns) > JOB_TIMEOUT_NS {
            ban(dev, rt, queue);
            return u64::MAX;
        }
        finished
    }

    fn queue_banned(&mut self, queue: u32) -> bool {
        let queues = self.rt.as_ref().map(|rt| &rt.programs.queues);
        queues.and_then(|q| q.get(&queue)).is_some_and(|q| q.banned)
    }

    fn queue_free(&mut self, queue: u32) {
        let Ok((dev, rt)) = self.parts() else { return };
        if let Some(q) = rt.programs.queues.remove(&queue) {
            if q.submitted >= USAGE_LOG_JOBS {
                let m = q.usage.mhz;
                let asked: Vec<u64> = PAT_ASKED.iter().map(|n| n.load(Ordering::Relaxed)).collect();
                crate::kprintln!(
                    "[intel-gt] queue ends: {} jobs, engine busy {} ms, GT frequency while busy <1000/<1700/<2200/more MHz: {}/{}/{}/{} looks, held back by heat/power/other {:?}; GPU interrupts taken/engine/GuC/lost {:?}; pages bound per cache attribute asked 0..4: {asked:?}; 2 MiB entries {}",
                    q.submitted,
                    q.usage.busy_ns / 1_000_000,
                    m[0], m[1], m[2], m[3],
                    q.usage.limited,
                    super::gt_irq::counts(),
                    HUGE_ENTRIES.load(Ordering::Relaxed)
                );
            }
            retire(dev, rt, q);
            if rt.programs.queues.is_empty() {
                super::gt_slpc::boost(dev, rt, false);
            }
        }
    }

    fn now_ns(&mut self) -> u64 {
        crate::arch::uptime_ns()
    }
}
