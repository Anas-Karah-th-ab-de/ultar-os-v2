//! Telefon-App: Wählfeld, Anrufliste, Anrufzustand.
//! Untermodul von `app`. Die eigentliche Telefonie läuft über ein `TelephonyBackend`;
//! bis der Modem-Stack (crate `ultar-modem`, Shannon-CP) steht, wird ein Stub benutzt.
//! So lässt sich später das echte Modem ohne Änderung der Oberfläche anklemmen.

use super::App;
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Ui};

/// Zustand eines Anrufs.
#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub enum CallState {
    Idle,
    Dialing(String),
    Active(String),
    Incoming(String),
}

/// Ein Eintrag der Anrufliste. dir: 0 ausgehend, 1 eingehend, 2 verpasst.
#[allow(dead_code)]
#[derive(Clone)]
pub struct CallLogEntry {
    pub number: String,
    pub dir: u8,
    pub when: String,
}

/// Schnittstelle zum Funkteil. Das echte Modem implementiert das später.
pub trait TelephonyBackend: Send {
    fn dial(&mut self, number: &str);
    fn hangup(&mut self);
    fn answer(&mut self);
    fn state(&self) -> CallState;
    fn log(&self) -> Vec<CallLogEntry>;
    /// true, wenn ein Funknetz verfügbar ist (Modem online + SIM bereit).
    fn ready(&self) -> bool;
    /// Kann das Backend Stumm/Lautsprecher/Tastentöne während des Gesprächs? (sonst Knöpfe gedimmt)
    fn supports_controls(&self) -> bool {
        false
    }
    fn set_mute(&mut self, _on: bool) {}
    fn set_speaker(&mut self, _on: bool) {}
    /// Tastenton (DTMF) während des Gesprächs senden
    fn dtmf(&mut self, _digit: char) {}
    /// true, wenn der Signalserver seit dem letzten Aufruf einen Sicherheitshinweis gemeldet hat
    /// (z. B. beantragter Nummern-Umzug) – die Oberfläche fragt dann den Kontostand neu ab.
    fn take_notice(&mut self) -> bool {
        false
    }
    /// Test/Vorführung: eingehenden Anruf vortäuschen (nur Platzhalter-Backend; echte Backends ignorieren es)
    fn simulate_incoming(&mut self, _number: &str) {}
}

/// Platzhalter-Backend ohne echtes Modem: merkt sich den Zustand, führt aber keinen echten Anruf.
pub struct StubTelephony {
    state: CallState,
    log: Vec<CallLogEntry>,
}

impl Default for StubTelephony {
    fn default() -> Self {
        Self {
            state: CallState::Idle,
            log: Vec::new(),
        }
    }
}

impl TelephonyBackend for StubTelephony {
    fn dial(&mut self, number: &str) {
        if number.is_empty() {
            return;
        }
        self.state = CallState::Dialing(number.to_string());
        self.log.insert(
            0,
            CallLogEntry {
                number: number.to_string(),
                dir: 0,
                when: String::new(),
            },
        );
        if self.log.len() > 100 {
            self.log.truncate(100);
        }
    }
    fn simulate_incoming(&mut self, number: &str) {
        self.state = CallState::Incoming(number.to_string());
        self.log.insert(
            0,
            CallLogEntry {
                number: number.to_string(),
                dir: 1,
                when: String::new(),
            },
        );
    }
    fn hangup(&mut self) {
        self.state = CallState::Idle;
    }
    fn answer(&mut self) {
        if let CallState::Incoming(n) = self.state.clone() {
            self.state = CallState::Active(n);
        }
    }
    fn state(&self) -> CallState {
        self.state.clone()
    }
    fn log(&self) -> Vec<CallLogEntry> {
        self.log.clone()
    }
    fn ready(&self) -> bool {
        false // kein echtes Modem -> kein Netz
    }
}

/// Weist dem Signalserver den Geräteschlüssel nach; verbindet `ultar-cloud` (Schlüssel) mit `ultar-call`.
/// Der Nachweis gilt nur für den Server, zu dem die Verbindung aufgebaut wird (siehe `signal_proof`).
struct DeviceProof(ultar_cloud::SignalProof);

impl ultar_call::SignalAuth for DeviceProof {
    fn hello(&self) -> Result<String, String> {
        self.0.hello().map_err(|e| e.to_string())
    }
    fn answer(&self, challenge_frame: &str) -> Result<String, String> {
        self.0.answer(challenge_frame).map_err(|e| e.to_string())
    }
}

/// Telefonie über die Ultar-Cloud (Internet-Anrufe zwischen Ultar-Geräten).
/// Setzt das `CallBackend` aus dem Crate `ultar-call` auf das `TelephonyBackend` der Oberfläche um.
pub struct CloudTelephony {
    inner: Box<dyn ultar_call::CallBackend>,
    log: Vec<CallLogEntry>,
}

impl Default for CloudTelephony {
    fn default() -> Self {
        let mut inner = ultar_call::default_backend();
        // beim Signalserver anmelden, wenn das Gerät registriert ist (Nachweis mit dem Geräteschlüssel)
        // Der Nachweis wird für genau die Adresse ausgestellt, zu der verbunden wird: zeigt die
        // Einstellung auf einen anderen Server als den der Registrierung, gibt es keinen Nachweis
        // und keine Anmeldung (kein Signieren für einen fremden Server).
        let base = ultar_cloud::base_url();
        if let Ok(proof) = ultar_cloud::DeviceHome::system().signal_proof(&base) {
            inner.register(&base, Box::new(DeviceProof(proof)));
        }
        Self {
            inner,
            log: Vec::new(),
        }
    }
}

impl TelephonyBackend for CloudTelephony {
    fn dial(&mut self, number: &str) {
        if number.is_empty() {
            return;
        }
        self.inner.place(number, false);
        self.log.insert(
            0,
            CallLogEntry {
                number: number.to_string(),
                dir: 0,
                when: String::new(),
            },
        );
        self.log.truncate(100);
    }
    fn hangup(&mut self) {
        self.inner.hangup();
    }
    fn answer(&mut self) {
        self.inner.answer();
    }
    fn state(&self) -> CallState {
        // Ereignisse abholen, damit sich der Zustand fortschreibt (poll braucht &mut -> hier nur Zustand lesen)
        match self.inner.state() {
            ultar_call::CallState::Idle | ultar_call::CallState::Ended => CallState::Idle,
            ultar_call::CallState::Dialing(n) => CallState::Dialing(n),
            ultar_call::CallState::Incoming(n) => CallState::Incoming(n),
            ultar_call::CallState::Active(n) => CallState::Active(n),
        }
    }
    fn log(&self) -> Vec<CallLogEntry> {
        self.log.clone()
    }
    fn ready(&self) -> bool {
        self.inner.ready()
    }
    fn take_notice(&mut self) -> bool {
        self.inner
            .poll()
            .iter()
            .any(|e| matches!(e, ultar_call::CallEvent::Notice { .. }))
    }
}

/// Mobilfunk-Telefonie über die SIM: spricht mit dem Funkdienst `ultar-ril` (Befehle über
/// /run/ultar/radio/cmd, Zustand aus /run/ultar/radio/call). Ultar-Nummern (UOS-…) und Anrufe ohne Netz gehen
/// an die Cloud-Telefonie, wenn ein Ultar-Konto besteht.
pub struct SimTelephony {
    cloud: Option<CloudTelephony>,
    log: Vec<CallLogEntry>,
}

impl Default for SimTelephony {
    fn default() -> Self {
        let cloud = ultar_cloud::my_number()
            .is_some()
            .then(CloudTelephony::default);
        let log = load_call_log();
        Self { cloud, log }
    }
}

const CALL_FILE: &str = "/run/ultar/radio/call";
const USSD_FILE: &str = "/run/ultar/radio/ussd";

/// USSD-/MMI-Code wie `*111#`?
pub fn is_ussd(s: &str) -> bool {
    s.len() >= 2
        && (s.starts_with('*') || s.starts_with('#'))
        && s.ends_with('#')
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '*' || c == '#')
}

/// USSD-Zustand des Funkdienstes: (state, text). state: 9 wartet, 1 fertig, 2 Antwort erwartet, 3–6 Ende/Fehler.
pub fn ussd_state() -> (String, String) {
    let t = std::fs::read_to_string(USSD_FILE).unwrap_or_default();
    let get = |k: &str| {
        t.lines()
            .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
            .unwrap_or_default()
    };
    let hex = get("text=");
    let b: Vec<u8> = hex
        .as_bytes()
        .chunks(2)
        .filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .collect();
    (get("state="), String::from_utf8_lossy(&b).into_owned())
}

/// Anrufzustand des Funkdienstes lesen: (state, number)
fn sim_call() -> (String, String) {
    let t = std::fs::read_to_string(CALL_FILE).unwrap_or_default();
    let get = |k: &str| {
        t.lines()
            .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
            .unwrap_or_default()
    };
    (get("state="), get("number="))
}

/// Ist eine SIM bereit und das Modem online?
pub fn sim_ready() -> bool {
    let s = ultar_radio::read_state();
    matches!(s.modem, ultar_radio::Modem::Online)
        && s.sims
            .iter()
            .any(|x| x.state == ultar_radio::SimState::Ready)
}

fn call_log_path() -> String {
    format!("{}/calls.tsv", crate::sys::data_dir())
}

fn load_call_log() -> Vec<CallLogEntry> {
    let t = std::fs::read_to_string(call_log_path()).unwrap_or_default();
    t.lines()
        .rev()
        .take(100)
        .filter_map(|l| {
            let mut it = l.split('\t');
            let when = it.next()?.to_string();
            let dir = it.next()?.parse().ok()?;
            Some(CallLogEntry {
                when,
                dir,
                number: it.next()?.to_string(),
            })
        })
        .collect()
}

fn now_hm() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    t.to_string()
}

impl SimTelephony {
    fn add_log(&mut self, number: &str, dir: u8) {
        let e = CallLogEntry {
            number: number.to_string(),
            dir,
            when: now_hm(),
        };
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(call_log_path())
        {
            let _ = writeln!(f, "{}\t{}\t{}", e.when, e.dir, e.number);
        }
        self.log.insert(0, e);
        self.log.truncate(100);
    }
    fn cloud_active(&self) -> bool {
        self.cloud
            .as_ref()
            .is_some_and(|c| !matches!(c.state(), CallState::Idle))
    }
}

impl TelephonyBackend for SimTelephony {
    fn dial(&mut self, number: &str) {
        if number.is_empty() {
            return;
        }
        // USSD/MMI-Codes (*111#) bzw. Antwort in einer laufenden USSD-Sitzung → Funkdienst, kein Anruf
        let compact = number.replace(' ', "");
        let (ust, _) = ussd_state();
        if is_ussd(&compact)
            || (ust == "2"
                && compact.len() <= 20
                && compact
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '*' || c == '#'))
        {
            let hex: String = compact.bytes().map(|b| format!("{b:02x}")).collect();
            let _ = ultar_radio::send_cmd(&format!("ussd 0 {hex}"));
            return;
        }
        let ultar_nr = number.to_ascii_uppercase().starts_with("UOS");
        if let Some(c) = self.cloud.as_mut().filter(|_| ultar_nr || !sim_ready()) {
            c.dial(number);
        } else {
            let _ = ultar_radio::send_cmd(&format!("dial 0 {}", number.replace(' ', "")));
        }
        self.add_log(number, 0);
    }
    fn hangup(&mut self) {
        if self.cloud_active() {
            if let Some(c) = self.cloud.as_mut() {
                c.hangup();
            }
        } else {
            let _ = ultar_radio::send_cmd("hangup");
        }
    }
    fn answer(&mut self) {
        if self.cloud_active() {
            if let Some(c) = self.cloud.as_mut() {
                c.answer();
            }
        } else {
            let (st, n) = sim_call();
            if st == "incoming" {
                self.add_log(&n, 1);
            }
            let _ = ultar_radio::send_cmd("answer");
        }
    }
    fn state(&self) -> CallState {
        if let Some(c) = self.cloud.as_ref() {
            let s = c.state();
            if !matches!(s, CallState::Idle) {
                return s;
            }
        }
        let (st, n) = sim_call();
        let n = if n.is_empty() { "?".to_string() } else { n };
        match st.as_str() {
            "incoming" => CallState::Incoming(n),
            "dialing" => CallState::Dialing(n),
            "active" => CallState::Active(n),
            _ => CallState::Idle,
        }
    }
    fn log(&self) -> Vec<CallLogEntry> {
        self.log.clone()
    }
    fn ready(&self) -> bool {
        sim_ready() || self.cloud.as_ref().is_some_and(|c| c.ready())
    }
    // Gespräch über die SIM: Lautsprecher/Stumm schalten ultar-ril (Modem) und ultar-audio (Tonweg)
    fn take_notice(&mut self) -> bool {
        self.cloud.as_mut().is_some_and(|c| c.take_notice())
    }
    fn supports_controls(&self) -> bool {
        !self.cloud_active()
    }
    fn set_speaker(&mut self, on: bool) {
        let _ = std::fs::write("/run/ultar/callaudio", if on { "speaker" } else { "auto" });
    }
    fn set_mute(&mut self, on: bool) {
        let _ = ultar_radio::send_cmd(if on { "mute 0 1" } else { "mute 0 0" });
    }
}

/// Zustand der Telefon-App (Feld auf `App`).
pub struct PhoneState {
    pub number: String,
    /// 0 = Wählfeld, 1 = Anrufliste
    pub tab: u8,
    pub tel: Box<dyn TelephonyBackend>,
    /// Beginn des laufenden Gesprächs (für die Gesprächszeit in Anrufbildschirm, Statusleiste, Schnelleinstellungen)
    pub since: Option<std::time::Instant>,
    pub mute: bool,
    pub speaker: bool,
    pub keypad: bool,
    pub dtmf: String,
    /// klingelte gerade (Nummer) – wird beim Auflegen ohne Annehmen zum verpassten Anruf
    pub ringing: Option<String>,
    /// Sicherheitshinweis des Ultar-Kontos (beantragter Nummern-Umzug)
    pub alert: super::app_alert::SecurityAlert,
}

impl Default for PhoneState {
    fn default() -> Self {
        // Hat das Gerät eine Ultar-Nummer, läuft die Telefonie über die Cloud (Internet-Anrufe);
        // sonst Stub (bis echtes Mobilfunk-Modem). Beide nutzen dieselbe Oberfläche.
        // Gerät mit Mobilfunk-Modem (A30: /dev/umts_ipc0) → SIM-Telefonie (mit Cloud für Ultar-Nummern);
        // sonst Cloud-Telefonie bei Ultar-Konto, sonst Platzhalter.
        let tel: Box<dyn TelephonyBackend> = if std::path::Path::new("/dev/umts_ipc0").exists()
            || std::env::var_os("ULTAR_SIM_TEL").is_some()
        {
            Box::new(SimTelephony::default())
        } else if ultar_cloud::my_number().is_some() {
            Box::new(CloudTelephony::default())
        } else {
            Box::new(StubTelephony::default())
        };
        Self {
            number: String::new(),
            tab: 0,
            tel,
            since: None,
            mute: false,
            speaker: false,
            keypad: false,
            dtmf: String::new(),
            ringing: None,
            alert: Default::default(),
        }
    }
}

impl App {
    /// Sicherheitshinweis des Ultar-Kontos als bleibende Karte über dem Wählfeld; gibt die
    /// verbleibende Fläche zurück. Solange ein Nummern-Umzug beantragt ist, bleibt die Karte stehen.
    fn security_banner(&mut self, ui: &mut Ui, body: Rect) -> Rect {
        use super::app_alert::Banner;
        let Some((banner, texts)) = self.phone.alert.banner_with_text(&self.set.lang) else {
            return body;
        };
        let text = texts.message;
        let action: &str = match banner {
            Banner::Stopped | Banner::Removed => self.t("Close"),
            Banner::Pending { .. } | Banner::Added { .. } => &texts.action,
        };
        let pad = ui.dp(12.0);
        let (tw, btn_h) = (body.w - 2.0 * pad, ui.dp(36.0));
        let th = ui.tx.measure(&text, ui.dp(13.5), false, tw).1;
        let r = Rect::new(body.x, body.y + ui.dp(4.0), body.w, th + btn_h + pad * 3.0);
        ui.card(r);
        let warn = ui.th.text;
        ui.para(r.x + pad, r.y + pad, tw, &text, 13.5, warn);
        let button = Rect::new(r.x + pad, r.bottom() - pad - btn_h, tw, btn_h);
        let can_act = texts.can_act;
        if can_act && ui.button(button, action, crate::ui::Btn::Plain) {
            match banner {
                Banner::Pending { .. } | Banner::Added { .. } => self.phone.alert.not_me(&banner),
                Banner::Stopped | Banner::Removed => self.phone.alert.dismiss(),
            }
            ui.dirty = true;
        } else if !can_act {
            let c = ui.th.sub;
            ui.label(button, action, 12.0, c, false, Al::Center);
        }
        Rect::new(
            body.x,
            r.bottom(),
            body.w,
            (body.bottom() - r.bottom()).max(0.0),
        )
    }

    pub(super) fn s_phone(&mut self, ui: &mut Ui, area: Rect) {
        // Laufender/klingelnder Anruf: Vollbild-Anrufbildschirm
        let st = self.phone.tel.state();
        if !matches!(st, CallState::Idle) {
            self.call_screen(ui, area, st);
            return;
        }
        let area = self.header(ui, area, self.t("Phone"));
        let m = ui.dp(12.0);

        // Hinweis, wenn kein Netz (Modem/SIM noch nicht aktiv)
        let state = self.phone.tel.state();
        let ready = self.phone.tel.ready();

        // Reiter wie in der Web-Demo: Wählen / Verlauf / Kontakte als Segment-Umschalter
        let mut ty = area.y + ui.dp(2.0);
        let labels = [self.t("Dial"), self.t("Recents"), self.t("Contacts")];
        if let Some(i) = ui.chips(&mut ty, &labels, self.phone.tab as usize) {
            if i == 2 {
                self.go(super::Screen::Contacts);
                ui.dirty = true;
                return;
            }
            self.phone.tab = i as u8;
            ui.dirty = true;
        }
        let body = Rect::new(m, ty, ui.w - 2.0 * m, area.bottom() - ty);
        let body = self.security_banner(ui, body);

        if !ready {
            let c = ui.th.sub;
            ui.label(
                Rect::new(body.x, body.y + ui.dp(2.0), body.w, ui.dp(20.0)),
                self.t("No mobile network yet (modem/SIM not active)"),
                12.0,
                c,
                false,
                Al::Center,
            );
        }

        // USSD-Antwort des Netzes (z. B. Guthaben, Paket-Menü): eigene Karte oben; bei Menüs (Zustand 2) darunter
        // Antwortzeile + Tastenfeld im freien Platz – nichts überlappt.
        let (ust, utext) = ussd_state();
        let mut body = body;
        if !ust.is_empty() && ust != "0" && !(ust == "1" && utext.is_empty()) {
            let menu = ust == "2";
            let pad = ui.dp(14.0);
            let tw = body.w - 2.0 * pad;
            let msg = if ust == "9" {
                "USSD …".to_string()
            } else if !utext.trim().is_empty() {
                utext.trim().to_string()
            } else {
                match ust.as_str() {
                    "3" => self.t("USSD session ended by the network").to_string(),
                    "6" => self.t("No answer from the network (timeout)").to_string(),
                    _ => self.t("The network does not support this code").to_string(),
                }
            };
            let fs = if menu { 13.5 } else { 15.0 };
            let th = ui.tx.measure(&msg, ui.dp(fs), false, tw).1;
            let btn_h = ui.dp(36.0);
            // Menü: Karte höchstens 42 % der Fläche, damit Antwortzeile und Tastenfeld Platz haben
            let max_h = if menu {
                body.h * 0.42
            } else {
                body.h - ui.dp(16.0)
            };
            let h = (th + pad * 3.0 + btn_h).min(max_h);
            let r = Rect::new(body.x, body.y + ui.dp(6.0), body.w, h);
            ui.card(r);
            let tc = ui.th.text;
            ui.set_clip(Some(Rect::new(r.x, r.y, r.w, h - btn_h - pad)));
            ui.para(r.x + pad, r.y + pad, tw, &msg, fs, tc);
            ui.set_clip(None);
            let by = r.bottom() - pad / 2.0 - btn_h;
            if ui.button(
                Rect::new(r.x + pad, by, ui.dp(110.0), btn_h),
                self.t("Close"),
                crate::ui::Btn::Plain,
            ) {
                let _ = ultar_radio::send_cmd("ussd_cancel");
                self.phone.number.clear();
                ui.dirty = true;
            }
            if !menu {
                return;
            }
            // Antwortzeile: was getippt wird, geht mit der grünen Taste als Antwort ans Netz
            let hint = self.t("Enter your reply on the keypad and press call");
            let sc = ui.th.sub;
            ui.label(
                Rect::new(body.x, r.bottom() + ui.dp(4.0), body.w, ui.dp(16.0)),
                hint,
                11.0,
                sc,
                false,
                Al::Center,
            );
            let top = r.bottom() + ui.dp(22.0);
            body = Rect::new(body.x, top, body.w, (body.bottom() - top).max(ui.dp(200.0)));
            self.phone.tab = 0;
        }

        if self.phone.tab == 1 {
            self.phone_log_modern(ui, body);
            return;
        }

        let _ = state;

        // Nummernanzeige
        let ussd_menu = ust == "2";
        let disp = if ussd_menu {
            Rect::new(body.x, body.y, body.w, ui.dp(40.0))
        } else {
            Rect::new(body.x, body.y + ui.dp(8.0), body.w, ui.dp(70.0))
        };
        let old_rtl = ui.rtl;
        ui.rtl = false;
        let shown = if self.phone.number.is_empty() {
            String::new()
        } else {
            self.phone.number.clone()
        };
        let tc = ui.th.text;
        ui.label(
            disp,
            &shown,
            if ussd_menu { 28.0 } else { 40.0 },
            tc,
            true,
            Al::Center,
        );
        ui.rtl = old_rtl;

        // Name zum Kontakt, falls bekannt
        self.extra_load();
        let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
        let cur = digits(&self.phone.number);
        if cur.len() >= 4 {
            if let Some(ct) =
                self.extra.contacts.list.iter().find(|c| {
                    digits(&c.number).ends_with(&cur) || cur.ends_with(&digits(&c.number))
                })
            {
                let (n, cs) = (ct.name.clone(), ui.th.accent);
                ui.label(
                    Rect::new(body.x, disp.bottom() - ui.dp(8.0), body.w, ui.dp(22.0)),
                    &n,
                    14.0,
                    cs,
                    true,
                    Al::Center,
                );
            }
        }
        // Tastenfeld 3x4 mit runden Tasten, darunter Anrufen (grün) und Löschen
        let keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];
        let subs = [
            "", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ", "", "+", "",
        ];
        let top = disp.bottom() + ui.dp(16.0);
        let call_d = ui.dp(68.0);
        let avail = (body.bottom() - top - call_d - ui.dp(22.0)).max(ui.dp(160.0));
        let ch = (avail / 4.0).min(ui.dp(84.0));
        let gw = (ui.w - 2.0 * m).min(ch * 3.9).min(ui.dp(360.0));
        let gx = body.x + (body.w - gw) / 2.0;
        let cw = gw / 3.0;
        let kr = (ch.min(cw) / 2.0 - ui.dp(5.0)).max(ui.dp(20.0));
        // Tastenfeld unten verankern wie in der Web-Demo (Nummer steht darüber)
        let top = (body.bottom() - 4.0 * ch - ui.dp(10.0) - call_d - ui.dp(18.0)).max(top);
        let old_rtl = ui.rtl;
        ui.rtl = false;
        for (i, k) in keys.iter().enumerate() {
            let (kx, ky) = (
                gx + (i % 3) as f32 * cw + cw / 2.0,
                top + (i / 3) as f32 * ch + ch / 2.0,
            );
            let kw = (cw - ui.dp(14.0)).min(kr * 2.6);
            let hit = Rect::new(kx - kw / 2.0, ky - kr, kw, 2.0 * kr);
            let held = ui.held(hit);
            ui.cv
                .rrect(hit, kr * 0.62, if held { ui.th.press } else { ui.th.card });
            let (tc, cs) = (ui.th.text, ui.th.sub);
            let has_sub = !subs[i].is_empty();
            ui.label(
                Rect::new(
                    hit.x,
                    hit.y + if has_sub { -ui.dp(6.0) } else { 0.0 },
                    hit.w,
                    hit.h,
                ),
                k,
                26.0,
                tc,
                false,
                Al::Center,
            );
            if has_sub {
                ui.label(
                    Rect::new(hit.x, ky + ui.dp(10.0), hit.w, ui.dp(14.0)),
                    subs[i],
                    9.5,
                    cs,
                    false,
                    Al::Center,
                );
            }
            if ui.tap(hit) {
                if self.phone.number.chars().count() < 24 {
                    self.phone.number.push_str(k);
                }
                ui.dirty = true;
            }
            // langer Druck auf 0 = „+" (Ländervorwahl)
            if *k == "0" && self.long_press(ui, hit) && self.phone.number.chars().count() < 24 {
                self.phone.number.push('+');
                ui.dirty = true;
            }
        }
        ui.rtl = old_rtl;
        let ay = top + 4.0 * ch + ui.dp(10.0) + call_d / 2.0;
        let ok = ui.th.ok;
        if ui.icon_btn(
            gx + gw / 2.0,
            ay,
            call_d / 2.0,
            crate::ui::Ic::Phone,
            ok,
            rgb(0xFFFFFF),
        ) {
            let n = self.phone.number.clone();
            if n.is_empty() {
                // leer: letzte Nummer übernehmen
                if let Some(e) = self.phone.tel.log().first() {
                    self.phone.number = e.number.clone();
                }
            } else {
                self.phone.tel.dial(&n);
                if ussd_menu || is_ussd(&n.replace(' ', "")) {
                    self.phone.number.clear();
                }
            }
            ui.dirty = true;
        }
        // links „UOS" (Ultar-Nummer), rechts Löschen – wie in der Web-Demo
        let ac = ui.th.accent;
        let ur = Rect::new(
            gx + gw / 2.0 - cw - ui.dp(30.0),
            ay - ui.dp(22.0),
            ui.dp(60.0),
            ui.dp(44.0),
        );
        ui.label(ur, "UOS", 13.0, ac, true, Al::Center);
        if ui.tap(ur) && !self.phone.number.starts_with("UOS-") {
            self.phone.number = format!("UOS-{}", self.phone.number);
            ui.dirty = true;
        }
        let tc = if self.phone.number.is_empty() {
            ui.th.sub.alpha(0.5)
        } else {
            ui.th.text
        };
        if ui.icon_btn(
            gx + gw / 2.0 + cw,
            ay,
            ui.dp(22.0),
            crate::ui::Ic::Back,
            ui.th.card.alpha(0.0),
            tc,
        ) {
            self.phone.number.pop();
            if self.phone.number == "UOS-" {
                self.phone.number.clear();
            }
            ui.dirty = true;
        }
    }
}
