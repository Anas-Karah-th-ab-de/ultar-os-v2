//! Bedienelemente für Touch (Immediate-Mode): Schaltflächen, Zeilen, Schalter,
//! Regler, Textfelder, Bildschirmtastatur, Symbole.

use crate::gfx::{rgb, Canvas, Color, Rect};
use crate::text::Text;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    /// unterer Verlaufston des Hintergrunds (für Tiefe)
    pub bg2: Color,
    pub card: Color,
    pub press: Color,
    pub text: Color,
    pub sub: Color,
    pub accent: Color,
    /// hellerer Akzent (für Verläufe/Glanz auf Knöpfen)
    pub accent2: Color,
    pub on_accent: Color,
    pub line: Color,
    pub danger: Color,
    pub ok: Color,
}

/// Erste Farbe: Gold aus dem Logo von KM Code Craft (Verlauf #F0F090 → #D8C870 → #B08838),
/// dann Bronze und KM-Rot aus dem Logo, danach freie Akzente („Ultar Noir").
pub const ACCENTS: [u32; 7] = [0xD8C870, 0xB08838, 0xF80000, 0x5B8DEF, 0x4ADE80, 0xA855F7, 0x14B8A6];
/// Rot der Marke (KM im Logo) – nur für Marke, Aufnahme und Alarm
pub const KM_RED: u32 = 0xF80000;

/// Pixel je dp für eine Bildschirmgröße – damit dieselbe Oberfläche auf jedem Format passt.
/// Handy (schmal und hoch): kurze Seite = 360 dp, aber mindestens 640 dp in der Länge (kurze Handys werden nicht abgeschnitten).
/// Tablet/Monitor (breiter als 10:17 und kurze Seite ab 700 px): kurze Seite = 600 dp, es passt also mehr Inhalt aufs Bild.
pub fn scale_for(w: f32, h: f32, user: f32) -> f32 {
    let (short, long) = (w.min(h), w.max(h));
    // Handys wie die Web-Demo: 390 logische Punkte Breite (1080 px -> 2,77); Tablets 600
    let k = if short >= 700.0 && long / short < 1.7 { short / 600.0 } else { (short / 390.0).min(long / 700.0) };
    (k * user).max(0.5)
}

pub fn theme(dark: bool, accent: usize) -> Theme {
    let a = rgb(ACCENTS[accent % ACCENTS.len()]);
    // Designsprache „Ultar Noir": tiefes Schwarz (OLED), Glasflächen, Gold nur für „an"/Hauptaktion.
    if dark {
        Theme {
            bg: rgb(0x0E1014),
            bg2: rgb(0x08090B),
            card: rgb(0x14161B),
            press: rgb(0x1C1F26),
            text: rgb(0xF2F0EA),
            sub: rgb(0xA6A6AE),
            accent: a,
            // hellerer Akzent für Verläufe/Glanz (zum hellen Logo-Gold hin)
            accent2: a.mix(rgb(0xF0F090), 0.45),
            on_accent: rgb(0x10100A),
            line: rgb(0x2A2D35),
            danger: rgb(0xF87171),
            ok: rgb(0x4ADE80),
        }
    } else {
        // Hell: Gold dunkler, damit Schrift/Symbole auf Weiß lesbar bleiben (Kontrast ≥ 4,5:1)
        let ad = a.mix(rgb(0), 0.38);
        Theme {
            bg: rgb(0xF6F4EE),
            bg2: rgb(0xECE9DF),
            card: rgb(0xFFFFFF),
            press: rgb(0xECE9DF),
            text: rgb(0x17181C),
            sub: rgb(0x5D5F67),
            accent: ad,
            accent2: a,
            on_accent: rgb(0xFFFFFF),
            line: rgb(0xDCD8CB),
            danger: rgb(0xC62828),
            ok: rgb(0x16A34A),
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct Ptr {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub px: f32,
    pub py: f32,
    pub moved: bool,
    pub dy: f32,
    pub dx: f32,
    pub capture: Option<&'static str>,
    /// Mausrad: zu rollende Bildpunkte (positiv = nach unten), verbraucht von der Rollfläche unter dem Zeiger
    pub rad: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Al {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Primary,
    Plain,
    Danger,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Ic {
    Gear,
    Box,
    Term,
    List,
    Power,
    Globe,
    Sun,
    Lang,
    Clock,
    Disk,
    Info,
    Back,
    Home,
    Chevron,
    Check,
    Bot,
    Chip,
    Calc,
    Folder,
    Image,
    Shield,
    /// Funkturm (Mast mit Wellen) – Mobilfunk
    Tower,
    Wifi,
    UpDown,
    Bt,
    Plane,
    Torch,
    Speaker,
    Lock,
    Grid,
    Sliders,
    Phone,
    Chat,
    Camera,
    Play,
    Pause,
    Note,
    Person,
    Send,
    Flip,
    Flash,
    /// Drittel-Raster (Kamera-Sucher)
    Thirds,
    Timer,
    Heart,
    Trash,
    Rotate,
    Plus,
    Close,
    Spark,
    Music,
    Video,
    Star,
    Search,
    Wall,
    Apps,
    Bksp,
    Bell,
    Moon,
    /// Mikrofon (Sprachsuche / Helfer)
    Mic,
    /// Stift (Schreiben)
    Pen,
    /// Kopieren (zwei Blätter)
    Copy,
    /// Dokument (Blatt mit Zeilen)
    Doc,
    /// Herunterladen (Pfeil auf Linie)
    Download,
    /// USB-Stick
    Usb,
    /// Stecknadel (angeheftet)
    Pin,
    /// Fingerabdruck
    Finger,
    /// Schere (Ausschneiden)
    Cut,
    /// Beliebiges Demo-Symbol nach Name (siehe icons_svg.rs)
    Svg(&'static str),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
}

pub struct Ui<'a> {
    pub cv: &'a mut Canvas,
    pub tx: &'a mut Text,
    pub p: &'a mut Ptr,
    pub th: Theme,
    /// Pixel pro dp
    pub k: f32,
    pub w: f32,
    pub h: f32,
    pub rtl: bool,
    pub enabled: bool,
    pub focus: Option<&'static str>,
    pub dirty: bool,
    pub clip: Option<Rect>,
    /// Web-Adressen, die in diesem Bild als Text gezeichnet wurden (für den Helfer)
    pub links: Vec<String>,
    /// Abgleich mit der Bildschirmtastatur (fokussiertes Feld sichtbar halten)
    pub kb: KbFit,
    /// Sichtbarer Text dieses Bildes (nur wenn angefordert – für den Assistenten: „was steht auf dem Bildschirm")
    pub seen: Option<Vec<String>>,
}

/// Was die Seite beim Zeichnen über Eingabefelder und Bedienelemente meldet,
/// damit nichts hinter der Bildschirmtastatur verschwindet.
#[derive(Default)]
pub struct KbFit {
    /// Rechteck des fokussierten Eingabefelds und der dabei gültige Ausschnitt
    pub focus: std::cell::Cell<Option<(Rect, Option<Rect>)>>,
    /// Unterste Kante aller nicht abgeschnittenen Bedienelemente (Seite ragt unter die Tastatur?)
    pub reach: std::cell::Cell<f32>,
    /// Fokussiertes Feld noch in den sichtbaren Bereich holen
    pub ensure: std::cell::Cell<bool>,
    /// Untere Kante der zuletzt gezeichneten Listenzeile (aneinanderliegende Zeilen = eine Gruppe)
    pub row_end: std::cell::Cell<f32>,
    /// Farbiges Symbolfeld für die nächste Listenzeile (Einstellungen wie in der Demo: weißes Symbol)
    pub row_bg: std::cell::Cell<Option<Color>>,
    /// Wert der nächsten Listenzeile in Gold
    pub row_gold: std::cell::Cell<bool>,
    /// Symbol für die nächste Schalterzeile
    pub row_ic: std::cell::Cell<Option<Ic>>,
}

impl Ui<'_> {
    pub fn dp(&self, v: f32) -> f32 {
        (v * self.k).round()
    }

    fn in_clip(&self, x: f32, y: f32) -> bool {
        self.clip.map_or(true, |c| c.contains(x, y))
    }

    pub fn visible(&self, r: Rect) -> bool {
        self.clip.map_or(true, |c| r.bottom() > c.y && r.y < c.bottom())
    }

    pub fn tap(&self, r: Rect) -> bool {
        if self.clip.is_none() && r.bottom() > self.kb.reach.get() {
            self.kb.reach.set(r.bottom());
        }
        self.enabled && self.p.released && !self.p.moved && self.p.capture.is_none() && r.contains(self.p.px, self.p.py) && r.contains(self.p.x, self.p.y) && self.in_clip(self.p.px, self.p.py)
    }

    pub fn held(&self, r: Rect) -> bool {
        self.enabled && self.p.down && !self.p.moved && self.p.capture.is_none() && r.contains(self.p.px, self.p.py) && self.in_clip(self.p.px, self.p.py)
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = r;
        self.cv.set_clip(r);
    }

    pub fn text_w(&mut self, s: &str, size: f32, bold: bool) -> f32 {
        self.tx.measure(s, self.dp(size), bold, 0.0).0
    }

    /// Text auf eine Maximalbreite kürzen (mit „…"), damit nichts überläuft/überlappt.
    pub fn fit(&mut self, s: &str, size: f32, bold: bool, max_w: f32) -> String {
        if max_w <= 0.0 || self.text_w(s, size, bold) <= max_w {
            return s.to_string();
        }
        let ew = self.text_w("…", size, bold);
        let mut out = String::new();
        let mut w = 0.0;
        for ch in s.chars() {
            let cw = self.text_w(&ch.to_string(), size, bold);
            if w + cw + ew > max_w {
                break;
            }
            out.push(ch);
            w += cw;
        }
        out.push('…');
        out
    }

    /// Einzeiliger Text in einem Rechteck, vertikal zentriert; zu langer Text wird abgeschnitten.
    pub fn label(&mut self, r: Rect, s: &str, size: f32, c: Color, bold: bool, al: Al) {
        if s.is_empty() || !self.visible(r) {
            return;
        }
        self.note_links(s);
        self.note_seen(s);
        let sz = self.dp(size);
        let (tw, th) = self.tx.measure(s, sz, bold, 0.0);
        let al = match (al, self.rtl) {
            (Al::Start, true) => Al::End,
            (Al::End, true) => Al::Start,
            (a, _) => a,
        };
        let x = match al {
            Al::Start => r.x,
            Al::Center => r.x + (r.w - tw) / 2.0,
            Al::End => r.right() - tw,
        };
        let old = self.clip;
        if tw > r.w {
            let c2 = match old {
                Some(o) => {
                    let (x0, y0) = (r.x.max(o.x), r.y.max(o.y));
                    Rect::new(x0, y0, (r.right().min(o.right()) - x0).max(0.0), (r.bottom().min(o.bottom()) - y0).max(0.0))
                }
                None => r,
            };
            self.cv.set_clip(Some(c2));
        }
        self.tx.draw(self.cv, s, x.max(if tw > r.w && al != Al::End { r.x } else { f32::MIN }), r.cy() - th / 2.0, sz, c, bold, 0.0);
        self.cv.set_clip(old);
    }

    /// Mehrzeiliger Text; liefert die Höhe.
    /// Adressen im gezeichneten Text merken (billige Vorprüfung, damit normale Texte nichts kosten).
    fn note_links(&mut self, s: &str) {
        if self.links.len() < 8 && s.len() >= 6 && s.contains('.') && (s.contains('/') || s.contains("www.")) {
            for u in crate::linkcheck::find_urls(s) {
                if self.links.len() < 8 && !self.links.contains(&u) {
                    self.links.push(u);
                }
            }
        }
    }

    /// Text für die Bildschirm-Beschreibung merken (nur wenn angefordert, ohne Doppelte, begrenzt)
    fn note_seen(&mut self, s: &str) {
        if let Some(v) = self.seen.as_mut() {
            let t = s.trim().trim_end_matches('|');
            if !t.is_empty() && v.len() < 400 && v.last().map(|l| l != t).unwrap_or(true) {
                v.push(t.to_string());
            }
        }
    }

    pub fn para(&mut self, x: f32, y: f32, w: f32, s: &str, size: f32, c: Color) -> f32 {
        self.note_links(s);
        if self.visible(Rect::new(x, y, w, self.dp(size))) {
            self.note_seen(s);
        }
        let sz = self.dp(size);
        let h = self.tx.measure(s, sz, false, w).1;
        if self.visible(Rect::new(x, y, w, h)) {
            self.tx.draw(self.cv, s, x, y, sz, c, false, w);
        }
        h
    }

    /// Karte wie in der Web-Demo (.card): s1, feiner Rand, Radius 20
    pub fn card(&mut self, r: Rect) {
        self.card_h(r, false);
    }

    pub fn button(&mut self, r: Rect, label: &str, kind: Btn) -> bool {
        if !self.visible(r) {
            return false;
        }
        let held = self.held(r);
        // runde Pillen-Knöpfe wie in der Web-Demo (.btn: Höhe 46, Radius = halbe Höhe)
        let rad = (r.h / 2.0).min(self.dp(23.0));
        match kind {
            Btn::Primary => {
                // Grundfarbe + sanfter Glanz oben (heller Akzent) = plastischer Knopf
                let mut base = self.th.accent;
                if held {
                    base = base.mix(rgb(0), 0.2);
                }
                if !self.enabled {
                    base = base.mix(self.th.bg, 0.55);
                }
                self.cv.rrect(r, rad, base);
                self.label(r.inset(self.dp(6.0)), label, 15.0, self.th.on_accent, true, Al::Center);
            }
            Btn::Danger => {
                let bg = rgb(0xC62828);
                let bg = if held { bg.mix(rgb(0), 0.18) } else { bg };
                let bg = if self.enabled { bg } else { bg.mix(self.th.bg, 0.5) };
                self.cv.rrect(r, rad, bg);
                self.label(r.inset(self.dp(6.0)), label, 15.0, rgb(0xFFFFFF), true, Al::Center);
            }
            Btn::Plain => {
                // wie .btn.ghost in der Demo: durchsichtig mit feinem Rand, mittlere Schrift
                self.cv.rrect(r, rad, self.th.line);
                self.cv.rrect(r.inset(1.0), rad - 1.0, if held { self.th.press } else { self.th.bg });
                let lf = self.fit(label, 15.0, false, r.w - self.dp(12.0));
                self.label(r.inset(self.dp(6.0)), &lf, 15.0, self.th.text, false, Al::Center);
            }
        }
        self.tap(r)
    }

    /// Karte wie in der Web-Demo (.card): s1, feiner Rand, Radius 20
    pub fn card_h(&mut self, r: Rect, held: bool) {
        let rad = self.dp(20.0).min(r.h / 2.0);
        self.cv.rrect(r, rad, self.th.line);
        self.cv.rrect(r.inset(1.0), rad - 1.0, if held { self.th.press } else { self.th.card });
    }

    /// Avatar mit Anfangsbuchstaben (Kreis in Farbe)
    pub fn avatar(&mut self, cx: f32, cy: f32, d: f32, name: &str, col: Color) {
        self.cv.circle(cx, cy, d / 2.0, col);
        let ini: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
        let ini = if ini.is_empty() { "?".to_string() } else { ini };
        self.label(Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), &ini, d * 0.36 / self.k.max(0.01) * 1.0, rgb(0xFFFFFF), true, Al::Center);
    }

    /// Abschnittsüberschrift wie in der Web-Demo: klein, gedämpft, in Großbuchstaben
    pub fn section(&mut self, y: &mut f32, s: &str) {
        if (self.kb.row_end.get() - *y).abs() < 1.0 {
            *y += self.dp(10.0);
        }
        let r = Rect::new(self.dp(24.0), *y + self.dp(6.0), self.w - self.dp(48.0), self.dp(22.0));
        let c = self.th.sub;
        let up = s.to_uppercase();
        self.label(r, &up, 12.0, c, false, Al::Start);
        *y += self.dp(32.0);
    }

    /// Listenzeile als Karte. Liefert true bei Antippen.
    pub fn row(&mut self, y: &mut f32, icon: Option<Ic>, title: &str, sub: Option<&str>, value: Option<&str>, chevron: bool) -> bool {
        // Gruppierte Liste wie in der Web-Demo: aneinanderliegende Zeilen bilden eine Karte mit Trennlinien
        let m = self.dp(16.0);
        let h = self.dp(if sub.is_some() { 62.0 } else { 54.0 });
        let r = Rect::new(m, *y, self.w - 2.0 * m, h);
        let joined = (self.kb.row_end.get() - *y).abs() < 1.0;
        *y += h;
        self.kb.row_end.set(*y);
        if !self.visible(r) {
            self.kb.row_bg.set(None);
            self.kb.row_gold.set(false);
            return false;
        }
        let held = self.held(r) && (chevron || icon.is_some());
        let rad = self.dp(20.0);
        let bg = if held { self.th.press } else { self.th.card };
        let ln = self.th.line;
        self.cv.rrect(r, rad, ln);
        self.cv.rrect(r.inset(1.0), rad - 1.0, bg);
        if joined {
            // Ecken zur vorigen Zeile schließen (Rand außen bleibt) und eine feine Trennlinie ziehen
            let card = self.th.card;
            for (x, inx) in [(r.x, r.x + 1.0), (r.right() - rad, r.right() - rad)] {
                self.cv.rect(Rect::new(x, r.y - rad, rad, 2.0 * rad), ln);
                self.cv.rect(Rect::new(inx, r.y - rad, rad - 1.0, rad), card);
                self.cv.rect(Rect::new(inx, r.y, rad - 1.0, rad), bg);
            }
            let lr = Rect::new(r.x + 1.0, r.y, r.w - 2.0, 1.0);
            let lc = self.th.line;
            self.cv.rect(lr, lc);
        }
        let pad = self.dp(14.0);
        let mut inner = r.inset(pad);
        inner.y = r.y;
        inner.h = r.h;
        if let Some(ic) = icon {
            let s = self.dp(32.0);
            let cx = if self.rtl { inner.right() - s / 2.0 } else { inner.x + s / 2.0 };
            let (bgc, fgc) = match self.kb.row_bg.take() {
                Some(b) => (b, rgb(0xFFFFFF)),
                None => (self.th.accent.alpha(0.16), self.th.accent),
            };
            self.cv.rrect(Rect::new(cx - s / 2.0, r.cy() - s / 2.0, s, s), self.dp(10.0), bgc);
            self.icon(ic, cx, r.cy(), self.dp(17.0), fgc);
            if !self.rtl {
                inner.x += s + pad;
            }
            inner.w -= s + pad;
        }
        if chevron {
            let s = self.dp(14.0);
            let cx = if self.rtl { inner.x + s / 2.0 } else { inner.right() - s / 2.0 };
            let c = self.th.sub.alpha(0.6);
            self.icon(Ic::Chevron, cx, r.cy(), s, c);
            if self.rtl {
                inner.x += s + self.dp(6.0);
            }
            inner.w -= s + self.dp(6.0);
        }
        let mut tw = inner.w;
        if let Some(v) = value {
            // Wert auf höchstens ~58 % kürzen, damit er nie den Titel überlappt
            let v = self.fit(v, 13.0, false, inner.w * 0.58);
            let vw = self.text_w(&v, 13.0, false);
            let c = if self.kb.row_gold.take() { self.th.accent } else { self.th.sub };
            self.label(inner, &v, 13.0, c, false, Al::End);
            tw = inner.w - vw - self.dp(12.0);
        }
        let tr = if self.rtl { Rect::new(inner.right() - tw, inner.y, tw, inner.h) } else { Rect::new(inner.x, inner.y, tw, inner.h) };
        let c = self.th.text;
        match sub {
            Some(s) => {
                let tf = self.fit(title, 15.0, true, tr.w);
                self.label(Rect::new(tr.x, r.y + self.dp(9.0), tr.w, self.dp(24.0)), &tf, 15.0, c, false, Al::Start);
                let cs = self.th.sub;
                let sf = self.fit(s, 12.5, false, tr.w);
                self.label(Rect::new(tr.x, r.y + self.dp(32.0), tr.w, self.dp(20.0)), &sf, 12.5, cs, false, Al::Start);
            }
            None => {
                let tf = self.fit(title, 15.0, true, tr.w);
                self.label(tr, &tf, 15.0, c, false, Al::Start)
            }
        }
        self.tap(r)
    }

    pub fn toggle_row(&mut self, y: &mut f32, title: &str, sub: Option<&str>, on: &mut bool) -> bool {
        let m = self.dp(16.0);
        let h = self.dp(if sub.is_some() { 62.0 } else { 54.0 });
        let r = Rect::new(m, *y, self.w - 2.0 * m, h);
        let (tw, thh) = (self.dp(44.0), self.dp(26.0));
        let pad = self.dp(14.0);
        // Zeile zeichnen (Platz für den Schalter über value freihalten)
        let ic = self.kb.row_ic.take();
        let tapped = self.row(y, ic, title, sub, Some("            "), false);
        if !self.visible(r) {
            return false;
        }
        let tx = if self.rtl { r.x + pad } else { r.right() - pad - tw };
        let t = Rect::new(tx, r.cy() - thh / 2.0, tw, thh);
        // Schalter wie in der Web-Demo: an = goldene Bahn mit dunklem Knopf, aus = graue Bahn
        self.cv.rrect(t, thh / 2.0, if *on { self.th.accent } else { self.th.line.mix(self.th.sub, 0.25) });
        let kx = if *on != self.rtl { t.right() - thh / 2.0 } else { t.x + thh / 2.0 };
        let kc = if *on { self.th.on_accent } else { self.th.sub };
        self.cv.circle(kx, t.cy(), thh / 2.0 - self.dp(3.0), kc);
        if tapped {
            *on = !*on;
            self.dirty = true;
        }
        tapped
    }

    pub fn slider(&mut self, id: &'static str, r: Rect, v: &mut f32) -> bool {
        let kr = self.dp(13.0);
        let (x0, x1) = (r.x + kr, r.right() - kr);
        if self.enabled && self.p.pressed && r.contains(self.p.x, self.p.y) && self.in_clip(self.p.x, self.p.y) {
            self.p.capture = Some(id);
        }
        let mut changed = false;
        if self.p.capture == Some(id) && (self.p.down || self.p.released) {
            let mut nv = ((self.p.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
            if self.rtl {
                nv = 1.0 - nv;
            }
            if (nv - *v).abs() > 0.002 {
                *v = nv;
                changed = true;
                self.dirty = true;
            }
        }
        let th = self.dp(6.0);
        let pos = if self.rtl { 1.0 - *v } else { *v };
        let kx = x0 + (x1 - x0) * pos;
        self.cv.rrect(Rect::new(x0, r.cy() - th / 2.0, x1 - x0, th), th / 2.0, self.th.line);
        let fill = if self.rtl { Rect::new(kx, r.cy() - th / 2.0, x1 - kx, th) } else { Rect::new(x0, r.cy() - th / 2.0, kx - x0, th) };
        self.cv.rrect(fill, th / 2.0, self.th.accent);
        self.cv.circle(kx, r.cy(), kr, self.th.accent);
        self.cv.circle(kx, r.cy(), kr - self.dp(4.0), rgb(0xFFFFFF));
        changed
    }

    /// Auswahlleiste; liefert den neu gewählten Index.
    pub fn chips(&mut self, y: &mut f32, opts: &[&str], sel: usize) -> Option<usize> {
        // Reiter wie in der Web-Demo (.tabs): Leiste s1 mit Rand, gewählter Teil s3
        let m = self.dp(16.0);
        let h = self.dp(46.0);
        let r = Rect::new(m, *y, self.w - 2.0 * m, h);
        *y += h + self.dp(12.0);
        if !self.visible(r) {
            return None;
        }
        self.cv.rrect(r, self.dp(16.0), self.th.line);
        self.cv.rrect(r.inset(1.0), self.dp(15.0), self.th.card);
        let cw = r.w / opts.len() as f32;
        let mut out = None;
        for (i, o) in opts.iter().enumerate() {
            let idx = if self.rtl { opts.len() - 1 - i } else { i };
            let c = Rect::new(r.x + cw * idx as f32, r.y, cw, h).inset(self.dp(5.0));
            if i == sel {
                self.cv.rrect(c, self.dp(12.0), self.th.line);
            }
            let col = if i == sel { self.th.text } else { self.th.sub };
            let fs = if self.text_w(o, 13.5, false) <= c.w - self.dp(6.0) { 13.5 } else { 12.0 };
            let of = self.fit(o, fs, false, c.w - self.dp(4.0));
            self.label(c, &of, fs, col, false, Al::Center);
            if self.tap(c) && i != sel {
                out = Some(i);
                self.dirty = true;
            }
        }
        out
    }

    /// Zeile mit −/+; liefert -1, 0 oder 1.
    pub fn stepper(&mut self, y: &mut f32, title: &str, value: &str) -> i32 {
        // wie eine Listenzeile der Demo; − und + als runde Knöpfe (.ib.fill)
        let m = self.dp(16.0);
        let h = self.dp(56.0);
        let r = Rect::new(m, *y, self.w - 2.0 * m, h);
        *y += h + self.dp(10.0);
        if !self.visible(r) {
            return 0;
        }
        self.card(r);
        let pad = self.dp(12.0);
        let b = self.dp(38.0);
        let (minus, plus) = if self.rtl {
            (Rect::new(r.x + pad + b + self.dp(8.0), r.cy() - b / 2.0, b, b), Rect::new(r.x + pad, r.cy() - b / 2.0, b, b))
        } else {
            (Rect::new(r.right() - pad - 2.0 * b - self.dp(8.0), r.cy() - b / 2.0, b, b), Rect::new(r.right() - pad - b, r.cy() - b / 2.0, b, b))
        };
        let lw = r.w - 2.0 * pad - 2.0 * b - self.dp(20.0);
        let lr = if self.rtl { Rect::new(r.right() - pad - lw, r.y, lw, h) } else { Rect::new(r.x + pad, r.y, lw, h) };
        let (ct, cs) = (self.th.text, self.th.sub);
        self.label(lr, title, 15.0, ct, false, Al::Start);
        self.label(lr, value, 15.0, cs, false, Al::End);
        let mut d = 0;
        for (rc, sign) in [(minus, -1), (plus, 1)] {
            let held = self.held(rc);
            self.cv.circle(rc.cx(), rc.cy(), b / 2.0, if held { self.th.line } else { self.th.press });
            let a = self.th.text;
            let l = self.dp(7.0);
            let t = self.dp(2.5);
            self.cv.line(rc.cx() - l, rc.cy(), rc.cx() + l, rc.cy(), t, a);
            if sign > 0 {
                self.cv.line(rc.cx(), rc.cy() - l, rc.cx(), rc.cy() + l, t, a);
            }
            if self.tap(rc) {
                d = sign;
                self.dirty = true;
            }
        }
        d
    }

    pub fn field(&mut self, r: Rect, id: &'static str, val: &str, hint: &str) {
        // Kennwörter/PINs nie in die Bildschirm-Beschreibung für den Assistenten
        let hidden = ultar_ime::purpose_for(id).sensitive();
        let saved = if hidden { self.seen.take() } else { None };
        self.field_inner(r, id, val, hint);
        if hidden {
            self.seen = saved;
            if let Some(v) = self.seen.as_mut() {
                v.push(format!("[{id}: hidden]"));
            }
        }
    }

    fn field_inner(&mut self, r: Rect, id: &'static str, val: &str, hint: &str) {
        let focused = self.focus == Some(id);
        if focused {
            self.kb.focus.set(Some((r, self.clip)));
        }
        // Eingabefeld wie in der Web-Demo (.inp): s1, feiner Rand, im Fokus goldener Rand
        let rad = self.dp(14.0);
        self.cv.rrect(r, rad, if focused { self.th.accent } else { self.th.line });
        self.cv.rrect(r.inset(1.0), rad - 1.0, self.th.card);
        let inner = Rect::new(r.x + self.dp(14.0), r.y, r.w - self.dp(28.0), r.h);
        if val.is_empty() && !focused {
            let c = self.th.sub;
            self.label(inner, hint, 16.0, c, false, Al::Start);
        } else {
            let shown = if focused { format!("{val}|") } else { val.to_string() };
            let c = self.th.text;
            // langen Text am Ende zeigen
            let tw = self.text_w(&shown, 16.0, false);
            let al = if tw > inner.w { Al::End } else { Al::Start };
            let al = if self.rtl && tw <= inner.w { Al::Start } else { al };
            self.label(inner, &shown, 16.0, c, false, al);
        }
        if self.tap(r) && !focused {
            self.focus = Some(id);
            self.dirty = true;
        }
    }

    pub fn progress(&mut self, r: Rect, v: f32) {
        self.cv.rrect(r, r.h / 2.0, self.th.line);
        let w = (r.w * v.clamp(0.0, 1.0)).max(r.h);
        let c = if v > 0.9 { self.th.danger } else { self.th.accent };
        let fr = if self.rtl { Rect::new(r.right() - w, r.y, w, r.h) } else { Rect::new(r.x, r.y, w, r.h) };
        self.cv.rrect(fr, r.h / 2.0, c);
    }

    /// Runder Symbol-Knopf (z. B. in Kamera/Galerie). Liefert true bei Antippen.
    pub fn icon_btn(&mut self, cx: f32, cy: f32, r: f32, ic: Ic, bg: Color, fg: Color) -> bool {
        let hit = Rect::new(cx - r * 1.25, cy - r * 1.25, r * 2.5, r * 2.5);
        let held = self.held(hit);
        if bg.a > 0 {
            self.cv.circle(cx, cy, r, if held { bg.mix(rgb(0x808080), 0.35) } else { bg });
        } else if held {
            self.cv.circle(cx, cy, r, fg.alpha(0.15));
        }
        self.icon(ic, cx, cy, r * 1.05, fg);
        self.tap(hit)
    }

    /// Kleine Pille mit Text (Vorschläge, Filter). Liefert die Breite und ob sie angetippt wurde.
    pub fn pill(&mut self, x: f32, y: f32, label: &str, active: bool) -> (f32, bool) {
        // wie .pill in der Demo: Höhe 32, Schrift 13
        let h = self.dp(32.0);
        let w = self.text_w(label, 13.0, active) + self.dp(26.0);
        let r = Rect::new(x, y, w, h);
        let held = self.held(r);
        if active {
            self.cv.rrect(r, h / 2.0, self.th.accent);
        } else {
            self.cv.rrect(r, h / 2.0, self.th.line);
            self.cv.rrect(r.inset(1.0), h / 2.0, if held { self.th.press } else { self.th.card });
        }
        let c = if active { self.th.on_accent } else { self.th.text };
        self.label(r, label, 13.0, c, active, Al::Center);
        (w, self.tap(r))
    }

    /// Scrollbereich beginnen: Ziehen mit Gummiband am Ende, Schwung nach dem Loslassen, weiches Zurückfedern.
    /// Der Inhalt wird mit dem hier bereits begrenzten/gefederten Wert gezeichnet – kein Springen am Ende.
    pub fn scroll_begin(&mut self, area: Rect, scroll: &mut f32) {
        self.set_clip(Some(area));
        let k = scroll_key(area);
        let now = std::time::Instant::now();
        let inside = area.contains(self.p.px, self.p.py);
        let grab = self.enabled && self.p.down && self.p.moved && self.p.capture.is_none() && inside;
        let limit = area.h * 0.35;
        SP.with(|sp| {
            let mut sp = sp.borrow_mut();
            let max = sp.max.get(&k).copied().unwrap_or(f32::INFINITY);
            if self.p.rad != 0.0 && self.enabled && area.contains(self.p.x, self.p.y) {
                *scroll = (*scroll + self.p.rad).clamp(0.0, max.max(0.0));
                self.p.rad = 0.0;
                sp.anim = false;
                sp.vel = 0.0;
            }
            // Finger auf eine laufende Schwungbewegung: anhalten, aber nicht als Tipp werten
            if self.p.pressed && inside && sp.key == Some(k) && sp.anim && sp.vel.abs() > 150.0 {
                sp.anim = false;
                sp.vel = 0.0;
                self.p.moved = true;
            }
            if grab {
                if !sp.dragging || sp.key != Some(k) {
                    sp.key = Some(k);
                    sp.dragging = true;
                    sp.anim = false;
                    sp.vel = 0.0;
                    sp.last = now;
                }
                let mut d = -self.p.dy;
                self.p.dy = 0.0;
                if d == 0.0 {
                    // zweiter Zeichendurchlauf ohne neue Bewegung: Geschwindigkeit nicht verfälschen
                    return;
                }
                // Gummiband: über das Ende hinaus wird der Weg immer zäher
                let over = if *scroll < 0.0 { -*scroll } else if *scroll > max { *scroll - max } else { 0.0 };
                if (*scroll <= 0.0 && d < 0.0) || (*scroll >= max && d > 0.0) {
                    d *= 0.5 * (1.0 - (over / limit).min(0.95));
                }
                *scroll += d;
                let dt = now.duration_since(sp.last).as_secs_f32().max(0.004);
                let v = d / dt;
                sp.vel = if dt > 0.12 { v } else { sp.vel * 0.55 + v * 0.45 };
                sp.last = now;
            } else if sp.key == Some(k) && sp.dragging && !self.p.down {
                // losgelassen: Schwung übernehmen
                sp.dragging = false;
                sp.anim = true;
                if now.duration_since(sp.last).as_secs_f32() > 0.08 || sp.vel.abs() < 60.0 {
                    sp.vel = 0.0;
                }
                sp.vel = sp.vel.clamp(-9000.0, 9000.0);
                sp.last = now;
            }
            if sp.anim && !sp.dragging && sp.key == Some(k) {
                let dt = now.duration_since(sp.last).as_secs_f32().clamp(0.0, 0.05);
                sp.last = now;
                let target = scroll.clamp(0.0, if max.is_finite() { max } else { f32::MAX });
                if (*scroll - target).abs() > 0.01 {
                    // außerhalb: weich zum Rand zurückfedern, Schwung schnell abbauen
                    *scroll += (target - *scroll) * (1.0 - (-dt * 14.0).exp());
                    sp.vel *= (-dt * 25.0).exp();
                    if (*scroll - target).abs() < 0.5 {
                        *scroll = target;
                        sp.vel = 0.0;
                    }
                } else {
                    *scroll += sp.vel * dt;
                    sp.vel *= (-dt * 3.0).exp();
                    if sp.vel.abs() < 25.0 {
                        sp.vel = 0.0;
                    }
                    // in den Rand hineinlaufender Schwung: nur leicht überschießen
                    if max.is_finite() && (*scroll < 0.0 || *scroll > max) {
                        let o = if *scroll < 0.0 { *scroll } else { *scroll - max };
                        if o.abs() > limit * 0.25 {
                            *scroll = if *scroll < 0.0 { -limit * 0.25 } else { max + limit * 0.25 };
                            sp.vel = 0.0;
                        }
                    }
                }
                let inside_bounds = *scroll >= 0.0 && (!max.is_finite() || *scroll <= max);
                if sp.vel == 0.0 && inside_bounds {
                    sp.anim = false;
                }
            }
        });
    }

    pub fn scroll_end(&mut self, area: Rect, content_h: f32, scroll: &mut f32) {
        let max = (content_h - area.h).max(0.0);
        let k = scroll_key(area);
        let live = SP.with(|sp| {
            let mut sp = sp.borrow_mut();
            sp.max.insert(k, max);
            if sp.max.len() > 64 {
                sp.max.clear();
                sp.max.insert(k, max);
            }
            sp.key == Some(k) && (sp.dragging || sp.anim)
        });
        // Fokussiertes Feld dieses Bereichs über die Tastatur holen (einmal nach dem Fokuswechsel)
        if self.kb.ensure.get() && !live {
            if let Some((fr, Some(c))) = self.kb.focus.get() {
                if c == area {
                    let m = self.dp(14.0);
                    let extra = self.dp(76.0).min((area.h - fr.h - 2.0 * m).max(0.0));
                    let d = if fr.bottom() + extra + m > area.bottom() { (fr.bottom() + extra + m - area.bottom()).min((fr.y - m - area.y).max(0.0)) } else if fr.y - m < area.y { fr.y - m - area.y } else { 0.0 };
                    let ns = (*scroll + d).clamp(0.0, max);
                    if (ns - *scroll).abs() > 0.5 {
                        let fr2 = Rect::new(fr.x, fr.y - (ns - *scroll), fr.w, fr.h);
                        self.kb.focus.set(Some((fr2, Some(c))));
                        *scroll = ns;
                        self.dirty = true;
                    } else {
                        self.kb.ensure.set(false);
                    }
                }
            }
        }
        if live {
            // Gummiband erlaubt, aber begrenzt
            let limit = area.h * 0.35;
            *scroll = scroll.clamp(-limit, max + limit);
        } else {
            *scroll = scroll.clamp(0.0, max);
        }
        if max > 0.0 {
            // Rollbalken (schrumpft beim Überziehen)
            let over = if *scroll < 0.0 { -*scroll } else if *scroll > max { *scroll - max } else { 0.0 };
            let bh = ((area.h * area.h / content_h).max(self.dp(24.0)) - over * 0.5).max(self.dp(10.0));
            let pos = (*scroll / max).clamp(0.0, 1.0);
            let by = area.y + (area.h - bh) * pos;
            let x = if self.rtl { area.x + self.dp(3.0) } else { area.right() - self.dp(6.0) };
            self.cv.rrect(Rect::new(x, by, self.dp(3.0), bh), self.dp(1.5), self.th.sub.alpha(0.5));
        }
        self.set_clip(None);
    }

    pub fn icon(&mut self, ic: Ic, cx: f32, cy: f32, s: f32, c: Color) {
        self.icon_w(ic, cx, cy, s, c, 1.8);
    }

    /// Symbol wie in der Web-Demo: SVG-Linienpfad (24er Raster) mit Strichstärke `sw`, kantengeglättet.
    pub fn icon_w(&mut self, ic: Ic, cx: f32, cy: f32, s: f32, c: Color, sw: f32) {
        if let Some(name) = svg_name(ic) {
            if let Some(m) = svg_mask(name, s, sw) {
                self.cv.mask((cx - m.1 as f32 / 2.0).round() as i32, (cy - m.2 as f32 / 2.0).round() as i32, m.1, m.2, &m.0, c);
                return;
            }
        }
        self.icon_old(ic, cx, cy, s, c);
    }

    fn icon_old(&mut self, ic: Ic, cx: f32, cy: f32, s: f32, c: Color) {
        let t = (s * 0.11).max(1.5);
        let r = s / 2.0;
        let cv = &mut *self.cv;
        // Farbe für Aussparungen (Linse, Zeilen): Gegenton zum Symbol
        let cut = if (c.r as u32 * 3 + c.g as u32 * 6 + c.b as u32) / 10 < 90 { rgb(0xFFFFFF).alpha(0.8) } else { rgb(0x101216).alpha(0.85) };
        match ic {
            Ic::Svg(_) => {}
            Ic::Gear => {
                for i in 0..8 {
                    let a = i as f32 * std::f32::consts::PI / 4.0;
                    cv.line(cx + a.cos() * r * 0.62, cy + a.sin() * r * 0.62, cx + a.cos() * r * 0.98, cy + a.sin() * r * 0.98, t * 1.8, c);
                }
                cv.ring(cx, cy, r * 0.5, t * 1.6, c);
            }
            Ic::Box => {
                let (x0, x1, y0, y1) = (cx - r * 0.85, cx + r * 0.85, cy - r * 0.45, cy + r * 0.85);
                cv.line(x0, y0, x1, y0, t, c);
                cv.line(x0, y0, x0, y1, t, c);
                cv.line(x1, y0, x1, y1, t, c);
                cv.line(x0, y1, x1, y1, t, c);
                cv.line(x0, y0, cx - r * 0.45, cy - r * 0.9, t, c);
                cv.line(x1, y0, cx + r * 0.45, cy - r * 0.9, t, c);
                cv.line(cx - r * 0.45, cy - r * 0.9, cx + r * 0.45, cy - r * 0.9, t, c);
                cv.line(cx, y0, cx, cy + r * 0.1, t, c);
            }
            Ic::Term => {
                cv.line(cx - r * 0.8, cy - r * 0.5, cx - r * 0.2, cy, t * 1.2, c);
                cv.line(cx - r * 0.2, cy, cx - r * 0.8, cy + r * 0.5, t * 1.2, c);
                cv.line(cx + r * 0.05, cy + r * 0.55, cx + r * 0.8, cy + r * 0.55, t * 1.2, c);
            }
            Ic::List => {
                for i in -1..=1 {
                    let y = cy + i as f32 * r * 0.6;
                    cv.circle(cx - r * 0.75, y, t * 0.9, c);
                    cv.line(cx - r * 0.35, y, cx + r * 0.85, y, t, c);
                }
            }
            Ic::Power => {
                let rr = r * 0.78;
                cv.sdf(Rect::new(cx - r - 1.0, cy - r - 1.0, s + 2.0, s + 2.0), c, |x, y| {
                    let d = (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rr).abs() - t / 2.0;
                    // oben offen
                    if y < cy - rr * 0.45 && (x - cx).abs() < rr * 0.5 { 1.0 } else { d }
                });
                cv.line(cx, cy - r, cx, cy - r * 0.1, t, c);
            }
            Ic::Globe => {
                cv.ring(cx, cy, r * 0.85, t, c);
                cv.line(cx - r * 0.85, cy, cx + r * 0.85, cy, t, c);
                cv.sdf(Rect::new(cx - r, cy - r, s, s), c, |x, y| {
                    let e = (((x - cx) / 0.42).powi(2) + (y - cy).powi(2)).sqrt() - r * 0.85;
                    e.abs() * 0.6 - t / 2.0
                });
            }
            Ic::Sun => {
                cv.circle(cx, cy, r * 0.38, c);
                for i in 0..8 {
                    let a = i as f32 * std::f32::consts::PI / 4.0;
                    cv.line(cx + a.cos() * r * 0.62, cy + a.sin() * r * 0.62, cx + a.cos() * r * 0.95, cy + a.sin() * r * 0.95, t, c);
                }
            }
            Ic::Lang => {
                cv.line(cx - r * 0.7, cy + r * 0.8, cx, cy - r * 0.8, t * 1.2, c);
                cv.line(cx, cy - r * 0.8, cx + r * 0.7, cy + r * 0.8, t * 1.2, c);
                cv.line(cx - r * 0.4, cy + r * 0.2, cx + r * 0.4, cy + r * 0.2, t * 1.2, c);
            }
            Ic::Clock => {
                cv.ring(cx, cy, r * 0.85, t, c);
                cv.line(cx, cy, cx, cy - r * 0.5, t, c);
                cv.line(cx, cy, cx + r * 0.38, cy + r * 0.2, t, c);
            }
            Ic::Disk => {
                for i in -1..=1 {
                    let y = cy + i as f32 * r * 0.62;
                    cv.rrect(Rect::new(cx - r * 0.85, y - r * 0.22, r * 1.7, r * 0.44), r * 0.14, c);
                }
            }
            Ic::Info => {
                cv.ring(cx, cy, r * 0.85, t, c);
                cv.circle(cx, cy - r * 0.42, t * 0.8, c);
                cv.line(cx, cy - r * 0.08, cx, cy + r * 0.48, t * 1.2, c);
            }
            Ic::Back | Ic::Chevron => {
                let flip = (ic == Ic::Back) != self.rtl;
                let d = if flip { -1.0 } else { 1.0 };
                cv.line(cx - d * r * 0.3, cy - r * 0.7, cx + d * r * 0.4, cy, t * 1.3, c);
                cv.line(cx + d * r * 0.4, cy, cx - d * r * 0.3, cy + r * 0.7, t * 1.3, c);
            }
            Ic::Home => cv.ring(cx, cy, r * 0.7, t * 1.3, c),
            Ic::Bot => {
                cv.rrect(Rect::new(cx - r * 0.8, cy - r * 0.45, r * 1.6, r * 1.25), r * 0.3, c);
                cv.line(cx, cy - r * 0.45, cx, cy - r * 0.9, t, c);
                cv.circle(cx, cy - r * 0.92, t * 1.1, c);
                let eye = if c.r as u32 + c.g as u32 + c.b as u32 > 384 { rgb(0x20242C) } else { rgb(0xFFFFFF) };
                cv.circle(cx - r * 0.35, cy + r * 0.1, r * 0.16, eye);
                cv.circle(cx + r * 0.35, cy + r * 0.1, r * 0.16, eye);
            }
            Ic::Calc => {
                cv.line(cx - r * 0.75, cy - r * 0.45, cx - r * 0.15, cy - r * 0.45, t * 1.3, c);
                cv.line(cx - r * 0.45, cy - r * 0.75, cx - r * 0.45, cy - r * 0.15, t * 1.3, c);
                cv.line(cx + r * 0.2, cy - r * 0.45, cx + r * 0.75, cy - r * 0.45, t * 1.3, c);
                cv.line(cx - r * 0.68, cy + r * 0.22, cx - r * 0.22, cy + r * 0.68, t * 1.3, c);
                cv.line(cx - r * 0.22, cy + r * 0.22, cx - r * 0.68, cy + r * 0.68, t * 1.3, c);
                cv.line(cx + r * 0.2, cy + r * 0.32, cx + r * 0.75, cy + r * 0.32, t * 1.3, c);
                cv.line(cx + r * 0.2, cy + r * 0.6, cx + r * 0.75, cy + r * 0.6, t * 1.3, c);
            }
            Ic::Folder => {
                let (x0, x1, y0, y1) = (cx - r * 0.9, cx + r * 0.9, cy - r * 0.45, cy + r * 0.7);
                cv.line(x0, y0, x1, y0, t, c);
                cv.line(x0, y1, x1, y1, t, c);
                cv.line(x0, cy - r * 0.75, x0, y1, t, c);
                cv.line(x1, y0, x1, y1, t, c);
                cv.line(x0, cy - r * 0.75, cx - r * 0.2, cy - r * 0.75, t, c);
                cv.line(cx - r * 0.2, cy - r * 0.75, cx + r * 0.05, y0, t, c);
            }
            Ic::Image => {
                let (x0, x1, y0, y1) = (cx - r * 0.9, cx + r * 0.9, cy - r * 0.7, cy + r * 0.7);
                cv.line(x0, y0, x1, y0, t, c);
                cv.line(x0, y1, x1, y1, t, c);
                cv.line(x0, y0, x0, y1, t, c);
                cv.line(x1, y0, x1, y1, t, c);
                cv.circle(cx + r * 0.4, cy - r * 0.3, r * 0.16, c);
                cv.line(x0, y1 - r * 0.1, cx - r * 0.25, cy, t, c);
                cv.line(cx - r * 0.25, cy, cx + r * 0.2, cy + r * 0.45, t, c);
                cv.line(cx + r * 0.2, cy + r * 0.45, cx + r * 0.5, cy + r * 0.2, t, c);
                cv.line(cx + r * 0.5, cy + r * 0.2, x1, y1 - r * 0.2, t, c);
            }
            Ic::Shield => {
                cv.line(cx - r * 0.75, cy - r * 0.6, cx, cy - r * 0.9, t * 1.2, c);
                cv.line(cx, cy - r * 0.9, cx + r * 0.75, cy - r * 0.6, t * 1.2, c);
                cv.line(cx - r * 0.75, cy - r * 0.6, cx - r * 0.75, cy + r * 0.1, t * 1.2, c);
                cv.line(cx + r * 0.75, cy - r * 0.6, cx + r * 0.75, cy + r * 0.1, t * 1.2, c);
                cv.line(cx - r * 0.75, cy + r * 0.1, cx, cy + r * 0.92, t * 1.2, c);
                cv.line(cx + r * 0.75, cy + r * 0.1, cx, cy + r * 0.92, t * 1.2, c);
                cv.line(cx - r * 0.3, cy, cx - r * 0.05, cy + r * 0.28, t * 1.2, c);
                cv.line(cx - r * 0.05, cy + r * 0.28, cx + r * 0.35, cy - r * 0.25, t * 1.2, c);
            }
            Ic::Wifi => {
                // drei Bögen + Punkt (WLAN)
                for (k, rad) in [(0usize, r * 0.95), (1, r * 0.62), (2, r * 0.3)] {
                    let _ = k;
                    let seg = 8;
                    let mut prev: Option<(f32, f32)> = None;
                    for i in 0..=seg {
                        let a = (-135.0 + 90.0 * i as f32 / seg as f32).to_radians();
                        let px = cx + a.cos() * rad;
                        let py = cy + r * 0.55 + a.sin() * rad;
                        if let Some((qx, qy)) = prev {
                            cv.line(qx, qy, px, py, t * 1.2, c);
                        }
                        prev = Some((px, py));
                    }
                }
                cv.circle(cx, cy + r * 0.55, t * 1.3, c);
            }
            Ic::UpDown => {
                cv.line(cx - r * 0.4, cy + r * 0.8, cx - r * 0.4, cy - r * 0.8, t * 1.2, c);
                cv.line(cx - r * 0.4, cy - r * 0.8, cx - r * 0.75, cy - r * 0.42, t * 1.2, c);
                cv.line(cx - r * 0.4, cy - r * 0.8, cx - r * 0.05, cy - r * 0.42, t * 1.2, c);
                cv.line(cx + r * 0.4, cy - r * 0.8, cx + r * 0.4, cy + r * 0.8, t * 1.2, c);
                cv.line(cx + r * 0.4, cy + r * 0.8, cx + r * 0.05, cy + r * 0.42, t * 1.2, c);
                cv.line(cx + r * 0.4, cy + r * 0.8, cx + r * 0.75, cy + r * 0.42, t * 1.2, c);
            }
            Ic::Bt => {
                let (x0, x1) = (cx - r * 0.45, cx + r * 0.45);
                cv.line(cx, cy - r * 0.9, cx, cy + r * 0.9, t * 1.2, c);
                cv.line(cx, cy - r * 0.9, x1, cy - r * 0.45, t * 1.2, c);
                cv.line(x1, cy - r * 0.45, x0, cy + r * 0.45, t * 1.2, c);
                cv.line(cx, cy + r * 0.9, x1, cy + r * 0.45, t * 1.2, c);
                cv.line(x1, cy + r * 0.45, x0, cy - r * 0.45, t * 1.2, c);
            }
            Ic::Plane => {
                cv.line(cx, cy - r * 0.9, cx, cy + r * 0.75, t * 1.4, c);
                cv.line(cx - r * 0.9, cy + r * 0.15, cx, cy - r * 0.25, t * 1.2, c);
                cv.line(cx + r * 0.9, cy + r * 0.15, cx, cy - r * 0.25, t * 1.2, c);
                cv.line(cx - r * 0.4, cy + r * 0.9, cx, cy + r * 0.65, t * 1.2, c);
                cv.line(cx + r * 0.4, cy + r * 0.9, cx, cy + r * 0.65, t * 1.2, c);
            }
            Ic::Torch => {
                cv.line(cx - r * 0.45, cy - r * 0.85, cx + r * 0.45, cy - r * 0.85, t * 1.2, c);
                cv.line(cx - r * 0.45, cy - r * 0.85, cx - r * 0.45, cy - r * 0.45, t * 1.2, c);
                cv.line(cx + r * 0.45, cy - r * 0.85, cx + r * 0.45, cy - r * 0.45, t * 1.2, c);
                cv.line(cx - r * 0.45, cy - r * 0.45, cx - r * 0.2, cy, t * 1.2, c);
                cv.line(cx + r * 0.45, cy - r * 0.45, cx + r * 0.2, cy, t * 1.2, c);
                cv.line(cx - r * 0.2, cy, cx - r * 0.2, cy + r * 0.9, t * 1.2, c);
                cv.line(cx + r * 0.2, cy, cx + r * 0.2, cy + r * 0.9, t * 1.2, c);
                cv.line(cx - r * 0.2, cy + r * 0.9, cx + r * 0.2, cy + r * 0.9, t * 1.2, c);
            }
            Ic::Speaker => {
                cv.line(cx - r * 0.85, cy - r * 0.3, cx - r * 0.5, cy - r * 0.3, t * 1.2, c);
                cv.line(cx - r * 0.85, cy + r * 0.3, cx - r * 0.5, cy + r * 0.3, t * 1.2, c);
                cv.line(cx - r * 0.85, cy - r * 0.3, cx - r * 0.85, cy + r * 0.3, t * 1.2, c);
                cv.line(cx - r * 0.5, cy - r * 0.3, cx - r * 0.05, cy - r * 0.75, t * 1.2, c);
                cv.line(cx - r * 0.5, cy + r * 0.3, cx - r * 0.05, cy + r * 0.75, t * 1.2, c);
                cv.line(cx - r * 0.05, cy - r * 0.75, cx - r * 0.05, cy + r * 0.75, t * 1.2, c);
                cv.line(cx + r * 0.3, cy - r * 0.3, cx + r * 0.42, cy, t * 1.1, c);
                cv.line(cx + r * 0.42, cy, cx + r * 0.3, cy + r * 0.3, t * 1.1, c);
                cv.line(cx + r * 0.6, cy - r * 0.6, cx + r * 0.85, cy, t * 1.1, c);
                cv.line(cx + r * 0.85, cy, cx + r * 0.6, cy + r * 0.6, t * 1.1, c);
            }
            Ic::Lock => {
                cv.rrect(Rect::new(cx - r * 0.7, cy - r * 0.1, r * 1.4, r * 1.0), r * 0.18, c);
                cv.line(cx - r * 0.42, cy - r * 0.1, cx - r * 0.42, cy - r * 0.45, t * 1.2, c);
                cv.line(cx + r * 0.42, cy - r * 0.1, cx + r * 0.42, cy - r * 0.45, t * 1.2, c);
                cv.line(cx - r * 0.42, cy - r * 0.45, cx - r * 0.2, cy - r * 0.8, t * 1.2, c);
                cv.line(cx + r * 0.42, cy - r * 0.45, cx + r * 0.2, cy - r * 0.8, t * 1.2, c);
                cv.line(cx - r * 0.2, cy - r * 0.8, cx + r * 0.2, cy - r * 0.8, t * 1.2, c);
            }
            Ic::Phone => {
                // Hörer: zwei Muscheln, verbunden durch einen Bogen
                let seg = 8;
                let mut prev: Option<(f32, f32)> = None;
                for i in 0..=seg {
                    let a = (200.0 - 130.0 * i as f32 / seg as f32).to_radians();
                    let px = cx + r * 0.15 + a.cos() * r * 0.75;
                    let py = cy + r * 0.15 - a.sin() * r * 0.75;
                    if let Some((qx, qy)) = prev {
                        cv.line(qx, qy, px, py, t * 2.4, c);
                    }
                    prev = Some((px, py));
                }
                cv.circle(cx - r * 0.55, cy + r * 0.45, r * 0.32, c);
                cv.circle(cx + r * 0.45, cy - r * 0.55, r * 0.32, c);
            }
            Ic::Chat => {
                let (x0, x1, y0, y1) = (cx - r * 0.85, cx + r * 0.85, cy - r * 0.7, cy + r * 0.45);
                cv.line(x0, y0, x1, y0, t * 1.2, c);
                cv.line(x0, y1, x1, y1, t * 1.2, c);
                cv.line(x0, y0, x0, y1, t * 1.2, c);
                cv.line(x1, y0, x1, y1, t * 1.2, c);
                cv.line(cx - r * 0.45, cy - r * 0.25, cx + r * 0.45, cy - r * 0.25, t, c);
                cv.line(cx - r * 0.45, cy + r * 0.05, cx + r * 0.2, cy + r * 0.05, t, c);
                cv.line(cx - r * 0.45, cy + r * 0.4, cx - r * 0.6, cy + r * 0.9, t * 1.4, c);
                cv.line(cx - r * 0.6, cy + r * 0.9, cx - r * 0.05, cy + r * 0.45, t * 1.4, c);
            }
            Ic::Camera => {
                cv.line(cx - r * 0.9, cy - r * 0.45, cx + r * 0.9, cy - r * 0.45, t * 1.2, c);
                cv.line(cx - r * 0.9, cy + r * 0.75, cx + r * 0.9, cy + r * 0.75, t * 1.2, c);
                cv.line(cx - r * 0.9, cy - r * 0.45, cx - r * 0.9, cy + r * 0.75, t * 1.2, c);
                cv.line(cx + r * 0.9, cy - r * 0.45, cx + r * 0.9, cy + r * 0.75, t * 1.2, c);
                cv.line(cx - r * 0.35, cy - r * 0.45, cx - r * 0.2, cy - r * 0.75, t * 1.2, c);
                cv.line(cx + r * 0.35, cy - r * 0.45, cx + r * 0.2, cy - r * 0.75, t * 1.2, c);
                cv.line(cx - r * 0.2, cy - r * 0.75, cx + r * 0.2, cy - r * 0.75, t * 1.2, c);
                cv.ring(cx, cy + r * 0.15, r * 0.36, t * 1.2, c);
            }
            Ic::Grid => {
                for i in 0..3 {
                    for j in 0..3 {
                        cv.circle(cx + (i as f32 - 1.0) * r * 0.62, cy + (j as f32 - 1.0) * r * 0.62, t * 1.4, c);
                    }
                }
            }
            Ic::Sliders => {
                for (i, k) in [(-0.6f32, 0.3f32), (0.0, -0.4), (0.6, 0.15)] {
                    cv.line(cx + i * r, cy - r * 0.85, cx + i * r, cy + r * 0.85, t * 1.1, c);
                    cv.line(cx + i * r - r * 0.22, cy + k * r, cx + i * r + r * 0.22, cy + k * r, t * 1.6, c);
                }
            }
            Ic::Bell => {
                // Glocke: Kuppel, Rand, Klöppel
                let seg = 8;
                let mut prev: Option<(f32, f32)> = None;
                for i in 0..=seg {
                    let a = (180.0 * i as f32 / seg as f32).to_radians();
                    let (px, py) = (cx - a.cos() * r * 0.62, cy - r * 0.1 - a.sin() * r * 0.75);
                    if let Some((qx, qy)) = prev {
                        cv.line(qx, qy, px, py, t * 1.2, c);
                    }
                    prev = Some((px, py));
                }
                cv.line(cx - r * 0.62, cy - r * 0.1, cx - r * 0.62, cy + r * 0.4, t * 1.2, c);
                cv.line(cx + r * 0.62, cy - r * 0.1, cx + r * 0.62, cy + r * 0.4, t * 1.2, c);
                cv.line(cx - r * 0.9, cy + r * 0.45, cx + r * 0.9, cy + r * 0.45, t * 1.3, c);
                cv.circle(cx, cy + r * 0.8, t * 1.4, c);
            }
            Ic::Moon => {
                // Sichel aus zwei Bögen
                let seg = 10;
                for (rad, off, from, to) in [(r * 0.9, 0.0, 40.0f32, 320.0f32), (r * 0.72, r * 0.42, 75.0, 285.0)] {
                    let mut prev: Option<(f32, f32)> = None;
                    for i in 0..=seg {
                        let a = (from + (to - from) * i as f32 / seg as f32).to_radians();
                        let (px, py) = (cx + off + a.cos() * rad, cy - a.sin() * rad);
                        if let Some((qx, qy)) = prev {
                            cv.line(qx, qy, px, py, t * 1.2, c);
                        }
                        prev = Some((px, py));
                    }
                }
            }
            Ic::Tower => {
                // Mast mit zwei Beinen
                cv.line(cx, cy - r * 0.15, cx - r * 0.5, cy + r * 0.92, t * 1.2, c);
                cv.line(cx, cy - r * 0.15, cx + r * 0.5, cy + r * 0.92, t * 1.2, c);
                cv.line(cx - r * 0.26, cy + r * 0.42, cx + r * 0.26, cy + r * 0.42, t, c);
                cv.circle(cx, cy - r * 0.22, t * 1.1, c);
                // Funkwellen als Bögen links und rechts
                for (k, rad) in [(0usize, r * 0.42), (1, r * 0.8)] {
                    let _ = k;
                    let seg = 6;
                    for side in [-1.0f32, 1.0] {
                        let mut prev: Option<(f32, f32)> = None;
                        for i in 0..=seg {
                            let a = (-50.0 + 100.0 * i as f32 / seg as f32).to_radians();
                            let px = cx + side * a.cos() * rad;
                            let py = cy - r * 0.22 + a.sin() * rad;
                            if let Some((qx, qy)) = prev {
                                cv.line(qx, qy, px, py, t, c);
                            }
                            prev = Some((px, py));
                        }
                    }
                }
            }
            Ic::Chip => {
                cv.rrect(Rect::new(cx - r * 0.55, cy - r * 0.55, r * 1.1, r * 1.1), r * 0.15, c);
                for i in -1..=1 {
                    let o = i as f32 * r * 0.36;
                    cv.line(cx + o, cy - r * 0.95, cx + o, cy - r * 0.55, t, c);
                    cv.line(cx + o, cy + r * 0.55, cx + o, cy + r * 0.95, t, c);
                    cv.line(cx - r * 0.95, cy + o, cx - r * 0.55, cy + o, t, c);
                    cv.line(cx + r * 0.55, cy + o, cx + r * 0.95, cy + o, t, c);
                }
            }
            Ic::Play => {
                let (x0, x1) = (cx - r * 0.45, cx + r * 0.75);
                cv.sdf(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), c, |x, y| {
                    // Dreieck als Schnitt dreier Halbebenen
                    let k = (x - x0) / (x1 - x0);
                    let half = r * 0.8 * (1.0 - k);
                    (x0 - x).max((y - cy).abs() - half)
                });
            }
            Ic::Pause => {
                cv.rrect(Rect::new(cx - r * 0.6, cy - r * 0.75, r * 0.42, r * 1.5), r * 0.12, c);
                cv.rrect(Rect::new(cx + r * 0.18, cy - r * 0.75, r * 0.42, r * 1.5), r * 0.12, c);
            }
            Ic::Note => {
                let b = Rect::new(cx - r * 0.72, cy - r * 0.92, r * 1.44, r * 1.84);
                cv.rrect(b, r * 0.2, c);
                for i in 0..3 {
                    let y = cy - r * 0.42 + i as f32 * r * 0.42;
                    let w = if i == 2 { r * 0.5 } else { r * 0.92 };
                    cv.line(cx - r * 0.42, y, cx - r * 0.42 + w, y, t, cut);
                }
            }
            Ic::Person => {
                cv.circle(cx, cy - r * 0.42, r * 0.42, c);
                cv.sdf(Rect::new(cx - r, cy, 2.0 * r, r), c, |x, y| {
                    let d = ((x - cx).powi(2) / 0.72 + (y - (cy + r * 0.98)).powi(2)).sqrt() - r * 0.86;
                    d.max(y - (cy + r * 0.92))
                });
            }
            Ic::Send => {
                cv.line(cx - r * 0.8, cy - r * 0.7, cx + r * 0.85, cy, t * 1.5, c);
                cv.line(cx - r * 0.8, cy + r * 0.7, cx + r * 0.85, cy, t * 1.5, c);
                cv.line(cx - r * 0.8, cy - r * 0.7, cx - r * 0.45, cy, t * 1.5, c);
                cv.line(cx - r * 0.8, cy + r * 0.7, cx - r * 0.45, cy, t * 1.5, c);
                cv.line(cx - r * 0.45, cy, cx + r * 0.2, cy, t * 1.5, c);
            }
            Ic::Flip => {
                cv.ring(cx, cy, r * 0.72, t * 1.2, c);
                cv.rect(Rect::new(cx - r * 0.2, cy - r, r * 0.4, r * 0.5), rgb(0).alpha(0.0));
                cv.line(cx + r * 0.72, cy - r * 0.05, cx + r * 0.42, cy - r * 0.3, t * 1.2, c);
                cv.line(cx + r * 0.72, cy - r * 0.05, cx + r * 0.98, cy - r * 0.32, t * 1.2, c);
                cv.line(cx - r * 0.72, cy + r * 0.05, cx - r * 0.42, cy + r * 0.3, t * 1.2, c);
                cv.line(cx - r * 0.72, cy + r * 0.05, cx - r * 0.98, cy + r * 0.32, t * 1.2, c);
            }
            Ic::Flash => {
                cv.line(cx + r * 0.25, cy - r * 0.95, cx - r * 0.4, cy + r * 0.08, t * 1.7, c);
                cv.line(cx - r * 0.4, cy + r * 0.08, cx + r * 0.4, cy - r * 0.08, t * 1.7, c);
                cv.line(cx + r * 0.4, cy - r * 0.08, cx - r * 0.25, cy + r * 0.95, t * 1.7, c);
            }
            Ic::Thirds => {
                for i in [-1.0f32, 1.0] {
                    cv.line(cx + i * r * 0.3, cy - r * 0.85, cx + i * r * 0.3, cy + r * 0.85, t, c);
                    cv.line(cx - r * 0.85, cy + i * r * 0.3, cx + r * 0.85, cy + i * r * 0.3, t, c);
                }
            }
            Ic::Timer => {
                cv.ring(cx, cy + r * 0.1, r * 0.72, t * 1.2, c);
                cv.line(cx, cy + r * 0.1, cx, cy - r * 0.32, t * 1.2, c);
                cv.line(cx - r * 0.22, cy - r * 0.95, cx + r * 0.22, cy - r * 0.95, t * 1.4, c);
            }
            Ic::Heart => {
                cv.sdf(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), c, |x, y| {
                    // Herz: auf die Spitze gestelltes Quadrat + zwei Kreise auf den oberen Kanten
                    let (px, py) = (((x - cx) / r).abs(), (y - cy) / r);
                    let dia = (px + (py - 0.1).abs() - 0.62) / std::f32::consts::SQRT_2;
                    let cir = ((px - 0.31).powi(2) + (py + 0.21).powi(2)).sqrt() - 0.438;
                    dia.min(cir) * r
                });
            }
            Ic::Trash => {
                cv.line(cx - r * 0.8, cy - r * 0.55, cx + r * 0.8, cy - r * 0.55, t * 1.3, c);
                cv.line(cx - r * 0.25, cy - r * 0.85, cx + r * 0.25, cy - r * 0.85, t * 1.3, c);
                cv.line(cx - r * 0.55, cy - r * 0.55, cx - r * 0.42, cy + r * 0.85, t * 1.2, c);
                cv.line(cx + r * 0.55, cy - r * 0.55, cx + r * 0.42, cy + r * 0.85, t * 1.2, c);
                cv.line(cx - r * 0.42, cy + r * 0.85, cx + r * 0.42, cy + r * 0.85, t * 1.2, c);
                cv.line(cx, cy - r * 0.2, cx, cy + r * 0.5, t, c);
            }
            Ic::Rotate => {
                cv.ring(cx, cy, r * 0.7, t * 1.2, c);
                cv.line(cx + r * 0.7, cy, cx + r * 0.38, cy - r * 0.28, t * 1.2, c);
                cv.line(cx + r * 0.7, cy, cx + r * 0.98, cy - r * 0.3, t * 1.2, c);
            }
            Ic::Plus => {
                cv.line(cx - r * 0.7, cy, cx + r * 0.7, cy, t * 1.6, c);
                cv.line(cx, cy - r * 0.7, cx, cy + r * 0.7, t * 1.6, c);
            }
            Ic::Close => {
                cv.line(cx - r * 0.6, cy - r * 0.6, cx + r * 0.6, cy + r * 0.6, t * 1.4, c);
                cv.line(cx + r * 0.6, cy - r * 0.6, cx - r * 0.6, cy + r * 0.6, t * 1.4, c);
            }
            Ic::Spark => {
                // KI-Funke: vierzackiger Stern mit eingezogenen Flanken + kleiner Begleiter
                let star = |cv: &mut Canvas, sx: f32, sy: f32, sr: f32| {
                    cv.sdf(Rect::new(sx - sr, sy - sr, 2.0 * sr, 2.0 * sr), c, |x, y| {
                        let (dx, dy) = ((x - sx).abs() / sr, (y - sy).abs() / sr);
                        ((dx.powf(0.58) + dy.powf(0.58)).powf(1.0 / 0.58) - 1.0) * sr * 0.6
                    });
                };
                star(cv, cx - r * 0.12, cy + r * 0.1, r * 0.88);
                star(cv, cx + r * 0.66, cy - r * 0.66, r * 0.34);
            }
            Ic::Star => {
                cv.sdf(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), c, |x, y| {
                    let (dx, dy) = (x - cx, y - cy);
                    let seg = std::f32::consts::TAU / 5.0;
                    let a = dy.atan2(dx) + std::f32::consts::FRAC_PI_2 + seg / 2.0;
                    let a = ((a % seg) + seg) % seg - seg / 2.0;
                    let d = (dx * dx + dy * dy).sqrt();
                    d - r * (0.45 + 0.55 * (1.0 - a.abs() / (seg / 2.0)).powf(1.5))
                });
            }
            Ic::Music => {
                cv.circle(cx - r * 0.42, cy + r * 0.55, r * 0.3, c);
                cv.circle(cx + r * 0.5, cy + r * 0.35, r * 0.3, c);
                cv.line(cx - r * 0.16, cy + r * 0.55, cx - r * 0.16, cy - r * 0.7, t * 1.2, c);
                cv.line(cx + r * 0.76, cy + r * 0.35, cx + r * 0.76, cy - r * 0.9, t * 1.2, c);
                cv.line(cx - r * 0.16, cy - r * 0.7, cx + r * 0.76, cy - r * 0.9, t * 1.8, c);
            }
            Ic::Video => {
                cv.rrect(Rect::new(cx - r * 0.95, cy - r * 0.6, r * 1.35, r * 1.2), r * 0.22, c);
                cv.line(cx + r * 0.5, cy, cx + r * 0.88, cy - r * 0.42, t * 1.6, c);
                cv.line(cx + r * 0.5, cy, cx + r * 0.88, cy + r * 0.42, t * 1.6, c);
                cv.line(cx + r * 0.88, cy - r * 0.42, cx + r * 0.88, cy + r * 0.42, t * 1.6, c);
            }
            Ic::Doc => {
                let a = Rect::new(cx - r * 0.65, cy - r * 0.85, r * 1.3, r * 1.7);
                cv.rrect(a, r * 0.18, c);
                cv.rrect(a.inset(t), (r * 0.18 - t).max(0.0), cut);
                for k in 0..3 {
                    let yy = cy - r * 0.35 + k as f32 * r * 0.35;
                    cv.line(cx - r * 0.35, yy, cx + r * 0.35, yy, t, c);
                }
            }
            Ic::Download => {
                cv.line(cx, cy - r * 0.85, cx, cy + r * 0.35, t * 1.2, c);
                cv.line(cx - r * 0.45, cy - r * 0.05, cx, cy + r * 0.4, t * 1.2, c);
                cv.line(cx + r * 0.45, cy - r * 0.05, cx, cy + r * 0.4, t * 1.2, c);
                cv.line(cx - r * 0.8, cy + r * 0.85, cx + r * 0.8, cy + r * 0.85, t * 1.2, c);
            }
            Ic::Cut => {
                cv.ring(cx - r * 0.45, cy + r * 0.5, r * 0.28, t, c);
                cv.ring(cx + r * 0.45, cy + r * 0.5, r * 0.28, t, c);
                cv.line(cx - r * 0.3, cy + r * 0.28, cx + r * 0.45, cy - r * 0.85, t, c);
                cv.line(cx + r * 0.3, cy + r * 0.28, cx - r * 0.45, cy - r * 0.85, t, c);
            }
            Ic::Finger => {
                // Fingerabdruck: konzentrische Bögen, unten offen
                for k in 0..4 {
                    let rr = r * (0.22 + k as f32 * 0.24);
                    let gap = 0.16 + k as f32 * 0.03;
                    let bb = Rect::new(cx - rr - t, cy - rr - t, 2.0 * (rr + t), 2.0 * (rr + t));
                    cv.sdf(bb, c, |x, y| {
                        let (dx, dy) = (x - cx, y - cy);
                        let a = (dx.atan2(dy) / std::f32::consts::TAU).abs();
                        if a < gap {
                            return 9.0;
                        }
                        ((dx * dx + dy * dy).sqrt() - rr).abs() - t * 0.5
                    });
                }
            }
            Ic::Pin => {
                cv.circle(cx, cy - r * 0.35, r * 0.55, c);
                cv.line(cx, cy + r * 0.1, cx - r * 0.25, cy + r * 1.0, t * 1.4, rgb(0xB0B0B0));
            }
            Ic::Usb => {
                let a = Rect::new(cx - r * 0.45, cy - r * 0.2, r * 0.9, r * 1.1);
                cv.rrect(a, r * 0.15, c);
                cv.rrect(a.inset(t), (r * 0.15 - t).max(0.0), cut);
                let b = Rect::new(cx - r * 0.3, cy - r * 0.85, r * 0.6, r * 0.65);
                cv.rrect(b, r * 0.08, c);
                cv.rrect(b.inset(t), 0.0, cut);
            }
            Ic::Copy => {
                let a = Rect::new(cx - r * 0.75, cy - r * 0.45, r * 1.1, r * 1.25);
                let b = Rect::new(cx - r * 0.3, cy - r * 0.85, r * 1.1, r * 1.25);
                for q in [b, a] {
                    cv.rrect(q, r * 0.2, c);
                    cv.rrect(q.inset(t), (r * 0.2 - t).max(0.0), cut);
                }
            }
            Ic::Pen => {
                cv.line(cx - r * 0.55, cy + r * 0.55, cx + r * 0.5, cy - r * 0.5, t * 2.2, c);
                cv.line(cx + r * 0.45, cy - r * 0.75, cx + r * 0.75, cy - r * 0.45, t * 2.2, c);
                cv.line(cx - r * 0.8, cy + r * 0.8, cx - r * 0.6, cy + r * 0.6, t * 1.2, c);
                cv.line(cx - r * 0.2, cy + r * 0.9, cx + r * 0.85, cy + r * 0.9, t, c);
            }
            Ic::Mic => {
                cv.rrect(Rect::new(cx - r * 0.3, cy - r * 0.85, r * 0.6, r * 1.15), r * 0.3, c);
                let (a, b) = (r * 0.58, r * 0.15);
                cv.line(cx - a, cy - b, cx - a * 0.8, cy + r * 0.25, t, c);
                cv.line(cx + a, cy - b, cx + a * 0.8, cy + r * 0.25, t, c);
                cv.line(cx - a * 0.8, cy + r * 0.25, cx, cy + r * 0.55, t, c);
                cv.line(cx + a * 0.8, cy + r * 0.25, cx, cy + r * 0.55, t, c);
                cv.line(cx, cy + r * 0.55, cx, cy + r * 0.9, t, c);
                cv.line(cx - r * 0.35, cy + r * 0.9, cx + r * 0.35, cy + r * 0.9, t, c);
            }
            Ic::Search => {
                cv.ring(cx - r * 0.15, cy - r * 0.15, r * 0.55, t * 1.3, c);
                cv.line(cx + r * 0.28, cy + r * 0.28, cx + r * 0.8, cy + r * 0.8, t * 1.5, c);
            }
            Ic::Wall => {
                cv.rrect(Rect::new(cx - r * 0.9, cy - r * 0.7, r * 1.8, r * 1.4), r * 0.2, c);
                cv.circle(cx + r * 0.4, cy - r * 0.25, r * 0.18, cut);
                cv.line(cx - r * 0.6, cy + r * 0.4, cx - r * 0.1, cy - r * 0.1, t, cut);
                cv.line(cx - r * 0.1, cy - r * 0.1, cx + r * 0.5, cy + r * 0.4, t, cut);
            }
            Ic::Bksp => {
                let pts = [(-0.95f32, 0.0f32), (-0.35, -0.62), (0.9, -0.62), (0.9, 0.62), (-0.35, 0.62), (-0.95, 0.0)];
                for w in pts.windows(2) {
                    cv.line(cx + w[0].0 * r, cy + w[0].1 * r, cx + w[1].0 * r, cy + w[1].1 * r, t * 1.1, c);
                }
                cv.line(cx - r * 0.02, cy - r * 0.26, cx + r * 0.5, cy + r * 0.26, t * 1.1, c);
                cv.line(cx + r * 0.5, cy - r * 0.26, cx - r * 0.02, cy + r * 0.26, t * 1.1, c);
            }
            Ic::Apps => {
                for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    cv.rrect(Rect::new(cx + dx * r * 0.45 - r * 0.32, cy + dy * r * 0.45 - r * 0.32, r * 0.64, r * 0.64), r * 0.16, c);
                }
            }
            Ic::Check => {
                cv.line(cx - r * 0.7, cy, cx - r * 0.15, cy + r * 0.55, t * 1.4, c);
                cv.line(cx - r * 0.15, cy + r * 0.55, cx + r * 0.75, cy - r * 0.55, t * 1.4, c);
            }
        }
    }
}

#[cfg(test)]
mod scale_tests {
    use super::scale_for;

    #[test]
    fn every_screen_gets_enough_room() {
        // (Breite, Höhe) -> mindestens 360 x 600 dp nutzbar, auf Tablets mehr
        for (w, h) in [(480.0, 800.0), (720.0, 1280.0), (1080.0, 2340.0), (1280.0, 800.0), (800.0, 1280.0), (1920.0, 1080.0), (240.0, 320.0), (2560.0, 1600.0)] {
            let k = scale_for(w, h, 1.0);
            let (dw, dh): (f32, f32) = (w / k, h / k);
            assert!(dw.min(dh) >= 359.0 && dw.max(dh) >= 599.0, "{w}x{h}: {dw}x{dh} dp");
        }
        assert!((scale_for(720.0, 1280.0, 1.0) - 2.0).abs() < 0.01); // Handy unverändert
        assert!(scale_for(1280.0, 800.0, 1.0) < 1.4); // Tablet: mehr Inhalt statt größerer Elemente
    }
}

// ---------------------------------------------------------------- Scroll-Physik

type ScrollKey = (i32, i32, i32, i32);

fn scroll_key(r: Rect) -> ScrollKey {
    (r.x as i32, r.y as i32, r.w as i32, r.h as i32)
}

struct ScrollPhys {
    key: Option<ScrollKey>,
    /// zuletzt bekannte Maximalposition je Bereich
    max: std::collections::HashMap<ScrollKey, f32>,
    /// Geschwindigkeit in px/s (positiv = Inhalt nach oben)
    vel: f32,
    last: std::time::Instant,
    dragging: bool,
    anim: bool,
}

thread_local! {
    static SP: std::cell::RefCell<ScrollPhys> = std::cell::RefCell::new(ScrollPhys {
        key: None,
        max: std::collections::HashMap::new(),
        vel: 0.0,
        last: std::time::Instant::now(),
        dragging: false,
        anim: false,
    });
}

/// Läuft gerade Schwung/Zurückfedern? Dann braucht die Oberfläche flüssige Bilder (~60/s).
/// Wird der Bereich nicht mehr gezeichnet (anderer Bildschirm), endet die Animation von selbst.
pub fn scroll_animating() -> bool {
    SP.with(|sp| {
        let sp = sp.borrow();
        sp.anim && !sp.dragging && sp.last.elapsed() < std::time::Duration::from_millis(250)
    })
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    use crate::gfx::Canvas;
    use crate::text::Text;

    fn with_ui<F: FnMut(&mut Ui)>(cv: &mut Canvas, tx: &mut Text, p: &mut Ptr, mut f: F) {
        let mut ui = Ui { th: theme(true, 0), k: 1.0, w: 360.0, h: 640.0, cv, tx, p, rtl: false, enabled: true, focus: None, dirty: false, clip: None, links: Vec::new(), kb: Default::default(), seen: None };
        f(&mut ui);
    }

    /// Über das Ende ziehen: Position wächst nur gedämpft, springt nie zurück, solange der Finger liegt;
    /// nach dem Loslassen federt sie weich (ohne Sprung) auf das Ende zurück.
    #[test]
    fn rubber_band_no_jitter() {
        let mut cv = Canvas::new(360, 640);
        let mut tx = Text::new();
        let area = Rect::new(0.0, 0.0, 360.0, 500.0);
        let content = 800.0; // max = 300
        let mut sc = 290.0f32;
        let mut p = Ptr { x: 100.0, y: 400.0, px: 100.0, py: 400.0, down: true, pressed: true, moved: true, ..Default::default() };
        let mut prev = sc;
        for _ in 0..40 {
            p.dy = -12.0; // Finger nach oben = Inhalt nach oben (scroll wächst)
            with_ui(&mut cv, &mut tx, &mut p, |ui| {
                ui.scroll_begin(area, &mut sc);
                ui.scroll_end(area, content, &mut sc);
            });
            p.pressed = false;
            assert!(sc >= prev - 0.001, "kein Zurückspringen beim Ziehen: {prev} -> {sc}");
            prev = sc;
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        assert!(sc > 300.0 && sc < 300.0 + 500.0 * 0.35 + 0.1, "gedämpftes Überziehen: {sc}");
        // loslassen -> zurückfedern
        p.down = false;
        p.released = true;
        let mut last = sc;
        for _ in 0..80 {
            with_ui(&mut cv, &mut tx, &mut p, |ui| {
                ui.scroll_begin(area, &mut sc);
                ui.scroll_end(area, content, &mut sc);
            });
            p.released = false;
            assert!(sc <= last + 0.001, "Zurückfedern ohne Zittern: {last} -> {sc}");
            last = sc;
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        assert!((sc - 300.0).abs() < 0.6, "endet am Rand: {sc}");
        assert!(!scroll_animating());
    }
}

/// Zuordnung der eigenen Symbole zu den Demo-Symbolen.
fn svg_name(ic: Ic) -> Option<&'static str> {
    Some(match ic {
        Ic::Svg(n) => n,
        Ic::Gear => "gear",
        Ic::Box => "box",
        Ic::Term => "term",
        Ic::List => "list",
        Ic::Power => "power",
        Ic::Globe => "globe",
        Ic::Sun => "sun",
        Ic::Lang => "lang",
        Ic::Clock => "clock",
        Ic::Disk => "drive",
        Ic::Info => "info",
        Ic::Back => "back",
        Ic::Home => "home",
        Ic::Chevron => "chev",
        Ic::Check => "check",
        Ic::Bot => "sparkle",
        Ic::Chip => "chip",
        Ic::Calc => "calc",
        Ic::Folder => "folder",
        Ic::Image => "img",
        Ic::Shield => "shield",
        Ic::Tower => "cell",
        Ic::Wifi => "wifi",
        Ic::UpDown => "data",
        Ic::Bt => "bt",
        Ic::Plane => "plane",
        Ic::Torch => "torch",
        Ic::Speaker => "vol",
        Ic::Lock => "lock",
        Ic::Grid => "grid",
        Ic::Sliders => "sliders",
        Ic::Phone => "phone",
        Ic::Chat => "msg",
        Ic::Camera => "cam",
        Ic::Play => "play",
        Ic::Pause => "pause",
        Ic::Note => "note",
        Ic::Person => "user",
        Ic::Send => "send",
        Ic::Flip => "flip",
        Ic::Flash => "bolt",
        Ic::Thirds => "grid",
        Ic::Timer => "timer",
        Ic::Heart => "heart",
        Ic::Trash => "trash",
        Ic::Rotate => "rotate",
        Ic::Plus => "plus",
        Ic::Close => "close",
        Ic::Spark => "sparkle",
        Ic::Music => "music",
        Ic::Video => "video",
        Ic::Star => "star",
        Ic::Search => "search",
        Ic::Wall => "palette",
        Ic::Apps => "apps",
        Ic::Bksp => "bksp",
        Ic::Bell => "bell",
        Ic::Moon => "moon",
        Ic::Mic => "mic",
        Ic::Pen => "edit",
        Ic::Copy => "copy",
        Ic::Doc => "file",
        Ic::Download => "dl",
        Ic::Usb => "usb",
        Ic::Pin => "pin",
        Ic::Finger => "fp",
        Ic::Cut => "cut",
    })
}

type SvgMask = std::rc::Rc<(Vec<u8>, i32, i32)>;

thread_local! {
    static SVG_CACHE: std::cell::RefCell<std::collections::HashMap<(&'static str, u32, u32), Option<SvgMask>>> = Default::default();
}

/// Gerasterte Deckungsmaske eines Demo-Symbols in Pixelgröße `s` (zwischengespeichert).
fn svg_mask(name: &'static str, s: f32, sw: f32) -> Option<SvgMask> {
    let key = (name, (s * 4.0).round() as u32, (sw * 100.0).round() as u32);
    if let Some(v) = SVG_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return v;
    }
    let v = (|| {
        let path = crate::icons_svg::ICONS.iter().find(|(n, _)| *n == name)?.1;
        let px = s.ceil().max(2.0) as u32 + 2;
        let k = s / 24.0;
        let off = (px as f32 - s) / 2.0;
        let mut buf = vec![0u8; (px * px) as usize];
        zeno::Mask::new(path)
            .style(zeno::Stroke::new(sw).cap(zeno::Cap::Round).join(zeno::Join::Round))
            .transform(Some(zeno::Transform::scale(k, k).then_translate(off, off)))
            .size(px, px)
            .render_into(&mut buf, None);
        Some(std::rc::Rc::new((buf, px as i32, px as i32)))
    })();
    if SVG_CACHE.with(|c| c.borrow().len()) > 600 {
        SVG_CACHE.with(|c| c.borrow_mut().clear());
    }
    SVG_CACHE.with(|c| c.borrow_mut().insert(key, v.clone()));
    v
}
