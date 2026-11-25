//! The GT of the Intel graphics device (render, copy and compute engines) – the same PCI device
//! and BAR0 as the display engine, so it lives in this driver. The logic is in
//! `drivers/kernel/intel-gt` (pure, tested on the host); this module adds memory, the GGTT and
//! the self test. Plan and state: `docs/integration/intel-gt.md`.
//!
//! Stage G0: identification (graphics/media IP), forcewake, engines, GuC status – read at probe.
//! Stage G1a/G1b: the self test (GTTEST) loads the GuC firmware from the boot medium, resets the
//! GuC, hands over signature, image and the minimal ADS block through the GGTT, waits until the
//! firmware is ready, asks it for the hardware config table over the register message channel,
//! then loads a second time with the full ADS (engines, register lists, golden context slots) and
//! all features, switches the command buffers (CTB) on and sends a first request through them.
//! Stage G3 (`gt_job`): contexts for the copy, render and compute engine are registered and
//! enabled, and their commands (in the ring, then in a batch in the context's own address space)
//! must store a value in memory. The GuC is put back into reset afterwards.

use super::device::Device;
use super::gt_mem::{GgttBuf, TableMem, Words};
use super::mmio::Mmio;
use super::DEVICE;
use crate::kprintln;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_gt::ctb::{self, Ct};
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::guc::{self, Params, Stage, Status};
use ultar_intel_gt::state::{self, GtFacts};
use ultar_intel_gt::{ads, fw, hwconfig, hxg, regs, wopcm, Hw};

/// What the probe read (for reports).
static FACTS: Mutex<Option<GtFacts>> = Mutex::new(None);

/// Where the GuC firmware may lie: the boot medium first, then an installed system.
const GUC_PATHS: &[&str] = &[
    "/boot/efi/firmware/mtl_guc_70.bin",
    "/firmware/mtl_guc_70.bin",
    "/lib/firmware/i915/mtl_guc_70.bin",
];

/// Reads the GT's state at probe (the only writes are forcewake requests) and logs it.
pub fn probe(mmio: &mut Mmio) {
    match state::read(mmio) {
        Ok(f) => {
            kprintln!("[intel-gt] {f}");
            *FACTS.lock() = Some(f);
        }
        Err(e) => kprintln!("[intel-gt] not readable: {e}"),
    }
}

/// One line for reports.
pub fn text() -> Option<String> {
    FACTS.lock().as_ref().map(|f| format!("{f}"))
}

/// Failed checks, collected.
struct Report {
    fails: Vec<&'static str>,
}

impl Report {
    fn check(&mut self, ok: bool, what: &'static str, detail: &str) {
        kprintln!(
            "GTTEST: {} {what}{}{detail}",
            if ok { "ok  " } else { "FAIL" },
            if detail.is_empty() { "" } else { ": " }
        );
        if !ok {
            self.fails.push(what);
        }
    }
}

pub(super) fn firmware() -> Option<(&'static str, alloc::sync::Arc<Vec<u8>>)> {
    GUC_PATHS
        .iter()
        .find_map(|p| crate::fs::read_all(p).map(|d| (*p, d)))
}

/// How a load ended.
struct Loaded {
    status: Status,
    /// Size in bytes and number of entries of the hardware config table, if the GuC got ready.
    hwconfig: Option<Result<(u32, usize), String>>,
    /// What the request over the command buffers brought, if the GuC got ready.
    ctb: Option<Result<String, String>>,
    /// What the jobs on the engines brought, if the command buffers work.
    jobs: Vec<(&'static str, Result<String, String>)>,
}

/// GuC action: opt into features by a list of keys in memory.
const ACTION_OPT_IN_FEATURE_KLV: u32 = 0x550e;
/// Key (length 0): report the extended kind of a memory access fault.
const KLV_OPT_IN_EXT_CAT_ERR_TYPE: u32 = 0x4001 << 16;

/// Switches the command buffers on and sends the first request Linux sends over them; then runs
/// jobs on the engines. Returns what the request and each job brought.
fn ctb_and_jobs(
    dev: &mut Device,
    mem: &mut Memory,
) -> (
    Result<String, String>,
    Vec<(&'static str, Result<String, String>)>,
) {
    let mut channel = None;
    let ctb = ctb_request(dev, &mut mem.bufs, &mut channel);
    let jobs = match channel {
        Some((mut ct, mut words)) => super::gt_job::jobs(&mut super::gt_job::Env {
            dev,
            bufs: &mut mem.bufs,
            ct: &mut ct,
            mem: &mut words,
            tables: &mut mem.tables,
            ads: mem.ads,
            goldens: Vec::new(),
        }),
        None => Vec::new(),
    };
    (ctb, jobs)
}

/// What a load keeps until the GuC is back in reset: buffers and page tables.
#[derive(Default)]
pub(super) struct Memory {
    pub(super) bufs: Vec<GgttBuf>,
    pub(super) tables: TableMem,
    /// Index of the ADS block in `bufs`.
    pub(super) ads: usize,
}

impl Memory {
    /// Gives everything back (the GuC is in reset and no engine runs).
    pub(super) fn release(self, dev: &mut Device) {
        for b in self.bufs {
            b.release(dev);
        }
        self.tables.release();
    }
}

pub(super) fn ctb_request(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    channel: &mut Option<(Ct, Words)>,
) -> Result<String, String> {
    let ct_gtt = buffer(dev, bufs, ctb::SIZE_BYTES as u64)?;
    let mut mem = bufs.last().map(GgttBuf::words).ok_or("no CTB buffer")?;
    let klv_gtt = buffer(dev, bufs, 4096)?;
    if let Some(b) = bufs.last_mut() {
        b.fill(&KLV_OPT_IN_EXT_CAT_ERR_TYPE.to_le_bytes());
    }
    let mut ct = Ct::enable(&mut dev.mmio, &mut mem, ct_gtt).map_err(|e| format!("enable: {e}"))?;
    let t0 = crate::arch::uptime_ns();
    let action = [ACTION_OPT_IN_FEATURE_KLV, klv_gtt, 0, 1];
    let (data0, events) = ct
        .request(&mut dev.mmio, &mut mem, &action, 1_000_000)
        .map_err(|e| format!("request: {e}"))?;
    let us = (crate::arch::uptime_ns() - t0) / 1000;
    *channel = Some((ct, mem));
    Ok(format!(
        "opt-in request answered in {us} µs (data {data0:#x}, {} event(s))",
        events.len()
    ))
}

/// Allocates a GuC buffer, keeps it in `bufs` (released by the caller) and returns its GGTT
/// address.
pub(super) fn buffer(dev: &mut Device, bufs: &mut Vec<GgttBuf>, bytes: u64) -> Result<u32, String> {
    let buf = GgttBuf::new(dev, bytes)?;
    let gtt = buf.gtt;
    bufs.push(buf);
    Ok(gtt)
}

/// Asks the running GuC for the hardware config table and checks its structure.
fn hwconfig_table(dev: &mut Device, bufs: &mut Vec<GgttBuf>) -> Result<(u32, usize), String> {
    let size = hxg::get_hwconfig(&mut dev.mmio, 0, 0).map_err(|e| format!("{e}"))?;
    if size == 0 || size > 1 << 20 {
        return Err(format!("implausible table size {size}"));
    }
    let gtt = buffer(dev, bufs, size as u64)?;
    hxg::get_hwconfig(&mut dev.mmio, gtt as u64, size).map_err(|e| format!("{e}"))?;
    let table = bufs
        .last()
        .map(|b| b.bytes(size as usize))
        .unwrap_or_default();
    *super::gt_uapi::HWCONFIG.lock() = Some(table.clone());
    let entries = hwconfig::parse(&table).map_err(|e| {
        let head: Vec<String> = table
            .chunks_exact(4)
            .take(16)
            .map(|c| format!("{:x}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect();
        format!("{e} ({size} bytes, first words: {})", head.join(" "))
    })?;
    Ok((size, entries.len()))
}

/// The boot parameter words for this device and load.
fn boot_params(
    dev: &Device,
    img: &fw::GucImage,
    log_ggtt: u32,
    ads_ggtt: u32,
    stage: Stage,
) -> Result<[u32; guc::PARAM_WORDS], String> {
    let (b, d, f) = dev.pci;
    Params {
        log_ggtt,
        ads_ggtt,
        devid: crate::pci::read16(b, d, f, 0x02),
        revid: crate::pci::read8(b, d, f, 0x08),
        release: img.release,
    }
    .words(stage)
    .map_err(|e| format!("{e}"))
}

/// Builds the ADS block of `stage` in a new buffer; returns its GGTT address and size.
fn ads_block(
    dev: &mut Device,
    bufs: &mut Vec<GgttBuf>,
    img: &fw::GucImage,
    stage: Stage,
) -> Result<(u32, u64), String> {
    let private = img.private_data_size;
    let ads_size = match stage {
        Stage::Minimal => ads::minimal_size(private),
        Stage::Full => ads::full_size(private, &ads::XE_LPG_ENGINES),
    } as u64;
    let ads_gtt = buffer(dev, bufs, ads_size)?;
    let doorbells = dev.mmio.read(regs::DIST_DBS_POPULATED);
    let blob = match stage {
        Stage::Minimal => ads::minimal(ads_gtt, private, doorbells),
        Stage::Full => ads::full(ads_gtt, private, doorbells, &ads::XE_LPG_ENGINES),
    }
    .map_err(|e| format!("{e}"))?;
    if let Some(b) = bufs.last_mut() {
        b.fill(&blob);
    }
    Ok((ads_gtt, ads_size))
}

/// Maps image, log and the minimal ADS, resets the GuC, uploads and – if it gets ready – talks
/// to it.
/// Buffers, boot parameters and the upload: the GuC's status afterwards. The ADS block's
/// index goes into `mem.ads`.
pub(super) fn bring_up(
    dev: &mut Device,
    file: &[u8],
    img: &fw::GucImage,
    wopcm_size: u32,
    stage: Stage,
    mem: &mut Memory,
) -> Result<Status, String> {
    let bufs = &mut mem.bufs;
    let image_gtt = buffer(dev, bufs, file.len() as u64)?;
    if let Some(b) = bufs.last_mut() {
        b.fill(file);
    }
    let log_gtt = buffer(dev, bufs, guc::LOG_BUFFER_SIZE as u64)?;
    let (ads_gtt, ads_size) = ads_block(dev, bufs, img, stage)?;
    mem.ads = bufs.len() - 1;
    for (gtt, len) in [(log_gtt, guc::LOG_BUFFER_SIZE as u64), (ads_gtt, ads_size)] {
        guc::check_ggtt(gtt as u64, len, wopcm_size).map_err(|e| format!("{e}"))?;
    }
    let params = boot_params(dev, img, log_gtt, ads_gtt, stage)?;
    guc::reset(&mut dev.mmio).map_err(|e| format!("{e}"))?;
    match guc::upload(
        &mut dev.mmio,
        img,
        file,
        image_gtt as u64,
        wopcm_size,
        &params,
    ) {
        Ok(st) => Ok(st),
        Err(ultar_intel_gt::Error::GucLoad { status }) => Ok(Status::decode(status)),
        Err(e) => Err(format!("{e}")),
    }
}

fn load(
    dev: &mut Device,
    file: &[u8],
    img: &fw::GucImage,
    wopcm_size: u32,
    stage: Stage,
    mem: &mut Memory,
) -> Result<Loaded, String> {
    let status = bring_up(dev, file, img, wopcm_size, stage, mem)?;
    let bufs = &mut mem.bufs;
    let ready = status.load() == guc::Load::Ready;
    // first load: the register channel; second load: the command buffers
    let hwconfig = (ready && stage == Stage::Minimal).then(|| hwconfig_table(dev, bufs));
    let (ctb, jobs) = match ready && stage == Stage::Full {
        true => {
            let (ctb, jobs) = ctb_and_jobs(dev, mem);
            (Some(ctb), jobs)
        }
        false => (None, Vec::new()),
    };
    Ok(Loaded {
        status,
        hwconfig,
        ctb,
        jobs,
    })
}

/// Loads the GuC with `file`; afterwards the GuC is back in reset and every buffer is freed.
fn upload(
    dev: &mut Device,
    file: &[u8],
    img: &fw::GucImage,
    stage: Stage,
) -> Result<Loaded, String> {
    let layout = wopcm::init(&mut dev.mmio, wopcm::MTL_WOPCM_SIZE, img.upload_size(), 0)
        .map_err(|e| format!("{e}"))?;
    let mut mem = Memory::default();
    let result = load(dev, file, img, layout.size, stage, &mut mem);
    // the GuC must not keep running on buffers that are about to be freed
    if let Err(e) = guc::reset(&mut dev.mmio) {
        kprintln!("GTTEST: GuC reset after the upload: {e}");
    }
    mem.release(dev);
    result
}

fn run_on(dev: &mut Device, r: &mut Report) {
    match state::read(&mut dev.mmio) {
        Ok(f) => {
            r.check(true, "GT identified", &format!("{f}"));
            r.check(
                f.engines.count() == 3,
                "render, copy and compute engine present",
                "",
            );
            *FACTS.lock() = Some(f);
        }
        Err(e) => {
            r.check(false, "GT identified", &format!("{e}"));
            return;
        }
    }
    guc_check(dev, r);
}

/// Loads the GuC firmware from the boot medium and checks that the hardware accepts it.
fn guc_check(dev: &mut Device, r: &mut Report) {
    let Some((path, file)) = firmware() else {
        kprintln!(
            "GTTEST: skip GuC upload (no firmware file; put mtl_guc_70.bin into \\firmware on the boot medium)"
        );
        return;
    };
    let img = match fw::parse_guc(&file) {
        Ok(i) => i,
        Err(e) => return r.check(false, "GuC firmware file", &format!("{path}: {e}")),
    };
    *super::gt_uapi::SUBMISSION.lock() = Some(img.submission);
    r.check(
        true,
        "GuC firmware file",
        &format!(
            "{path}: GuC {} (submission {}), {} bytes, RSA key {} bytes",
            img.release,
            img.submission,
            img.upload_size(),
            img.rsa_size
        ),
    );
    let mut fwk = Forcewake::new(0);
    if let Err(e) = fwk.get(&mut dev.mmio, Domain::Gt) {
        return r.check(false, "GT forcewake", &format!("{e}"));
    }
    for stage in [Stage::Minimal, Stage::Full] {
        let t0 = crate::arch::uptime_ms();
        let res = upload(dev, &file, &img, stage);
        let ms = crate::arch::uptime_ms() - t0;
        match res {
            Ok(l) => report_load(r, &l, stage, ms),
            Err(e) => r.check(false, "GuC upload", &e),
        }
    }
    if let Err(e) = fwk.put(&mut dev.mmio, Domain::Gt) {
        kprintln!("GTTEST: forcewake release: {e}");
    }
}

/// Reports what a load brought.
fn report_load(r: &mut Report, l: &Loaded, stage: Stage, ms: u64) {
    let st = l.status;
    let took = format!("{st} ({ms} ms)");
    let (accepted, ready) = match stage {
        Stage::Minimal => (
            "GuC image transferred and signature accepted",
            "GuC ready with the minimal ADS",
        ),
        Stage::Full => (
            "GuC image accepted again (second load)",
            "GuC ready with the full ADS (render, copy, compute engine; features on)",
        ),
    };
    r.check(st.authenticated(), accepted, &took);
    r.check(st.load() == guc::Load::Ready, ready, "");
    match &l.hwconfig {
        Some(Ok((size, entries))) => r.check(
            *entries > 0,
            "hardware config table from the GuC",
            &format!("{size} bytes, {entries} entries"),
        ),
        Some(Err(e)) => r.check(false, "hardware config table from the GuC", e),
        None => {}
    }
    match &l.ctb {
        Some(Ok(t)) => r.check(true, "command buffers (CTB)", t),
        Some(Err(e)) => r.check(false, "command buffers (CTB)", e),
        None => {}
    }
    for (what, result) in &l.jobs {
        match result {
            Ok(t) => r.check(true, what, t),
            Err(e) => r.check(false, what, e),
        }
    }
}

/// GTTEST – the GT's self test on real hardware. Ends with `GTTEST: ALLES OK` or
/// `GTTEST: FEHLER (<list>)`.
pub fn selftest() {
    let mut guard = DEVICE.lock();
    let Some(dev) = guard.as_mut() else {
        kprintln!("GTTEST: no Intel display device bound");
        return;
    };
    let mut r = Report { fails: Vec::new() };
    run_on(dev, &mut r);
    let facts = *FACTS.lock();
    if let (true, Some(f)) = (r.fails.is_empty(), facts) {
        // everything a program is told has been seen on this device: the render node appears
        match super::gt_run::start(dev) {
            Ok(t) => r.check(true, "GT stays in operation", &t),
            Err(e) => r.check(false, "GT stays in operation", &e),
        }
        if super::gt_run::running() {
            match super::gt_run::compute_probe(dev) {
                Ok(t) => r.check(true, "compute job on the GT in operation", &t),
                Err(e) => r.check(false, "compute job on the GT in operation", &e),
            }
        }
        if let Ok(t) = super::gt_uapi::publish(dev, &f) {
            r.check(true, "render node published", &t);
            match super::gt_uapi::selfcheck() {
                Ok(t) => r.check(true, "program interface answers the first questions", &t),
                Err(e) => r.check(false, "program interface answers the first questions", &e),
            }
        }
    }
    if r.fails.is_empty() {
        kprintln!("GTTEST: ALLES OK");
    } else {
        kprintln!("GTTEST: FEHLER ({})", r.fails.join("; "));
    }
}
