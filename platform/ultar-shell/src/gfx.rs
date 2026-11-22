//! Software-Zeichenfläche (XRGB8888) mit kantengeglätteten Grundformen.

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Mischt `src` mit Deckung `a` (0..=255) über `dst` (beide 0x00RRGGBB); Rot+Blau und Grün je in einem Schritt.
#[inline(always)]
pub fn mix_px(dst: u32, src: u32, a: u32) -> u32 {
    let ia = 255 - a;
    let rb = (dst & 0xFF00FF) * ia + (src & 0xFF00FF) * a;
    let g = (dst & 0x00FF00) * ia + (src & 0x00FF00) * a;
    let rb = ((rb + 0x80_0080 + ((rb >> 8) & 0xFF00FF)) >> 8) & 0xFF00FF;
    let g = ((g + 0x8000 + ((g >> 8) & 0xFF00)) >> 8) & 0x00FF00;
    rb | g
}

pub const fn rgb(v: u32) -> Color {
    Color {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
        a: 255,
    }
}

impl Color {
    pub fn alpha(self, a: f32) -> Color {
        Color {
            a: (self.a as f32 * a.clamp(0.0, 1.0)) as u8,
            ..self
        }
    }
    pub fn mix(self, o: Color, t: f32) -> Color {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        Color {
            r: l(self.r, o.r),
            g: l(self.g, o.g),
            b: l(self.b, o.b),
            a: l(self.a, o.a),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    clip: [i32; 4], // x0, y0, x1, y1 (exklusiv)
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h) as usize],
            clip: [0, 0, w, h],
        }
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = match r {
            Some(r) => [
                (r.x.round() as i32).clamp(0, self.w),
                (r.y.round() as i32).clamp(0, self.h),
                (r.right().round() as i32).clamp(0, self.w),
                (r.bottom().round() as i32).clamp(0, self.h),
            ],
            None => [0, 0, self.w, self.h],
        };
    }

    pub fn clear(&mut self, c: Color) {
        let v = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
        self.px.fill(v);
    }

    #[inline]
    fn blend(&mut self, i: usize, c: Color, cov: u32) {
        let a = (c.a as u32 * cov + 128) * 257 >> 16;
        if a == 0 {
            return;
        }
        let v = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
        let p = unsafe { self.px.get_unchecked_mut(i) };
        *p = if a >= 255 { v } else { mix_px(*p, v, a) };
    }

    /// Einzelner Punkt mit Deckung 0..=255 (für Schrift).
    #[inline]
    pub fn point(&mut self, x: i32, y: i32, c: Color, cov: u32) {
        if x >= self.clip[0] && x < self.clip[2] && y >= self.clip[1] && y < self.clip[3] {
            self.blend((y * self.w + x) as usize, c, cov);
        }
    }

    /// Deckungsmaske (z. B. gerasterter Text) in einer Farbe einblenden.
    pub fn mask(&mut self, x: i32, y: i32, w: i32, h: i32, m: &[u8], c: Color) {
        let (cx0, cx1) = (x.max(self.clip[0]), (x + w).min(self.clip[2]));
        if cx0 >= cx1 {
            return;
        }
        for py in y.max(self.clip[1])..(y + h).min(self.clip[3]) {
            let src = ((py - y) * w + (cx0 - x)) as usize;
            let dst = (py * self.w + cx0) as usize;
            for i in 0..(cx1 - cx0) as usize {
                let a = m[src + i];
                if a != 0 {
                    self.blend(dst + i, c, a as u32);
                }
            }
        }
    }

    fn span(&mut self, y: i32, x0: i32, x1: i32, c: Color) {
        if y < self.clip[1] || y >= self.clip[3] {
            return;
        }
        let (x0, x1) = (x0.max(self.clip[0]), x1.min(self.clip[2]));
        if x0 >= x1 {
            return;
        }
        let base = (y * self.w) as usize;
        if c.a == 255 {
            let v = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
            self.px[base + x0 as usize..base + x1 as usize].fill(v);
        } else if c.a > 0 {
            // durchscheinend: ganze Zeile in einem Zug mischen (zwei Kanäle je Multiplikation)
            let v = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
            let a = c.a as u32;
            for p in &mut self.px[base + x0 as usize..base + x1 as usize] {
                *p = mix_px(*p, v, a);
            }
        }
    }

    pub fn rect(&mut self, r: Rect, c: Color) {
        let (x0, x1) = (r.x.round() as i32, r.right().round() as i32);
        for y in r.y.round() as i32..r.bottom().round() as i32 {
            self.span(y, x0, x1, c);
        }
    }

    /// Senkrechter Farbverlauf (oben → unten). Für den Hintergrund.
    pub fn vgrad(&mut self, r: Rect, top: Color, bottom: Color) {
        let (y0, y1) = (r.y.round() as i32, r.bottom().round() as i32);
        let (x0, x1) = (r.x.round() as i32, r.right().round() as i32);
        let span = (y1 - y0).max(1) as f32;
        for y in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let t = (y - y0) as f32 / span;
            self.span(y, x0, x1, top.mix(bottom, t));
        }
    }

    /// Weicher Schatten unter einem abgerundeten Rechteck (günstig: wenige gestapelte Lagen).
    /// Das Innere (vom Rechteck selbst danach verdeckt) wird ausgespart – nur der Rand wird gemischt.
    pub fn soft_shadow(&mut self, r: Rect, rad: f32, c: Color) {
        let saved = self.clip;
        let (hx0, hx1) = ((r.x + 1.0).ceil() as i32, (r.right() - 1.0).floor() as i32);
        let (hy0, hy1) = (
            (r.y + rad + 1.0).ceil() as i32,
            (r.bottom() - rad - 1.0).floor() as i32,
        );
        let parts: [[i32; 4]; 4] = if hx1 > hx0 && hy1 > hy0 {
            [
                [i32::MIN, i32::MIN, i32::MAX, hy0],
                [i32::MIN, hy1, i32::MAX, i32::MAX],
                [i32::MIN, hy0, hx0, hy1],
                [hx1, hy0, i32::MAX, hy1],
            ]
        } else {
            [
                [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
                [0; 4],
                [0; 4],
                [0; 4],
            ]
        };
        for i in 0..3 {
            let g = (i as f32 + 1.0) * 2.0;
            let a = 0.10 - i as f32 * 0.025;
            let lr = Rect::new(r.x - g, r.y - g + g * 1.4, r.w + 2.0 * g, r.h + 2.0 * g);
            for p in &parts {
                let cl = [
                    saved[0].max(p[0]),
                    saved[1].max(p[1]),
                    saved[2].min(p[2]),
                    saved[3].min(p[3]),
                ];
                if cl[0] >= cl[2] || cl[1] >= cl[3] {
                    continue;
                }
                self.clip = cl;
                self.rrect(lr, rad + g, c.alpha(a));
            }
        }
        self.clip = saved;
    }

    /// Abgerundetes Rechteck.
    pub fn rrect(&mut self, r: Rect, rad: f32, c: Color) {
        let (x0, y0, x1, y1) = (
            r.x.round() as i32,
            r.y.round() as i32,
            r.right().round() as i32,
            r.bottom().round() as i32,
        );
        let rad = rad
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0)
            .max(0.0);
        let ri = rad.ceil() as i32;
        for y in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let fy = y as f32 + 0.5;
            let dy = if fy < y0 as f32 + rad {
                y0 as f32 + rad - fy
            } else if fy > y1 as f32 - rad {
                fy - (y1 as f32 - rad)
            } else {
                0.0
            };
            if dy <= 0.0 {
                self.span(y, x0, x1, c);
                continue;
            }
            self.span(y, x0 + ri, x1 - ri, c);
            for i in 0..ri {
                let dx = rad - (i as f32 + 0.5);
                let d = (dx * dx + dy * dy).sqrt() - rad;
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
                if cov > 0 {
                    self.point(x0 + i, y, c, cov);
                    self.point(x1 - 1 - i, y, c, cov);
                }
            }
        }
    }

    /// Abgerundetes Rechteck mit senkrechtem Farbverlauf (oben → unten).
    pub fn rrect_vgrad(&mut self, r: Rect, rad: f32, top: Color, bottom: Color) {
        let hh = r.h.max(1.0);
        let (x0, y0, x1, y1) = (
            r.x.round() as i32,
            r.y.round() as i32,
            r.right().round() as i32,
            r.bottom().round() as i32,
        );
        let rad = rad
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0)
            .max(0.0);
        let ri = rad.ceil() as i32;
        for y in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let fy = y as f32 + 0.5;
            let c = top.mix(bottom, ((fy - r.y) / hh).clamp(0.0, 1.0));
            let dy = if fy < y0 as f32 + rad {
                y0 as f32 + rad - fy
            } else if fy > y1 as f32 - rad {
                fy - (y1 as f32 - rad)
            } else {
                0.0
            };
            if dy <= 0.0 {
                self.span(y, x0, x1, c);
                continue;
            }
            self.span(y, x0 + ri, x1 - ri, c);
            for i in 0..ri {
                let dx = rad - (i as f32 + 0.5);
                let d = (dx * dx + dy * dy).sqrt() - rad;
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
                if cov > 0 {
                    self.point(x0 + i, y, c, cov);
                    self.point(x1 - 1 - i, y, c, cov);
                }
            }
        }
    }

    /// Beliebige Form über Abstandsfunktion (negativ = innen).
    pub fn sdf(&mut self, bb: Rect, c: Color, f: impl Fn(f32, f32) -> f32) {
        for y in
            (bb.y.floor() as i32).max(self.clip[1])..(bb.bottom().ceil() as i32).min(self.clip[3])
        {
            for x in (bb.x.floor() as i32).max(self.clip[0])
                ..(bb.right().ceil() as i32).min(self.clip[2])
            {
                let d = f(x as f32 + 0.5, y as f32 + 0.5);
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
                if cov > 0 {
                    self.blend((y * self.w + x) as usize, c, cov);
                }
            }
        }
    }

    /// Form über Abstandsfunktion mit senkrechtem Farbverlauf (oben → unten über `bb`).
    pub fn sdf_vgrad(&mut self, bb: Rect, top: Color, bottom: Color, f: impl Fn(f32, f32) -> f32) {
        let h = bb.h.max(1.0);
        for y in
            (bb.y.floor() as i32).max(self.clip[1])..(bb.bottom().ceil() as i32).min(self.clip[3])
        {
            let c = top.mix(bottom, ((y as f32 - bb.y) / h).clamp(0.0, 1.0));
            for x in (bb.x.floor() as i32).max(self.clip[0])
                ..(bb.right().ceil() as i32).min(self.clip[2])
            {
                let d = f(x as f32 + 0.5, y as f32 + 0.5);
                let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
                if cov > 0 {
                    self.blend((y * self.w + x) as usize, c, cov);
                }
            }
        }
    }

    /// Ellipse (optional gedreht, Winkel in Bogenmaß).
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, rot: f32, c: Color) {
        let m = rx.max(ry) + 1.5;
        let (sn, cs) = rot.sin_cos();
        let k = rx.min(ry);
        self.sdf(Rect::new(cx - m, cy - m, 2.0 * m, 2.0 * m), c, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let (u, v) = (dx * cs + dy * sn, -dx * sn + dy * cs);
            (((u / rx).powi(2) + (v / ry).powi(2)).sqrt() - 1.0) * k
        });
    }

    /// Punktraster („Punktmatrix") über einen Bereich legen: Gitterlinien in der Untergrundfarbe.
    /// `bg(y)` liefert die Untergrundfarbe je Zeile (Verlauf).
    pub fn dot_screen(&mut self, r: Rect, period: f32, bg: impl Fn(f32) -> Color) {
        let gap = (period * 0.36).max(1.0);
        let (x0, y0) = (r.x.floor() as i32, r.y.floor() as i32);
        let (x1, y1) = (r.right().ceil() as i32, r.bottom().ceil() as i32);
        for y in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let c = bg(y as f32);
            let fy = (y - y0) as f32 % period;
            let row_gap = fy >= period - gap;
            for x in x0.max(self.clip[0])..x1.min(self.clip[2]) {
                let fx = (x - x0) as f32 % period;
                if row_gap || fx >= period - gap {
                    self.blend((y * self.w + x) as usize, c, 255);
                }
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        self.sdf(
            Rect::new(cx - r - 1.0, cy - r - 1.0, 2.0 * r + 2.0, 2.0 * r + 2.0),
            c,
            |x, y| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r,
        );
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, th: f32, c: Color) {
        let o = r + th;
        self.sdf(
            Rect::new(cx - o - 1.0, cy - o - 1.0, 2.0 * o + 2.0, 2.0 * o + 2.0),
            c,
            |x, y| (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs() - th / 2.0,
        );
    }

    /// Linie mit runden Enden.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, th: f32, c: Color) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let p = th / 2.0 + 1.0;
        let bb = Rect::new(
            x0.min(x1) - p,
            y0.min(y1) - p,
            (x1 - x0).abs() + 2.0 * p,
            (y1 - y0).abs() + 2.0 * p,
        );
        self.sdf(bb, c, |x, y| {
            let t = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
            ((x - x0 - t * dx).powi(2) + (y - y0 - t * dy).powi(2)).sqrt() - th / 2.0
        });
    }

    /// RGBA-Bild (eigener Speicher, z. B. Foto) skaliert zeichnen – nächster Nachbar, deckend; beachtet den Ausschnitt.
    pub fn blit_rgba(&mut self, rgba: &[u8], iw: u32, ih: u32, dst: Rect) {
        let (x0, y0, x1, y1) = (
            dst.x.round() as i32,
            dst.y.round() as i32,
            (dst.x + dst.w).round() as i32,
            (dst.y + dst.h).round() as i32,
        );
        if x1 <= x0 || y1 <= y0 || iw == 0 || ih == 0 || rgba.len() < (iw * ih * 4) as usize {
            return;
        }
        let (sx, sy) = (iw as f32 / (x1 - x0) as f32, ih as f32 / (y1 - y0) as f32);
        for py in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let iy = (((py - y0) as f32 + 0.5) * sy) as usize;
            let row = iy.min(ih as usize - 1) * iw as usize;
            for px in x0.max(self.clip[0])..x1.min(self.clip[2]) {
                let ix = ((((px - x0) as f32 + 0.5) * sx) as usize).min(iw as usize - 1);
                let o = (row + ix) * 4;
                self.px[(py * self.w + px) as usize] =
                    (rgba[o] as u32) << 16 | (rgba[o + 1] as u32) << 8 | rgba[o + 2] as u32;
            }
        }
    }

    /// XRGB-Bild (z. B. Kamerabild) zeichnen: Ausschnitt `src` (in Bildpunkten) auf `dst`, wahlweise gespiegelt.
    pub fn blit_xrgb(&mut self, px: &[u32], iw: u32, ih: u32, src: Rect, dst: Rect, mirror: bool) {
        let (x0, y0, x1, y1) = (
            dst.x.round() as i32,
            dst.y.round() as i32,
            dst.right().round() as i32,
            dst.bottom().round() as i32,
        );
        if x1 <= x0 || y1 <= y0 || iw == 0 || ih == 0 || px.len() < (iw * ih) as usize {
            return;
        }
        let (sx, sy) = (src.w / (x1 - x0) as f32, src.h / (y1 - y0) as f32);
        for py in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let iy =
                ((src.y + ((py - y0) as f32 + 0.5) * sy) as i32).clamp(0, ih as i32 - 1) as usize;
            let row = iy * iw as usize;
            let base = (py * self.w) as usize;
            for qx in x0.max(self.clip[0])..x1.min(self.clip[2]) {
                let rel = if mirror {
                    (x1 - 1 - qx) as f32
                } else {
                    (qx - x0) as f32
                };
                let ix = ((src.x + (rel + 0.5) * sx) as i32).clamp(0, iw as i32 - 1) as usize;
                self.px[base + qx as usize] = px[row + ix] & 0x00FF_FFFF;
            }
        }
    }

    /// Ecken eines Rechtecks mit einer Farbe „abrunden" (für Bilder, die eckig gezeichnet wurden).
    pub fn round_corners(&mut self, r: Rect, rad: f32, bg: Color) {
        let (x0, y0, x1, y1) = (
            r.x.round(),
            r.y.round(),
            r.right().round(),
            r.bottom().round(),
        );
        for (cx, cy, bx, by) in [
            (x0 + rad, y0 + rad, x0, y0),
            (x1 - rad, y0 + rad, x1 - rad, y0),
            (x0 + rad, y1 - rad, x0, y1 - rad),
            (x1 - rad, y1 - rad, x1 - rad, y1 - rad),
        ] {
            self.sdf(Rect::new(bx, by, rad, rad), bg, |x, y| {
                rad - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()
            });
        }
    }

    /// Bildfläche abdunkeln/aufhellen (Schleier), z. B. über einem Hintergrundbild.
    pub fn veil(&mut self, r: Rect, top: Color, bottom: Color) {
        let (y0, y1) = (r.y.round() as i32, r.bottom().round() as i32);
        let (x0, x1) = (r.x.round() as i32, r.right().round() as i32);
        let span = (y1 - y0).max(1) as f32;
        for y in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let t = (y - y0) as f32 / span;
            let c = Color {
                r: top.r,
                g: top.g,
                b: top.b,
                a: (top.a as f32 + (bottom.a as f32 - top.a as f32) * t) as u8,
            };
            let c = Color {
                r: (c.r as f32 + (bottom.r as f32 - c.r as f32) * t) as u8,
                g: (c.g as f32 + (bottom.g as f32 - c.g as f32) * t) as u8,
                b: (c.b as f32 + (bottom.b as f32 - c.b as f32) * t) as u8,
                a: c.a,
            };
            self.span(y, x0, x1, c);
        }
    }

    /// RGBA-Bild skaliert zeichnen (bilineare Abtastung, mit Transparenz).
    pub fn image(&mut self, im: &crate::brand::Img, x: f32, y: f32, w: f32, h: f32) {
        let (x0, y0, x1, y1) = (
            x.round() as i32,
            y.round() as i32,
            (x + w).round() as i32,
            (y + h).round() as i32,
        );
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let (bw, bh) = (x1 - x0, y1 - y0);
        // Skaliertes Bild je (Quelle, Größe) merken – Logos werden sonst in jedem Bild neu gerechnet
        let key = (im.rgba.as_ptr() as usize, im.rgba.len(), bw, bh);
        let buf = IMG_CACHE
            .with(|c| c.borrow().get(&key).cloned())
            .unwrap_or_else(|| {
                let b = std::rc::Rc::new(scale_rgba(im, bw, bh));
                IMG_CACHE.with(|c| {
                    let mut c = c.borrow_mut();
                    if c.len() > 24 {
                        c.clear();
                    }
                    c.insert(key, b.clone());
                });
                b
            });
        for py in y0.max(self.clip[1])..y1.min(self.clip[3]) {
            let row = ((py - y0) * bw) as usize;
            for px in x0.max(self.clip[0])..x1.min(self.clip[2]) {
                let (v, a) = buf[row + (px - x0) as usize];
                if a == 0 {
                    continue;
                }
                let p = &mut self.px[(py * self.w + px) as usize];
                *p = if a >= 255 { v } else { mix_px(*p, v, a as u32) };
            }
        }
    }
}

thread_local! {
    static IMG_CACHE: std::cell::RefCell<std::collections::HashMap<(usize, usize, i32, i32), std::rc::Rc<Vec<(u32, u8)>>>> = Default::default();
}

/// Bilinear skalieren (mit Alpha vorgewichtet, damit transparente Ränder nicht ausfransen) → (0x00RRGGBB, Deckung)
fn scale_rgba(im: &crate::brand::Img, bw: i32, bh: i32) -> Vec<(u32, u8)> {
    let mut out = vec![(0u32, 0u8); (bw * bh) as usize];
    let (sx, sy) = (im.w as f32 / bw as f32, im.h as f32 / bh as f32);
    let at = |ix: usize, iy: usize, c: usize| im.rgba[(iy * im.w as usize + ix) * 4 + c] as f32;
    for py in 0..bh {
        let fy = (py as f32 + 0.5) * sy - 0.5;
        let iy = (fy.max(0.0) as usize).min(im.h as usize - 1);
        let iy2 = (iy + 1).min(im.h as usize - 1);
        let ty = (fy - iy as f32).clamp(0.0, 1.0);
        for px in 0..bw {
            let fx = (px as f32 + 0.5) * sx - 0.5;
            let ix = (fx.max(0.0) as usize).min(im.w as usize - 1);
            let ix2 = (ix + 1).min(im.w as usize - 1);
            let tx = (fx - ix as f32).clamp(0.0, 1.0);
            let mut acc = [0f32; 4];
            for (cx, cy, wgt) in [
                (ix, iy, (1.0 - tx) * (1.0 - ty)),
                (ix2, iy, tx * (1.0 - ty)),
                (ix, iy2, (1.0 - tx) * ty),
                (ix2, iy2, tx * ty),
            ] {
                let a = at(cx, cy, 3) * wgt;
                acc[0] += at(cx, cy, 0) * a;
                acc[1] += at(cx, cy, 1) * a;
                acc[2] += at(cx, cy, 2) * a;
                acc[3] += a;
            }
            if acc[3] < 1.0 {
                continue;
            }
            let v = ((acc[0] / acc[3]) as u32) << 16
                | ((acc[1] / acc[3]) as u32) << 8
                | (acc[2] / acc[3]) as u32;
            out[(py * bw + px) as usize] = (v, acc[3].min(255.0) as u8);
        }
    }
    out
}
