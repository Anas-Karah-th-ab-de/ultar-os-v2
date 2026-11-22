//! Sprachsteuerung des Helfers: Weckwort, geheimes Wort, eigene Sprachbefehle – eingelernt mit der Stimme
//! des Besitzers, erkannt im Rust-Kern ohne Netz (`voice.rs`), Ton vom Mikrofon (`mic.rs`).
//!
//! Ablauf: „Hey KM" (entsperrt) oder das geheime Wort (auch gesperrt) → er hört 8 s auf einen eingelernten
//! Befehl. Passt das Wort, aber nicht die Stimme: Abweisung; der Besitzer kann sich dann mit Finger, Gesicht
//! oder PIN ausweisen (Sperrbildschirm) und wird danach gehört – etwa bei Erkältung.
//! Gesperrt führt er per Stimme nur Unkritisches aus (Uhrzeit, Erinnerungen, harmlose Schalter).

use super::App;
use crate::mind::{self, Intent};
use crate::voice::{self, Cutter, Heard, Profile, Utt};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROFILE: &str = "/data/ultar/voice.tsv";
const ENABLED: &str = "/data/ultar/voice.on";
const LOG: &str = "/run/ultar/voice.log";
/// Stimmprobe: zählt nur für das Stimmbild, ist kein Befehl
const PRINT: &str = "print";

struct Ear {
    stop: Arc<AtomicBool>,
    rx: Receiver<Vec<i16>>,
    level: Arc<AtomicU32>,
}

impl Drop for Ear {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Learn {
    name: String,
    action: String,
    need: usize,
    got: Vec<Utt>,
    title: String,
    hint: String,
}

#[derive(Default)]
pub struct Voice {
    loaded: bool,
    pub enabled: bool,
    profile: Profile,
    ear: Option<Ear>,
    learn: Option<Learn>,
    queue: Vec<Learn>,
    armed: Option<Instant>,
    /// Stimme passte nicht: bis dahin gilt ein Entsperren als Ausweis
    verify: Option<Instant>,
    was_locked: bool,
}

fn log(line: &str) {
    use std::io::Write;
    if std::fs::metadata(LOG).is_ok_and(|m| m.len() > 60_000) {
        let _ = std::fs::remove_file(LOG);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(LOG) {
        let _ = writeln!(f, "{} {line}", crate::sys::unix_now());
    }
}

impl App {
    fn voice_load(&mut self) {
        if self.voice.loaded {
            return;
        }
        self.voice.loaded = true;
        self.voice.profile = Profile::from_text(&std::fs::read_to_string(PROFILE).unwrap_or_default());
        self.voice.enabled = std::path::Path::new(ENABLED).exists();
    }

    fn voice_save(&self) {
        let _ = std::fs::write(PROFILE, self.voice.profile.to_text());
    }

    pub(super) fn voice_enable(&mut self, on: bool) {
        self.voice.enabled = on;
        if on {
            let _ = std::fs::write(ENABLED, "1\n");
        } else {
            let _ = std::fs::remove_file(ENABLED);
            self.voice.armed = None;
        }
    }

    fn voice_sound(&self, good: bool) {
        let (snd, _, _) = self.sys_alert(true, self.set.sound, 0);
        if snd {
            crate::audio::play(if good { vec![(1319.0, 70), (1760.0, 90)] } else { vec![(330.0, 160), (262.0, 220)] }, self.set.volume * 0.7);
        }
    }

    fn voice_step(&self, name: &str, action: &str, need: usize) -> Learn {
        let (title, hint) = match name {
            voice::WAKE => (self.t("Wake word"), self.t("Say: Hey KM").to_string()),
            voice::SECRET => (self.t("Secret word"), self.t("Say your secret word – one that nobody guesses. With it I listen even when the screen is locked.").to_string()),
            PRINT => (self.t("Your voice"), format!("{}\n„{}“", self.t("Read this sentence aloud so I learn your voice:"), self.t("My assistant listens only to my voice."))),
            _ => (self.t("Voice command"), format!("{}\n„{action}“", self.t("Say it the way you will say it later:"))),
        };
        Learn { name: name.into(), action: action.into(), need, got: Vec::new(), title: title.to_string(), hint }
    }

    /// Einrichtung: Weckwort dreimal, dann die Stimmprobe zweimal.
    pub(super) fn voice_setup(&mut self) {
        self.voice_load();
        if !crate::mic::available() {
            self.buddy.answer = Some(self.t("No microphone found on this device.").to_string());
            return;
        }
        self.voice.queue = vec![self.voice_step(PRINT, "", 2)];
        self.voice.learn = Some(self.voice_step(voice::WAKE, "", 3));
        self.buddy.learning = true;
    }

    /// Einen Satz einlernen (geheimes Wort oder eigener Befehl).
    pub(super) fn voice_learn(&mut self, name: &str, action: &str, need: usize) {
        self.voice_load();
        if !crate::mic::available() {
            self.buddy.answer = Some(self.t("No microphone found on this device.").to_string());
            return;
        }
        if self.voice.profile.find(voice::WAKE).is_none() {
            // erst die Grundeinrichtung, der gewünschte Satz kommt danach
            self.voice_setup();
            let st = self.voice_step(name, action, need);
            self.voice.queue.push(st);
            return;
        }
        self.voice.queue.clear();
        self.voice.learn = Some(self.voice_step(name, action, need));
        self.buddy.learning = true;
    }

    pub(super) fn voice_cancel(&mut self) {
        self.voice.learn = None;
        self.voice.queue.clear();
        self.buddy.learning = false;
    }

    /// Für die Karte: (Titel, Aufforderung, schon aufgenommen, nötig, Pegel)
    pub(super) fn voice_card(&self) -> Option<(String, String, usize, usize, f32)> {
        let l = self.voice.learn.as_ref()?;
        let level = self.voice.ear.as_ref().map(|e| f32::from_bits(e.level.load(Ordering::Relaxed))).unwrap_or(0.0);
        Some((l.title.clone(), l.hint.clone(), l.got.len(), l.need, level))
    }

    fn ear_start(&self) -> Result<Ear, String> {
        let mic = crate::mic::Mic::start()?;
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (tx, rx) = channel();
        let (st, lv, bg) = (stop.clone(), level.clone(), self.bg.clone());
        std::thread::spawn(move || {
            let mut cut = Cutter::default();
            while !st.load(Ordering::Relaxed) {
                let Ok(block) = mic.rx.recv_timeout(Duration::from_secs(3)) else { break };
                // eigene Töne nicht für Sprache halten
                if crate::audio::is_busy() {
                    cut = Cutter::default();
                    continue;
                }
                let done = cut.feed(&block);
                lv.store(cut.level.to_bits(), Ordering::Relaxed);
                if let Some(u) = done {
                    if tx.send(u).is_err() {
                        break;
                    }
                    bg.poke();
                }
            }
        });
        Ok(Ear { stop, rx, level })
    }

    /// Darf diese Anweisung bei gesperrtem Gerät per Stimme laufen?
    fn voice_harmless(action: &str) -> bool {
        match mind::understand(action) {
            Some(Intent::Time | Intent::Reminders | Intent::Remind(..) | Intent::Say(_)) => true,
            Some(Intent::Tool(c)) => !c.risky(),
            _ => false,
        }
    }

    fn voice_run(&mut self, action: String) {
        if self.sx.locked && !Self::voice_harmless(&action) {
            self.voice_sound(false);
            return self.buddy_show(format!("{}\n„{action}“", self.t("Please unlock first – I only do harmless things by voice while the screen is locked.")));
        }
        self.voice_sound(true);
        self.buddy.voice_reply = true;
        self.buddy_run(action.clone());
        if let Some(a) = self.buddy.answer.take() {
            self.buddy.voice_reply = false;
            self.buddy_show(a);
        } else if !self.buddy.native {
            self.buddy.voice_reply = false;
            self.buddy_show(format!("{}: {action}", self.t("Done")));
        }
        self.buddy.open = false;
    }

    fn voice_heard(&mut self, samples: Vec<i16>) -> bool {
        let Some(u) = voice::analyze(&samples) else { return false };
        // ---- Einlernen ----
        if let Some(l) = &mut self.voice.learn {
            l.got.push(u);
            if l.got.len() < l.need {
                return true;
            }
            let l = self.voice.learn.take().unwrap();
            self.voice.profile.enroll(&l.name, &l.action, l.got);
            self.voice_save();
            self.voice_sound(true);
            if self.voice.queue.is_empty() {
                self.buddy.learning = false;
                self.voice_enable(true);
                let msg = match l.name.as_str() {
                    voice::SECRET => self.t("Secret word saved. Say it and I listen – even when the screen is locked.").to_string(),
                    PRINT | voice::WAKE => self.t("Voice control is ready. Say \"Hey KM\", then a command you taught me. Teach me one with: teach voice command flashlight on.").to_string(),
                    _ => format!("{}\n„{}“", self.t("Learned. After \"Hey KM\" say:"), l.action),
                };
                self.buddy_show(msg);
            } else {
                self.voice.learn = Some(self.voice.queue.remove(0));
            }
            return true;
        }
        if !self.voice.enabled {
            return false;
        }
        let locked = self.sx.locked;
        log(&format!("{}{}", if locked { "gesperrt " } else { "" }, self.voice.profile.scores(&u)));
        let special = |n: &str| n == voice::WAKE || n == voice::SECRET || n == PRINT;
        // ---- nach dem Weckwort: Befehl ----
        if self.voice.armed.is_some_and(|t| Instant::now() < t) {
            return match self.voice.profile.hear(&u, |p| !special(&p.name)) {
                Heard::Owner(i) => {
                    self.voice.armed = None;
                    let action = self.voice.profile.phrases[i].action.clone();
                    self.voice_run(action);
                    true
                }
                Heard::Stranger(_) => self.voice_reject(),
                Heard::Nothing => false,
            };
        }
        // ---- Weckwort (nur entsperrt) oder geheimes Wort (immer) ----
        match self.voice.profile.hear(&u, |p| p.name == voice::SECRET || (!locked && p.name == voice::WAKE)) {
            Heard::Owner(_) => {
                self.voice_arm();
                true
            }
            Heard::Stranger(_) => self.voice_reject(),
            Heard::Nothing => false,
        }
    }

    fn voice_arm(&mut self) {
        self.voice.armed = Some(Instant::now() + Duration::from_secs(8));
        self.voice.verify = None;
        self.voice_sound(true);
        let any = self.voice.profile.phrases.iter().any(|p| p.name != voice::WAKE && p.name != voice::SECRET && p.name != PRINT);
        let msg = if any { self.t("I am listening …") } else { self.t("I hear you. Teach me commands with: teach voice command flashlight on.") };
        self.buddy_show(msg.to_string());
    }

    /// Wort stimmt, Stimme nicht: abweisen – der Besitzer kann sich mit Finger, Gesicht oder PIN ausweisen.
    fn voice_reject(&mut self) -> bool {
        self.voice_sound(false);
        self.voice.armed = None;
        self.voice.verify = Some(Instant::now() + Duration::from_secs(30));
        // entsperrt und Sperre eingerichtet: Sperrbildschirm zeigen, damit der Besitzer sich ausweisen kann
        if !self.sx.locked && self.sx.cfg.lock {
            self.sx.locked = true;
        }
        self.voice.was_locked = self.sx.locked;
        self.buddy_show(self.t("You are not my owner – I may not talk to you. If it is you after all (a cold, a hoarse voice): confirm with fingerprint, face or PIN.").to_string());
        true
    }

    /// Uhrwerk: Mikrofon nach Bedarf ein/aus, fertige Äußerungen auswerten. true = es gibt etwas zu zeigen.
    pub(super) fn voice_tick(&mut self, asleep: bool) -> bool {
        self.voice_load();
        let v = &self.voice;
        let secret = v.profile.find(voice::SECRET).is_some();
        // lauschen: beim Einlernen immer; sonst wenn eingeschaltet – bei dunklem/gesperrtem Bildschirm nur mit geheimem Wort
        let want = v.learn.is_some() || (v.enabled && v.profile.find(voice::WAKE).is_some() && !self.call_active() && (secret || !(asleep || self.sx.locked)));
        if want && self.voice.ear.is_none() {
            match self.ear_start() {
                Ok(e) => self.voice.ear = Some(e),
                Err(e) => {
                    log(&format!("Mikrofon: {e}"));
                    if self.voice.learn.is_some() {
                        self.voice_cancel();
                        self.buddy_show(format!("{}: {e}", self.t("Microphone")));
                    }
                    self.voice_enable(false);
                }
            }
        } else if !want {
            self.voice.ear = None;
        }
        let mut show = false;
        // Ausweis nach Abweisung: wurde inzwischen entsperrt?
        if let Some(t) = self.voice.verify {
            if Instant::now() > t {
                self.voice.verify = None;
            } else if self.voice.was_locked && !self.sx.locked {
                self.voice_arm();
                show = true;
            }
            self.voice.was_locked = self.sx.locked;
        }
        let heard: Vec<Vec<i16>> = self.voice.ear.as_ref().map(|e| e.rx.try_iter().collect()).unwrap_or_default();
        for u in heard {
            show |= self.voice_heard(u);
        }
        show
    }
}
