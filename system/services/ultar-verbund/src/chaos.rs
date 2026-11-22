//! Eingebautes Chaos für Tests: ULTAR_VERBUND_CHAOS="abbruch=0.05,kaputt=0.02,verzoegerung=80,absturz=0.01"
//!   abbruch       – Wahrscheinlichkeit je Rahmen, die Verbindung zu kappen
//!   kaputt        – Wahrscheinlichkeit je Rahmen, ein Byte zu verfälschen (Noise erkennt es → Abbruch)
//!   verzoegerung  – zufällige Verzögerung bis zu n ms je Rahmen
//!   absturz       – Wahrscheinlichkeit je Aufgabe, dass der Dienst sich hart beendet (nur im Dienst)
//! Ohne Variable ist alles aus.

use rand::Rng;
use std::sync::OnceLock;

#[derive(Default, Clone, Copy, Debug)]
pub struct Chaos {
    pub abbruch: f64,
    pub kaputt: f64,
    pub verzoegerung: u64,
    pub absturz: f64,
}

pub fn get() -> Chaos {
    static C: OnceLock<Chaos> = OnceLock::new();
    *C.get_or_init(|| {
        let mut c = Chaos::default();
        if let Ok(s) = std::env::var("ULTAR_VERBUND_CHAOS") {
            for t in s.split(',') {
                let mut kv = t.splitn(2, '=');
                let (k, v) = (
                    kv.next().unwrap_or("").trim(),
                    kv.next().unwrap_or("0").trim(),
                );
                match k {
                    "abbruch" => c.abbruch = v.parse().unwrap_or(0.0),
                    "kaputt" => c.kaputt = v.parse().unwrap_or(0.0),
                    "verzoegerung" => c.verzoegerung = v.parse().unwrap_or(0),
                    "absturz" => c.absturz = v.parse().unwrap_or(0.0),
                    _ => {}
                }
            }
        }
        c
    })
}

impl Chaos {
    pub fn aktiv(&self) -> bool {
        self.abbruch > 0.0 || self.kaputt > 0.0 || self.verzoegerung > 0 || self.absturz > 0.0
    }

    /// auf einen ausgehenden Rahmen anwenden; Err = Verbindung „gekappt“
    pub fn rahmen(&self, b: &mut [u8]) -> std::io::Result<()> {
        if !self.aktiv() {
            return Ok(());
        }
        let mut r = rand::thread_rng();
        if self.verzoegerung > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                r.gen_range(0..=self.verzoegerung),
            ));
        }
        if self.abbruch > 0.0 && r.gen_bool(self.abbruch.min(1.0)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "chaos: abbruch",
            ));
        }
        if self.kaputt > 0.0 && !b.is_empty() && r.gen_bool(self.kaputt.min(1.0)) {
            let i = r.gen_range(0..b.len());
            b[i] ^= 0x5a;
        }
        Ok(())
    }

    pub fn absturz_jetzt(&self) -> bool {
        self.absturz > 0.0 && rand::thread_rng().gen_bool(self.absturz.min(1.0))
    }
}
