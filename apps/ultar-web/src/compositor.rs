//! Compositor-Thread (wie der Viz-/GPU-Prozess bei Chrome): setzt Seiteninhalt und Browserleisten zu einem
//! Bild zusammen und gibt es aus. Läuft getrennt vom Hauptthread, damit WebKit-Ereignisse nie auf die
//! Bildausgabe warten. Ältere Bilder werden übersprungen (nur das neueste zählt), der Puffer geht sofort
//! an WebKit zurück, damit der nächste Frame ohne Wartezeit gezeichnet werden kann.

use crate::platform::Frame;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub enum Msg {
    Content(Frame),
    /// Leisten/Overlays: nur die Bereiche, die den Leisten gehören (mit ihren Pixeln), und der Inhaltsbereich
    Chrome {
        pieces: Vec<(IRect, Vec<u32>)>,
        content: IRect,
    },
    /// Vorschaubild des aktuellen Inhalts anfordern (für die Tab-Übersicht)
    Thumb(u64),
    /// Bildschirmfoto als BMP schreiben (Test/Hilfe)
    Shot(String),
    Quit,
}

pub enum Out {
    Done {
        view: usize,
        buffer: usize,
    },
    Thumb {
        tag: u64,
        w: i32,
        h: i32,
        px: Vec<u32>,
    },
    Shot(String),
}

/// Kennzahlen für ultar://leistung
#[derive(Default)]
pub struct Stats {
    pub frames: AtomicU64,
    pub skipped: AtomicU64,
    /// Summe/letzter/größter Wert für Zusammensetzen + Ausgabe in µs
    pub sum_us: AtomicU64,
    pub last_us: AtomicU64,
    pub max_us: AtomicU64,
}

pub trait Screen: Send {
    fn size(&self) -> (i32, i32);
    fn present(&mut self, px: &[u32], w: i32);
}

/// Bildschirm ohne Ausgabe (Testlauf mit --shot)
pub struct NullScreen(pub i32, pub i32);
impl Screen for NullScreen {
    fn size(&self) -> (i32, i32) {
        (self.0, self.1)
    }
    fn present(&mut self, _px: &[u32], _w: i32) {}
}

pub fn run(
    mut scr: Box<dyn Screen>,
    rx: Receiver<Msg>,
    tx: Sender<Out>,
    wake: impl Fn() + Send + 'static,
    stats: Arc<Stats>,
    bg: u32,
) {
    crate::sched::pin_current_to_big();
    let (w, h) = scr.size();
    // Inhaltsebene (letztes Seitenbild) und Ausgabebild (Inhalt + Leisten)
    let mut base = vec![bg; (w * h) as usize];
    let mut out = vec![bg; (w * h) as usize];
    let mut chrome: Vec<(IRect, Vec<u32>)> = Vec::new();
    let mut content = IRect { x: 0, y: 0, w, h };
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut newest: Option<Frame> = None;
        let mut chrome_changed = false;
        let mut thumbs = Vec::new();
        let mut shots = Vec::new();
        let mut quit = false;
        let mut handle = |m: Msg, newest: &mut Option<Frame>| match m {
            Msg::Content(f) => {
                if let Some(old) = newest.replace(f) {
                    stats.skipped.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(Out::Done {
                        view: old.view,
                        buffer: old.buffer,
                    });
                }
            }
            Msg::Chrome { pieces, content: c } => {
                chrome = pieces;
                content = c;
                chrome_changed = true;
            }
            Msg::Thumb(t) => thumbs.push(t),
            Msg::Shot(p) => shots.push(p),
            Msg::Quit => quit = true,
        };
        handle(first, &mut newest);
        while let Ok(m) = rx.try_recv() {
            handle(m, &mut newest);
        }
        if quit {
            if let Some(f) = newest {
                let _ = tx.send(Out::Done {
                    view: f.view,
                    buffer: f.buffer,
                });
            }
            wake();
            return;
        }
        let t0 = Instant::now();
        if let Some(f) = newest {
            blit_frame(&mut base, w, h, content, &f, bg);
            // Puffer sofort zurück: WebKit kann schon am nächsten Bild arbeiten, während wir ausgeben
            let _ = tx.send(Out::Done {
                view: f.view,
                buffer: f.buffer,
            });
            wake();
            chrome_changed = true;
        }
        if chrome_changed {
            out.copy_from_slice(&base);
            for (r, px) in &chrome {
                put_piece(&mut out, px, w, h, *r);
            }
            scr.present(&out, w);
            let us = t0.elapsed().as_micros() as u64;
            stats.frames.fetch_add(1, Ordering::Relaxed);
            stats.sum_us.fetch_add(us, Ordering::Relaxed);
            stats.last_us.store(us, Ordering::Relaxed);
            stats.max_us.fetch_max(us, Ordering::Relaxed);
        }
        for tag in thumbs {
            let (tw, th, px) = thumb(&base, w, content);
            let _ = tx.send(Out::Thumb {
                tag,
                w: tw,
                h: th,
                px,
            });
        }
        for p in shots {
            let r = write_bmp(&p, &out, w, h);
            let _ = tx.send(Out::Shot(match r {
                Ok(()) => p,
                Err(e) => format!(
                    "Fehler: {e} ({p}; Ordner da: {}; uid {})",
                    std::path::Path::new(&p)
                        .parent()
                        .is_some_and(|d| d.is_dir()),
                    unsafe { libc::getuid() }
                ),
            }));
        }
        wake();
    }
}

/// Seiteninhalt (ARGB vormultipliziert, Zeilenabstand `stride`) in den Inhaltsbereich kopieren.
fn blit_frame(dst: &mut [u32], w: i32, h: i32, r: IRect, f: &Frame, bg: u32) {
    let rw = r.w.min(w - r.x).max(0);
    let rh = r.h.min(h - r.y).max(0);
    let cw = f.w.min(rw).max(0) as usize;
    let ch = f.h.min(rh).max(0);
    let src = unsafe { std::slice::from_raw_parts(f.data as *const u8, f.len) };
    for y in 0..rh {
        let row = ((r.y + y) * w + r.x) as usize;
        let d = &mut dst[row..row + rw as usize];
        if y < ch {
            let off = y as usize * f.stride as usize;
            if off + cw * 4 <= src.len() {
                let s =
                    unsafe { std::slice::from_raw_parts(src.as_ptr().add(off) as *const u32, cw) };
                d[..cw].copy_from_slice(s);
            }
            d[cw..].fill(bg);
        } else {
            d.fill(bg);
        }
    }
}

fn put_piece(dst: &mut [u32], px: &[u32], w: i32, h: i32, r: IRect) {
    let (x0, x1) = (r.x.max(0), (r.x + r.w).min(w));
    let (y0, y1) = (r.y.max(0), (r.y + r.h).min(h));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let pw = (x1 - x0) as usize;
    for (i, y) in (y0..y1).enumerate() {
        let s = &px[(i * pw).min(px.len())..((i + 1) * pw).min(px.len())];
        if s.len() < pw {
            break;
        }
        let a = (y * w + x0) as usize;
        dst[a..a + pw].copy_from_slice(s);
    }
}

/// Vorschaubild (¼ Größe) des Inhaltsbereichs.
fn thumb(canvas: &[u32], w: i32, r: IRect) -> (i32, i32, Vec<u32>) {
    let (tw, th) = ((r.w / 4).max(1), (r.h / 4).max(1));
    let mut out = vec![0u32; (tw * th) as usize];
    for y in 0..th {
        for x in 0..tw {
            let (sx, sy) = (r.x + x * 4 + 1, r.y + y * 4 + 1);
            out[(y * tw + x) as usize] = canvas.get((sy * w + sx) as usize).copied().unwrap_or(0);
        }
    }
    (tw, th, out)
}

pub fn write_bmp(path: &str, px: &[u32], w: i32, h: i32) -> std::io::Result<()> {
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        let start = b.len();
        for x in 0..w {
            let v = px[(y * w + x) as usize];
            b.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
        }
        while (b.len() - start) % 4 != 0 {
            b.push(0);
        }
    }
    std::fs::write(path, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leisten_ueber_inhalt() {
        let mut d = vec![0u32; 16];
        put_piece(
            &mut d,
            &[7, 7, 7, 7],
            4,
            4,
            IRect {
                x: 1,
                y: 1,
                w: 2,
                h: 2,
            },
        );
        assert_eq!(d[5], 7);
        assert_eq!(d[10], 7);
        assert_eq!(d[0], 0);
    }
}
