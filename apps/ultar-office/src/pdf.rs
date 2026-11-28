//! PDF lesen: Seiten rendern (hayro, reines Rust auf der CPU), Text herausziehen, suchen, Metadaten.
//! Arabischer Text: Präsentationsformen werden auf Grundbuchstaben zurückgeführt, Zeilen mit
//! überwiegend arabischer Schrift von rechts nach links gelesen.

use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask,
};
use hayro::hayro_syntax::Pdf;
use hayro::kurbo::{BezPath, Rect};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use std::sync::Arc;

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
        let s = scale.max(0.02).min((max_pixels / (pw * ph).max(1.0)).sqrt());
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
        let mut dev = TextDev { glyphs: Vec::new() };
        interpret_page(page, &mut ctx, &mut dev);
        dev.glyphs
    }

    /// Wörter einer Seite mit ihren Rahmen (in PDF-Punkten, y nach oben).
    pub fn page_words(&self, i: usize) -> Vec<Word> {
        self.page_lines(i).into_iter().flat_map(|l| l.words).collect()
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
    page_ids.iter().position(|id| *id == Some(target)).unwrap_or(0)
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

/// Rahmen eines Glyphs in PDF-Punkten (y nach oben): (x0, y0, x1, y1).
fn gbox(g: &G) -> (f32, f32, f32, f32) {
    let x0 = g.x;
    let y0 = g.y;
    let x1 = g.x + g.adv.unwrap_or(g.size).max(g.size * 0.1);
    let y1 = g.y + g.size;
    (x0 as f32, y0 as f32, x1 as f32, y1 as f32)
}

fn union_box(a: Option<(f32, f32, f32, f32)>, b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    match a {
        None => b,
        Some((x0, y0, x1, y1)) => (
            x0.min(b.0),
            y0.min(b.1),
            x1.max(b.2),
            y1.max(b.3),
        ),
    }
}

fn push_word(words: &mut Vec<Word>, cur: &mut String, bx: &mut Option<(f32, f32, f32, f32)>) {
    if cur.is_empty() {
        *bx = None;
        return;
    }
    let (x0, y0, x1, y1) = bx.unwrap_or((0.0, 0.0, 0.0, 0.0));
    words.push(Word {
        text: std::mem::take(cur),
        x0,
        y0,
        x1,
        y1,
    });
    *bx = None;
}

/// Arabische Zeile: Wortfolge umdrehen, arabische Wörter zeichenweise umdrehen,
/// eingebettete lateinische Wortgruppen behalten ihre Reihenfolge.
fn reorder_ar(mut words: Vec<Word>) -> Vec<Word> {
    words.reverse();
    let mut fixed: Vec<Word> = Vec::new();
    let mut latin: Vec<Word> = Vec::new();
    for mut w in words {
        if arabic(&w.text) {
            latin.reverse();
            fixed.append(&mut latin);
            w.text = normalize(&w.text.chars().rev().collect::<String>());
            fixed.push(w);
        } else {
            w.text = normalize(&w.text);
            latin.push(w);
        }
    }
    latin.reverse();
    fixed.append(&mut latin);
    fixed
}

/// Glyphen zu Zeilen gruppieren und Wörter mit Rahmen bilden (y nach oben).
fn arrange(mut gs: Vec<G>) -> Vec<Line> {
    gs.retain(|g| !g.s.is_empty());
    if gs.is_empty() {
        return Vec::new();
    }
    gs.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut groups: Vec<Vec<G>> = Vec::new();
    for g in gs {
        match groups.last_mut() {
            Some(l) if (l[0].y - g.y).abs() < l[0].size.max(g.size) * 0.45 => l.push(g),
            _ => groups.push(vec![g]),
        }
    }
    let mut out = Vec::new();
    for mut l in groups {
        let ar = l.iter().filter(|g| arabic(&g.s)).count() * 2 > l.len();
        l.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        let has_space = l.iter().any(|g| g.s == " ");
        let mut d: Vec<f64> = l
            .windows(2)
            .map(|w| w[1].x - w[0].x)
            .filter(|v| *v > 0.01)
            .collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = d
            .get(d.len() / 2)
            .copied()
            .unwrap_or(l[0].size * 0.5)
            .min(l[0].size * 0.8);
        let mut words: Vec<Word> = Vec::new();
        let mut cur = String::new();
        let mut bx: Option<(f32, f32, f32, f32)> = None;
        let mut prev: Option<(f64, usize, Option<f64>)> = None;
        for g in &l {
            if let Some((px, n, padv)) = prev {
                let dx = g.x - px;
                let gap = match padv {
                    Some(a) => (dx - a).abs() > g.size * 0.15 && dx > a,
                    None if has_space => dx > g.size * (1.2 + 0.6 * (n as f64 - 1.0)),
                    None => dx > med * (n as f64) * 1.45,
                };
                if gap && g.s != " " {
                    push_word(&mut words, &mut cur, &mut bx);
                }
            }
            if g.s == " " {
                push_word(&mut words, &mut cur, &mut bx);
            } else {
                cur.push_str(&g.s);
                bx = Some(union_box(bx, gbox(g)));
            }
            prev = Some((g.x, g.s.chars().count().max(1), g.adv));
        }
        push_word(&mut words, &mut cur, &mut bx);
        words.retain(|w| !w.text.is_empty());
        let words = if ar {
            reorder_ar(words)
        } else {
            words
                .into_iter()
                .map(|mut w| {
                    w.text = normalize(&w.text);
                    w
                })
                .collect()
        };
        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for w in &words {
            x0 = x0.min(w.x0);
            y0 = y0.min(w.y0);
            x1 = x1.max(w.x1);
            y1 = y1.max(w.y1);
        }
        if words.is_empty() {
            x0 = 0.0;
            y0 = 0.0;
            x1 = 0.0;
            y1 = 0.0;
        }
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        out.push(Line {
            text,
            words,
            x0,
            y0,
            x1,
            y1,
        });
    }
    out
}

#[derive(Clone)]
struct G {
    x: f64,
    y: f64,
    size: f64,
    adv: Option<f64>,
    s: String,
}

struct TextDev {
    glyphs: Vec<G>,
}

impl<'a> Device<'a> for TextDev {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(
        &mut self,
        run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>,
        _: DrawProps<'a>,
        _: &DrawMode,
    ) {
        for g in run.glyphs() {
            let t = g.transform();
            let c = t.as_coeffs();
            let p = t * hayro::kurbo::Point::new(0.0, 0.0);
            // Glyphenraum: 1000 Einheiten je Geviert; Koordinaten im PDF-Raum (y nach oben)
            let size = ((c[0] * c[3] - c[1] * c[2]).abs().sqrt() * 1000.0).max(0.5);
            let s = match g.as_unicode() {
                Some(hayro::hayro_interpret::hayro_cmap::BfString::Char(ch)) => ch.to_string(),
                Some(hayro::hayro_interpret::hayro_cmap::BfString::String(s)) => s,
                None => continue,
            };
            let adv = match &**g {
                hayro::hayro_interpret::font::Glyph::Outline(o) => o.advance_width(),
                hayro::hayro_interpret::font::Glyph::Type3(t) => t.advance_width(),
            };
            // Vorschub in 1/1000 Geviert → Punkt
            let adv = adv.filter(|a| *a > 0.0).map(|a| a as f64 / 1000.0 * size);
            self.glyphs.push(G {
                x: p.x,
                y: p.y,
                size,
                adv,
                s,
            });
        }
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Arabische Präsentationsformen (FB50–FDFF, FE70–FEFF) → Grundbuchstaben
pub fn normalize(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        let u = ch as u32;
        if (0xFE70..=0xFEFF).contains(&u) || (0xFB50..=0xFDFF).contains(&u) {
            o.push_str(&presentation_base(ch));
        } else {
            o.push(ch);
        }
    }
    o
}

fn presentation_base(ch: char) -> String {
    let u = ch as u32;
    // Tabelle für den häufigen Block FE80–FEFC (Formen: isoliert/End/Anfang/Mitte)
    const BASE: &[(u32, u32, char)] = &[
        (0xFE80, 0xFE80, 'ء'),
        (0xFE81, 0xFE82, 'آ'),
        (0xFE83, 0xFE84, 'أ'),
        (0xFE85, 0xFE86, 'ؤ'),
        (0xFE87, 0xFE88, 'إ'),
        (0xFE89, 0xFE8C, 'ئ'),
        (0xFE8D, 0xFE8E, 'ا'),
        (0xFE8F, 0xFE92, 'ب'),
        (0xFE93, 0xFE94, 'ة'),
        (0xFE95, 0xFE98, 'ت'),
        (0xFE99, 0xFE9C, 'ث'),
        (0xFE9D, 0xFEA0, 'ج'),
        (0xFEA1, 0xFEA4, 'ح'),
        (0xFEA5, 0xFEA8, 'خ'),
        (0xFEA9, 0xFEAA, 'د'),
        (0xFEAB, 0xFEAC, 'ذ'),
        (0xFEAD, 0xFEAE, 'ر'),
        (0xFEAF, 0xFEB0, 'ز'),
        (0xFEB1, 0xFEB4, 'س'),
        (0xFEB5, 0xFEB8, 'ش'),
        (0xFEB9, 0xFEBC, 'ص'),
        (0xFEBD, 0xFEC0, 'ض'),
        (0xFEC1, 0xFEC4, 'ط'),
        (0xFEC5, 0xFEC8, 'ظ'),
        (0xFEC9, 0xFECC, 'ع'),
        (0xFECD, 0xFED0, 'غ'),
        (0xFED1, 0xFED4, 'ف'),
        (0xFED5, 0xFED8, 'ق'),
        (0xFED9, 0xFEDC, 'ك'),
        (0xFEDD, 0xFEE0, 'ل'),
        (0xFEE1, 0xFEE4, 'م'),
        (0xFEE5, 0xFEE8, 'ن'),
        (0xFEE9, 0xFEEC, 'ه'),
        (0xFEED, 0xFEEE, 'و'),
        (0xFEEF, 0xFEF0, 'ى'),
        (0xFEF1, 0xFEF4, 'ي'),
    ];
    for &(a, b, c) in BASE {
        if (a..=b).contains(&u) {
            return c.to_string();
        }
    }
    match u {
        0xFEF5 | 0xFEF6 => "لآ".into(),
        0xFEF7 | 0xFEF8 => "لأ".into(),
        0xFEF9 | 0xFEFA => "لإ".into(),
        0xFEFB | 0xFEFC => "لا".into(),
        0xFDF2 => "الله".into(),
        0xFEFF => String::new(),
        _ => ch.to_string(),
    }
}

fn arabic(s: &str) -> bool {
    s.chars()
        .any(|c| (0x0600..=0x06FF).contains(&(c as u32)) || (0xFB50..=0xFEFC).contains(&(c as u32)))
}

/// Glyphen zu Zeilen gruppieren (y), in Zeilen nach x sortieren, Wortabstände einfügen
fn layout(mut gs: Vec<G>) -> String {
    gs.retain(|g| !g.s.is_empty());
    if gs.is_empty() {
        return String::new();
    }
    // PDF-Raum: y wächst nach oben → oben zuerst = absteigend
    gs.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut lines: Vec<Vec<G>> = Vec::new();
    for g in gs {
        match lines.last_mut() {
            Some(l) if (l[0].y - g.y).abs() < l[0].size.max(g.size) * 0.45 => l.push(g),
            _ => lines.push(vec![g]),
        }
    }
    let mut out = String::new();
    let mut last_y: Option<(f64, f64)> = None;
    for mut l in lines {
        let ar = l.iter().filter(|g| arabic(&g.s)).count() * 2 > l.len();
        l.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((ly, ls)) = last_y {
            if ly - l[0].y > ls * 2.2 {
                out.push('\n');
            }
        }
        last_y = Some((l[0].y, l[0].size));
        // Zeilen mit echten Leerzeichen: nur dort trennen (und bei Spaltenlücken). Sonst Wortgrenze, wenn der
        // Abstand deutlich über dem typischen Zeichenabstand (Median) liegt.
        let has_space = l.iter().any(|g| g.s == " ");
        let mut d: Vec<f64> = l
            .windows(2)
            .map(|w| w[1].x - w[0].x)
            .filter(|v| *v > 0.01)
            .collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = d
            .get(d.len() / 2)
            .copied()
            .unwrap_or(l[0].size * 0.5)
            .min(l[0].size * 0.8);
        let mut s = String::new();
        let mut prev: Option<(f64, usize, Option<f64>)> = None;
        let mut parts: Vec<String> = Vec::new();
        for g in &l {
            if let Some((px, n, padv)) = prev {
                let dx = g.x - px;
                let gap = match padv {
                    // Vorschub der Schrift bekannt: Lücke hinter dem Zeichen ab ~0,15 Geviert = Wortgrenze
                    Some(a) => (dx - a).abs() > g.size * 0.15 && dx > a,
                    None if has_space => dx > g.size * (1.2 + 0.6 * (n as f64 - 1.0)),
                    None => dx > med * (n as f64) * 1.45,
                };
                if gap && g.s != " " {
                    parts.push(std::mem::take(&mut s));
                }
            }
            if g.s == " " {
                parts.push(std::mem::take(&mut s));
            } else {
                s.push_str(&g.s);
            }
            prev = Some((g.x, g.s.chars().count().max(1), g.adv));
        }
        parts.push(s);
        parts.retain(|p| !p.is_empty());
        if ar {
            // Glyphen stehen in Bildreihenfolge (links→rechts): Wortfolge umdrehen, arabische Wörter zeichenweise
            // umdrehen, eingebettete lateinische Wortgruppen behalten ihre Reihenfolge
            parts.reverse();
            let mut fixed: Vec<String> = Vec::new();
            let mut latin: Vec<String> = Vec::new();
            for p in parts {
                if arabic(&p) {
                    latin.reverse();
                    fixed.append(&mut latin);
                    fixed.push(p.chars().rev().collect());
                } else {
                    latin.push(p);
                }
            }
            latin.reverse();
            fixed.append(&mut latin);
            out.push_str(&normalize(&fixed.join(" ")));
        } else {
            out.push_str(&normalize(&parts.join(" ")));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn praesentationsformen() {
        assert_eq!(
            normalize("\u{FEE3}\u{FEAE}\u{FEA3}\u{FE92}\u{FE8E}"),
            "مرحبا"
        );
        assert_eq!(normalize("\u{FEFB}"), "لا");
    }

    #[test]
    fn kaputte_datei_meldet_damaged() {
        assert_eq!(PdfDoc::open_kind(b"kein pdf".to_vec()).err(), Some(OpenError::Damaged));
    }

    /// Glyphen einer Zeile (y nach oben): zwei Wörter mit einem Leerraum dazwischen.
    fn word_glyphs() -> Vec<G> {
        let mut gs = Vec::new();
        let y = 700.0;
        let mut x = 50.0;
        for ch in "hello world".chars() {
            if ch == ' ' {
                x += 10.0;
                continue;
            }
            gs.push(G {
                x,
                y,
                size: 12.0,
                adv: Some(6.0),
                s: ch.to_string(),
            });
            x += 6.0;
        }
        gs
    }

    #[test]
    fn arrange_bildet_woerter_mit_rahmen() {
        let lines = arrange(word_glyphs());
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.text, "hello world");
        assert_eq!(line.words.len(), 2);
        assert_eq!(line.words[0].text, "hello");
        assert_eq!(line.words[1].text, "world");
        assert!(line.words[0].x0 < line.words[1].x0);
        assert!(line.y1 > line.y0);
    }

    #[test]
    fn arabischer_text_stimmt_mit_layout_ueberein() {
        let mut gs = Vec::new();
        let mut x = 50.0;
        // Zwei arabische Wörter, gezeichnet in Bildreihenfolge (von links nach rechts).
        for word in ["مَرْحَبًا", "بِكُمْ"] {
            for ch in word.chars() {
                gs.push(G {
                    x,
                    y: 700.0,
                    size: 12.0,
                    adv: Some(6.0),
                    s: ch.to_string(),
                });
                x += 6.0;
            }
            x += 10.0;
        }
        let text = layout(gs.clone());
        let lines = arrange(gs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].words.len(), 2);
        // `arrange` muss denselben Lesetext liefern wie die bestehende `layout`.
        assert_eq!(&lines[0].text, text.trim_end());
    }
}
