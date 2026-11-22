//! Schrift: Shaping (auch Arabisch/RTL) über cosmic-text, eingebettete Noto-Schriften.

use crate::gfx::{Canvas, Color};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap};
use std::collections::HashMap;

static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf");
static ARABIC: &[u8] = include_bytes!("../../../assets/fonts/NotoSansArabic-Regular.ttf");
/// Festbreitenschrift für den Code-Editor
static MONO: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");

/// Fertig gerasterter Text: Größe und Deckungsmaske. Gesetzt und gerastert wird nur einmal,
/// jedes weitere Zeichnen blendet nur noch die Maske ein.
struct Entry {
    w: f32,
    h: f32,
    ox: i32,
    oy: i32,
    mw: i32,
    mh: i32,
    mask: Vec<u8>,
}

pub struct Text {
    fs: FontSystem,
    cache: SwashCache,
    bufs: HashMap<(String, u32, u8, u32), Entry>,
    /// vorige Generation: wird bei Bedarf zurückgeholt statt neu gesetzt und gerastert
    old: HashMap<(String, u32, u8, u32), Entry>,
}

impl Text {
    pub fn new() -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        for f in [REGULAR, BOLD, ARABIC, MONO] {
            db.load_font_source(cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(f)));
        }
        db.set_sans_serif_family("Noto Sans");
        Self {
            fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
            bufs: HashMap::new(),
            old: HashMap::new(),
        }
    }

    /// style: 0 normal, 1 fett, 2 Festbreite
    fn entry(&mut self, text: &str, size: f32, style: u8, wrap_w: f32) -> &Entry {
        // Zwei Generationen: läuft der Speicher voll, wird die ältere verworfen – häufig Gezeichnetes
        // wandert dabei zurück und muss nie neu gesetzt werden (vorher: alles löschen = jedes Bild neu setzen)
        if self.bufs.len() > 2500 {
            self.old = std::mem::take(&mut self.bufs);
        }
        let key = (text.to_string(), size.to_bits(), style, wrap_w.to_bits());
        if !self.bufs.contains_key(&key) {
            if let Some(e) = self.old.remove(&key) {
                self.bufs.insert(key.clone(), e);
            }
        }
        let (fs, cache) = (&mut self.fs, &mut self.cache);
        self.bufs.entry(key).or_insert_with(|| {
            let mut buf = Buffer::new(fs, Metrics::new(size, (size * 1.35).round()));
            if wrap_w > 0.0 {
                buf.set_wrap(Wrap::WordOrGlyph);
                buf.set_size(Some(wrap_w), None);
            } else {
                buf.set_wrap(Wrap::None);
                buf.set_size(None, None);
            }
            let attrs = if style == 2 {
                Attrs::new().family(Family::Name("DejaVu Sans Mono"))
            } else {
                Attrs::new()
                    .family(Family::Name("Noto Sans"))
                    .weight(if style == 1 {
                        Weight::BOLD
                    } else {
                        Weight::NORMAL
                    })
            };
            buf.set_text(text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(fs, false);
            let (mut w, mut h, mut min_x) = (0f32, 0f32, f32::MAX);
            for run in buf.layout_runs() {
                w = w.max(run.line_w);
                h = h.max(run.line_top + run.line_height);
                for g in run.glyphs {
                    min_x = min_x.min(g.x);
                }
            }
            // Bei RTL ohne feste Breite richtet cosmic-text nicht an 0 aus; kleinste Glyphenposition ausgleichen
            let shift = if wrap_w == 0.0 && min_x != f32::MAX {
                -min_x.round() as i32
            } else {
                0
            };
            let mut spans: Vec<(i32, i32, i32, u8)> = Vec::new();
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            buf.draw(
                fs,
                cache,
                cosmic_text::Color::rgb(255, 255, 255),
                |gx, gy, gw, gh, c| {
                    if c.a() == 0 {
                        return;
                    }
                    let gx = gx + shift;
                    for yy in gy..gy + gh as i32 {
                        spans.push((gx, yy, gw as i32, c.a()));
                    }
                    x0 = x0.min(gx);
                    y0 = y0.min(gy);
                    x1 = x1.max(gx + gw as i32);
                    y1 = y1.max(gy + gh as i32);
                },
            );
            if spans.is_empty() {
                return Entry {
                    w,
                    h,
                    ox: 0,
                    oy: 0,
                    mw: 0,
                    mh: 0,
                    mask: Vec::new(),
                };
            }
            let (mw, mh) = (x1 - x0, y1 - y0);
            let mut mask = vec![0u8; (mw * mh) as usize];
            for (sx, sy, sw, a) in spans {
                let row = ((sy - y0) * mw + (sx - x0)) as usize;
                for m in &mut mask[row..row + sw as usize] {
                    *m = (*m).max(a);
                }
            }
            Entry {
                w,
                h,
                ox: x0,
                oy: y0,
                mw,
                mh,
                mask,
            }
        })
    }

    /// Breite und Höhe (wrap_w = 0: einzeilig).
    pub fn measure(&mut self, text: &str, size: f32, bold: bool, wrap_w: f32) -> (f32, f32) {
        let e = self.entry(text, size, bold as u8, wrap_w);
        (e.w, e.h)
    }

    pub fn draw(
        &mut self,
        cv: &mut Canvas,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        wrap_w: f32,
    ) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let e = self.entry(text, size, bold as u8, wrap_w);
        cv.mask(
            x.round() as i32 + e.ox,
            y.round() as i32 + e.oy,
            e.mw,
            e.mh,
            &e.mask,
            color,
        );
        (e.w, e.h)
    }

    /// Festbreitenschrift (Code-Editor): Breite und Höhe
    pub fn measure_mono(&mut self, text: &str, size: f32) -> (f32, f32) {
        let e = self.entry(text, size, 2, 0.0);
        (e.w, e.h)
    }

    /// Festbreitenschrift zeichnen (einzeilig)
    pub fn draw_mono(
        &mut self,
        cv: &mut Canvas,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
    ) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        let e = self.entry(text, size, 2, 0.0);
        cv.mask(
            x.round() as i32 + e.ox,
            y.round() as i32 + e.oy,
            e.mw,
            e.mh,
            &e.mask,
            color,
        );
        (e.w, e.h)
    }
}
