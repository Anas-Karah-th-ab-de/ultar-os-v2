//! Stage G3 of the GT plan: jobs on the engines. A context is built (ring, context image, for
//! most jobs its own page tables and a batch), registered with the running GuC and enabled –
//! and the engine must then have written a value into memory.
//!
//! 1. Copy engine, store command in the ring (GGTT only); then a second job on the same,
//!    still enabled context.
//! 2. Copy, render and compute engine with a batch in the context's own address space. What
//!    each engine saved when the context left is the class's golden image: it is copied into
//!    the GuC's ADS block.
//! 3. Each class again with a context that starts from its golden image.
//!
//! Logic: `ultar-intel-gt` (`lrc`, `mi`, `submit`, `engines`, `ppgtt`, `ads`); this module adds
//! the buffers.

use super::device::Device;
use super::gt::buffer;
use super::gt_mem::{GgttBuf, TableMem, Words};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ultar_intel_gt::ads::{self, Class};
use ultar_intel_gt::ctb::Ct;
use ultar_intel_gt::engines::{self, RingState};
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::lrc::{self, Context};
use ultar_intel_gt::ppgtt::Ppgtt;
use ultar_intel_gt::submit::{self, Policy};
use ultar_intel_gt::{mi, Hw};

/// How a job's context is built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Store command in the ring, GGTT only, never-run image; followed by a second job.
    Ggtt,
    /// Batch in the context's own address space, never-run image; records the golden image.
    Batch,
    /// Batch in the context's own address space, image from the class's golden image.
    FromGolden,
}

/// One job of the test.
struct Spec {
    /// Name of the check in the report.
    what: &'static str,
    class: Class,
    /// Register base of the engine.
    base: u32,
    kind: Kind,
}

const fn spec(what: &'static str, class: Class, base: u32, kind: Kind) -> Spec {
    Spec {
        what,
        class,
        base,
        kind,
    }
}

const BCS: u32 = 0x22000;
const RCS: u32 = 0x2000;
const CCS: u32 = 0x1a000;

const JOBS: &[Spec] = &[
    spec(
        "two jobs on the copy engine (GGTT)",
        Class::Blitter,
        BCS,
        Kind::Ggtt,
    ),
    spec(
        "batch on the copy engine (own page tables)",
        Class::Blitter,
        BCS,
        Kind::Batch,
    ),
    spec(
        "batch on the render engine (own page tables)",
        Class::Render,
        RCS,
        Kind::Batch,
    ),
    spec(
        "batch on the compute engine (own page tables)",
        Class::Compute,
        CCS,
        Kind::Batch,
    ),
    spec(
        "copy engine context from the golden image",
        Class::Blitter,
        BCS,
        Kind::FromGolden,
    ),
    spec(
        "render engine context from the golden image",
        Class::Render,
        RCS,
        Kind::FromGolden,
    ),
    spec(
        "compute engine context from the golden image",
        Class::Compute,
        CCS,
        Kind::FromGolden,
    ),
];

/// What the jobs store (plus the job's number).
const VALUE: u32 = 0xc0de_6000;
/// What the second job of a context stores (plus the job's number).
const SECOND_VALUE: u32 = 0x5ec0_0000;
const RING_BYTES: u32 = 16384;
/// Where batch and target live in a context's address space.
const BATCH_VA: u64 = 0x0010_0000;
const TARGET_VA: u64 = 0x1_2340_0000;
/// How long the GuC and the engine get for each step.
const STEP_TIMEOUT_US: u32 = 500_000;

/// What the jobs work with: the device, the caller's buffers and page table memory (both
/// released after the GuC is reset), the command buffers and the golden images recorded so far.
pub struct Env<'a> {
    /// The device.
    pub dev: &'a mut Device,
    /// Every buffer the GuC or an engine may still read.
    pub bufs: &'a mut Vec<GgttBuf>,
    /// The command buffers' host side.
    pub ct: &'a mut Ct,
    /// The command buffers' memory.
    pub mem: &'a mut Words,
    /// Memory of the contexts' page tables.
    pub tables: &'a mut TableMem,
    /// Index of the ADS block in `bufs`.
    pub ads: usize,
    /// Golden images by class.
    pub goldens: Vec<(Class, Vec<u8>)>,
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn word_at(buf: &GgttBuf, index: usize) -> u32 {
    let b = buf.bytes(index * 4 + 4);
    let at = index * 4;
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl Env<'_> {
    /// A buffer of at least `min_bytes` that starts with `data`; returns its index in `bufs`.
    fn filled(&mut self, data: &[u8], min_bytes: u32) -> Result<usize, String> {
        buffer(
            self.dev,
            self.bufs,
            (data.len() as u64).max(min_bytes as u64),
        )?;
        let at = self.bufs.len() - 1;
        self.bufs[at].fill(data);
        Ok(at)
    }

    fn golden(&self, class: Class) -> Option<&[u8]> {
        let found = self.goldens.iter().find(|(c, _)| *c == class);
        found.map(|(_, image)| image.as_slice())
    }

    /// Waits until word `index` of buffer `target` is `value`.
    fn wait_value(&mut self, job: &Job, index: usize, value: u32) -> Result<(), String> {
        let hw = &mut self.dev.mmio;
        let start = hw.now_us();
        while word_at(&self.bufs[job.target], index) != value {
            if hw.now_us().saturating_sub(start) >= STEP_TIMEOUT_US as u64 {
                let image = self.bufs[job.image].bytes(2 * 4096);
                return Err(format!(
                    "value not written (read {:#x}); engine: {}; saved head {:#x?} tail {:#x?}",
                    word_at(&self.bufs[job.target], index),
                    RingState::read(hw, job.ctx.mmio_base),
                    lrc::state_word(&image, lrc::CTX_RING_HEAD),
                    lrc::state_word(&image, lrc::CTX_RING_TAIL),
                ));
            }
            hw.delay_us(50);
        }
        Ok(())
    }
}

/// The buffers of a test context: indexes into the buffer list.
struct Job {
    id: u32,
    value: u32,
    target: usize,
    ring: usize,
    image: usize,
    ctx: Context,
}

/// Target page and the ring contents that make the engine store `value` there: directly (GGTT)
/// or through a batch in the address space `space`.
fn ring_for(
    env: &mut Env,
    space: Option<&mut Ppgtt>,
    value: u32,
) -> Result<(usize, Vec<u32>), String> {
    let target_gtt = buffer(env.dev, env.bufs, 4096)?;
    let target = env.bufs.len() - 1;
    let Some(space) = space else {
        return Ok((target, mi::store_job(target_gtt, value)));
    };
    let batch = env.filled(&words_to_bytes(&mi::store_batch(TARGET_VA, value)), 0)?;
    for (va, index) in [(BATCH_VA, batch), (TARGET_VA, target)] {
        space
            .map(env.tables, va, &env.bufs[index].pages())
            .map_err(|e| format!("page tables: {e}"))?;
    }
    Ok((target, mi::batch_job(BATCH_VA)))
}

/// Engine setup, target, ring and the context image.
fn prepare(env: &mut Env, spec: &Spec, id: u32) -> Result<Job, String> {
    let status_page = buffer(env.dev, env.bufs, 4096)?;
    engines::setup(&mut env.dev.mmio, spec.base, status_page);
    if spec.class == Class::Compute {
        engines::enable_compute(&mut env.dev.mmio);
    }
    let value = VALUE + id;
    let mut space = match spec.kind {
        Kind::Ggtt => None,
        _ => Some(Ppgtt::new(env.tables).map_err(|e| format!("page tables: {e}"))?),
    };
    let (target, commands) = ring_for(env, space.as_mut(), value)?;
    let ring = env.filled(&words_to_bytes(&commands), RING_BYTES)?;
    let ctx = Context {
        class: spec.class,
        mmio_base: spec.base,
        ring_ggtt: env.bufs[ring].gtt,
        ring_bytes: RING_BYTES,
        tail: commands.len() as u32 * 4,
        pml4: space.as_ref().map(Ppgtt::root),
    };
    let image = match spec.kind {
        Kind::FromGolden => {
            let golden = env.golden(spec.class).ok_or("no golden image recorded")?;
            ctx.image_from(golden)
        }
        _ => ctx.image(),
    }
    .map_err(|e| format!("{e}"))?;
    let image = env.filled(&image, 0)?;
    Ok(Job {
        id,
        value,
        target,
        ring,
        image,
        ctx,
    })
}

/// Registers and enables the context and waits until the value is in memory. Returns the time
/// from the enable request to the value in µs.
fn run(env: &mut Env, job: &Job) -> Result<u64, String> {
    let (hw, mem) = (&mut env.dev.mmio, &mut *env.mem);
    let descriptor = job.ctx.descriptor(env.bufs[job.image].gtt);
    let register = submit::register(job.id, job.ctx.class, 1, descriptor);
    env.ct
        .post(hw, mem, &register)
        .map_err(|e| format!("register: {e}"))?;
    env.ct
        .post(hw, mem, &submit::policies(job.id, Policy::DEFAULT))
        .map_err(|e| format!("policies: {e}"))?;
    let t0 = crate::arch::uptime_ns();
    env.ct
        .post(hw, mem, &submit::schedule(job.id, true))
        .map_err(|e| format!("enable: {e}"))?;
    let event = submit::EVENT_SCHED_CONTEXT_MODE_DONE;
    let done = env
        .ct
        .wait_event(hw, mem, event, STEP_TIMEOUT_US)
        .map_err(|e| format!("enable not confirmed: {e}"))?;
    if done.first() != Some(&job.id) {
        return Err(format!("enable confirmed for another context: {done:x?}"));
    }
    env.wait_value(job, 0, job.value)?;
    Ok((crate::arch::uptime_ns() - t0) / 1000)
}

/// A second job on the enabled context: more commands behind the ring's tail, the new tail into
/// the context image, and the request to schedule the context. Returns the time to the value.
fn second(env: &mut Env, job: &Job) -> Result<u64, String> {
    let value = SECOND_VALUE + job.id;
    let commands = mi::store_job(env.bufs[job.target].gtt + 4, value);
    let tail = job.ctx.tail + commands.len() as u32 * 4;
    let tail_at = lrc::PPHWSP_SIZE + lrc::CTX_RING_TAIL * 4;
    let written = env.bufs[job.ring].write_at(job.ctx.tail as usize, &words_to_bytes(&commands))
        && env.bufs[job.image].write_at(tail_at, &tail.to_le_bytes());
    if !written {
        return Err("second job does not fit".into());
    }
    let t0 = crate::arch::uptime_ns();
    env.ct
        .post(&mut env.dev.mmio, env.mem, &submit::submit(job.id))
        .map_err(|e| format!("submit: {e}"))?;
    env.wait_value(job, 1, value)
        .map_err(|e| format!("second job: {e}"))?;
    Ok((crate::arch::uptime_ns() - t0) / 1000)
}

/// Disables and deregisters the context, so the GuC no longer uses its buffers.
fn retire(env: &mut Env, id: u32) -> Result<(), String> {
    let (hw, mem) = (&mut env.dev.mmio, &mut *env.mem);
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
        env.ct
            .post(hw, mem, action)
            .map_err(|e| format!("{what}: {e}"))?;
        env.ct
            .wait_event(hw, mem, event, STEP_TIMEOUT_US)
            .map_err(|e| format!("{what} not confirmed: {e}"))?;
    }
    Ok(())
}

/// Keeps what the engine saved when the context left as the class's golden image and copies it
/// into the ADS block, where the GuC takes it from when it resets an engine.
fn record_golden(env: &mut Env, class: Class, image: Vec<u8>) -> Result<String, String> {
    let (at, size) = ads::golden_slot(&ads::XE_LPG_ENGINES, class).ok_or("no golden slot")?;
    if image.len() != size || !env.bufs[env.ads].write_at(at, &image) {
        return Err(format!(
            "golden image of {} bytes does not fit",
            image.len()
        ));
    }
    let set = image[lrc::PPHWSP_SIZE..]
        .chunks_exact(4)
        .filter(|w| w.iter().any(|b| *b != 0));
    let text = format!("; golden image in the ADS ({} words set)", set.count());
    env.goldens.push((class, image));
    Ok(text)
}

/// One job from buffers to the retired context.
fn one(env: &mut Env, spec: &Spec, id: u32) -> Result<String, String> {
    let job = prepare(env, spec, id)?;
    let us = run(env, &job)?;
    let mut text = format!("stored {:#x} after {us} µs", job.value);
    if spec.kind == Kind::Ggtt {
        let us = second(env, &job)?;
        text += &format!(", second job on the enabled context after {us} µs");
    }
    retire(env, id)?;
    text += "; context retired";
    if spec.kind == Kind::Batch && env.golden(spec.class).is_none() {
        let image = env.bufs[job.image].bytes(spec.class.lrc_size() as usize);
        text += &record_golden(env, spec.class, image)?;
    }
    Ok(text)
}

/// Runs the test jobs through the GuC (the command buffers are on).
pub fn jobs(env: &mut Env) -> Vec<(&'static str, Result<String, String>)> {
    let mut fwk = Forcewake::new(0);
    if let Err(e) = fwk.get(&mut env.dev.mmio, Domain::Render) {
        return alloc::vec![(JOBS[0].what, Err(format!("render forcewake: {e}")))];
    }
    let mut out = Vec::new();
    for (id, spec) in JOBS.iter().enumerate() {
        let result = one(env, spec, id as u32);
        let failed = result.is_err();
        out.push((spec.what, result));
        if failed {
            // a context that could not be retired may still be in use: no further jobs
            break;
        }
    }
    if let Err(e) = fwk.put(&mut env.dev.mmio, Domain::Render) {
        crate::kprintln!("GTTEST: render forcewake release: {e}");
    }
    out
}
