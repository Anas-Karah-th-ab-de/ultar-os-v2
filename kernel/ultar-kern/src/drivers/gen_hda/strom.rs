//! Running sound streams of gen-hda for `/dev/ultar/pcm` (svcdev/pcm.rs): one output and one
//! input stream, 48 kHz / 16 bit / stereo, each a ring of 64 KiB (≈ 341 ms) that the controller
//! walks round and round (Abschnitt 3.3.35–3.3.42, Pufferliste mit 4 Einträgen, ohne Meldungen).
//! The position comes from LPIB (Link Position in Buffer) whenever a program reads or writes –
//! a caller every 20 ms is far inside the ring.
//!
//! * output: what is played is cleared behind the controller, so nothing old sounds again; a
//!   writer that falls behind gets 50 ms of silence in front, one that runs ahead more than 200 ms
//!   is told to wait;
//! * input: a reader that falls behind more than the ring loses the oldest part, never the newest.

use super::{r32, r8, w16, w32, w8, wait, Hda, FORMAT};

/// Bytes of one ring.
pub(super) const RING: usize = 16 * 4096;
/// Pages of one ring.
pub(super) const RING_PAGES: usize = RING / 4096;
/// Entries of a ring's buffer list.
const ENTRIES: usize = 4;
/// Silence put in front after the writer fell behind (50 ms).
const LEAD: u64 = 9600;
/// Most a writer may run ahead (200 ms).
const AHEAD: u64 = 38_400;
/// Stream tags (any number 1..15; output and input count separately).
const TAG_OUT: u8 = 1;
const TAG_IN: u8 = 2;

/// One ring and where the controller and the program are in it (counted from the start).
pub(super) struct Ring {
    /// Stream descriptor registers.
    pub sd: u64,
    /// Buffer list (one page).
    pub bdl: u64,
    /// The ring itself.
    pub buf: u64,
    last: usize,
    hw: u64,
    sw: u64,
}

impl Ring {
    pub(super) fn new(sd: u64, bdl: u64, buf: u64) -> Self {
        Self { sd, bdl, buf, last: 0, hw: 0, sw: 0 }
    }

    /// Stops the stream, sets it up as a ring and lets it run.
    fn start(&mut self, tag: u8) {
        let sd = self.sd;
        w8(sd, r8(sd) & !2);
        wait(|| r8(sd) & 2 == 0, 20);
        w8(sd, 1);
        wait(|| r8(sd) & 1 != 0, 20);
        w8(sd, 0);
        wait(|| r8(sd) & 1 == 0, 20);
        let part = (RING / ENTRIES) as u64;
        for i in 0..ENTRIES {
            // SAFETY: the buffer list is one page of this driver, ENTRIES · 16 bytes fit in it.
            unsafe {
                let e = (self.bdl as *mut u64).add(i * 2);
                e.write_volatile(self.buf + i as u64 * part);
                e.add(1).write_volatile(part);
            }
        }
        // SAFETY: the ring is RING bytes of this driver's own pages.
        unsafe { core::ptr::write_bytes(self.buf as *mut u8, 0, RING) };
        w32(sd + 0x18, self.bdl as u32);
        w32(sd + 0x1c, (self.bdl >> 32) as u32);
        w32(sd + 0x08, RING as u32);
        w16(sd + 0x0c, (ENTRIES - 1) as u16);
        w16(sd + 0x12, FORMAT);
        w8(sd + 2, tag << 4);
        w8(sd + 3, 0x1c);
        w8(sd, 2);
        self.last = 0;
        self.hw = 0;
        self.sw = 0;
    }

    fn stop(&self) {
        w8(self.sd, r8(self.sd) & !2);
        wait(|| r8(self.sd) & 2 == 0, 20);
    }

    /// Follows the controller; returns the ring range it passed since the last look.
    fn advance(&mut self) -> (usize, usize) {
        let lpib = r32(self.sd + 4) as usize % RING;
        let passed = (lpib + RING - self.last) % RING;
        let from = self.last;
        self.last = lpib;
        self.hw += passed as u64;
        (from, passed)
    }

    fn zero(&self, at: usize, n: usize) {
        let first = n.min(RING - at);
        // SAFETY: both ranges lie inside the ring.
        unsafe {
            core::ptr::write_bytes((self.buf + at as u64) as *mut u8, 0, first);
            core::ptr::write_bytes(self.buf as *mut u8, 0, n - first);
        }
    }

    fn copy_in(&self, at: usize, data: &[u8]) {
        let first = data.len().min(RING - at);
        // SAFETY: both ranges lie inside the ring; `data` is a separate buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), (self.buf + at as u64) as *mut u8, first);
            core::ptr::copy_nonoverlapping(data.as_ptr().add(first), self.buf as *mut u8, data.len() - first);
        }
    }

    fn copy_out(&self, at: usize, out: &mut [u8]) {
        let first = out.len().min(RING - at);
        // SAFETY: both ranges lie inside the ring; `out` is a separate buffer.
        unsafe {
            core::ptr::copy_nonoverlapping((self.buf + at as u64) as *const u8, out.as_mut_ptr(), first);
            core::ptr::copy_nonoverlapping(self.buf as *const u8, out.as_mut_ptr().add(first), out.len() - first);
        }
    }

    /// Output: queues as much of `data` as fits; returns the bytes taken (whole frames).
    fn write(&mut self, data: &[u8]) -> usize {
        let (from, passed) = self.advance();
        self.zero(from, passed); // played: never again
        if self.sw < self.hw {
            // the writer fell behind: silence first, then its sound
            self.sw = self.hw + LEAD;
            self.zero((self.hw % RING as u64) as usize, LEAD as usize);
        }
        let queued = self.sw - self.hw;
        let n = (data.len() as u64).min(AHEAD.saturating_sub(queued)) as usize & !3;
        self.copy_in((self.sw % RING as u64) as usize, &data[..n]);
        self.sw += n as u64;
        n
    }

    /// Input: bytes ready to read.
    fn ready(&mut self) -> u64 {
        self.advance();
        if self.hw - self.sw > (RING - 4096) as u64 {
            self.sw = self.hw - LEAD; // too far behind: the oldest part is gone
        }
        self.hw - self.sw
    }

    /// Input: reads what is there (whole frames).
    fn read(&mut self, out: &mut [u8]) -> usize {
        let n = (self.ready() as usize).min(out.len()) & !3;
        self.copy_out((self.sw % RING as u64) as usize, &mut out[..n]);
        self.sw += n as u64;
        n
    }
}

impl Hda {
    /// Starts both streams (the input only if the codec has a way from a microphone or line in).
    pub(super) fn pcm_start(&mut self) {
        let _ = self.cmd(self.dac, 0x2_0000 | FORMAT as u32);
        let _ = self.cmd(self.dac, 0x7_0600 | (TAG_OUT as u32) << 4);
        self.out.start(TAG_OUT);
        if let Some(adc) = self.adc {
            let _ = self.cmd(adc, 0x2_0000 | FORMAT as u32);
            let _ = self.cmd(adc, 0x7_0600 | (TAG_IN as u32) << 4);
            if let Some(r) = self.inn.as_mut() {
                r.start(TAG_IN);
            }
        }
    }

    /// Stops both streams.
    pub(super) fn pcm_stop(&mut self) {
        self.out.stop();
        if let Some(r) = self.inn.as_ref() {
            r.stop();
        }
    }

    pub(super) fn pcm_write(&mut self, data: &[u8]) -> usize {
        self.out.write(data)
    }

    pub(super) fn pcm_read(&mut self, out: &mut [u8]) -> usize {
        self.inn.as_mut().map_or(0, |r| r.read(out))
    }

    pub(super) fn pcm_readable(&mut self) -> bool {
        self.inn.as_mut().is_some_and(|r| r.ready() >= 4)
    }

    pub(super) fn pcm_writable(&mut self) -> bool {
        let (from, passed) = self.out.advance();
        self.out.zero(from, passed);
        self.out.sw < self.out.hw + AHEAD
    }

    /// Way into the input: an audio input (ADC) and a pin with input (microphone first, then
    /// line in, then any) – the input amplifiers on the way at 0 dB, not muted.
    pub(super) fn ein_einrichten(&mut self, fg: u8) -> Option<(u8, u8)> {
        let knoten = self.param(fg, 0x04).ok()?;
        let (start, anzahl) = ((knoten >> 16) as u8, knoten as u8);
        let mut adcs = alloc::vec::Vec::new();
        for nid in start..start.saturating_add(anzahl) {
            if (self.param(nid, 0x09).unwrap_or(0) >> 20) & 0xf == 1 {
                adcs.push(nid);
            }
        }
        for want in [0xa, 0x8, 0xff] {
            for &adc in &adcs {
                let mut weg = alloc::vec::Vec::new();
                if self.suche_ein(adc, 5, want, &mut weg) {
                    let pin = *weg.last()?;
                    for &n in &weg {
                        self.laut(n, fg);
                    }
                    // the ADC's own input amplifiers (laut() handles mixers and selectors)
                    if self.param(adc, 0x09).unwrap_or(0) & (1 << 1) != 0 {
                        let a = self.param(adc, 0x0d).or_else(|_| self.param(fg, 0x0d)).unwrap_or(0);
                        let n = self.verbindungen(adc).len().clamp(1, 16) as u32;
                        for i in 0..n {
                            let _ = self.cmd(adc, 0x3_0000 | 0x7000 | i << 8 | (a & 0x7f));
                        }
                    }
                    // pin: input on; a microphone gets its bias (VRef 80 %)
                    let vref = if want == 0xa { 0x04 } else { 0 };
                    let _ = self.cmd(pin, 0x7_0700 | 0x20 | vref);
                    return Some((adc, pin));
                }
            }
        }
        None
    }

    /// Depth search from an ADC towards an input pin of device kind `want` (`0xff`: any).
    fn suche_ein(&mut self, nid: u8, tiefe: u8, want: u32, weg: &mut alloc::vec::Vec<u8>) -> bool {
        let typ = (self.param(nid, 0x09).unwrap_or(0xf0_0000) >> 20) & 0xf;
        weg.push(nid);
        if typ == 4 {
            let input = self.param(nid, 0x0c).unwrap_or(0) & (1 << 5) != 0;
            let cfg = self.cmd(nid, 0xF_1C00).unwrap_or(0);
            let kind = (cfg >> 20) & 0xf;
            if input && cfg >> 30 != 1 && (want == 0xff || kind == want) {
                return true;
            }
        } else if tiefe > 0 && typ != 0 {
            let c = self.verbindungen(nid);
            for (i, &n) in c.iter().enumerate() {
                if self.suche_ein(n, tiefe - 1, want, weg) {
                    if c.len() > 1 && typ != 2 {
                        let _ = self.cmd(nid, 0x7_0100 | i as u32);
                    }
                    return true;
                }
            }
        }
        weg.pop();
        false
    }
}
