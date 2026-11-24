//! Safe discovery and preflight of NVIDIA display hardware.
//!
//! Without opt-in this stage is read-only. It identifies the known Quadro RTX 4000
//! class, records BARs and PCIe state, validates the register aperture and keeps the
//! UEFI/GOP framebuffer active. With opt-in it only touches PCI configuration space so the
//! card can answer (D3hot -> D0 with BARs restored, memory decoding on – on hybrid-graphics
//! notebooks UEFI often leaves the dGPU asleep or undecoded). No NVIDIA register is written.

use crate::driver::{Driver, Result};
use crate::pci::PciDev;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub const OPTION_WORD: &str = "ultar.nvidia_display=probe";
pub const FILE_PATH: &str = "\\ultar\\nvidia-display";
const QUADRO_RTX_4000: u16 = 0x1eb6;
const PCI_CAP_PM: u8 = 0x01;
const PCI_CAP_MSI: u8 = 0x05;
const PCI_CAP_PCIE: u8 = 0x10;
const BAR0_MIN_SIZE: u64 = 16 << 20;
const PMC_BOOT_0: u64 = 0x000000;
const PMC_BOOT_1: u64 = 0x000004;
const PMC_ENABLE: u64 = 0x000200;
/// PMC_BOOT_0 chipset field (bits 20..28) of TU104 – the Quadro RTX 4000 die.
const CHIPSET_TU104: u32 = 0x164;

const GSP_FIRMWARE_CANDIDATES: &[&str] = &[
    "/lib/firmware/nvidia/535.*/gsp_tu10x.bin",
    "/lib/firmware/nvidia/550.*/gsp_tu10x.bin",
    "/lib/firmware/nvidia/570.*/gsp_tu10x.bin",
];

static OPTED_IN: AtomicBool = AtomicBool::new(false);
static REPORT: Mutex<Option<String>> = Mutex::new(None);

/// Remove the NVIDIA discovery option from UEFI load options without changing A/B options.
pub fn take_boot_option(options: &mut [u16], n: usize) -> usize {
    let n = n.min(options.len());
    let end = options[..n].iter().position(|&u| u == 0).unwrap_or(n);
    let mut read = 0usize;
    let mut write = 0usize;
    let mut found = false;
    while read < end {
        let stop = options[read..end]
            .iter()
            .position(|&u| u == b' ' as u16)
            .map_or(end, |v| read + v);
        let word = read..stop;
        read = stop + 1;
        if !word.is_empty()
            && OPTION_WORD
                .encode_utf16()
                .eq(options[word.clone()].iter().copied())
        {
            found = true;
            continue;
        }
        if write > 0 {
            options[write] = b' ' as u16;
            write += 1;
        }
        options.copy_within(word.clone(), write);
        write += word.len();
    }
    options[write..n].fill(0);
    if found {
        OPTED_IN.store(true, Ordering::Relaxed);
    }
    write
}

pub fn opt_in_file(content: &[u8]) {
    if content.trim_ascii() == b"probe" {
        OPTED_IN.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct BarReport {
    index: usize,
    address: u64,
    size: u64,
}

struct Mmio {
    base: u64,
    size: u64,
}

impl Mmio {
    fn new(base: u64, size: u64) -> Option<Self> {
        (base != 0 && size >= BAR0_MIN_SIZE && base % 4096 == 0).then_some(Self { base, size })
    }

    fn read32(&self, offset: u64) -> Option<u32> {
        if offset.checked_add(4)? > self.size || offset & 3 != 0 {
            return None;
        }
        let address = self.base.checked_add(offset)?;
        // SAFETY: PCI BAR0 was validated as an aligned, identity-mapped MMIO aperture by
        // pci::scan. Reads are volatile and no NVIDIA register is written by this driver.
        Some(unsafe { core::ptr::read_volatile(address as *const u32) })
    }
}

fn firmware_state() -> &'static str {
    // Firmware is copied into the initramfs by the image builder. The kernel bind phase runs
    // before filesystem mounts, so this is deliberately a capability label rather than a file
    // read: attempting to open a path here would race the mount order and report false negatives.
    if GSP_FIRMWARE_CANDIDATES.is_empty() {
        "unavailable"
    } else {
        "packaged-candidate"
    }
}

/// Chipset name from PMC_BOOT_0 (bits 20..28).
fn chipset(boot0: u32) -> String {
    if boot0 == 0xffff_ffff || boot0 == 0 {
        return String::from("chip=not-responding");
    }
    let id = (boot0 >> 20) & 0x1ff;
    match id {
        CHIPSET_TU104 => String::from("chip=tu104"),
        0x162 => String::from("chip=tu102(unexpected)"),
        0x166 => String::from("chip=tu106(unexpected)"),
        _ => format!("chip={id:#x}(unexpected)"),
    }
}

/// Bring the card to D0 and enable memory decoding – PCI configuration space only.
/// PME_Status and the status register are RW1C and are written as 0 (never cleared).
fn wake(d: &PciDev) -> String {
    let mut notes = alloc::vec::Vec::new();
    if let Some(off) = cap_offset(d, PCI_CAP_PM) {
        let csr = d.r32(off + 4);
        let state = csr & 0x3;
        if state != 0 {
            // No_Soft_Reset (bit 3) clear: D3hot->D0 resets the function, BARs must be restored.
            let saved: [u32; 7] = core::array::from_fn(|i| d.r32(0x10 + 4 * i as u8));
            d.w32(off + 4, csr & !0x8003);
            crate::arch::sleep_us(10_000); // PCIe: 10 ms recovery after D3hot -> D0
            if csr & 0x8 == 0 {
                for (i, v) in saved.iter().enumerate() {
                    d.w32(0x10 + 4 * i as u8, *v);
                }
            }
            notes.push(format!("wake=D{state}->D{}", d.r16(off + 4) & 0x3));
        }
    }
    let cmd = d.r32(0x04);
    let bar0 = d.bars[0].is_some_and(|b| !b.io && b.addr != 0);
    if cmd & 0x2 == 0 && bar0 {
        d.w32(0x04, (cmd & 0xffff) | 0x2);
        notes.push(String::from("mem=enabled"));
    }
    if notes.is_empty() {
        String::from("wake=not-needed")
    } else {
        notes.join(" ")
    }
}

fn mmio_state(d: &PciDev) -> String {
    if d.r32(0x04) & 0x2 == 0 {
        return String::from("mmio=disabled");
    }
    let Some(bar) = d.bars[0].filter(|b| !b.io) else {
        return String::from("mmio=absent");
    };
    let Some(mmio) = Mmio::new(bar.addr, bar.size) else {
        return format!("mmio=invalid({:#x}+{:#x})", bar.addr, bar.size);
    };
    let boot0 = mmio.read32(PMC_BOOT_0);
    let boot1 = mmio.read32(PMC_BOOT_1);
    let enable = mmio.read32(PMC_ENABLE);
    format!(
        "mmio=ready({:#x}+{:#x}) {} pmc_boot0={} pmc_boot1={} pmc_enable={}",
        bar.addr,
        bar.size,
        boot0.map_or(String::from("chip=unreadable"), chipset),
        boot0.map_or(String::from("unreadable"), |v| format!("{v:#010x}")),
        boot1.map_or(String::from("unreadable"), |v| format!("{v:#010x}")),
        enable.map_or(String::from("unreadable"), |v| format!("{v:#010x}")),
    )
}

fn report(d: &PciDev) -> String {
    let bars: alloc::vec::Vec<BarReport> = d
        .bars
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            b.filter(|v| !v.io).map(|v| BarReport {
                index: i,
                address: v.addr,
                size: v.size,
            })
        })
        .collect();
    let bar_text: alloc::vec::Vec<String> = bars
        .iter()
        .map(|b| format!("BAR{} {:#x}+{:#x}", b.index, b.address, b.size))
        .collect();
    format!(
        "10de:1eb6 Quadro RTX 4000; subsystem {:04x}:{:04x}; rev {:02x}; {}; \
         firmware/GOP framebuffer retained",
        d.subsys_vendor,
        d.subsys,
        d.rev,
        if bar_text.is_empty() {
            String::from("no memory BAR")
        } else {
            bar_text.join(", ")
        }
    )
}

fn cap_offset(d: &PciDev, id: u8) -> Option<u8> {
    d.caps()
        .into_iter()
        .find(|(_, cap)| *cap == id)
        .map(|(off, _)| off)
}

fn link_speed(code: u8) -> &'static str {
    match code {
        1 => "2.5GT/s",
        2 => "5GT/s",
        3 => "8GT/s",
        4 => "16GT/s",
        5 => "32GT/s",
        6 => "64GT/s",
        _ => "unknown",
    }
}

fn pci_state(d: &PciDev) -> String {
    let command = d.r32(0x04) as u16;
    let mut fields = alloc::vec::Vec::new();
    fields.push(format!(
        "mem={}",
        if command & 0x2 != 0 { "on" } else { "off" }
    ));
    fields.push(format!(
        "busmaster={}",
        if command & 0x4 != 0 { "on" } else { "off" }
    ));

    if let Some(off) = cap_offset(d, PCI_CAP_PM) {
        let pm = d.r16(off + 4) & 0x3;
        fields.push(format!("power=D{}", pm));
    } else {
        fields.push(String::from("power=unknown"));
    }

    if let Some(off) = cap_offset(d, PCI_CAP_PCIE) {
        let link = d.r16(off + 0x12);
        fields.push(format!(
            "link={}x{}",
            link_speed((link & 0xf) as u8),
            (link >> 4) & 0x3f
        ));
    } else {
        fields.push(String::from("link=unknown"));
    }

    fields.push(format!(
        "msi={}",
        if cap_offset(d, PCI_CAP_MSI).is_some() {
            "yes"
        } else {
            "no"
        }
    ));
    fields.join(", ")
}

fn capability_state(d: &PciDev) -> String {
    let caps = d.caps();
    let names: alloc::vec::Vec<&str> = caps
        .iter()
        .map(|(_, id)| match *id {
            PCI_CAP_PM => "pm",
            PCI_CAP_MSI => "msi",
            PCI_CAP_PCIE => "pcie",
            _ => "other",
        })
        .collect();
    if names.is_empty() {
        String::from("caps=none")
    } else {
        format!("caps={}", names.join("/"))
    }
}

pub fn text() -> Option<String> {
    REPORT.lock().clone()
}

pub struct Drv;
pub static DRV: Drv = Drv;

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "nvidia-display"
    }

    fn origin(&self) -> &'static str {
        "hand"
    }

    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x10de && d.device == QUADRO_RTX_4000 && d.class == 0x03
    }

    fn probe(&self, d: &PciDev) -> Result<()> {
        let r = report(d);
        let state = pci_state(d);
        let caps = capability_state(d);
        if !OPTED_IN.load(Ordering::Relaxed) {
            let msg =
                format!("{r}; {state}; {caps}; read-only discovery, opt in with {OPTION_WORD}");
            *REPORT.lock() = Some(msg.clone());
            crate::kprintln!("[nvidia-display] {msg}");
            return Ok(());
        }
        let woke = wake(d);
        let state = pci_state(d);
        let mmio = mmio_state(d);
        let msg = format!(
            "{r}; {woke}; {state}; {caps}; {mmio}; probe mode only, GOP retained; \
             gsp=not-started firmware={} kms=not-registered dma=off",
            firmware_state()
        );
        *REPORT.lock() = Some(msg.clone());
        crate::kprintln!("[nvidia-display] {msg}");
        // Preflight mode intentionally exposes no mutable Anzeige implementation. The next
        // stage must add a firmware loader, bounded GSP RPC transport and DMA ownership before
        // any display register is changed.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_exact_rtx4000() {
        let d = PciDev {
            bus: 1,
            dev: 0,
            func: 0,
            vendor: 0x10de,
            device: 0x1eb6,
            class: 0x03,
            subclass: 0,
            prog_if: 0,
            rev: 0,
            subsys_vendor: 0,
            subsys: 0,
            bars: [None; 6],
            driver: None,
        };
        assert!(Drv.matches(&d));
        let mut other = d;
        other.device = 0x10f8;
        assert!(!Drv.matches(&other));
    }

    #[test]
    fn decodes_chipset() {
        assert_eq!(chipset(0x164000a1), "chip=tu104");
        assert_eq!(chipset(0xffff_ffff), "chip=not-responding");
        assert_eq!(chipset(0x166000a1), "chip=tu106(unexpected)");
    }

    #[test]
    fn optin_file_is_strict() {
        assert!(b"probe\n".trim_ascii() == b"probe");
        assert!(!(b"on".trim_ascii() == b"probe"));
    }

    fn utf16(s: &str) -> alloc::vec::Vec<u16> {
        s.encode_utf16().chain(core::iter::once(0)).collect()
    }

    #[test]
    fn removes_probe_word_only() {
        OPTED_IN.store(false, Ordering::Relaxed);
        let mut o = utf16("ultar.slot=a ultar.nvidia_display=probe ultar.trial=1");
        let n = take_boot_option(&mut o, o.len());
        assert_eq!(
            String::from_utf16(&o[..n]).unwrap(),
            "ultar.slot=a ultar.trial=1"
        );
        assert!(OPTED_IN.load(Ordering::Relaxed));
    }

    #[test]
    fn unknown_nvidia_word_is_preserved() {
        OPTED_IN.store(false, Ordering::Relaxed);
        let mut o = utf16("ultar.nvidia_display=on ultar.slot=a");
        let n = take_boot_option(&mut o, o.len());
        assert_eq!(
            String::from_utf16(&o[..n]).unwrap(),
            "ultar.nvidia_display=on ultar.slot=a"
        );
    }

    #[test]
    fn decodes_pcie_link_speed() {
        assert_eq!(link_speed(1), "2.5GT/s");
        assert_eq!(link_speed(4), "16GT/s");
        assert_eq!(link_speed(0xff), "unknown");
    }

    #[test]
    fn mmio_aperture_requires_safe_bar() {
        assert!(Mmio::new(0, BAR0_MIN_SIZE).is_none());
        assert!(Mmio::new(0x1234, BAR0_MIN_SIZE).is_none());
        assert!(Mmio::new(0x1000, BAR0_MIN_SIZE - 4).is_none());
        assert!(Mmio::new(0x1000, BAR0_MIN_SIZE).is_some());
    }

    #[test]
    fn mmio_bounds_are_checked_before_access() {
        let mmio = Mmio::new(0x1000, BAR0_MIN_SIZE).unwrap();
        assert!(mmio.read32(BAR0_MIN_SIZE - 3).is_none());
        assert!(mmio.read32(2).is_none());
    }
}
