//! Marken-Grafiken von KM Code Craft (eingebettet, siehe scripts/brand-assets.py)
//! und Startbild für Bootloader und Systemstart.

use crate::gfx::{rgb, Canvas, Color};
use crate::i18n::tr;
use crate::text::Text;

pub struct Img {
    pub w: u32,
    pub h: u32,
    pub rgba: &'static [u8],
}

fn img(raw: &'static [u8]) -> Img {
    Img { w: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]), h: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]), rgba: &raw[8..] }
}

/// Logo mit Schriftzug (für dunklen Hintergrund)
pub fn logo() -> Img {
    img(include_bytes!("../../../assets/brand/logo.rgba"))
}

/// Bildmarke allein
pub fn mark() -> Img {
    img(include_bytes!("../../../assets/brand/mark-circuit.rgba"))
}

pub const BOOT_BG: Color = rgb(0x0B0D11);

/// Startbild: Logo mittig, darunter Systemname und "Wird gestartet…" in der gewählten Sprache.
pub fn splash(cv: &mut Canvas, tx: &mut Text, lang: &str) {
    cv.set_clip(None);
    cv.clear(BOOT_BG);
    let (w, h) = (cv.w as f32, cv.h as f32);
    let short = w.min(h);
    let l = logo();
    let lw = short * 0.62;
    let lh = lw * l.h as f32 / l.w as f32;
    cv.image(&l, (w - lw) / 2.0, h * 0.42 - lh / 2.0, lw, lh);
    let sz = short * 0.05;
    let name = "Ultar OS";
    let (tw, _) = tx.measure(name, sz, true, 0.0);
    tx.draw(cv, name, (w - tw) / 2.0, h * 0.42 + lh / 2.0 + sz * 1.2, sz, rgb(0xE9D68A), true, 0.0);
    let msg = tr(lang, "Starting…");
    let sz2 = short * 0.036;
    let (tw, _) = tx.measure(msg, sz2, false, 0.0);
    tx.draw(cv, msg, (w - tw) / 2.0, h * 0.86, sz2, rgb(0x9AA1AD), false, 0.0);
}

/// 24-Bit-BMP schreiben (Format, das U-Boot und die meisten Bootloader als Startbild laden).
pub fn write_bmp(cv: &Canvas, path: &str) -> std::io::Result<()> {
    let (w, h) = (cv.w as usize, cv.h as usize);
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&((row * h) as u32).to_le_bytes());
    b.extend_from_slice(&[0x13, 0x0B, 0, 0, 0x13, 0x0B, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    for y in (0..h).rev() {
        let start = b.len();
        for x in 0..w {
            let v = cv.px[y * w + x];
            b.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
        }
        b.resize(start + row, 0);
    }
    std::fs::write(path, b)
}
