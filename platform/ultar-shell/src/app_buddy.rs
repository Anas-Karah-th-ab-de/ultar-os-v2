//! Der Helfer: eine kleine Figur, die über jeder Seite schwebt.
//!
//! - antippen: Feld öffnet sich (Links der aktuellen Seite prüfen, etwas fragen)
//! - ziehen: an eine andere Stelle setzen
//! - „Wegschicken": verschwindet bis auf eine schmale Lasche am Rand; Lasche antippen ruft ihn zurück
//!
//! Er lebt: gleitet und hüpft an seinen Platz, macht ab und zu einen kleinen Ausflug über den Bildschirm,
//! begrüßt nach längerer Pause und **ruft an**, wenn eine Erinnerung oder ein Termin fällig ist
//! (kommt in die Mitte, wird groß, klingelt, vibriert; „Erledigt" oder „In 10 Min").
//!
//! Link-Prüfung läuft in einer abgeschotteten Box (`linkcheck::probe`). Freie Fragen gehen an den
//! Agenten, wenn er eingerichtet ist.

use super::App;
use crate::gfx::{rgb, Rect};
use crate::linkcheck::{self, Report, Verdict};
use crate::mind::{self, Intent};
use super::Screen;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use crate::ui::{Al, Ic, Ui};
use std::sync::{Arc, Mutex};

const STATE: &str = "/data/ultar/buddy";
const REMINDERS: &str = "/data/ultar/reminders.tsv";

/// Er ruft an: Text, seit wann, letztes Klingeln, wie oft schon unbeantwortet
struct Calling {
    text: String,
    since: Instant,
    rung: Instant,
    tries: u8,
    /// nur eine Mitteilung (Antwort auf einen Sprachbefehl): kein Klingeln, verschwindet von selbst
    quiet: bool,
}

#[derive(Default)]
pub struct Buddy {
    /// Bühne auf der Helfer-Seite: (Mitte x, Mitte y, Radius) – er steht dann groß in der Seite
    pub stage: Option<(f32, f32, f32)>,
    /// Vorschau-Zustand auf der Helfer-Seite: 1 Hallo, 2 Hört zu, 3 Denkt, 4 Spricht, 5 Ruft an, 6 Fremde Stimme, 7 Schläft
    pub preview: u8,
    loaded: bool,
    pub away: bool,
    pos: Option<(f32, f32)>,
    pub open: bool,
    drag: bool,
    moved: bool,
    /// Flächen des letzten Bilds, die Berührungen für sich beanspruchen
    hit: Vec<Rect>,
    links: Vec<String>,
    slot: Arc<Mutex<Option<Report>>>,
    report: Option<Report>,
    checking: Option<String>,
    note: Option<&'static str>,
    asked: Option<usize>,
    /// Fernbedienung (Tür): ersten Link der Seite prüfen
    pub remote_check: bool,
    /// Fernbedienung (Tür): diesen Text ausführen, als wäre er eingetippt
    pub remote_say: Option<String>,
    /// Das nächste Werkzeug-Ergebnis gehört dem Helfer (Schicht 1, ohne Sprachmodell)
    pub native: bool,
    pub answer: Option<String>,
    /// Wächter: Schnellurteil je schon gesehener Adresse
    pub(super) guard: HashMap<String, Verdict>,
    /// Hinweisblase neben der Figur: (Adresse, Urteil, seit wann)
    alert: Option<(String, Verdict, Instant)>,
    // ---- Bewegung ----
    /// gezeichnete Stelle; gleitet der Zielstelle hinterher
    cur: Option<(f32, f32)>,
    frame: Option<Instant>,
    /// 0..1: wie weit er zum Anrufen gewachsen ist
    grow: f32,
    /// Schrittphase beim Laufen und zuletzt gemessene Geschwindigkeit (Punkte/s)
    phase: f32,
    vx: f32,
    moving: bool,
    hop: Option<Instant>,
    /// Ausflug: Ziel als Bruchteil der freien Fläche, bis wann
    roam: Option<(f32, f32, Instant)>,
    stroll_at: Option<Instant>,
    /// kurze Sprechblase
    say: Option<(String, Instant)>,
    slept: Option<Instant>,
    /// Bildschirmfoto/Fernbedienung: ohne Gleiten sofort am Ziel
    pub snap: bool,
    // ---- Erinnerungen ----
    rem_loaded: bool,
    /// (fällig, unbeantwortete Versuche, Text), nach Zeit sortiert
    reminders: Vec<(i64, u8, String)>,
    calling: Option<Calling>,
    // ---- Vorstellung und Führung ----
    introduced: bool,
    pub tour: Option<usize>,
    /// Hilfethema der letzten Antwort (Knopf „Hilfeseite öffnen")
    help_id: Option<&'static str>,
    /// das nächste Werkzeug-Ergebnis als Mitteilung zeigen (Befehl kam per Stimme)
    pub voice_reply: bool,
    /// gerade wird ein Sprachmuster eingelernt (Karte in der Mitte)
    pub learning: bool,
}

/// Stationen der Führung: Text und Stelle (Bruchteil der freien Fläche), an die er dafür geht.
const TOUR: [(&str, f32, f32); 8] = [
    ("I am KM Code Craft, your personal assistant. I work on your device, keep your data here and keep it safe.", 0.5, 0.3),
    ("I stay beside you on every page. Tap me to talk to me, drag me wherever you like, or send me away – the small tab at the edge calls me back.", 0.85, 0.42),
    ("Up here: pull down from the top edge for quick settings – Wi-Fi, flashlight, brightness and more.", 0.5, 0.0),
    ("Down here: back, home and recent apps.", 0.5, 1.0),
    ("Just tell me what to do: flashlight on, scan document, open camera, set brightness to 40 – I do it at once, without internet.", 0.2, 0.45),
    ("Say: remind me at 17:30 of the dentist. When it is time I come to the middle of the screen and call you – even when the screen is off.", 0.8, 0.3),
    ("I watch the links on every page and warn you. Paste a link and I test it in a sealed box before you open it.", 0.25, 0.3),
    ("Every part of the system has a help page, and I know them: ask me \"how does the scanner work?\". Say \"tour\" to see this again.", 0.5, 0.36),
];

impl Buddy {
    /// Gehört die laufende Berührung dem Helfer? Dann darf die Seite darunter nicht reagieren.
    pub fn blocks(&self, p: &crate::ui::Ptr) -> bool {
        self.drag || self.calling.is_some() || self.tour.is_some() || self.learning || self.hit.iter().any(|r| r.contains(p.x, p.y) || ((p.down || p.released) && r.contains(p.px, p.py)))
    }
}

impl Buddy {
    pub fn is_calling(&self) -> bool {
        self.calling.is_some()
    }

    /// Bewegt sich gerade etwas an ihm? Dann zeichnet die Hauptschleife zügig weiter.
    pub fn animating(&self) -> bool {
        self.calling.is_some() || self.tour.is_some() || self.learning || self.moving || self.roam.is_some() || self.hop.is_some_and(|t| t.elapsed() < Duration::from_millis(520)) || self.say.is_some() || self.alert.is_some()
    }

    pub fn on_sleep(&mut self) {
        self.slept.get_or_insert_with(Instant::now);
        self.roam = None;
        self.say = None;
    }

    fn rem_save(&mut self) {
        self.reminders.sort_by_key(|r| r.0);
        let s: String = self.reminders.iter().map(|(t, n, x)| format!("{t}\t{n}\t{}\n", x.replace(['\t', '\n'], " "))).collect();
        let _ = std::fs::write(REMINDERS, s);
    }

    fn rem_load(&mut self) {
        self.rem_loaded = true;
        self.reminders = std::fs::read_to_string(REMINDERS)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?.to_string()))
            })
            .collect();
        self.reminders.sort_by_key(|r| r.0);
    }
}

/// billiger Zufall für Ausflüge (kein Kryptobedarf)
fn dice() -> u64 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    (n ^ (n >> 17)).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 20
}

impl App {
    /// Er ruft an: kommt in die Mitte, klingelt und zeigt den Text, bis geantwortet wird.
    pub(super) fn buddy_call(&mut self, text: String, tries: u8) {
        let b = &mut self.buddy;
        b.away = false;
        b.open = false;
        b.roam = None;
        b.say = None;
        b.alert = None;
        let now = Instant::now();
        b.calling = Some(Calling { text, since: now, rung: now, tries, quiet: false });
        self.shade = false;
        if self.focus == Some("buddy") {
            self.focus = None;
            self.focus_reset = true;
        }
        self.buddy_ring();
    }

    /// Kurze Mitteilung in der Mitte (auch über dem Sperrbildschirm), ohne Klingeln – z. B. Antwort auf Zuruf.
    pub(super) fn buddy_show(&mut self, text: String) {
        if self.buddy.calling.as_ref().is_some_and(|c| !c.quiet) {
            return;
        }
        let b = &mut self.buddy;
        b.away = false;
        b.open = false;
        b.roam = None;
        b.say = None;
        let now = Instant::now();
        b.calling = Some(Calling { text, since: now, rung: now, tries: 0, quiet: true });
        b.hop = Some(now);
    }

    fn buddy_ring(&self) {
        let (snd, vib, _) = self.sys_alert(true, self.set.sound, 350);
        if snd {
            crate::audio::play(vec![(784.0, 110), (0.0, 30), (1047.0, 110), (0.0, 30), (1319.0, 110), (0.0, 30), (1568.0, 220)], self.set.volume);
        }
        if vib > 0 {
            crate::sys::vibrate(vib);
        }
    }

    fn buddy_hop(&mut self) {
        self.buddy.hop = Some(Instant::now());
    }

    fn rem_line(&self, t: i64, text: &str) -> String {
        let (d, n) = (crate::sys::from_unix(t, self.set.tz_min), crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min));
        let clock = self.clock_str(d.h, d.mi);
        if (d.y, d.mo, d.d) == (n.y, n.mo, n.d) { format!("{clock}  {text}") } else { format!("{:02}.{:02}. {clock}  {text}", d.d, d.mo) }
    }

    /// Uhrwerk des Helfers: fällige Erinnerungen, Klingeln, Ausflüge, Begrüßung. true = es gibt etwas zu zeigen
    /// (bei ausgeschaltetem Bildschirm: aufwecken).
    pub(super) fn buddy_tick(&mut self, asleep: bool) -> bool {
        if !self.set.setup_done {
            return false;
        }
        if !self.buddy.rem_loaded {
            self.buddy.rem_load();
        }
        self.buddy_load();
        let now = crate::sys::unix_now();
        let mut started = false;
        // während eines Telefonats nicht dazwischenrufen – danach sofort
        if self.buddy.calling.is_none() && !self.call_active() {
            if let Some(i) = self.buddy.reminders.iter().position(|r| r.0 <= now) {
                let (_, tries, text) = self.buddy.reminders.remove(i);
                self.buddy.rem_save();
                self.buddy_call(text, tries);
                started = true;
            }
        }
        if self.buddy.calling.as_ref().is_some_and(|c| c.quiet && c.since.elapsed() > Duration::from_secs(7)) {
            self.buddy.calling = None;
            return true;
        }
        if let Some(c) = self.buddy.calling.as_mut().filter(|c| !c.quiet) {
            if c.since.elapsed() > Duration::from_secs(45) {
                // niemand reagiert: zweimal nach je 5 Minuten wiederholen, dann als verpasst melden
                let (text, tries) = (c.text.clone(), c.tries);
                self.buddy.calling = None;
                if tries < 2 {
                    self.buddy.reminders.push((now + 300, tries + 1, text));
                    self.buddy.rem_save();
                } else {
                    ultar_notify::post("system", "helper", &self.t_owned("Missed reminder"), &text, ultar_notify::Urgency::Normal);
                }
                return true;
            } else if c.rung.elapsed() >= Duration::from_millis(3200) {
                c.rung = Instant::now();
                self.buddy_ring();
            }
        }
        if asleep {
            return started;
        }
        let b = &mut self.buddy;
        if b.roam.is_some_and(|r| Instant::now() >= r.2) {
            b.roam = None;
        }
        if b.say.as_ref().is_some_and(|s| s.1.elapsed() > Duration::from_secs(6)) {
            b.say = None;
        }
        let idle = !b.away && !b.open && b.calling.is_none() && b.tour.is_none() && !b.drag && self.focus.is_none() && !self.sx.locked && !self.shade;
        // Begrüßung nach längerer Pause (mit der nächsten Erinnerung von heute)
        if let Some(t) = self.buddy.slept {
            if !self.sx.locked {
                self.buddy.slept = None;
                if idle && t.elapsed() > Duration::from_secs(30 * 60) {
                    let h = crate::sys::from_unix(now, self.set.tz_min).h;
                    let mut msg = self.t(if h < 11 { "Good morning!" } else if h < 18 { "Hello again!" } else { "Good evening!" }).to_string();
                    if let Some((t, _, x)) = self.buddy.reminders.first().filter(|r| r.0 - now < 86400).cloned() {
                        msg = format!("{msg}  {}", self.rem_line(t, &x));
                    }
                    self.buddy.say = Some((msg, Instant::now()));
                    self.buddy_hop();
                }
            }
        }
        // erster Start: sich vorstellen und durch das System führen
        if !self.buddy.introduced && !self.buddy.snap && self.buddy.tour.is_none() && self.buddy.calling.is_none() && !self.sx.locked && !self.shade && matches!(self.stack.last().map(|s| &s.0), Some(Screen::Home)) {
            self.buddy_tour();
            return true;
        }
        // ab und zu ein kleiner Ausflug – nicht über Kamera, Bildern oder beim Tippen
        let calm = idle && !self.ptr_down && !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Camera | Screen::Photo(_)));
        let b = &mut self.buddy;
        match b.stroll_at {
            None => b.stroll_at = Some(Instant::now() + Duration::from_secs(40 + dice() % 50)),
            Some(t) if Instant::now() >= t => {
                b.stroll_at = Some(Instant::now() + Duration::from_secs(50 + dice() % 80));
                if calm {
                    let d = dice();
                    if d % 4 == 0 {
                        b.hop = Some(Instant::now());
                    } else {
                        b.roam = Some((0.15 + (d % 70) as f32 / 100.0, 0.12 + ((d >> 8) % 60) as f32 / 100.0, Instant::now() + Duration::from_millis(3200 + d % 1800)));
                    }
                }
            }
            _ => {}
        }
        started || self.buddy.animating()
    }

    fn buddy_load(&mut self) {
        if !self.buddy.loaded {
            self.buddy.loaded = true;
            if let Ok(s) = std::fs::read_to_string(STATE) {
                let v: Vec<f32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 3 {
                    self.buddy.introduced = v.get(3).is_some_and(|x| *x > 0.5);
                    self.buddy.away = v[0] > 0.5;
                    if v[1] >= 0.0 {
                        self.buddy.pos = Some((v[1], v[2]));
                    }
                }
            }
        }
    }

    /// Vorstellung und Führung beginnen (beim ersten Start von selbst, sonst auf Zuruf).
    pub(super) fn buddy_tour(&mut self) {
        let b = &mut self.buddy;
        b.away = false;
        b.open = false;
        b.roam = None;
        b.say = None;
        b.tour = Some(0);
        b.introduced = true;
        if self.focus == Some("buddy") {
            self.focus = None;
            self.focus_reset = true;
        }
        self.buddy_save();
    }

    /// Passendes Hilfethema zu einer Frage suchen (Titel zählt mehr als Text).
    fn buddy_help(&self, text: &str, asked: bool) -> Option<&'static crate::help::Topic> {
        const STOP: [&str; 30] = [
            "wie", "was", "ist", "der", "die", "das", "ein", "eine", "funktioniert", "geht", "hilfe", "help", "how", "does", "the", "work", "works", "what", "about", "mit", "dem", "den", "und", "ich", "kann", "can", "كيف", "ما", "هو",
            "يعمل",
        ];
        let li = crate::help::li_pub(self.lang());
        let q = text.to_lowercase();
        let words: Vec<&str> = q.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3 && !STOP.contains(w)).collect();
        if words.is_empty() {
            return None;
        }
        let mut best: Option<(u32, &'static crate::help::Topic)> = None;
        for t in crate::help::TOPICS {
            let (title, en, body) = (self.t(t.title).to_lowercase(), t.title.to_lowercase(), t.body[li].to_lowercase());
            let mut sc = 0;
            for w in &words {
                if title.contains(w) || en.contains(w) || t.id == *w {
                    sc += 4;
                } else if body.contains(w) {
                    sc += if w.chars().count() >= 5 { 2 } else { 1 };
                }
            }
            if sc >= if asked { 2 } else { 4 } && best.is_none_or(|b| sc > b.0) {
                best = Some((sc, t));
            }
        }
        best.map(|b| b.1)
    }

    fn buddy_show_help(&mut self, t: &'static crate::help::Topic) {
        let body = t.body[crate::help::li_pub(self.lang())];
        let short: String = body.chars().take(360).collect();
        self.buddy.answer = Some(format!("{}\n{}{}", self.t(t.title), short, if body.chars().count() > 360 { "…" } else { "" }));
        self.buddy.help_id = Some(t.id);
    }

    fn buddy_save(&self) {
        let (x, y) = self.buddy.pos.unwrap_or((-1.0, -1.0));
        let _ = std::fs::write(STATE, format!("{} {x:.0} {y:.0} {}\n", self.buddy.away as u8, self.buddy.introduced as u8));
    }

    fn buddy_check(&mut self, url: String) {
        self.buddy.report = None;
        self.buddy.note = None;
        self.buddy.asked = None;
        self.buddy.checking = Some(url.clone());
        let (slot, bg) = (self.buddy.slot.clone(), self.bg.clone());
        std::thread::spawn(move || {
            let full = if url.contains("://") { url.clone() } else { format!("http://{url}") };
            let trace = linkcheck::probe(&full);
            let rep = linkcheck::judge(&full, Some(&trace));
            if let Ok(mut s) = slot.lock() {
                *s = Some(rep);
            }
            bg.poke();
        });
    }

    /// Ergebnis eines Werkzeugs, das der Helfer selbst ausgelöst hat (kommt aus `App::tool_result`).
    pub(super) fn buddy_tool_result(&mut self, res: String) {
        let res = res.trim();
        let txt = if let Some(rest) = res.strip_prefix("ok") {
            let rest = rest.trim_start_matches([':', ' ']);
            if rest.is_empty() { self.t("Done").to_string() } else { format!("{}: {}", self.t("Done"), rest) }
        } else if let Some(rest) = res.strip_prefix("error:") {
            format!("{}: {}", self.t("Error"), rest.trim())
        } else {
            res.replace("; ", "\n")
        };
        let txt: String = txt.chars().take(480).collect();
        if std::mem::take(&mut self.buddy.voice_reply) {
            return self.buddy_show(txt);
        }
        self.buddy.answer = Some(txt);
        self.buddy_hop();
    }

    fn buddy_send(&mut self) {
        let text = self.f("buddy").trim().to_string();
        if text.is_empty() {
            return;
        }
        self.fields.insert("buddy", String::new());
        self.buddy_run(text);
    }

    /// Anweisung ausführen: erst Schicht 1 (sofort, ohne Modell), sonst der Agent.
    pub(super) fn buddy_run(&mut self, text: String) {
        self.buddy.report = None;
        self.buddy.note = None;
        self.buddy.asked = None;
        self.buddy.answer = None;
        self.buddy.help_id = None;
        // „Wie funktioniert …?" – erst in den Hilfeseiten des Systems nachsehen
        let low = text.to_lowercase();
        if ["wie ", "how ", "كيف", "was ist", "what is", "ما هو", "ما هي", "erklär", "explain", "اشرح", "hilfe ", "help "].iter().any(|m| low.contains(m)) {
            if let Some(t) = self.buddy_help(&text, true) {
                return self.buddy_show_help(t);
            }
        }
        // „auf Server: …“ → an das Gerät im Verbund weiterreichen (auch für eingelernte Sprachbefehle)
        if let Some(geraet) = super::app_cloud::verbund_weiterleiten(&text) {
            self.buddy.native = true;
            self.buddy.answer = Some(format!("{} {geraet}", self.t("Sent to")));
            return;
        }
        match mind::understand(&text) {
            Some(Intent::Link(u)) => self.buddy_check(u),
            Some(Intent::Tool(c)) => {
                self.buddy.native = true;
                if c.risky() && self.agent.confirm {
                    self.pending = Some(c);
                } else {
                    self.exec(c);
                }
            }
            Some(Intent::Scan) => {
                self.cam.mode = 3;
                self.buddy.open = false;
                self.focus = None;
                self.focus_reset = true;
                self.go(Screen::Camera);
            }
            Some(Intent::Time) => {
                let d = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min);
                self.buddy.answer = Some(format!("{}  ·  {:02}.{:02}.{:04}", self.clock_str(d.h, d.mi), d.d, d.mo, d.y));
            }
            Some(Intent::Away) => {
                self.buddy.away = true;
                self.buddy.open = false;
                self.focus = None;
                self.focus_reset = true;
                self.buddy_save();
            }
            Some(Intent::Say(k)) => self.buddy.answer = Some(self.t(k).to_string()),
            Some(Intent::Remind(when, what)) => {
                let t = mind::due(when, crate::sys::unix_now(), self.set.tz_min);
                self.buddy.reminders.push((t, 0, what.clone()));
                self.buddy.rem_save();
                self.buddy.answer = Some(format!("{}\n{}", self.t("I will call you:"), self.rem_line(t, &what)));
                self.buddy_hop();
            }
            Some(Intent::Reminders) => {
                let list: Vec<String> = self.buddy.reminders.iter().take(8).map(|(t, _, x)| self.rem_line(*t, x)).collect();
                self.buddy.answer = Some(if list.is_empty() { self.t("No reminders").to_string() } else { format!("{}\n{}", self.t("Your reminders:"), list.join("\n")) });
            }
            Some(Intent::Tour) => self.buddy_tour(),
            Some(Intent::VoiceSetup) => self.voice_setup(),
            Some(Intent::VoiceSecret) => self.voice_learn(crate::voice::SECRET, "", 3),
            Some(Intent::VoiceOff) => {
                self.voice_enable(false);
                self.buddy.answer = Some(self.t("Voice control is off. I am not listening.").to_string());
            }
            Some(Intent::VoiceTeach(cmd)) => self.voice_learn(&cmd.clone(), &cmd, 3),
            Some(Intent::ForgetReminders) => {
                self.buddy.reminders.clear();
                self.buddy.rem_save();
                self.buddy.answer = Some(self.t("All reminders deleted").to_string());
            }
            None => {
                if let Some(t) = self.buddy_help(&text, false) {
                    self.buddy_show_help(t);
                } else if self.agent.enabled {
                    self.buddy.asked = Some(self.chat.len() + 1);
                    self.fields.insert("chat", text);
                    self.send_chat();
                } else {
                    self.buddy.note = Some("I did not understand that yet. Without the language model I know fixed commands – ask me \"what can you do\".");
                }
            }
        }
    }

    /// Helfer über die fertige Seite zeichnen. `bottom` = Unterkante des freien Bereichs (über Leiste/Tastatur).
    pub(super) fn d_buddy(&mut self, ui: &mut Ui, top: f32, bottom: f32) {
        self.buddy_load();
        self.buddy.links = std::mem::take(&mut ui.links);
        self.buddy.hit.clear();
        if let Some(rep) = self.buddy.slot.lock().ok().and_then(|mut s| s.take()) {
            self.buddy.report = Some(rep);
            self.buddy.checking = None;
            ui.dirty = true;
        }
        if let Some(t) = self.buddy.remote_say.take() {
            self.buddy_run(t);
        }
        // Wächter (Schicht 0): jede neue Adresse der Seite sofort grob beurteilen – ohne Netz, kostet Mikrosekunden
        let mut worst = Verdict::Ok;
        for u in self.buddy.links.clone() {
            let v = match self.buddy.guard.get(&u) {
                Some(v) => *v,
                None => {
                    let full = if u.contains("://") { u.clone() } else { format!("http://{u}") };
                    let v = linkcheck::analyze(&full).verdict;
                    if self.buddy.guard.len() > 400 {
                        self.buddy.guard.clear();
                    }
                    self.buddy.guard.insert(u.clone(), v);
                    // auffällige neue Adresse: von selbst melden
                    if v == Verdict::Bad && !self.buddy.away && !self.buddy.open {
                        self.buddy.alert = Some((u.clone(), v, Instant::now()));
                    }
                    v
                }
            };
            if v == Verdict::Bad || (v == Verdict::Warn && worst == Verdict::Ok) {
                worst = v;
            }
        }
        if self.buddy.alert.as_ref().is_some_and(|a| a.2.elapsed().as_secs() >= 8 || !self.buddy.links.contains(&a.0)) {
            self.buddy.alert = None;
        }
        if std::mem::take(&mut self.buddy.remote_check) {
            let u = self.buddy.links.first().cloned().unwrap_or_else(|| "http://example.com/".into());
            self.buddy_check(u);
        }
        let old_enabled = ui.enabled;
        ui.enabled = !self.power_dialog && !self.shade;
        let (white, gold) = (rgb(0xFFFFFF), ui.th.accent);

        // ---------- weggeschickt: nur eine Lasche am Rand ----------
        if self.buddy.away {
            let (tw, th) = (ui.dp(9.0), ui.dp(64.0));
            let x = if ui.rtl { 0.0 } else { ui.w - tw };
            let tab = Rect::new(x, top + (bottom - top) * 0.42, tw, th);
            ui.cv.rrect(Rect::new(tab.x - if ui.rtl { tw } else { 0.0 }, tab.y, tw * 2.0, th), tw, gold.alpha(0.85));
            let grab = Rect::new(if ui.rtl { 0.0 } else { ui.w - ui.dp(26.0) }, tab.y - ui.dp(8.0), ui.dp(26.0), th + ui.dp(16.0));
            self.buddy.hit.push(grab);
            if ui.tap(grab) {
                self.buddy.away = false;
                self.buddy.open = true;
                self.buddy_save();
                ui.dirty = true;
            }
            self.buddy.cur = Some((if ui.rtl { 0.0 } else { ui.w }, tab.y + th / 2.0));
            self.buddy.frame = None;
            self.buddy.stage = None;
            ui.enabled = old_enabled;
            return;
        }

        // Tastatur für ein anderes Feld offen: Helfer tritt beiseite (wie in der Web-Demo), nichts verdecken
        if !self.buddy.open && self.focus.is_some_and(|f| f != "buddy") && bottom < ui.h - ui.dp(60.0) {
            self.buddy.frame = None;
            self.buddy.stage = None;
            ui.enabled = old_enabled;
            return;
        }
        let r = ui.dp(27.0);
        let m = ui.dp(10.0);
        // Startbildschirm: sitzt oben rechts neben der Uhr (über keinem App-Symbol), sonst unten am Rand
        let on_home = matches!(self.stack.last().map(|e| &e.0), Some(Screen::Home)) && ui.h > ui.w && ui.h >= ui.dp(680.0);
        let home = if on_home { (if ui.rtl { r + m } else { ui.w - r - m }, top + ui.dp(122.0)) } else { (if ui.rtl { r + m } else { ui.w - r - m }, bottom - ui.dp(120.0)) };
        let mut pos = self.buddy.pos.unwrap_or(home);
        pos.0 = pos.0.clamp(r + m * 0.5, ui.w - r - m * 0.5);
        pos.1 = pos.1.clamp(top + r + m, (bottom - r - m).max(top + r + m));
        // Bildschirme mit Tasten bis an den Rand (Wählfeld, Rechner, Kamera): weiter oben hereinschauen
        let keypad = matches!(self.stack.last().map(|e| &e.0), Some(Screen::Camera));
        // Außerhalb des Startbildschirms schaut er standardmäßig nur halb vom Rand herein, damit er keine
        // Bedienelemente verdeckt (antippen öffnet ihn, ziehen setzt ihn frei hin)
        if self.buddy.pos.is_none() && !on_home {
            let y = if keypad { top + (bottom - top) * 0.28 } else { bottom - ui.dp(120.0) };
            pos = (if ui.rtl { r * 0.45 } else { ui.w - r * 0.45 }, y);
        }

        // ---------- Feld ----------
        let mut panel = None;
        if self.buddy.learning {
            self.buddy.open = false;
        }
        if self.buddy.open {
            let pw = ui.w - ui.dp(24.0);
            let inner = pw - ui.dp(28.0);
            let reply: Option<String> = self.buddy.asked.and_then(|n| self.chat.iter().skip(n).rev().find(|c| c.0 != 0).map(|c| c.1.chars().take(420).collect()));
            let waiting = self.buddy.asked.is_some() && reply.is_none();
            let shown_links: Vec<String> = if self.buddy.report.is_none() && self.buddy.checking.is_none() { self.buddy.links.iter().take(3).cloned().collect() } else { Vec::new() };
            let answer = self.buddy.answer.clone();
            let mut h = ui.dp(58.0) + ui.dp(62.0) + ui.dp(44.0);
            if let Some(t) = &answer {
                h += ui.tx.measure(t, ui.dp(14.5), false, inner).1 + ui.dp(14.0);
            }
            if self.buddy.help_id.is_some() {
                h += ui.dp(46.0);
            }
            if let Some(rep) = &self.buddy.report {
                h += ui.dp(52.0) + rep.reasons.len().min(6) as f32 * ui.dp(21.0) + ui.dp(26.0);
            }
            if self.buddy.checking.is_some() || waiting {
                h += ui.dp(34.0);
            }
            if !shown_links.is_empty() {
                h += ui.dp(24.0) + shown_links.len() as f32 * ui.dp(44.0);
            }
            if let Some(t) = &reply {
                h += ui.tx.measure(t, ui.dp(13.5), false, inner).1 + ui.dp(14.0);
            }
            if let Some(n) = self.buddy.note {
                h += ui.tx.measure(self.t(n), ui.dp(13.5), false, inner).1 + ui.dp(14.0);
            }
            let h = h.min(bottom - top - r - ui.dp(20.0));
            let pr = Rect::new(ui.dp(12.0), bottom - h - ui.dp(8.0), pw, h);
            self.buddy.hit.push(pr);
            ui.cv.soft_shadow(pr, ui.dp(22.0), rgb(0).alpha(0.35));
            ui.cv.rrect(pr, ui.dp(22.0), ui.th.line);
            ui.cv.rrect(pr.inset(1.0), ui.dp(21.0), ui.th.card);
            let (c, cs) = (ui.th.text, ui.th.sub);
            let x0 = pr.x + ui.dp(14.0);
            let mut y = pr.y + ui.dp(10.0);
            // Kopf
            ui.label(Rect::new(x0, y, inner * 0.5, ui.dp(24.0)), self.t("Helper"), 17.0, c, true, Al::Start);
            let sub = if let Some((t, _, x)) = self.buddy.reminders.first().cloned() {
                format!("{}: {}", self.t("Up next"), self.rem_line(t, &x))
            } else if self.buddy.links.is_empty() { self.t("No links on this page").to_string() } else { format!("{}: {}", self.t("Links on this page"), self.buddy.links.len()) };
            ui.label(Rect::new(x0, y + ui.dp(23.0), inner * 0.62, ui.dp(18.0)), &sub, 12.0, cs, false, Al::Start);
            {
                let label = self.t("Send away");
                let bw = ui.text_w(label, 13.5, false) + ui.dp(26.0);
                let bx = if ui.rtl { pr.x + ui.dp(12.0) } else { pr.right() - bw - ui.dp(12.0) };
                let (_, tapped) = ui.pill(bx, y + ui.dp(4.0), label, false);
                if tapped {
                    self.buddy.away = true;
                    self.buddy.open = false;
                    self.focus = None;
                    self.focus_reset = true;
                    self.buddy_save();
                    ui.dirty = true;
                }
            }
            y += ui.dp(52.0);
            ui.set_clip(Some(Rect::new(pr.x, y, pr.w, pr.bottom() - ui.dp(106.0) - y)));
            // Ergebnis der Prüfung
            if let Some(rep) = self.buddy.report.clone() {
                let (head, col) = match rep.verdict {
                    Verdict::Ok => (self.t("Looks safe"), ui.th.ok),
                    Verdict::Warn => (self.t("Be careful"), rgb(0xE8A317)),
                    Verdict::Bad => (self.t("Probably dangerous"), ui.th.danger),
                };
                ui.cv.circle(if ui.rtl { pr.right() - ui.dp(24.0) } else { x0 + ui.dp(10.0) }, y + ui.dp(13.0), ui.dp(10.0), col);
                ui.label(Rect::new(x0 + if ui.rtl { 0.0 } else { ui.dp(28.0) }, y, inner - ui.dp(28.0), ui.dp(26.0)), head, 17.0, col, true, Al::Start);
                let host = ui.fit(&rep.host, 13.0, false, inner);
                ui.label(Rect::new(x0, y + ui.dp(28.0), inner, ui.dp(20.0)), &host, 13.0, cs, false, Al::Start);
                y += ui.dp(52.0);
                if rep.reasons.is_empty() {
                    ui.label(Rect::new(x0, y, inner, ui.dp(20.0)), self.t("No problems found in the address"), 13.0, c, false, Al::Start);
                    y += ui.dp(21.0);
                }
                for (why, extra) in rep.reasons.iter().take(6) {
                    let line = if extra.is_empty() { format!("• {}", self.t(why)) } else { format!("• {}: {extra}", self.t(why)) };
                    let line = ui.fit(&line, 13.0, false, inner);
                    ui.label(Rect::new(x0, y, inner, ui.dp(20.0)), &line, 13.0, c, false, Al::Start);
                    y += ui.dp(21.0);
                }
                ui.label(Rect::new(x0, y + ui.dp(2.0), inner, ui.dp(18.0)), self.t("An estimate, not a guarantee"), 11.5, cs, false, Al::Start);
                y += ui.dp(26.0);
            }
            if self.buddy.checking.is_some() || waiting {
                let msg = if waiting { self.t("Thinking…") } else { self.t("Checking in the sealed box…") };
                ui.label(Rect::new(x0, y, inner, ui.dp(26.0)), msg, 13.5, cs, false, Al::Start);
                y += ui.dp(34.0);
                ui.dirty = true;
            }
            if !shown_links.is_empty() {
                ui.label(Rect::new(x0, y, inner, ui.dp(18.0)), self.t("Links on this page"), 12.0, cs, true, Al::Start);
                y += ui.dp(24.0);
                let label = self.t("Check");
                let bw = ui.text_w(label, 13.5, false) + ui.dp(26.0);
                for u in shown_links {
                    let name = ui.fit(&u, 13.0, false, inner - bw - ui.dp(10.0));
                    let (tx, bx) = if ui.rtl { (x0 + bw + ui.dp(10.0), x0) } else { (x0, pr.right() - bw - ui.dp(14.0)) };
                    ui.label(Rect::new(tx, y, inner - bw - ui.dp(10.0), ui.dp(36.0)), &name, 13.0, c, false, Al::Start);
                    let (_, tapped) = ui.pill(bx, y, label, true);
                    if tapped {
                        self.buddy_check(u);
                        ui.dirty = true;
                    }
                    y += ui.dp(44.0);
                }
            }
            if let Some(t) = &answer {
                y += ui.para(x0, y, inner, t, 14.5, c) + ui.dp(14.0);
            }
            if let Some(id) = self.buddy.help_id {
                let label = self.t("Open help page");
                let bx = if ui.rtl { pr.right() - ui.dp(14.0) - ui.text_w(label, 13.5, false) - ui.dp(26.0) } else { x0 };
                if ui.pill(bx, y, label, true).1 {
                    self.buddy.open = false;
                    self.focus = None;
                    self.focus_reset = true;
                    self.go(Screen::Help(Some(id.to_string())));
                    ui.dirty = true;
                }
                y += ui.dp(46.0);
            }
            if let Some(t) = &reply {
                y += ui.para(x0, y, inner, t, 13.5, c) + ui.dp(14.0);
            }
            if let Some(n) = self.buddy.note {
                y += ui.para(x0, y, inner, self.t(n), 13.5, cs) + ui.dp(14.0);
            }
            let _ = y;
            ui.set_clip(None);
            // Vorschläge passend zur Seite (ein Tipp genügt)
            {
                let torch = crate::quick::torch_on();
                let on_cam = matches!(self.stack.last(), Some((Screen::Camera, _)));
                let chips: [(&'static str, &'static str); 3] = [
                    if on_cam { ("Scan", "scan document") } else { ("Flashlight", if torch { "flashlight off" } else { "flashlight on" }) },
                    ("Status", "status"),
                    ("Help", "help"),
                ];
                let cy = pr.bottom() - ui.dp(46.0) - ui.dp(10.0) - ui.dp(42.0);
                let mut cx = x0;
                let th0 = ui.th;
                ui.th.card = ui.th.bg;
                for (label, cmd) in chips {
                    let lw = ui.text_w(self.t(label), 13.5, false) + ui.dp(26.0);
                    let px = if ui.rtl { pr.right() - ui.dp(14.0) - (cx - x0) - lw } else { cx };
                    let (w, tapped) = ui.pill(px, cy, self.t(label), label == "Flashlight" && torch);
                    cx += w + ui.dp(8.0);
                    if tapped {
                        self.buddy_run(cmd.to_string());
                        ui.dirty = true;
                    }
                }
                ui.th = th0;
            }
            // Eingabe
            let fh = ui.dp(46.0);
            let fy = pr.bottom() - fh - ui.dp(10.0);
            let fw = inner - fh - ui.dp(8.0);
            let (fx, bx) = if ui.rtl { (x0 + fh + ui.dp(8.0), x0 + fh / 2.0) } else { (x0, x0 + fw + ui.dp(8.0) + fh / 2.0) };
            ui.field(Rect::new(fx, fy, fw, fh), "buddy", &self.f("buddy"), self.t("Ask or paste a link"));
            let can = !self.f("buddy").trim().is_empty() && !self.agent_busy;
            let (bgc, fgc) = if can { (gold, ui.th.on_accent) } else { (ui.th.bg, cs) };
            if ui.icon_btn(bx, fy + fh / 2.0, fh / 2.0, Ic::Send, bgc, fgc) && can {
                self.buddy_send();
                ui.dirty = true;
            }
            panel = Some(pr);
        }

        // ---------- Figur: wohin will er, wo ist er ----------
        let calling = self.buddy.calling.as_ref().map(|c| (c.text.clone(), !c.quiet && c.rung.elapsed().as_millis() < 900));
        let quiet_call = self.buddy.calling.as_ref().is_some_and(|c| c.quiet);
        let learn = self.voice_card();
        let free = (bottom - top - 2.0 * (r + m)).max(1.0);
        let tour = self.buddy.tour.filter(|_| calling.is_none() && panel.is_none() && learn.is_none());
        let target = match (&calling, panel, self.buddy.roam) {
            (Some(_), _, _) => (ui.w / 2.0, top + (bottom - top) * 0.34),
            (None, _, _) if learn.is_some() => (ui.w / 2.0, top + (bottom - top) * 0.3),
            (None, None, _) if tour.is_some() => {
                let (_, ax, ay) = TOUR[tour.unwrap_or(0).min(TOUR.len() - 1)];
                (r + m + ax * (ui.w - 2.0 * (r + m)), top + r * 1.4 + m + ay * (bottom - top - 2.0 * (r * 1.4 + m)))
            }
            // geöffnet sitzt er oben auf dem Feld
            (None, Some(pr), _) => (if ui.rtl { pr.x + r + ui.dp(18.0) } else { pr.right() - r - ui.dp(18.0) }, pr.y - r * 0.55),
            (None, None, Some((ax, ay, _))) if !self.buddy.drag => (r + m + ax * (ui.w - 2.0 * (r + m)), top + r + m + ay * free),
            _ => pos,
        };
        let now_i = Instant::now();
        let dt = self.buddy.frame.map(|t| now_i.duration_since(t).as_secs_f32()).unwrap_or(0.0).min(0.1);
        self.buddy.frame = Some(now_i);
        let dragging = self.buddy.drag && self.buddy.moved && panel.is_none();
        let grow_to = if calling.is_some() { 1.0 } else if learn.is_some() { 0.7 } else if tour.is_some() { 0.4 } else { 0.0 };
        let mut cur = self.buddy.cur.unwrap_or(target);
        if self.buddy.snap || dragging {
            cur = target;
            if self.buddy.snap {
                self.buddy.grow = grow_to;
            }
            self.buddy.vx = 0.0;
        } else {
            let k = 1.0 - (-dt * 6.5).exp();
            let step = ((target.0 - cur.0) * k, (target.1 - cur.1) * k);
            cur = (cur.0 + step.0, cur.1 + step.1);
            let dist = (step.0 * step.0 + step.1 * step.1).sqrt();
            self.buddy.phase += dist / ui.dp(30.0) * std::f32::consts::PI;
            self.buddy.vx = if dt > 0.0 { step.0 / dt } else { 0.0 };
            self.buddy.grow += (grow_to - self.buddy.grow) * (1.0 - (-dt * 9.0).exp());
        }
        let rest = ((target.0 - cur.0).powi(2) + (target.1 - cur.1).powi(2)).sqrt();
        if rest < 0.6 {
            cur = target;
        }
        if (grow_to - self.buddy.grow).abs() < 0.01 {
            self.buddy.grow = grow_to;
        }
        self.buddy.cur = Some(cur);
        self.buddy.moving = rest >= 0.6 || self.buddy.grow != grow_to;
        if self.buddy.animating() {
            ui.dirty = true;
        }
        let grow = self.buddy.grow;
        let stage = self.buddy.stage.take();
        let pv = if stage.is_some() { self.buddy.preview } else { 0 };
        let rr = stage.map(|s| s.2).unwrap_or(r * (1.0 + 0.8 * grow));
        let shown = stage.map(|s| (s.0, s.1)).unwrap_or(cur);
        if stage.is_some() {
            ui.dirty = true;
        }
        // Hüpfen: beim Laufen kleine Schritte, auf Zuruf ein Sprung
        let walk = (rest / ui.dp(36.0)).min(1.0) * self.buddy.phase.sin().abs() * ui.dp(9.0);
        let jump = self.buddy.hop.map(|t| t.elapsed().as_millis() as f32 / 480.0).filter(|t| *t < 1.0).map(|t| (t * std::f32::consts::PI).sin() * ui.dp(18.0)).unwrap_or(0.0);
        let lift = if dragging { 0.0 } else { walk + jump };

        let body = Rect::new(shown.0 - rr, shown.1 - rr, 2.0 * rr, 2.0 * rr);
        let grab = body.inset(-ui.dp(6.0));
        self.buddy.hit.push(grab);
        let p = (ui.p.x, ui.p.y, ui.p.px, ui.p.py, ui.p.down, ui.p.pressed, ui.p.released);
        if ui.enabled && p.5 && grab.contains(p.2, p.3) && calling.is_none() && tour.is_none() && learn.is_none() {
            self.buddy.drag = true;
            self.buddy.moved = false;
            self.buddy.roam = None;
            self.buddy.say = None;
        }
        if self.buddy.drag {
            if p.4 {
                if (p.0 - p.2).abs() + (p.1 - p.3).abs() > ui.dp(10.0) {
                    self.buddy.moved = true;
                }
                if self.buddy.moved && panel.is_none() {
                    self.buddy.pos = Some((p.0, p.1));
                    ui.dirty = true;
                }
            } else {
                self.buddy.drag = false;
                if !self.buddy.moved {
                    self.buddy.open = !self.buddy.open;
                    self.buddy.hop = Some(Instant::now());
                    if !self.buddy.open && self.focus == Some("buddy") {
                        self.focus = None;
                        self.focus_reset = true;
                    }
                } else if let Some((x, y)) = self.buddy.pos {
                    // an den nächsten Rand gleiten
                    let nx = if x < ui.w / 2.0 { r + m } else { ui.w - r - m };
                    self.buddy.pos = Some((nx, y));
                    self.buddy_save();
                }
                ui.dirty = true;
            }
        }
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let wave = |period: u128| ((now_ms % period) as f32 / period as f32 * std::f32::consts::TAU).sin();

        // ---------- Anruf: Seite abdunkeln, Karte mit dem Anliegen ----------
        if let Some((text, _)) = &calling {
            let all = Rect::new(0.0, 0.0, ui.w, ui.h);
            self.buddy.hit.push(all);
            ui.cv.rect(all, rgb(0).alpha(0.62 * grow));
            for i in 0..3 {
                let t = ((now_ms + i * 500) % 1500) as f32 / 1500.0;
                ui.cv.ring(shown.0, shown.1, rr + ui.dp(6.0) + t * ui.dp(46.0), ui.dp(2.5), gold.alpha((1.0 - t) * 0.8 * grow));
            }
            let cw = ui.w - ui.dp(48.0);
            let inner = cw - ui.dp(36.0);
            let th_ = ui.tx.measure(text, ui.dp(19.0), false, inner).1;
            let ch = ui.dp(54.0) + th_ + ui.dp(74.0);
            let card = Rect::new(ui.dp(24.0), shown.1 + rr + ui.dp(34.0), cw, ch);
            ui.cv.soft_shadow(card, ui.dp(24.0), rgb(0).alpha(0.4));
            ui.cv.rrect(card, ui.dp(24.0), gold);
            ui.cv.rrect(card.inset(1.5), ui.dp(23.0), ui.th.card);
            let d = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min);
            let head = if quiet_call { "KM Code Craft".to_string() } else { format!("{}  ·  {}", self.t("Reminder"), self.clock_str(d.h, d.mi)) };
            ui.label(Rect::new(card.x, card.y + ui.dp(14.0), cw, ui.dp(22.0)), &head, 13.5, gold, true, Al::Center);
            ui.para(card.x + ui.dp(18.0), card.y + ui.dp(46.0), inner, text, 19.0, ui.th.text);
            let (l1, l2) = (self.t(if quiet_call { "OK" } else { "Done" }), self.t("In 10 min"));
            let (w1, w2) = (ui.text_w(l1, 13.5, false) + ui.dp(26.0), if quiet_call { -ui.dp(14.0) } else { ui.text_w(l2, 13.5, false) + ui.dp(26.0) });
            let by = card.bottom() - ui.dp(54.0);
            let x1 = card.x + (cw - w1 - w2 - ui.dp(14.0)) / 2.0;
            let (xa, xb) = if ui.rtl { (x1 + w2 + ui.dp(14.0), x1) } else { (x1, x1 + w1 + ui.dp(14.0)) };
            let done = ui.pill(xa, by, l1, true).1;
            let later = !quiet_call && ui.pill(xb, by, l2, false).1;
            if quiet_call && done {
                self.buddy.calling = None;
                ui.dirty = true;
            } else if done || later {
                if later {
                    self.buddy.reminders.push((crate::sys::unix_now() + 600, 0, text.clone()));
                    self.buddy.rem_save();
                }
                self.buddy.calling = None;
                self.buddy.hop = Some(Instant::now());
                self.buddy.say = Some((self.t(if later { "OK, in 10 minutes again" } else { "Done" }).to_string(), Instant::now()));
                ui.dirty = true;
            }
        }

        // ---------- Sprache einlernen: Karte mit Aufforderung, Fortschritt und Pegel ----------
        if let (Some((title, hint, got, need, level)), None) = (&learn, &calling) {
            let all = Rect::new(0.0, 0.0, ui.w, ui.h);
            self.buddy.hit.push(all);
            ui.cv.rect(all, rgb(0).alpha(0.55));
            let cw = ui.w - ui.dp(40.0);
            let inner = cw - ui.dp(36.0);
            let th_ = ui.tx.measure(hint, ui.dp(16.5), false, inner).1;
            let ch = ui.dp(48.0) + th_ + ui.dp(112.0);
            let card = Rect::new(ui.dp(20.0), target.1 + rr + ui.dp(30.0), cw, ch);
            ui.cv.soft_shadow(card, ui.dp(24.0), rgb(0).alpha(0.4));
            ui.cv.rrect(card, ui.dp(24.0), gold);
            ui.cv.rrect(card.inset(1.5), ui.dp(23.0), ui.th.card);
            ui.label(Rect::new(card.x, card.y + ui.dp(12.0), cw, ui.dp(22.0)), title, 13.0, gold, true, Al::Center);
            ui.para(card.x + ui.dp(18.0), card.y + ui.dp(42.0), inner, hint, 16.5, ui.th.text);
            // Fortschritt: ein Punkt je Aufnahme
            let dy = card.y + ui.dp(42.0) + th_ + ui.dp(20.0);
            for i in 0..*need {
                let cx = card.x + cw / 2.0 + (i as f32 - (*need as f32 - 1.0) / 2.0) * ui.dp(26.0);
                if i < *got {
                    ui.cv.circle(cx, dy, ui.dp(8.0), gold);
                } else {
                    ui.cv.ring(cx, dy, ui.dp(8.0), ui.dp(2.0), ui.th.sub);
                }
            }
            // Pegel
            let bar = Rect::new(card.x + ui.dp(18.0), dy + ui.dp(20.0), inner, ui.dp(6.0));
            ui.cv.rrect(bar, ui.dp(3.0), ui.th.bg);
            let lv = ((level / 30.0).max(1.0).log10() / 2.0).clamp(0.0, 1.0);
            ui.cv.rrect(Rect::new(bar.x, bar.y, bar.w * lv, bar.h), ui.dp(3.0), gold);
            let label = self.t("Cancel");
            let bw = ui.text_w(label, 13.5, false) + ui.dp(26.0);
            if ui.pill(card.x + (cw - bw) / 2.0, card.bottom() - ui.dp(50.0), label, false).1 {
                self.voice_cancel();
            }
            ui.dirty = true;
        }

        // ---------- Vorstellung und Führung: Karte neben ihm, Seite dahinter ruht ----------
        if let Some(step) = tour {
            let step = step.min(TOUR.len() - 1);
            let all = Rect::new(0.0, 0.0, ui.w, ui.h);
            self.buddy.hit.push(all);
            ui.cv.rect(all, rgb(0).alpha(0.5));
            let text = self.t(TOUR[step].0).to_string();
            let cw = ui.w - ui.dp(40.0);
            let inner = cw - ui.dp(36.0);
            let th_ = ui.tx.measure(&text, ui.dp(16.5), false, inner).1;
            let ch = ui.dp(48.0) + th_ + ui.dp(70.0);
            // Karte unter ihm, wenn er oben steht – sonst über ihm (Zielstelle zählt, damit sie nicht springt)
            let cy = if target.1 < top + (bottom - top) * 0.5 { target.1 + rr + ui.dp(30.0) } else { target.1 - rr - ui.dp(40.0) - ch };
            let card = Rect::new(ui.dp(20.0), cy, cw, ch);
            ui.cv.soft_shadow(card, ui.dp(24.0), rgb(0).alpha(0.4));
            ui.cv.rrect(card, ui.dp(24.0), gold);
            ui.cv.rrect(card.inset(1.5), ui.dp(23.0), ui.th.card);
            let head = format!("KM Code Craft  ·  {} / {}", step + 1, TOUR.len());
            ui.label(Rect::new(card.x, card.y + ui.dp(12.0), cw, ui.dp(22.0)), &head, 13.0, gold, true, Al::Center);
            ui.para(card.x + ui.dp(18.0), card.y + ui.dp(42.0), inner, &text, 16.5, ui.th.text);
            let last = step + 1 == TOUR.len();
            let (l1, l2) = (self.t(if last { "Done" } else { "Next" }), self.t("Skip"));
            let (w1, w2) = (ui.text_w(l1, 13.5, false) + ui.dp(26.0), if last { 0.0 } else { ui.text_w(l2, 13.5, false) + ui.dp(26.0) + ui.dp(14.0) });
            let by = card.bottom() - ui.dp(52.0);
            let x1 = card.x + (cw - w1 - w2) / 2.0;
            let (xa, xb) = if ui.rtl { (x1 + w2, x1) } else { (x1, x1 + w1 + ui.dp(14.0)) };
            let next = ui.pill(xa, by, l1, true).1;
            let skip = !last && ui.pill(xb, by, l2, false).1;
            if next || skip {
                self.buddy.tour = if skip || last { None } else { Some(step + 1) };
                self.buddy.hop = Some(Instant::now());
                ui.dirty = true;
            }
        }

        let busy = self.buddy.checking.is_some() || (self.buddy.asked.is_some() && self.agent_busy) || pv == 2 || pv == 3;
        // Schatten bleibt am Boden und wird beim Abheben blasser
        if busy {
            let t = (now_ms % 1200) as f32 / 1200.0;
            ui.cv.ring(shown.0, shown.1, rr + ui.dp(3.0) + t * ui.dp(8.0), ui.dp(2.0), gold.alpha(1.0 - t));
            ui.dirty = true;
        }
        // Stimmung: Ergebnis der letzten Prüfung, sonst der Wächter über die Links der Seite
        let mood = if calling.is_some() { None } else { self.buddy.report.as_ref().map(|x| x.verdict).or(if worst != Verdict::Ok && panel.is_none() { Some(worst) } else { None }) };
        let tone = match mood {
            Some(Verdict::Bad) => ui.th.danger,
            Some(Verdict::Warn) => rgb(0xE8A317),
            Some(Verdict::Ok) => ui.th.ok,
            None => gold,
        };
        // wackeln: bei Gefahr, und beim Klingeln wie ein Telefon
        let ringing = calling.as_ref().is_some_and(|c| c.1) || pv == 5;
        let shake = if ringing { wave(110) * ui.dp(4.0) } else if self.buddy.alert.is_some() { wave(300) * ui.dp(2.0) } else { 0.0 };
        // atmen
        let r = rr * (1.0 + 0.018 * wave(3600));
        let (fx, fy) = (shown.0 + shake, shown.1 - lift);
        // ---------- Figur „KM-Helfer" (eigene Figur, Designsprache Ultar Noir) ----------
        // Maße aus der Vorlage (200 × 220 Einheiten, Körper 136 breit): s = Pixel je Einheit
        let s = r / 68.0;
        let (ox, oy) = (fx, fy);
        let px = |u: f32| ox + (u - 100.0) * s;
        let py = |v: f32| oy + (v - 115.0) * s;
        let ink = rgb(0x1B1C22);
        let (cream_t, cream_m, cream_b) = (rgb(0xFFFBEE), rgb(0xF1E7C2), rgb(0xCDB872));
        let state_happy = calling.is_some() || tour.is_some() || self.buddy.hop.is_some_and(|t| t.elapsed().as_millis() < 700) || pv == 1 || pv == 5;
        let state_alert = mood == Some(Verdict::Bad) || pv == 6;
        let talking = (self.buddy.say.is_some() || quiet_call || pv == 4) && !state_alert;
        let gem = if state_alert { ui.th.danger } else if ringing { ui.th.ok } else { tone };
        // Schatten/Glanz am Boden: bleibt unten, wird beim Abheben kleiner
        let ground = shown.1 + 95.0 * s;
        let sk = (1.0 - lift / ui.dp(40.0)).clamp(0.55, 1.0);
        ui.cv.ellipse(shown.0, ground, 44.0 * s * sk, 7.0 * s, 0.0, rgb(0).alpha(0.32 * sk));
        // Fühler: schwingen der Bewegung hinterher, Lichter an der Spitze (Statusfarbe)
        let sway = (-self.buddy.vx * 0.035).clamp(-r * 0.4, r * 0.4) + shake * 0.8;
        for (side, base_x, tip_x, ph) in [(-1.0f32, 80.0f32, 65.0f32, 0u128), (1.0, 120.0, 135.0, 1150)] {
            let w = ((now_ms + ph) % 2300) as f32 / 2300.0 * std::f32::consts::TAU;
            let tx = px(tip_x) + sway + w.sin() * 3.0 * s * 2.0 + side * 0.0;
            let ty = py(22.0);
            ui.cv.line(px(base_x), py(52.0), (px(base_x) + tx) / 2.0 + side * 2.0 * s, py(36.0), 4.5 * s, cream_b);
            ui.cv.line((px(base_x) + tx) / 2.0 + side * 2.0 * s, py(36.0), tx, ty, 4.5 * s, cream_b);
            let glow = if busy || calling.is_some() || self.buddy.say.is_some() { 0.35 + 0.25 * wave(700) } else { 0.0 };
            if glow > 0.0 {
                ui.cv.circle(tx, ty, 13.0 * s, gem.alpha(glow));
            }
            ui.cv.circle(tx, ty, 7.5 * s, if state_alert || ringing { gem } else { rgb(0xF0F090) });
        }
        // Körper: weiche Bohne mit Verlauf (Creme → Logo-Gold), Glanzlicht oben links
        let (bx0, by0, bw, bh) = (px(32.0), py(42.0), 136.0 * s, 146.0 * s);
        let (ccx, ccy, rx, ry) = (px(100.0), py(117.0), 68.0 * s, 73.0 * s);
        let bb = Rect::new(bx0 - 2.0, by0 - 2.0, bw + 4.0, bh + 4.0);
        ui.cv.sdf_vgrad(bb, cream_t, cream_b, |x, y| {
            let (dx, dy) = ((x - ccx) / rx, (y - ccy) / ry);
            // unten etwas breiter und flacher (Bohne): Exponent wächst nach unten
            let e = if dy > 0.0 { 2.0 + dy * 1.2 } else { 2.0 };
            ((dx.abs().powf(e) + dy.abs().powf(e)).powf(1.0 / e) - 1.0) * rx.min(ry)
        });
        ui.cv.sdf(bb, cream_m.alpha(0.55), |x, y| {
            let (dx, dy) = ((x - ccx) / (rx * 0.9), (y - ccy + 18.0 * s) / (ry * 0.62));
            ((dx * dx + dy * dy).sqrt() - 1.0) * rx * 0.6
        });
        ui.cv.ellipse(px(70.0), py(70.0), 18.0 * s, 9.0 * s, -0.49, rgb(0xFFFFFF).alpha(0.7));
        // Bäckchen (Rot der Marke, zart)
        ui.cv.ellipse(px(64.0), py(124.0), 9.0 * s, 5.0 * s, 0.0, rgb(crate::ui::KM_RED).alpha(0.2));
        ui.cv.ellipse(px(136.0), py(124.0), 9.0 * s, 5.0 * s, 0.0, rgb(crate::ui::KM_RED).alpha(0.2));
        // Augen: schauen zur Berührung/Laufrichtung, blinzeln; fröhlich (^ ^), streng (Gefahr), müde (Nacht)
        let look = if calling.is_some() || learn.is_some() {
            (0.0, 0.25)
        } else if p.4 {
            (p.0 - fx, p.1 - fy)
        } else if rest > ui.dp(6.0) {
            (target.0 - cur.0, target.1 - cur.1)
        } else if busy {
            (0.5, -1.0)
        } else if self.buddy.roam.is_some() {
            (wave(1700), 0.3 * wave(2900))
        } else if panel.is_some() {
            (0.0, 1.0)
        } else {
            (if fx > ui.w / 2.0 { -1.0 } else { 1.0 }, 0.2)
        };
        let ln = (look.0 * look.0 + look.1 * look.1).sqrt().max(1.0);
        let (lx, ly) = (look.0 / ln * 5.0 * s, look.1 / ln * 4.0 * s);
        let blink = now_ms % 4300 < 140;
        let hour = crate::sys::from_unix(crate::sys::unix_now(), self.set.tz_min).h;
        let sleepy = pv == 7 || pv == 0 && !(6..23).contains(&hour) && mood.is_none() && !busy && calling.is_none() && tour.is_none() && learn.is_none() && !self.buddy.moving && self.buddy.roam.is_none();
        let arc = |ui: &mut Ui, cx: f32, cy: f32, w: f32, h: f32, th: f32, c: crate::gfx::Color| {
            // Bogen aus kurzen Strichen: h > 0 = nach oben gewölbt (^), h < 0 = nach unten (∪)
            let mut prev: Option<(f32, f32)> = None;
            for i in 0..=8 {
                let t = -1.0 + i as f32 / 4.0;
                let pt = (cx + t * w, cy - h * (1.0 - t * t));
                if let Some(q) = prev {
                    ui.cv.line(q.0, q.1, pt.0, pt.1, th, c);
                }
                prev = Some(pt);
            }
        };
        for sx in [-1.0f32, 1.0] {
            let (ex, ey) = (px(100.0 + sx * 20.0) + lx, py(106.0) + ly);
            if state_happy {
                arc(ui, ex, ey + 4.0 * s, 10.0 * s, 9.0 * s, 5.5 * s, ink);
            } else if sleepy {
                arc(ui, ex, ey, 10.0 * s, -7.0 * s, 5.0 * s, ink);
            } else if blink {
                ui.cv.line(ex - 9.0 * s, ey, ex + 9.0 * s, ey, 5.0 * s, ink);
            } else if state_alert {
                ui.cv.ellipse(ex, ey + 2.0 * s, 10.0 * s, 5.0 * s, 0.0, ink);
                ui.cv.line(px(100.0 + sx * 34.0), py(90.0), px(100.0 + sx * 8.0), py(98.0), 5.0 * s, ink);
            } else {
                let big = if p.4 || busy { 1.12 } else { 1.0 };
                ui.cv.ellipse(ex, ey, 10.5 * s * big, 13.5 * s * big, 0.0, ink);
                ui.cv.circle(ex + 4.0 * s, ey - 6.0 * s, 3.8 * s, rgb(0xFFFFFF));
            }
        }
        // Mund: lächeln, offen (fröhlich), sprechen (bewegt), rund (nachdenken), traurig (Gefahr)
        let (mx, my) = (px(100.0), py(131.0));
        if state_alert {
            arc(ui, mx, my + 5.0 * s, 10.0 * s, 6.0 * s, 4.5 * s, ink);
        } else if talking {
            let o = 0.35 + 0.75 * wave(320).abs();
            ui.cv.ellipse(mx, my + 2.0 * s, 9.0 * s, 7.5 * s * o, 0.0, ink);
        } else if state_happy {
            ui.cv.sdf(Rect::new(mx - 14.0 * s, my - 4.0 * s, 28.0 * s, 22.0 * s), ink, |x, y| {
                let (dx, dy) = ((x - mx) / (13.0 * s), (y - (my - 3.0 * s)) / (16.0 * s));
                let e = ((dx * dx + dy * dy).sqrt() - 1.0) * 13.0 * s;
                e.max((my - 3.0 * s) - y)
            });
        } else if busy {
            ui.cv.ellipse(mx, my + 2.0 * s, 5.0 * s, 6.0 * s, 0.0, ink);
        } else {
            arc(ui, mx, my - 4.0 * s, 10.0 * s, -8.0 * s, 4.5 * s, ink);
        }
        // Brust-Licht: zeigt den Zustand (Gold bereit, Grün Anruf, Rot Gefahr), atmet
        ui.cv.circle(px(100.0), py(162.0), 8.5 * s, rgb(0xFFFFFF).alpha(0.55));
        ui.cv.circle(px(100.0), py(162.0), 6.5 * s, gem.alpha(0.75 + 0.25 * wave(2600)));
        // Hände: schweben; winken beim Anruf/Begrüßen, Hand am Kinn beim Nachdenken, „Stopp" bei Gefahr,
        // gestikulieren beim Sprechen
        let bob = |ph: u128| wave(2600 + ph) * 3.0 * s;
        let (mut hl, mut hr) = ((px(42.0), py(142.0) + bob(0)), (px(158.0), py(142.0) + bob(1300)));
        if calling.is_some() || tour.is_some() {
            let w = wave(if ringing { 500 } else { 900 });
            hr = (px(160.0) + w * 6.0 * s, py(84.0) - (w.abs() * 4.0) * s);
        } else if state_alert {
            hl = (px(72.0), py(116.0));
            hr = (px(128.0), py(116.0));
        } else if busy {
            hr = (px(116.0), py(140.0));
        } else if talking {
            hl.1 -= wave(1400).max(0.0) * 14.0 * s;
            hr.1 -= (-wave(1400)).max(0.0) * 14.0 * s;
        }
        for (hx, hy) in [hl, hr] {
            ui.cv.sdf_vgrad(Rect::new(hx - 13.5 * s, hy - 13.5 * s, 27.0 * s, 27.0 * s), cream_t, cream_b, |x, y| ((x - hx).powi(2) + (y - hy).powi(2)).sqrt() - 12.5 * s);
        }
        // Denkblasen beim Nachdenken, „z" beim Schlafen
        if busy {
            for (i, (u, v, rr2)) in [(150.0f32, 34.0f32, 5.0f32), (166.0, 20.0, 7.0), (186.0, 2.0, 10.0)].iter().enumerate() {
                let a = 0.35 + 0.65 * (((now_ms + i as u128 * 200) % 1200) as f32 / 1200.0 * std::f32::consts::TAU).sin().abs();
                let u = if ui.rtl { 200.0 - u } else { *u };
                ui.cv.circle(px(u), py(*v), rr2 * s, ui.th.accent2.alpha(a));
            }
        } else if sleepy {
            let t = (now_ms % 2400) as f32 / 2400.0;
            let zr = Rect::new(px(146.0) + t * 14.0 * s, py(30.0) - t * 26.0 * s, 24.0 * s, 24.0 * s);
            ui.label(zr, "z", (16.0 * s / ui.k).max(8.0), ui.th.accent2.alpha(1.0 - t), true, Al::Center);
            ui.dirty = true;
        }
        let quiet = panel.is_none() && calling.is_none() && tour.is_none() && learn.is_none() && !self.buddy.moving;
        // Zeichen an der Figur: Ausrufezeichen bei auffälligen Links, sonst Zahl der Links
        if quiet && !self.buddy.links.is_empty() {
            let (bx, by) = (fx + r * 0.72, fy - r * 0.72);
            let warn = worst != Verdict::Ok;
            ui.cv.circle(bx, by, ui.dp(9.5), if warn { tone } else { gold });
            let n = if warn { "!".to_string() } else { self.buddy.links.len().min(9).to_string() };
            ui.label(Rect::new(bx - ui.dp(9.0), by - ui.dp(9.0), ui.dp(18.0), ui.dp(18.0)), &n, 11.5, if warn { white } else { ui.th.on_accent }, true, Al::Center);
        }
        // Sprechblasen neben der Figur: Warnung vor einem gefährlichen Link (antippen = genau prüfen) oder ein kurzer Satz
        if quiet {
            let alert = self.buddy.alert.clone();
            let msg = match &alert {
                Some(_) => Some(self.t("Careful – suspicious link on this page").to_string()),
                None => self.buddy.say.as_ref().map(|s| s.0.clone()),
            };
            if let Some(msg) = msg {
                let tw = (ui.text_w(&msg, 13.0, true) + ui.dp(26.0)).min(ui.w - 2.0 * r - ui.dp(40.0));
                let left = fx > ui.w / 2.0;
                let br = Rect::new(if left { fx - r - ui.dp(8.0) - tw } else { fx + r + ui.dp(8.0) }, fy - ui.dp(19.0), tw, ui.dp(38.0));
                ui.cv.soft_shadow(br, ui.dp(19.0), rgb(0).alpha(0.3));
                if alert.is_some() {
                    ui.cv.rrect(br, ui.dp(19.0), ui.th.danger);
                } else {
                    ui.cv.rrect(br, ui.dp(19.0), gold);
                    ui.cv.rrect(br.inset(1.5), ui.dp(17.5), ui.th.card);
                }
                let m2 = ui.fit(&msg, 13.0, true, tw - ui.dp(20.0));
                ui.label(br, &m2, 13.0, if alert.is_some() { white } else { ui.th.text }, true, Al::Center);
                self.buddy.hit.push(br);
                if ui.tap(br) {
                    self.buddy.open = true;
                    self.buddy.say = None;
                    if let Some((u, _, _)) = self.buddy.alert.take() {
                        self.buddy_check(u);
                    }
                }
                ui.dirty = true;
            }
        }
        ui.enabled = old_enabled;
    }
}
