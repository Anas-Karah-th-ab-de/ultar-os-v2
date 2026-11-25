//! Stage G4b of the GT plan: what programs learn about the GPU. After the GuC handed out the
//! hardware config table and the jobs ran, the device record for the render node
//! (`svcdev/dri.rs`) is built and published; the self-test then asks the node the questions
//! Mesa asks first.

use super::device::Device;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_gt::engines::{self, BCS0, CCS0, RCS0};
use ultar_intel_gt::forcewake::{Domain, Forcewake};
use ultar_intel_gt::fw::Version;
use ultar_intel_gt::state::GtFacts;
use ultar_intel_gt::uapi::{self, drm, EngineClass};

/// The hardware config table as the GuC handed it out (set by the GT self-test).
pub static HWCONFIG: Mutex<Option<Vec<u8>>> = Mutex::new(None);
/// Submission interface version of the GuC firmware that was loaded.
pub static SUBMISSION: Mutex<Option<Version>> = Mutex::new(None);

/// Builds the device record from what the driver found and publishes the render node.
pub fn publish(dev: &mut Device, facts: &GtFacts) -> Result<String, String> {
    let hwconfig = HWCONFIG.lock().clone().ok_or("no hardware config table")?;
    let sub = (*SUBMISSION.lock()).ok_or("no GuC firmware version")?;
    // the clock configuration only reads while the GT domain is awake; the GT in operation
    // keeps it awake itself (and a second holder here would put it to sleep on release)
    let clock = if super::gt_run::running() {
        engines::reference_clock(&mut dev.mmio)
    } else {
        let mut fwk = Forcewake::new(0);
        fwk.get(&mut dev.mmio, Domain::Gt)
            .map_err(|e| format!("GT forcewake: {e}"))?;
        let clock = engines::reference_clock(&mut dev.mmio);
        if let Err(e) = fwk.put(&mut dev.mmio, Domain::Gt) {
            crate::kprintln!("[intel-gt] GT forcewake release: {e}");
        }
        clock
    };
    let clock = clock.ok_or("unknown crystal frequency")?;
    let (b, d, f) = dev.pci;
    let classes = [
        (RCS0, EngineClass::Render),
        (BCS0, EngineClass::Copy),
        (CCS0, EngineClass::Compute),
    ];
    let present = classes.iter().filter(|(bit, _)| facts.engines.has(*bit));
    let ip = facts.graphics;
    let (total, _, used, _) = crate::mem::stats();
    let record = uapi::Device {
        devid: crate::pci::read16(b, d, f, 0x02),
        revid: crate::pci::read8(b, d, f, 0x08),
        engines: present.map(|(_, class)| (*class, 0, 0)).collect(),
        gts: alloc::vec![uapi::Gt {
            media: false,
            id: 0,
            reference_clock: clock,
            ip: (
                (ip.verx100 / 100) as u16,
                (ip.verx100 % 100) as u16,
                ip.revid as u16
            ),
        }],
        memory_bytes: total,
        memory_used: used,
        hwconfig,
        guc_submission: (0, sub.major as u32, sub.minor as u32, sub.patch as u32),
    };
    let text = format!(
        "/dev/{}: {} engine(s), time stamps at {clock} Hz, {} MiB memory",
        crate::svcdev::dri::NODE,
        record.engines.len(),
        total >> 20
    );
    crate::svcdev::dri::publish(record);
    Ok(text)
}

/// A program's memory for the self-test: one buffer at a made-up address.
struct Probe {
    addr: u64,
    data: Vec<u8>,
}

impl drm::User for Probe {
    fn write(&mut self, addr: u64, data: &[u8]) -> bool {
        let fits = addr == self.addr && data.len() <= self.data.len();
        if fits {
            self.data[..data.len()].copy_from_slice(data);
        }
        fits
    }
    fn read(&mut self, addr: u64, len: usize) -> Option<Vec<u8>> {
        (addr == self.addr).then(|| self.data.get(..len).map(<[u8]>::to_vec))?
    }
}

const ADDR: u64 = 0x10_0000;
const IOCTL_VERSION: u32 = 0xc040_6400;
const IOCTL_DEVICE_QUERY: u32 = 0xc028_6440;

/// Asks query `query` the way a program does: first for the size, then for the data.
fn query(query: u32) -> Result<Vec<u8>, String> {
    let mut user = Probe {
        addr: ADDR,
        data: Vec::new(),
    };
    let mut arg = [0u8; 40];
    arg[8..12].copy_from_slice(&query.to_le_bytes());
    let ret = crate::svcdev::dri::request(
        &crate::svcdev::dri::selftest_file(),
        IOCTL_DEVICE_QUERY,
        &mut arg,
        &mut user,
    );
    let size = u32::from_le_bytes([arg[12], arg[13], arg[14], arg[15]]) as usize;
    if ret != 0 || size == 0 {
        return Err(format!(
            "query {query}: size request returned {ret}, size {size}"
        ));
    }
    user.data = alloc::vec![0; size];
    arg[16..24].copy_from_slice(&ADDR.to_le_bytes());
    match crate::svcdev::dri::request(
        &crate::svcdev::dri::selftest_file(),
        IOCTL_DEVICE_QUERY,
        &mut arg,
        &mut user,
    ) {
        0 => Ok(user.data),
        e => Err(format!("query {query}: returned {e}")),
    }
}

/// The first questions of a program: driver name, then engines, config, GT list and the
/// hardware config table.
pub fn selfcheck() -> Result<String, String> {
    let mut user = Probe {
        addr: ADDR,
        data: alloc::vec![0; 8],
    };
    let mut arg = [0u8; 64];
    arg[16] = 8;
    arg[24..32].copy_from_slice(&ADDR.to_le_bytes());
    let ret = crate::svcdev::dri::request(
        &crate::svcdev::dri::selftest_file(),
        IOCTL_VERSION,
        &mut arg,
        &mut user,
    );
    if ret != 0 || &user.data[..2] != drm::DRIVER_NAME.as_bytes() {
        return Err(format!("version request returned {ret}"));
    }
    let engines = query(uapi::QUERY_ENGINES)?;
    let config = query(uapi::QUERY_CONFIG)?;
    let gts = query(uapi::QUERY_GT_LIST)?;
    let table = query(uapi::QUERY_HWCONFIG)?;
    query(uapi::QUERY_MEM_REGIONS)?;
    let id = u32::from_le_bytes([config[8], config[9], config[10], config[11]]);
    Ok(format!(
        "driver \"{}\" {}.{}, device {:#06x} rev {}, {} engine(s), IP {}.{:02}, hardware table {} bytes",
        drm::DRIVER_NAME,
        arg[0],
        arg[4],
        id & 0xffff,
        id >> 16,
        engines[0],
        gts[40],
        gts[42],
        table.len()
    ))
}
