//! Safe discovery of NVIDIA display hardware.
//!
//! This stage is intentionally read-only. It identifies the known Quadro RTX 4000
//! class, records BARs and PCIe state, and keeps the UEFI/GOP framebuffer active.
//! NVIDIA MMIO, GSP, VRAM and display registers are not touched.

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
        let stop = options[read..end].iter().position(|&u| u == b' ' as u16).map_or(end, |v| read + v);
        let word = read..stop;
        read = stop + 1;
        if !word.is_empty() && OPTION_WORD.encode_utf16().eq(options[word.clone()].iter().copied()) {
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

fn report(d: &PciDev) -> String {
    let bars: alloc::vec::Vec<BarReport> = d.bars.iter().enumerate().filter_map(|(i, b)| {
        b.filter(|v| !v.io).map(|v| BarReport { index: i, address: v.addr, size: v.size })
    }).collect();
    let bar_text: alloc::vec::Vec<String> = bars.iter().map(|b| {
        format!("BAR{} {:#x}+{:#x}", b.index, b.address, b.size)
    }).collect();
    format!(
        "10de:1eb6 Quadro RTX 4000; subsystem {:04x}:{:04x}; rev {:02x}; {}; \
         firmware/GOP framebuffer retained",
        d.subsys_vendor,
        d.subsys,
        d.rev,
        if bar_text.is_empty() { String::from("no memory BAR") } else { bar_text.join(", ") }
    )
}

fn cap_offset(d: &PciDev, id: u8) -> Option<u8> {
    d.caps().into_iter().find(|(_, cap)| *cap == id).map(|(off, _)| off)
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
    fields.push(format!("mem={}", if command & 0x2 != 0 { "on" } else { "off" }));
    fields.push(format!("busmaster={}", if command & 0x4 != 0 { "on" } else { "off" }));

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
        if cap_offset(d, PCI_CAP_MSI).is_some() { "yes" } else { "no" }
    ));
    fields.join(", ")
}

fn capability_state(d: &PciDev) -> String {
    let caps = d.caps();
    let names: alloc::vec::Vec<&str> = caps.iter().map(|(_, id)| match *id {
        PCI_CAP_PM => "pm",
        PCI_CAP_MSI => "msi",
        PCI_CAP_PCIE => "pcie",
        _ => "other",
    }).collect();
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
            let msg = format!("{r}; {state}; {caps}; read-only discovery, opt in with {OPTION_WORD}");
            *REPORT.lock() = Some(msg.clone());
            crate::kprintln!("[nvidia-display] {msg}");
            return Ok(());
        }
        let msg = format!(
            "{r}; {state}; {caps}; probe mode only, GOP retained; \
             gsp=not-started kms=not-registered"
        );
        *REPORT.lock() = Some(msg.clone());
        crate::kprintln!("[nvidia-display] {msg}");
        // Probe mode intentionally exposes no mutable Anzeige implementation.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_exact_rtx4000() {
        let d = PciDev {
            bus: 1, dev: 0, func: 0,
            vendor: 0x10de, device: 0x1eb6,
            class: 0x03, subclass: 0, prog_if: 0, rev: 0,
            subsys_vendor: 0, subsys: 0,
            bars: [None; 6], driver: None,
        };
        assert!(Drv.matches(&d));
        let mut other = d;
        other.device = 0x10f8;
        assert!(!Drv.matches(&other));
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
}
