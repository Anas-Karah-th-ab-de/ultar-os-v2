//! The GT of the Intel graphics device (render, copy and compute engines) – the same PCI device
//! and BAR0 as the display engine, so it lives in this driver. The logic is in
//! `drivers/kernel/intel-gt` (pure, tested on the host); this module adds memory, the GGTT and
//! the self test. Plan and state: `docs/integration/intel-gt.md`.
//!
//! Stage G0: identification (graphics/media IP), forcewake, engines, GuC status – read at probe.
//! Stage G1a/G1b: the self test (GTTEST) loads the GuC firmware from the boot medium, resets the
//! GuC, hands over signature, image and the minimal ADS block through the GGTT, waits until the
//! firmware is ready and asks it for the hardware config table over the register message channel.
//! The GuC is put back into reset afterwards: engines and submission come with stage G3.

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
        // the GT does not look into the CPU's caches: write the zeroes out, and drop the lines so
        // what the GuC writes later is read from memory
        flush(phys, pages * 4096);
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

    /// A copy of the first `len` bytes (what the GuC wrote there).
    fn bytes(&self, len: usize) -> Vec<u8> {
        let n = len.min((self.pages * 4096) as usize);
        let mut out = alloc::vec![0u8; n];
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `out` is a fresh heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(self.phys as *const u8, out.as_mut_ptr(), n) };
        out
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

/// How a load ended.
struct Loaded {
    status: Status,
    /// Size in bytes and number of entries of the hardware config table, if the GuC got ready.
    hwconfig: Option<Result<(u32, usize), String>>,
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
    let entries = hwconfig::parse(&table).map_err(|e| {
        let head: Vec<String> = table
            .chunks_exact(4)
            .take(200)
            .map(|c| format!("{:x}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect();
        format!("{e} ({size} bytes, first words: {})", head.join(" "))
    })?;
    Ok((size, entries.len()))
}

/// Maps image, log and the minimal ADS, resets the GuC, uploads and – if it gets ready – talks
/// to it.
fn load(
    dev: &mut Device,
    file: &[u8],
    img: &fw::GucImage,
    wopcm_size: u32,
    bufs: &mut Vec<GgttBuf>,
) -> Result<Loaded, String> {
    let image_gtt = buffer(dev, bufs, file.len() as u64)?;
    if let Some(b) = bufs.last_mut() {
        b.fill(file);
    }
    let log_gtt = buffer(dev, bufs, guc::LOG_BUFFER_SIZE as u64)?;
    let ads_size = ads::minimal_size(img.private_data_size) as u64;
    let ads_gtt = buffer(dev, bufs, ads_size)?;
    let doorbells = dev.mmio.read(regs::DIST_DBS_POPULATED);
    let blob =
        ads::minimal(ads_gtt, img.private_data_size, doorbells).map_err(|e| format!("{e}"))?;
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
    .words(Stage::Minimal)
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
    let hwconfig = (status.load() == guc::Load::Ready).then(|| hwconfig_table(dev, bufs));
    Ok(Loaded { status, hwconfig })
}

/// Loads the GuC with `file`; afterwards the GuC is back in reset and every buffer is freed.
fn upload(dev: &mut Device, file: &[u8], img: &fw::GucImage) -> Result<Loaded, String> {
    let layout = wopcm::init(&mut dev.mmio, wopcm::MTL_WOPCM_SIZE, img.upload_size(), 0)
        .map_err(|e| format!("{e}"))?;
    let mut bufs: Vec<GgttBuf> = Vec::new();
    let result = load(dev, file, img, layout.size, &mut bufs);
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
        Ok(l) => {
            let st = l.status;
            r.check(
                st.authenticated(),
                "GuC image transferred and signature accepted",
                &format!("{st} ({ms} ms)"),
            );
            r.check(
                st.load() == guc::Load::Ready,
                "GuC ready with the minimal ADS",
                "",
            );
            match l.hwconfig {
                Some(Ok((size, entries))) => r.check(
                    entries > 0,
                    "hardware config table from the GuC",
                    &format!("{size} bytes, {entries} entries"),
                ),
                Some(Err(e)) => r.check(false, "hardware config table from the GuC", &e),
                None => {}
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
