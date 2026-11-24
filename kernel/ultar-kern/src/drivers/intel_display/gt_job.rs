//! Stage G3 of the GT plan: the first job on an engine. A context for the copy engine is built
//! (ring with one store command, context image), registered with the running GuC, enabled – and
//! the engine must then have written the value into memory. Logic: `ultar-intel-gt`
//! (`lrc`, `mi`, `submit`, `engines`); this module adds the buffers.

use super::device::Device;
use super::gt::buffer;
use super::gt_mem::{GgttBuf, Words};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ultar_intel_gt::ads::Class;
use ultar_intel_gt::ctb::Ct;
use ultar_intel_gt::engines::{self, RingState};
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::lrc::{self, Context};
use ultar_intel_gt::submit::{self, Policy};
use ultar_intel_gt::{mi, Hw};

/// Register base of the copy engine (bcs0).
const BCS0: u32 = 0x22000;
/// The context id the test uses.
const CONTEXT_ID: u32 = 0;
/// What the job stores.
const VALUE: u32 = 0xc0de_600d;
const RING_BYTES: u32 = 16384;
/// How long the GuC and the engine get for each step.
const STEP_TIMEOUT_US: u32 = 500_000;

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn first_word(buf: &GgttBuf) -> u32 {
    let b = buf.bytes(4);
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// The buffers of the test context: indexes into the caller's buffer list.
struct Job {
    target: usize,
    image: usize,
    ctx: Context,
    image_gtt: u32,
}

/// Engine setup, target page, ring with the store command and the context image.
fn prepare(dev: &mut Device, bufs: &mut Vec<GgttBuf>) -> Result<Job, String> {
    let status_page = buffer(dev, bufs, 4096)?;
    engines::setup(&mut dev.mmio, BCS0, status_page);
    let target_gtt = buffer(dev, bufs, 4096)?;
    let target = bufs.len() - 1;
    let ring_gtt = buffer(dev, bufs, RING_BYTES as u64)?;
    let commands = mi::store_job(target_gtt, VALUE);
    if let Some(b) = bufs.last_mut() {
        b.fill(&words_to_bytes(&commands));
    }
    let ctx = Context {
        class: Class::Blitter,
        mmio_base: BCS0,
        ring_ggtt: ring_gtt,
        ring_bytes: RING_BYTES,
        tail: commands.len() as u32 * 4,
        pml4: None,
    };
    let image = ctx.image().map_err(|e| format!("{e}"))?;
    let image_gtt = buffer(dev, bufs, image.len() as u64)?;
    if let Some(b) = bufs.last_mut() {
        b.fill(&image);
    }
    Ok(Job {
        target,
        image: bufs.len() - 1,
        ctx,
        image_gtt,
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
    let descriptor = job.ctx.descriptor(job.image_gtt);
    let register = submit::register(CONTEXT_ID, Class::Blitter, 1, descriptor);
    ct.post(hw, mem, &register)
        .map_err(|e| format!("register: {e}"))?;
    ct.post(hw, mem, &submit::policies(CONTEXT_ID, Policy::DEFAULT))
        .map_err(|e| format!("policies: {e}"))?;
    let t0 = crate::arch::uptime_ns();
    ct.post(hw, mem, &submit::schedule(CONTEXT_ID, true))
        .map_err(|e| format!("enable: {e}"))?;
    let done = ct
        .wait_event(
            hw,
            mem,
            submit::EVENT_SCHED_CONTEXT_MODE_DONE,
            STEP_TIMEOUT_US,
        )
        .map_err(|e| format!("enable not confirmed: {e}"))?;
    if done.first() != Some(&CONTEXT_ID) {
        return Err(format!("enable confirmed for another context: {done:x?}"));
    }
    let start = hw.now_us();
    while first_word(&bufs[job.target]) != VALUE {
        if hw.now_us().saturating_sub(start) >= STEP_TIMEOUT_US as u64 {
            let image = bufs[job.image].bytes(2 * 4096);
            return Err(format!(
                "value not written (read {:#x}); engine: {}; saved head {:#x?} tail {:#x?}",
                first_word(&bufs[job.target]),
                RingState::read(hw, BCS0),
                lrc::state_word(&image, lrc::CTX_RING_HEAD),
                lrc::state_word(&image, lrc::CTX_RING_TAIL),
            ));
        }
        hw.delay_us(50);
    }
    Ok((crate::arch::uptime_ns() - t0) / 1000)
}

/// Disables and deregisters the context, so the GuC no longer uses its buffers.
fn retire(dev: &mut Device, ct: &mut Ct, mem: &mut Words) -> Result<(), String> {
    let hw = &mut dev.mmio;
    for (what, action, event) in [
        (
            "disable",
            &submit::schedule(CONTEXT_ID, false)[..],
            submit::EVENT_SCHED_CONTEXT_MODE_DONE,
        ),
        (
            "deregister",
            &submit::deregister(CONTEXT_ID)[..],
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

/// Runs one store command on the copy engine through the GuC (command buffers `ct`/`mem` are
/// on). The buffers go into `bufs` and are released by the caller after the GuC is reset.
pub fn first_job(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    ct: &mut Ct,
    mem: &mut Words,
) -> Result<String, String> {
    let mut fwk = Forcewake::new(0);
    fwk.get(&mut dev.mmio, Domain::Render)
        .map_err(|e| format!("render forcewake: {e}"))?;
    let result = prepare(dev, bufs).and_then(|job| {
        let us = run(dev, bufs, ct, mem, &job)?;
        let image = bufs[job.image].bytes(2 * 4096);
        let head = lrc::state_word(&image, lrc::CTX_RING_HEAD).unwrap_or(0);
        retire(dev, ct, mem)?;
        Ok(format!(
            "bcs0 stored {VALUE:#x} {us} µs after the enable request; context retired, saved ring head {head:#x} of tail {:#x}",
            job.ctx.tail
        ))
    });
    if let Err(e) = fwk.put(&mut dev.mmio, Domain::Render) {
        crate::kprintln!("GTTEST: render forcewake release: {e}");
    }
    result
}
