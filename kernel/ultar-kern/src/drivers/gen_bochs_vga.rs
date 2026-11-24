//! Ultar-Kern – Anzeigetreiber „gen-bochs-vga“ (Treiber-Werkstatt).
//! Gerät: Bochs/QEMU-Standardgrafik (1234:1111) und VirtualBox-VGA (80ee:beef) · Norm: Bochs VBE-DISPI
//! (öffentlich dokumentiert): Register ID/XRES/YRES/BPP/ENABLE/VIRT_WIDTH über MMIO (BAR2 + 0x500) oder
//! E/A-Ports 0x1CE/0x1CF, linearer Bildspeicher in BAR0.
//! Zweck: erster eigener Anzeigetreiber hinter der Anzeige-Schicht (grafik.rs) – Auflösungen umschalten in
//! virtuellen Maschinen (PC-Ausgabe). Prüfung: QEMU `-vga std`, Konsole `anzeige 1920x1080`.

use crate::driver::{Driver, Result};
use crate::efi::Framebuffer;
use crate::grafik::Anzeige;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Drv;
pub static DRV: Drv = Drv;

const ID: u16 = 0;
const XRES: u16 = 1;
const YRES: u16 = 2;
const BPP: u16 = 3;
const ENABLE: u16 = 4;
const VIRT_W: u16 = 6;
const VIRT_H: u16 = 7;
const X_OFF: u16 = 8;
const Y_OFF: u16 = 9;

const MODI: &[(u32, u32)] = &[
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1280, 1024),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
    (2560, 1440),
];

struct Bochs {
    fb: u64,
    fb_size: u64,
    mmio: Option<u64>,
}

impl Bochs {
    fn wr(&self, reg: u16, v: u16) {
        match self.mmio {
            Some(m) => unsafe {
                core::ptr::write_volatile((m + 0x500 + reg as u64 * 2) as *mut u16, v)
            },
            None => unsafe {
                core::arch::asm!("out dx, ax", in("dx") 0x1ceu16, in("ax") reg, options(nomem, nostack));
                core::arch::asm!("out dx, ax", in("dx") 0x1cfu16, in("ax") v, options(nomem, nostack));
            },
        }
    }
    fn rd(&self, reg: u16) -> u16 {
        match self.mmio {
            Some(m) => unsafe {
                core::ptr::read_volatile((m + 0x500 + reg as u64 * 2) as *const u16)
            },
            None => unsafe {
                let v: u16;
                core::arch::asm!("out dx, ax", in("dx") 0x1ceu16, in("ax") reg, options(nomem, nostack));
                core::arch::asm!("in ax, dx", in("dx") 0x1cfu16, out("ax") v, options(nomem, nostack));
                v
            },
        }
    }
}

impl Anzeige for Bochs {
    fn name(&self) -> &'static str {
        "gen-bochs-vga"
    }
    fn herkunft(&self) -> &'static str {
        "werkstatt"
    }
    fn modi(&self) -> Vec<(u32, u32)> {
        MODI.iter()
            .copied()
            .filter(|(w, h)| (*w as u64 * *h as u64 * 4) <= self.fb_size)
            .collect()
    }
    fn setzen(&mut self, w: u32, h: u32) -> core::result::Result<Framebuffer, String> {
        if !(320..=4096).contains(&w)
            || !(200..=4096).contains(&h)
            || w as u64 * h as u64 * 4 > self.fb_size
        {
            return Err(format!(
                "{w}x{h} passt nicht in den Bildspeicher ({} MB)",
                self.fb_size >> 20
            ));
        }
        self.wr(ENABLE, 0);
        self.wr(XRES, w as u16);
        self.wr(YRES, h as u16);
        self.wr(BPP, 32);
        self.wr(VIRT_W, w as u16);
        self.wr(VIRT_H, h as u16);
        self.wr(X_OFF, 0);
        self.wr(Y_OFF, 0);
        self.wr(ENABLE, 0x01 | 0x40); // an, linearer Bildspeicher
        if self.rd(XRES) != w as u16 || self.rd(YRES) != h as u16 {
            return Err(format!("Gerät lehnt {w}x{h} ab"));
        }
        Ok(Framebuffer {
            base: self.fb,
            width: w,
            height: h,
            stride: w,
            bgr: true,
        })
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-bochs-vga"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x03
            && ((d.vendor == 0x1234 && d.device == 0x1111)
                || (d.vendor == 0x80ee && d.device == 0xbeef))
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let fb = d.bars[0]
            .filter(|b| !b.io)
            .ok_or("BAR0 (Bildspeicher) fehlt")?;
        d.enable();
        let mmio = d.bars[2]
            .filter(|b| !b.io && b.size >= 0x600)
            .map(|b| b.addr);
        let b = Bochs {
            fb: fb.addr,
            fb_size: fb.size,
            mmio,
        };
        let id = b.rd(ID);
        if !(0xb0c0..=0xb0cf).contains(&id) {
            return Err(format!("keine VBE-DISPI-Kennung ({id:#06x})"));
        }
        crate::kprintln!(
            "[bochs-vga] DISPI {id:#06x}, Bildspeicher {} MB, Register über {}",
            fb.size >> 20,
            if mmio.is_some() { "MMIO" } else { "E/A-Ports" }
        );
        crate::grafik::anmelden(Box::new(b));
        Ok(())
    }
}
