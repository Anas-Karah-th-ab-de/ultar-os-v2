//! Video ohne Zusatzprogramme: Motion-JPEG im AVI-Behälter.
//!
//! Jedes Einzelbild ist ein JPEG; der Behälter ist so einfach, dass ihn jeder Player (VLC, mpv, ffmpeg,
//! Windows, Android) abspielt. Kein Ton – der Mikrofonweg ist gerätespezifisch und kommt getrennt.
//! `Writer` schreibt fortlaufend und trägt Bildzahl/Bildrate beim Abschluss in den Kopf ein;
//! `Reader` liest das Bildverzeichnis und liefert einzelne Bilder (für Vorschau und Wiedergabe).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const HDR: u64 = 224; // Beginn des ersten Bild-Chunks
const MOVI_FOURCC: u64 = 220;

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

pub struct Writer {
    f: File,
    w: u32,
    h: u32,
    /// (Abstand ab 'movi', Länge) je Bild
    idx: Vec<(u32, u32)>,
    pos: u64,
    max: u32,
}

impl Writer {
    pub fn create(path: &str, w: u32, h: u32) -> std::io::Result<Writer> {
        let mut f = File::create(path)?;
        f.write_all(&[0u8; HDR as usize])?;
        Ok(Writer {
            f,
            w,
            h,
            idx: Vec::new(),
            pos: HDR,
            max: 0,
        })
    }

    pub fn frames(&self) -> usize {
        self.idx.len()
    }

    /// Ein fertig kodiertes JPEG anhängen.
    pub fn push(&mut self, jpeg: &[u8]) -> std::io::Result<()> {
        let len = jpeg.len() as u32;
        self.f.write_all(b"00dc")?;
        self.f.write_all(&le32(len))?;
        self.f.write_all(jpeg)?;
        if len % 2 == 1 {
            self.f.write_all(&[0])?;
        }
        self.idx.push(((self.pos - MOVI_FOURCC) as u32, len));
        self.pos += 8 + len as u64 + (len % 2) as u64;
        self.max = self.max.max(len);
        Ok(())
    }

    /// Verzeichnis und Kopf schreiben. `usec_per_frame`: gemessene mittlere Bilddauer.
    pub fn finish(mut self, usec_per_frame: u32) -> std::io::Result<()> {
        let n = self.idx.len() as u32;
        let movi_size = (self.pos - MOVI_FOURCC) as u32; // ab 'movi' gezählt
        self.f.write_all(b"idx1")?;
        self.f.write_all(&le32(n * 16))?;
        for (off, len) in &self.idx {
            self.f.write_all(b"00dc")?;
            self.f.write_all(&le32(0x10))?;
            self.f.write_all(&le32(*off))?;
            self.f.write_all(&le32(*len))?;
        }
        let total = self.pos + 8 + n as u64 * 16;
        let usec = usec_per_frame.max(1);
        let mut h = Vec::with_capacity(HDR as usize);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&le32((total - 8) as u32));
        h.extend_from_slice(b"AVI ");
        h.extend_from_slice(b"LIST");
        h.extend_from_slice(&le32(4 + 64 + 12 + 64 + 48));
        h.extend_from_slice(b"hdrl");
        h.extend_from_slice(b"avih");
        h.extend_from_slice(&le32(56));
        for v in [
            usec,
            self.max.saturating_mul(1_000_000 / usec.max(1)).max(1),
            0,
            0x10,
            n,
            0,
            1,
            self.max,
            self.w,
            self.h,
            0,
            0,
            0,
            0,
        ] {
            h.extend_from_slice(&le32(v));
        }
        h.extend_from_slice(b"LIST");
        h.extend_from_slice(&le32(4 + 64 + 48));
        h.extend_from_slice(b"strl");
        h.extend_from_slice(b"strh");
        h.extend_from_slice(&le32(56));
        h.extend_from_slice(b"vids");
        h.extend_from_slice(b"MJPG");
        // flags, priority+language, initial frames, scale, rate, start, length, buffer, quality, sample size
        for v in [0, 0, 0, usec, 1_000_000, 0, n, self.max, u32::MAX, 0] {
            h.extend_from_slice(&le32(v));
        }
        for v in [0u16, 0, self.w as u16, self.h as u16] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(b"strf");
        h.extend_from_slice(&le32(40));
        for v in [40, self.w, self.h] {
            h.extend_from_slice(&le32(v));
        }
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&24u16.to_le_bytes());
        h.extend_from_slice(b"MJPG");
        for v in [self.w * self.h * 3, 0, 0, 0, 0] {
            h.extend_from_slice(&le32(v));
        }
        h.extend_from_slice(b"LIST");
        h.extend_from_slice(&le32(movi_size));
        h.extend_from_slice(b"movi");
        debug_assert_eq!(h.len() as u64, HDR);
        self.f.seek(SeekFrom::Start(0))?;
        self.f.write_all(&h)?;
        self.f.sync_all()
    }
}

pub struct Reader {
    f: File,
    /// (Dateiposition der Bilddaten, Länge)
    pub index: Vec<(u64, u32)>,
    pub usec: u32,
    pub w: u32,
    pub h: u32,
}

impl Reader {
    pub fn open(path: &str) -> Result<Reader, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let flen = f.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; 12];
        f.read_exact(&mut head).map_err(|e| e.to_string())?;
        if &head[0..4] != b"RIFF" || &head[8..12] != b"AVI " {
            return Err("kein AVI".into());
        }
        let (mut usec, mut w, mut h) = (33_333u32, 0u32, 0u32);
        let mut index = Vec::new();
        let mut pos = 12u64;
        let rd = |f: &mut File, at: u64, n: usize| -> Option<Vec<u8>> {
            let mut b = vec![0u8; n];
            f.seek(SeekFrom::Start(at)).ok()?;
            f.read_exact(&mut b).ok()?;
            Some(b)
        };
        let u = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        while pos + 8 <= flen {
            let Some(c) = rd(&mut f, pos, 8) else { break };
            let size = u(&c, 4) as u64;
            if &c[0..4] == b"LIST" {
                let Some(t) = rd(&mut f, pos + 8, 4) else {
                    break;
                };
                if &t == b"hdrl" {
                    if let Some(a) = rd(&mut f, pos + 12, 8 + 56) {
                        if &a[0..4] == b"avih" {
                            usec = u(&a, 8).max(1);
                            w = u(&a, 8 + 32);
                            h = u(&a, 8 + 36);
                        }
                    }
                } else if &t == b"movi" {
                    let end = (pos + 8 + size).min(flen);
                    let mut p = pos + 12;
                    while p + 8 <= end && index.len() < 200_000 {
                        let Some(k) = rd(&mut f, p, 8) else { break };
                        let len = u(&k, 4);
                        if &k[2..4] == b"dc" || &k[2..4] == b"db" {
                            index.push((p + 8, len));
                        }
                        p += 8 + len as u64 + (len % 2) as u64;
                    }
                }
            }
            pos += 8 + size + (size % 2);
        }
        if index.is_empty() {
            return Err("keine Bilder im Video".into());
        }
        Ok(Reader {
            f,
            index,
            usec,
            w,
            h,
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.index.len() as u64 * self.usec as u64 / 1000
    }

    /// Rohes JPEG des Bildes `i`.
    pub fn jpeg(&mut self, i: usize) -> Option<Vec<u8>> {
        let (off, len) = *self.index.get(i)?;
        if len > 32 * 1024 * 1024 {
            return None;
        }
        let mut b = vec![0u8; len as usize];
        self.f.seek(SeekFrom::Start(off)).ok()?;
        self.f.read_exact(&mut b).ok()?;
        Some(b)
    }

    /// Bild `i` dekodiert als (Breite, Höhe, RGBA).
    pub fn frame(&mut self, i: usize) -> Option<(u32, u32, Vec<u8>)> {
        let j = self.jpeg(i)?;
        let im = image::load_from_memory_with_format(&j, image::ImageFormat::Jpeg)
            .ok()?
            .to_rgba8();
        Some((im.width(), im.height(), im.into_raw()))
    }
}

/// XRGB-Bild als JPEG kodieren.
pub fn encode_jpeg(px: &[u32], w: u32, h: u32, quality: u8) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || (px.len() as u32) < w * h {
        return None;
    }
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for &p in px.iter().take((w * h) as usize) {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    let mut out = Vec::with_capacity((w * h / 4) as usize);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(out)
}

pub fn is_video(p: &str) -> bool {
    p.to_lowercase().ends_with(".avi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schreiben_und_lesen() {
        let path = std::env::temp_dir().join("ultar-mjpeg-test.avi");
        let path = path.to_str().unwrap();
        let (w, h) = (64u32, 48u32);
        let mut wr = Writer::create(path, w, h).unwrap();
        for n in 0..5u32 {
            let px: Vec<u32> = (0..w * h)
                .map(|i| ((i + n * 7) % 255) << 16 | 0x40 << 8 | n * 40)
                .collect();
            wr.push(&encode_jpeg(&px, w, h, 80).unwrap()).unwrap();
        }
        assert_eq!(wr.frames(), 5);
        wr.finish(100_000).unwrap();
        let mut rd = Reader::open(path).unwrap();
        assert_eq!((rd.index.len(), rd.usec, rd.w, rd.h), (5, 100_000, w, h));
        assert_eq!(rd.duration_ms(), 500);
        let (fw, fh, rgba) = rd.frame(4).unwrap();
        assert_eq!((fw, fh, rgba.len() as u32), (w, h, w * h * 4));
        // ULTAR_KEEP=1 lässt die Datei liegen (Gegenprobe mit ffprobe/VLC)
        if std::env::var("ULTAR_KEEP").is_err() {
            let _ = std::fs::remove_file(path);
        }
    }
}
