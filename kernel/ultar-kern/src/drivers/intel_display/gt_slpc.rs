//! Starts the GuC's own power management (SLPC, `ultar_intel_gt::slpc`) on the running GT: a
//! shared page, a reset request, wait until it runs, then the limits, sleep when idle and the
//! strategy for compute work – the order of Linux' `xe_guc_pc_start`.

use super::device::Device;
use super::gt;
use super::gt_run::Runtime;
use alloc::format;
use alloc::string::String;
use ultar_intel_gt::{freq, slpc};

/// How long SLPC may take to report that it runs.
const START_TIMEOUT_US: u64 = 1_000_000;
/// How long the GuC may take to answer one request.
const REQUEST_TIMEOUT_US: u32 = 100_000;

fn send(dev: &mut Device, rt: &mut Runtime, action: &[u32], what: &str) -> Result<(), String> {
    rt.ct
        .request(&mut dev.mmio, &mut rt.words, action, REQUEST_TIMEOUT_US)
        .map(|_| ())
        .map_err(|e| format!("SLPC {what}: {e}"))
}

/// Starts SLPC and returns what it reports as text.
pub(super) fn start(dev: &mut Device, rt: &mut Runtime) -> Result<String, String> {
    let at = gt::buffer(dev, &mut rt.mem.bufs, slpc::SHARED_BYTES as u64)?;
    let idx = rt.mem.bufs.len() - 1;
    rt.mem.bufs[idx].fill(&slpc::shared_init());
    send(dev, rt, &slpc::reset(at), "reset")?;
    let t0 = crate::arch::uptime_ns();
    let mut state = 0;
    while (crate::arch::uptime_ns() - t0) / 1000 < START_TIMEOUT_US {
        state = slpc::global_state(&rt.mem.bufs[idx].bytes(8));
        if state == slpc::STATE_RUNNING {
            break;
        }
        crate::arch::sleep_us(1_000);
    }
    let waited = (crate::arch::uptime_ns() - t0) / 1_000_000;
    if state != slpc::STATE_RUNNING {
        return Err(format!(
            "SLPC not running after {waited} ms (state {state})"
        ));
    }
    send(dev, rt, &slpc::query_task_state(at), "query")?;
    let (low, high) = slpc::limits_mhz(&rt.mem.bufs[idx].bytes(slpc::SHARED_BYTES));
    let rp0 = freq::limits(&mut dev.mmio).1.mhz();
    if rp0 != 0 && high > rp0 {
        send(
            dev,
            rt,
            &slpc::set_param(slpc::PARAM_MAX_FREQ_MHZ, rp0),
            "max",
        )?;
    }
    send(dev, rt, &slpc::setup_gucrc(), "sleep control")?;
    send(
        dev,
        rt,
        &slpc::set_param(slpc::PARAM_STRATEGIES, slpc::STRATEGY_COMPUTE),
        "strategy",
    )?;
    Ok(format!(
        "running after {waited} ms, range {low}–{high} MHz (fuses up to {rp0} MHz), now {} MHz",
        freq::current(&mut dev.mmio).mhz()
    ))
}
