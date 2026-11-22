//! Schrift mit Shaping (auch Arabisch, rechts nach links) über cosmic-text und die eingebetteten Noto-Schriften –
//! dieselben wie in der Ultar-Oberfläche.

use super::canvas::{Canvas, Rgba};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap};
use std::collections::HashMap;

static REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Bold.ttf");
static ARABIC: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansArabic-Regular.ttf");

struct Entry {
    w: f32,
    ox: i32,
    oy: i32,
    mw: i32,
    mh: i32,
    mask: Vec<u8>,
}

pub struct Text {
    fs: FontSystem,
    cache: SwashCache,
    bufs: HashMap<(String, u32, bool), Entry>,
}

impl Text {
    pub fn new() -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        for f in [REGULAR, BOLD, ARABIC] {
            db.load_font_source(cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(f)));
        }
        db.set_sans_serif_family("Noto Sans");
        Self {
            fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
            bufs: HashMap::new(),
        }
    }

    fn entry(&mut self, text: &str, size: f32, bold: bool) -> &Entry {
        if self.bufs.len() > 800 {
            self.bufs.clear();
        }
        let key = (text.to_string(), size.to_bits(), bold);
        let (fs, cache) = (&mut self.fs, &mut self.cache);
        self.bufs.entry(key).or_insert_with(|| {
            let mut buf = Buffer::new(fs, Metrics::new(size, (size * 1.35).round()));
            buf.set_wrap(Wrap::None);
            buf.set_size(None, None);
            let attrs = Attrs::new()
                .family(Family::Name("Noto Sans"))
                .weight(if bold { Weight::BOLD } else { Weight::NORMAL });
            buf.set_text(text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(fs, false);
            let (mut w, mut min_x) = (0f32, f32::MAX);
            for run in buf.layout_runs() {
                w = w.max(run.line_w);
                for g in run.glyphs {
                    min_x = min_x.min(g.x);
                }
            }
            let shift = if min_x != f32::MAX { -min_x.round() as i32 } else { 0 };
            let mut spans: Vec<(i32, i32, i32, u8)> = Vec::new();
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            buf.draw(fs, cache, cosmic_text::Color::rgb(255, 255, 255), |gx, gy, gw, gh, c| {
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
            });
            if spans.is_empty() {
                return Entry { w, ox: 0, oy: 0, mw: 0, mh: 0, mask: Vec::new() };
            }
            let (mw, mh) = (x1 - x0, y1 - y0);
            let mut mask = vec![0u8; (mw * mh) as usize];
            for (sx, sy, sw, a) in spans {
                let row = ((sy - y0) * mw + (sx - x0)) as usize;
                for m in &mut mask[row..row + sw as usize] {
                    *m = (*m).max(a);
                }
            }
            Entry { w, ox: x0, oy: y0, mw, mh, mask }
        })
    }

    pub fn width(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        self.entry(text, size, bold).w
    }

    /// Text zeichnen; `y` ist die Oberkante der Zeile. Liefert die Breite.
    pub fn draw(&mut self, cv: &mut Canvas, text: &str, x: f32, y: f32, size: f32, c: Rgba, bold: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let e = self.entry(text, size, bold);
        cv.mask(x.round() as i32 + e.ox, y.round() as i32 + e.oy, e.mw, e.mh, &e.mask, c);
        e.w
    }

    /// Text auf `max_w` kürzen (mit „…“).
    pub fn fit(&mut self, text: &str, size: f32, bold: bool, max_w: f32) -> String {
        if self.width(text, size, bold) <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let s: String = chars[..mid].iter().collect::<String>() + "…";
            if self.width(&s, size, bold) <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        chars[..lo].iter().collect::<String>() + "…"
    }

    /// Zeilenhöhe zu einer Schriftgröße.
    pub fn line_h(size: f32) -> f32 {
        (size * 1.35).round()
    }
}
