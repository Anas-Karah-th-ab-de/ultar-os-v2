//! ultar-radio – gemeinsames Mobilfunk-Modell für Oberfläche (ultar-shell) und Funkdienst (ultar-ril --daemon).
//!
//! Drei Dateien verbinden die beiden Seiten (kein Socket, kein Bus – bleibt auch bei Absturz einer Seite lesbar):
//!
//! | Datei | Schreibt | Liest | Inhalt |
//! |---|---|---|---|
//! | `/run/ultar/radio/state` | Funkdienst | Oberfläche | Modem, SIMs, Netz, Zellen (Zeilen `key=value`) |
//! | `/data/ultar/radio.conf` | Oberfläche | Funkdienst | Vorgaben: Netzwahl-Modus, angepinnte Zelle, SIM-Rollen |
//! | `/run/ultar/radio/cmd` | Oberfläche | Funkdienst | einmalige Befehle (`select 4`, `pin 2 1234`, `rescan`) |
//!
//! Dazu die Zellwahl: **Auto** = stärkstes Signal (wie Android), **Smart** = bestes geschätztes Tempo
//! (Signal × Störabstand × freie Kapazität) mit Hysterese, **Manuell** = angepinnte Zelle.

use std::fmt::Write as _;

// ---------------------------------------------------------------- Pfade

pub const STATE_PATH: &str = "/run/ultar/radio/state";
pub const CMD_PATH: &str = "/run/ultar/radio/cmd";

/// Zustandsdatei (für Tests/Bildschirmfotos über `ULTAR_RADIO_STATE` umlenkbar).
pub fn state_path() -> String {
    std::env::var("ULTAR_RADIO_STATE").unwrap_or_else(|_| STATE_PATH.into())
}

/// Befehlsdatei (über `ULTAR_RADIO_CMD` umlenkbar).
pub fn cmd_path() -> String {
    std::env::var("ULTAR_RADIO_CMD").unwrap_or_else(|_| CMD_PATH.into())
}

/// Vorgaben liegen bei den Nutzerdaten (überleben Neustarts).
pub fn prefs_path() -> String {
    format!(
        "{}/radio.conf",
        std::env::var("ULTAR_DATA").unwrap_or_else(|_| "/data/ultar".into())
    )
}

// ---------------------------------------------------------------- Zustand

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Modem {
    #[default]
    Offline,
    Booting,
    Online,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SimState {
    Ready,
    Pin,
    Puk,
    Locked,
    Absent,
    Error,
    #[default]
    Unknown,
}

impl SimState {
    pub fn key(self) -> &'static str {
        match self {
            SimState::Ready => "ready",
            SimState::Pin => "pin",
            SimState::Puk => "puk",
            SimState::Locked => "locked",
            SimState::Absent => "absent",
            SimState::Error => "error",
            SimState::Unknown => "unknown",
        }
    }
    pub fn from_key(s: &str) -> Self {
        match s {
            "ready" => SimState::Ready,
            "pin" => SimState::Pin,
            "puk" => SimState::Puk,
            "locked" => SimState::Locked,
            "absent" => SimState::Absent,
            "error" => SimState::Error,
            _ => SimState::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sim {
    pub slot: u8,
    pub state: SimState,
    /// Anbietername (leer = unbekannt)
    pub operator: String,
    /// Rufnummer, falls die SIM sie kennt
    pub number: String,
}

/// Eine Funkzelle (ein Sektor eines Funkturms) des eigenen Anbieters.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cell {
    /// Laufende Nummer in dieser Liste (stabil, solange die Zelle sichtbar bleibt)
    pub id: u32,
    /// "LTE", "UMTS", "GSM" …
    pub rat: String,
    /// "B3", "B20" …
    pub band: String,
    pub mhz: u32,
    pub pci: Option<u32>,
    /// Signalstärke in dBm
    pub rsrp: Option<i32>,
    /// Qualität in dB
    pub rsrq: Option<i32>,
    /// Störabstand in dB
    pub sinr: Option<i32>,
    /// gemessenes Tempo (Probe), falls vorhanden
    pub mbps: Option<f32>,
    /// Auslastung 0..100 %, falls bekannt
    pub load: Option<u8>,
    /// Entfernung in Metern (Zeitvorlauf/Datenbank), falls bekannt
    pub dist_m: Option<u32>,
    /// Richtung in Grad (0 = Norden), falls bekannt
    pub bearing: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct RadioState {
    pub modem: Modem,
    pub sims: Vec<Sim>,
    /// Funktechnik der Verbindung ("LTE", "LTE-A", "UMTS" …), leer = keine
    pub rat: String,
    pub plmn: String,
    pub operator: String,
    pub roaming: bool,
    /// Id der verbundenen Zelle
    pub serving: Option<u32>,
    pub cells: Vec<Cell>,
    /// Kann das Modem eine Zelle fest erzwingen? (sonst nur „bevorzugt")
    pub pin_supported: bool,
    /// Liefert das Modem Nachbarzellen? (sonst nur die verbundene)
    pub neighbours: bool,
    /// Unix-Sekunden der letzten Aktualisierung
    pub updated: u64,
    /// Freitext des Dienstes (z. B. Grund, warum etwas fehlt)
    pub note: String,
}

impl RadioState {
    pub fn serving_cell(&self) -> Option<&Cell> {
        let id = self.serving?;
        self.cells.iter().find(|c| c.id == id)
    }
    pub fn cell(&self, id: u32) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }
    pub fn sim(&self, slot: u8) -> Option<&Sim> {
        self.sims.iter().find(|s| s.slot == slot)
    }
    /// Gibt es überhaupt Dienst?
    pub fn in_service(&self) -> bool {
        self.modem == Modem::Online && !self.rat.is_empty()
    }
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, ';' | '\n' | '\r') {
                ','
            } else {
                c
            }
        })
        .collect()
}

fn opt<T: std::fmt::Display>(o: &Option<T>) -> String {
    o.as_ref().map(|v| v.to_string()).unwrap_or_default()
}

/// Zustand als Text (Format der Datei `state`).
pub fn format_state(s: &RadioState) -> String {
    let mut o = String::new();
    let modem = match s.modem {
        Modem::Offline => "offline",
        Modem::Booting => "booting",
        Modem::Online => "online",
    };
    let _ = writeln!(o, "modem={modem}");
    let _ = writeln!(o, "rat={}", clean(&s.rat));
    let _ = writeln!(o, "plmn={}", clean(&s.plmn));
    let _ = writeln!(o, "operator={}", clean(&s.operator));
    let _ = writeln!(o, "roaming={}", s.roaming as u8);
    let _ = writeln!(o, "serving={}", opt(&s.serving));
    let _ = writeln!(o, "pin_supported={}", s.pin_supported as u8);
    let _ = writeln!(o, "neighbours={}", s.neighbours as u8);
    let _ = writeln!(o, "updated={}", s.updated);
    let _ = writeln!(o, "note={}", clean(&s.note));
    for m in &s.sims {
        let _ = writeln!(
            o,
            "sim=slot:{};state:{};op:{};num:{}",
            m.slot,
            m.state.key(),
            clean(&m.operator).replace(':', " "),
            clean(&m.number).replace(':', " ")
        );
    }
    for c in &s.cells {
        let _ = writeln!(
            o,
            "cell=id:{};rat:{};band:{};mhz:{};pci:{};rsrp:{};rsrq:{};sinr:{};mbps:{};load:{};dist:{};bearing:{}",
            c.id,
            clean(&c.rat).replace(':', " "),
            clean(&c.band).replace(':', " "),
            c.mhz,
            opt(&c.pci),
            opt(&c.rsrp),
            opt(&c.rsrq),
            opt(&c.sinr),
            c.mbps.map(|v| format!("{v:.1}")).unwrap_or_default(),
            opt(&c.load),
            opt(&c.dist_m),
            opt(&c.bearing)
        );
    }
    o
}

fn fields(v: &str) -> impl Iterator<Item = (&str, &str)> {
    v.split(';').filter_map(|p| p.split_once(':'))
}

fn num<T: std::str::FromStr>(v: &str) -> Option<T> {
    let v = v.trim();
    if v.is_empty() {
        None
    } else {
        v.parse().ok()
    }
}

/// Zustandsdatei lesen (unbekannte Zeilen werden übersprungen – Format darf wachsen).
pub fn parse_state(text: &str) -> RadioState {
    let mut s = RadioState::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "modem" => {
                s.modem = match v.trim() {
                    "online" => Modem::Online,
                    "booting" => Modem::Booting,
                    _ => Modem::Offline,
                }
            }
            "rat" => s.rat = v.trim().into(),
            "plmn" => s.plmn = v.trim().into(),
            "operator" => s.operator = v.trim().into(),
            "roaming" => s.roaming = v.trim() == "1",
            "serving" => s.serving = num(v),
            "pin_supported" => s.pin_supported = v.trim() == "1",
            "neighbours" => s.neighbours = v.trim() == "1",
            "updated" => s.updated = num(v).unwrap_or(0),
            "note" => s.note = v.trim().into(),
            "sim" => {
                let mut m = Sim::default();
                for (fk, fv) in fields(v) {
                    match fk {
                        "slot" => m.slot = num(fv).unwrap_or(0),
                        "state" => m.state = SimState::from_key(fv.trim()),
                        "op" => m.operator = fv.trim().into(),
                        "num" => m.number = fv.trim().into(),
                        _ => {}
                    }
                }
                if m.slot > 0 {
                    s.sims.push(m);
                }
            }
            "cell" => {
                let mut c = Cell::default();
                let mut has_id = false;
                for (fk, fv) in fields(v) {
                    match fk {
                        "id" => {
                            if let Some(i) = num(fv) {
                                c.id = i;
                                has_id = true;
                            }
                        }
                        "rat" => c.rat = fv.trim().into(),
                        "band" => c.band = fv.trim().into(),
                        "mhz" => c.mhz = num(fv).unwrap_or(0),
                        "pci" => c.pci = num(fv),
                        "rsrp" => c.rsrp = num(fv),
                        "rsrq" => c.rsrq = num(fv),
                        "sinr" => c.sinr = num(fv),
                        "mbps" => c.mbps = num(fv),
                        "load" => c.load = num(fv),
                        "dist" => c.dist_m = num(fv),
                        "bearing" => c.bearing = num(fv),
                        _ => {}
                    }
                }
                if has_id {
                    s.cells.push(c);
                }
            }
            _ => {}
        }
    }
    s
}

/// Zustand lesen; fehlt die Datei (Dienst läuft nicht), gilt „Modem aus".
pub fn read_state() -> RadioState {
    std::fs::read_to_string(state_path())
        .map(|t| parse_state(&t))
        .unwrap_or_default()
}

/// Zustand atomar schreiben (erst .tmp, dann umbenennen – die Oberfläche liest nie eine halbe Datei).
pub fn write_state(s: &RadioState) -> std::io::Result<()> {
    let p = state_path();
    if let Some(dir) = std::path::Path::new(&p).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{p}.tmp");
    std::fs::write(&tmp, format_state(s))?;
    std::fs::rename(tmp, p)
}

// ---------------------------------------------------------------- Vorgaben

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Modem entscheidet (stärkstes Signal)
    Auto,
    /// Ultar wählt die Zelle mit dem besten Tempo
    #[default]
    Smart,
    /// Nutzer pinnt eine Zelle
    Manual,
}

impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Smart => "smart",
            Mode::Manual => "manual",
        }
    }
    pub fn from_key(s: &str) -> Self {
        match s {
            "auto" => Mode::Auto,
            "manual" => Mode::Manual,
            _ => Mode::Smart,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Self {
        [Mode::Auto, Mode::Smart, Mode::Manual][i.min(2)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tech {
    #[default]
    Auto,
    Lte,
    Umts,
    Gsm,
}

impl Tech {
    pub fn key(self) -> &'static str {
        match self {
            Tech::Auto => "auto",
            Tech::Lte => "4g",
            Tech::Umts => "3g",
            Tech::Gsm => "2g",
        }
    }
    pub fn from_key(s: &str) -> Self {
        match s {
            "4g" => Tech::Lte,
            "3g" => Tech::Umts,
            "2g" => Tech::Gsm,
            _ => Tech::Auto,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Self {
        [Tech::Auto, Tech::Lte, Tech::Umts, Tech::Gsm][i.min(3)]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub mode: Mode,
    pub pinned: Option<u32>,
    /// SIM für mobile Daten (1/2)
    pub data_sim: u8,
    /// SIM für Anrufe (1/2, 0 = jedes Mal fragen)
    pub call_sim: u8,
    /// SIM für SMS (1/2, 0 = jedes Mal fragen)
    pub sms_sim: u8,
    pub roaming: bool,
    pub tech: Tech,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            mode: Mode::Smart,
            pinned: None,
            data_sim: 1,
            call_sim: 1,
            sms_sim: 1,
            roaming: false,
            tech: Tech::Auto,
        }
    }
}

pub fn format_prefs(p: &Prefs) -> String {
    format!(
        "# Ultar Mobilfunk-Vorgaben (von der Oberfläche geschrieben)\nmode={}\npinned={}\ndata_sim={}\ncall_sim={}\nsms_sim={}\nroaming={}\ntech={}\n",
        p.mode.key(),
        opt(&p.pinned),
        p.data_sim,
        p.call_sim,
        p.sms_sim,
        p.roaming as u8,
        p.tech.key()
    )
}

pub fn parse_prefs(text: &str) -> Prefs {
    let mut p = Prefs::default();
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "mode" => p.mode = Mode::from_key(v),
            "pinned" => p.pinned = num(v),
            "data_sim" => p.data_sim = num(v).filter(|n| (1..=2).contains(n)).unwrap_or(1),
            "call_sim" => p.call_sim = num(v).filter(|n| *n <= 2).unwrap_or(1),
            "sms_sim" => p.sms_sim = num(v).filter(|n| *n <= 2).unwrap_or(1),
            "roaming" => p.roaming = v == "1",
            "tech" => p.tech = Tech::from_key(v),
            _ => {}
        }
    }
    p
}

pub fn read_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path())
        .map(|t| parse_prefs(&t))
        .unwrap_or_default()
}

pub fn write_prefs(p: &Prefs) -> std::io::Result<()> {
    let path = prefs_path();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, format_prefs(p))?;
    std::fs::rename(tmp, path)
}

/// Einmaligen Befehl an den Funkdienst anhängen (eine Zeile).
pub fn send_cmd(line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let p = cmd_path();
    if let Some(dir) = std::path::Path::new(&p).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Befehle können eine PIN enthalten: nur root darf lesen
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    writeln!(f, "{}", line.replace('\n', " "))
}

/// Alle anstehenden Befehle abholen und die Datei leeren (Funkdienst).
pub fn take_cmds() -> Vec<String> {
    let p = cmd_path();
    let Ok(t) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    let _ = std::fs::write(&p, "");
    t.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// ---------------------------------------------------------------- Bewertung

/// Signalstufe 0..4 aus RSRP (LTE): ≥ −80 ausgezeichnet, ≥ −95 gut, ≥ −105 mittel, ≥ −115 schwach, sonst keins.
pub fn level(rsrp: Option<i32>) -> u8 {
    match rsrp {
        Some(v) if v >= -80 => 4,
        Some(v) if v >= -95 => 3,
        Some(v) if v >= -105 => 2,
        Some(v) if v >= -115 => 1,
        _ => 0,
    }
}

/// Unterhalb dieser Signalstärke gilt eine Zelle als nicht verlässlich nutzbar.
pub const MIN_USABLE_RSRP: i32 = -115;
/// Smart wechselt nur, wenn die neue Zelle mindestens so viel schneller ist …
pub const SMART_GAIN: f32 = 1.2;
/// … und das über diese Dauer stabil bleibt (Sekunden).
pub const SMART_HOLD_S: u64 = 30;

/// Typische Kanalbreite je LTE-Band in MHz (für die Schätzung, wenn keine Messung vorliegt).
fn bandwidth_mhz(band: &str) -> f32 {
    match band
        .trim_start_matches(['B', 'b'])
        .parse::<u32>()
        .unwrap_or(0)
    {
        20 | 8 | 28 => 10.0,
        3 | 7 => 20.0,
        1 => 15.0,
        _ => 10.0,
    }
}

/// Tempo einer Zelle in Mbit/s: gemessen, sonst geschätzt aus Störabstand, Kanalbreite und Auslastung.
pub fn est_mbps(c: &Cell) -> Option<f32> {
    if let Some(m) = c.mbps {
        return Some(m);
    }
    let sinr = match (c.sinr, c.rsrp) {
        (Some(s), _) => s as f32,
        (None, Some(r)) => ((r + 120) as f32 / 2.0).clamp(-5.0, 25.0),
        _ => return None,
    };
    // Shannon-Grenze × Praxisfaktor; freie Kapazität nach Auslastung
    let eff = (1.0 + 10f32.powf(sinr / 10.0)).log2();
    let free = 1.0
        - c.load
            .map(|l| l.min(100) as f32 / 100.0 * 0.85)
            .unwrap_or(0.4);
    Some((bandwidth_mhz(&c.band) * eff * 0.6 * free).max(0.1))
}

fn usable(c: &Cell) -> bool {
    c.rsrp.map(|r| r >= MIN_USABLE_RSRP).unwrap_or(true)
}

/// Zelle mit dem stärksten Signal (Auto-Modus).
pub fn strongest(cells: &[Cell]) -> Option<u32> {
    cells
        .iter()
        .filter(|c| c.rsrp.is_some())
        .max_by_key(|c| c.rsrp.unwrap_or(i32::MIN))
        .map(|c| c.id)
}

/// Zelle mit dem besten Tempo unter den verlässlich nutzbaren (Smart-Modus).
pub fn fastest(cells: &[Cell]) -> Option<u32> {
    cells
        .iter()
        .filter(|c| usable(c))
        .filter_map(|c| est_mbps(c).map(|m| (c.id, m)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id)
}

/// Welche Zelle der gewählte Modus anstrebt (für die Anzeige „Ziel").
pub fn target(p: &Prefs, s: &RadioState) -> Option<u32> {
    match p.mode {
        Mode::Auto => strongest(&s.cells).or(s.serving),
        Mode::Smart => fastest(&s.cells).or(s.serving),
        Mode::Manual => p.pinned.filter(|id| s.cell(*id).is_some()).or(s.serving),
    }
}

/// Hysterese für Smart: verhindert ständiges Springen zwischen ähnlich guten Zellen.
#[derive(Clone, Debug, Default)]
pub struct SmartGate {
    cand: Option<u32>,
    since: u64,
}

impl SmartGate {
    /// Liefert die Zelle, zu der jetzt gewechselt werden soll (oder None).
    pub fn step(&mut self, s: &RadioState, now_s: u64) -> Option<u32> {
        let best = fastest(&s.cells);
        let (Some(best), Some(cur)) = (best, s.serving) else {
            self.cand = None;
            return None;
        };
        let cur_m = s.cell(cur).and_then(est_mbps).unwrap_or(0.0);
        let best_m = s.cell(best).and_then(est_mbps).unwrap_or(0.0);
        if best == cur || best_m < cur_m * SMART_GAIN {
            self.cand = None;
            return None;
        }
        if self.cand != Some(best) {
            self.cand = Some(best);
            self.since = now_s;
            return None;
        }
        if now_s.saturating_sub(self.since) >= SMART_HOLD_S {
            self.cand = None;
            return Some(best);
        }
        None
    }

    /// Wartet gerade ein Wechsel? (Zelle, verbleibende Sekunden)
    pub fn pending(&self, now_s: u64) -> Option<(u32, u64)> {
        self.cand.map(|c| {
            (
                c,
                SMART_HOLD_S.saturating_sub(now_s.saturating_sub(self.since)),
            )
        })
    }
}

/// Beispielzustand (für Bildschirmfotos und Tests – nie auf dem Gerät als echt ausgeben).
pub fn demo() -> RadioState {
    let cell = |id, band: &str, mhz, pci, rsrp, rsrq, sinr, mbps, load, dist, bearing| Cell {
        id,
        rat: "LTE".into(),
        band: band.into(),
        mhz,
        pci: Some(pci),
        rsrp: Some(rsrp),
        rsrq: Some(rsrq),
        sinr: Some(sinr),
        mbps: Some(mbps),
        load: Some(load),
        dist_m: Some(dist),
        bearing: Some(bearing),
    };
    RadioState {
        modem: Modem::Online,
        sims: vec![
            Sim {
                slot: 1,
                state: SimState::Ready,
                operator: "Demo-Netz".into(),
                number: String::new(),
            },
            Sim {
                slot: 2,
                state: SimState::Pin,
                operator: String::new(),
                number: String::new(),
            },
        ],
        rat: "LTE-A".into(),
        plmn: "00101".into(),
        operator: "Demo-Netz".into(),
        roaming: false,
        serving: Some(2),
        cells: vec![
            cell(1, "B20", 800, 117, -84, -9, 8, 22.0, 78, 1900, 300),
            cell(2, "B3", 1800, 214, -92, -11, 14, 48.0, 55, 800, 50),
            cell(3, "B1", 2100, 88, -97, -12, 11, 31.0, 40, 1200, 220),
            cell(4, "B7", 2600, 301, -103, -13, 12, 96.0, 12, 600, 130),
            cell(5, "B8", 900, 402, -108, -15, 3, 9.0, 85, 3100, 160),
        ],
        pin_supported: false,
        neighbours: true,
        updated: 0,
        note: "Beispieldaten".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let mut s = demo();
        s.sims[0].operator = "A;B:C".into();
        let t = format_state(&s);
        let p = parse_state(&t);
        assert_eq!(p.cells, s.cells);
        assert_eq!(p.serving, Some(2));
        assert_eq!(p.modem, Modem::Online);
        assert_eq!(p.sims.len(), 2);
        assert_eq!(p.sims[0].operator, "A,B C");
        assert_eq!(p.sims[1].state, SimState::Pin);
        assert_eq!(p.rat, "LTE-A");
    }

    #[test]
    fn missing_values_stay_none() {
        let p = parse_state("modem=online\ncell=id:7;rat:LTE;band:B3;mhz:1800;pci:;rsrp:-90;rsrq:;sinr:;mbps:;load:;dist:;bearing:\nunbekannt=1\n");
        assert_eq!(p.cells.len(), 1);
        assert_eq!(p.cells[0].rsrp, Some(-90));
        assert_eq!(p.cells[0].sinr, None);
        assert!(est_mbps(&p.cells[0]).is_some(), "Schätzung aus RSRP");
    }

    #[test]
    fn prefs_roundtrip() {
        let p = Prefs {
            mode: Mode::Manual,
            pinned: Some(4),
            data_sim: 2,
            call_sim: 0,
            sms_sim: 1,
            roaming: true,
            tech: Tech::Lte,
        };
        assert_eq!(parse_prefs(&format_prefs(&p)), p);
        assert_eq!(parse_prefs("data_sim=9\n").data_sim, 1);
    }

    #[test]
    fn modes_pick_expected_cells() {
        let s = demo();
        assert_eq!(strongest(&s.cells), Some(1));
        assert_eq!(fastest(&s.cells), Some(4));
        let mut p = Prefs::default();
        assert_eq!(target(&p, &s), Some(4));
        p.mode = Mode::Auto;
        assert_eq!(target(&p, &s), Some(1));
        p.mode = Mode::Manual;
        p.pinned = Some(3);
        assert_eq!(target(&p, &s), Some(3));
        p.pinned = Some(99);
        assert_eq!(target(&p, &s), Some(2), "unbekannte Zelle -> verbundene");
    }

    #[test]
    fn unusable_cells_are_ignored() {
        let mut s = demo();
        s.cells[3].rsrp = Some(-120);
        assert_ne!(fastest(&s.cells), Some(4));
    }

    #[test]
    fn smart_gate_waits_and_needs_gain() {
        let s = demo();
        let mut g = SmartGate::default();
        assert_eq!(g.step(&s, 100), None);
        assert_eq!(g.pending(110), Some((4, 20)));
        assert_eq!(g.step(&s, 120), None);
        assert_eq!(g.step(&s, 130), Some(4));
        // kaum schneller -> kein Wechsel
        let mut s2 = demo();
        s2.cells[3].mbps = Some(50.0);
        let mut g2 = SmartGate::default();
        g2.step(&s2, 0);
        assert_eq!(g2.step(&s2, 100), None);
    }

    #[test]
    fn levels() {
        assert_eq!(level(Some(-70)), 4);
        assert_eq!(level(Some(-92)), 3);
        assert_eq!(level(Some(-103)), 2);
        assert_eq!(level(Some(-110)), 1);
        assert_eq!(level(Some(-130)), 0);
        assert_eq!(level(None), 0);
    }
}
