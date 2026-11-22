//! Kleine Zeichenfläche (CPU) für Leiste, Startmenü, Hintergrund und Fensterköpfe.
//! Pixel im Speicherformat ARGB8888 (little endian: B, G, R, A), vormultipliziertes Alpha.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn hex(c: u32) -> Self {
        Rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255)
    }
    pub const fn a(self, a: u8) -> Self {
        Rgba(self.0, self.1, self.2, a)
    }
    pub fn mix(self, o: Rgba, t: f32) -> Rgba {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgba(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2), m(self.3, o.3))
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        let w = w.max(1);
        let h = h.max(1);
        Self { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    /// Einen Pixel mit Deckung `cov` (0..1) einmischen.
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov <= 0.0 {
            return;
        }
        let a = (c.3 as f32 / 255.0) * cov.min(1.0);
        if a <= 0.0 {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let p = &mut self.px[i..i + 4];
        let inv = 1.0 - a;
        p[0] = (c.2 as f32 * a + p[0] as f32 * inv).round() as u8;
        p[1] = (c.1 as f32 * a + p[1] as f32 * inv).round() as u8;
        p[2] = (c.0 as f32 * a + p[2] as f32 * inv).round() as u8;
        p[3] = (255.0 * a + p[3] as f32 * inv).round() as u8;
    }

    pub fn clear(&mut self) {
        self.px.fill(0);
    }

    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.blend(xx, yy, c, 1.0);
            }
        }
    }

    /// Senkrechter Verlauf über die ganze Fläche (deckend).
    pub fn vgrad(&mut self, top: Rgba, bot: Rgba) {
        for y in 0..self.h {
            let c = top.mix(bot, y as f32 / (self.h.max(2) - 1) as f32);
            for x in 0..self.w {
                let i = ((y * self.w + x) * 4) as usize;
                self.px[i] = c.2;
                self.px[i + 1] = c.1;
                self.px[i + 2] = c.0;
                self.px[i + 3] = 255;
            }
        }
    }

    /// Weicher, runder Lichtschein (für den Hintergrund).
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (x0, x1) = ((cx - r).max(0.0) as i32, (cx + r).min(self.w as f32) as i32);
        let (y0, y1) = ((cy - r).max(0.0) as i32, (cy + r).min(self.h as f32) as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
                if d < 1.0 {
                    let t = (1.0 - d).powi(2);
                    self.blend(x, y, c, t);
                }
            }
        }
    }

    /// Abgerundetes Rechteck mit Kantenglättung.
    pub fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, c: Rgba) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w as f32 / 2.0).min(h as f32 / 2.0).max(0.0);
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let px = xx as f32 + 0.5 - x as f32;
                let py = yy as f32 + 0.5 - y as f32;
                let dx = (r - px).max(px - (w as f32 - r)).max(0.0);
                let dy = (r - py).max(py - (h as f32 - r)).max(0.0);
                let cov = if dx > 0.0 && dy > 0.0 {
                    (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                self.blend(xx, yy, c, cov);
            }
        }
    }

    /// Umriss eines abgerundeten Rechtecks (Linienstärke 1 px).
    pub fn rrect_line(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, c: Rgba) {
        let r = r.min(w as f32 / 2.0).min(h as f32 / 2.0).max(0.0);
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let px = xx as f32 + 0.5 - x as f32;
                let py = yy as f32 + 0.5 - y as f32;
                let dx = (r - px).max(px - (w as f32 - r)).max(0.0);
                let dy = (r - py).max(py - (h as f32 - r)).max(0.0);
                let dist_edge = if dx > 0.0 && dy > 0.0 {
                    r - (dx * dx + dy * dy).sqrt()
                } else {
                    px.min(py).min(w as f32 - px).min(h as f32 - py)
                };
                let cov = (1.0 - (dist_edge - 0.5).abs()).clamp(0.0, 1.0);
                if dist_edge > -0.5 {
                    self.blend(xx, yy, c, cov);
                }
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
            for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, c, (r - d + 0.5).clamp(0.0, 1.0));
            }
        }
    }

    /// Gerade Linie mit Dicke (für Symbole).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, th: f32, c: Rgba) {
        let (minx, maxx) = (x0.min(x1) - th, x0.max(x1) + th);
        let (miny, maxy) = (y0.min(y1) - th, y0.max(y1) + th);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in miny as i32..=maxy as i32 {
            for x in minx as i32..=maxx as i32 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (x0 + t * dx, y0 + t * dy);
                let d = ((px - qx).powi(2) + (py - qy).powi(2)).sqrt();
                self.blend(x, y, c, (th / 2.0 - d + 0.5).clamp(0.0, 1.0));
            }
        }
    }

    /// Deckungsmaske (Text) in Farbe einblenden.
    pub fn mask(&mut self, x: i32, y: i32, mw: i32, mh: i32, m: &[u8], c: Rgba) {
        for yy in 0..mh {
            for xx in 0..mw {
                let a = m[(yy * mw + xx) as usize];
                if a > 0 {
                    self.blend(x + xx, y + yy, c, a as f32 / 255.0);
                }
            }
        }
    }

    /// RGBA-Bild (nicht vormultipliziert) skaliert einzeichnen (bilinear), mit Gesamtdeckung `alpha`.
    pub fn image(&mut self, img: &Img, x: i32, y: i32, w: i32, h: i32, alpha: f32) {
        if img.w == 0 || img.h == 0 || w <= 0 || h <= 0 {
            return;
        }
        let sx = img.w as f32 / w as f32;
        let sy = img.h as f32 / h as f32;
        for yy in 0..h {
            for xx in 0..w {
                let fx = ((xx as f32 + 0.5) * sx - 0.5).clamp(0.0, (img.w - 1) as f32);
                let fy = ((yy as f32 + 0.5) * sy - 0.5).clamp(0.0, (img.h - 1) as f32);
                let (ix, iy) = (fx as u32, fy as u32);
                let (ix1, iy1) = ((ix + 1).min(img.w - 1), (iy + 1).min(img.h - 1));
                let (tx, ty) = (fx - ix as f32, fy - iy as f32);
                let p = |x: u32, y: u32| {
                    let i = ((y * img.w + x) * 4) as usize;
                    [img.px[i] as f32, img.px[i + 1] as f32, img.px[i + 2] as f32, img.px[i + 3] as f32]
                };
                let (a, b, c, d) = (p(ix, iy), p(ix1, iy), p(ix, iy1), p(ix1, iy1));
                let mut o = [0f32; 4];
                for k in 0..4 {
                    let top = a[k] + (b[k] - a[k]) * tx;
                    let bot = c[k] + (d[k] - c[k]) * tx;
                    o[k] = top + (bot - top) * ty;
                }
                let col = Rgba(o[0] as u8, o[1] as u8, o[2] as u8, 255);
                self.blend(x + xx, y + yy, col, o[3] / 255.0 * alpha);
            }
        }
    }
}

/// Bild im Speicher (RGBA, nicht vormultipliziert).
#[derive(Clone)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Img {
    pub fn from_png(bytes: &[u8]) -> Option<Self> {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        Some(Self { w: img.width(), h: img.height(), px: img.into_raw() })
    }
    pub fn load(path: &std::path::Path) -> Option<Self> {
        Self::from_png(&std::fs::read(path).ok()?)
    }
}
