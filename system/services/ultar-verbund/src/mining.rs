//! Mining im Leerlauf: ungenutzte Geräte im Verbund schürfen Monero (XMR, RandomX auf dem Prozessor).
//!
//! * Läuft nur, wenn der Nutzer es einschaltet, eine Wallet eingetragen ist und das Gerät ruht:
//!   keine Bedienung seit `leerlauf_min`, keine Verbund-Aufgaben, am Strom (Handy/Laptop), nicht zu heiß.
//!   Jede Bedienung, jede Verbund-Aufgabe, Akku oder Hitze beenden den Miner sofort.
//! * Miner: XMRig (Open Source). Einrichten mit einem Klick lädt die aktuelle Version von GitHub und prüft die
//!   SHA-256-Summe; ARM-Geräte nehmen das Ubuntu-Paket über den Kompatibilitätsbereich.
//! * Ethereum ist seit dem 15.09.2022 Proof-of-Stake und nicht mehr minebar. Für andere Münzen/GPU-Miner
//!   (z. B. Ethereum Classic mit lolMiner) gibt es `eigener_befehl`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const API_PORT: u16 = 47890;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Konfig {
    pub aktiv: bool,
    /// "xmr" (eingebaut) oder "eigen" (eigener_befehl)
    pub waehrung: String,
    pub pool: String,
    pub tls: bool,
    pub wallet: String,
    /// Name dieses Geräts beim Pool (leer = Gerätename)
    pub arbeiter: String,
    pub nur_leerlauf: bool,
    pub leerlauf_min: u32,
    /// Anteil der Kerne in Prozent (1..100)
    pub kerne_prozent: u32,
    pub nur_am_strom: bool,
    pub max_temp_c: u32,
    /// z. B. ["/opt/lolminer/lolMiner", "--algo", "ETCHASH", …] für waehrung = "eigen"
    pub eigener_befehl: Vec<String>,
}

impl Default for Konfig {
    fn default() -> Self {
        Konfig {
            aktiv: false,
            waehrung: "xmr".into(),
            pool: "pool.supportxmr.com:443".into(),
            tls: true,
            wallet: String::new(),
            arbeiter: String::new(),
            nur_leerlauf: true,
            leerlauf_min: 5,
            kerne_prozent: 75,
            nur_am_strom: true,
            max_temp_c: 75,
            eigener_befehl: Vec::new(),
        }
    }
}

pub fn pfad(dir: &Path) -> PathBuf {
    dir.join("mining.json")
}

impl Konfig {
    pub fn laden(dir: &Path) -> Konfig {
        std::fs::read_to_string(pfad(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn speichern(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(pfad(dir), serde_json::to_string_pretty(self).unwrap())
    }
    pub fn pruefen(&self) -> Result<(), String> {
        match self.waehrung.as_str() {
            "xmr" => {
                // Monero-Adressen: Base58, 95 (Standard/Unteradresse) oder 106 Zeichen (integriert), beginnen mit 4 oder 8
                let ok = (self.wallet.len() == 95 || self.wallet.len() == 106)
                    && (self.wallet.starts_with('4') || self.wallet.starts_with('8'))
                    && self.wallet.chars().all(|c| c.is_ascii_alphanumeric() && !"0OIl".contains(c));
                if !ok {
                    return Err("Monero-Wallet-Adresse ungültig (95 oder 106 Zeichen, beginnt mit 4 oder 8)".into());
                }
                if !self.pool.contains(':') {
                    return Err("Pool als host:port angeben".into());
                }
                Ok(())
            }
            "eth" | "ethereum" => Err("Ethereum ist seit 15.09.2022 Proof-of-Stake – Mining ist dort nicht mehr möglich. Für Ethereum Classic einen GPU-Miner als eigener_befehl eintragen.".into()),
            "eigen" if !self.eigener_befehl.is_empty() => Ok(()),
            "eigen" => Err("eigener_befehl fehlt".into()),
            w => Err(format!("unbekannte Währung {w} (xmr oder eigen)")),
        }
    }
}

// ---------------- Leerlauf erkennen ----------------

#[derive(Debug, Clone, Serialize, Default)]
pub struct Lage {
    pub sekunden_ohne_bedienung: u64,
    pub am_strom: bool,
    pub temp_c: Option<f32>,
}

pub fn lage() -> Lage {
    // letzte Bedienung: von der Oberfläche geschrieben (Unix-Sekunden); fehlt sie, gilt das Gerät als unbedient
    let run = crate::bildschirm::run_dir();
    let jetzt = crate::jetzt_ms() / 1000;
    let zuletzt = std::fs::read_to_string(run.join("eingabe.zuletzt")).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    let mut am_strom = true;
    if let Ok(d) = std::fs::read_dir("/sys/class/power_supply") {
        for e in d.flatten() {
            if std::fs::read_to_string(e.path().join("type")).map(|t| t.trim() == "Battery").unwrap_or(false)
                && std::fs::read_to_string(e.path().join("status")).map(|s| s.trim() == "Discharging").unwrap_or(false)
            {
                am_strom = false;
            }
        }
    }
    let temp_c = std::fs::read_dir("/sys/class/thermal")
        .ok()
        .map(|d| d.flatten().filter_map(|e| std::fs::read_to_string(e.path().join("temp")).ok()?.trim().parse::<f32>().ok()).fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.max(v)))))
        .flatten()
        .map(|t| if t > 1000.0 { t / 1000.0 } else { t });
    Lage { sekunden_ohne_bedienung: jetzt.saturating_sub(zuletzt), am_strom, temp_c }
}

/// darf jetzt geschürft werden? (Grund, falls nicht)
pub fn darf(k: &Konfig, l: &Lage, verbund_belegt: usize, laeuft: bool) -> Result<(), String> {
    if !k.aktiv {
        return Err("aus".into());
    }
    k.pruefen()?;
    if k.nur_leerlauf && l.sekunden_ohne_bedienung < k.leerlauf_min as u64 * 60 {
        return Err("Gerät wird benutzt".into());
    }
    if verbund_belegt > 0 {
        return Err("Verbund-Aufgaben haben Vorrang".into());
    }
    if k.nur_am_strom && !l.am_strom {
        return Err("Akkubetrieb".into());
    }
    if let Some(t) = l.temp_c {
        // 5 °C Abstand zum Wiedereinschalten, damit er nicht dauernd an/aus geht
        let grenze = if laeuft { k.max_temp_c as f32 } else { k.max_temp_c as f32 - 5.0 };
        if t >= grenze {
            return Err(format!("zu warm ({t:.0} °C)"));
        }
    }
    Ok(())
}

// ---------------- Miner ----------------

pub fn miner_pfad(dir: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("ULTAR_MINER") {
        return p.into();
    }
    let eigen = dir.join("miner").join("xmrig");
    if eigen.exists() {
        return eigen;
    }
    for p in ["/usr/bin/xmrig", "/compat/usr/bin/xmrig", "/usr/local/bin/xmrig"] {
        if Path::new(p).exists() {
            return p.into();
        }
    }
    eigen
}

pub fn installiert(dir: &Path) -> bool {
    miner_pfad(dir).exists()
}

/// XMRig einrichten: x86_64 = offizielles statisches Paket von GitHub (SHA-256 geprüft), sonst Ubuntu-Paket
pub fn installieren(dir: &Path) -> Result<String, String> {
    if installiert(dir) {
        return Ok(format!("schon eingerichtet: {}", miner_pfad(dir).display()));
    }
    let ziel = dir.join("miner");
    std::fs::create_dir_all(&ziel).map_err(|e| e.to_string())?;
    if cfg!(target_arch = "x86_64") {
        let rel: serde_json::Value = ureq::get("https://api.github.com/repos/xmrig/xmrig/releases/latest")
            .set("user-agent", "ultar-verbund")
            .timeout(Duration::from_secs(30))
            .call()
            .map_err(|e| format!("GitHub: {e}"))?
            .into_json()
            .map_err(|e| e.to_string())?;
        let assets = rel["assets"].as_array().cloned().unwrap_or_default();
        let url_von = |f: &dyn Fn(&str) -> bool| assets.iter().find(|a| f(a["name"].as_str().unwrap_or(""))).and_then(|a| a["browser_download_url"].as_str().map(String::from));
        let paket = url_von(&|n| n.ends_with("linux-static-x64.tar.gz")).ok_or("kein statisches Linux-Paket im aktuellen Release")?;
        let summen = url_von(&|n| n == "SHA256SUMS").ok_or("SHA256SUMS fehlt im Release")?;
        let name = paket.rsplit('/').next().unwrap_or("xmrig.tar.gz").to_string();
        let mut daten = Vec::new();
        use std::io::Read;
        ureq::get(&paket).timeout(Duration::from_secs(300)).call().map_err(|e| e.to_string())?.into_reader().take(64 << 20).read_to_end(&mut daten).map_err(|e| e.to_string())?;
        let liste = ureq::get(&summen).timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
        let soll = liste.lines().find(|l| l.contains(&name)).and_then(|l| l.split_whitespace().next()).ok_or("Prüfsumme nicht gefunden")?.to_lowercase();
        use sha2::Digest;
        let ist = crate::ident::hex(&sha2::Sha256::digest(&daten));
        if ist != soll {
            return Err(format!("Prüfsumme falsch ({ist} ≠ {soll}) – nicht eingerichtet"));
        }
        let tgz = ziel.join(&name);
        std::fs::write(&tgz, &daten).map_err(|e| e.to_string())?;
        let ok = Command::new("tar").args(["-xzf"]).arg(&tgz).arg("-C").arg(&ziel).arg("--strip-components=1").status().map_err(|e| format!("tar: {e}"))?;
        let _ = std::fs::remove_file(&tgz);
        if !ok.success() || !ziel.join("xmrig").exists() {
            return Err("Entpacken fehlgeschlagen".into());
        }
        Ok(format!("XMRig {} eingerichtet (SHA-256 geprüft)", rel["tag_name"].as_str().unwrap_or("?")))
    } else {
        // ARM (Handy, Platine): Ubuntu-Paket im Kompatibilitätsbereich
        for (prog, args) in [("ultar-compat", vec!["apt", "install", "-y", "xmrig"]), ("apt-get", vec!["install", "-y", "xmrig"])] {
            if let Ok(s) = Command::new(prog).args(&args).status() {
                if s.success() && installiert(dir) {
                    return Ok(format!("XMRig eingerichtet ({})", miner_pfad(dir).display()));
                }
            }
        }
        Err("XMRig für diese Architektur nicht gefunden (Ubuntu-Paket xmrig)".into())
    }
}

pub struct Miner {
    kind: Option<Child>,
    seit: Option<Instant>,
    pub grund: String,
}

pub static MINER: Mutex<Miner> = Mutex::new(Miner { kind: None, seit: None, grund: String::new() });

fn befehl(dir: &Path, k: &Konfig, name: &str) -> Vec<String> {
    if k.waehrung == "eigen" {
        return k.eigener_befehl.clone();
    }
    let kerne = crate::ressourcen::kerne();
    let n = ((kerne as u32 * k.kerne_prozent.clamp(1, 100) + 99) / 100).clamp(1, kerne as u32);
    let mut v = vec![
        miner_pfad(dir).display().to_string(),
        "-o".into(),
        k.pool.clone(),
        "-u".into(),
        k.wallet.clone(),
        "-p".into(),
        if k.arbeiter.is_empty() { name.to_string() } else { k.arbeiter.clone() },
        "--threads".into(),
        n.to_string(),
        "--cpu-priority".into(),
        "0".into(),
        "--donate-level".into(),
        "1".into(),
        "--no-color".into(),
        "--http-host".into(),
        "127.0.0.1".into(),
        "--http-port".into(),
        API_PORT.to_string(),
    ];
    if k.tls {
        v.push("--tls".into());
    }
    v
}

impl Miner {
    pub fn laeuft(&mut self) -> bool {
        if let Some(k) = self.kind.as_mut() {
            if k.try_wait().ok().flatten().is_none() {
                return true;
            }
            self.kind = None;
            self.seit = None;
        }
        false
    }

    fn starten(&mut self, dir: &Path, k: &Konfig, name: &str) -> Result<(), String> {
        let b = befehl(dir, k, name);
        // niedrigste Priorität: Bedienung und andere Programme gehen immer vor
        let kind = Command::new("nice")
            .arg("-n")
            .arg("19")
            .args(&b)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| Command::new(&b[0]).args(&b[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn())
            .map_err(|e| format!("Miner: {e}"))?;
        self.kind = Some(kind);
        self.seit = Some(Instant::now());
        Ok(())
    }

    pub fn stoppen(&mut self) {
        if let Some(mut k) = self.kind.take() {
            let _ = k.kill();
            let _ = k.wait();
        }
        self.seit = None;
    }
}

/// vom Dienst regelmäßig aufgerufen: Miner nach Lage starten oder sofort stoppen
pub fn steuern(dir: &Path, name: &str, verbund_belegt: usize) {
    let k = Konfig::laden(dir);
    let l = lage();
    let mut m = MINER.lock().unwrap();
    let laeuft = m.laeuft();
    match darf(&k, &l, verbund_belegt, laeuft) {
        Ok(()) if !laeuft => {
            if !installiert(dir) && k.waehrung == "xmr" {
                m.grund = "Miner nicht eingerichtet".into();
                return;
            }
            match m.starten(dir, &k, name) {
                Ok(()) => {
                    m.grund = "läuft".into();
                    crate::dienst::log("mining: gestartet (Gerät ruht)");
                }
                Err(e) => m.grund = e,
            }
        }
        Ok(()) => m.grund = "läuft".into(),
        Err(g) => {
            if laeuft {
                m.stoppen();
                crate::dienst::log(&format!("mining: gestoppt – {g}"));
            }
            m.grund = g;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Status {
    pub aktiv: bool,
    pub laeuft: bool,
    pub grund: String,
    pub hashrate: f64,
    pub installiert: bool,
    pub waehrung: String,
    pub minuten: u64,
}

pub fn status(dir: &Path) -> Status {
    let k = Konfig::laden(dir);
    let mut m = MINER.lock().unwrap();
    let laeuft = m.laeuft();
    let hashrate = if laeuft {
        ureq::get(&format!("http://127.0.0.1:{API_PORT}/2/summary"))
            .timeout(Duration::from_millis(800))
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .and_then(|v| v["hashrate"]["total"][0].as_f64())
            .unwrap_or(0.0)
    } else {
        0.0
    };
    Status {
        aktiv: k.aktiv,
        laeuft,
        grund: m.grund.clone(),
        hashrate,
        installiert: installiert(dir),
        waehrung: k.waehrung,
        minuten: m.seit.map(|s| s.elapsed().as_secs() / 60).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const W: &str = "44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A";
    #[test]
    fn regeln() {
        let mut k = Konfig { aktiv: true, wallet: W.into(), ..Default::default() };
        assert!(k.pruefen().is_ok());
        let ruhig = Lage { sekunden_ohne_bedienung: 3600, am_strom: true, temp_c: Some(50.0) };
        assert!(darf(&k, &ruhig, 0, false).is_ok());
        assert!(darf(&k, &Lage { sekunden_ohne_bedienung: 10, ..ruhig.clone() }, 0, false).is_err());
        assert!(darf(&k, &ruhig, 1, true).is_err());
        assert!(darf(&k, &Lage { am_strom: false, ..ruhig.clone() }, 0, false).is_err());
        assert!(darf(&k, &Lage { temp_c: Some(72.0), ..ruhig.clone() }, 0, false).is_err()); // Abstand
        assert!(darf(&k, &Lage { temp_c: Some(72.0), ..ruhig.clone() }, 0, true).is_ok());
        k.waehrung = "eth".into();
        assert!(k.pruefen().unwrap_err().contains("Proof-of-Stake"));
        k.waehrung = "xmr".into();
        k.wallet = "0xabc".into();
        assert!(k.pruefen().is_err());
    }
}
