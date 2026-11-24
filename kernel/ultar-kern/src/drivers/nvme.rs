//! NVMe: SSDs in Laptops, PCs und Servern (PCI-Klasse 01/08/02). Eigene Umsetzung nach NVM Express Base 2.0:
//! Admin-Warteschlange, Identify, eine E/A-Warteschlange, Lesen über PRP-Listen.

use crate::driver::{BlockDev, Driver, Result, BLOCK};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::ptr::{read_volatile, write_volatile};

pub struct Drv;
pub static DRV: Drv = Drv;

const QN: u16 = 64;

struct Q {
    sq: u64,
    cq: u64,
    sq_tail: u16,
    cq_head: u16,
    phase: bool,
    sq_db: u64,
    cq_db: u64,
}

struct Nvme {
    name: String,
    admin: Q,
    io: Q,
    nsid: u32,
    lba_size: usize,
    sectors: u64,
    data: u64,
    prp: u64,
    cid: u16,
}

unsafe fn r32(a: u64) -> u32 {
    read_volatile(a as *const u32)
}
unsafe fn w32(a: u64, v: u32) {
    write_volatile(a as *mut u32, v)
}
unsafe fn r64(a: u64) -> u64 {
    r32(a) as u64 | (r32(a + 4) as u64) << 32
}
unsafe fn w64(a: u64, v: u64) {
    w32(a, v as u32);
    w32(a + 4, (v >> 32) as u32);
}

impl Q {
    fn new(bar: u64, qid: u16, stride: u64) -> Option<Q> {
        Some(Q {
            sq: alloc_pages(1)?,
            cq: alloc_pages(1)?,
            sq_tail: 0,
            cq_head: 0,
            phase: true,
            sq_db: bar + 0x1000 + (2 * qid as u64) * stride,
            cq_db: bar + 0x1000 + (2 * qid as u64 + 1) * stride,
        })
    }
    /// Befehl (16 Doppelwörter) einstellen und auf die Erledigung warten. Liefert (Status, DW0).
    fn run(&mut self, cmd: &[u32; 16]) -> Result<u32> {
        unsafe {
            let e = self.sq + self.sq_tail as u64 * 64;
            for (i, v) in cmd.iter().enumerate() {
                w32(e + i as u64 * 4, *v);
            }
            self.sq_tail = (self.sq_tail + 1) % QN;
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            w32(self.sq_db, self.sq_tail as u32);
            let end = crate::arch::uptime_ms() + 3000;
            loop {
                let c = self.cq + self.cq_head as u64 * 16;
                let st = read_volatile((c + 14) as *const u16);
                if (st & 1 == 1) == self.phase {
                    let dw0 = r32(c);
                    self.cq_head = (self.cq_head + 1) % QN;
                    if self.cq_head == 0 {
                        self.phase = !self.phase;
                    }
                    w32(self.cq_db, self.cq_head as u32);
                    let code = st >> 1;
                    return if code == 0 {
                        Ok(dw0)
                    } else {
                        Err(format!("NVMe-Fehler {:#x}", code))
                    };
                }
                if crate::arch::uptime_ms() > end {
                    return Err("NVMe: Zeitüberschreitung".into());
                }
                crate::arch::pause();
            }
        }
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "nvme"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x01 && d.subclass == 0x08 && d.prog_if == 0x02
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        d.enable();
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?.addr;
        unsafe {
            let cap = r64(bar);
            let stride = 4u64 << ((cap >> 32) & 0xf);
            let timeout = ((cap >> 24) & 0xff) * 500 + 1000;
            // abschalten und warten
            w32(bar + 0x14, r32(bar + 0x14) & !1);
            let t = crate::arch::uptime_ms() + timeout;
            while r32(bar + 0x1c) & 1 != 0 {
                if crate::arch::uptime_ms() > t {
                    return Err("Controller lässt sich nicht abschalten".into());
                }
            }
            let mut admin = Q::new(bar, 0, stride).ok_or("kein Speicher")?;
            w32(bar + 0x24, ((QN as u32 - 1) << 16) | (QN as u32 - 1));
            w64(bar + 0x28, admin.sq);
            w64(bar + 0x30, admin.cq);
            // EN, NVM-Befehlssatz, 4-KB-Seiten, SQ-Eintrag 64 B (2^6), CQ-Eintrag 16 B (2^4)
            w32(bar + 0x14, 1 | (6 << 16) | (4 << 20));
            let t = crate::arch::uptime_ms() + timeout;
            while r32(bar + 0x1c) & 1 == 0 {
                if r32(bar + 0x1c) & 2 != 0 {
                    return Err("Controller meldet schweren Fehler".into());
                }
                if crate::arch::uptime_ms() > t {
                    return Err("Controller wird nicht bereit".into());
                }
            }
            let data = alloc_pages(17).ok_or("kein Speicher")?; // 64 KB Daten + 4 KB Identify
            let ident = data + 16 * 4096;
            // Identify Controller (CNS 1): Modell
            let mut c = [0u32; 16];
            c[0] = 0x06 | (1 << 16);
            c[6] = ident as u32;
            c[7] = (ident >> 32) as u32;
            c[10] = 1;
            admin.run(&c)?;
            let model_raw = core::slice::from_raw_parts((ident + 24) as *const u8, 40);
            let model =
                alloc::string::ToString::to_string(String::from_utf8_lossy(model_raw).trim());
            // Namensraum 1 (CNS 0): Größe und Blockgröße
            c = [0u32; 16];
            c[0] = 0x06 | (2 << 16);
            c[1] = 1;
            c[6] = ident as u32;
            c[7] = (ident >> 32) as u32;
            c[10] = 0;
            admin.run(&c)?;
            let nsze = r64(ident);
            let flbas = read_volatile((ident + 26) as *const u8) & 0xf;
            let lbaf = r32(ident + 128 + flbas as u64 * 4);
            let lba_size = 1usize << ((lbaf >> 16) & 0xff);
            // E/A-Warteschlange 1: erst Abschluss-, dann Übergabe-Schlange
            let io = Q::new(bar, 1, stride).ok_or("kein Speicher")?;
            c = [0u32; 16];
            c[0] = 0x05 | (3 << 16);
            c[6] = io.cq as u32;
            c[7] = (io.cq >> 32) as u32;
            c[10] = ((QN as u32 - 1) << 16) | 1;
            c[11] = 1; // physisch zusammenhängend, keine Interrupts
            admin.run(&c)?;
            c = [0u32; 16];
            c[0] = 0x01 | (4 << 16);
            c[6] = io.sq as u32;
            c[7] = (io.sq >> 32) as u32;
            c[10] = ((QN as u32 - 1) << 16) | 1;
            c[11] = (1 << 16) | 1; // gehört zu CQ 1, zusammenhängend
            admin.run(&c)?;
            let prp = alloc_pages(1).ok_or("kein Speicher")?;
            let mut bl = BLOCK.lock();
            let name = format!(
                "nvme{}n1",
                bl.iter().filter(|b| b.name().starts_with("nvme")).count()
            );
            crate::kprintln!(
                "[nvme] {name}: {model}, {} MB, Block {lba_size} B",
                nsze * lba_size as u64 / (1 << 20)
            );
            bl.push(Box::new(Nvme {
                name,
                admin,
                io,
                nsid: 1,
                lba_size,
                sectors: nsze,
                data,
                prp,
                cid: 10,
            }));
        }
        Ok(())
    }
}

impl BlockDev for Nvme {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn sector_size(&self) -> usize {
        self.lba_size
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        let per = 64 * 1024 / self.lba_size;
        for (i, chunk) in buf.chunks_mut(64 * 1024).enumerate() {
            self.rw(0x02, lba + (i * per) as u64, chunk.len())?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.data as *const u8,
                    chunk.as_mut_ptr(),
                    chunk.len(),
                )
            };
        }
        let _ = &self.admin;
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<()> {
        let per = 64 * 1024 / self.lba_size;
        for (i, chunk) in buf.chunks(64 * 1024).enumerate() {
            unsafe {
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.data as *mut u8, chunk.len())
            };
            self.rw(0x01, lba + (i * per) as u64, chunk.len())?;
        }
        Ok(())
    }
    /// NVM command Flush (opcode 0x00, NVMe 2.0 §7.1): volatile write cache to media.
    fn flush(&mut self) -> Result<()> {
        self.cid = self.cid.wrapping_add(1);
        let mut c = [0u32; 16];
        c[0] = (self.cid as u32) << 16;
        c[1] = self.nsid;
        self.io.run(&c)?;
        Ok(())
    }
}

impl Nvme {
    /// Lese-/Schreibbefehl (0x02/0x01) über den DMA-Puffer
    fn rw(&mut self, op: u32, start: u64, len: usize) -> Result<()> {
        let blocks = len.div_ceil(self.lba_size);
        let pages = len.div_ceil(4096);
        self.cid = self.cid.wrapping_add(1);
        let mut c = [0u32; 16];
        c[0] = op | ((self.cid as u32) << 16);
        c[1] = self.nsid;
        c[6] = self.data as u32;
        c[7] = (self.data >> 32) as u32;
        // zweite Seite direkt in PRP2, mehr Seiten über eine PRP-Liste
        let prp2 = match pages {
            0 | 1 => 0,
            2 => self.data + 4096,
            _ => {
                for p in 1..pages {
                    unsafe { w64(self.prp + (p as u64 - 1) * 8, self.data + p as u64 * 4096) };
                }
                self.prp
            }
        };
        c[8] = prp2 as u32;
        c[9] = (prp2 >> 32) as u32;
        c[10] = start as u32;
        c[11] = (start >> 32) as u32;
        c[12] = (blocks - 1) as u32;
        self.io.run(&c)?;
        Ok(())
    }
}
