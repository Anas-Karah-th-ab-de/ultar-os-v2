//! Weitere Grund-Apps: Notizen, Kontakte und der Starter für grafische Ubuntu-Programme.
//! Untermodul von `app`. Daten liegen als TOML unter /data/ultar (notes.toml, contacts.toml).

use super::{App, Screen};
use crate::gfx::{rgb, Rect};
use crate::sys;
use crate::ui::{Al, Btn, Ic, Ui, ACCENTS};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Note {
    pub text: String,
    pub mtime: i64,
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Notes {
    pub list: Vec<Note>,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Contact {
    pub name: String,
    pub number: String,
    pub fav: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Contacts {
    pub list: Vec<Contact>,
}

/// Grafisches Ubuntu-Programm (aus einer .desktop-Datei im Ubuntu-Bereich)
#[derive(Clone)]
pub struct DeskApp {
    pub name: String,
    pub exec: String,
    pub comment: String,
}

#[derive(Default)]
pub struct Extra {
    loaded: bool,
    pub notes: Notes,
    pub contacts: Contacts,
    pub desk: Option<Vec<DeskApp>>,
    pub del_armed: bool,
}

/// .desktop-Dateien lesen: nur sichtbare Programme mit Fenster.
pub fn desktop_apps(dir: &str) -> Vec<DeskApp> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return v;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(s) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Some(a) = parse_desktop(&s) {
            v.push(a);
        }
    }
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v.dedup_by(|a, b| a.name == b.name);
    v
}

pub fn parse_desktop(s: &str) -> Option<DeskApp> {
    let (mut name, mut exec, mut comment) = (String::new(), String::new(), String::new());
    let mut ultar_id = String::new();
    let mut main = false;
    for l in s.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            main = l == "[Desktop Entry]";
            continue;
        }
        if !main {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" => name = v.trim().into(),
            "Comment" => comment = v.trim().into(),
            "Exec" => {
                // Platzhalter wie %U, %f entfernen
                exec = v
                    .split_whitespace()
                    .filter(|w| !w.starts_with('%'))
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            // Von uapp installiert: Start immer über „uapp run“ (kennt Wine-Bereich, Android-Paket, Umgebung)
            "X-Ultar-App" => ultar_id = v.trim().into(),
            "NoDisplay" | "Hidden" | "Terminal" if v.trim() == "true" => return None,
            "Type" if v.trim() != "Application" => return None,
            _ => {}
        }
    }
    if !ultar_id.is_empty()
        && ultar_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        exec = format!("uapp run {ultar_id}");
    }
    (!name.is_empty() && !exec.is_empty()).then_some(DeskApp {
        name,
        exec,
        comment,
    })
}

fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

impl App {
    pub(super) fn extra_load(&mut self) {
        if !self.extra.loaded {
            self.extra.notes = sys::load("notes.toml");
            self.extra.contacts = sys::load("contacts.toml");
            self.extra.loaded = true;
        }
    }

    fn notes_save(&mut self) {
        if let Err(e) = sys::save("notes.toml", &self.extra.notes) {
            self.toast(e);
        }
    }

    pub(super) fn contacts_save(&mut self) {
        self.extra.contacts.list.sort_by(|a, b| {
            b.fav
                .cmp(&a.fav)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        if let Err(e) = sys::save("contacts.toml", &self.extra.contacts) {
            self.toast(e);
        }
    }

    pub(super) fn note_add(&mut self, text: &str) {
        self.extra_load();
        self.extra.notes.list.insert(
            0,
            Note {
                text: text.into(),
                mtime: sys::unix_now(),
                pinned: false,
            },
        );
        self.notes_save();
    }

    pub(super) fn contact_add(&mut self, name: &str, number: &str) {
        self.extra_load();
        self.extra.contacts.list.retain(|c| c.name != name);
        self.extra.contacts.list.push(Contact {
            name: name.into(),
            number: number.into(),
            fav: false,
        });
        self.contacts_save();
    }

    /// Neu-Knopf wie in der Web-Demo: Notizen = abgerundetes Quadrat unten am Ende, sonst „+" in der Kopfzeile
    fn fab(&mut self, ui: &mut Ui, area: Rect) -> bool {
        if matches!(self.stack.last().map(|s| &s.0), Some(Screen::Notes)) {
            let d = ui.dp(58.0);
            let x = if ui.rtl {
                ui.dp(16.0)
            } else {
                ui.w - ui.dp(16.0) - d
            };
            let r = Rect::new(x, area.bottom() - ui.dp(16.0) - d, d, d);
            let held = ui.held(r);
            ui.cv.soft_shadow(r, ui.dp(20.0), rgb(0));
            ui.cv.rrect(
                r,
                ui.dp(20.0),
                if held {
                    ui.th.accent.mix(rgb(0), 0.1)
                } else {
                    ui.th.accent
                },
            );
            ui.icon(Ic::Plus, r.cx(), r.cy(), ui.dp(24.0), ui.th.on_accent);
            return ui.tap(r);
        }
        std::mem::take(&mut self.hdr_tap)
    }

    pub(super) fn empty(&mut self, ui: &mut Ui, area: Rect, ic: Ic, title: &str, sub: &str) {
        let cy = area.y + area.h * 0.3;
        ui.cv
            .circle(area.cx(), cy, ui.dp(42.0), ui.th.accent.alpha(0.13));
        let a = ui.th.accent;
        ui.icon(ic, area.cx(), cy, ui.dp(38.0), a);
        let (c, cs) = (ui.th.text, ui.th.sub);
        ui.label(
            Rect::new(area.x, cy + ui.dp(56.0), area.w, ui.dp(28.0)),
            title,
            18.0,
            c,
            true,
            Al::Center,
        );
        ui.label(
            Rect::new(
                area.x + ui.dp(20.0),
                cy + ui.dp(86.0),
                area.w - ui.dp(40.0),
                ui.dp(22.0),
            ),
            sub,
            13.0,
            cs,
            false,
            Al::Center,
        );
    }

    // ------------------------------------------------------------ Notizen

    pub(super) fn s_notes(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        self.extra_load();
        let area = self.header(ui, area, self.t("Notes"));
        // leer gebliebene Notizen (angelegt, nichts geschrieben) wieder entfernen
        let n0 = self.extra.notes.list.len();
        self.extra.notes.list.retain(|n| !n.text.trim().is_empty());
        if self.extra.notes.list.len() != n0 {
            self.notes_save();
        }
        let list = self.extra.notes.list.clone();
        if list.is_empty() {
            self.empty(
                ui,
                area,
                Ic::Note,
                self.t("No notes yet"),
                self.t("Tap + to write your first note."),
            );
        } else {
            ui.scroll_begin(area, sc);
            // zwei Spalten auf dem Handy, mehr auf breiten Bildschirmen; Karten gleich hoch
            let cols = ((ui.w / ui.dp(190.0)) as usize).clamp(2, 5);
            let (side, gap) = (ui.dp(16.0), ui.dp(10.0));
            let cw = (ui.w - 2.0 * side - gap * (cols - 1) as f32) / cols as f32;
            let ch = ui.dp(120.0);
            // angeheftete zuerst
            let mut order: Vec<usize> = (0..list.len()).collect();
            order.sort_by_key(|&i| (!list[i].pinned, std::cmp::Reverse(list[i].mtime)));
            let (c, cs) = (ui.th.text, ui.th.sub);
            let mut open = None;
            for (k, &i) in order.iter().enumerate() {
                let n = &list[i];
                let col = k % cols;
                let col = if ui.rtl { cols - 1 - col } else { col };
                let r = Rect::new(
                    side + col as f32 * (cw + gap),
                    area.y - *sc + (k / cols) as f32 * (ch + gap),
                    cw,
                    ch,
                );
                if !ui.visible(r) {
                    continue;
                }
                // Karte wie in der Demo: angeheftete mit goldenem Rand und Stecknadel
                let held = ui.held(r);
                ui.cv.rrect(
                    r,
                    ui.dp(20.0),
                    if n.pinned {
                        ui.th.accent.mix(ui.th.card, 0.5)
                    } else {
                        ui.th.line
                    },
                );
                ui.cv.rrect(
                    r.inset(1.0),
                    ui.dp(19.0),
                    if held { ui.th.press } else { ui.th.card },
                );
                let inner = r.inset(ui.dp(14.0));
                let mut lines = n.text.lines();
                let title = lines.next().unwrap_or("").to_string();
                let rest: String = lines.collect::<Vec<_>>().join(" ");
                let (tx, tw) = if n.pinned {
                    let px = if ui.rtl {
                        inner.right() - ui.dp(8.0)
                    } else {
                        inner.x + ui.dp(8.0)
                    };
                    ui.icon(
                        Ic::Pin,
                        px,
                        inner.y + ui.dp(11.0),
                        ui.dp(9.0),
                        rgb(0xE5484D),
                    );
                    (
                        if ui.rtl {
                            inner.x
                        } else {
                            inner.x + ui.dp(20.0)
                        },
                        inner.w - ui.dp(20.0),
                    )
                } else {
                    (inner.x, inner.w)
                };
                let tf = ui.fit(&title, 15.0, true, tw);
                ui.label(
                    Rect::new(tx, inner.y, tw, ui.dp(22.0)),
                    &tf,
                    15.0,
                    c,
                    true,
                    Al::Start,
                );
                let old = ui.clip;
                let body = Rect::new(
                    inner.x,
                    inner.y + ui.dp(28.0),
                    inner.w,
                    inner.h - ui.dp(28.0),
                );
                let cl = match old {
                    Some(o) => Rect::new(
                        body.x,
                        body.y.max(o.y),
                        body.w,
                        (body.bottom().min(o.bottom()) - body.y.max(o.y)).max(0.0),
                    ),
                    None => body,
                };
                ui.set_clip(Some(cl));
                ui.para(
                    body.x,
                    body.y,
                    body.w,
                    &rest.chars().take(160).collect::<String>(),
                    13.0,
                    cs,
                );
                let _ = ACCENTS;
                ui.set_clip(old);
                if ui.tap(r) {
                    open = Some(i);
                }
            }
            ui.scroll_end(
                area,
                list.len().div_ceil(cols) as f32 * (ch + gap) + ui.dp(90.0),
                sc,
            );
            if let Some(i) = open {
                self.fields.insert("note", list[i].text.clone());
                self.extra.del_armed = false;
                self.go(Screen::NoteEdit(i));
                ui.dirty = true;
            }
        }
        if self.fab(ui, area) {
            self.extra.notes.list.insert(
                0,
                Note {
                    text: String::new(),
                    mtime: sys::unix_now(),
                    pinned: false,
                },
            );
            self.fields.insert("note", String::new());
            self.extra.del_armed = false;
            self.go(Screen::NoteEdit(0));
            self.focus = Some("note");
            ui.dirty = true;
        }
    }

    pub(super) fn s_note_edit(&mut self, ui: &mut Ui, area: Rect, idx: usize, sc: &mut f32) {
        self.extra_load();
        if idx >= self.extra.notes.list.len() {
            self.back();
            return;
        }
        let text = self.f("note");
        // fortlaufend sichern – es gibt keinen Speichern-Knopf, den man vergessen könnte
        if self.extra.notes.list[idx].text != text {
            self.extra.notes.list[idx].text = text.clone();
            self.extra.notes.list[idx].mtime = sys::unix_now();
            self.notes_save();
        }
        // Wie in der Web-Demo: Kopf mit Zurück, rechts Anheften / Kopieren / Löschen; darunter Titel groß und Text,
        // ohne Rahmen
        let (c, cs) = (ui.th.text, ui.th.sub);
        let hh = ui.dp(56.0);
        let cy = area.y + hh / 2.0;
        let bx = if ui.rtl {
            ui.w - ui.dp(30.0)
        } else {
            ui.dp(30.0)
        };
        if ui.icon_btn(bx, cy, ui.dp(19.0), Ic::Back, ui.th.card.alpha(0.0), c) {
            self.focus = None;
            self.back();
            ui.dirty = true;
            return;
        }
        let pinned = self.extra.notes.list[idx].pinned;
        let step = ui.dp(44.0);
        let pos = |k: f32| {
            if ui.rtl {
                ui.dp(28.0) + k * step
            } else {
                ui.w - ui.dp(28.0) - k * step
            }
        };
        let (xd, xc, xp) = (pos(0.0), pos(1.0), pos(2.0));
        let dc = ui.th.danger;
        if ui.icon_btn(
            xd,
            cy,
            ui.dp(17.0),
            Ic::Trash,
            if self.extra.del_armed {
                dc.alpha(0.25)
            } else {
                ui.th.card.alpha(0.0)
            },
            if self.extra.del_armed { dc } else { c },
        ) {
            if self.extra.del_armed || text.trim().is_empty() {
                self.extra.notes.list.remove(idx);
                self.notes_save();
                self.extra.del_armed = false;
                self.focus = None;
                self.back();
            } else {
                self.extra.del_armed = true;
                self.toast(self.t("Tap again to delete").into());
            }
            ui.dirty = true;
            return;
        }
        if ui.icon_btn(xc, cy, ui.dp(17.0), Ic::Copy, ui.th.card.alpha(0.0), c) {
            self.copy_text(&text);
            ui.dirty = true;
        }
        let ac = ui.th.accent;
        if ui.icon_btn(
            xp,
            cy,
            ui.dp(17.0),
            Ic::Pin,
            ui.th.card.alpha(0.0),
            if pinned { ac } else { c },
        ) {
            self.extra.notes.list[idx].pinned = !pinned;
            self.notes_save();
            ui.dirty = true;
        }
        let r = Rect::new(
            ui.dp(16.0),
            area.y + hh,
            ui.w - ui.dp(32.0),
            (area.h - hh - ui.dp(10.0)).max(ui.dp(60.0)),
        );
        let focused = self.focus == Some("note");
        let inner = r.inset(ui.dp(4.0));
        ui.scroll_begin(inner, sc);
        let (title, body) = match text.split_once('\n') {
            Some((t, b)) => (t.to_string(), Some(b.to_string())),
            None => (text.clone(), None),
        };
        let mut yy = inner.y - *sc;
        let tshown = if title.is_empty() && !focused {
            self.t("Title").to_string()
        } else if focused && body.is_none() {
            format!("{title}|")
        } else {
            title.clone()
        };
        let tf = ui.fit(&tshown, 22.0, true, inner.w);
        ui.label(
            Rect::new(inner.x, yy, inner.w, ui.dp(32.0)),
            &tf,
            22.0,
            if title.is_empty() && !focused { cs } else { c },
            true,
            Al::Start,
        );
        yy += ui.dp(42.0);
        let bshown = match &body {
            Some(b) => {
                if focused {
                    format!("{b}|")
                } else {
                    b.clone()
                }
            }
            None => {
                if focused {
                    String::new()
                } else {
                    self.t("Write something…").to_string()
                }
            }
        };
        let bh = ui.para(
            inner.x,
            yy,
            inner.w,
            &bshown,
            16.0,
            if body.is_none() { cs } else { c },
        );
        let h = yy + bh + *sc - inner.y;
        if focused && h > inner.h {
            *sc = h - inner.h;
        }
        ui.scroll_end(inner, h, sc);
        if !focused {
            ui.label(
                Rect::new(inner.x, area.bottom() - ui.dp(26.0), inner.w, ui.dp(18.0)),
                self.t("Saved continuously"),
                12.0,
                cs,
                false,
                Al::Start,
            );
        }
        if ui.tap(r) && !focused {
            ui.focus = Some("note");
            ui.dirty = true;
        }
    }

    // ------------------------------------------------------------ Kontakte

    pub(super) fn s_contacts(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        self.extra_load();
        let area = self.header(ui, area, self.t("Contacts"));
        let list = self.extra.contacts.list.clone();
        if list.is_empty() {
            self.empty(
                ui,
                area,
                Ic::Person,
                self.t("No contacts yet"),
                self.t("Tap + to add a contact."),
            );
        } else {
            ui.scroll_begin(area, sc);
            let mut y = area.y - *sc;
            let m = ui.dp(12.0);
            let (c, cs) = (ui.th.text, ui.th.sub);
            for (i, ct) in list.iter().enumerate() {
                let h = ui.dp(68.0);
                let r = Rect::new(m, y, ui.w - 2.0 * m, h);
                y += h + ui.dp(8.0);
                if !ui.visible(r) {
                    continue;
                }
                ui.cv.rrect(
                    r,
                    ui.dp(16.0),
                    if ui.held(r) { ui.th.press } else { ui.th.card },
                );
                let hue = rgb(ACCENTS[(ct.name.len() + i) % ACCENTS.len()]);
                let ar = ui.dp(22.0);
                let ax = if ui.rtl {
                    r.right() - ui.dp(14.0) - ar
                } else {
                    r.x + ui.dp(14.0) + ar
                };
                ui.cv.circle(ax, r.cy(), ar, hue.alpha(0.22));
                ui.label(
                    Rect::new(ax - ar, r.cy() - ar, 2.0 * ar, 2.0 * ar),
                    &initial(&ct.name),
                    17.0,
                    hue.mix(rgb(0xFFFFFF), 0.3),
                    true,
                    Al::Center,
                );
                // rechts: Anrufen
                let cx = if ui.rtl {
                    r.x + ui.dp(34.0)
                } else {
                    r.right() - ui.dp(34.0)
                };
                let ok = ui.th.ok;
                let call = ui.icon_btn(cx, r.cy(), ui.dp(19.0), Ic::Phone, ok.alpha(0.18), ok);
                let tx = if ui.rtl {
                    r.x + ui.dp(64.0)
                } else {
                    r.x + ui.dp(72.0)
                };
                let tw = r.w - ui.dp(72.0) - ui.dp(64.0);
                ui.label(
                    Rect::new(tx, r.y + ui.dp(11.0), tw, ui.dp(24.0)),
                    &ct.name,
                    16.0,
                    c,
                    true,
                    Al::Start,
                );
                let old = ui.rtl;
                ui.rtl = false;
                ui.label(
                    Rect::new(tx, r.y + ui.dp(36.0), tw, ui.dp(20.0)),
                    &ct.number,
                    13.0,
                    cs,
                    false,
                    if old { Al::End } else { Al::Start },
                );
                ui.rtl = old;
                if call {
                    let n = ct.number.clone();
                    self.phone.number = n.clone();
                    self.phone.tab = 0;
                    self.go(Screen::Phone);
                    self.phone.tel.dial(&n);
                    ui.dirty = true;
                    return;
                } else if ui.tap(r) {
                    self.fields.insert("cname", ct.name.clone());
                    self.fields.insert("cnum", ct.number.clone());
                    self.extra.del_armed = false;
                    self.go(Screen::ContactEdit(Some(i)));
                    ui.dirty = true;
                    return;
                }
            }
            ui.scroll_end(area, y + *sc - area.y + ui.dp(90.0), sc);
        }
        if self.fab(ui, area) {
            self.fields.insert("cname", String::new());
            self.fields.insert("cnum", String::new());
            self.go(Screen::ContactEdit(None));
            ui.dirty = true;
        }
    }

    pub(super) fn s_contact_edit(&mut self, ui: &mut Ui, area: Rect, idx: Option<usize>) {
        self.extra_load();
        let area = self.header(
            ui,
            area,
            self.t(if idx.is_some() {
                "Edit contact"
            } else {
                "New contact"
            }),
        );
        let mut y = area.y;
        let m = ui.dp(12.0);
        let name = self.f("cname");
        // Avatar mit Anfangsbuchstabe
        let hue = ui.th.accent;
        ui.cv
            .circle(ui.w / 2.0, y + ui.dp(40.0), ui.dp(38.0), hue.alpha(0.18));
        ui.label(
            Rect::new(0.0, y, ui.w, ui.dp(80.0)),
            &initial(&name),
            32.0,
            hue,
            true,
            Al::Center,
        );
        y += ui.dp(94.0);
        self.caption(ui, &mut y, self.t("Name"));
        ui.field(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            "cname",
            &name,
            self.t("Name"),
        );
        y += ui.dp(62.0);
        self.caption(ui, &mut y, self.t("Phone number"));
        ui.field(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
            "cnum",
            &self.f("cnum"),
            "+49 …",
        );
        y += ui.dp(70.0);
        if ui.button(
            Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
            self.t("Save"),
            Btn::Primary,
        ) {
            let (n, num) = (
                self.f("cname").trim().to_string(),
                self.f("cnum").trim().to_string(),
            );
            if n.is_empty() || num.is_empty() {
                self.toast(self.t("Invalid input").into());
            } else {
                let fav = idx
                    .and_then(|i| self.extra.contacts.list.get(i))
                    .is_some_and(|c| c.fav);
                if let Some(i) = idx.filter(|i| *i < self.extra.contacts.list.len()) {
                    self.extra.contacts.list.remove(i);
                }
                self.extra.contacts.list.push(Contact {
                    name: n,
                    number: num,
                    fav,
                });
                self.contacts_save();
                self.focus = None;
                self.back();
                self.toast(self.t("Saved").into());
            }
            ui.dirty = true;
            return;
        }
        y += ui.dp(62.0);
        if let Some(i) = idx {
            let label = if self.extra.del_armed {
                self.t("Tap again to delete")
            } else {
                self.t("Delete")
            };
            if ui.button(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(46.0)),
                label,
                Btn::Danger,
            ) {
                if self.extra.del_armed {
                    if i < self.extra.contacts.list.len() {
                        self.extra.contacts.list.remove(i);
                    }
                    self.contacts_save();
                    self.extra.del_armed = false;
                    self.focus = None;
                    self.back();
                } else {
                    self.extra.del_armed = true;
                }
                ui.dirty = true;
            }
        }
    }

    // ------------------------------------------------------------ Ubuntu-Programme

    pub(super) fn s_ubuntu_apps(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Ubuntu apps"));
        if self.extra.desk.is_none() {
            let mut v = desktop_apps("/compat/usr/share/applications");
            // Apps aus uapp (Windows-/Android-Programme, AppImages, Flatpak): /data/ultar/launchers
            v.extend(desktop_apps("/data/ultar/launchers"));
            v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.extra.desk = Some(v);
        }
        let list = self.extra.desk.clone().unwrap_or_default();
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let cs = ui.th.sub;
        y += ui.para(
            ui.dp(22.0),
            y,
            ui.w - ui.dp(44.0),
            self.t("Graphical programs from Ubuntu packages run full screen."),
            13.5,
            cs,
        ) + ui.dp(10.0);
        if !sys::compat_ready() {
            self.caption(ui, &mut y, self.t("Ubuntu area not available"));
        } else if list.is_empty() {
            self.caption(
                ui,
                &mut y,
                self.t("No graphical Ubuntu programs installed yet."),
            );
        }
        let session = std::path::Path::new("/compat/usr/bin/weston").exists();
        // Ultar-Desktop (Fenstersystem ultar-wm): alle Programme in Fenstern nebeneinander, mit Leiste und Startmenü
        let wm = [
            "/usr/lib/ultar/ultar-wm",
            "/compat/usr/bin/ultar-wm",
            "/compat/usr/local/bin/ultar-wm",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists());
        if wm && sys::compat_ready() {
            y += ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                self.t(
                    "Windows, taskbar and all Ubuntu programs side by side (keyboard and mouse).",
                ),
                13.0,
                cs,
            ) + ui.dp(6.0);
            if ui.button(
                Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
                self.t("Start desktop"),
                Btn::Primary,
            ) {
                if crate::sys::has_drm() {
                    self.launch = Some("desktop:".into());
                } else {
                    self.toast(
                        self.t("Desktop needs a display device (DRM or framebuffer).")
                            .into(),
                    );
                }
                ui.dirty = true;
            }
            y += ui.dp(62.0);
        }
        for a in &list {
            let sub = if a.comment.is_empty() {
                a.exec.clone()
            } else {
                a.comment.clone()
            };
            if ui.row(&mut y, Some(Ic::Apps), &a.name, Some(&sub), None, true) {
                if session {
                    self.launch = Some(format!("exec:{}", a.exec));
                } else {
                    self.toast(self.t("Install the browser first (window system).").into());
                }
                ui.dirty = true;
            }
        }
        y += ui.dp(6.0);
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Install more (packages)"),
            Btn::Plain,
        ) {
            self.store_tab = 1;
            self.extra.desk = None;
            self.go(Screen::Store);
            ui.dirty = true;
        }
        y += ui.dp(62.0);
        self.caption(ui, &mut y, self.t("Back to Ultar: press the power button."));
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_desktop;

    #[test]
    fn desktop_dateien() {
        let a = parse_desktop("[Desktop Entry]\nType=Application\nName=VLC\nExec=/usr/bin/vlc --started-from-file %U\nComment=Player\n[Desktop Action x]\nName=Other\nExec=other\n").unwrap();
        assert_eq!(
            (a.name.as_str(), a.exec.as_str(), a.comment.as_str()),
            ("VLC", "/usr/bin/vlc --started-from-file", "Player")
        );
        assert!(parse_desktop("[Desktop Entry]\nName=Hidden\nExec=x\nNoDisplay=true\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nName=Shell\nExec=x\nTerminal=true\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nName=NoExec\n").is_none());
    }
}
