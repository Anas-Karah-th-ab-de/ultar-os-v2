//! Der Verbund-Dienst: meldet das Gerät, nimmt verschlüsselte Verbindungen an (direkt + Relais),
//! führt Aufgaben aus (idempotent, mit Zeitlimit) und schreibt den Zustand für CLI/Oberfläche.

use crate::config::{Config, Partner, Rolle};
use crate::entdeckung::{self, Gesehen, Meldung, Tabelle};
use crate::ident::{self, Ident};
use crate::kanal::{Kanal, TcpTransport, Transport};
use crate::proto::{Aufgabe, Ergebnis, Nachricht, Zustand};
use base64::Engine;
use crate::relay::{Anmeldung, Relais};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

pub fn log(s: &str) {
    if std::env::var("ULTAR_VERBUND_LOG").map(|v| v != "0").unwrap_or(true) {
        eprintln!("[verbund] {s}");
    }
}

// ---------------- Ausführen ----------------

const MAX_AUSGABE: usize = 16 << 20;

/// Befehl mit Eingabe und Zeitlimit ausführen
pub fn ausfuehren(a: &Aufgabe, knoten: &str) -> Ergebnis {
    let t = Instant::now();
    let mut e = Ergebnis { id: a.id.clone(), code: -1, ausgabe: String::new(), fehler: String::new(), dauer_ms: 0, knoten: knoten.into() };
    if a.befehl.is_empty() {
        e.fehler = "leerer Befehl".into();
        return e;
    }
    let mut cmd = Command::new(&a.befehl[0]);
    cmd.args(&a.befehl[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    ohne_netz(&mut cmd);
    let kind = cmd.spawn();
    let mut kind = match kind {
        Ok(k) => k,
        Err(x) => {
            e.fehler = format!("start: {x}");
            e.code = 127;
            return e;
        }
    };
    let eingabe = a.eingabe.clone().into_bytes();
    let mut stdin = kind.stdin.take().unwrap();
    let w = std::thread::spawn(move || {
        let _ = stdin.write_all(&eingabe);
    });
    let mut so = kind.stdout.take().unwrap();
    let mut se = kind.stderr.take().unwrap();
    let ro = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = (&mut so).take(MAX_AUSGABE as u64).read_to_end(&mut v);
        v
    });
    let re = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = (&mut se).take(1 << 20).read_to_end(&mut v);
        v
    });
    let limit = if a.zeit_ms == 0 { Duration::from_secs(600) } else { Duration::from_millis(a.zeit_ms) };
    let code = loop {
        match kind.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(-1),
            Ok(None) if t.elapsed() > limit => {
                let _ = kind.kill();
                let _ = kind.wait();
                e.fehler = "zeitlimit".into();
                break -2;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break -1,
        }
    };
    let _ = w.join();
    e.ausgabe = String::from_utf8_lossy(&ro.join().unwrap_or_default()).into_owned();
    let f = String::from_utf8_lossy(&re.join().unwrap_or_default()).into_owned();
    if e.fehler.is_empty() {
        e.fehler = f;
    }
    e.code = code;
    e.dauer_ms = t.elapsed().as_millis() as u64;
    e
}

// ---------------- Zustand ----------------

type Platz = Arc<(Mutex<Option<Ergebnis>>, Condvar)>;

pub struct Dienst {
    pub ich: Ident,
    pub dir: PathBuf,
    pub cfg: RwLock<Config>,
    pub netz: Tabelle,
    pub belegt: AtomicUsize,
    cache: Mutex<(HashMap<String, Ergebnis>, VecDeque<String>)>,
    laufend: Mutex<HashMap<String, Platz>>,
    pub relais: Mutex<Option<Arc<Relais>>>,
    pub erledigt: AtomicUsize,
    cfg_zeit: Mutex<Option<std::time::SystemTime>>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct ZustandDatei {
    pub ich: Option<Zustand>,
    pub geraete: Vec<Gesehen>,
    pub relais: bool,
    pub erledigt: usize,
    pub zeit_ms: u64,
    /// Mining-Zustand („läuft“, „Gerät wird benutzt“, …)
    #[serde(default)]
    pub mining: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct KoppelCode {
    pub code: String,
    pub bis_ms: u64,
    pub rolle: Rolle,
    #[serde(default)]
    pub fehlversuche: u32,
}

pub fn koppel_mac(code: &str, a: &[u8], b: &[u8]) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(code.as_bytes()).unwrap();
    m.update(a);
    m.update(b);
    ident::hex(&m.finalize().into_bytes())
}

impl Dienst {
    pub fn zustand(&self) -> Zustand {
        let c = self.cfg.read().unwrap();
        let kerne = c.kerne();
        Zustand {
            kennung: self.ich.kennung(),
            name: c.name.clone(),
            ausgabe: crate::ressourcen::ausgabe(),
            kerne,
            frei: kerne.saturating_sub(self.belegt.load(Ordering::SeqCst)),
            last: crate::ressourcen::last(),
            ram_frei_mb: crate::ressourcen::ram_frei_mb(),
            aktiv: c.aktiv,
            version: crate::VERSION.into(),
        }
    }

    pub fn alle_geraete(&self) -> Vec<Gesehen> {
        let c = self.cfg.read().unwrap();
        let mut m: HashMap<String, Gesehen> = HashMap::new();
        if let Some(r) = &*self.relais.lock().unwrap() {
            for (k, g) in r.geraete.lock().unwrap().iter() {
                if k != &self.ich.pk_hex() {
                    m.insert(k.clone(), g.clone());
                }
            }
        }
        // direkt gesehene haben Vorrang (schneller)
        for (k, g) in self.netz.lock().unwrap().iter() {
            m.insert(k.clone(), g.clone());
        }
        let mut v: Vec<Gesehen> = m
            .into_values()
            .map(|mut g| {
                g.vertraut = c.partner(&g.pk).is_some();
                g
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    fn schreibe_zustand(&self) {
        let z = ZustandDatei {
            ich: Some(self.zustand()),
            geraete: self.alle_geraete(),
            relais: self.relais.lock().unwrap().as_ref().map(|r| r.verbunden.load(Ordering::SeqCst)).unwrap_or(false),
            erledigt: self.erledigt.load(Ordering::SeqCst),
            zeit_ms: crate::jetzt_ms(),
            mining: crate::mining::MINER.try_lock().map(|m| m.grund.clone()).unwrap_or_default(),
        };
        let p = self.dir.join("zustand.json");
        let tmp = self.dir.join("zustand.json.neu");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&z).unwrap()).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }

    /// Aufgabe idempotent ausführen: gleiche (Auftraggeber, id) → gleiches Ergebnis, nur einmal gerechnet
    fn rechnen(&self, von: &str, a: &Aufgabe) -> Nachricht {
        let key = format!("{von}/{}", a.id);
        if let Some(e) = self.cache.lock().unwrap().0.get(&key) {
            return Nachricht::Ergebnis(e.clone());
        }
        let (platz, neu) = {
            let mut l = self.laufend.lock().unwrap();
            match l.get(&key) {
                Some(p) => (p.clone(), false),
                None => {
                    let kerne = self.cfg.read().unwrap().kerne();
                    if self.belegt.load(Ordering::SeqCst) >= kerne {
                        return Nachricht::Belegt;
                    }
                    self.belegt.fetch_add(1, Ordering::SeqCst);
                    // Verbund-Aufgaben haben Vorrang vor dem Mining
                    if let Ok(mut m) = crate::mining::MINER.try_lock() {
                        if m.laeuft() {
                            m.stoppen();
                            m.grund = "Verbund-Aufgaben haben Vorrang".into();
                        }
                    }
                    let p: Platz = Arc::new((Mutex::new(None), Condvar::new()));
                    l.insert(key.clone(), p.clone());
                    (p, true)
                }
            }
        };
        if !neu {
            let (m, cv) = &*platz;
            let mut g = m.lock().unwrap();
            while g.is_none() {
                g = cv.wait(g).unwrap();
            }
            return Nachricht::Ergebnis(g.clone().unwrap());
        }
        let e = ausfuehren(a, &self.ich.kennung());
        if crate::chaos::get().absturz_jetzt() {
            log("chaos: absturz während einer Aufgabe");
            std::process::exit(137);
        }
        {
            let mut c = self.cache.lock().unwrap();
            c.0.insert(key.clone(), e.clone());
            c.1.push_back(key.clone());
            while c.1.len() > 4096 {
                if let Some(alt) = c.1.pop_front() {
                    c.0.remove(&alt);
                }
            }
        }
        self.laufend.lock().unwrap().remove(&key);
        self.belegt.fetch_sub(1, Ordering::SeqCst);
        self.erledigt.fetch_add(1, Ordering::SeqCst);
        let (m, cv) = &*platz;
        *m.lock().unwrap() = Some(e.clone());
        cv.notify_all();
        Nachricht::Ergebnis(e)
    }

    fn koppeln(&self, partner_pk: &[u8], name: &str, mac: &str) -> Nachricht {
        let p = self.dir.join("koppeln.json");
        let Some(mut kc) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<KoppelCode>(&s).ok()) else {
            return Nachricht::Fehler { text: "keine Kopplung offen".into() };
        };
        if crate::jetzt_ms() > kc.bis_ms {
            let _ = std::fs::remove_file(&p);
            return Nachricht::Fehler { text: "Code abgelaufen".into() };
        }
        let erwartet = koppel_mac(&kc.code, partner_pk, &self.ich.oeffentlich);
        if erwartet != mac {
            kc.fehlversuche += 1;
            if kc.fehlversuche >= 5 {
                let _ = std::fs::remove_file(&p);
                log("kopplung: zu viele Fehlversuche – Code verworfen");
            } else {
                let _ = std::fs::write(&p, serde_json::to_vec(&kc).unwrap());
            }
            return Nachricht::Fehler { text: "falscher Code".into() };
        }
        let _ = std::fs::remove_file(&p);
        let pk = ident::hex(partner_pk);
        let partner = Partner { kennung: ident::kennung(partner_pk), pk, name: name.chars().take(64).collect(), rolle: kc.rolle, quelle: "code".into() };
        {
            let mut c = self.cfg.write().unwrap();
            // frisch von der Platte, damit parallele CLI-Änderungen nicht verloren gehen
            let mut neu = Config::datei(&self.dir);
            neu.hinzufuegen(partner.clone());
            let _ = neu.speichern(&self.dir);
            c.partner = neu.partner;
            *self.cfg_zeit.lock().unwrap() = None;
        }
        log(&format!("gekoppelt mit {} ({})", partner.name, partner.kennung));
        let name = self.cfg.read().unwrap().name.clone();
        Nachricht::KoppelnOk { name, mac: koppel_mac(&kc.code, &self.ich.oeffentlich, partner_pk) }
    }

    /// Einstellungen neu laden, wenn die Datei sich geändert hat (an/aus, Partner, Relais) – vor jeder Vertrauensentscheidung
    pub fn frisch(&self) {
        let m = std::fs::metadata(crate::config::pfad(&self.dir)).and_then(|m| m.modified()).ok();
        let kern = std::path::Path::new("/proc/ultar/verbund").exists();
        {
            let mut z = self.cfg_zeit.lock().unwrap();
            if *z == m && !kern {
                return;
            }
            *z = m;
        }
        let neu = Config::laden(&self.dir);
        let alt_aktiv = self.cfg.read().unwrap().aktiv;
        if neu.aktiv != alt_aktiv {
            log(if neu.aktiv { "Verbund AN" } else { "Verbund AUS" });
            if !neu.aktiv {
                self.netz.lock().unwrap().clear();
            }
        }
        *self.cfg.write().unwrap() = neu;
    }

    /// Aufgabe rechnen und dabei alle 2 s „Laeuft“ senden; None = Verbindung weg
    fn mit_lebenszeichen(self: &Arc<Self>, k: &mut Kanal, pk: &str, a: &Aufgabe) -> Option<Nachricht> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (d, pk2, a2) = (self.clone(), pk.to_string(), a.clone());
        std::thread::spawn(move || {
            let _ = tx.send(d.rechnen(&pk2, &a2));
        });
        loop {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(n) => return Some(n),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if k.nachricht_senden(&Nachricht::Laeuft).is_err() {
                        return None; // Ergebnis landet im Zwischenspeicher – Wiederholung bekommt es sofort
                    }
                }
                Err(_) => return Some(Nachricht::Fehler { text: "intern".into() }),
            }
        }
    }

    /// eine angenommene Verbindung bedienen
    pub fn bedienen(self: &Arc<Self>, t: Box<dyn Transport>) {
        self.frisch();
        if !self.cfg.read().unwrap().aktiv {
            return;
        }
        let mut k = match Kanal::annehmen(t, &self.ich) {
            Ok(k) => k,
            Err(_) => return,
        };
        let pk = ident::hex(&k.partner_pk);
        let mut sitzung = Sitzung::default();
        loop {
            k.zeitlimit(Some(Duration::from_secs(120)));
            let m = match k.nachricht_empfangen() {
                Ok(m) => m,
                Err(_) => return,
            };
            self.frisch();
            let (aktiv, partner, steuern, rechnen_ok, c) = {
                let c = self.cfg.read().unwrap();
                (c.aktiv, c.partner(&pk).cloned(), c.steuern_erlauben, c.rechnen_erlauben, c.clone())
            };
            if !aktiv {
                let _ = k.nachricht_senden(&Nachricht::Fehler { text: "Verbund ist aus".into() });
                return;
            }
            let antwort = match (&m, &partner) {
                (Nachricht::Koppeln { name, mac }, _) => self.koppeln(&k.partner_pk, name, mac),
                (_, None) => {
                    let _ = k.nachricht_senden(&Nachricht::Fehler { text: "nicht gekoppelt".into() });
                    log(&format!("abgewiesen: {} ist nicht gekoppelt ({})", ident::kennung(&k.partner_pk), k.weg()));
                    return;
                }
                (Nachricht::Zustand, Some(_)) => Nachricht::ZustandAntwort(self.zustand()),
                (Nachricht::Rechnen(a), Some(_)) => {
                    if rechnen_ok {
                        match self.mit_lebenszeichen(&mut k, &pk, a) {
                            Some(n) => n,
                            None => return,
                        }
                    } else {
                        Nachricht::Fehler { text: "Rechnen auf diesem Gerät nicht erlaubt".into() }
                    }
                }
                (Nachricht::Steuern(a), Some(p)) => {
                    if steuern && p.rolle == Rolle::Voll {
                        log(&format!("steuern von {}: {:?}", p.name, a.befehl));
                        match self.mit_lebenszeichen(&mut k, &pk, a) {
                            Some(n) => n,
                            None => return,
                        }
                    } else {
                        Nachricht::Fehler { text: "Steuern nicht erlaubt".into() }
                    }
                }
                (m, Some(p)) => self.weitere(m, p, &c, &mut sitzung),
            };
            if k.nachricht_senden(&antwort).is_err() {
                return;
            }
        }
    }
}

// ---------------- Teilen, Ton, Weitergeben, Mining ----------------

/// Zustand einer Verbindung (Bild teilen, Ton)
#[derive(Default)]
pub struct Sitzung {
    bild: Option<crate::bildschirm::Bild>,
    gemeldet: bool,
    aufnahme: Option<crate::ton::Aufnahme>,
    wiedergabe: Option<crate::ton::Wiedergabe>,
}

impl Dienst {
    fn weitere(self: &Arc<Self>, m: &Nachricht, p: &Partner, c: &Config, s: &mut Sitzung) -> Nachricht {
        use crate::bildschirm as bs;
        let voll = p.rolle == Rolle::Voll;
        let nein = |t: &str| Nachricht::Fehler { text: format!("{t} auf diesem Gerät nicht erlaubt") };
        let b64 = base64::engine::general_purpose::STANDARD;
        match m {
            Nachricht::Teilen | Nachricht::Fern { .. } | Nachricht::Aktion { .. } if !(voll && c.bildschirm_erlauben) => nein("Bildschirm teilen"),
            Nachricht::Teilen => {
                if let Err(e) = bs::oberflaeche_sicherstellen() {
                    return Nachricht::Fehler { text: e };
                }
                bs::anfordern();
                let _ = bs::an_oberflaeche("bildschirm an");
                if !s.gemeldet {
                    s.gemeldet = true;
                    log(&format!("bildschirm geteilt mit {}", p.name));
                    crate::weitergeben::hinweis("Bildschirm wird geteilt", &format!("{} sieht und bedient diesen Bildschirm", p.name));
                }
                s.bild = None;
                Nachricht::Ok { text: "teilen".into() }
            }
            Nachricht::Fern { eingaben, warte_ms, voll: ganz } => {
                bs::anfordern();
                bs::benutzt();
                for z in eingaben.iter().filter(|z| bs::eingabe_erlaubt(z)) {
                    let _ = bs::an_oberflaeche(z);
                }
                // auf ein neues Bild warten (höchstens warte_ms, max. 2 s)
                let alt_nr = s.bild.as_ref().map(|b| b.nr).unwrap_or(0);
                let t = Instant::now();
                let ende = Duration::from_millis((*warte_ms).min(2000) as u64);
                let mut neu = None;
                loop {
                    if let Some(b) = bs::lesen() {
                        if b.nr != alt_nr || *ganz || s.bild.is_none() {
                            neu = Some(b);
                            break;
                        }
                    }
                    if t.elapsed() >= ende {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                match neu {
                    Some(b) => {
                        let daten = bs::kodieren(&b, s.bild.as_ref(), *ganz);
                        let antwort = Nachricht::Bild { w: b.w as u32, h: b.h as u32, nr: b.nr, daten: b64.encode(daten) };
                        s.bild = Some(b);
                        antwort
                    }
                    None => match &s.bild {
                        Some(b) => Nachricht::Bild { w: b.w as u32, h: b.h as u32, nr: b.nr, daten: String::new() },
                        None => Nachricht::Fehler { text: "noch kein Bild (Oberfläche startet)".into() },
                    },
                }
            }
            Nachricht::Aktion { zeile } => {
                if !bs::eingabe_erlaubt(zeile) {
                    return Nachricht::Fehler { text: "unbekannte Aktion".into() };
                }
                if let Err(e) = bs::oberflaeche_sicherstellen() {
                    return Nachricht::Fehler { text: e };
                }
                bs::benutzt();
                log(&format!("aktion von {}: {zeile}", p.name));
                match bs::an_oberflaeche(zeile) {
                    Ok(()) => Nachricht::Ok { text: format!("ausgeführt: {zeile}") },
                    Err(e) => Nachricht::Fehler { text: e },
                }
            }
            Nachricht::Datei { name, daten, oeffnen } => {
                if !c.dateien_erlauben {
                    return nein("Dateien annehmen");
                }
                let roh = match b64.decode(daten) {
                    Ok(r) => r,
                    Err(_) => return Nachricht::Fehler { text: "Daten kaputt".into() },
                };
                match crate::weitergeben::ablegen(name, &roh) {
                    Ok(pfad) => {
                        log(&format!("datei von {}: {}", p.name, pfad.display()));
                        crate::weitergeben::hinweis(&format!("Von {}", p.name), &pfad.display().to_string());
                        if *oeffnen && bs::oberflaeche_laeuft() {
                            let _ = bs::an_oberflaeche(&format!("datei {}", pfad.display()));
                        }
                        Nachricht::Ok { text: pfad.display().to_string() }
                    }
                    Err(e) => Nachricht::Fehler { text: e },
                }
            }
            Nachricht::Ansehen => {
                if !(voll && c.bildschirm_erlauben) {
                    return nein("Ansicht öffnen");
                }
                if !bs::oberflaeche_laeuft() {
                    return Nachricht::Fehler { text: "dieses Gerät hat keine Oberfläche zum Anzeigen".into() };
                }
                // Ansicht des Absenders öffnen: eigener Prozess, die Oberfläche bekommt die Adresse
                let exe = std::env::current_exe().unwrap_or_else(|_| "ultar-verbund".into());
                let r = Command::new(exe)
                    .args(["anzeigen", &p.kennung, "--oeffnen"])
                    .env("ULTAR_VERBUND_DIR", &self.dir)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                match r {
                    Ok(_) => Nachricht::Ok { text: format!("zeige {}", p.name) },
                    Err(e) => Nachricht::Fehler { text: e.to_string() },
                }
            }
            Nachricht::TonHoeren | Nachricht::TonWeiter | Nachricht::TonDaten { .. } if !(voll && c.ton_erlauben) => nein("Ton"),
            Nachricht::TonHoeren => match crate::ton::Aufnahme::start() {
                Ok(a) => {
                    log(&format!("{} hört mit", p.name));
                    crate::weitergeben::hinweis("Mikrofon geteilt", &format!("{} hört dieses Gerät", p.name));
                    s.aufnahme = Some(a);
                    Nachricht::Ok { text: "hören".into() }
                }
                Err(e) => Nachricht::Fehler { text: e },
            },
            Nachricht::TonWeiter => match &s.aufnahme {
                Some(a) => Nachricht::TonDaten { pcm: b64.encode(a.holen(250)) },
                None => Nachricht::Fehler { text: "erst TonHoeren".into() },
            },
            Nachricht::TonDaten { pcm } => {
                if s.wiedergabe.is_none() {
                    match crate::ton::Wiedergabe::start() {
                        Ok(w) => {
                            log(&format!("durchsage von {}", p.name));
                            s.wiedergabe = Some(w);
                        }
                        Err(e) => return Nachricht::Fehler { text: e },
                    }
                }
                let roh = b64.decode(pcm).unwrap_or_default();
                match s.wiedergabe.as_mut().unwrap().spielen(&roh) {
                    Ok(()) => Nachricht::Ok { text: String::new() },
                    Err(e) => {
                        s.wiedergabe = None;
                        Nachricht::Fehler { text: e }
                    }
                }
            }
            Nachricht::Mining { konfig, einrichten } => {
                if !(voll && c.steuern_erlauben) {
                    return nein("Mining einrichten");
                }
                if let Some(v) = konfig {
                    let mut k = crate::mining::Konfig::laden(&self.dir);
                    let mut j = serde_json::to_value(&k).unwrap();
                    if let (Some(a), Some(b)) = (j.as_object_mut(), v.as_object()) {
                        for (x, y) in b {
                            a.insert(x.clone(), y.clone());
                        }
                    }
                    match serde_json::from_value::<crate::mining::Konfig>(j) {
                        Ok(neu) => {
                            if neu.aktiv {
                                if let Err(e) = neu.pruefen() {
                                    return Nachricht::Fehler { text: e };
                                }
                            }
                            k = neu;
                            let _ = k.speichern(&self.dir);
                            log(&format!("mining-einstellungen von {}", p.name));
                        }
                        Err(e) => return Nachricht::Fehler { text: e.to_string() },
                    }
                }
                if *einrichten && !crate::mining::installiert(&self.dir) {
                    let dir = self.dir.clone();
                    std::thread::spawn(move || match crate::mining::installieren(&dir) {
                        Ok(t) => log(&format!("mining: {t}")),
                        Err(e) => log(&format!("mining: einrichten fehlgeschlagen: {e}")),
                    });
                }
                Nachricht::MiningStatus { status: serde_json::to_value(crate::mining::status(&self.dir)).unwrap() }
            }
            _ => Nachricht::Fehler { text: "unerwartet".into() },
        }
    }
}

// ---------------- Hauptschleife ----------------

pub fn starten(dir: PathBuf) -> std::io::Result<()> {
    let ich = Ident::laden(&dir)?;
    let cfg = Config::laden(&dir);
    let d = Arc::new(Dienst {
        ich,
        dir: dir.clone(),
        cfg: RwLock::new(cfg.clone()),
        netz: Default::default(),
        belegt: AtomicUsize::new(0),
        cache: Mutex::new((HashMap::new(), VecDeque::new())),
        laufend: Mutex::new(HashMap::new()),
        relais: Mutex::new(None),
        erledigt: AtomicUsize::new(0),
        cfg_zeit: Mutex::new(None),
    });
    log(&format!("Ultar-Verbund {} – {} „{}“ ({}), {}", crate::VERSION, d.ich.kennung(), cfg.name, crate::ressourcen::ausgabe(), if cfg.aktiv { "AN" } else { "aus" }));
    if crate::chaos::get().aktiv() {
        log(&format!("chaos aktiv: {:?}", crate::chaos::get()));
    }

    // TCP
    let l = TcpListener::bind(("0.0.0.0", cfg.tcp_port))?;
    {
        let d = d.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let _ = s.set_nodelay(true);
                let d = d.clone();
                std::thread::spawn(move || d.bedienen(Box::new(TcpTransport(s))));
            }
        });
    }

    // UDP-Meldungen
    let u = entdeckung::socket(cfg.udp_port)?;
    {
        let d = d.clone();
        let u2 = u.try_clone()?;
        std::thread::spawn(move || {
            let mut b = [0u8; 2048];
            loop {
                if let Ok((n, von)) = u2.recv_from(&mut b) {
                    let c = d.cfg.read().unwrap();
                    if !c.aktiv {
                        continue;
                    }
                    let neu = !d.netz.lock().unwrap().contains_key(&{
                        serde_json::from_slice::<Meldung>(&b[..n]).map(|m| m.pk).unwrap_or_default()
                    });
                    if let Some(g) = entdeckung::verarbeiten(&d.netz, &d.ich.pk_hex(), &b[..n], von, |pk| c.partner(pk).is_some()) {
                        if neu {
                            log(&format!("gefunden: {} „{}“ ({}) {} {}", g.kennung, g.name, g.ausgabe, g.adresse, if g.vertraut { "vertraut" } else { "nicht gekoppelt" }));
                        }
                    }
                }
            }
        });
    }

    // Relais: eingehende Sitzungen bedienen
    let (etx, erx) = std::sync::mpsc::channel::<crate::relay::RelaisTransport>();
    {
        let d = d.clone();
        std::thread::spawn(move || {
            for t in erx {
                let d = d.clone();
                std::thread::spawn(move || d.bedienen(Box::new(t)));
            }
        });
    }

    let mut letzte_meldung = Instant::now() - Duration::from_secs(10);
    let mut letzter_abgleich = Instant::now() - Duration::from_secs(3600);
    let takt = std::env::var("ULTAR_VERBUND_TAKT").is_ok();
    let mut letzte_wartung = Instant::now();
    loop {
        let runde = Instant::now();
        d.frisch();
        let c = d.cfg.read().unwrap().clone();

        // Relais nach Bedarf
        {
            let mut r = d.relais.lock().unwrap();
            let soll = c.aktiv && !c.token.is_empty() && !c.relais.is_empty();
            match (&*r, soll) {
                (None, true) => {
                    let d2 = d.clone();
                    *r = Some(Relais::starten(
                        Anmeldung {
                            url: c.relais.clone(),
                            token: c.token.clone(),
                            pk: d.ich.pk_hex(),
                            name: c.name.clone(),
                            ausgabe: crate::ressourcen::ausgabe(),
                            rolle: "dienst".into(),
                            kerne: c.kerne(),
                        },
                        Some(etx.clone()),
                        Arc::new(move || d2.zustand().frei),
                    ));
                    log(&format!("relais: {}", c.relais));
                }
                (Some(rl), s) => rl.an.store(s, Ordering::SeqCst),
                _ => {}
            }
        }

        // Kontogeräte abgleichen (alle 10 min)
        if c.aktiv && !c.token.is_empty() && letzter_abgleich.elapsed() > Duration::from_secs(600) {
            letzter_abgleich = Instant::now();
            let d2 = d.clone();
            std::thread::spawn(move || {
                let mut neu = Config::datei(&d2.dir);
                match crate::konto::abgleichen(&mut neu, &d2.ich) {
                    Ok(n) => {
                        let _ = neu.speichern(&d2.dir);
                        log(&format!("konto: {n} Geräte im Konto"));
                    }
                    Err(e) => log(&format!("konto: {e}")),
                }
            });
        }

        if c.aktiv && letzte_meldung.elapsed() >= Duration::from_secs(2) {
            letzte_meldung = Instant::now();
            let z = d.zustand();
            let meld = Meldung { v: 1, k: z.kennung, pk: d.ich.pk_hex(), n: z.name, a: z.ausgabe, p: c.tcp_port, kerne: z.kerne, frei: z.frei, aktiv: true };
            entdeckung::senden(&u, &meld, &entdeckung::ziele(c.udp_port, &c.ziele));
            entdeckung::aufraeumen(&d.netz);
        }
        if letzte_wartung.elapsed() >= Duration::from_secs(5) {
            letzte_wartung = Instant::now();
            crate::bildschirm::aufraeumen(Duration::from_secs(90));
            let (dir2, name, belegt) = (dir.clone(), c.name.clone(), d.belegt.load(Ordering::SeqCst));
            std::thread::spawn(move || crate::mining::steuern(&dir2, &name, belegt));
        }
        d.schreibe_zustand();
        if takt {
            log(&format!("takt {} ms", runde.elapsed().as_millis()));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

// ---------------- Client-Seite ----------------

/// Verbindung zu einem Gerät aufbauen: direkt, sonst über das Relais. Prüft, dass der Schlüssel stimmt.
pub fn verbinden(ich: &Ident, g: &Gesehen, relais: Option<&Arc<Relais>>) -> std::io::Result<Kanal> {
    let pk = ident::unhex(&g.pk).unwrap_or_default();
    let k = if let Ok(a) = g.adresse.parse::<std::net::SocketAddr>() {
        let s = TcpStream::connect_timeout(&a, Duration::from_secs(3))?;
        let _ = s.set_nodelay(true);
        Kanal::verbinden(Box::new(TcpTransport(s)), ich)?
    } else if let Some(r) = relais {
        Kanal::verbinden(Box::new(r.oeffnen(&pk)?), ich)?
    } else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "kein Weg (Relais aus)"));
    };
    if k.partner_pk != pk {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "falscher Geräteschlüssel"));
    }
    Ok(k)
}

pub fn zustand_lesen(dir: &std::path::Path) -> ZustandDatei {
    std::fs::read_to_string(dir.join("zustand.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Kindprozess ohne die Netz-Gruppe starten: darf der Verbund bei „Gerät offline“ ins Internet,
/// bekommen ferngesteuerte Befehle (z. B. Paket-Installationen) trotzdem keins (Internet-Regeln, zelle netz)
pub fn ohne_netz(cmd: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        extern "C" {
            fn getegid() -> u32;
            fn getuid() -> u32;
            fn setgid(g: u32) -> i32;
        }
        unsafe {
            cmd.pre_exec(|| {
                // Netz-Gruppe (47900) gegen die eigene Gruppe des Benutzers tauschen
                if getegid() == 47900 {
                    setgid(getuid());
                }
                Ok(())
            });
        }
    }
}
