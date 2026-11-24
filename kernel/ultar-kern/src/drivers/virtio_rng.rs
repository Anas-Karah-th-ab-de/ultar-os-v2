//! virtio-rng (virtio 1.2, Abschnitt 5.4): Zufall vom Wirt. Eine Warteschlange, der Wirt füllt angebotene Puffer.
//! Der Kern mischt den Zufall beim Start in seinen Vorrat (`arch::zufall_mischen`) und holt bei Bedarf nach.

use super::virtio::{device_type, Buf, Queue, Transport};
use crate::driver::{Driver, Result};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use spin::Mutex;

pub struct Drv;
pub static DRV: Drv = Drv;

struct Rng {
    q: Queue,
    buf: u64,
}

static RNG: Mutex<Option<Rng>> = Mutex::new(None);

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "virtio-rng"
    }
    fn matches(&self, d: &PciDev) -> bool {
        device_type(d) == Some(4)
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let t = Transport::new(d)?;
        t.negotiate(0)?;
        let q = t.queue(0, 8)?;
        t.ready();
        let buf = alloc_pages(1).ok_or("kein Speicher")?;
        *RNG.lock() = Some(Rng { q, buf });
        let mut z = [0u8; 32];
        let n = fuelle(&mut z);
        if n == 0 {
            *RNG.lock() = None;
            return Err("Wirt liefert keinen Zufall".into());
        }
        for c in z.chunks(8) {
            crate::arch::zufall_mischen(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        crate::kprintln!("[virtio-rng] rng0: {n} Byte Zufall vom Wirt in den Vorrat gemischt");
        Ok(())
    }
}

/// Puffer mit Zufall vom Wirt füllen; liefert die Zahl der gefüllten Bytes (0 = kein Gerät/keine Antwort)
pub fn fuelle(aus: &mut [u8]) -> usize {
    let mut g = RNG.lock();
    let Some(r) = g.as_mut() else { return 0 };
    let mut fertig = 0;
    while fertig < aus.len() {
        let want = (aus.len() - fertig).min(4096) as u32;
        r.q.submit(&[Buf { addr: r.buf, len: want, write: true }]);
        r.q.kick();
        let Some((_, n)) = r.q.wait(200) else { break };
        let n = (n.min(want)) as usize;
        if n == 0 {
            break;
        }
        unsafe { core::ptr::copy_nonoverlapping(r.buf as *const u8, aus[fertig..].as_mut_ptr(), n) };
        fertig += n;
    }
    fertig
}
