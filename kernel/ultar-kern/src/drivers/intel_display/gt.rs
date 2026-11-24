//! The GT of the Intel graphics device (render, copy and compute engines) – the same PCI device
//! and BAR0 as the display engine, so it lives in this driver. The logic is in
//! `drivers/kernel/intel-gt` (pure, tested on the host); this module adds memory, the GGTT and
//! the self test. Plan and state: `docs/integration/intel-gt.md`.
//!
//! Stage G0: identification (graphics/media IP), forcewake, engines, GuC status – read at probe.
//! Stage G1a: the self test (GTTEST) loads the GuC firmware from the boot medium, resets the GuC,
//! hands over signature and image through the GGTT and checks that the boot ROM accepts them.
//! The GuC is put back into reset afterwards: without the ADS block (stage G1b) it cannot run.

use super::buffer::flush;
use super::device::Device;
use super::mmio::Mmio;
use super::DEVICE;
use crate::kprintln;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_display::ggtt;
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::guc::{self, Params, Status};
use ultar_intel_gt::state::{self, GtFacts};
use ultar_intel_gt::{fw, wopcm};

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

/// Kernel pages mapped into the GGTT for the GuC.
struct GgttBuf {
    phys: u64,
    pages: u64,
    gtt: u32,
}

impl GgttBuf {
    fn new(dev: &mut Device, bytes: u64) -> Result<GgttBuf, String> {
        let pages = bytes.div_ceil(4096).max(1);
        let phys = crate::mem::alloc_pages(pages as usize).ok_or("no memory for a GuC buffer")?;
        let gtt = match dev.win.alloc(pages * 4096) {
            Ok(g) => g,
            Err(e) => {
                crate::mem::free_run(phys, pages);
                return Err(format!("GGTT: {e}"));
            }
        };
        let list: Vec<u64> = (0..pages).map(|i| phys + i * 4096).collect();
        let buf = GgttBuf { phys, pages, gtt };
        if let Err(e) = ggtt::map(&mut dev.mmio, gtt, &list, dev.pte_attrs) {
            buf.release(dev);
            return Err(format!("GGTT: {e}"));
        }
        Ok(buf)
    }

    /// Copies `data` to the start of the buffer and writes it out of the CPU caches.
    fn fill(&mut self, data: &[u8]) {
        let n = data.len().min((self.pages * 4096) as usize);
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `data` is a separate heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.phys as *mut u8, n) };
        flush(self.phys, n as u64);
    }

    /// Points the entries at the scratch page again and frees the pages (the caller made sure
    /// the hardware no longer reads them).
    fn release(self, dev: &mut Device) {
        ggtt::unmap(&mut dev.mmio, self.gtt, self.pages as u32, dev.scratch);
        dev.win.free(self.gtt);
        crate::mem::free_run(self.phys, self.pages);
    }
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

/// Resets the GuC and uploads `file`; `Ok` carries the status the load ended with.
fn upload(dev: &mut Device, file: &[u8], img: &fw::GucImage) -> Result<Status, String> {
    let layout = wopcm::init(&mut dev.mmio, wopcm::MTL_WOPCM_SIZE, img.upload_size(), 0)
        .map_err(|e| format!("{e}"))?;
    let mut bufs: Vec<GgttBuf> = Vec::new();
    let result = (|| {
        let mut image = GgttBuf::new(dev, file.len() as u64)?;
        image.fill(file);
        let image_gtt = image.gtt;
        bufs.push(image);
        let log = GgttBuf::new(dev, guc::LOG_BUFFER_SIZE as u64)?;
        let log_gtt = log.gtt;
        bufs.push(log);
        // stage G1b builds the real ADS; an empty page makes the firmware stop at its check
        let ads = GgttBuf::new(dev, 4096)?;
        let ads_gtt = ads.gtt;
        bufs.push(ads);
        for (gtt, len) in [(log_gtt, guc::LOG_BUFFER_SIZE as u64), (ads_gtt, 4096)] {
            guc::check_ggtt(gtt as u64, len, layout.size).map_err(|e| format!("{e}"))?;
        }
        let (b, d, f) = dev.pci;
        let params = Params {
            log_ggtt: log_gtt,
            ads_ggtt: ads_gtt,
            devid: crate::pci::read16(b, d, f, 0x02),
            revid: crate::pci::read8(b, d, f, 0x08),
            release: img.release,
        }
        .words()
        .map_err(|e| format!("{e}"))?;
        guc::reset(&mut dev.mmio).map_err(|e| format!("{e}"))?;
        match guc::upload(
            &mut dev.mmio,
            img,
            file,
            image_gtt as u64,
            layout.size,
            &params,
        ) {
            Ok(st) => Ok(st),
            Err(ultar_intel_gt::Error::GucLoad { status }) => Ok(Status::decode(status)),
            Err(e) => Err(format!("{e}")),
        }
    })();
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
    let t0 = crate::arch::uptime_ms();
    let res = upload(dev, &file, &img);
    let ms = crate::arch::uptime_ms() - t0;
    if let Err(e) = fwk.put(&mut dev.mmio, Domain::Gt) {
        kprintln!("GTTEST: forcewake release: {e}");
    }
    match res {
        Ok(st) => {
            r.check(
                st.authenticated(),
                "GuC image transferred and signature accepted",
                &format!("{st} ({ms} ms)"),
            );
            if st.load() == guc::Load::Ready {
                kprintln!("GTTEST: note: the GuC reported ready");
            } else if st.authenticated() {
                kprintln!("GTTEST: note: the firmware stops at its init data (ADS) – stage G1b");
            }
        }
        Err(e) => r.check(false, "GuC upload", &e),
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
