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
//! The GuC is put back into reset afterwards: engines and submission come with stage G3.

use super::device::Device;
use super::gt_mem::GgttBuf;
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

fn firmware() -> Option<(&'static str, alloc::sync::Arc<Vec<u8>>)> {
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
}

/// GuC action: opt into features by a list of keys in memory.
const ACTION_OPT_IN_FEATURE_KLV: u32 = 0x550e;
/// Key (length 0): report the extended kind of a memory access fault.
const KLV_OPT_IN_EXT_CAT_ERR_TYPE: u32 = 0x4001 << 16;

/// Switches the command buffers on and sends the first request Linux sends over them.
fn ctb_request(dev: &mut Device, bufs: &mut Vec<GgttBuf>) -> Result<String, String> {
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
    Ok(format!(
        "opt-in request answered in {us} µs (data {data0:#x}, {} event(s))",
        events.len()
    ))
}

/// Allocates a GuC buffer, keeps it in `bufs` (released by the caller) and returns its GGTT
/// address.
fn buffer(dev: &mut Device, bufs: &mut Vec<GgttBuf>, bytes: u64) -> Result<u32, String> {
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
    coherent_mapping_probe(dev, bufs, &table);
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

/// GGTT entry bits of cache attribute index 3 on Xe_LPG: write-back with the GT keeping the CPU's
/// caches coherent (what Linux uses for such buffers).
const GGTT_PAT_WB_COHERENT: u64 = 1 << 52 | 1 << 53;

/// Experiment for the buffer design: fetches the table again into a buffer mapped with the
/// coherent cache attribute and reads it without dropping the CPU's lines. Prints whether it
/// equals `expected`; not a check of the self test.
fn coherent_mapping_probe(dev: &mut Device, bufs: &mut Vec<GgttBuf>, expected: &[u8]) {
    let size = expected.len() as u32;
    let buf = match GgttBuf::with_attrs(dev, size as u64, GGTT_PAT_WB_COHERENT) {
        Ok(b) => b,
        Err(e) => return kprintln!("GTTEST: note: coherent mapping probe: {e}"),
    };
    let sent = hxg::get_hwconfig(&mut dev.mmio, buf.gtt as u64, size);
    let same = buf.bytes_as_cached(size as usize) == expected;
    bufs.push(buf);
    kprintln!(
        "GTTEST: note: coherent mapping (cache attribute 3), read without dropping lines: {} ({sent:?})",
        if same { "table identical" } else { "table differs" }
    );
}

/// Maps image, log and the minimal ADS, resets the GuC, uploads and – if it gets ready – talks
/// to it.
fn load(
    dev: &mut Device,
    file: &[u8],
    img: &fw::GucImage,
    wopcm_size: u32,
    stage: Stage,
    bufs: &mut Vec<GgttBuf>,
) -> Result<Loaded, String> {
    let image_gtt = buffer(dev, bufs, file.len() as u64)?;
    if let Some(b) = bufs.last_mut() {
        b.fill(file);
    }
    let log_gtt = buffer(dev, bufs, guc::LOG_BUFFER_SIZE as u64)?;
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
    for (gtt, len) in [(log_gtt, guc::LOG_BUFFER_SIZE as u64), (ads_gtt, ads_size)] {
        guc::check_ggtt(gtt as u64, len, wopcm_size).map_err(|e| format!("{e}"))?;
    }
    let (b, d, f) = dev.pci;
    let params = Params {
        log_ggtt: log_gtt,
        ads_ggtt: ads_gtt,
        devid: crate::pci::read16(b, d, f, 0x02),
        revid: crate::pci::read8(b, d, f, 0x08),
        release: img.release,
    }
    .words(stage)
    .map_err(|e| format!("{e}"))?;
    guc::reset(&mut dev.mmio).map_err(|e| format!("{e}"))?;
    let status = match guc::upload(
        &mut dev.mmio,
        img,
        file,
        image_gtt as u64,
        wopcm_size,
        &params,
    ) {
        Ok(st) => st,
        Err(ultar_intel_gt::Error::GucLoad { status }) => Status::decode(status),
        Err(e) => return Err(format!("{e}")),
    };
    let ready = status.load() == guc::Load::Ready;
    // first load: the register channel; second load: the command buffers
    let hwconfig = (ready && stage == Stage::Minimal).then(|| hwconfig_table(dev, bufs));
    let ctb = (ready && stage == Stage::Full).then(|| ctb_request(dev, bufs));
    Ok(Loaded {
        status,
        hwconfig,
        ctb,
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
    let mut bufs: Vec<GgttBuf> = Vec::new();
    let result = load(dev, file, img, layout.size, stage, &mut bufs);
    // the GuC must not keep running on buffers that are about to be freed
    if let Err(e) = guc::reset(&mut dev.mmio) {
        kprintln!("GTTEST: GuC reset after the upload: {e}");
    }
    for b in bufs {
        b.release(dev);
    }
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
    if r.fails.is_empty() {
        kprintln!("GTTEST: ALLES OK");
    } else {
        kprintln!("GTTEST: FEHLER ({})", r.fails.join("; "));
    }
}
