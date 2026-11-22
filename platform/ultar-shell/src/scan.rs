//! Dokumentenscanner: Blatt im Bild finden, entzerren, aufbereiten, als PDF ablegen.
//! Alles reine Funktionen auf XRGB-Bildern (`0x00RRGGBB`), ohne Abhängigkeiten – damit testbar.
//!
//! Ablauf: `detect` (Ecken des Blatts) → `warp` (perspektivisch gerade ziehen) → `enhance`
//! (Beleuchtung ausgleichen, Farbe/Grau/Schwarz-Weiß) → JPEG → `pdf` (mehrseitig, A4).

/// Ecken eines Blatts, normiert auf 0..1: oben links, oben rechts, unten rechts, unten links.
pub type Quad = [(f32, f32); 4];

pub const FULL: Quad = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

#[inline]
fn luma(p: u32) -> u32 {
    (((p >> 16) & 255) * 77 + ((p >> 8) & 255) * 150 + (p & 255) * 29) >> 8
}

/// Auf höchstens `max` Pixel Kantenlänge verkleinertes Graubild (Mittelwert je Block).
fn small_gray(px: &[u32], w: u32, h: u32, max: u32) -> (usize, usize, Vec<u8>) {
    let k = ((w.max(h) + max - 1) / max).max(1) as usize;
    let (sw, sh) = ((w as usize / k).max(1), (h as usize / k).max(1));
    let mut g = vec![0u8; sw * sh];
    // je Block nur ein 2x2-Raster abtasten – reicht für die Suche und ist schnell
    let st = (k / 2).max(1);
    for y in 0..sh {
        for x in 0..sw {
            let (mut s, mut n) = (0u32, 0u32);
            let mut yy = y * k;
            while yy < (y + 1) * k && yy < h as usize {
                let mut xx = x * k;
                while xx < (x + 1) * k && xx < w as usize {
                    s += luma(px[yy * w as usize + xx]);
                    n += 1;
                    xx += st;
                }
                yy += st;
            }
            g[y * sw + x] = (s / n.max(1)) as u8;
        }
    }
    (sw, sh, g)
}

/// Otsu-Schwelle eines Graubilds.
fn otsu(g: &[u8]) -> u8 {
    let mut hist = [0u32; 256];
    for &v in g {
        hist[v as usize] += 1;
    }
    let total = g.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut wb, mut sb, mut best, mut thr) = (0f64, 0f64, -1f64, 127u8);
    for (i, &c) in hist.iter().enumerate() {
        wb += c as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += i as f64 * c as f64;
        let (mb, mf) = (sb / wb, (sum - sb) / wf);
        let v = wb * wf * (mb - mf) * (mb - mf);
        if v > best {
            best = v;
            thr = i as u8;
        }
    }
    thr
}

/// Blatt suchen: helle, zusammenhängende Fläche vor dunklerem Grund. Liefert die vier Ecken
/// oder `None`, wenn kein brauchbares Viereck gefunden wurde (dann nimmt man das ganze Bild).
pub fn detect(px: &[u32], w: u32, h: u32) -> Option<Quad> {
    if w < 16 || h < 16 || (px.len() as u64) < w as u64 * h as u64 {
        return None;
    }
    let (sw, sh, g) = small_gray(px, w, h, 160);
    let thr = otsu(&g);
    // Kontrast zu gering → kein Blatt erkennbar
    let (mut lo, mut hi, mut nl, mut nh) = (0u64, 0u64, 0u64, 0u64);
    for &v in &g {
        if v > thr {
            hi += v as u64;
            nh += 1;
        } else {
            lo += v as u64;
            nl += 1;
        }
    }
    if nl == 0 || nh == 0 || (hi / nh) < (lo / nl) + 24 {
        return None;
    }
    // größte helle Zusammenhangskomponente (4er-Nachbarschaft)
    let n = sw * sh;
    let mut lab = vec![0u32; n];
    let mut stack: Vec<usize> = Vec::new();
    let (mut best_id, mut best_n, mut id) = (0u32, 0usize, 0u32);
    for s in 0..n {
        if g[s] <= thr || lab[s] != 0 {
            continue;
        }
        id += 1;
        let mut cnt = 0usize;
        lab[s] = id;
        stack.push(s);
        while let Some(i) = stack.pop() {
            cnt += 1;
            let (x, y) = (i % sw, i / sw);
            let mut go = |j: usize, stack: &mut Vec<usize>| {
                if g[j] > thr && lab[j] == 0 {
                    lab[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1, &mut stack);
            }
            if x + 1 < sw {
                go(i + 1, &mut stack);
            }
            if y > 0 {
                go(i - sw, &mut stack);
            }
            if y + 1 < sh {
                go(i + sw, &mut stack);
            }
        }
        if cnt > best_n {
            best_n = cnt;
            best_id = id;
        }
    }
    let frac = best_n as f32 / n as f32;
    if !(0.10..=0.97).contains(&frac) {
        return None;
    }
    // Ecken = Extrempunkte entlang der Diagonalen
    let (mut tl, mut tr, mut br, mut bl) = ((0usize, 0usize), (0usize, 0usize), (0usize, 0usize), (0usize, 0usize));
    let (mut s_min, mut s_max, mut d_min, mut d_max) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    for i in 0..n {
        if lab[i] != best_id {
            continue;
        }
        let (x, y) = (i % sw, i / sw);
        // Seitenverhältnis ausgleichen, damit die Diagonalen auch bei breiten Bildern passen
        let (xs, ys) = ((x * sh) as i64, (y * sw) as i64);
        let (s, d) = (xs + ys, xs - ys);
        if s < s_min {
            s_min = s;
            tl = (x, y);
        }
        if s > s_max {
            s_max = s;
            br = (x, y);
        }
        if d > d_max {
            d_max = d;
            tr = (x, y);
        }
        if d < d_min {
            d_min = d;
            bl = (x, y);
        }
    }
    let nrm = |p: (usize, usize), dx: f32, dy: f32| (((p.0 as f32 + dx) / sw as f32).clamp(0.0, 1.0), ((p.1 as f32 + dy) / sh as f32).clamp(0.0, 1.0));
    let q: Quad = [nrm(tl, 0.0, 0.0), nrm(tr, 1.0, 0.0), nrm(br, 1.0, 1.0), nrm(bl, 0.0, 1.0)];
    if valid(&q) {
        Some(q)
    } else {
        None
    }
}

/// Viereck brauchbar? (konvex, richtig herum, Seiten nicht zu kurz)
pub fn valid(q: &Quad) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        let cr = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cr.abs() < 1e-4 {
            return false;
        }
        if sign == 0.0 {
            sign = cr.signum();
        } else if cr.signum() != sign {
            return false;
        }
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if len < 0.12 {
            return false;
        }
    }
    sign > 0.0
}

/// Erkennung über die Zeit beruhigen (gegen Zittern der Vorschau-Linien).
pub fn smooth(old: Option<Quad>, new: Quad) -> Quad {
    match old {
        Some(o) => {
            let far = (0..4).any(|i| (o[i].0 - new[i].0).abs() > 0.12 || (o[i].1 - new[i].1).abs() > 0.12);
            if far {
                new
            } else {
                let mut r = new;
                for i in 0..4 {
                    r[i] = (o[i].0 * 0.6 + new[i].0 * 0.4, o[i].1 * 0.6 + new[i].1 * 0.4);
                }
                r
            }
        }
        None => new,
    }
}

/// Perspektivisch entzerren: das Viereck `q` wird zum Rechteck. `max_side` begrenzt die längere Kante.
pub fn warp(px: &[u32], w: u32, h: u32, q: &Quad, max_side: u32) -> (u32, u32, Vec<u32>) {
    let (wf, hf) = (w as f32, h as f32);
    let p: Vec<(f32, f32)> = q.iter().map(|c| (c.0 * (wf - 1.0), c.1 * (hf - 1.0))).collect();
    let dist = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let mut ow = (dist(p[0], p[1]) + dist(p[3], p[2])) / 2.0;
    let mut oh = (dist(p[0], p[3]) + dist(p[1], p[2])) / 2.0;
    let k = (max_side as f32 / ow.max(oh)).min(1.0);
    ow = (ow * k).max(8.0);
    oh = (oh * k).max(8.0);
    let (ow, oh) = (ow as u32, oh as u32);
    // Abbildung Einheitsquadrat → Viereck (Heckbert): x = (a u + b v + c) / (g u + h v + 1)
    let (x0, y0, x1, y1, x2, y2, x3, y3) = (p[0].0, p[0].1, p[1].0, p[1].1, p[2].0, p[2].1, p[3].0, p[3].1);
    let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
    let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);
    let den = dx1 * dy2 - dx2 * dy1;
    let (g, hh) = if den.abs() < 1e-6 { (0.0, 0.0) } else { ((dx3 * dy2 - dx2 * dy3) / den, (dx1 * dy3 - dx3 * dy1) / den) };
    let (a, b, c) = (x1 - x0 + g * x1, x3 - x0 + hh * x3, x0);
    let (d, e, f) = (y1 - y0 + g * y1, y3 - y0 + hh * y3, y0);
    let mut out = vec![0u32; (ow * oh) as usize];
    let (wi, hi) = (w as i32, h as i32);
    for oy in 0..oh {
        let v = (oy as f32 + 0.5) / oh as f32;
        for ox in 0..ow {
            let u = (ox as f32 + 0.5) / ow as f32;
            let z = g * u + hh * v + 1.0;
            let (sx, sy) = ((a * u + b * v + c) / z, (d * u + e * v + f) / z);
            let (xi, yi) = (sx.floor() as i32, sy.floor() as i32);
            let (fx, fy) = (((sx - xi as f32) * 256.0) as u32, ((sy - yi as f32) * 256.0) as u32);
            let at = |x: i32, y: i32| px[(y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize];
            let (p00, p10, p01, p11) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
            let mut o = 0u32;
            for sh in [16u32, 8, 0] {
                let ch = |p: u32| (p >> sh) & 255;
                let top = ch(p00) * (256 - fx) + ch(p10) * fx;
                let bot = ch(p01) * (256 - fx) + ch(p11) * fx;
                o |= ((top * (256 - fy) + bot * fy) >> 16) << sh;
            }
            out[(oy * ow + ox) as usize] = o;
        }
    }
    (ow, oh, out)
}

/// Aufbereitung: 0 Farbe, 1 Grau, 2 Schwarz-Weiß.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Filter {
    Color,
    Gray,
    Bw,
}

impl Filter {
    pub fn from(n: u8) -> Filter {
        match n {
            1 => Filter::Gray,
            2 => Filter::Bw,
            _ => Filter::Color,
        }
    }
}

/// Beleuchtung ausgleichen (Papier wird gleichmäßig weiß) und Filter anwenden – an Ort und Stelle.
pub fn enhance(px: &mut [u32], w: u32, h: u32, filter: Filter) {
    let (w, h) = (w as usize, h as usize);
    if w < 8 || h < 8 || px.len() < w * h {
        return;
    }
    // Papierhelligkeit je Kachel: 85. Perzentil der Helligkeit (Text ist dunkler und fällt heraus)
    const N: usize = 16;
    let (tw, th) = ((w + N - 1) / N, (h + N - 1) / N);
    let mut tile = vec![255f32; N * N];
    let mut hist = [0u32; 256];
    for ty in 0..N {
        for tx in 0..N {
            hist.fill(0);
            let (mut cnt, y0, x0) = (0u32, ty * th, tx * tw);
            let mut y = y0;
            while y < (y0 + th).min(h) {
                let mut x = x0;
                while x < (x0 + tw).min(w) {
                    hist[luma(px[y * w + x]) as usize] += 1;
                    cnt += 1;
                    x += 2;
                }
                y += 2;
            }
            if cnt == 0 {
                continue;
            }
            let (mut acc, want) = (0u32, cnt * 85 / 100);
            let mut v = 255usize;
            for (i, &c) in hist.iter().enumerate() {
                acc += c;
                if acc >= want {
                    v = i;
                    break;
                }
            }
            tile[ty * N + tx] = (v as f32).max(40.0);
        }
    }
    // Ausreißer glätten (3x3-Mittel), damit dunkle Flächen im Blatt nicht aufgehellt werden
    let src = tile.clone();
    for ty in 0..N {
        for tx in 0..N {
            let (mut s, mut n) = (0f32, 0f32);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (yy, xx) = (ty as i32 + dy, tx as i32 + dx);
                    if yy >= 0 && xx >= 0 && (yy as usize) < N && (xx as usize) < N {
                        s += src[yy as usize * N + xx as usize];
                        n += 1.0;
                    }
                }
            }
            tile[ty * N + tx] = (s / n).max(src[ty * N + tx] * 0.9);
        }
    }
    for y in 0..h {
        let gy = ((y as f32 + 0.5) / th as f32 - 0.5).clamp(0.0, (N - 1) as f32);
        let (y0, fy) = (gy as usize, gy.fract());
        let y1 = (y0 + 1).min(N - 1);
        for x in 0..w {
            let gx = ((x as f32 + 0.5) / tw as f32 - 0.5).clamp(0.0, (N - 1) as f32);
            let (x0, fx) = (gx as usize, gx.fract());
            let x1 = (x0 + 1).min(N - 1);
            let white = (tile[y0 * N + x0] * (1.0 - fx) + tile[y0 * N + x1] * fx) * (1.0 - fy) + (tile[y1 * N + x0] * (1.0 - fx) + tile[y1 * N + x1] * fx) * fy;
            // Papier auf ~245 ziehen, leichte Kontrastanhebung
            let gain = 245.0 / white;
            let p = px[y * w + x];
            let adj = |c: u32| -> f32 {
                let v = c as f32 * gain;
                // S-Kurve light: dunkles dunkler
                let v = (v - 128.0) * 1.15 + 128.0;
                v.clamp(0.0, 255.0)
            };
            let (r, g, b) = (adj((p >> 16) & 255), adj((p >> 8) & 255), adj(p & 255));
            px[y * w + x] = match filter {
                Filter::Color => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
                Filter::Gray => {
                    let l = (r * 0.30 + g * 0.59 + b * 0.11) as u32;
                    (l << 16) | (l << 8) | l
                }
                Filter::Bw => {
                    let l = r * 0.30 + g * 0.59 + b * 0.11;
                    // weiche Schwelle: scharfe Schrift ohne Treppen
                    let v = ((l - 150.0) * 4.0 + 128.0).clamp(0.0, 255.0) as u32;
                    (v << 16) | (v << 8) | v
                }
            };
        }
    }
}

/// Mehrseitiges PDF (A4 hoch bzw. quer je Seite) aus JPEG-Seiten: `(jpeg, breite, höhe)`.
pub fn pdf(pages: &[(Vec<u8>, u32, u32)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offs: Vec<usize> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let n = pages.len();
    // Objekte: 1 Katalog, 2 Seitenbaum, dann je Seite: Seite, Inhalt, Bild
    let obj = |out: &mut Vec<u8>, offs: &mut Vec<usize>, body: &[u8]| {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offs.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, &mut offs, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: String = (0..n).map(|i| format!("{} 0 R ", 3 + i * 3)).collect();
    obj(&mut out, &mut offs, format!("<< /Type /Pages /Count {n} /Kids [ {kids}] >>").as_bytes());
    for (i, (jpg, w, h)) in pages.iter().enumerate() {
        let (pw, ph) = if w > h { (842.0f32, 595.0f32) } else { (595.0f32, 842.0f32) };
        let m = 18.0f32;
        let k = ((pw - 2.0 * m) / *w as f32).min((ph - 2.0 * m) / *h as f32);
        let (iw, ih) = (*w as f32 * k, *h as f32 * k);
        let (x, y) = ((pw - iw) / 2.0, (ph - ih) / 2.0);
        let base = 3 + i * 3;
        obj(
            &mut out,
            &mut offs,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                base + 2,
                base + 1
            )
            .as_bytes(),
        );
        let content = format!("q {iw:.2} 0 0 {ih:.2} {x:.2} {y:.2} cm /Im0 Do Q");
        obj(&mut out, &mut offs, format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).as_bytes());
        let mut body = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpg.len()).into_bytes();
        body.extend_from_slice(jpg);
        body.extend_from_slice(b"\nendstream");
        obj(&mut out, &mut offs, &body);
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offs.len() + 1).as_bytes());
    for o in &offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offs.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dunkler Tisch mit hellem, schräg liegendem Blatt (Viereck mit bekannten Ecken).
    fn scene(w: u32, h: u32, q: &Quad) -> Vec<u32> {
        let mut px = vec![0x302820u32; (w * h) as usize];
        let p: Vec<(f32, f32)> = q.iter().map(|c| (c.0 * w as f32, c.1 * h as f32)).collect();
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = (0..4).all(|i| {
                    let (a, b) = (p[i], p[(i + 1) % 4]);
                    (b.0 - a.0) * (fy - a.1) - (b.1 - a.1) * (fx - a.0) >= 0.0
                });
                if inside {
                    // Papier mit „Textzeilen"
                    px[(y * w + x) as usize] = if y % 12 < 2 && x % 7 < 5 { 0x202020 } else { 0xE8E4DC };
                }
            }
        }
        px
    }

    #[test]
    fn findet_schraeges_blatt() {
        let q: Quad = [(0.22, 0.15), (0.80, 0.20), (0.86, 0.85), (0.15, 0.78)];
        let px = scene(640, 480, &q);
        let d = detect(&px, 640, 480).expect("Blatt nicht gefunden");
        for i in 0..4 {
            assert!((d[i].0 - q[i].0).abs() < 0.04 && (d[i].1 - q[i].1).abs() < 0.04, "Ecke {i}: {:?} statt {:?}", d[i], q[i]);
        }
    }

    #[test]
    fn kein_blatt_bei_einfarbigem_bild() {
        let px = vec![0x808080u32; 320 * 240];
        assert!(detect(&px, 320, 240).is_none());
    }

    #[test]
    fn entzerren_liefert_blatt_formatfuellend() {
        let q: Quad = [(0.22, 0.15), (0.80, 0.20), (0.86, 0.85), (0.15, 0.78)];
        let px = scene(640, 480, &q);
        let (w, h, out) = warp(&px, 640, 480, &q, 1000);
        assert!(w > 300 && h > 250);
        // fast alles im entzerrten Bild ist Papier oder Text, kein Tisch (0x302820)
        let tisch = out.iter().filter(|&&p| p == 0x302820).count();
        assert!(tisch * 50 < out.len(), "{tisch} Tischpixel von {}", out.len());
        // Ecken sind Papier
        assert!(luma(out[(5 * w + 5) as usize]) > 150 || luma(out[(5 * w + 6) as usize]) < 80);
    }

    #[test]
    fn aufbereitung_macht_papier_weiss_und_text_dunkel() {
        // Blatt mit Helligkeitsverlauf (Schatten links)
        let (w, h) = (400u32, 300u32);
        let mut px = vec![0u32; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let l = 110 + x * 110 / w;
                let v = if y % 20 < 3 && x % 9 < 6 { l / 4 } else { l };
                px[(y * w + x) as usize] = (v << 16) | (v << 8) | v;
            }
        }
        let mut bw = px.clone();
        enhance(&mut bw, w, h, Filter::Bw);
        let (mut weiss, mut schwarz) = (0, 0);
        for &p in &bw {
            match p & 255 {
                230..=255 => weiss += 1,
                0..=40 => schwarz += 1,
                _ => {}
            }
        }
        assert!(weiss as f32 > bw.len() as f32 * 0.80, "weiß {weiss}");
        assert!(schwarz as f32 > bw.len() as f32 * 0.05, "schwarz {schwarz}");
        // Schatten links ist weg: Papier links und rechts gleich hell
        let mut c = px.clone();
        enhance(&mut c, w, h, Filter::Gray);
        let (l, r) = (c[(10 * w + 8) as usize] & 255, c[(10 * w + w - 8) as usize] & 255);
        assert!((l as i32 - r as i32).abs() < 25, "links {l} rechts {r}");
    }

    #[test]
    fn pdf_aufbau() {
        let jpg = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let p = pdf(&[(jpg.clone(), 1000, 1414), (jpg, 1414, 1000)]);
        let s = String::from_utf8_lossy(&p);
        assert!(s.starts_with("%PDF-1.4"));
        assert!(s.contains("/Count 2"));
        assert!(s.contains("/MediaBox [0 0 595 842]") && s.contains("/MediaBox [0 0 842 595]"));
        assert!(s.trim_end().ends_with("%%EOF"));
        // xref-Einträge zeigen auf "n 0 obj" (auf den Bytes prüfen, nicht auf dem verlustbehafteten Text)
        let pos = p.windows(9).rposition(|w| w == b"startxref").unwrap();
        let xref: usize = String::from_utf8_lossy(&p[pos + 10..]).lines().next().unwrap().trim().parse().unwrap();
        assert!(p[xref..].starts_with(b"xref"));
        let tab = String::from_utf8_lossy(&p[xref..]).to_string();
        let off: usize = tab.lines().nth(3).unwrap()[..10].parse().unwrap();
        assert!(p[off..].starts_with(b"1 0 obj"));
    }

    #[test]
    fn glaetten_und_gueltigkeit() {
        assert!(valid(&FULL));
        assert!(!valid(&[(0.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0)]));
        let a: Quad = [(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)];
        let b: Quad = [(0.12, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)];
        let s = smooth(Some(a), b);
        assert!(s[0].0 > 0.1 && s[0].0 < 0.12);
    }
}
