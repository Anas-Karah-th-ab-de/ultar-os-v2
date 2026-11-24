//! Stage G3 of the GT plan: jobs on the engines. A context is built (ring, context image, for
//! most jobs its own page tables and a batch), registered with the running GuC and enabled –
//! and the engine must then have written a value into memory. First the copy engine with a
//! store command in the ring (GGTT only), then copy, render and compute engine with a batch in
//! the context's own address space. Logic: `ultar-intel-gt` (`lrc`, `mi`, `submit`, `engines`,
//! `ppgtt`); this module adds the buffers.

use super::device::Device;
use super::gt::buffer;
use super::gt_mem::{GgttBuf, TableMem, Words};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ultar_intel_gt::ads::Class;
use ultar_intel_gt::ctb::Ct;
use ultar_intel_gt::engines::{self, RingState};
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::lrc::{self, Context};
use ultar_intel_gt::ppgtt::Ppgtt;
use ultar_intel_gt::submit::{self, Policy};
use ultar_intel_gt::{mi, Hw};

/// One job of the test.
struct Spec {
    /// Name of the check in the report.
    what: &'static str,
    class: Class,
    /// Register base of the engine.
    base: u32,
    /// Whether the context gets its own address space and runs a batch in it.
    vm: bool,
}

const JOBS: &[Spec] = &[
    Spec {
        what: "first job on the copy engine (GGTT)",
        class: Class::Blitter,
        base: 0x22000,
        vm: false,
    },
    Spec {
        what: "batch on the copy engine (own page tables)",
        class: Class::Blitter,
        base: 0x22000,
        vm: true,
    },
    Spec {
        what: "batch on the render engine (own page tables)",
        class: Class::Render,
        base: 0x2000,
        vm: true,
    },
    Spec {
        what: "batch on the compute engine (own page tables)",
        class: Class::Compute,
        base: 0x1a000,
        vm: true,
    },
];

/// What the jobs store (plus the job's number).
const VALUE: u32 = 0xc0de_6000;
const RING_BYTES: u32 = 16384;
/// Where batch and target live in a context's address space.
const BATCH_VA: u64 = 0x0010_0000;
const TARGET_VA: u64 = 0x1_2340_0000;
/// How long the GuC and the engine get for each step.
const STEP_TIMEOUT_US: u32 = 500_000;

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn first_word(buf: &GgttBuf) -> u32 {
    let b = buf.bytes(4);
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn bytes_to_words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// A buffer of at least `min_bytes` that starts with `words`; returns its index in `bufs`.
fn filled(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    words: &[u32],
    min_bytes: u32,
) -> Result<usize, String> {
    buffer(dev, bufs, (words.len() as u64 * 4).max(min_bytes as u64))?;
    let at = bufs.len() - 1;
    bufs[at].fill(&words_to_bytes(words));
    Ok(at)
}

/// The buffers of a test context: indexes into the caller's buffer list.
struct Job {
    id: u32,
    value: u32,
    target: usize,
    image: usize,
    ctx: Context,
}

/// Target page and the ring that makes the engine store `value` there: directly (GGTT) or
/// through a batch in the address space `vm`.
fn ring_for(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    vm: Option<(&mut Ppgtt, &mut TableMem)>,
    value: u32,
) -> Result<(usize, Vec<u32>), String> {
    let target_gtt = buffer(dev, bufs, 4096)?;
    let target = bufs.len() - 1;
    let Some((space, tables)) = vm else {
        return Ok((target, mi::store_job(target_gtt, value)));
    };
    let batch = filled(dev, bufs, &mi::store_batch(TARGET_VA, value), 0)?;
    for (va, index) in [(BATCH_VA, batch), (TARGET_VA, target)] {
        space
            .map(tables, va, &bufs[index].pages())
            .map_err(|e| format!("page tables: {e}"))?;
    }
    Ok((target, mi::batch_job(BATCH_VA)))
}

/// Engine setup, target, ring and the context image.
fn prepare(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    spec: &Spec,
    id: u32,
    tables: &mut TableMem,
) -> Result<Job, String> {
    let status_page = buffer(dev, bufs, 4096)?;
    engines::setup(&mut dev.mmio, spec.base, status_page);
    if spec.class == Class::Compute {
        engines::enable_compute(&mut dev.mmio);
    }
    let value = VALUE + id;
    let mut space = match spec.vm {
        true => Some(Ppgtt::new(tables).map_err(|e| format!("page tables: {e}"))?),
        false => None,
    };
    let vm = space.as_mut().map(|s| (s, &mut *tables));
    let (target, commands) = ring_for(dev, bufs, vm, value)?;
    let ring = filled(dev, bufs, &commands, RING_BYTES)?;
    let ctx = Context {
        class: spec.class,
        mmio_base: spec.base,
        ring_ggtt: bufs[ring].gtt,
        ring_bytes: RING_BYTES,
        tail: commands.len() as u32 * 4,
        pml4: space.as_ref().map(Ppgtt::root),
    };
    let image = ctx.image().map_err(|e| format!("{e}"))?;
    let image = filled(dev, bufs, &bytes_to_words(&image), 0)?;
    Ok(Job {
        id,
        value,
        target,
        image,
        ctx,
    })
}

/// Registers and enables the context and waits until the value is in memory. Returns the time
/// from the enable request to the value in µs.
fn run(
    dev: &mut Device,
    bufs: &[GgttBuf],
    ct: &mut Ct,
    mem: &mut Words,
    job: &Job,
) -> Result<u64, String> {
    let hw = &mut dev.mmio;
    let descriptor = job.ctx.descriptor(bufs[job.image].gtt);
    let register = submit::register(job.id, job.ctx.class, 1, descriptor);
    ct.post(hw, mem, &register)
        .map_err(|e| format!("register: {e}"))?;
    ct.post(hw, mem, &submit::policies(job.id, Policy::DEFAULT))
        .map_err(|e| format!("policies: {e}"))?;
    let t0 = crate::arch::uptime_ns();
    ct.post(hw, mem, &submit::schedule(job.id, true))
        .map_err(|e| format!("enable: {e}"))?;
    let event = submit::EVENT_SCHED_CONTEXT_MODE_DONE;
    let done = ct
        .wait_event(hw, mem, event, STEP_TIMEOUT_US)
        .map_err(|e| format!("enable not confirmed: {e}"))?;
    if done.first() != Some(&job.id) {
        return Err(format!("enable confirmed for another context: {done:x?}"));
    }
    let start = hw.now_us();
    while first_word(&bufs[job.target]) != job.value {
        if hw.now_us().saturating_sub(start) >= STEP_TIMEOUT_US as u64 {
            let image = bufs[job.image].bytes(2 * 4096);
            return Err(format!(
                "value not written (read {:#x}); engine: {}; saved head {:#x?} tail {:#x?}",
                first_word(&bufs[job.target]),
                RingState::read(hw, job.ctx.mmio_base),
                lrc::state_word(&image, lrc::CTX_RING_HEAD),
                lrc::state_word(&image, lrc::CTX_RING_TAIL),
            ));
        }
        hw.delay_us(50);
    }
    Ok((crate::arch::uptime_ns() - t0) / 1000)
}

/// Disables and deregisters the context, so the GuC no longer uses its buffers.
fn retire(dev: &mut Device, ct: &mut Ct, mem: &mut Words, id: u32) -> Result<(), String> {
    let hw = &mut dev.mmio;
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
        ct.post(hw, mem, action)
            .map_err(|e| format!("{what}: {e}"))?;
        ct.wait_event(hw, mem, event, STEP_TIMEOUT_US)
            .map_err(|e| format!("{what} not confirmed: {e}"))?;
    }
    Ok(())
}

/// One job from buffers to the retired context.
fn one(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    ct: &mut Ct,
    mem: &mut Words,
    tables: &mut TableMem,
    (spec, id): (&Spec, u32),
) -> Result<String, String> {
    let job = prepare(dev, bufs, spec, id, tables)?;
    let us = run(dev, bufs, ct, mem, &job)?;
    retire(dev, ct, mem, id)?;
    // what the engine saved when the context left: the state a new context starts from
    let image = bufs[job.image].bytes(spec.class.lrc_size() as usize);
    let head = lrc::state_word(&image, lrc::CTX_RING_HEAD).unwrap_or(0);
    let state = &image[2 * 4096..];
    let saved = state.chunks_exact(4).filter(|w| w.iter().any(|b| *b != 0));
    Ok(format!(
        "stored {:#x} {us} µs after the enable request; context retired, saved ring head {head:#x} of tail {:#x}, {} of {} engine state words set",
        job.value,
        job.ctx.tail,
        saved.count(),
        state.len() / 4
    ))
}

/// Runs the test jobs through the GuC (command buffers `ct`/`mem` are on). The buffers go into
/// `bufs` and are released by the caller after the GuC is reset; the page tables are kept in
/// `tables` for the same reason.
pub fn jobs(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    ct: &mut Ct,
    mem: &mut Words,
    tables: &mut TableMem,
) -> Vec<(&'static str, Result<String, String>)> {
    let mut fwk = Forcewake::new(0);
    if let Err(e) = fwk.get(&mut dev.mmio, Domain::Render) {
        return alloc::vec![(JOBS[0].what, Err(format!("render forcewake: {e}")))];
    }
    let mut out = Vec::new();
    for (id, spec) in JOBS.iter().enumerate() {
        let result = one(dev, bufs, ct, mem, tables, (spec, id as u32));
        let failed = result.is_err();
        out.push((spec.what, result));
        if failed {
            // a context that could not be retired may still be in use: no further jobs
            break;
        }
    }
    if let Err(e) = fwk.put(&mut dev.mmio, Domain::Render) {
        crate::kprintln!("GTTEST: render forcewake release: {e}");
    }
    out
}
