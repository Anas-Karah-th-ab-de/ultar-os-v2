//! Die Textschicht einer PDF-Seite: Glyphen aus dem Interpreter einsammeln, zu Zeilen und
//! Wörtern mit Rahmen ordnen (arabische Zeilen von rechts nach links) und Präsentationsformen
//! auf Grundbuchstaben zurückführen.

use crate::pdf::{Line, Word};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Device, DrawMode, DrawProps, ImageDrawProps, SoftMask,
};
use hayro::kurbo::BezPath;

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
        Some((x0, y0, x1, y1)) => (x0.min(b.0), y0.min(b.1), x1.max(b.2), y1.max(b.3)),
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

/// Glyphen nach Zeilen gruppieren (oben zuerst; y wächst im PDF-Raum nach oben).
fn group_lines(mut gs: Vec<G>) -> Vec<Vec<G>> {
    gs.retain(|g| !g.s.is_empty());
    gs.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut groups: Vec<Vec<G>> = Vec::new();
    for g in gs {
        match groups.last_mut() {
            Some(l) if (l[0].y - g.y).abs() < l[0].size.max(g.size) * 0.45 => l.push(g),
            _ => groups.push(vec![g]),
        }
    }
    groups
}

/// Typischer Zeichenabstand einer nach x sortierten Zeile (Median, nach oben begrenzt).
fn median_step(l: &[G]) -> f64 {
    let mut d: Vec<f64> = l
        .windows(2)
        .map(|w| w[1].x - w[0].x)
        .filter(|v| *v > 0.01)
        .collect();
    d.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    d.get(d.len() / 2)
        .copied()
        .unwrap_or(l[0].size * 0.5)
        .min(l[0].size * 0.8)
}

/// Wörter einer nach x sortierten Zeile: Lücken hinter dem Zeichen (Vorschub der Schrift
/// bekannt: ab ~0,15 Geviert) oder, ohne Vorschub, über dem Median trennen Wörter.
fn split_words(l: &[G]) -> Vec<Word> {
    let has_space = l.iter().any(|g| g.s == " ");
    let med = median_step(l);
    let mut words: Vec<Word> = Vec::new();
    let mut cur = String::new();
    let mut bx: Option<(f32, f32, f32, f32)> = None;
    let mut prev: Option<(f64, usize, Option<f64>)> = None;
    for g in l {
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
    words
}

/// Eine Zeile aus ihren Wörtern: Text und umschließender Rahmen.
fn line_of(words: Vec<Word>) -> Line {
    let mut b = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for w in &words {
        b = (b.0.min(w.x0), b.1.min(w.y0), b.2.max(w.x1), b.3.max(w.y1));
    }
    if words.is_empty() {
        b = (0.0, 0.0, 0.0, 0.0);
    }
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Line {
        text,
        words,
        x0: b.0,
        y0: b.1,
        x1: b.2,
        y1: b.3,
    }
}

/// Glyphen zu Zeilen gruppieren und Wörter mit Rahmen bilden (y nach oben).
pub(crate) fn arrange(gs: Vec<G>) -> Vec<Line> {
    let mut out = Vec::new();
    for mut l in group_lines(gs) {
        let ar = l.iter().filter(|g| arabic(&g.s)).count() * 2 > l.len();
        l.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        let words = split_words(&l);
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
        out.push(line_of(words));
    }
    out
}

#[derive(Clone)]
pub(crate) struct G {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) size: f64,
    pub(crate) adv: Option<f64>,
    pub(crate) s: String,
}

#[derive(Default)]
pub(crate) struct TextDev {
    pub(crate) glyphs: Vec<G>,
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
        0xFEF5 | 0xFEF6 => "\u{0644}\u{0622}".into(),
        0xFEF7 | 0xFEF8 => "\u{0644}\u{0623}".into(),
        0xFEF9 | 0xFEFA => "\u{0644}\u{0625}".into(),
        0xFEFB | 0xFEFC => "\u{0644}\u{0627}".into(),
        0xFDF2 => "\u{0627}\u{0644}\u{0644}\u{0647}".into(),
        0xFEFF => String::new(),
        _ => ch.to_string(),
    }
}

fn arabic(s: &str) -> bool {
    s.chars()
        .any(|c| (0x0600..=0x06FF).contains(&(c as u32)) || (0xFB50..=0xFEFC).contains(&(c as u32)))
}

/// Lesetext einer Seite: die Zeilen von `arrange`, eine Leerzeile bei großem Abstand.
pub(crate) fn layout(gs: Vec<G>) -> String {
    let mut out = String::new();
    let mut last: Option<(f32, f32)> = None;
    for l in arrange(gs) {
        let size = (l.y1 - l.y0).max(0.1);
        if let Some((ly, ls)) = last {
            if ly - l.y0 > ls * 2.2 {
                out.push('\n');
            }
        }
        last = Some((l.y0, size));
        out.push_str(&l.text);
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
