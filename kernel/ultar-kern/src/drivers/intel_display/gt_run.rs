//! The GT in operation: the GuC stays loaded with all engines, the command buffers stay on,
//! the golden context images are recorded – so that contexts of programs can be run (stage
//! G4 of `docs/integration/intel-gt.md`). Until now the GuC only lived inside the self-test.
//!
//! [`start`] is called by the GT self-test after everything passed. A normal boot does not call
//! it yet: the kernel has never run this on bare metal, only under device passthrough.
//!
//! Not there yet: the GT and render forcewake domains are simply held (the GuC's power management
//! runs, `gt_slpc`), interrupts
//! (events are polled), and what to do when the GuC reports a hung engine.

use super::device::Device;
use super::gt::{self, Memory};
use super::gt_job::{self, Env};
use super::gt_mem::Words;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_gt::ads::Class;
use ultar_intel_gt::ctb::Ct;
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::guc::{self, Stage};
use ultar_intel_gt::{fw, wopcm};

/// Everything the running GT owns.
pub(super) struct Runtime {
    pub(super) mem: Memory,
    pub(super) ct: Ct,
    pub(super) words: Words,
    /// Golden images by class.
    pub(super) goldens: Vec<(Class, Vec<u8>)>,
    /// Holds the GT and the render domain awake.
    fwk: Forcewake,
    /// The next context id (and base of the sequence numbers).
    pub(super) next_id: u32,
    /// What programs created: address spaces and queues (`gt_exec`).
    pub(super) programs: super::gt_exec::Programs,
}

pub(super) static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

/// Whether the GT is in operation.
pub fn running() -> bool {
    RUNTIME.lock().is_some()
}

impl Runtime {
    /// Runs `f` with the job environment built from this runtime.
    fn with_env<T>(&mut self, dev: &mut Device, f: impl FnOnce(&mut Env) -> T) -> T {
        let mut env = Env {
            dev,
            bufs: &mut self.mem.bufs,
            ct: &mut self.ct,
            mem: &mut self.words,
            tables: &mut self.mem.tables,
            ads: self.mem.ads,
            goldens: core::mem::take(&mut self.goldens),
        };
        let out = f(&mut env);
        self.goldens = env.goldens;
        out
    }

    /// Engines up, golden images recorded.
    fn prepare(&mut self, dev: &mut Device) -> Result<u32, String> {
        self.fwk
            .get(&mut dev.mmio, Domain::Render)
            .map_err(|e| format!("render forcewake: {e}"))?;
        ultar_intel_gt::engines::program_pat(&mut dev.mmio);
        let before = ultar_intel_gt::engines::program_mocs(&mut dev.mmio);
        let after = ultar_intel_gt::Hw::read(&mut dev.mmio, ultar_intel_gt::engines::LNCFCMOCS0);
        crate::kprintln!("[intel-gt] cache settings (MOCS) programmed: L3 of index 0/1 {before:#010x} -> {after:#010x}");
        match super::gt_slpc::start(dev, self) {
            Ok(text) => crate::kprintln!("[intel-gt] power management by the GuC (SLPC): {text}"),
            Err(e) => {
                crate::kprintln!("[intel-gt] {e}; asking for the highest frequency instead");
                full_speed(dev);
            }
        }
        let first = self.next_id;
        let ran = self.with_env(dev, |env| {
            gt_job::engines_up(env)?;
            gt_job::record_goldens(env, first)
        })?;
        self.next_id += ran;
        Ok(ran)
    }

    /// Puts the GuC into reset and gives all memory back.
    fn shut_down(mut self, dev: &mut Device) {
        if let Err(e) = guc::reset(&mut dev.mmio) {
            crate::kprintln!("[intel-gt] GuC reset: {e}");
        }
        for domain in [Domain::Render, Domain::Gt] {
            if let Err(e) = self.fwk.put(&mut dev.mmio, domain) {
                crate::kprintln!("[intel-gt] forcewake release: {e}");
            }
        }
        self.mem.release(dev);
    }
}

/// Asks for the GT's highest frequency (it would stay near the lowest and compute several times
/// slower) and logs what the GT runs at before and after.
fn full_speed(dev: &mut Device) {
    use ultar_intel_gt::freq;
    let (low, high) = freq::limits(&mut dev.mmio);
    let before = freq::current(&mut dev.mmio);
    if high.0 == 0 {
        crate::kprintln!(
            "[intel-gt] frequency: no limits readable, left as it is ({} MHz)",
            before.mhz()
        );
        return;
    }
    freq::request(&mut dev.mmio, high);
    crate::arch::sleep_us(5_000);
    let after = freq::current(&mut dev.mmio);
    crate::kprintln!("[intel-gt] frequency: {} MHz requested (range {}–{} MHz), running at {} MHz, before {} MHz", high.mhz(), low.mhz(), high.mhz(), after.mhz(), before.mhz());
}

/// Loads the GuC and switches the command buffers on; the pieces of a [`Runtime`].
fn load(dev: &mut Device, mem: &mut Memory) -> Result<(Ct, Words, String), String> {
    let (path, file) = gt::firmware().ok_or("no GuC firmware file on the boot medium")?;
    let img = fw::parse_guc(&file).map_err(|e| format!("{path}: {e}"))?;
    let layout = wopcm::init(&mut dev.mmio, wopcm::MTL_WOPCM_SIZE, img.upload_size(), 0)
        .map_err(|e| format!("{e}"))?;
    let status = gt::bring_up(dev, &file, &img, layout.size, Stage::Full, mem)?;
    if status.load() != guc::Load::Ready {
        return Err(format!("GuC not ready: {status}"));
    }
    let mut channel = None;
    gt::ctb_request(dev, &mut mem.bufs, &mut channel).map_err(|e| format!("CTB: {e}"))?;
    let (ct, words) = channel.ok_or("no command buffers")?;
    Ok((ct, words, format!("GuC {}", img.release)))
}

/// Brings the GT into operation and keeps it there. Does nothing if it already is.
pub fn start(dev: &mut Device) -> Result<String, String> {
    let mut guard = RUNTIME.lock();
    if guard.is_some() {
        return Ok("already in operation".into());
    }
    let t0 = crate::arch::uptime_ns();
    // the GuC's registers only answer while the GT domain is awake; it stays awake from here
    let mut fwk = Forcewake::new(0);
    fwk.get(&mut dev.mmio, Domain::Gt)
        .map_err(|e| format!("GT forcewake: {e}"))?;
    let mut mem = Memory::default();
    let (ct, words, what) = match load(dev, &mut mem) {
        Ok(parts) => parts,
        Err(e) => {
            if let Err(r) = guc::reset(&mut dev.mmio) {
                crate::kprintln!("[intel-gt] GuC reset: {r}");
            }
            if let Err(r) = fwk.put(&mut dev.mmio, Domain::Gt) {
                crate::kprintln!("[intel-gt] GT forcewake release: {r}");
            }
            mem.release(dev);
            return Err(e);
        }
    };
    let mut rt = Runtime {
        mem,
        ct,
        words,
        goldens: Vec::new(),
        fwk,
        next_id: 0,
        programs: Default::default(),
    };
    let ran = match rt.prepare(dev) {
        Ok(n) => n,
        Err(e) => {
            rt.shut_down(dev);
            return Err(e);
        }
    };
    let ms = (crate::arch::uptime_ns() - t0) / 1_000_000;
    let text = format!(
        "{what} running with command buffers, {} golden image(s) from {ran} first job(s), up in {ms} ms",
        rt.goldens.len()
    );
    *guard = Some(rt);
    Ok(text)
}

/// One batch on the compute engine of the GT in operation (context from the golden image).
pub fn compute_probe(dev: &mut Device) -> Result<String, String> {
    let mut guard = RUNTIME.lock();
    let rt = guard.as_mut().ok_or("GT not in operation")?;
    let id = rt.next_id;
    rt.next_id += 1;
    rt.with_env(dev, |env| gt_job::compute_probe(env, id))
}

/// Takes the GT out of operation (GuC in reset, memory given back). No caller yet: it belongs
/// to shutdown and suspend, which the GT does not take part in so far.
#[allow(dead_code)]
pub fn stop(dev: &mut Device) {
    if let Some(rt) = RUNTIME.lock().take() {
        rt.shut_down(dev);
    }
}
