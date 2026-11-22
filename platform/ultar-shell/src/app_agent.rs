//! Agent-App: Gespräch mit dem lokalen Sprachmodell (oder Claude), Vorschläge, Modell-Einrichtung und
//! die Werkzeuge, mit denen der Agent die Apps des Geräts bedient (Kamera, Wecker, Notizen, Kontakte, Funk …).
//! Untermodul von `app`.

use super::tools::Alarm;
use super::{App, Screen};
use crate::agent::{self, Call};
use crate::gfx::Rect;
use crate::sys;
use crate::ui::{Al, Btn, Ic, Ui};
use std::time::{Duration, Instant};

const SUGGEST: [&str; 5] = ["What is on my screen?", "Is my device secure?", "What is using my memory?", "Set an alarm for 7:00", "Turn on the flashlight"];

impl App {
    pub(super) fn s_agent(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let m = ui.dp(14.0);
        let (c, cs) = (ui.th.text, ui.th.sub);
        // Kopf: Titel, Zustand, Einstellungen
        let hh = ui.dp(64.0);
        ui.label(Rect::new(ui.dp(22.0), area.y + ui.dp(4.0), ui.w - ui.dp(100.0), ui.dp(36.0)), self.t("Agent"), 26.0, c, true, Al::Start);
        let claude = self.agent.backend == "claude";
        let ready = self.agent.enabled && self.agent_up == Some(true);
        let (stxt, scol) = if !self.agent.enabled {
            (self.t("Agent is disabled").to_string(), cs)
        } else if ready {
            (if claude { format!("Claude · {}", self.t("online")) } else { self.t("On the device · ready").to_string() }, ui.th.ok)
        } else if self.agent_up.is_none() {
            (self.t("Checking…").to_string(), cs)
        } else {
            (self.t("Language model not reachable").to_string(), ui.th.danger)
        };
        let dot_x = if ui.rtl { ui.w - ui.dp(27.0) } else { ui.dp(27.0) };
        ui.cv.circle(dot_x, area.y + ui.dp(50.0), ui.dp(4.0), scol);
        let sr = if ui.rtl { Rect::new(ui.dp(70.0), area.y + ui.dp(40.0), ui.w - ui.dp(108.0), ui.dp(20.0)) } else { Rect::new(ui.dp(38.0), area.y + ui.dp(40.0), ui.w - ui.dp(108.0), ui.dp(20.0)) };
        ui.label(sr, &stxt, 12.5, cs, false, Al::Start);
        let gx = if ui.rtl { ui.dp(38.0) } else { ui.w - ui.dp(38.0) };
        if ui.icon_btn(gx, area.y + hh / 2.0, ui.dp(20.0), Ic::Gear, ui.th.card, c) {
            self.go(Screen::AgentSet);
            ui.dirty = true;
            return;
        }
        if self.chat.len() > 0 {
            let nx = if ui.rtl { ui.dp(86.0) } else { ui.w - ui.dp(86.0) };
            if ui.icon_btn(nx, area.y + hh / 2.0, ui.dp(20.0), Ic::Plus, ui.th.card, c) && !self.agent_busy {
                self.chat.clear();
                self.history.clear();
                *sc = 0.0;
                ui.dirty = true;
            }
        }

        let fh = ui.dp(52.0);
        let fy = area.bottom() - fh - ui.dp(8.0);
        let body = Rect::new(m, area.y + hh + ui.dp(4.0), ui.w - 2.0 * m, (fy - area.y - hh - ui.dp(12.0)).max(0.0));

        if !self.agent.enabled || (!ready && self.chat.is_empty() && self.agent_up == Some(false)) {
            // Einrichtungshinweis statt leerem Verlauf
            let r = Rect::new(body.x, body.y + ui.dp(12.0), body.w, ui.dp(250.0).min(body.h));
            ui.card(r);
            ui.cv.circle(r.cx(), r.y + ui.dp(52.0), ui.dp(34.0), ui.th.accent.alpha(0.14));
            let a = ui.th.accent;
            ui.icon(Ic::Spark, r.cx(), r.y + ui.dp(52.0), ui.dp(36.0), a);
            ui.label(Rect::new(r.x, r.y + ui.dp(98.0), r.w, ui.dp(26.0)), self.t("Your assistant on the device"), 17.0, c, true, Al::Center);
            let msg = if !self.agent.enabled {
                self.t("It operates the phone for you – without cloud. Switch it on and install a language model.")
            } else if claude {
                self.t("Enter your Anthropic API key to use Claude.")
            } else if !agent::model_installed() {
                self.t("No language model installed yet.")
            } else {
                self.t("The language model is starting or not reachable.")
            };
            let h = ui.para(r.x + ui.dp(20.0), r.y + ui.dp(128.0), r.w - ui.dp(40.0), msg, 13.5, cs);
            let by = (r.y + ui.dp(140.0) + h).min(r.bottom() - ui.dp(62.0));
            if ui.button(Rect::new(r.x + ui.dp(20.0), by, r.w - ui.dp(40.0), ui.dp(48.0)), self.t("Set up"), Btn::Primary) {
                self.go(Screen::AgentSet);
                ui.dirty = true;
            }
            return;
        }

        // ---------- Verlauf (von unten nach oben, mit Rollen) ----------
        let mut items: Vec<(u8, String)> = self.chat.clone();
        if self.agent_busy && self.pending.is_none() {
            items.push((4, self.t("Thinking…").into()));
        }
        if items.is_empty() {
            // Begrüßung + Vorschläge
            let cy = body.y + body.h * 0.2;
            ui.cv.circle(body.cx(), cy, ui.dp(38.0), ui.th.accent.alpha(0.13));
            let a = ui.th.accent;
            ui.icon(Ic::Spark, body.cx(), cy, ui.dp(40.0), a);
            ui.label(Rect::new(body.x, cy + ui.dp(52.0), body.w, ui.dp(30.0)), self.t("How can I help?"), 20.0, c, true, Al::Center);
            ui.label(Rect::new(body.x, cy + ui.dp(82.0), body.w, ui.dp(22.0)), self.t("I can operate apps, settings and Ubuntu packages."), 13.0, cs, false, Al::Center);
            let mut y = cy + ui.dp(124.0);
            let mut pick = None;
            for s in SUGGEST {
                let label = self.t(s);
                let w = ui.text_w(label, 13.5, false) + ui.dp(26.0);
                if y + ui.dp(40.0) > body.bottom() {
                    break;
                }
                let (_, tapped) = ui.pill(body.cx() - w / 2.0, y, label, false);
                if tapped {
                    pick = Some(label.to_string());
                }
                y += ui.dp(44.0);
            }
            if let Some(p) = pick {
                self.fields.insert("chat", p);
                self.send_chat();
                ui.dirty = true;
            }
        } else {
            ui.set_clip(Some(body));
            if ui.enabled && ui.p.down && ui.p.moved && ui.p.capture.is_none() && body.contains(ui.p.px, ui.p.py) {
                *sc += ui.p.dy;
                ui.p.dy = 0.0;
            }
            let maxw = (body.w * 0.82).min(ui.dp(520.0));
            let pad = ui.dp(12.0);
            let mut by = body.bottom() + sc.max(0.0);
            let mut total = 0.0;
            for (kind, text) in items.iter().rev() {
                let (sz, bold) = match kind {
                    2 | 4 => (12.0, false),
                    _ => (15.0, false),
                };
                let szp = ui.dp(sz);
                if matches!(kind, 2 | 4) {
                    // Werkzeugschritt / Denken: kleine mittige Zeile
                    let s = ui.fit(text, sz, false, body.w - ui.dp(60.0));
                    let w = ui.text_w(&s, sz, false) + ui.dp(40.0);
                    let h = ui.dp(28.0);
                    by -= h + ui.dp(6.0);
                    total += h + ui.dp(6.0);
                    let r = Rect::new(body.cx() - w / 2.0, by, w, h);
                    if r.bottom() > body.y && r.y < body.bottom() {
                        ui.cv.rrect(r, h / 2.0, ui.th.card);
                        let ic = if *kind == 4 { Ic::Spark } else { Ic::Chip };
                        let a = ui.th.accent;
                        ui.icon(ic, r.x + ui.dp(16.0), r.cy(), ui.dp(13.0), a);
                        ui.label(Rect::new(r.x + ui.dp(28.0), r.y, r.w - ui.dp(36.0), h), &s, sz, cs, false, Al::Center);
                    }
                    continue;
                }
                let (tw, th) = ui.tx.measure(text, szp, bold, maxw - 2.0 * pad);
                let (bw, bh) = (tw.min(maxw - 2.0 * pad) + 2.0 * pad, th.max(szp * 1.35) + 2.0 * pad * 0.8);
                by -= bh + ui.dp(8.0);
                total += bh + ui.dp(8.0);
                let mine = *kind == 0;
                let right = mine != ui.rtl;
                let r = Rect::new(if right { body.right() - bw } else { body.x }, by, bw, bh);
                if r.bottom() < body.y || r.y > body.bottom() {
                    continue;
                }
                let (bgc, fg) = match kind {
                    0 => (ui.th.accent, ui.th.on_accent),
                    1 => (ui.th.card, c),
                    _ => (ui.th.danger.alpha(0.16), ui.th.danger),
                };
                ui.cv.rrect(r, ui.dp(18.0), bgc);
                // „Schwänzchen": Ecke zur eigenen Seite hin weniger rund
                let q = ui.dp(10.0);
                ui.cv.rect(Rect::new(if right { r.right() - q } else { r.x }, r.bottom() - q, q, q), bgc);
                // Umbruchbreite = gemessene Breite: so sitzt auch rechtsbündiger (arabischer) Text genau in der Blase
                let ww = if th > szp * 1.9 { maxw - 2.0 * pad } else { 0.0 };
                let tx0 = if ww > 0.0 { r.right() - pad - (maxw - 2.0 * pad) } else { r.x + pad };
                let rtl_text = text.chars().any(|ch| ('\u{0590}'..='\u{08FF}').contains(&ch));
                ui.tx.draw(ui.cv, text, if ww > 0.0 && rtl_text { tx0 } else { r.x + pad }, r.y + pad * 0.8, szp, fg, bold, ww);
            }
            // Rollen begrenzen: nicht weiter als der Anfang des Gesprächs
            *sc = sc.clamp(0.0, (total - body.h + ui.dp(8.0)).max(0.0));
            ui.set_clip(None);
        }

        // ---------- Eingabe: + (mitbringen) · Feld · Senden ----------
        self.jarvis_attach_ui(ui, fy - ui.dp(4.0));
        let bw = fh;
        let fw = ui.w - 2.0 * m - 2.0 * bw - ui.dp(16.0);
        let (ax, fx, bx) = if ui.rtl { (ui.w - m - bw / 2.0, m + bw + ui.dp(8.0), m + bw / 2.0) } else { (m + bw / 2.0, m + bw + ui.dp(8.0), m + bw + fw + ui.dp(16.0) + bw / 2.0) };
        self.jarvis_button(ui, ax, fy + fh / 2.0, fh / 2.0 - ui.dp(4.0));
        ui.field(Rect::new(fx, fy, fw, fh), "chat", &self.f("chat"), self.t("Ask the agent…"));
        let can = !self.agent_busy && (!self.f("chat").trim().is_empty() || !self.jv.attach.is_empty());
        let (bgc, fgc) = if can { (ui.th.accent, ui.th.on_accent) } else { (ui.th.card, cs) };
        if ui.icon_btn(bx, fy + fh / 2.0, fh / 2.0, Ic::Send, bgc, fgc) && can {
            *sc = 0.0;
            self.send_chat();
            ui.dirty = true;
        }
    }

    /// Werkzeuge des Agenten, die die Apps bedienen. None = hier nicht bekannt.
    pub(super) fn exec_apps(&mut self, c: &Call) -> Option<String> {
        // Dokumente, Tabellen, PDF, Textdateien (app_office.rs)
        if let Some(r) = self.exec_office(c) {
            return Some(r);
        }
        let res = match c.tool.as_str() {
            "help" => {
                let topic = c.arg("topic");
                let topic = topic.trim();
                if topic.is_empty() {
                    crate::help::digest()
                } else if let Some(b) = crate::help::body(topic, self.lang()) {
                    b.to_string()
                } else {
                    format!("error: unknown topic. Known topics:\n{}", crate::help::digest())
                }
            }
            "open_app" => {
                let scr = match c.arg("name").as_str() {
                    "home" => None,
                    "camera" => Some(Screen::Camera),
                    "gallery" | "photos" => Some(Screen::Gallery),
                    "phone" => Some(Screen::Phone),
                    "messages" => Some(Screen::Messages),
                    "contacts" => Some(Screen::Contacts),
                    "notes" => Some(Screen::Notes),
                    "clock" => Some(Screen::Clock),
                    "calculator" => Some(Screen::Calc),
                    "files" => Some(Screen::Files),
                    "browser" => Some(Screen::Web("about:blank".into())),
                    "store" | "packages" => Some(Screen::Store),
                    "terminal" => Some(Screen::Terminal),
                    "monitor" => Some(Screen::Monitor),
                    "privacy" => Some(Screen::Privacy),
                    "settings" => Some(Screen::Settings),
                    "network" => Some(Screen::Net),
                    "display" => Some(Screen::Display),
                    "language" => Some(Screen::Lang),
                    "time" => Some(Screen::Time),
                    "storage" => Some(Screen::Storage),
                    "services" => Some(Screen::Services),
                    "system" => Some(Screen::System),
                    "hardware" => Some(Screen::Hardware),
                    "ubuntu_apps" => Some(Screen::UbuntuApps),
                    "pdf" | "pdf_reader" => Some(Screen::Office(super::app_office::Ox::List(0))),
                    "documents" | "word" => Some(Screen::Office(super::app_office::Ox::List(1))),
                    "sheets" | "excel" => Some(Screen::Office(super::app_office::Ox::List(2))),
                    "code_editor" | "editor" => Some(Screen::Office(super::app_office::Ox::List(3))),
                    _ => return Some("error: unknown app".into()),
                };
                match scr {
                    Some(s) => self.go(s),
                    None => self.home(),
                }
                "ok".into()
            }
            "open_url" => {
                let u = c.arg("url");
                if u.starts_with("https://") || u.starts_with("http://") {
                    self.go(Screen::Web(u));
                    "ok: browser page prepared, the user taps Open".into()
                } else {
                    "error: url must start with https://".into()
                }
            }
            "take_photo" => {
                if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Camera) {
                    "error: camera is switched off in privacy settings".into()
                } else {
                    self.cam.mode = 0;
                    self.go(Screen::Camera);
                    self.camera_auto_shot(3);
                    "ok: camera opened, photo is taken in 3 seconds and saved to the gallery".into()
                }
            }
            "set_alarm" => {
                let (h, m) = (c.arg("hour").trim().parse::<u8>(), c.arg("minute").trim().parse::<u8>().or_else(|e| if c.arg("minute").is_empty() { Ok(0) } else { Err(e) }));
                match (h, m) {
                    (Ok(h), Ok(m)) if h < 24 && m < 60 => {
                        self.tl.alarms.list.retain(|a| !(a.h == h && a.m == m));
                        self.tl.alarms.list.push(Alarm { h, m, on: true, days: String::new() });
                        let _ = sys::save("alarms.toml", &self.tl.alarms);
                        format!("ok: alarm set for {h:02}:{m:02}")
                    }
                    _ => "error: hour 0-23, minute 0-59".into(),
                }
            }
            "start_timer" => match c.arg("minutes").trim().parse::<f32>() {
                Ok(m) if m > 0.0 && m <= 600.0 => {
                    self.tl.timer_end = Some(Instant::now() + Duration::from_secs_f32(m * 60.0));
                    format!("ok: timer started, {m} minutes")
                }
                _ => "error: minutes 1-600".into(),
            },
            "toggle" => {
                let on = matches!(c.arg("on").as_str(), "true" | "1" | "on");
                let name = c.arg("name");
                match name.as_str() {
                    "wifi" => crate::quick::set_wifi(on),
                    "bluetooth" => crate::quick::set_bt(on),
                    "mobile_data" => crate::quick::set_mobile(on),
                    "airplane" => crate::quick::set_airplane(on),
                    "flashlight" | "torch" => crate::quick::set_torch(on),
                    "vpn" => {
                        let r = if on { { crate::sys::memo_clear(); ultar_vpn::connect_saved() } } else { { crate::sys::memo_clear(); ultar_vpn::disconnect() } };
                        return Some(match r {
                            Ok(_) => format!("ok: vpn {}", if on { "on" } else { "off" }),
                            Err(e) => format!("error: {e}"),
                        });
                    }
                    _ => return Some("error: unknown switch".into()),
                }
                format!("ok: {name} {}", if on { "on" } else { "off" })
            }
            "add_note" => {
                let t = c.arg("text");
                if t.trim().is_empty() {
                    "error: empty note".into()
                } else {
                    self.note_add(t.trim());
                    "ok: note saved".into()
                }
            }
            "list_notes" => {
                self.extra_load();
                let v: Vec<String> = self.extra.notes.list.iter().take(12).map(|n| n.text.lines().next().unwrap_or("").chars().take(80).collect()).collect();
                if v.is_empty() { "no notes".into() } else { v.join("\n") }
            }
            "add_contact" => {
                let (n, num) = (c.arg("name"), c.arg("number"));
                if n.trim().is_empty() || num.trim().is_empty() {
                    "error: name and number needed".into()
                } else {
                    self.contact_add(n.trim(), num.trim());
                    "ok: contact saved".into()
                }
            }
            "find_contact" => {
                self.extra_load();
                let q = c.arg("name").to_lowercase();
                let v: Vec<String> = self.extra.contacts.list.iter().filter(|x| x.name.to_lowercase().contains(&q)).take(8).map(|x| format!("{}: {}", x.name, x.number)).collect();
                if v.is_empty() { "no contact found".into() } else { v.join("\n") }
            }
            "call" => {
                let n: String = c.arg("number").chars().filter(|ch| ch.is_ascii_digit() || "+*#".contains(*ch)).collect();
                if n.is_empty() {
                    "error: number missing".into()
                } else {
                    self.phone.number = n.clone();
                    self.phone.tab = 0;
                    self.go(Screen::Phone);
                    self.phone.tel.dial(&n);
                    format!("ok: calling {n}")
                }
            }
            "list_files" => {
                let p = c.arg("path");
                let p = if p.is_empty() { "/data".to_string() } else { p };
                if !agent_path_ok(&p) {
                    "error: only folders under /data are allowed".into()
                } else {
                    match std::fs::read_dir(&p) {
                        Ok(rd) => {
                            let mut v: Vec<String> = rd.flatten().take(60).map(|e| format!("{}{}", e.file_name().to_string_lossy(), if e.path().is_dir() { "/" } else { "" })).collect();
                            v.sort();
                            if v.is_empty() { "(empty)".into() } else { v.join("\n") }
                        }
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "read_file" => {
                let p = c.arg("path");
                if !agent_path_ok(&p) {
                    "error: only files under /data are allowed".into()
                } else {
                    match std::fs::read(&p) {
                        Ok(b) if b.iter().take(2048).any(|&x| x == 0) => "error: not a text file".into(),
                        Ok(b) => String::from_utf8_lossy(&b).chars().take(1400).collect(),
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "notify" => {
                ultar_notify::post("app", "agent", &c.arg("title"), &c.arg("text"), ultar_notify::Urgency::Normal);
                "ok".into()
            }
            _ => return None,
        };
        Some(res)
    }
}

/// Der Agent darf nur im Nutzerbereich lesen – nicht in /data/ultar (Schlüssel, WLAN-Kennwörter, Konten).
pub(super) fn agent_path_ok(p: &str) -> bool {
    let p = p.trim_end_matches('/');
    (p == "/data" || p.starts_with("/data/")) && !p.contains("..") && p != "/data/ultar" && !p.starts_with("/data/ultar/")
}

#[cfg(test)]
mod tests {
    use super::agent_path_ok;

    #[test]
    fn agent_liest_keine_geheimnisse() {
        assert!(agent_path_ok("/data"));
        assert!(agent_path_ok("/data/Pictures/a.txt"));
        assert!(!agent_path_ok("/data/ultar/wifi.conf"));
        assert!(!agent_path_ok("/data/ultar"));
        assert!(!agent_path_ok("/data/../etc/shadow"));
        assert!(!agent_path_ok("/etc/passwd"));
    }
}
