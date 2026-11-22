//! Bündeln: Aufgaben auf alle freien Kerne im Verbund verteilen.
//! Jedes Gerät bekommt so viele Arbeiter wie freie Kerne. Fällt ein Gerät aus, gehen seine Aufgaben zurück
//! in die Warteschlange; langsame Nachzügler werden am Ende zusätzlich auf freien Geräten gestartet.
//! Jede Aufgabe liefert genau ein Ergebnis (das erste gewinnt), Wiederholungen nutzen dieselbe Kennung (idempotent).

use crate::dienst;
use crate::entdeckung::Gesehen;
use crate::ident::Ident;
use crate::kanal::Kanal;
use crate::proto::{Aufgabe, Ergebnis, Nachricht};
use crate::relay::Relais;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const MAX_VERSUCHE: u32 = 8;

#[derive(Clone)]
pub enum Knoten {
    Lokal { kerne: usize, name: String },
    Fern { g: Gesehen, kerne: usize },
}

impl Knoten {
    pub fn name(&self) -> String {
        match self {
            Knoten::Lokal { name, .. } => format!("{name} (hier)"),
            Knoten::Fern { g, .. } => format!("{} ({})", g.name, g.kennung),
        }
    }
    fn kerne(&self) -> usize {
        match self {
            Knoten::Lokal { kerne, .. } | Knoten::Fern { kerne, .. } => (*kerne).max(1),
        }
    }
}

struct Stand {
    offen: VecDeque<usize>,
    laufend: HashMap<usize, (Instant, u32)>,
    fertig: Vec<Option<Ergebnis>>,
    versuche: Vec<u32>,
    rest: usize,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct Statistik {
    pub je_knoten: HashMap<String, usize>,
    pub wiederholt: usize,
    pub spekulativ: usize,
    pub ausgefallen: Vec<String>,
    pub lokal_nachgeholt: usize,
    pub dauer_ms: u64,
}

pub struct Planer {
    ich: Ident,
    relais: Option<Arc<Relais>>,
    aufgaben: Vec<Aufgabe>,
    s: Mutex<Stand>,
    cv: Condvar,
    stat: Mutex<Statistik>,
    pub leise: bool,
}

impl Planer {
    pub fn neu(ich: Ident, relais: Option<Arc<Relais>>, aufgaben: Vec<Aufgabe>) -> Arc<Planer> {
        let n = aufgaben.len();
        Arc::new(Planer {
            ich,
            relais,
            s: Mutex::new(Stand {
                offen: (0..n).collect(),
                laufend: HashMap::new(),
                fertig: vec![None; n],
                versuche: vec![0; n],
                rest: n,
            }),
            aufgaben,
            cv: Condvar::new(),
            stat: Mutex::new(Statistik::default()),
            leise: false,
        })
    }

    fn holen(&self) -> Option<usize> {
        let mut s = self.s.lock().unwrap();
        loop {
            if s.rest == 0 {
                return None;
            }
            if let Some(i) = s.offen.pop_front() {
                if s.fertig[i].is_some() {
                    continue;
                }
                let e = s.laufend.entry(i).or_insert((Instant::now(), 0));
                e.1 += 1;
                return Some(i);
            }
            // Nachzügler doppelt starten (einmal), wenn sie schon > 3 s laufen
            let kandidat = s
                .laufend
                .iter()
                .filter(|(i, (t, n))| {
                    *n == 1 && s.fertig[**i].is_none() && t.elapsed() > Duration::from_secs(3)
                })
                .map(|(i, _)| *i)
                .next();
            if let Some(i) = kandidat {
                s.laufend.get_mut(&i).unwrap().1 += 1;
                self.stat.lock().unwrap().spekulativ += 1;
                return Some(i);
            }
            s = self
                .cv
                .wait_timeout(s, Duration::from_millis(100))
                .unwrap()
                .0;
        }
    }

    fn fertig(&self, i: usize, e: Ergebnis, knoten: &str) {
        let mut s = self.s.lock().unwrap();
        if let Some(l) = s.laufend.get_mut(&i) {
            l.1 = l.1.saturating_sub(1);
            if l.1 == 0 {
                s.laufend.remove(&i);
            }
        }
        if s.fertig[i].is_none() {
            s.fertig[i] = Some(e);
            s.rest -= 1;
            *self
                .stat
                .lock()
                .unwrap()
                .je_knoten
                .entry(knoten.to_string())
                .or_default() += 1;
            if !self.leise && std::env::var("ULTAR_VERBUND_FORTSCHRITT").is_ok() {
                eprintln!(
                    "[verbund] {}/{} fertig ({knoten})",
                    self.aufgaben.len() - s.rest,
                    self.aufgaben.len()
                );
            }
        }
        self.cv.notify_all();
    }

    fn zurueck(&self, i: usize) {
        let mut s = self.s.lock().unwrap();
        let noch_laeuft = match s.laufend.get_mut(&i) {
            Some(l) => {
                l.1 = l.1.saturating_sub(1);
                l.1 > 0
            }
            None => false,
        };
        if !noch_laeuft {
            s.laufend.remove(&i);
        }
        if s.fertig[i].is_none() && !noch_laeuft {
            s.versuche[i] += 1;
            self.stat.lock().unwrap().wiederholt += 1;
            if s.versuche[i] >= MAX_VERSUCHE {
                let a = &self.aufgaben[i];
                s.fertig[i] = Some(Ergebnis {
                    id: a.id.clone(),
                    code: -3,
                    ausgabe: String::new(),
                    fehler: "aufgegeben nach zu vielen Ausfällen".into(),
                    dauer_ms: 0,
                    knoten: String::new(),
                });
                s.rest -= 1;
            } else {
                s.offen.push_front(i);
            }
        }
        self.cv.notify_all();
    }

    fn arbeiter(self: Arc<Self>, k: Knoten) {
        let name = k.name();
        let mut kanal: Option<Kanal> = None;
        let mut fehler_folge = 0u32;
        while let Some(i) = self.holen() {
            let a = self.aufgaben[i].clone();
            match &k {
                Knoten::Lokal { .. } => {
                    let e = dienst::ausfuehren(&a, &self.ich.kennung());
                    self.fertig(i, e, &name);
                }
                Knoten::Fern { g, .. } => {
                    let r = (|| -> std::io::Result<Nachricht> {
                        if kanal.is_none() {
                            kanal = Some(dienst::verbinden(&self.ich, g, self.relais.as_ref())?);
                        }
                        let kn = kanal.as_mut().unwrap();
                        // Lebenszeichen kommen alle 2 s; 8 s Stille = Gerät weg
                        kn.zeitlimit(Some(Duration::from_secs(8)));
                        kn.nachricht_senden(&Nachricht::Rechnen(a.clone()))?;
                        loop {
                            match kn.nachricht_empfangen()? {
                                Nachricht::Laeuft => {
                                    if self.s.lock().unwrap().fertig[i].is_some() {
                                        // anderswo schon fertig (doppelt gestartet): Verbindung aufgeben
                                        return Err(std::io::Error::other("schon fertig"));
                                    }
                                }
                                n => return Ok(n),
                            }
                        }
                    })();
                    match r {
                        Ok(Nachricht::Ergebnis(e)) if e.id == a.id => {
                            fehler_folge = 0;
                            self.fertig(i, e, &name);
                        }
                        Ok(Nachricht::Belegt) => {
                            self.zurueck_ohne_versuch(i);
                            std::thread::sleep(Duration::from_millis(150));
                        }
                        other => {
                            if std::env::var("ULTAR_VERBUND_LOG").is_ok() {
                                eprintln!(
                                    "[verbund] {name}: {:?}",
                                    other.as_ref().map(|_| "antwort").map_err(|e| e.to_string())
                                );
                            }
                            kanal = None;
                            fehler_folge += 1;
                            self.zurueck(i);
                            if fehler_folge >= 4 {
                                let mut st = self.stat.lock().unwrap();
                                if !st.ausgefallen.contains(&name) {
                                    st.ausgefallen.push(name.clone());
                                }
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(
                                100 * (1 << fehler_folge.min(4)),
                            ));
                        }
                    }
                }
            }
        }
    }

    fn zurueck_ohne_versuch(&self, i: usize) {
        let mut s = self.s.lock().unwrap();
        let noch = match s.laufend.get_mut(&i) {
            Some(l) => {
                l.1 = l.1.saturating_sub(1);
                l.1 > 0
            }
            None => false,
        };
        if !noch {
            s.laufend.remove(&i);
            if s.fertig[i].is_none() {
                s.offen.push_back(i);
            }
        }
        self.cv.notify_all();
    }

    /// alles ausführen; liefert Ergebnisse in Aufgabenreihenfolge
    pub fn ausfuehren(self: &Arc<Self>, knoten: Vec<Knoten>) -> (Vec<Ergebnis>, Statistik) {
        let t = Instant::now();
        let mut h = Vec::new();
        for k in knoten {
            for _ in 0..k.kerne() {
                let p = self.clone();
                let k = k.clone();
                h.push(std::thread::spawn(move || p.arbeiter(k)));
            }
        }
        for x in h {
            let _ = x.join();
        }
        // alle Geräte weg? Rest hier rechnen
        let rest: Vec<usize> = {
            let s = self.s.lock().unwrap();
            (0..self.aufgaben.len())
                .filter(|i| s.fertig[*i].is_none())
                .collect()
        };
        for i in rest {
            let e = dienst::ausfuehren(&self.aufgaben[i], &self.ich.kennung());
            self.stat.lock().unwrap().lokal_nachgeholt += 1;
            self.fertig(i, e, "hier (Ersatz)");
        }
        let s = self.s.lock().unwrap();
        let mut st = self.stat.lock().unwrap().clone();
        st.dauer_ms = t.elapsed().as_millis() as u64;
        (s.fertig.iter().map(|e| e.clone().unwrap()).collect(), st)
    }
}
