//! Ultar-Tastatur – Zeichnen und Bedienung. Belegungen, Maße und Trefferflächen kommen aus `ultar-ime`.
//!
//! Aufbau (wie Gboard-Vorlage): Werkzeugleiste (Zwischenablage, Einstellungen, Helfer, Ausblenden) →
//! Zahlenreihe 1–0 → drei Buchstabenreihen → untere Reihe mit Pillen und runder Aktionstaste in Gold.
//! Zeichen werden beim Loslassen eingefügt (Gleiten auf die Nachbartaste ist erlaubt), langer Druck zeigt
//! Varianten (Umlaute, Hamza, Taschkil), Löschen wiederholt beim Halten.

use crate::gfx::{rgb, Color, Rect};
use crate::ui::{Al, Ic, Ui};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use ultar_ime::{self as ime, Act, Lang, Page, Placed, Purpose, Shift, Style};

/// Einstellungen der Tastatur (`/data/ultar/keyboard.toml`).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct KbCfg {
    /// eigene Zahlenreihe 1–0 über den Buchstaben
    pub number_row: bool,
    /// 0 = klein, 1 = normal, 2 = groß
    pub size: u8,
    /// Blase mit dem Zeichen über der gedrückten Taste
    pub preview: bool,
}

impl Default for KbCfg {
    fn default() -> Self {
        Self {
            number_row: true,
            size: 1,
            preview: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Panel {
    Keys,
    Clip,
    Settings,
}

struct Popup {
    key: usize,
    items: Vec<String>,
    sel: usize,
}

pub struct KbState {
    pub cfg: KbCfg,
    pub lang: Lang,
    sys: Lang,
    pub page: Page,
    pub shift: Shift,
    pub panel: Panel,
    last_shift: Option<Instant>,
    down: Option<(usize, Instant)>,
    popup: Option<Popup>,
    repeat: Option<Instant>,
    field: &'static str,
    /// nur für Bildschirmfotos: gedrückte Taste bzw. offene Varianten-Leiste
    dbg_press: Option<String>,
    dbg_alt: bool,
    dbg_state: Option<(Page, Panel, Shift)>,
}

impl KbState {
    pub fn new(sys_lang: &str) -> Self {
        let l = Lang::from_code(sys_lang);
        Self {
            cfg: crate::sys::load("keyboard.toml"),
            lang: l,
            sys: l,
            page: Page::Letters,
            shift: Shift::Off,
            panel: Panel::Keys,
            last_shift: None,
            down: None,
            popup: None,
            repeat: None,
            field: "",
            dbg_press: None,
            dbg_alt: false,
            dbg_state: None,
        }
    }

    /// Zustand für Bildschirmfotos setzen (`--shot … kb-<art>-<sprache>`).
    pub fn debug(&mut self, art: &str, lang: &str) {
        self.lang = Lang::from_code(lang);
        self.sys = self.lang;
        self.field = "";
        let (page, panel, shift) = match art {
            "sym" => (Page::Symbols, Panel::Keys, Shift::Off),
            "sym2" => (Page::Symbols2, Panel::Keys, Shift::Off),
            "clip" => (Page::Letters, Panel::Clip, Shift::Off),
            "set" => (Page::Letters, Panel::Settings, Shift::Off),
            "shift" => (Page::Letters, Panel::Keys, Shift::Once),
            _ => (Page::Letters, Panel::Keys, Shift::Off),
        };
        self.dbg_press = match (art, lang) {
            ("pop", "ar") => Some("ا".into()),
            ("pop", _) => Some("u".into()),
            ("press", "ar") => Some("ب".into()),
            ("press", _) => Some("g".into()),
            _ => None,
        };
        self.dbg_alt = art == "pop";
        self.dbg_state = Some((page, panel, shift));
    }

    /// Höhe der Tastatur in Pixeln für den aktuellen Bildschirm und das Feld.
    pub fn height(&self, ui: &Ui, field: &str) -> f32 {
        let m = ime::metrics(
            ui.w / ui.k,
            ui.h / ui.k,
            self.cfg.number_row,
            self.cfg.size,
            ime::purpose_for(field).pad(),
        );
        ui.dp(m.total)
    }
}

/// Ergebnis einer Bedienung.
#[derive(Clone, PartialEq, Debug)]
pub enum KbOut {
    Insert(String),
    Backspace,
    Enter,
    Hide,
    #[allow(dead_code)] // never constructed; kept, see docs/OFFEN-WARNUNGEN.md
    Helper,
    Paste(usize),
    CopyField,
    /// Feldinhalt kopieren und leeren
    Cut,
    CfgChanged,
}

struct Pal {
    bg: Color,
    key: Color,
    func: Color,
    press: Color,
    text: Color,
    sub: Color,
    tog_bg: Color,
    tog_fg: Color,
    gold: Color,
    on_gold: Color,
}

fn pal(ui: &Ui) -> Pal {
    let th = &ui.th;
    let dark = (th.bg.r as u32 + th.bg.g as u32 + th.bg.b as u32) < 3 * 128;
    let white = rgb(0xFFFFFF);
    let (bg, key, func) = if dark {
        (
            th.bg.mix(white, 0.045),
            th.bg.mix(white, 0.14),
            th.bg.mix(white, 0.095),
        )
    } else {
        (th.bg.mix(th.text, 0.08), white, th.bg.mix(th.text, 0.16))
    };
    Pal {
        bg,
        key,
        func,
        press: key.mix(if dark { white } else { th.text }, 0.18),
        text: th.text,
        sub: th.sub,
        tog_bg: th.accent.mix(bg, if dark { 0.74 } else { 0.8 }),
        tog_fg: if dark {
            th.accent.mix(white, 0.5)
        } else {
            th.accent.mix(th.text, 0.45)
        },
        gold: th.accent,
        on_gold: th.on_accent,
    }
}

fn rr(r: &ime::R) -> Rect {
    Rect::new(r.x, r.y, r.w, r.h)
}

/// Tastatur zeichnen und bedienen. `area` = ganze Tastaturfläche (Breite des Bildschirms).
pub fn keyboard(
    ui: &mut Ui,
    area: Rect,
    st: &mut KbState,
    sys_lang: &str,
    field: &'static str,
    field_text: &str,
    clip: &[String],
    tr: &dyn Fn(&'static str) -> &'static str,
) -> Option<KbOut> {
    let sys = Lang::from_code(sys_lang);
    if sys != st.sys {
        st.sys = sys;
        st.lang = sys;
    }
    if st.field != field {
        // neues Feld: Buchstaben, kein Umschalter, Tasten statt Zwischenablage
        st.field = field;
        st.page = Page::Letters;
        st.shift = Shift::Off;
        st.panel = Panel::Keys;
        st.popup = None;
        st.down = None;
        if let Some((pg, pn, sh)) = st.dbg_state.take() {
            st.page = pg;
            st.panel = pn;
            st.shift = sh;
        }
    }
    let purpose = ime::purpose_for(field);
    let p = pal(ui);
    let k = ui.k;
    let m = ime::metrics(
        ui.w / k,
        ui.h / k,
        st.cfg.number_row,
        st.cfg.size,
        purpose.pad(),
    );
    let kw = m.width * k;
    let kx = area.x + (area.w - kw) / 2.0;
    ui.cv.rect(area, p.bg);
    let mut out = None;

    // ---------- Werkzeugleiste ----------
    let tb = Rect::new(kx, area.y, kw, ui.dp(m.toolbar));
    let s = ui.dp(20.0);
    let slot = ui.dp(48.0).min(kw / 6.0);
    let rtl = st.lang.rtl() && ui.rtl;
    // Wie in der Web-Demo: Kopieren · ✂ · Einfügen · letzte Ablage-Einträge … Einstellungen · Ausblenden
    let gold = p.gold;
    let mut x = ui.dp(10.0);
    let mut acts: Vec<(String, u8)> = Vec::new();
    if !matches!(purpose, Purpose::Pin) && !purpose.sensitive() {
        acts.push((tr("Copy").to_string(), 0));
        acts.push((String::new(), 1));
    }
    if !matches!(purpose, Purpose::Pin) {
        acts.push((tr("Paste").to_string(), 2));
    }
    let end_res = slot * 2.0 + ui.dp(8.0);
    for (lab, k) in acts.iter() {
        let iw = ui.dp(20.0);
        let w = if lab.is_empty() {
            ui.dp(32.0)
        } else {
            ui.text_w(lab, 12.5, true) + iw + ui.dp(14.0)
        };
        let r = Rect::new(
            if rtl { tb.right() - x - w } else { tb.x + x },
            tb.y + ui.dp(4.0),
            w,
            tb.h - ui.dp(8.0),
        );
        if ui.held(r) {
            ui.cv.rrect(r, r.h / 2.0, p.func);
        }
        let ic = match k {
            0 => Ic::Copy,
            1 => Ic::Cut,
            _ => Ic::Doc,
        };
        let icx = if lab.is_empty() {
            r.cx()
        } else if rtl {
            r.right() - ui.dp(14.0)
        } else {
            r.x + ui.dp(14.0)
        };
        ui.icon(ic, icx, r.cy(), ui.dp(13.0), gold);
        if !lab.is_empty() {
            let lr = if rtl {
                Rect::new(r.x, r.y, r.w - iw - ui.dp(6.0), r.h)
            } else {
                Rect::new(r.x + iw + ui.dp(6.0), r.y, r.w - iw - ui.dp(6.0), r.h)
            };
            ui.label(lr, lab, 12.5, gold, true, Al::Start);
        }
        if ui.tap(r) {
            out = Some(match k {
                0 => KbOut::CopyField,
                1 => KbOut::Cut,
                _ => KbOut::Paste(0),
            });
            ui.dirty = true;
        }
        x += w + ui.dp(2.0);
    }
    if !purpose.sensitive() {
        for (i, c) in clip.iter().take(6).enumerate() {
            let one: String = c.replace('\n', " ");
            let lab = ui.fit(&one, 12.5, false, ui.dp(120.0));
            let w = ui.text_w(&lab, 12.5, false) + ui.dp(22.0);
            if x + w > kw - end_res {
                break;
            }
            let r = Rect::new(
                if rtl { tb.right() - x - w } else { tb.x + x },
                tb.y + ui.dp(6.0),
                w,
                tb.h - ui.dp(12.0),
            );
            ui.cv.rrect(r, r.h / 2.0, p.sub.alpha(0.35));
            ui.cv.rrect(
                r.inset(1.0),
                r.h / 2.0 - 1.0,
                if ui.held(r) { p.func } else { p.key },
            );
            ui.label(r, &lab, 12.5, p.text, false, Al::Center);
            if ui.tap(r) {
                out = Some(KbOut::Paste(i));
                ui.dirty = true;
            }
            x += w + ui.dp(6.0);
        }
    }
    // Einstellungen klein vor dem Ausblenden
    let gx = if rtl {
        tb.x + slot * 1.5 + ui.dp(4.0)
    } else {
        tb.right() - slot * 1.5 - ui.dp(4.0)
    };
    let gr = Rect::new(gx - slot / 2.0, tb.y, slot, tb.h);
    if st.panel == Panel::Settings || ui.held(gr) {
        ui.cv.circle(gx, tb.cy(), tb.h * 0.42, p.func);
    }
    ui.icon(Ic::Gear, gx, tb.cy(), s * 0.85, p.sub.mix(p.text, 0.35));
    if ui.tap(gr) {
        st.panel = if st.panel == Panel::Settings {
            Panel::Keys
        } else {
            Panel::Settings
        };
        ui.dirty = true;
    }
    let _ = draw_clip;
    // Ausblenden (Pfeil nach unten) auf der anderen Seite
    let hx = if rtl {
        tb.x + slot / 2.0 + ui.dp(4.0)
    } else {
        tb.right() - slot / 2.0 - ui.dp(4.0)
    };
    let hr = Rect::new(hx - slot / 2.0, tb.y, slot, tb.h);
    if ui.held(hr) {
        ui.cv.circle(hx, tb.cy(), tb.h * 0.42, p.func);
    }
    draw_chevron_down(ui, hx, tb.cy(), s, p.sub.mix(p.text, 0.35));
    if ui.tap(hr) {
        out = Some(KbOut::Hide);
    }

    let body = Rect::new(kx, tb.bottom(), kw, area.bottom() - tb.bottom());
    match st.panel {
        Panel::Clip => return clip_panel(ui, body, &p, purpose, field_text, clip, tr).or(out),
        Panel::Settings => return settings_panel(ui, body, &p, st, tr).or(out),
        Panel::Keys => {}
    }

    // ---------- Tasten ----------
    let rows = ime::rows(st.lang, st.page, purpose, st.shift, m.number_row);
    let placed = ime::place(&rows, body.x, body.y, body.w, &m, k);
    let bias = ui.dp(m.key_h) * 0.08;
    let touch = ui.enabled && ui.p.capture.is_none();
    let inside =
        |x: f32, y: f32| x >= body.x && x < body.right() && y >= body.y && y < body.bottom();

    // Berührung beginnt
    if touch && ui.p.pressed && inside(ui.p.x, ui.p.y) {
        if let Some(i) = ime::hit(&placed, ui.p.x, ui.p.y, bias) {
            st.down = Some((i, Instant::now()));
            st.popup = None;
            st.repeat = None;
            if placed[i].def.act == Act::Backspace {
                out = Some(KbOut::Backspace);
            }
            ui.dirty = true;
        }
    }
    // Finger liegt: Gleiten, langer Druck, Löschen wiederholen
    if let Some((i0, t0)) = st.down {
        if ui.p.down {
            let held = t0.elapsed();
            if let Some(pp) = &mut st.popup {
                // Auswahl in der Varianten-Leiste folgt dem Finger
                let (r, per) = popup_geom(
                    ui,
                    &placed[pp.key].r,
                    pp.items.len(),
                    body,
                    rtl_items(&pp.items),
                );
                let col = (((ui.p.x - r.x) / per).floor().max(0.0) as usize)
                    .min(pp.items.len().saturating_sub(1));
                let n_row = popup_cols(pp.items.len());
                let row = if pp.items.len() > n_row && ui.p.y < r.y + r.h / 2.0 {
                    0
                } else if pp.items.len() > n_row {
                    1
                } else {
                    0
                };
                let idx = (row * n_row + col.min(n_row - 1)).min(pp.items.len() - 1);
                let idx = if rtl_items(&pp.items) {
                    mirror(idx, pp.items.len(), n_row)
                } else {
                    idx
                };
                if idx != pp.sel {
                    pp.sel = idx;
                }
                ui.dirty = true;
            } else if placed[i0].def.act == Act::Backspace {
                if held > Duration::from_millis(450) {
                    let due = st
                        .repeat
                        .map_or(true, |t| t.elapsed() > Duration::from_millis(60));
                    if due {
                        st.repeat = Some(Instant::now());
                        out = Some(KbOut::Backspace);
                    }
                }
                ui.dirty = true;
            } else {
                // Gleiten auf eine andere Taste
                if let Some(j) = ime::hit(&placed, ui.p.x, ui.p.y, bias) {
                    if j != i0 && inside(ui.p.x, ui.p.y) {
                        st.down = Some((j, Instant::now()));
                        ui.dirty = true;
                    }
                }
                let (i, t) = st.down.unwrap();
                if t.elapsed() > Duration::from_millis(380) {
                    if let Some(txt) = placed[i].def.text() {
                        let items = ime::alts(st.lang, txt);
                        if !items.is_empty() {
                            st.popup = Some(Popup {
                                key: i,
                                items,
                                sel: 0,
                            });
                            crate::sys::vibrate(12);
                        }
                    }
                }
                ui.dirty = true;
            }
        }
    }
    // Loslassen: Zeichen einfügen bzw. Taste ausführen
    if ui.p.released {
        if let Some(pp) = st.popup.take() {
            out = Some(KbOut::Insert(pp.items[pp.sel].clone()));
            after_char(st);
        } else if let Some((i, _)) = st.down {
            let def = &placed[i].def;
            match &def.act {
                Act::Text(t) => {
                    out = Some(KbOut::Insert(t.clone()));
                    after_char(st);
                }
                Act::Space => out = Some(KbOut::Insert(" ".into())),
                Act::Action => out = Some(KbOut::Enter),
                Act::Backspace => {}
                Act::Shift => {
                    let quick = st
                        .last_shift
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(350));
                    st.shift = match st.shift {
                        Shift::Off => Shift::Once,
                        Shift::Once if quick => Shift::Lock,
                        _ => Shift::Off,
                    };
                    st.last_shift = Some(Instant::now());
                }
                Act::ToSymbols => st.page = Page::Symbols,
                Act::ToSymbols2 => st.page = Page::Symbols2,
                Act::ToLetters => st.page = Page::Letters,
                Act::Lang => {
                    st.lang = st.lang.next(st.sys);
                    st.page = Page::Letters;
                    st.shift = Shift::Off;
                }
            }
            ui.dirty = true;
        }
        st.down = None;
        st.repeat = None;
    }
    if !ui.p.down && !ui.p.released {
        st.down = None;
        st.popup = None;
    }

    // Bildschirmfoto: gedrückte Taste / Varianten vorgeben
    if let Some(lbl) = &st.dbg_press {
        if let Some(i) = placed.iter().position(|pk| &pk.def.label == lbl) {
            st.down = Some((i, Instant::now()));
            if st.dbg_alt {
                let items = ime::alts(st.lang, lbl);
                st.popup = Some(Popup {
                    key: i,
                    items,
                    sel: 0,
                });
            }
        }
    }
    // Zeichnen
    let pressed = st.down.map(|d| d.0);
    for (i, pk) in placed.iter().enumerate() {
        draw_key(ui, &p, pk, pressed == Some(i), st, purpose);
    }
    // Vorschau-Blase bzw. Varianten-Leiste über der Taste
    if let Some(pp) = &st.popup {
        draw_popup(ui, &p, &placed[pp.key].r, &pp.items, pp.sel, body);
    } else if let Some(i) = pressed {
        let d = &placed[i].def;
        if st.cfg.preview
            && matches!(d.style, Style::Letter | Style::Digit | Style::Pill)
            && d.text().is_some()
        {
            draw_preview(ui, &p, &placed[i].r, &d.label);
        }
    }
    if out.is_some() {
        ui.dirty = true;
    }
    out
}

fn after_char(st: &mut KbState) {
    if st.shift == Shift::Once {
        st.shift = Shift::Off;
    }
}

fn label_size(d: &ime::KeyDef) -> f32 {
    let arabic = d
        .label
        .chars()
        .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
    match d.style {
        Style::Digit => 17.0,
        Style::Pad => 24.0,
        Style::Space => 12.5,
        Style::Toggle => 13.5,
        _ if d.label.chars().count() > 1 && !arabic => 14.0,
        _ if arabic => 20.0,
        _ => 21.0,
    }
}

fn draw_key(ui: &mut Ui, p: &Pal, pk: &Placed, down: bool, st: &KbState, purpose: Purpose) {
    let r = rr(&pk.r);
    let d = &pk.def;
    let pill = r.h / 2.0;
    let rad = ui.dp(9.0);
    match d.style {
        Style::Digit => {
            if down {
                ui.cv.rrect(r, rad, p.func);
            }
        }
        Style::Letter => ui.cv.rrect(r, rad, if down { p.press } else { p.key }),
        Style::Pad => ui
            .cv
            .rrect(r, rad * 1.4, if down { p.press } else { p.key }),
        Style::Func => ui.cv.rrect(r, rad, if down { p.press } else { p.func }),
        Style::Toggle => ui.cv.rrect(
            r,
            pill,
            if down {
                p.tog_bg.mix(p.text, 0.15)
            } else {
                p.tog_bg
            },
        ),
        Style::Pill => ui.cv.rrect(r, pill, if down { p.press } else { p.func }),
        Style::Space => ui.cv.rrect(r, pill, if down { p.press } else { p.key }),
        Style::Action => {
            let c = if down {
                p.gold.mix(p.text, 0.2)
            } else {
                p.gold
            };
            ui.cv.rrect(r, pill, c);
        }
    }
    let old = ui.rtl;
    ui.rtl = false;
    let ic = ui.dp(20.0);
    match &d.act {
        Act::Backspace => ui.icon(Ic::Bksp, r.cx(), r.cy(), ic, p.text),
        Act::Shift => draw_shift(ui, r.cx(), r.cy(), ic, p.text, st.shift),
        Act::Lang => ui.icon(Ic::Globe, r.cx(), r.cy(), ic * 0.95, p.text.mix(p.sub, 0.3)),
        Act::Action => {
            let c = p.on_gold;
            match purpose {
                Purpose::Search => ui.icon(Ic::Search, r.cx(), r.cy(), ic, c),
                Purpose::Send => ui.icon(Ic::Send, r.cx(), r.cy(), ic, c),
                Purpose::Multiline => draw_return(ui, r.cx(), r.cy(), ic, c),
                _ => ui.icon(Ic::Check, r.cx(), r.cy(), ic, c),
            }
        }
        Act::Space if d.label.is_empty() => draw_space(ui, r.cx(), r.cy(), ic, p.text),
        // „؟123“: gemischte Richtung einzeln setzen (؟ links, wie bei Gboard)
        _ if d.label == "123؟" => {
            let c = if d.style == Style::Toggle {
                p.tog_fg
            } else {
                p.text
            };
            let sz = label_size(d);
            let (w1, w2) = (ui.text_w("؟", sz, true), ui.text_w("123", sz, true));
            let x = r.cx() - (w1 + w2) / 2.0;
            ui.label(Rect::new(x, r.y, w1, r.h), "؟", sz, c, true, Al::Center);
            ui.label(
                Rect::new(x + w1, r.y, w2, r.h),
                "123",
                sz,
                c,
                true,
                Al::Center,
            );
        }
        _ => {
            let c = match d.style {
                Style::Space => p.sub,
                Style::Toggle => p.tog_fg,
                _ => p.text,
            };
            let bold = matches!(d.style, Style::Toggle);
            ui.label(r, &d.label, label_size(d), c, bold, Al::Center);
            if let Some(h) = &d.hint {
                let hr = Rect::new(
                    r.right() - ui.dp(13.0),
                    r.y + ui.dp(2.0),
                    ui.dp(11.0),
                    ui.dp(13.0),
                );
                ui.label(hr, h, 10.0, p.sub, false, Al::Center);
            }
        }
    }
    ui.rtl = old;
}

fn draw_preview(ui: &mut Ui, p: &Pal, kr: &ime::R, label: &str) {
    let w = (kr.w * 1.45).max(ui.dp(44.0));
    let h = kr.h * 1.25;
    let x = (kr.cx() - w / 2.0).clamp(ui.dp(2.0), ui.w - w - ui.dp(2.0));
    let r = Rect::new(x, kr.y - h - ui.dp(6.0), w, h);
    ui.cv.soft_shadow(r, ui.dp(12.0), rgb(0));
    ui.cv.rrect(r, ui.dp(12.0), p.press);
    let old = ui.rtl;
    ui.rtl = false;
    ui.label(r, label, 28.0, p.text, false, Al::Center);
    ui.rtl = old;
}

fn popup_cols(n: usize) -> usize {
    if n > 7 {
        n.div_ceil(2)
    } else {
        n.max(1)
    }
}

/// Arabische Varianten von rechts nach links anordnen.
fn rtl_items(items: &[String]) -> bool {
    items
        .first()
        .is_some_and(|s| s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)))
}

/// Index in einer Reihe spiegeln (für Rechts-nach-links).
fn mirror(idx: usize, n: usize, cols: usize) -> usize {
    let (row, col) = (idx / cols, idx % cols);
    let in_row = (n - row * cols).min(cols);
    let col = in_row
        .saturating_sub(1)
        .saturating_sub(col.min(in_row.saturating_sub(1)));
    (row * cols + col).min(n - 1)
}

/// Rechteck der Varianten-Leiste und Breite je Eintrag.
fn popup_geom(ui: &Ui, kr: &ime::R, n: usize, body: Rect, rtl: bool) -> (Rect, f32) {
    let cols = popup_cols(n);
    let rows = n.div_ceil(cols);
    let per = kr.w.max(ui.dp(36.0));
    let w = per * cols as f32 + ui.dp(8.0);
    let h = kr.h.min(ui.dp(48.0)) * rows as f32 + ui.dp(8.0);
    // die erste (vorausgewählte) Variante steht genau über dem Finger
    let x = if rtl {
        kr.cx() + per / 2.0 + ui.dp(4.0) - w
    } else {
        kr.cx() - per / 2.0 - ui.dp(4.0)
    };
    let x = x.clamp(body.x + ui.dp(2.0), body.right() - w - ui.dp(2.0));
    (Rect::new(x, kr.y - h - ui.dp(6.0), w, h), per)
}

fn draw_popup(ui: &mut Ui, p: &Pal, kr: &ime::R, items: &[String], sel: usize, body: Rect) {
    let (r, per) = popup_geom(ui, kr, items.len(), body, rtl_items(items));
    let cols = popup_cols(items.len());
    let ih = (r.h - ui.dp(8.0)) / items.len().div_ceil(cols) as f32;
    ui.cv.soft_shadow(r, ui.dp(12.0), rgb(0));
    ui.cv.rrect(r, ui.dp(12.0), p.press);
    ui.cv.rrect(r.inset(1.0), ui.dp(12.0), p.key);
    let rtl = rtl_items(items);
    let old = ui.rtl;
    ui.rtl = false;
    for (i, it) in items.iter().enumerate() {
        let vis = if rtl { mirror(i, items.len(), cols) } else { i };
        let (row, col) = (vis / cols, vis % cols);
        let cell = Rect::new(
            r.x + ui.dp(4.0) + col as f32 * per,
            r.y + ui.dp(4.0) + row as f32 * ih,
            per,
            ih,
        );
        let fg = if i == sel {
            ui.cv.rrect(cell.inset(ui.dp(2.0)), ui.dp(8.0), p.gold);
            p.on_gold
        } else {
            p.text
        };
        ui.label(
            cell,
            it,
            if it.chars().count() > 2 && !rtl {
                13.0
            } else {
                20.0
            },
            fg,
            false,
            Al::Center,
        );
    }
    ui.rtl = old;
}

fn clip_panel(
    ui: &mut Ui,
    body: Rect,
    p: &Pal,
    purpose: Purpose,
    field_text: &str,
    clip: &[String],
    tr: &dyn Fn(&'static str) -> &'static str,
) -> Option<KbOut> {
    let mut out = None;
    let m = ui.dp(10.0);
    let head = Rect::new(
        body.x + m,
        body.y + ui.dp(4.0),
        body.w - 2.0 * m,
        ui.dp(36.0),
    );
    let ta = Al::Start;
    ui.label(head, tr("Clipboard"), 14.0, p.sub, true, ta);
    if !purpose.sensitive() && !field_text.is_empty() {
        let lab = tr("Copy");
        let w = ui.text_w(lab, 13.5, false) + ui.dp(26.0);
        let x = if ui.rtl { head.x } else { head.right() - w };
        let r = Rect::new(x, head.y, w, head.h);
        ui.cv
            .rrect(r, r.h / 2.0, if ui.held(r) { p.press } else { p.func });
        ui.label(r, lab, 13.5, p.text, false, Al::Center);
        if ui.tap(r) {
            out = Some(KbOut::CopyField);
        }
    }
    let mut y = head.bottom() + ui.dp(6.0);
    if clip.is_empty() {
        ui.label(
            Rect::new(body.x, y, body.w, ui.dp(60.0)),
            tr("Nothing copied yet"),
            14.0,
            p.sub,
            false,
            Al::Center,
        );
        return out;
    }
    let rh = ui.dp(46.0);
    for (i, c) in clip.iter().enumerate() {
        if y + rh > body.bottom() - ui.dp(4.0) {
            break;
        }
        let r = Rect::new(body.x + m, y, body.w - 2.0 * m, rh);
        ui.cv
            .rrect(r, ui.dp(10.0), if ui.held(r) { p.press } else { p.key });
        let one: String = c.replace('\n', " ");
        let one = ui.fit(&one, 14.0, false, r.w - ui.dp(24.0));
        ui.label(r.inset(ui.dp(12.0)), &one, 14.0, p.text, false, ta);
        if ui.tap(r) {
            out = Some(KbOut::Paste(i));
        }
        y += rh + ui.dp(6.0);
    }
    out
}

fn settings_panel(
    ui: &mut Ui,
    body: Rect,
    p: &Pal,
    st: &mut KbState,
    tr: &dyn Fn(&'static str) -> &'static str,
) -> Option<KbOut> {
    let mut out = None;
    let m = ui.dp(12.0);
    let rh = ui.dp(48.0);
    let mut y = body.y + ui.dp(4.0);
    let ta = Al::Start;
    // Schalter-Zeilen
    let mut toggle = |ui: &mut Ui, y: &mut f32, title: &'static str, on: &mut bool| {
        let r = Rect::new(body.x + m, *y, body.w - 2.0 * m, rh);
        ui.label(r, tr(title), 15.0, p.text, false, ta);
        let (tw, th) = (ui.dp(46.0), ui.dp(28.0));
        let tx = if ui.rtl { r.x } else { r.right() - tw };
        let t = Rect::new(tx, r.cy() - th / 2.0, tw, th);
        ui.cv.rrect(t, th / 2.0, if *on { p.gold } else { p.func });
        let kx = if *on != ui.rtl {
            t.right() - th / 2.0
        } else {
            t.x + th / 2.0
        };
        ui.cv
            .circle(kx, t.cy(), th / 2.0 - ui.dp(3.0), rgb(0xFFFFFF));
        if ui.tap(r) {
            *on = !*on;
            out = Some(KbOut::CfgChanged);
            ui.dirty = true;
        }
        *y += rh;
    };
    toggle(ui, &mut y, "Number row", &mut st.cfg.number_row);
    toggle(ui, &mut y, "Key preview", &mut st.cfg.preview);
    // Größe
    let r = Rect::new(body.x + m, y, body.w - 2.0 * m, rh);
    ui.label(r, tr("Key size"), 15.0, p.text, false, ta);
    let names = [tr("Small"), tr("Normal"), tr("Large")];
    let pw: Vec<f32> = names
        .iter()
        .map(|n| ui.text_w(n, 13.0, false) + ui.dp(22.0))
        .collect();
    let total: f32 = pw.iter().sum::<f32>() + ui.dp(6.0) * 2.0;
    let mut x = if ui.rtl { r.x } else { r.right() - total };
    let order: Vec<usize> = if ui.rtl { vec![2, 1, 0] } else { vec![0, 1, 2] };
    for i in order {
        let pr = Rect::new(x, r.cy() - ui.dp(16.0), pw[i], ui.dp(32.0));
        let on = st.cfg.size == i as u8;
        ui.cv
            .rrect(pr, pr.h / 2.0, if on { p.gold } else { p.func });
        ui.label(
            pr,
            names[i],
            13.0,
            if on { p.on_gold } else { p.text },
            on,
            Al::Center,
        );
        if ui.tap(pr) {
            st.cfg.size = i as u8;
            out = Some(KbOut::CfgChanged);
            ui.dirty = true;
        }
        x += pw[i] + ui.dp(6.0);
    }
    out
}

// ---------- kleine Symbole ----------

fn draw_clip(ui: &mut Ui, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s / 2.0;
    let t = (s * 0.1).max(1.5);
    let b = Rect::new(cx - r * 0.72, cy - r * 0.78, r * 1.44, r * 1.75);
    for (x0, y0, x1, y1) in [
        (b.x, b.y, b.right(), b.y),
        (b.x, b.y, b.x, b.bottom()),
        (b.right(), b.y, b.right(), b.bottom()),
        (b.x, b.bottom(), b.right(), b.bottom()),
    ] {
        ui.cv.line(x0, y0, x1, y1, t, c);
    }
    ui.cv.rrect(
        Rect::new(cx - r * 0.38, cy - r * 0.98, r * 0.76, r * 0.42),
        r * 0.12,
        c,
    );
    for i in 0..2 {
        let y = cy - r * 0.05 + i as f32 * r * 0.42;
        ui.cv.line(cx - r * 0.38, y, cx + r * 0.38, y, t * 0.9, c);
    }
}

fn draw_chevron_down(ui: &mut Ui, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s / 2.0;
    let t = (s * 0.12).max(1.5);
    ui.cv
        .line(cx - r * 0.7, cy - r * 0.3, cx, cy + r * 0.4, t, c);
    ui.cv
        .line(cx, cy + r * 0.4, cx + r * 0.7, cy - r * 0.3, t, c);
}

fn draw_shift(ui: &mut Ui, cx: f32, cy: f32, s: f32, c: Color, st: Shift) {
    let r = s / 2.0;
    let t = (s * 0.1).max(1.5);
    let pts = [
        (0.0f32, -0.85f32),
        (0.8, 0.0),
        (0.38, 0.0),
        (0.38, 0.6),
        (-0.38, 0.6),
        (-0.38, 0.0),
        (-0.8, 0.0),
        (0.0, -0.85),
    ];
    if st == Shift::Off {
        for w in pts.windows(2) {
            ui.cv.line(
                cx + w[0].0 * r,
                cy + w[0].1 * r,
                cx + w[1].0 * r,
                cy + w[1].1 * r,
                t,
                c,
            );
        }
    } else {
        ui.cv
            .sdf(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), c, |x, y| {
                let (px, py) = ((x - cx) / r, (y - cy) / r);
                let tri = if py <= 0.0 && py >= -0.85 {
                    px.abs() - (0.8 * (py + 0.85) / 0.85)
                } else {
                    1.0
                };
                let stem = if py > -0.05 && py <= 0.6 {
                    px.abs() - 0.38
                } else {
                    1.0
                };
                tri.min(stem) * r
            });
        if st == Shift::Lock {
            ui.cv.line(
                cx - r * 0.45,
                cy + r * 0.9,
                cx + r * 0.45,
                cy + r * 0.9,
                t * 1.2,
                c,
            );
        }
    }
}

fn draw_return(ui: &mut Ui, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s / 2.0;
    let t = (s * 0.11).max(1.5);
    ui.cv
        .line(cx + r * 0.7, cy - r * 0.6, cx + r * 0.7, cy + r * 0.2, t, c);
    ui.cv
        .line(cx + r * 0.7, cy + r * 0.2, cx - r * 0.7, cy + r * 0.2, t, c);
    ui.cv
        .line(cx - r * 0.7, cy + r * 0.2, cx - r * 0.3, cy - r * 0.2, t, c);
    ui.cv
        .line(cx - r * 0.7, cy + r * 0.2, cx - r * 0.3, cy + r * 0.6, t, c);
}

fn draw_space(ui: &mut Ui, cx: f32, cy: f32, s: f32, c: Color) {
    let r = s / 2.0;
    let t = (s * 0.1).max(1.5);
    ui.cv
        .line(cx - r * 0.8, cy + r * 0.2, cx + r * 0.8, cy + r * 0.2, t, c);
    ui.cv
        .line(cx - r * 0.8, cy - r * 0.2, cx - r * 0.8, cy + r * 0.2, t, c);
    ui.cv
        .line(cx + r * 0.8, cy - r * 0.2, cx + r * 0.8, cy + r * 0.2, t, c);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jede Beschriftung passt mit der echten Schrift in ihre Taste – auf allen Prüfgrößen.
    #[test]
    fn labels_fit_their_keys() {
        let mut tx = crate::text::Text::new();
        for (w, h, k) in [
            (1080.0f32, 2340.0f32, 3.0f32),
            (720.0, 1280.0, 2.0),
            (2340.0, 1080.0, 3.0),
            (1280.0, 800.0, 1.33),
        ] {
            let (wd, hd) = (w / k, h / k);
            for lang in Lang::ALL {
                for page in [Page::Letters, Page::Symbols, Page::Symbols2] {
                    for purpose in [Purpose::Text, Purpose::Pin, Purpose::Phone, Purpose::Number] {
                        let m = ime::metrics(wd, hd, true, 0, purpose.pad());
                        let rows = ime::rows(lang, page, purpose, Shift::Once, m.number_row);
                        for pk in ime::place(&rows, 0.0, 0.0, m.width * k, &m, k) {
                            let d = &pk.def;
                            if d.label.is_empty() {
                                continue;
                            }
                            let bold = d.style == Style::Toggle;
                            let (tw, th) =
                                tx.measure(&d.label, (label_size(d) * k).round(), bold, 0.0);
                            assert!(
                                tw <= pk.r.w - 2.0,
                                "{w}x{h} {lang:?} {page:?}: „{}“ {tw} > {}",
                                d.label,
                                pk.r.w
                            );
                            assert!(
                                th <= pk.r.h * 1.45,
                                "{w}x{h} {lang:?}: „{}“ zu hoch",
                                d.label
                            );
                        }
                    }
                }
            }
        }
    }

    /// Bedienung: Tippen fügt beim Loslassen ein, langer Druck zeigt Umlaute, Löschen sofort, Umschalter einmalig.
    #[test]
    fn typing_long_press_backspace_shift() {
        use crate::gfx::Canvas;
        use crate::ui::{theme, Ptr};
        let mut cv = Canvas::new(360, 780);
        let mut tx = crate::text::Text::new();
        let mut st = KbState::new("de");
        let area = Rect::new(0.0, 780.0 - 320.0, 360.0, 320.0);
        let tr = |k: &'static str| k;
        let mut frame = |p: &mut Ptr, st: &mut KbState| {
            let mut ui = Ui {
                th: theme(true, 0),
                k: 1.0,
                w: 360.0,
                h: 780.0,
                cv: &mut cv,
                tx: &mut tx,
                p,
                rtl: false,
                enabled: true,
                focus: None,
                dirty: false,
                clip: None,
                links: Vec::new(),
                kb: Default::default(),
                seen: None,
            };
            keyboard(&mut ui, area, st, "de", "cname", "", &[], &tr)
        };
        let m = ime::metrics(360.0, 780.0, st.cfg.number_row, st.cfg.size, false);
        let rows = ime::rows(
            Lang::De,
            Page::Letters,
            Purpose::Text,
            Shift::Off,
            m.number_row,
        );
        let placed = ime::place(&rows, 0.0, area.y + m.toolbar, 360.0, &m, 1.0);
        let at = |lbl: &str| {
            placed
                .iter()
                .find(|k| {
                    k.def.label == lbl
                        || (lbl == "<bk>" && k.def.act == Act::Backspace)
                        || (lbl == "<sh>" && k.def.act == Act::Shift)
                })
                .unwrap()
                .r
        };
        let tap = |frame: &mut dyn FnMut(&mut Ptr, &mut KbState) -> Option<KbOut>,
                   st: &mut KbState,
                   r: ime::R,
                   hold_ms: u64| {
            let mut p = Ptr {
                x: r.cx(),
                y: r.cy(),
                px: r.cx(),
                py: r.cy(),
                down: true,
                pressed: true,
                ..Default::default()
            };
            let mut outs = vec![frame(&mut p, st)];
            p.pressed = false;
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(hold_ms) {
                outs.push(frame(&mut p, st));
                std::thread::sleep(Duration::from_millis(20));
            }
            p.down = false;
            p.released = true;
            outs.push(frame(&mut p, st));
            outs.into_iter().flatten().collect::<Vec<_>>()
        };
        assert_eq!(
            tap(&mut frame, &mut st, at("g"), 0),
            vec![KbOut::Insert("g".into())]
        );
        assert_eq!(
            tap(&mut frame, &mut st, at("u"), 500),
            vec![KbOut::Insert("ü".into())]
        );
        assert_eq!(
            tap(&mut frame, &mut st, at("<bk>"), 0),
            vec![KbOut::Backspace]
        );
        // Löschen halten wiederholt
        assert!(tap(&mut frame, &mut st, at("<bk>"), 700).len() >= 3);
        // Umschalter: ein Großbuchstabe, danach wieder klein
        assert!(tap(&mut frame, &mut st, at("<sh>"), 0).is_empty());
        assert_eq!(st.shift, Shift::Once);
        assert_eq!(
            tap(&mut frame, &mut st, at("a"), 0),
            vec![KbOut::Insert("A".into())]
        );
        assert_eq!(st.shift, Shift::Off);
        // Lücke zwischen zwei Tasten trifft trotzdem
        let (a, s_) = (at("a"), at("s"));
        let gap = ime::R {
            x: (a.right() + s_.x) / 2.0 - 0.5,
            y: a.y,
            w: 1.0,
            h: a.h,
        };
        assert_eq!(tap(&mut frame, &mut st, gap, 0).len(), 1);
    }

    #[test]
    fn mirror_reverses_row() {
        assert_eq!(mirror(0, 5, 5), 4);
        assert_eq!(mirror(4, 5, 5), 0);
        assert_eq!(mirror(0, 9, 5), 4);
        assert_eq!(mirror(5, 9, 5), 8);
        assert_eq!(mirror(8, 9, 5), 5);
    }
}
