//! PDF lesen: Seiten rendern (hayro, reines Rust auf der CPU), Text herausziehen, suchen, Metadaten.
//! Arabischer Text: Präsentationsformen werden auf Grundbuchstaben zurückgeführt, Zeilen mit
//! überwiegend arabischer Schrift von rechts nach links gelesen.

use crate::pdf_text::{arrange, layout, TextDev, G};
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{interpret_page, Context, InterpreterCache, InterpreterSettings};
use hayro::hayro_syntax::Pdf;
use hayro::kurbo::Rect;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use std::sync::Arc;

pub use crate::pdf_text::normalize;

pub struct PdfDoc {
    pdf: Pdf,
}

/// Gerenderte Seite: RGBA (deckend, weißer Hintergrund)
pub struct PageImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct PdfInfo {
    pub pages: usize,
    pub title: String,
    pub author: String,
    /// Seitengröße der ersten Seite in Punkt (1/72 Zoll)
    pub size_pt: (f32, f32),
}

/// Warum eine PDF nicht geöffnet werden konnte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Verschlüsselt: es fehlt das Kennwort (oder es war falsch).
    Password,
    /// Beschädigt oder kein lesbares PDF.
    Damaged,
    /// Verschlüsselt mit einem Verfahren, das hayro nicht entschlüsseln kann.
    Unsupported,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OpenError::Password => "password required",
            OpenError::Damaged => "damaged pdf",
            OpenError::Unsupported => "unsupported encryption",
        })
    }
}

fn open_error(e: hayro::hayro_syntax::LoadPdfError) -> OpenError {
    use hayro::hayro_syntax::{DecryptionError, LoadPdfError};
    match e {
        LoadPdfError::Decryption(DecryptionError::PasswordProtected) => OpenError::Password,
        LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm) => OpenError::Unsupported,
        LoadPdfError::Decryption(_) | LoadPdfError::Invalid => OpenError::Damaged,
    }
}

/// Ein Eintrag der Gliederung (Lesezeichen): Zielseite (ab 0), Titel, Tiefe (0 = oberste Ebene).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub page: usize,
    pub title: String,
    pub level: u8,
}

/// Ein Wort einer Seite mit seinem Rahmen in PDF-Punkten (Ursprung unten links, y nach oben).
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Eine Zeile in Lesereihenfolge mit ihren Wörtern (Rahmen in PDF-Punkten, y nach oben).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub words: Vec<Word>,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

fn lossy(b: &Option<Box<[u8]>>) -> String {
    match b {
        None => String::new(),
        Some(v) => bytes_text(v),
    }
}

impl PdfDoc {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        Self::open_with_password(bytes, "")
    }

    pub fn open_with_password(bytes: Vec<u8>, pw: &str) -> Result<Self, String> {
        Self::open_with_password_kind(bytes, pw).map_err(|e| e.to_string())
    }

    /// Öffnet ohne Kennwort und meldet sauber, ob ein Kennwort fehlt oder die Datei kaputt ist.
    pub fn open_kind(bytes: Vec<u8>) -> Result<Self, OpenError> {
        open_impl(bytes, "")
    }

    /// Öffnet mit Kennwort (leeres Kennwort = wie [`Self::open_kind`]).
    pub fn open_with_password_kind(bytes: Vec<u8>, pw: &str) -> Result<Self, OpenError> {
        open_impl(bytes, pw)
    }

    /// Seite i mit fester Skalierung `scale` (Punkte → Pixel) rendern; beide Achsen gleich.
    pub fn render_at(&self, i: usize, scale: f32) -> Option<PageImage> {
        self.render_limited(i, scale, 40_000_000.0)
    }

    /// Wie [`Self::render_at`], aber das Bild hat höchstens `max_pixels` Pixel (der Maßstab wird
    /// dafür verkleinert; der Aufrufer zeichnet das Bild dann auf die gewünschte Größe).
    pub fn render_limited(&self, i: usize, scale: f32, max_pixels: f32) -> Option<PageImage> {
        let page = self.pdf.pages().get(i)?;
        let (pw, ph) = page.render_dimensions();
        let s = scale
            .max(0.02)
            .min((max_pixels / (pw * ph).max(1.0)).sqrt());
        let cache = RenderCache::new();
        let pm = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: s,
                y_scale: s,
                bg_color: WHITE,
            },
        );
        let (w, h) = (pm.width() as u32, pm.height() as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for p in pm.data() {
            rgba.extend_from_slice(&[p.r, p.g, p.b, 255]);
        }
        Some(PageImage { w, h, rgba })
    }

    /// Roh-Glyphen einer Seite (für Zeilen und Wort-Rahmen).
    fn page_glyphs(&self, i: usize) -> Vec<G> {
        let Some(page) = self.pdf.pages().get(i) else {
            return Vec::new();
        };
        let cache = InterpreterCache::new();
        let (w, h) = page.render_dimensions();
        let tf = page.initial_transform(true).to_kurbo();
        let mut ctx = Context::new(
            tf,
            Rect::new(0.0, 0.0, w as f64, h as f64),
            &cache,
            page.xref(),
            InterpreterSettings::default(),
        );
        let mut dev = TextDev::default();
        interpret_page(page, &mut ctx, &mut dev);
        dev.glyphs
    }

    /// Wörter einer Seite mit ihren Rahmen (in PDF-Punkten, y nach oben).
    pub fn page_words(&self, i: usize) -> Vec<Word> {
        self.page_lines(i)
            .into_iter()
            .flat_map(|l| l.words)
            .collect()
    }

    /// Zeilen einer Seite in Lesereihenfolge mit Wort-Rahmen.
    pub fn page_lines(&self, i: usize) -> Vec<Line> {
        arrange(self.page_glyphs(i))
    }

    /// Gliederung (Lesezeichen) in Lesereihenfolge: Zielseite, Titel und Tiefe; Unterpunkte
    /// stehen hinter ihrem Elternpunkt. Leer, wenn das Dokument keine Gliederung hat oder sie
    /// nicht lesbar ist (höchstens 2000 Einträge, Schleifen enden damit).
    pub fn outline(&self) -> Vec<OutlineItem> {
        use hayro::hayro_syntax::object::{Dict, ObjectIdentifier};
        const MAX_ITEMS: usize = 2000;
        let xref = self.pdf.xref();
        let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else {
            return Vec::new();
        };
        let Some(outlines_ref) = root.get_ref(b"Outlines") else {
            return Vec::new();
        };
        let Some(outlines) = xref.get::<Dict<'_>>(outlines_ref.into()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // Seitennummern: Objektkennung jeder Seite (für /Dest [Seite ...]).
        let page_ids: Vec<Option<ObjectIdentifier>> = (0..self.pages())
            .map(|p| self.pdf.pages().get(p).and_then(|pg| pg.raw().obj_id()))
            .collect();
        // Vorrat: erst das Kind, danach der Nachbar (der Nachbar wird zuerst gelegt).
        let mut stack: Vec<(ObjectIdentifier, u8)> = outlines
            .get_ref(b"First")
            .map(|r| (r.into(), 0u8))
            .into_iter()
            .collect();
        while let Some((id, level)) = stack.pop() {
            if out.len() >= MAX_ITEMS {
                break;
            }
            let Some(item) = xref.get::<Dict<'_>>(id) else {
                continue;
            };
            if let Some(next) = item.get_ref(b"Next") {
                stack.push((next.into(), level));
            }
            if let Some(child) = item.get_ref(b"First") {
                stack.push((child.into(), level.saturating_add(1)));
            }
            if let Some(title) = item.get::<hayro::hayro_syntax::object::String<'_>>(b"Title") {
                out.push(OutlineItem {
                    page: page_of_dest(&item, &page_ids),
                    title: bytes_text(title.as_bytes()),
                    level,
                });
            }
        }
        out
    }

    pub fn pages(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Darstellungsgröße einer Seite in Punkt (Drehung berücksichtigt)
    pub fn page_size(&self, i: usize) -> (f32, f32) {
        self.pdf
            .pages()
            .get(i)
            .map(|p| p.render_dimensions())
            .unwrap_or((595.0, 842.0))
    }

    pub fn info(&self) -> PdfInfo {
        let m = self.pdf.metadata();
        PdfInfo {
            pages: self.pages(),
            title: lossy(&m.title),
            author: lossy(&m.author),
            size_pt: self.page_size(0),
        }
    }

    /// Seite i auf Breite `width` Pixel rendern
    pub fn render(&self, i: usize, width: u32) -> Option<PageImage> {
        let page = self.pdf.pages().get(i)?;
        let (pw, ph) = page.render_dimensions();
        let s = (width.max(16) as f32 / pw.max(1.0)).min(8.0);
        // Obergrenze ~ 40 MPixel, damit große Pläne nicht den Speicher sprengen
        let s = s.min((40_000_000.0 / (pw * ph).max(1.0)).sqrt());
        let cache = RenderCache::new();
        let pm = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: s,
                y_scale: s,
                bg_color: WHITE,
            },
        );
        let (w, h) = (pm.width() as u32, pm.height() as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for p in pm.data() {
            // deckender Hintergrund → vormultipliziert = gerade
            rgba.extend_from_slice(&[p.r, p.g, p.b, 255]);
        }
        Some(PageImage { w, h, rgba })
    }

    /// Text einer Seite in Lesereihenfolge (Zeilen von oben nach unten)
    pub fn page_text(&self, i: usize) -> String {
        layout(self.page_glyphs(i))
    }

    /// Ganzer Text (mit Seitenmarken "--- Seite n ---")
    pub fn text(&self, max_pages: usize) -> String {
        let n = self.pages().min(max_pages);
        let mut out = String::new();
        for i in 0..n {
            if n > 1 {
                out.push_str(&format!("--- {} {} ---\n", "page", i + 1));
            }
            out.push_str(&self.page_text(i));
            out.push('\n');
        }
        out
    }

    /// Seiten, auf denen `q` vorkommt (ohne Groß-/Kleinschreibung), mit Ausschnitt
    pub fn search(&self, q: &str, max: usize) -> Vec<(usize, String)> {
        let ql = normalize(q).to_lowercase();
        let mut out = Vec::new();
        if ql.trim().is_empty() {
            return out;
        }
        for i in 0..self.pages() {
            let t = self.page_text(i);
            for line in t.lines() {
                if line.to_lowercase().contains(&ql) {
                    out.push((i, line.trim().chars().take(140).collect()));
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
        out
    }
}

fn open_impl(bytes: Vec<u8>, pw: &str) -> Result<PdfDoc, OpenError> {
    let data = Arc::new(bytes);
    let pdf = Pdf::new_with_password(data, pw);
    pdf.map(|pdf| PdfDoc { pdf }).map_err(open_error)
}

/// Seitennummer eines Ziels `[Seite /Fit …]` über die Objektkennung der Seite; 0 als Fallback.
fn page_of_dest(
    item: &hayro::hayro_syntax::object::Dict<'_>,
    page_ids: &[Option<hayro::hayro_syntax::object::ObjectIdentifier>],
) -> usize {
    use hayro::hayro_syntax::object::{Array, Dict, MaybeRef};
    // Ziel direkt (/Dest) oder als Sprung-Aktion (/A << /S /GoTo /D [...] >>).
    let arr = item.get::<Array<'_>>(b"Dest").or_else(|| {
        item.get::<Dict<'_>>(b"A")
            .and_then(|a| a.get::<Array<'_>>(b"D"))
    });
    let Some(arr) = arr else {
        return 0;
    };
    let target = match arr.raw_iter().next() {
        Some(MaybeRef::Ref(r)) => r.into(),
        _ => return 0,
    };
    page_ids
        .iter()
        .position(|id| *id == Some(target))
        .unwrap_or(0)
}

/// Zeichenkette einer PDF (UTF-16BE mit BOM, sonst UTF-8).
fn bytes_text(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(b).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaputte_datei_meldet_damaged() {
        assert_eq!(
            PdfDoc::open_kind(b"kein pdf".to_vec()).err(),
            Some(OpenError::Damaged)
        );
    }
}
