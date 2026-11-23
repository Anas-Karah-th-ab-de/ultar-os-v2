//! Bildschirme von Ultar OS: Start, Einstellungen, Pakete, Dienste, Terminal.

use crate::agent::{self, AgentCfg, Call, Reply};
use crate::apps::{self, AppsCfg};
use crate::brand;
use crate::gfx::{rgb, Rect};
use crate::hw::{self, HwDev};
use crate::i18n::{tr, DAYS};
use crate::quick;
use crate::sys::{self, Bg, Msg, NetCfg, Settings, Svc};
use crate::ui::{Al, Btn, Ic, Key, Ui, ACCENTS};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[path = "app_agent.rs"]
mod app_agent;
#[path = "app_alert.rs"]
mod app_alert;
#[path = "app_bt.rs"]
mod app_bt;
#[path = "app_buddy.rs"]
mod app_buddy;
#[path = "app_camera.rs"]
mod app_camera;
#[path = "app_cloud.rs"]
mod app_cloud;
#[path = "app_demo.rs"]
mod app_demo;
#[path = "app_desktop.rs"]
pub(crate) mod app_desktop;
#[path = "app_extra.rs"]
mod app_extra;
#[path = "app_gallery.rs"]
mod app_gallery;
/// System-Assistent wie Jarvis: sieht Bildschirm/Kamera/Mitgebrachtes, Root-Werkzeuge mit Rückfrage, Modelle
#[path = "app_jarvis.rs"]
mod app_jarvis;
#[path = "app_layout.rs"]
mod app_layout;
#[path = "app_messages.rs"]
mod app_messages;
#[path = "app_modern.rs"]
mod app_modern;
#[path = "app_more.rs"]
pub(crate) mod app_more;
/// Büro-Apps: PDF, Dokumente, Tabellen, Code-Editor
#[path = "app_office.rs"]
mod app_office;
#[path = "app_phone.rs"]
mod app_phone;
#[path = "app_radio.rs"]
mod app_radio;
#[path = "app_security.rs"]
mod app_security;
#[path = "app_spotify.rs"]
mod app_spotify;
#[path = "app_sys.rs"]
pub(crate) mod app_sys;
#[path = "app_telegram.rs"]
mod app_telegram;
#[path = "app_voice.rs"]
mod app_voice;
#[path = "app_zellen.rs"]
mod app_zellen;
#[path = "monitor.rs"]
mod monitor;
#[path = "sensors.rs"]
mod sensors;
#[path = "tools.rs"]
mod tools;

#[derive(Clone, PartialEq, Debug)]
pub enum Screen {
    Home,
    Settings,
    Net,
    Display,
    Lang,
    Time,
    Storage,
    Services,
    System,
    Store,
    Pkg(String, String),
    Job,
    Terminal,
    /// Ersteinrichtung, Schritt 0..=4
    Setup(u8),
    Hardware,
    Agent,
    AgentSet,
    /// WLAN-Kennwort für (Netzname, verschlüsselt)
    WifiPw(String, bool),
    Update,
    Calc,
    Clock,
    Files,
    /// Datei anzeigen (Bild oder Text)
    Viewer(String),
    Gallery,
    Privacy,
    Monitor,
    /// Datei oder Ordner umbenennen
    Rename(String),
    Sound,
    /// Web-App bzw. Browser mit Startadresse
    Web(String),
    /// KM-Helfer als App (wie in der Demo)
    Helper,
    /// Web-App-Vorschaltseite wie in der Demo ("whatsapp", "youtube")
    WebApp(String),
    Sensors,
    Gnss,
    Bluetooth,
    Phone,
    Messages,
    MessageNew,
    Camera,
    Telegram,
    Spotify,
    Security,
    Account,
    Vpn,
    Verbund,
    /// Weitergeben an ein anderes Gerät: Datei (falls offen) oder diese App/Bildschirm
    Weitergeben(Option<String>),
    /// Zellen & Container, eine Zelle, Grafikkarten, Empfehlung vor dem Installieren
    Zellen,
    Zelle(String),
    Gpu,
    Empfehlung(String),
    StoreCloud,
    Updates,
    /// Mobilfunk-Zentrum (Dual-SIM, Live-Signal, SIM-Rollen)
    Radio,
    /// Funkturm-Wahl (Auto / Smart / Manuell)
    Cells,
    /// Verbindungsweg Gerät → Zelle → Anbieter → VPN → Internet
    Route,
    /// Alle Apps (App-Übersicht)
    Apps,
    /// Foto oder Video im Vollbild-Betrachter der Galerie
    Photo(String),
    Notes,
    NoteEdit(usize),
    Contacts,
    ContactEdit(Option<usize>),
    /// Grafische Programme aus Ubuntu-Paketen
    UbuntuApps,
    /// Systemfunktionen und Zusatz-Apps (app_sys.rs)
    Sys(app_sys::Sx),
    /// Werkzeuge, Übersicht, Pflege, Modi (app_more.rs)
    More(app_more::Mx),
    /// Hilfe-Zentrum: None = Übersicht, Some(id) = einzelner Artikel
    Help(Option<String>),
    /// Büro-Apps (app_office.rs): PDF-Leser, Dokumente, Tabellen, Code-Editor
    Office(app_office::Ox),
}

/// Merkliste der selbst installierten Ubuntu-Pakete
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(default)]
pub struct UserPkgs {
    pub list: Vec<String>,
}

pub struct Job {
    pub title: String,
    pub lines: Vec<String>,
    pub done: Option<i32>,
}

pub struct App {
    pub set: Settings,
    pub net: NetCfg,
    /// true: keine steuerbare Hintergrundbeleuchtung -> Helligkeit per Software
    pub soft_bright: bool,
    stack: Vec<(Screen, f32)>,
    fields: HashMap<&'static str, String>,
    focus: Option<&'static str>,
    focus_reset: bool,
    kb: crate::kbd::KbState,
    /// Seite, die bei offener Tastatur verschoben/gerollt wird, und ihre Verschiebung
    kb_pan: (Option<Screen>, f32),
    /// Aktionsknopf der Kopfzeile (Neu/Schreiben) wurde angetippt
    hdr_tap: bool,
    /// Kopfzeile auslassen (Bildschirm wird in einen anderen eingebettet, z. B. Rechner-Reiter)
    no_hdr: bool,
    /// Blatt von unten (wie in der Web-Demo)
    sheet: Option<app_demo::Sheet>,
    kb_ensure: bool,
    kb_last: Option<&'static str>,
    power_dialog: bool,
    /// Schnelleinstellungen/Benachrichtigungen (Wisch von oben)
    shade: bool,
    /// Benachrichtigungs-Eingang (von Apps/Diensten) + aktueller Kopf-Hinweis
    notifier: crate::notify::Notifier,
    heads: Option<(ultar_notify::Note, Instant)>,
    /// zuletzt gemeldeter eingehender Anruf (um Doppelmeldung zu vermeiden)
    last_incoming: String,
    /// Änderungszeit der USSD-Datei (neue Antwort → Telefon nach vorne)
    last_ussd: Option<std::time::SystemTime>,
    bg: Bg,
    job: Option<Job>,
    results: Vec<(String, String)>,
    installed: HashSet<String>,
    searching: bool,
    searched: bool,
    term: Vec<String>,
    term_busy: bool,
    toast: Option<(String, Instant)>,
    /// Schwebender Helfer über allen Seiten
    pub buddy: app_buddy::Buddy,
    pub voice: app_voice::Voice,
    svcs: Vec<Svc>,
    svc_at: Option<Instant>,
    disp: (u32, u32, String),
    hw: Vec<HwDev>,
    selftest: Vec<crate::selftest::Check>,
    pub agent: AgentCfg,
    /// Verlauf für die Anzeige: (Art, Text) – Art: 0 Nutzer, 1 Agent, 2 Werkzeug, 3 Fehler
    chat: Vec<(u8, String)>,
    history: Vec<serde_json::Value>,
    agent_busy: bool,
    agent_up: Option<bool>,
    pending: Option<Call>,
    steps: u32,
    wifi: Vec<(String, i32, bool)>,
    wifi_scanning: bool,
    /// gespeicherte WLAN-Netze (aus wpa_supplicant)
    wifi_saved: Vec<String>,
    /// Netz, mit dem gerade verbunden wird
    wifi_connecting: Option<String>,
    apps: Vec<apps::App>,
    apps_cfg: AppsCfg,
    /// Pakete-Bildschirm: 0 = KM-Apps, 1 = Ubuntu-Pakete
    store_tab: usize,
    /// Messwerte: (Start bis erstes Bild in ms, letztes Bild in ms, Mittel in ms)
    pub perf: (f32, f32, f32),
    /// Vom Hauptprogramm auszuführen: Browser mit dieser Adresse im Vollbild starten
    pub launch: Option<String>,
    /// Desktop-Oberfläche (Laptop/Server): Fenster, Leiste, Startmenü
    pub desk: app_desktop::Desk,
    desk_enter: bool,
    desk_start_sc: f32,
    upd: Option<crate::update::Release>,
    upd_checking: bool,
    /// Zustand der eingebauten Apps (Rechner, Uhr, Dateien, Galerie, Werbesperre)
    tl: tools::Tools,
    mon: monitor::Mon,
    gnss: crate::gnss::GnssState,
    phone: app_phone::PhoneState,
    radio_ui: app_radio::RadioUi,
    layout_rates: app_layout::Rates,
    /// Startbildschirm: aktuelle Seite des App-Rasters und Gleitposition (für die Animation)
    home_pg: (usize, f32),
    msg: app_messages::MessagesState,
    cam: app_camera::CameraState,
    gal: app_gallery::Gal,
    extra: app_extra::Extra,
    sx: app_sys::SxState,
    mx: app_more::MxState,
    /// Büro-Apps
    office: app_office::OfficeState,
    /// System-Assistent (Sehen, Mitbringen, Modelle)
    jv: app_jarvis::Jarvis,
    /// Hintergrundbild des Startbildschirms (None = noch nicht nachgesehen)
    wall: Option<Option<tools::Pic>>,
    tg: app_telegram::TelegramState,
    spotify: app_spotify::SpotifyState,
    cloud: app_cloud::CloudState,
    mon_tab: usize,
    /// Beginn der aktuellen Berührung und ob der lange Druck schon ausgelöst hat
    press_at: Option<Instant>,
    lp_fired: bool,
    ptr_down: bool,
    /// Vom Nutzer installierte Ubuntu-Pakete (für den Reiter „Installiert“)
    user_pkgs: UserPkgs,
}

impl App {
    pub fn new(bg: Bg, disp: (u32, u32, String)) -> Self {
        let mut set: Settings = sys::load("settings.toml");
        // Laptop/Server auf dem Ultar-Kern: die Installation hat die Sprache schon gewählt – Desktop direkt,
        // ohne die Einrichtung des Handys (Einstellungen bleiben jederzeit änderbar)
        if !set.setup_done
            && !std::path::Path::new(&format!("{}/settings.toml", sys::data_dir())).exists()
        {
            if let Ok(l) = std::env::var("ULTAR_SYSTEMSPRACHE") {
                if matches!(l.as_str(), "ar" | "de" | "en") {
                    set.lang = l;
                    if matches!(
                        ultar_hal_system::current_edition(),
                        ultar_profile::Edition::Desktop | ultar_profile::Edition::Server
                    ) {
                        set.setup_done = true;
                    }
                }
            }
        }
        if !set.hostname.is_empty() {
            let _ = nix::unistd::sethostname(&set.hostname);
        }
        let mut a = Self {
            set,
            net: sys::load("net.toml"),
            soft_bright: true,
            stack: vec![(Screen::Home, 0.0)],
            hw: Vec::new(),
            selftest: Vec::new(),
            agent: sys::load("agent.toml"),
            chat: Vec::new(),
            history: Vec::new(),
            agent_busy: false,
            agent_up: None,
            pending: None,
            steps: 0,
            wifi: Vec::new(),
            wifi_scanning: false,
            wifi_saved: Vec::new(),
            wifi_connecting: None,
            apps: apps::catalog(),
            apps_cfg: sys::load("apps-auth.toml"),
            store_tab: 0,
            perf: (0.0, 0.0, 0.0),
            launch: None,
            desk: Default::default(),
            desk_enter: false,
            desk_start_sc: 0.0,
            upd: None,
            upd_checking: false,
            tl: tools::Tools::default(),
            mon: monitor::Mon::default(),
            gnss: crate::gnss::GnssState::default(),
            phone: Default::default(),
            radio_ui: Default::default(),
            layout_rates: Default::default(),
            home_pg: (0, 0.0),
            msg: Default::default(),
            cam: Default::default(),
            gal: Default::default(),
            extra: Default::default(),
            sx: app_sys::SxState::load(),
            mx: app_more::MxState::load(),
            office: Default::default(),
            jv: Default::default(),
            wall: None,
            tg: Default::default(),
            spotify: Default::default(),
            cloud: Default::default(),
            mon_tab: 0,
            press_at: None,
            lp_fired: false,
            ptr_down: false,
            user_pkgs: sys::load("user-pkgs.toml"),
            fields: HashMap::new(),
            focus: None,
            focus_reset: false,
            kb: crate::kbd::KbState::new("ar"),
            kb_pan: (None, 0.0),
            hdr_tap: false,
            no_hdr: false,
            sheet: None,
            kb_ensure: false,
            kb_last: None,
            power_dialog: false,
            shade: false,
            notifier: Default::default(),
            heads: None,
            last_incoming: String::new(),
            last_ussd: None,
            bg,
            job: None,
            results: Vec::new(),
            installed: HashSet::new(),
            searching: false,
            searched: false,
            term: Vec::new(),
            term_busy: false,
            toast: None,
            buddy: Default::default(),
            voice: Default::default(),
            svcs: Vec::new(),
            svc_at: None,
            disp,
        };
        a.apply_brightness();
        a.tools_init();
        a.monitor_init();
        // Hilfe-Wissensbasis als Markdown ablegen (Basis für den lokalen Agenten)
        #[cfg(not(test))]
        crate::help::export("/data/ultar/help");
        if !a.set.setup_done {
            a.stack = vec![(Screen::Setup(0), 0.0)];
        }
        if a.agent.enabled && agent::model_installed() {
            sys::ctl("start llm");
        }
        #[cfg(not(test))]
        crate::sysd::start(a.bg.clone());
        a
    }

    /// Für Bildschirmfotos ohne Gerät: Sprache setzen und einen Bildschirm direkt öffnen.
    /// Fern-Navigation (Test über die Tür): Bildschirm öffnen, Sprache nur vorübergehend (nicht gespeichert).
    /// Anweisung an den Helfer von einem anderen Gerät (Ultar-Verbund, auch per Stimme weitergeleitet)
    pub fn remote_helfer(&mut self, text: String) {
        self.buddy_run(text);
        if let Some(a) = self.buddy.answer.take() {
            self.buddy_show(a);
        }
    }

    /// vom Verbund empfangene Datei öffnen (Bild → Fotoansicht, sonst Dateiansicht)
    pub fn remote_datei(&mut self, pfad: String) {
        let low = pfad.to_lowercase();
        if [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
            .iter()
            .any(|e| low.ends_with(e))
        {
            self.go(Screen::Photo(pfad));
        } else if let Some(s) = Self::office_screen_for(&pfad) {
            self.go(s);
        } else {
            self.go(Screen::Viewer(pfad));
        }
    }

    /// Weitergeben-Geste: offene Datei/offenes Bild mitnehmen, sonst diese App/diesen Bildschirm
    pub fn weitergeben_oeffnen(&mut self) {
        let datei = match self.stack.last().map(|s| &s.0) {
            Some(Screen::Photo(p)) | Some(Screen::Viewer(p)) => Some(p.clone()),
            Some(Screen::Weitergeben(_)) => return,
            _ => None,
        };
        self.go(Screen::Weitergeben(datei));
    }

    pub fn remote_open(&mut self, screen: &str, lang: Option<&str>) {
        let lang = lang
            .map(|l| l.to_string())
            .unwrap_or_else(|| self.set.lang.clone());
        let done = self.set.setup_done;
        self.debug_open(screen, &lang);
        self.set.setup_done = done;
    }

    pub fn debug_open(&mut self, screen: &str, lang: &str) {
        self.set.lang = lang.into();
        self.set.setup_done = true;
        // "shade" = Startbildschirm mit geöffneten Schnelleinstellungen
        // Fern-Navigation: "locknow" sperrt, jeder andere Bildschirm entsperrt (Tür = root, Testzugang)
        self.sx.locked = screen == "locknow";
        self.shade = screen == "shade" || screen == "call_shade";
        // Tastatur zum Prüfen: kb-<art>[-<sprache>], z. B. kb-text, kb-text-en, kb-sym, kb-pin, kb-clip, kb-pop-de
        if let Some(rest) = screen.strip_prefix("kb-") {
            let mut parts = rest.split('-');
            let art = parts.next().unwrap_or("text");
            let kl = parts.next().unwrap_or(lang);
            let (field, scr): (&'static str, &str) = match art {
                "pin" => ("simpin", "home"),
                "phone" => ("sms_to", "messages"),
                "num" => ("num", "home"),
                "search" => ("q", "home"),
                "note" => ("note", "home"),
                "contact" => ("cnum", "contactnew"),
                _ => ("sms_newtext", "messages"),
            };
            self.focus = Some(field);
            self.focus_reset = true;
            if art != "pin" && art != "phone" && art != "num" && art != "contact" {
                self.fields.insert(
                    field,
                    match kl {
                        "ar" => "مرحبا، نلتقي الساعة 6".into(),
                        "de" => "Treffen um 18 Uhr, dann gr".into(),
                        _ => "See you at 6, then".into(),
                    },
                );
            }
            self.kb.debug(art, kl);
            self.debug_open(scr, lang);
            self.focus = Some(field);
            self.focus_reset = true;
            return;
        }
        // Anruf-Zustände zum Prüfen (Platzhalter-Telefonie): call_out, call_in, call, call_shade
        match screen {
            "call_out" => self.phone.tel.dial("+49 170 1234567"),
            "call_in" => self.phone.tel.simulate_incoming("+49 170 1234567"),
            "msgthread" => self.msg.open = Some(0),
            "call" | "call_shade" => {
                self.phone.tel.simulate_incoming("+49 170 1234567");
                self.phone.tel.answer();
                self.phone.since = Some(Instant::now() - Duration::from_secs(83));
            }
            _ => {}
        }
        let screen = if screen.starts_with("call") && screen != "call_shade" {
            "phone"
        } else if screen == "call_shade" {
            "home"
        } else if screen == "msgthread" {
            "messages"
        } else {
            screen
        };
        let s = match screen {
            "settings" => Screen::Settings,
            "net" => Screen::Net,
            "display" => Screen::Display,
            "time" => Screen::Time,
            "storage" => Screen::Storage,
            "system" => Screen::System,
            "store" => Screen::Store,
            "terminal" => Screen::Terminal,
            "agent" => Screen::Agent,
            "helper" => Screen::Helper,
            "noteedit" => {
                self.extra_load();
                let t = self
                    .extra
                    .notes
                    .list
                    .first()
                    .map(|n| n.text.clone())
                    .unwrap_or_default();
                self.fields.insert("note", t);
                Screen::NoteEdit(0)
            }
            "browser" => Screen::Web("about:blank".into()),
            "calc" => Screen::Calc,
            "phone" => Screen::Phone,
            "messages" => Screen::Messages,
            "camera" => Screen::Camera,
            // Fernbedienung zum Prüfen: Scanner öffnen bzw. zusätzlich auslösen, sobald ein Bild da ist
            // Helfer zum Prüfen: Feld öffnen bzw. ersten Link der Seite prüfen
            "buddysay" => {
                self.buddy.away = false;
                self.buddy.open = true;
                self.buddy.remote_say = std::fs::read_to_string("/run/ultar/buddy.say")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or(Some("help".into()));
                Screen::Messages
            }
            "buddytour" => {
                self.buddy.snap = true;
                self.buddy_tour();
                self.buddy.tour = std::fs::read_to_string("/run/ultar/buddy.say")
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .or(Some(0));
                Screen::Home
            }
            "buddycall" => {
                let t = std::fs::read_to_string("/run/ultar/buddy.say")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Zahnarzt um 14:00 – in 30 Minuten losgehen".into());
                self.buddy.snap = true;
                self.buddy_call(t, 0);
                Screen::Home
            }
            "buddy" | "buddycheck" | "buddyaway" => {
                self.buddy.away = screen == "buddyaway";
                self.buddy.open = screen != "buddyaway";
                self.buddy.remote_check = screen == "buddycheck";
                self.msg.open = Some(0);
                Screen::Messages
            }
            "scanok" => {
                self.cam.mode = 3;
                self.cam.remote_accept = true;
                Screen::Camera
            }
            "scan" | "scanshot" | "photoshot" => {
                self.cam.mode = if screen == "photoshot" { 0 } else { 3 };
                self.cam.remote_shot = screen != "scan";
                Screen::Camera
            }
            "telegram" => Screen::Telegram,
            "spotify" => Screen::Spotify,
            "security" => Screen::Security,
            "account" => Screen::Account,
            "vpn" => Screen::Vpn,
            "verbund" => Screen::Verbund,
            "zellen" => Screen::Zellen,
            z if z.starts_with("zelle:") => Screen::Zelle(z[6..].into()),
            z if z.starts_with("empfehlung:") => Screen::Empfehlung(z[11..].into()),
            "gpu" => Screen::Gpu,
            "store2" => Screen::StoreCloud,
            "updates" => Screen::Updates,
            "sensors" => Screen::Sensors,
            "gnss" => Screen::Gnss,
            "bluetooth" => Screen::Bluetooth,
            "radio" => Screen::Radio,
            "cells" => Screen::Cells,
            "route" => Screen::Route,
            "apps" => Screen::Apps,
            "pdf" => Screen::Office(app_office::Ox::List(0)),
            "docs" => Screen::Office(app_office::Ox::List(1)),
            "sheets" => Screen::Office(app_office::Ox::List(2)),
            "code" => Screen::Office(app_office::Ox::List(3)),
            o if o.starts_with("officeedit:") => {
                // Prüfansicht: Bearbeitungszustand (Absatz gewählt, Zelle in Bearbeitung, Suchen + Menü offen)
                self.office.debug_edit = true;
                Self::office_screen_for(&o[11..]).unwrap_or(Screen::Viewer(o[11..].into()))
            }
            o if o.starts_with("office:") => match Self::office_screen_for(&o[7..]) {
                Some(s) => s,
                None => Screen::Viewer(o[7..].into()),
            },
            "clock" => Screen::Clock,
            "files" => Screen::Files,
            "gallery" => Screen::Gallery,
            "privacy" => Screen::Privacy,
            "monitor" => Screen::Monitor,
            "sound" => Screen::Sound,
            "setup" => Screen::Setup(0),
            "lock" | "locknow" => Screen::Sys(app_sys::Sx::LockSet),
            "pinset" => Screen::Sys(app_sys::Sx::PinSet),
            "timezone" => Screen::Sys(app_sys::Sx::Time),
            "battery" => Screen::Sys(app_sys::Sx::Battery),
            "nightlight" => Screen::Sys(app_sys::Sx::DisplayX),
            "profiles" => Screen::Sys(app_sys::Sx::SoundX),
            "history" => Screen::Sys(app_sys::Sx::NotifHist),
            "nettools" => Screen::Sys(app_sys::Sx::NetTools),
            "hiddenwifi" => Screen::Sys(app_sys::Sx::HiddenWifi),
            "logs" => Screen::Sys(app_sys::Sx::Logs),
            "backup" => Screen::Sys(app_sys::Sx::Backup),
            "dev" => Screen::Sys(app_sys::Sx::Dev),
            "calendar" => Screen::Sys(app_sys::Sx::Calendar),
            "event" => Screen::Sys(app_sys::Sx::EventEdit(None)),
            "world" => Screen::Sys(app_sys::Sx::World),
            "units" => Screen::Sys(app_sys::Sx::Units),
            "search" => Screen::Sys(app_sys::Sx::Search),
            "calchist" => Screen::Sys(app_sys::Sx::CalcHist),
            "tools" => Screen::More(app_more::Mx::Tools),
            "compass" => Screen::More(app_more::Mx::Compass),
            "level" => Screen::More(app_more::Mx::Level),
            "qibla" => Screen::More(app_more::Mx::Qibla),
            "prayer" => Screen::More(app_more::Mx::Prayer),
            "password" => Screen::More(app_more::Mx::Password),
            "dice" => Screen::More(app_more::Mx::Dice),
            "percent" => Screen::More(app_more::Mx::Percent),
            "loan" => Screen::More(app_more::Mx::Loan),
            "datecalc" => Screen::More(app_more::Mx::DateCalc),
            "bases" => Screen::More(app_more::Mx::Bases),
            "currency" => Screen::More(app_more::Mx::Currency),
            "counter" => Screen::More(app_more::Mx::Counter),
            "bigtext" => Screen::More(app_more::Mx::BigText),
            "sos" => Screen::More(app_more::Mx::Sos),
            "focus" => Screen::More(app_more::Mx::Focus),
            "countdown" => Screen::More(app_more::Mx::Countdown),
            "todo" => Screen::More(app_more::Mx::Todo),
            "reminders" => Screen::More(app_more::Mx::Reminders),
            "emergency" => Screen::More(app_more::Mx::Emergency),
            "recents" => Screen::More(app_more::Mx::Recents),
            "clipboard" => Screen::More(app_more::Mx::Clipboard),
            "batthist" => Screen::More(app_more::Mx::BattHist),
            "usage" => Screen::More(app_more::Mx::Usage),
            "care" => Screen::More(app_more::Mx::Care),
            "storagex" => Screen::More(app_more::Mx::StorageX),
            "tasks" => Screen::More(app_more::Mx::Tasks),
            "modes" => Screen::More(app_more::Mx::Modes),
            "blocked" => Screen::More(app_more::Mx::Blocked),
            "help" => Screen::Help(None),
            "help-radio" => Screen::Help(Some("radio".into())),
            "help-compass" => Screen::Help(Some("compass".into())),
            "help-agent" => Screen::Help(Some("agent".into())),
            "notes" => Screen::Notes,
            "contacts" => Screen::Contacts,
            "contactnew" => {
                self.new_contact_sheet();
                Screen::Contacts
            }
            "contact" => Screen::ContactEdit(Some(0)),
            "ubuntu" => Screen::UbuntuApps,
            "agentset" => Screen::AgentSet,
            // nur für Bildschirmfotos: Beispielgespräch
            "agentchat" => {
                self.agent.enabled = true;
                self.agent_up = Some(true);
                let de = lang == "de";
                self.chat = vec![
                    (
                        0,
                        if de {
                            "Mach ein Foto und stell den Wecker auf 7 Uhr".into()
                        } else {
                            "التقط صورة واضبط المنبّه على السابعة".into()
                        },
                    ),
                    (2, "take_photo()".into()),
                    (2, "set_alarm(hour=7, minute=0)".into()),
                    (
                        1,
                        if de {
                            "Erledigt: Das Foto liegt in der Galerie, der Wecker klingelt um 07:00."
                                .into()
                        } else {
                            "تم: الصورة في المعرض والمنبّه مضبوط على 07:00.".into()
                        },
                    ),
                    (
                        0,
                        if de {
                            "Installiere VLC".into()
                        } else {
                            "ثبّت VLC".into()
                        },
                    ),
                    (2, "install_package(name=vlc)".into()),
                ];
                self.agent_busy = true;
                Screen::Agent
            }
            // nur für Bildschirmfotos: System-Assistent (sieht Bildschirm, Root mit Rückfrage, Mitbringen)
            "jarvis" | "jarvisattach" => {
                self.agent.enabled = true;
                self.agent_up = Some(true);
                let (de, ar) = (lang == "de", lang == "ar");
                let tr = |d: &str, a: &str, e: &str| {
                    if de {
                        d.to_string()
                    } else if ar {
                        a.to_string()
                    } else {
                        e.to_string()
                    }
                };
                self.chat = vec![
                    (0, tr("Was ist auf meinem Bildschirm?", "ما الذي على شاشتي؟", "What is on my screen?")),
                    (2, "see_screen()".into()),
                    (1, tr("Du bist im Rechner: 12 × 7 = 84.", "أنت في الآلة الحاسبة: ‎12 × 7 = 84.", "You are in the calculator: 12 × 7 = 84.")),
                    (0, tr("Welches Programm sendet gerade Daten?", "أي برنامج يرسل بيانات الآن؟", "Which program is sending data right now?")),
                    (2, "network_connections()".into()),
                    (1, tr("Nur ultar-net (DHCP) und der Browser zu duckduckgo.com:443 – nichts Verdächtiges.", "فقط ultar-net ‏(DHCP) والمتصفح إلى duckduckgo.com:443 – لا شيء مريب.", "Only ultar-net (DHCP) and the browser to duckduckgo.com:443 – nothing suspicious.")),
                    (0, tr("Beende den Prozess, der am meisten Speicher braucht", "أنهِ العملية التي تستهلك أكبر قدر من الذاكرة", "Stop the process that uses the most memory")),
                    (2, "processes(sort=mem)".into()),
                ];
                self.jv.view = Some((1, 1, vec![0]));
                if screen == "jarvis" {
                    self.pending = Some(crate::agent::Call {
                        tool: "kill_process".into(),
                        args: serde_json::json!({"pid": 812, "signal": "term"}),
                    });
                    self.agent_busy = true;
                } else {
                    self.jv.attach_open = true;
                    self.jv.attach = vec!["/data/Pictures/rechnung.jpg".into()];
                }
                Screen::Agent
            }
            "photo" => match self.gal.newest() {
                Some(it) => {
                    let list: Vec<String> = self.gal.items.iter().map(|i| i.path.clone()).collect();
                    self.open_photo(it.path.clone(), list);
                    self.stack.pop();
                    Screen::Photo(it.path)
                }
                None => Screen::Gallery,
            },
            _ => Screen::Home,
        };
        self.stack = vec![(Screen::Home, 0.0)];
        if s != Screen::Home {
            self.go(s); // wie beim Tippen öffnen (z. B. WLAN-Suche beim Netzwerk-Bildschirm)
        }
    }

    /// Übersetzung für das Hauptprogramm (main.rs)
    pub fn tr(&self, k: &'static str) -> &'static str {
        self.t(k)
    }

    pub fn lang(&self) -> &str {
        &self.set.lang
    }

    fn t(&self, k: &'static str) -> &'static str {
        tr(&self.set.lang, k)
    }

    /// Übersetzung für einen zur Laufzeit gebildeten Schlüssel (z. B. App-Name aus einer Benachrichtigung)
    fn t_dyn(&self, k: &str) -> String {
        crate::i18n::tr_dyn(&self.set.lang, k)
    }

    pub fn apply_brightness(&mut self) {
        self.soft_bright = !sys::set_backlight(self.sys_bright(self.set.brightness));
    }

    /// true, wenn gerade ein Anruf läuft (klingelt, wählt oder verbunden) – für Näherungssensor.
    pub fn call_active(&self) -> bool {
        use app_phone::CallState::*;
        matches!(self.phone.tel.state(), Dialing(_) | Active(_) | Incoming(_))
    }

    /// Eingehende Benachrichtigungen prüfen und alarmieren. Gibt true zurück, wenn der
    /// Bildschirm geweckt werden soll (z. B. eingehender Anruf). Läuft auch bei Bildschirm aus.
    /// Rückgabe: (etwas Neues → neu zeichnen, Bildschirm wecken).
    pub fn poll_notifications(&mut self, asleep: bool) -> (bool, bool) {
        // Eigene Quelle: eingehender Ultar-/SIM-Anruf -> einmalig als Benachrichtigung melden
        let mut opened = false;
        // USSD-Antwort des Netzes (z. B. Paket-Menü nach *111#): Telefon mit der Antwortkarte sofort nach vorne holen
        if let Ok(m) = std::fs::metadata("/run/ultar/radio/ussd").and_then(|m| m.modified()) {
            if self.last_ussd.is_some_and(|t| t != m) {
                let (st, text) = app_phone::ussd_state();
                if (st == "1" || st == "2") && !text.is_empty() {
                    self.shade = false;
                    if !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Phone)) {
                        self.go(Screen::Phone);
                    }
                    self.phone.tab = 0;
                    opened = true;
                }
            }
            self.last_ussd = Some(m);
        }
        // Sicherheitshinweis des Ultar-Kontos: Meldung vom Signalserver -> sofort nachfragen; ein neu
        // beantragter Nummern-Umzug wird als dringende Benachrichtigung gemeldet (auch bei Bildschirm aus).
        if self.phone.tel.take_notice() {
            self.phone.alert.recheck();
        }
        if self.phone.alert.take_fresh() {
            ultar_notify::post(
                "security",
                "phone",
                &ultar_cloud::messages::render(
                    &self.set.lang,
                    ultar_cloud::watch::KEY_ALERT_TITLE,
                    &[],
                ),
                &self
                    .phone
                    .alert
                    .banner_with_text(&self.set.lang)
                    .map_or_else(String::new, |b| b.1.message),
                ultar_notify::Urgency::High,
            );
            opened = true;
        }
        match self.phone.tel.state() {
            app_phone::CallState::Incoming(from) if self.more_blocked(&from) => {
                // Sperrliste: still ablehnen, nur im Verlauf vermerken
                self.phone.tel.hangup();
                ultar_notify::post(
                    "call",
                    "phone",
                    &self.t_owned("Blocked call"),
                    &from,
                    ultar_notify::Urgency::Low,
                );
            }
            app_phone::CallState::Incoming(from) => {
                if self.last_incoming != from {
                    self.last_incoming = from.clone();
                    let who = self.contact_name(&from).unwrap_or_else(|| from.clone());
                    ultar_notify::post(
                        "call",
                        "phone",
                        &self.t_owned("Incoming call"),
                        &who,
                        ultar_notify::Urgency::High,
                    );
                    // Anrufbildschirm sofort zeigen (auch aus jeder anderen App), Schnelleinstellungen schließen
                    self.shade = false;
                    if !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Phone)) {
                        self.go(Screen::Phone);
                    }
                    opened = true;
                }
                self.phone.ringing = Some(from);
            }
            app_phone::CallState::Active(_) => {
                self.last_incoming.clear();
                self.phone.ringing = None;
                self.phone.since.get_or_insert_with(Instant::now);
            }
            app_phone::CallState::Dialing(_) => {
                self.last_incoming.clear();
            }
            app_phone::CallState::Idle => {
                self.last_incoming.clear();
                // Klingelte und wurde nicht angenommen -> verpasster Anruf
                if let Some(from) = self.phone.ringing.take() {
                    let who = self.contact_name(&from).unwrap_or_else(|| from.clone());
                    ultar_notify::post(
                        "call",
                        "phone",
                        &self.t_owned("Missed call"),
                        &who,
                        ultar_notify::Urgency::Normal,
                    );
                }
                self.phone.since = None;
                self.phone.mute = false;
                self.phone.speaker = false;
                self.phone.keypad = false;
                self.phone.dtmf.clear();
            }
        }

        let new = self.notifier.poll();
        let mut wake = false;
        let any = !new.is_empty();
        for n in new {
            // Termine aus dem Kalender sagt der Helfer persönlich an
            if n.app == "calendar" && self.set.setup_done {
                self.buddy_call(format!("{} – {}", n.title, n.body), 0);
                wake = true;
                continue;
            }
            let mut a = crate::notify::alert_for(&n, asleep, self.set.sound);
            let (snd, vib, heads) = self.sys_alert(
                n.urgency == ultar_notify::Urgency::High,
                a.sound,
                a.vibrate_ms,
            );
            a.sound = snd;
            a.vibrate_ms = vib;
            if !heads {
                continue;
            }
            if a.sound {
                if n.urgency == ultar_notify::Urgency::High {
                    crate::audio::alarm(self.set.volume);
                } else {
                    crate::audio::notify(self.set.volume);
                }
            }
            if a.vibrate_ms > 0 {
                sys::vibrate(a.vibrate_ms);
            }
            wake |= a.wake;
            self.heads = Some((n, Instant::now()));
        }
        (any || opened, wake || opened)
    }

    fn t_owned(&self, k: &'static str) -> String {
        self.t(k).to_string()
    }

    /// Lautstärke über die Hardware-Tasten ändern (Schrittweite 10 %).
    pub fn nudge_volume(&mut self, delta: f32) {
        self.set.volume = (self.set.volume + delta).clamp(0.0, 1.0);
        self.sx.vol_shown = Some(Instant::now());
        crate::audio::mixer_setup(self.set.volume);
        self.save();
        self.toast(format!(
            "{} {}%",
            self.t("Volume"),
            (self.set.volume * 100.0).round() as i32
        ));
        if self.set.volume > 0.0 {
            crate::audio::play(vec![(1175.0, 40)], self.set.volume);
        }
    }

    fn save(&mut self) {
        if let Err(e) = sys::save("settings.toml", &self.set) {
            self.toast(format!("{}: {e}", self.t("Could not save")));
        }
    }

    fn toast(&mut self, s: String) {
        self.toast = Some((s, Instant::now()));
    }

    fn f(&self, id: &str) -> String {
        self.fields.get(id).cloned().unwrap_or_default()
    }

    fn go(&mut self, s: Screen) {
        if matches!(self.stack.last().map(|x| &x.0), Some(Screen::Office(_))) {
            self.office_leave();
        }
        self.office_on_go(&s);
        self.more_on_go(&s);
        if let Screen::Sys(x) = &s {
            self.sys_open(x);
        }
        match s {
            Screen::Net => {
                // beim Öffnen gleich nach Netzen suchen
                if !self.wifi_scanning && sys::wifi_iface().is_some() && sys::wifi_tools() {
                    self.wifi_scanning = true;
                    sys::wifi_scan(&self.bg);
                }
                self.fields.insert("ip", self.net.address.clone());
                self.fields.insert("prefix", self.net.prefix.to_string());
                self.fields.insert("gw", self.net.gateway.clone());
                self.fields.insert("dns", self.net.dns.clone());
            }
            Screen::System => {
                self.fields
                    .insert("host", sys::read_trim("/proc/sys/kernel/hostname"));
            }
            Screen::WifiPw(..) => {
                self.fields.insert("wpw", String::new());
            }
            Screen::Store => {
                self.apps = apps::catalog();
                self.fields
                    .insert("ghtok", self.apps_cfg.github_token.clone());
                if sys::compat_ready() {
                    sys::load_installed(&self.bg);
                }
            }
            Screen::Services => self.svc_at = None,
            Screen::Hardware | Screen::Setup(2) => {
                self.hw = hw::scan();
                self.selftest = crate::selftest::run();
            }
            Screen::Agent => self.check_agent(),
            Screen::AgentSet | Screen::Setup(3) => {
                self.fields.insert("akey", self.agent.api_key.clone());
                self.fields
                    .insert("amodel", self.agent.claude_model.clone());
                self.fields.insert("ep", self.agent.endpoint.clone());
            }
            _ => {}
        }
        self.stack.push((s, 0.0));
        self.focus = None;
        self.focus_reset = true;
    }

    /// Zurück: erst Tastatur schließen, dann eine Ebene nach oben.
    pub fn back(&mut self) {
        // Desktop: Zurück wirkt im obersten Fenster
        if self.desk_zurueck() {
            return;
        }
        self.back_stack();
    }

    pub(super) fn back_stack(&mut self) {
        // Dateien: Zurück geht erst den Ordnerbaum hinauf bis zur Übersicht (wie in der Demo)
        if matches!(self.stack.last().map(|s| &s.0), Some(Screen::Files)) && self.tl.cwd != "/data"
        {
            let parent = std::path::Path::new(&self.tl.cwd)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.tl.cwd = if parent.len() < 5 || !parent.starts_with("/data") {
                "/data".into()
            } else {
                parent
            };
            if let Some(l) = self.stack.last_mut() {
                l.1 = 0.0;
            }
            return;
        }
        self.focus_reset = true;
        if self.shade {
            self.shade = false;
            return;
        }
        if self.focus.take().is_some() {
            return;
        }
        if self.pending.is_some() {
            self.deny();
        } else if self.gallery_back() {
        } else if self.tl.menu.take().is_some() {
        } else if self.power_dialog {
            self.power_dialog = false;
        } else if let Some((Screen::Setup(n), _)) = self.stack.last().cloned() {
            if n > 0 {
                self.stack.pop();
                self.go(Screen::Setup(n - 1));
            }
        } else if self.mx.pinned && self.stack.len() <= 2 {
            self.toast(self.t("Pinned – hold Home to release").into());
        } else if self.stack.len() > 1 {
            if matches!(self.stack.last().map(|x| &x.0), Some(Screen::Office(_))) {
                self.office_leave();
            }
            let left = self.stack.pop();
            if matches!(left, Some((Screen::More(app_more::Mx::Light), _))) {
                self.apply_brightness();
            }
        }
    }

    fn home(&mut self) {
        if !self.set.setup_done {
            return;
        }
        if self.mx.pinned {
            self.toast(self.t("Pinned – hold Home to release").into());
            return;
        }
        if matches!(self.stack.last().map(|x| &x.0), Some(Screen::Office(_))) {
            self.office_leave();
        }
        self.stack.truncate(1);
        self.focus = None;
        self.focus_reset = true;
        self.power_dialog = false;
    }

    pub fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::JobLine(l) => {
                if let Some(j) = &mut self.job {
                    j.lines.push(l);
                    if j.lines.len() > 400 {
                        j.lines.drain(..100);
                    }
                }
            }
            Msg::JobDone(c) => {
                if let Some(j) = &mut self.job {
                    j.done = Some(c);
                }
                self.adblock_refresh();
                sys::load_installed(&self.bg);
            }
            Msg::Search(q, res) => {
                if q == self.f("q") {
                    self.results = res;
                    self.searching = false;
                    self.searched = true;
                }
            }
            Msg::Installed(s) => self.installed = s,
            Msg::TermLine(l) => {
                self.term.push(l);
                if self.term.len() > 500 {
                    self.term.drain(..100);
                }
            }
            Msg::TermDone(c) => {
                if c != 0 {
                    self.term.push(format!("[Exit {c}]"));
                }
                self.term_busy = false;
            }
            Msg::HwChanged => self.hw = hw::scan(),
            Msg::Sys(m) => self.sys_msg(m),
            Msg::Wifi(v, saved) => {
                self.wifi = v;
                self.wifi_saved = saved;
                self.wifi_scanning = false;
            }
            Msg::WifiDone(ssid, r) => {
                self.wifi_connecting = None;
                let m = match r {
                    Ok(()) => format!("{} {ssid}", self.t("Connected to")),
                    Err(e) if e == "wrong-key" => format!("{ssid}: {}", self.t("Wrong password")),
                    Err(e) if e == "8–63" => format!("{}: 8–63", self.t("Invalid input")),
                    Err(e) => format!("{ssid}: {} ({e})", self.t("Connection failed")),
                };
                self.toast(m);
                // gespeicherte Netze aktualisieren
                if let Some(ifc) = sys::wifi_iface() {
                    self.wifi_saved = crate::wifi::saved(&ifc)
                        .into_iter()
                        .map(|(_, n)| n)
                        .collect();
                }
            }
            Msg::Update(r) => {
                self.upd = Some(r);
                self.upd_checking = false;
            }
            Msg::AppsChanged => {
                self.apps = apps::catalog();
                sys::load_installed(&self.bg);
            }
            Msg::AgentUp(b) => self.agent_up = Some(b),
            Msg::AgentTool(res) => self.tool_result(res),
            Msg::AgentToolImg(res, img) => {
                if std::mem::take(&mut self.buddy.native) {
                    self.agent_busy = false;
                    self.buddy_tool_result(res);
                } else {
                    self.tool_result_img(res, img);
                }
            }
            Msg::AgentReply(Err(e)) => {
                self.chat.push((3, e));
                self.agent_busy = false;
                self.agent_up = Some(false);
            }
            Msg::AgentReply(Ok(text)) => {
                self.agent_up = Some(true);
                // Bilder sind gesehen – nicht in jeder weiteren Runde neu rechnen/senden
                agent::strip_images(&mut self.history);
                self.history
                    .push(serde_json::json!({"role": "assistant", "content": text}));
                match agent::parse(&text) {
                    Reply::Answer(a) => {
                        self.chat.push((1, a));
                        self.agent_busy = false;
                    }
                    Reply::Call(c) => {
                        self.steps += 1;
                        if self.steps > 8 {
                            self.chat.push((3, "too many steps".into()));
                            self.agent_busy = false;
                        } else {
                            self.chat.push((2, c.describe()));
                            if self.jarvis_needs_confirm(&c) {
                                self.pending = Some(c);
                            } else {
                                self.exec(c);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Ergebnis der Bildschirmtastatur ausführen.
    fn on_kb(&mut self, o: crate::kbd::KbOut) {
        use crate::kbd::KbOut;
        let Some(id) = self.focus else { return };
        if id == "office_ed" {
            self.office_kb(o);
            return;
        }
        let purpose = ultar_ime::purpose_for(id);
        // Ziffernfelder: indische Ziffern (z. B. aus eingefügtem Text) als 0–9
        let norm = |s: &str| {
            if purpose.pad() {
                ultar_ime::normalize_digits(s)
            } else {
                s.to_string()
            }
        };
        match o {
            KbOut::Insert(t) => self.fields.entry(id).or_default().push_str(&norm(&t)),
            KbOut::Backspace => self.on_key(Key::Backspace),
            KbOut::Enter => self.on_key(Key::Enter),
            KbOut::Hide => {
                self.focus = None;
                self.focus_reset = true;
            }
            KbOut::Paste(i) => {
                if let Some(t) = self.mx.cfg.clip.get(i).cloned() {
                    let t = norm(&t);
                    self.fields.entry(id).or_default().push_str(&t);
                }
            }
            KbOut::CopyField => {
                let cur = self.f(id);
                if !purpose.sensitive() {
                    self.copy_text(&cur);
                }
            }
            KbOut::Cut => {
                let cur = self.f(id);
                if !purpose.sensitive() && !cur.is_empty() {
                    self.copy_text(&cur);
                    self.fields.insert(id, String::new());
                }
            }
            KbOut::Helper => {
                let cur = self.f(id);
                self.buddy.open = true;
                if !cur.trim().is_empty() && id != "buddy" && !purpose.sensitive() {
                    self.fields.insert("buddy", cur);
                }
                self.focus = Some("buddy");
                self.focus_reset = true;
            }
            KbOut::CfgChanged => {
                let _ = crate::sys::save("keyboard.toml", &self.kb.cfg);
            }
        }
    }

    pub fn on_key(&mut self, k: Key) {
        let Some(id) = self.focus else { return };
        if id == "office_ed" {
            self.office_key(k);
            return;
        }
        match k {
            Key::Char(c) => self.fields.entry(id).or_default().push(c),
            Key::Backspace => {
                self.fields.entry(id).or_default().pop();
            }
            Key::Enter => match id {
                "q" => self.do_search(),
                "cmd" => self.run_cmd(),
                "web_q" => self.web_go(),
                "chat" => self.send_chat(),
                "note" => self.fields.entry(id).or_default().push('\n'),
                "pdf_q" => self.office_pdf_search(),
                "office_cell" => self.office_cell_enter(),
                "code_find" | "code_goto" => self.office_find_enter(id),
                "desk_q" => self.desk_enter = true,
                _ => {
                    self.focus = None;
                    self.focus_reset = true;
                }
            },
        }
    }

    /// Braucht der aktuelle Bildschirm laufende Aktualisierung (sonst schläft die Oberfläche bis zur nächsten Minute)?
    /// Bei ausgeschaltetem Bildschirm: Wecker/Timer/Schlummern prüfen. true = es klingelt (Bildschirm wecken).
    pub fn alarm_tick(&mut self) -> bool {
        let was = self.tl.ringing.is_some();
        self.tools_tick();
        self.sys_tick();
        self.more_tick();
        let call = self.buddy_tick(true) | self.voice_tick(true);
        call || (!was && self.tl.ringing.is_some())
    }

    pub fn needs_tick(&self) -> bool {
        self.toast.is_some()
            || self.sys_needs_tick()
            || self.more_needs_tick()
            || self.tools_needs_tick()
            || self.gallery_needs_tick()
            || self.office_needs_tick()
            || self.ptr_down
            || matches!(
                self.stack.last().map(|s| &s.0),
                Some(
                    Screen::Monitor
                        | Screen::Sensors
                        | Screen::Gnss
                        | Screen::Bluetooth
                        | Screen::Camera
                        | Screen::Telegram
                        | Screen::Spotify
                        | Screen::Phone
                        | Screen::Radio
                        | Screen::Cells
                        | Screen::Route
                )
            )
            || self.call_active()
            || self.agent_busy
            || self.wifi_scanning
            || self.wifi_connecting.is_some()
            || self.upd_checking
            || self.job_running()
            || matches!(
                self.stack.last().map(|s| &s.0),
                Some(
                    Screen::Services
                        | Screen::Net
                        | Screen::Storage
                        | Screen::Time
                        | Screen::System
                        | Screen::Setup(1)
                        | Screen::Setup(2)
                )
            )
    }

    /// Läuft gerade etwas Bewegtes (Kamerabild, Video, Selbstauslöser)? Dann zeichnet die Hauptschleife zügig weiter.
    pub fn animating(&self) -> bool {
        self.buddy.animating()
            || match self.stack.last().map(|s| &s.0) {
                Some(Screen::Camera) => self.camera_animating(),
                Some(Screen::Photo(_)) => self.gallery_needs_tick(),
                _ => false,
            }
    }

    /// Für Bildschirmfotos: wird im Hintergrund noch etwas geladen (Vorschaubilder)?
    pub fn shot_pending(&self) -> bool {
        self.gallery_needs_tick() || self.office_needs_tick()
    }

    /// Bildschirm geht aus: Kamera (Sensor, Licht, Aufnahme) freigeben, Video anhalten – spart Akku.
    pub fn on_sleep(&mut self) {
        self.sys_on_sleep();
        self.more_on_sleep();
        self.buddy.on_sleep();
        self.camera_leave();
        self.gallery_pause();
    }

    /// Regelmäßige Aktualisierung; true = neu zeichnen.
    pub fn tick(&mut self) -> bool {
        let mut r = self.tools_tick();
        r |= self.sys_tick();
        r |= self.more_tick();
        r |= self.buddy_tick(false);
        r |= self.voice_tick(false);
        r |= self.animating();
        r |= self.monitor_tick();
        // Finger liegt auf: weiterzeichnen, damit der lange Druck erkannt wird
        r |= self.ptr_down && !self.lp_fired;
        // Laufender Anruf: Gesprächszeit in Statusleiste/Schnelleinstellungen weiterzählen
        r |= self.call_active();
        // Mobilfunk-Bildschirme: Zustand des Funkdienstes etwa sekündlich nachziehen
        if matches!(
            self.stack.last().map(|s| &s.0),
            Some(Screen::Radio | Screen::Cells | Screen::Route)
        ) {
            r = true;
        }
        if let Some((_, t)) = &self.toast {
            if t.elapsed() > Duration::from_millis(2500) {
                self.toast = None;
                r = true;
            }
        }
        match self.stack.last().map(|s| &s.0) {
            Some(Screen::Services)
            | Some(Screen::Net)
            | Some(Screen::Storage)
            | Some(Screen::Time) => r = true,
            Some(Screen::Job) | Some(Screen::Setup(2))
                if self.job.as_ref().is_some_and(|j| j.done.is_none()) =>
            {
                r = true
            }
            Some(Screen::Setup(1)) => r = true,
            _ => {}
        }
        r
    }

    fn do_search(&mut self) {
        let q = self.f("q").trim().to_string();
        self.focus = None;
        self.focus_reset = true;
        if q.is_empty() || !sys::compat_ready() {
            return;
        }
        self.fields.insert("q", q.clone());
        self.searching = true;
        sys::search_packages(&self.bg, q);
    }

    fn run_cmd(&mut self) {
        let c = self.f("cmd").trim().to_string();
        if c.is_empty() || self.term_busy {
            return;
        }
        self.fields.insert("cmd", String::new());
        self.term.push(format!("$ {c}"));
        self.term_busy = true;
        // Sprache der Oberfläche auch im Terminal: Ultar-Werkzeuge (ULTAR_LANG), gettext (LANGUAGE),
        // Ubuntu-Programme (LANG, nur wenn die Locale installiert ist: locale-gen ar_SA.UTF-8)
        let k = match self.set.lang.as_str() {
            "ar" => "ar",
            "en" => "en",
            _ => "de",
        };
        let loc = match k {
            "ar" => "ar_SA",
            "en" => "en_US",
            _ => "de_DE",
        };
        let hat_locale = ["/usr/lib/locale", "/compat/usr/lib/locale"]
            .iter()
            .any(|d| std::path::Path::new(&format!("{d}/{loc}.utf8")).exists());
        let c = format!(
            "export ULTAR_LANG={k} LANGUAGE={k}{}; {c}",
            if hat_locale {
                format!(" LANG={loc}.UTF-8")
            } else {
                String::new()
            }
        );
        if sys::compat_ready() {
            self.bg.run(
                sys::COMPAT,
                vec![
                    "run".into(),
                    "--".into(),
                    "/bin/bash".into(),
                    "-c".into(),
                    c,
                ],
                Msg::TermLine,
                Msg::TermDone,
            );
        } else {
            self.bg.run(
                "/bin/sh",
                vec!["-c".into(), c],
                Msg::TermLine,
                Msg::TermDone,
            );
        }
    }

    fn start_job(&mut self, title: String, args: &[&str]) {
        self.job = Some(Job {
            title,
            lines: Vec::new(),
            done: None,
        });
        self.bg.run(
            sys::COMPAT,
            args.iter().map(|s| s.to_string()).collect(),
            Msg::JobLine,
            Msg::JobDone,
        );
        self.go(Screen::Job);
    }

    // ---------------- Agent ----------------

    fn check_agent(&mut self) {
        if self.agent.backend == "claude" {
            // erreichbar gilt, sobald Schlüssel und Internet da sind; die erste Anfrage zeigt Fehler im Verlauf
            self.agent_up = Some(!self.agent.api_key.trim().is_empty() && sys::online());
            return;
        }
        let (bg, ep) = (self.bg.clone(), self.agent.endpoint.clone());
        std::thread::spawn(move || bg.send(Msg::AgentUp(agent::reachable(&ep))));
    }

    fn save_agent(&mut self) {
        if let Err(e) = sys::save("agent.toml", &self.agent) {
            self.toast(format!("{}: {e}", self.t("Could not save")));
        }
        if agent::model_installed() {
            sys::ctl(if self.agent.enabled && self.agent.backend != "claude" {
                "start llm"
            } else {
                "stop llm"
            });
        }
    }

    fn request(&mut self) {
        self.agent_busy = true;
        let (bg, cfg, hist) = (self.bg.clone(), self.agent.clone(), self.history.clone());
        std::thread::spawn(move || {
            let r = if cfg.backend == "claude" {
                agent::complete_claude(&cfg, &hist)
            } else {
                agent::complete(&cfg.endpoint, &hist)
            };
            bg.send(Msg::AgentReply(r))
        });
    }

    fn send_chat(&mut self) {
        let text = self.f("chat").trim().to_string();
        if (text.is_empty() && self.jv.attach.is_empty()) || self.agent_busy || !self.agent.enabled
        {
            return;
        }
        self.fields.insert("chat", String::new());
        self.focus = None;
        self.focus_reset = true;
        self.jv.attach_open = false;
        // Mitgebrachtes (Fotos, Bildschirm, Dateien) gehört zur Nachricht
        let (msg, shown) = self.jarvis_take_attachments(&text);
        self.chat.push((0, shown));
        self.history.push(msg);
        if self.history.len() > 24 {
            self.history.drain(..self.history.len() - 24);
        }
        self.steps = 0;
        self.request();
    }

    fn tool_result(&mut self, res: String) {
        // Hat der Helfer das Werkzeug selbst ausgelöst (ohne Sprachmodell), geht das Ergebnis an ihn zurück
        if std::mem::take(&mut self.buddy.native) {
            self.agent_busy = false;
            return self.buddy_tool_result(res);
        }
        let res: String = res.chars().take(1500).collect();
        self.history
            .push(serde_json::json!({"role": "user", "content": format!("TOOL_RESULT: {res}")}));
        self.request();
    }

    fn deny(&mut self) {
        if self.pending.take().is_some() {
            self.tool_result("The user denied this action.".into());
        }
    }

    /// Werkzeug ausführen. Kurze Aktionen sofort, lange im Hintergrund.
    fn exec(&mut self, c: Call) {
        if let Some(res) = self.exec_apps(&c) {
            return self.tool_result(res);
        }
        // System-Assistent: Sehen, Root-Werkzeuge, Sicherheit, Modelle (Err = Ergebnis kommt später/mit Bild)
        match self.exec_jarvis(&c) {
            Some(Ok(res)) => return self.tool_result(res),
            Some(Err(())) => return,
            None => {}
        }
        let bg = self.bg.clone();
        let later = |f: Box<dyn FnOnce() -> String + Send>| {
            std::thread::spawn(move || bg.send(Msg::AgentTool(f())));
        };
        let res: String = match c.tool.as_str() {
            "get_status" => {
                let dt = sys::from_unix(sys::unix_now(), self.set.tz_min);
                let (mt, ma) = sys::meminfo();
                let net: Vec<String> = sys::net_status()
                    .iter()
                    .map(|n| format!("{} {} {}", n.name, n.state, n.ip))
                    .collect();
                let disk = |p: &str| {
                    sys::disk(p)
                        .map(|d| format!("{} free of {}", sys::human(d.1), sys::human(d.0)))
                        .unwrap_or_else(|| "n/a".into())
                };
                format!("time {:04}-{:02}-{:02} {:02}:{:02}; network [{}]; ram {} free of {}; system {}; ubuntu area {}; user data {}; battery {:?}; language {}; brightness {}%; dark {}",
                    dt.y, dt.mo, dt.d, dt.h, dt.mi, net.join(", "), sys::human(ma), sys::human(mt), disk("/"), disk("/compat"), disk("/data"), sys::battery(), self.set.lang, (self.set.brightness * 100.0).round(), self.set.dark)
            }
            "set_setting" => {
                let (name, v) = (c.arg("name"), c.arg("value"));
                let num = v.trim().trim_end_matches('%').parse::<f32>();
                let ok = match (name.as_str(), num) {
                    ("brightness", Ok(n)) if (1.0..=100.0).contains(&n) => {
                        self.set.brightness = (n / 100.0).max(0.1);
                        self.apply_brightness();
                        true
                    }
                    ("dark", _) if v == "true" || v == "false" => {
                        self.set.dark = v == "true";
                        true
                    }
                    ("language", _) if ["ar", "en", "de"].contains(&v.as_str()) => {
                        self.set.lang = v.clone();
                        true
                    }
                    ("rotation", Ok(n)) if [0.0, 90.0, 180.0, 270.0].contains(&n) => {
                        self.set.rotation = n as u16;
                        true
                    }
                    ("scale", Ok(n)) if (85.0..=130.0).contains(&n) => {
                        self.set.scale = n / 100.0;
                        true
                    }
                    ("timeout", Ok(n)) if n >= 0.0 => {
                        self.set.timeout_s = n as u32;
                        true
                    }
                    ("timezone", Ok(n)) if (-720.0..=840.0).contains(&n) => {
                        self.set.tz_min = n as i32;
                        true
                    }
                    ("volume", Ok(n)) if (0.0..=100.0).contains(&n) => {
                        self.set.volume = n / 100.0;
                        self.set.sound = n > 0.0;
                        true
                    }
                    _ => false,
                };
                if ok {
                    self.save();
                    format!("ok: {name} = {v}")
                } else {
                    format!("error: invalid setting {name}={v}")
                }
            }
            "open_screen" => {
                let scr = match c.arg("name").as_str() {
                    "home" => None,
                    "settings" => Some(Screen::Settings),
                    "network" => Some(Screen::Net),
                    "display" => Some(Screen::Display),
                    "language" => Some(Screen::Lang),
                    "time" => Some(Screen::Time),
                    "storage" => Some(Screen::Storage),
                    "services" => Some(Screen::Services),
                    "system" => Some(Screen::System),
                    "packages" => Some(Screen::Store),
                    "terminal" => Some(Screen::Terminal),
                    "hardware" => Some(Screen::Hardware),
                    _ => return self.tool_result("error: unknown screen".into()),
                };
                match scr {
                    Some(s) => self.go(s),
                    None => self.home(),
                }
                "ok".into()
            }
            "list_services" => sys::ctl("status"),
            "service" => {
                let (a, n) = (c.arg("action"), c.arg("name"));
                if !["start", "stop", "restart"].contains(&a.as_str())
                    || n == "ui"
                    || !n
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                {
                    "error: not allowed".into()
                } else {
                    sys::ctl(&format!("{a} {n}"))
                }
            }
            "search_packages" => {
                let q = c.arg("query");
                return later(Box::new(move || {
                    let out = sys::capture(
                        sys::COMPAT,
                        &[
                            "run",
                            "--",
                            "/usr/bin/apt-cache",
                            "search",
                            "--names-only",
                            "--",
                            &q,
                        ],
                    );
                    let v: Vec<&str> = out.lines().take(12).collect();
                    if v.is_empty() {
                        "no results".into()
                    } else {
                        v.join("\n")
                    }
                }));
            }
            "install_package" | "remove_package" => {
                let name = c.arg("name");
                if !sys::valid_pkg(&name) {
                    "error: invalid package name".into()
                } else {
                    let verb = if c.tool == "install_package" {
                        "install"
                    } else {
                        "remove"
                    };
                    return later(Box::new(move || {
                        let out = std::process::Command::new(sys::COMPAT)
                            .args(["apt", verb, "--no-install-recommends", &name])
                            .output();
                        match out {
                            Ok(o) if o.status.success() => format!("ok: {verb} {name}"),
                            Ok(o) => format!(
                                "error: {}",
                                String::from_utf8_lossy(&o.stderr)
                                    .chars()
                                    .rev()
                                    .take(400)
                                    .collect::<String>()
                                    .chars()
                                    .rev()
                                    .collect::<String>()
                            ),
                            Err(e) => format!("error: {e}"),
                        }
                    }));
                }
            }
            "run_command" => {
                let cmd = c.arg("command");
                return later(Box::new(move || {
                    let wrapped = format!("({cmd}) 2>&1");
                    let out = if sys::compat_ready() {
                        sys::capture(sys::COMPAT, &["run", "--", "/bin/bash", "-c", &wrapped])
                    } else {
                        sys::capture("/bin/sh", &["-c", &wrapped])
                    };
                    if out.trim().is_empty() {
                        "(no output)".into()
                    } else {
                        out
                    }
                }));
            }
            "power" => match c.arg("action").as_str() {
                "reboot" => sys::ctl("reboot"),
                "poweroff" => sys::ctl("poweroff"),
                _ => "error: unknown action".into(),
            },
            other => format!("error: unknown tool {other}"),
        };
        self.tool_result(res);
    }

    fn start_driver_search(&mut self) {
        self.job = Some(Job {
            title: self.t("Driver search").into(),
            lines: Vec::new(),
            done: None,
        });
        hw::fix_online(&self.bg);
    }

    fn job_running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.done.is_none())
    }

    // ------------------------------------------------------------------

    /// Inhalt eines Bildschirms in `area` zeichnen (Vollbild oder Fenster der Desktop-Oberfläche)
    pub(super) fn draw_screen(&mut self, ui: &mut Ui, scr: &Screen, area: Rect, sc: &mut f32) {
        match &scr {
            Screen::Home => self.s_home(ui, area),
            Screen::Settings => self.s_settings(ui, area, sc),
            Screen::Net => self.s_net(ui, area, sc),
            Screen::Display => self.s_display(ui, area, sc),
            Screen::Lang => self.s_lang(ui, area),
            Screen::Time => self.s_time(ui, area, sc),
            Screen::Storage => self.s_storage(ui, area),
            Screen::Services => self.s_services(ui, area, sc),
            Screen::System => self.s_system(ui, area, sc),
            Screen::Store => self.s_store(ui, area, sc),
            Screen::Pkg(n, d) => self.s_pkg(ui, area, n, d),
            Screen::Job => self.s_job(ui, area),
            Screen::Terminal => self.s_terminal(ui, area),
            Screen::Setup(n) => self.s_setup(ui, area, *n, sc),
            Screen::Hardware => self.s_hardware(ui, area, sc),
            Screen::Agent => self.s_agent(ui, area, sc),
            Screen::AgentSet => self.s_agent_set(ui, area, sc),
            Screen::WifiPw(n, sec) => self.s_wifi_pw(ui, area, n, *sec),
            Screen::Update => self.s_update(ui, area),
            Screen::Calc => self.s_calc(ui, area, sc),
            Screen::Clock => self.s_clock(ui, area, sc),
            Screen::Files => self.s_files(ui, area, sc),
            Screen::Viewer(p) => self.s_viewer(ui, area, p, sc),
            Screen::Gallery => self.s_gallery(ui, area, sc),
            Screen::Privacy => self.s_privacy(ui, area, sc),
            Screen::Monitor => self.s_monitor(ui, area, sc),
            Screen::Rename(p) => self.s_rename(ui, area, p),
            Screen::Sound => self.s_sound(ui, area),
            Screen::Web(u) => self.s_web(ui, area, u),
            Screen::WebApp(k) => self.s_webapp(ui, area, k.clone(), sc),
            Screen::Helper => self.s_helper(ui, area, sc),
            Screen::Sensors => self.s_sensors(ui, area, sc),
            Screen::Gnss => self.s_gnss(ui, area, sc),
            Screen::Bluetooth => self.s_bluetooth(ui, area, sc),
            Screen::Phone => self.s_phone(ui, area),
            Screen::Messages => self.s_messages(ui, area, sc),
            Screen::MessageNew => self.s_message_new(ui, area),
            Screen::Camera => self.s_camera(ui, area),
            Screen::Telegram => self.s_telegram(ui, area),
            Screen::Spotify => self.s_spotify(ui, area, sc),
            Screen::Security => self.s_security(ui, area, sc),
            Screen::Account => self.s_account(ui, area, sc),
            Screen::Vpn => self.s_vpn(ui, area, sc),
            Screen::Verbund => self.s_verbund(ui, area, sc),
            Screen::Zellen => self.s_zellen(ui, area, sc),
            Screen::Zelle(n) => self.s_zelle(ui, area, n.clone(), sc),
            Screen::Gpu => self.s_gpu(ui, area, sc),
            Screen::Empfehlung(d) => self.s_empfehlung(ui, area, d.clone(), sc),
            Screen::Weitergeben(d) => self.s_weitergeben(ui, area, d.clone(), sc),
            Screen::StoreCloud => self.s_store_cloud(ui, area, sc),
            Screen::Updates => self.s_updates(ui, area),
            Screen::Radio => self.s_radio(ui, area, sc),
            Screen::Cells => self.s_cells(ui, area, sc),
            Screen::Route => self.s_route(ui, area, sc),
            Screen::Apps => self.s_all_apps(ui, area, sc),
            Screen::Photo(p) => self.s_photo(ui, area, p),
            Screen::Notes => self.s_notes(ui, area, sc),
            Screen::NoteEdit(i) => self.s_note_edit(ui, area, *i, sc),
            Screen::Contacts => self.s_contacts_demo(ui, area, sc),
            Screen::ContactEdit(Some(i)) => self.s_contact_demo(ui, area, *i, sc),
            Screen::ContactEdit(None) => {
                // „Neuer Kontakt" ist ein Blatt über der Kontaktliste
                self.stack.pop();
                let ok = match self.sheet {
                    Some(app_demo::Sheet::NewContact) => {
                        matches!(self.stack.last().map(|s| &s.0), Some(Screen::Contacts))
                    }
                    Some(app_demo::Sheet::NewAlarm) => {
                        matches!(self.stack.last().map(|s| &s.0), Some(Screen::Clock))
                    }
                    Some(app_demo::Sheet::NewEvent) => matches!(
                        self.stack.last().map(|s| &s.0),
                        Some(Screen::Sys(app_sys::Sx::Calendar))
                    ),
                    None => true,
                };
                if !ok {
                    self.stack.push((Screen::Contacts, 0.0));
                }
                self.new_contact_sheet();
                ui.dirty = true;
            }
            Screen::UbuntuApps => self.s_ubuntu_apps(ui, area, sc),
            Screen::Sys(s) => self.s_sys(ui, area, s.clone(), sc),
            Screen::More(m) => self.s_more(ui, area, m.clone(), sc),
            Screen::Help(t) => self.s_help(ui, area, t.clone(), sc),
            Screen::Office(x) => self.s_office(ui, area, x.clone(), sc),
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        // Für den Assistenten festhalten, was der Nutzer gerade sieht (Text + verkleinertes Bild)
        let capture = self.jarvis_capture_due();
        if capture {
            ui.seen = Some(Vec::new());
        }
        self.draw_screens(ui);
        if capture {
            self.jarvis_store(ui);
        }
    }

    fn draw_screens(&mut self, ui: &mut Ui) {
        ui.set_clip(None);
        // Hintergrund mit sanftem Verlauf für Tiefe (statt flacher Füllung)
        ui.cv
            .vgrad(Rect::new(0.0, 0.0, ui.w, ui.h), ui.th.bg, ui.th.bg2);
        ui.rtl = self.set.lang == "ar";
        ui.focus = self.focus;
        if self.sx.cfg.contrast {
            ui.th.sub = ui.th.text;
            ui.th.line = ui.th.text.alpha(0.55);
        }
        if ui.p.pressed && self.sx.cfg.haptic {
            sys::vibrate(12);
        }
        self.more_account();
        // Laptop/Server: Desktop-Oberfläche mit Fenstern statt Handy-Oberfläche (nach der Einrichtung)
        self.desk.an = false;
        if self.set.setup_done && self.desktop_modus(ui) {
            let dialog_was_open = self.power_dialog;
            ui.enabled = !self.power_dialog && self.pending.is_none();
            self.draw_desktop(ui);
            ui.enabled = true;
            if self.power_dialog {
                self.d_power(ui, dialog_was_open);
                ui.dirty |= !dialog_was_open;
            }
            if self.pending.is_some() {
                self.d_confirm(ui);
            }
            return;
        }
        // Der Helfer ruft auch über dem Sperrbildschirm an; solange gehört die Berührung ihm
        let calling = self.buddy.is_calling();
        ui.enabled = !calling;
        let locked = self.sys_lock_draw(ui);
        ui.enabled = true;
        if locked {
            self.more_lock_extra(ui);
            if calling {
                self.d_buddy(ui, ui.dp(30.0), ui.h - ui.dp(52.0));
            }
            return;
        }
        let sb = ui.dp(30.0);
        let nb = ui.dp(52.0);
        let kbh = match self.focus {
            Some(f) => self.kb.height(ui, f),
            None => 0.0,
        };
        ui.enabled = !self.power_dialog
            && !self.shade
            && self.pending.is_none()
            && self.tl.menu.is_none()
            && !self.buddy.blocks(ui.p);
        // Berührungsdauer mitführen (für den langen Druck)
        if ui.p.pressed {
            self.press_at = Some(Instant::now());
            self.lp_fired = false;
        }
        if !ui.p.down && !ui.p.released {
            self.press_at = None;
        }
        self.ptr_down = ui.p.down;
        let in_setup = matches!(self.stack.last(), Some((Screen::Setup(_), _)));
        let nb = if in_setup { 0.0 } else { nb };
        let area = Rect::new(0.0, sb, ui.w, ui.h - sb - nb - kbh);
        let dialog_was_open = self.power_dialog;

        let scr = self.stack.last().unwrap().0.clone();
        let mut sc = self.stack.last().unwrap().1;
        // ---- Bildschirmtastatur: nichts darf dahinter verschwinden ----
        // Neues Feld fokussiert → in den sichtbaren Bereich holen. Ragt eine Seite ohne eigenes Rollen
        // unter die Tastatur, wird sie in voller Höhe gezeichnet und im Bereich über der Tastatur rollbar.
        if self.focus != self.kb_last {
            self.kb_last = self.focus;
            self.kb_ensure = self.focus.is_some();
        }
        if kbh <= 0.0 {
            self.kb_pan = (None, 0.0);
        }
        ui.kb.reach.set(0.0);
        ui.kb.focus.set(None);
        ui.kb.ensure.set(self.kb_ensure && kbh > 0.0);
        let vis = area;
        let full = Rect::new(0.0, sb, ui.w, ui.h - sb - nb);
        let panning = kbh > 0.0 && self.kb_pan.0.as_ref() == Some(&scr);
        let mut pan = if panning { self.kb_pan.1 } else { 0.0 };
        let area = if panning {
            ui.scroll_begin(vis, &mut pan);
            Rect::new(full.x, full.y - pan, full.w, full.h)
        } else {
            area
        };
        // Kamera und Betrachter sind Vollbild-Apps auf Schwarz: dunkles Farbschema, egal was eingestellt ist
        let immersive = matches!(scr, Screen::Camera | Screen::Photo(_))
            || (scr == Screen::Home && matches!(self.wall, Some(Some(_))));
        let th_user = ui.th;
        if immersive {
            ui.th = crate::ui::theme(true, self.set.accent);
        }
        if !matches!(scr, Screen::Camera) {
            self.camera_leave();
        }
        let page_enabled = ui.enabled;
        if self.sheet.is_some() {
            ui.enabled = false;
        }
        // Schnelleinstellungen offen: sie decken alles ab – darunter nichts zeichnen (spart ein ganzes Bild)
        if !self.shade {
            self.draw_screen(ui, &scr, area, &mut sc);
        }
        if kbh > 0.0 {
            let fr = ui.kb.focus.get();
            if panning {
                ui.kb.ensure.set(false);
                ui.scroll_end(vis, full.h, &mut pan);
                if self.kb_ensure && !ui.p.down {
                    if let Some((fr, _)) = fr {
                        let m = ui.dp(14.0);
                        // Feld und das Nächste darunter (meist „Speichern") zeigen – das Feld selbst bleibt immer sichtbar
                        let extra = ui.dp(76.0).min((vis.h - fr.h - 2.0 * m).max(0.0));
                        let d = if fr.bottom() + extra + m > vis.bottom() {
                            (fr.bottom() + extra + m - vis.bottom())
                                .min((fr.y - m - vis.y).max(0.0))
                        } else if fr.y - m < vis.y {
                            fr.y - m - vis.y
                        } else {
                            0.0
                        };
                        let np = (pan + d).clamp(0.0, (full.h - vis.h).max(0.0));
                        if (np - pan).abs() > 0.5 {
                            pan = np;
                            ui.dirty = true;
                        } else {
                            self.kb_ensure = false;
                        }
                    }
                }
                self.kb_pan.1 = pan;
            } else {
                self.kb_ensure = ui.kb.ensure.get();
                let hidden = fr.is_some_and(|(r, c)| c.is_none() && r.bottom() > vis.bottom());
                if ui.kb.reach.get() > vis.bottom() + 1.0 || hidden {
                    self.kb_pan = (Some(scr.clone()), 0.0);
                    self.kb_ensure = true;
                    ui.dirty = true;
                }
            }
        }
        if std::mem::take(&mut self.mx.to_top) && sc > 0.0 {
            sc = 0.0;
            ui.dirty = true;
        }
        if let Some(l) = self.stack.last_mut() {
            if l.0 == scr {
                l.1 = sc;
            }
        }
        ui.set_clip(None);
        ui.enabled = page_enabled;
        if self.sheet.is_some() {
            if !matches!(self.stack.last().map(|s| &s.0), Some(Screen::Contacts)) {
                self.sheet = None;
            } else {
                self.d_sheet(ui, sb, ui.h - nb - kbh);
            }
        }
        // Helfer über der Seite (vor Leisten/Tastatur-Abgleich, damit sein Eingabefeld den Fokus bekommt)
        if self.set.setup_done && !matches!(scr, Screen::Setup(_)) {
            self.d_buddy(ui, sb, ui.h - nb - kbh);
        }
        self.status_bar(ui, sb);
        if !in_setup {
            self.nav_bar(ui, nb);
        }
        ui.th = th_user;

        if self.focus_reset {
            self.focus_reset = false;
            ui.focus = self.focus;
            ui.dirty = true;
        } else {
            self.focus = ui.focus;
        }
        if self.focus.is_some() && kbh > 0.0 {
            let ka = Rect::new(0.0, ui.h - nb - kbh, ui.w, kbh);
            let id = self.focus.unwrap_or("");
            let cur = self.f(id);
            let lang = self.set.lang.clone();
            let clip = self.mx.cfg.clip.clone();
            let tr = |k: &'static str| crate::i18n::tr(&lang, k);
            if let Some(o) = crate::kbd::keyboard(ui, ka, &mut self.kb, &lang, id, &cur, &clip, &tr)
            {
                self.on_kb(o);
            }
        }
        if self.power_dialog {
            // Frisch geöffnet: dieselbe Berührung darf nicht gleich eine Schaltfläche auslösen
            self.d_power(ui, dialog_was_open);
            ui.dirty |= !dialog_was_open;
        }
        if self.pending.is_some() {
            self.d_confirm(ui);
        }
        self.d_file_menu(ui);
        self.d_ring(ui);
        // Wischgesten: vom linken/rechten Rand zur Mitte = zurück; von der unteren Leiste nach oben = Startbildschirm
        if ui.p.released && ui.p.moved && ui.p.capture.is_none() && self.tl.ringing.is_none() {
            let (dx, dy) = (ui.p.x - ui.p.px, ui.p.y - ui.p.py);
            let (edge, far) = (ui.dp(24.0), ui.dp(90.0));
            // Wisch von ganz oben nach unten = Schnelleinstellungen öffnen
            if !self.shade && ui.p.py < ui.dp(40.0) && dy > far && dx.abs() < dy.abs() * 0.7 {
                self.shade = true;
                ui.dirty = true;
            } else if self.shade && dy < -far && dx.abs() < dy.abs() * 0.7 {
                self.shade = false;
                ui.dirty = true;
            } else if ((ui.p.px < edge && dx > far) || (ui.p.px > ui.w - edge && dx < -far))
                && dy.abs() < dx.abs() * 0.6
            {
                self.back();
                ui.dirty = true;
            } else if ui.p.py > ui.h - ui.dp(52.0) && dy < -far && dx.abs() < dy.abs() * 0.6 {
                self.home();
                ui.dirty = true;
            }
        }
        // Schnelleinstellungen/Benachrichtigungen über allem zeichnen
        if self.shade {
            self.d_shade(ui);
        }
        // Kopf-Hinweis (heads-up) für eine neue Benachrichtigung, ~4 s
        if !self.shade {
            self.d_heads(ui);
        }
        if let Some((s, _)) = &self.toast {
            let s = s.clone();
            let w = (ui.text_w(&s, 14.0, false) + ui.dp(36.0)).min(ui.w - ui.dp(24.0));
            let r = Rect::new(
                (ui.w - w) / 2.0,
                ui.h - nb - kbh - ui.dp(58.0),
                w,
                ui.dp(42.0),
            );
            ui.cv.rrect(r, ui.dp(21.0), ui.th.text);
            let c = ui.th.bg;
            ui.label(r.inset(ui.dp(10.0)), &s, 14.0, c, false, Al::Center);
        }
        self.sys_overlay(ui);
    }

    fn status_bar(&mut self, ui: &mut Ui, h: f32) {
        let dt = sys::from_unix(sys::unix_now(), self.set.tz_min);
        let r = Rect::new(ui.dp(16.0), 0.0, ui.w - ui.dp(32.0), h);
        let c = ui.th.text;
        let clock = self.clock_str(dt.h, dt.mi);
        ui.label(r, &clock, 13.0, c, true, Al::Start);
        // Netz-Pille direkt neben der Uhr (Tippen = Funkturm-Wahl, lange drücken = Mobilfunk-Zentrum)
        if !matches!(self.stack.last(), Some((Screen::Setup(_), _))) {
            let cw = ui.text_w(&clock, 13.0, true) + ui.dp(10.0);
            let px = if ui.rtl { r.right() - cw } else { r.x + cw };
            if self.call_active() {
                self.call_pill(ui, px, h);
            } else if self.mobilfunk_vorhanden() {
                // ohne Modem/SIM keine Mobilfunk-Anzeige
                self.net_pill(ui, px, h);
            }
        }
        let on = sys::online();
        let mut tw = 0.0;
        if let Some((cap, chg)) = sys::battery() {
            // Akkusymbol mit Füllstand, daneben Prozent; beim Laden grün, unter 15 % rot
            let info = format!("{cap} %");
            let txw = ui.text_w(&info, 13.0, false);
            let (bw, bh) = (ui.dp(24.0), ui.dp(12.0));
            let bx = if ui.rtl {
                r.x
            } else {
                r.right() - bw - ui.dp(2.0)
            };
            let by = (h - bh) / 2.0;
            ui.cv.rrect(Rect::new(bx, by, bw, bh), ui.dp(3.0), c);
            ui.cv.rrect(
                Rect::new(
                    bx + ui.dp(1.5),
                    by + ui.dp(1.5),
                    bw - ui.dp(3.0),
                    bh - ui.dp(3.0),
                ),
                ui.dp(2.0),
                ui.th.bg,
            );
            ui.cv.rrect(
                Rect::new(bx + bw, by + bh * 0.3, ui.dp(2.0), bh * 0.4),
                ui.dp(1.0),
                c,
            );
            let fillc = if chg {
                ui.th.ok
            } else if cap < 15 {
                ui.th.danger
            } else {
                c
            };
            let fw = ((bw - ui.dp(6.0)) * cap as f32 / 100.0).max(ui.dp(2.0));
            ui.cv.rrect(
                Rect::new(bx + ui.dp(3.0), by + ui.dp(3.0), fw, bh - ui.dp(6.0)),
                ui.dp(1.0),
                fillc,
            );
            let lr = if ui.rtl {
                Rect::new(r.x + bw + ui.dp(8.0), 0.0, txw, h)
            } else {
                Rect::new(r.right() - bw - ui.dp(8.0) - txw, 0.0, txw, h)
            };
            let old = ui.rtl;
            ui.rtl = false;
            ui.label(lr, &info, 13.0, c, false, Al::Start);
            ui.rtl = old;
            tw = bw + ui.dp(8.0) + txw;
        }
        // Funk-Anzeigen links des Akkus: Mobilfunk-Punkt, WLAN-Balken, VPN-Schloss
        let dir = if ui.rtl { 1.0 } else { -1.0 };
        let mut ix = if ui.rtl {
            r.x + tw + ui.dp(14.0)
        } else {
            r.right() - tw - ui.dp(14.0)
        };
        let step = ui.dp(18.0);

        // WLAN: Bögen-Symbol (hell = verbunden, gedämpft = an, aber ohne Verbindung; aus = nicht gezeigt)
        let wifi_on = quick::wifi_on();
        if wifi_on || on {
            let wcol = if on { ui.th.text } else { ui.th.sub.alpha(0.7) };
            match self.sx.rssi.filter(|_| on) {
                // Echte Signalstärke: gedämpftes Symbol, darüber von unten so viele Bögen hell wie das Signal hergibt
                Some(rssi) => {
                    let lvl = ((rssi + 90) as f32 / 35.0).clamp(0.2, 1.0);
                    let s = ui.dp(15.0);
                    ui.icon(Ic::Wifi, ix, h / 2.0, s, ui.th.sub.alpha(0.45));
                    let top = h / 2.0 + s * 0.6 - s * 1.2 * lvl;
                    ui.cv.set_clip(Some(Rect::new(ix - s, top, 2.0 * s, h)));
                    ui.icon(Ic::Wifi, ix, h / 2.0, s, wcol);
                    ui.cv.set_clip(None);
                }
                None => ui.icon(Ic::Wifi, ix, h / 2.0, ui.dp(15.0), wcol),
            }
            ix += dir * step;
        }

        // VPN: kleines Schloss/Schild in Gold, wenn aktiv
        if crate::sys::vpn_status().up {
            ui.icon(Ic::Shield, ix, h / 2.0, ui.dp(13.0), ui.th.accent);
            ix += dir * step;
        }
        // Mobile Daten: Pfeile, wenn aktiv (Netz und Signal zeigt die Netz-Pille links)
        if quick::mobile_on() {
            ui.icon(Ic::UpDown, ix, h / 2.0, ui.dp(12.0), ui.th.text);
            ix += dir * step;
        }
        self.status_icons(ui, ix, dir, h);
    }

    fn nav_bar(&mut self, ui: &mut Ui, h: f32) {
        let r = Rect::new(0.0, ui.h - h, ui.w, h);
        let immersive = matches!(
            self.stack.last().map(|s| &s.0),
            Some(Screen::Camera | Screen::Photo(_))
        );
        ui.cv.rect(r, if immersive { rgb(0) } else { ui.th.bg });
        if !immersive {
            ui.cv.rect(Rect::new(0.0, r.y, ui.w, 1.0), ui.th.line);
        }
        let old = ui.enabled;
        ui.enabled = true;
        let third = ui.w / 3.0;
        let (rb, rh) = if ui.rtl {
            (
                Rect::new(2.0 * third, r.y, third, h),
                Rect::new(third, r.y, third, h),
            )
        } else {
            (
                Rect::new(0.0, r.y, third, h),
                Rect::new(third, r.y, third, h),
            )
        };
        let can_back = self.stack.len() > 1
            || self.focus.is_some()
            || self.power_dialog
            || self.pending.is_some();
        let c = ui.th.text;
        let cs = ui.th.sub.alpha(0.4);
        ui.icon(
            Ic::Back,
            rb.cx(),
            rb.cy(),
            ui.dp(22.0),
            if can_back { c } else { cs },
        );
        ui.icon(
            Ic::Home,
            rh.cx(),
            rh.cy(),
            ui.dp(22.0),
            if self.mx.pinned { ui.th.accent } else { c },
        );
        // Dritte Taste: zuletzt verwendete Apps
        let rr = if ui.rtl {
            Rect::new(0.0, r.y, third, h)
        } else {
            Rect::new(2.0 * third, r.y, third, h)
        };
        let sq = ui.dp(8.5);
        ui.cv.rrect(
            Rect::new(rr.cx() - sq, rr.cy() - sq, 2.0 * sq, 2.0 * sq),
            ui.dp(4.0),
            if self.mx.pinned { cs } else { c },
        );
        ui.cv.rrect(
            Rect::new(rr.cx() - sq, rr.cy() - sq, 2.0 * sq, 2.0 * sq).inset(ui.dp(2.2)),
            ui.dp(2.5),
            if immersive { rgb(0) } else { ui.th.bg },
        );
        if !self.mx.pinned && ui.tap(rr) {
            if matches!(
                self.stack.last().map(|s| &s.0),
                Some(Screen::More(app_more::Mx::Recents))
            ) {
                self.back();
            } else if self.set.setup_done {
                self.focus = None;
                self.focus_reset = true;
                self.go(Screen::More(app_more::Mx::Recents));
            }
            ui.dirty = true;
        }
        if self.mx.pinned && self.long_press(ui, rh) {
            // Anheften lösen; mit PIN wird danach gesperrt
            self.mx.pinned = false;
            if !self.sx.cfg.pin_hash.is_empty() {
                self.sx.locked = true;
            }
            self.toast(self.t("Unpinned").into());
            ui.dirty = true;
        }
        if can_back && ui.tap(rb) {
            self.back();
            ui.dirty = true;
        }
        if ui.tap(rh) {
            self.home();
            ui.dirty = true;
        }
        ui.enabled = old;
    }

    /// Kopfzeile wie in der Web-Demo: Zurück-Pfeil, Titel, rechts (in RTL links) der Hilfe-Knopf
    fn header(&mut self, ui: &mut Ui, area: Rect, title: &str) -> Rect {
        if self.no_hdr {
            return area;
        }
        let h = ui.dp(56.0);
        let c = ui.th.text;
        let cy = area.y + h / 2.0;
        let help = if self.set.help_btn {
            self.cur_help()
        } else {
            None
        };
        // Zurück
        let bx = if ui.rtl {
            ui.w - ui.dp(30.0)
        } else {
            ui.dp(30.0)
        };
        let br = Rect::new(bx - ui.dp(22.0), cy - ui.dp(22.0), ui.dp(44.0), ui.dp(44.0));
        if ui.held(br) {
            ui.cv.circle(bx, cy, ui.dp(19.0), ui.th.press);
        }
        ui.icon(Ic::Back, bx, cy, ui.dp(20.0), c);
        if ui.tap(br) {
            self.back();
            ui.dirty = true;
        }
        let side = ui.dp(56.0);
        // Aktion oben am Ende wie in der Web-Demo: Kontakte/Notizen „+", Nachrichten „Schreiben"
        let act = match self.stack.last().map(|s| &s.0) {
            Some(Screen::Contacts) => Some(Ic::Plus),
            Some(Screen::ContactEdit(Some(_))) => Some(Ic::Trash),
            Some(Screen::Calc) => Some(Ic::Clock),
            Some(Screen::Gallery) => Some(Ic::Camera),
            Some(Screen::Terminal) => Some(Ic::Trash),
            Some(Screen::Helper) => Some(Ic::Gear),
            Some(Screen::Sys(app_sys::Sx::Calendar)) => Some(Ic::Plus),
            Some(Screen::Files) if self.tl.cwd != "/data" => Some(Ic::Plus),
            Some(Screen::Messages) if self.msg.open.is_none() => Some(Ic::Pen),
            _ => None,
        };
        self.hdr_tap = false;
        if let Some(ic) = act {
            let ax = if ui.rtl {
                ui.dp(28.0)
            } else {
                ui.w - ui.dp(28.0)
            };
            if ui.icon_btn(ax, cy, ui.dp(16.0), ic, ui.th.card.alpha(0.0), c) {
                self.hdr_tap = true;
                ui.dirty = true;
            }
        }
        let shift = if act.is_some() { ui.dp(44.0) } else { 0.0 };
        let tw = ui.w
            - side
            - shift
            - if help.is_some() {
                ui.dp(56.0)
            } else {
                ui.dp(16.0)
            };
        let tx = if ui.rtl { ui.w - side - tw } else { side };
        let tr = Rect::new(tx, area.y, tw, h);
        let tf = ui.fit(title, 20.0, true, tw);
        ui.label(tr, &tf, 20.0, c, true, Al::Start);
        // Titel antippen = zurück an den Anfang der Liste
        if ui.tap(tr) {
            self.mx.to_top = true;
        }
        if let Some(id) = help {
            let r = ui.dp(16.0);
            let hx = if ui.rtl {
                ui.dp(28.0) + shift
            } else {
                ui.w - ui.dp(28.0) - shift
            };
            if ui.icon_btn(hx, cy, r, Ic::Info, ui.th.card.alpha(0.0), ui.th.sub) {
                self.go(Screen::Help(Some(id.to_string())));
            }
        }
        ui.kb.row_end.set(-1.0);
        Rect::new(area.x, area.y + h, area.w, (area.h - h).max(0.0))
    }

    /// Hilfe-Kennung für den aktuell gezeigten Bildschirm (für den „?"-Knopf).
    fn cur_help(&self) -> Option<&'static str> {
        let scr = self.stack.last().map(|s| &s.0)?;
        Some(match scr {
            Screen::Home => "home",
            Screen::Apps => "apps",
            Screen::Settings => "settings",
            Screen::Agent | Screen::AgentSet => "agent",
            Screen::Radio => "radio",
            Screen::Cells => "cells",
            Screen::Route => "route",
            Screen::Net | Screen::WifiPw(..) => "net",
            Screen::Bluetooth => "bluetooth",
            Screen::Vpn => "vpn",
            Screen::Verbund | Screen::Weitergeben(_) => "verbund",
            Screen::Zellen | Screen::Zelle(_) | Screen::Gpu | Screen::Empfehlung(_) => "zellen",
            Screen::Phone => "phone",
            Screen::Messages | Screen::MessageNew => "messages",
            Screen::Contacts | Screen::ContactEdit(_) => "contacts",
            Screen::Camera => "camera",
            Screen::Gallery | Screen::Photo(_) => "gallery",
            Screen::Files | Screen::Viewer(_) | Screen::Rename(_) => "files",
            Screen::Notes | Screen::NoteEdit(_) => "notes",
            Screen::Clock => "clock",
            Screen::Calc => "calc",
            Screen::Web(_) => "browser",
            Screen::Store | Screen::Pkg(..) => "store",
            Screen::Terminal => "terminal",
            Screen::UbuntuApps => "ubuntuapps",
            Screen::Monitor => "monitor",
            Screen::Privacy => "privacy",
            Screen::Security => "security",
            Screen::Sensors => "sensors",
            Screen::Gnss => "gnss",
            Screen::Telegram => "telegram",
            Screen::Spotify => "spotify",
            Screen::Account => "account",
            Screen::Update | Screen::Updates => "update",
            Screen::Display => "display",
            Screen::Sound => "sound",
            Screen::Time => "time",
            Screen::Lang => "lang",
            Screen::System => "system",
            Screen::Hardware => "hardware",
            Screen::Storage => "storage",
            Screen::Services => "services",
            Screen::More(m) => Self::more_help(m)?,
            Screen::Office(_) => "office",
            _ => return None,
        })
    }

    /// Hilfe-Kennung für einen Werkzeuge-/Pflege-Bildschirm.
    fn more_help(m: &app_more::Mx) -> Option<&'static str> {
        use app_more::Mx::*;
        Some(match m {
            Compass => "compass",
            Qibla => "qibla",
            Level => "level",
            Prayer => "prayer",
            Password => "password",
            Loan => "loan",
            Currency => "currency",
            Sos => "sos",
            Focus => "focus",
            Countdown => "countdown",
            Todo | Tasks => "todo",
            Reminders => "reminders",
            Emergency => "emergency",
            Recents => "recents",
            Clipboard => "clipboard",
            BattHist => "batthist",
            Usage => "usage",
            Care => "care",
            StorageX => "storagex",
            Modes => "modes",
            Blocked => "blocked",
            // Dice, Percent, DateCalc, Bases, Counter, BigText, Light, Tools
            _ => "tools",
        })
    }

    /// Hilfe-Zentrum: Übersicht aller Themen oder ein einzelner Artikel.
    fn s_help(&mut self, ui: &mut Ui, area: Rect, topic: Option<String>, sc: &mut f32) {
        if let Some(id) = topic {
            let t = crate::help::get(&id);
            let title = t.map(|t| self.t(t.title)).unwrap_or_else(|| self.t("Help"));
            let area = self.header(ui, area, title);
            ui.scroll_begin(area, sc);
            let mut y = area.y - *sc + ui.dp(4.0);
            let x = ui.dp(22.0);
            let w = ui.w - ui.dp(44.0);
            if let Some(body) = crate::help::body(&id, self.lang()) {
                let c = ui.th.text;
                let h = ui.para(x, y, w, body, 15.0, c);
                y += h + ui.dp(20.0);
                // Verweis zum Assistenten
                let hint = ui.th.sub;
                let h2 = ui.para(
                    x,
                    y,
                    w,
                    self.t("Ask the assistant for anything you can't find here."),
                    12.5,
                    hint,
                );
                y += h2 + ui.dp(12.0);
            } else {
                self.caption(ui, &mut y, self.t("No help for this topic."));
            }
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        // Übersicht
        let area = self.header(ui, area, self.t("Help"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let fr = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0));
        if ui.visible(fr) {
            ui.field(fr, "help_q", &self.f("help_q"), self.t("Search help"));
        }
        y += ui.dp(54.0);
        let q = self.f("help_q").trim().to_lowercase();
        let mut shown = 0usize;
        let mut nav: Option<String> = None;
        for (sys_group, head) in [(false, "Apps"), (true, "System")] {
            let part: Vec<&crate::help::Topic> = crate::help::TOPICS
                .iter()
                .filter(|t| t.system == sys_group)
                .filter(|t| {
                    q.is_empty()
                        || self.t(t.title).to_lowercase().contains(&q)
                        || t.title.to_lowercase().contains(&q)
                        || t.body[crate::help::li_pub(self.lang())]
                            .to_lowercase()
                            .contains(&q)
                })
                .collect();
            if part.is_empty() {
                continue;
            }
            if q.is_empty() {
                ui.section(&mut y, self.t(head));
            }
            for t in part {
                shown += 1;
                let sub = crate::help::first_line(t.body[crate::help::li_pub(self.lang())]);
                if ui.row(
                    &mut y,
                    Some(Ic::Info),
                    self.t(t.title),
                    Some(sub),
                    None,
                    true,
                ) {
                    nav = Some(t.id.to_string());
                }
            }
        }
        if shown == 0 {
            self.caption(ui, &mut y, self.t("No results"));
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
        if let Some(id) = nav {
            self.fields.remove("help_q");
            self.focus = None;
            self.focus_reset = true;
            self.go(Screen::Help(Some(id)));
            ui.dirty = true;
        }
    }

    fn caption(&mut self, ui: &mut Ui, y: &mut f32, s: &str) {
        let r = Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(26.0));
        let c = ui.th.sub;
        ui.label(r, s, 13.0, c, false, Al::Start);
        *y += ui.dp(28.0);
    }

    /// Alle Apps des Startbildschirms (Symbol, Name, Ziel; None = Ein/Aus-Dialog).
    pub(super) fn all_apps(&self) -> Vec<(Ic, &'static str, Option<Screen>)> {
        vec![
            (Ic::Spark, "KM helper", Some(Screen::Helper)),
            (Ic::Gear, "Settings", Some(Screen::Settings)),
            (Ic::Svg("store"), "Ultar Store", Some(Screen::StoreCloud)),
            (Ic::Term, "Terminal", Some(Screen::Terminal)),
            (
                Ic::Globe,
                "Browser",
                Some(Screen::Web(sys::memo("browser_home", 3000, || {
                    std::fs::read_to_string(format!("{}/browser-home", sys::data_dir()))
                        .map(|s| s.trim().to_string())
                        .ok()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "about:blank".into())
                }))),
            ),
            (
                Ic::Svg("wa"),
                "WhatsApp",
                Some(Screen::WebApp("whatsapp".into())),
            ),
            (
                Ic::Svg("yt"),
                "YouTube",
                Some(Screen::WebApp("youtube".into())),
            ),
            (Ic::Svg("spot"), "Spotify", Some(Screen::Spotify)),
            (Ic::Svg("tg"), "Telegram", Some(Screen::Telegram)),
            (Ic::Phone, "Phone", Some(Screen::Phone)),
            (Ic::Chat, "Messages", Some(Screen::Messages)),
            (Ic::Camera, "Camera", Some(Screen::Camera)),
            (Ic::Image, "Gallery", Some(Screen::Gallery)),
            (Ic::Note, "Notes", Some(Screen::Notes)),
            (
                Ic::Svg("pdf"),
                "PDF reader",
                Some(Screen::Office(app_office::Ox::List(0))),
            ),
            (
                Ic::Doc,
                "Documents",
                Some(Screen::Office(app_office::Ox::List(1))),
            ),
            (
                Ic::Svg("xls"),
                "Sheets",
                Some(Screen::Office(app_office::Ox::List(2))),
            ),
            (
                Ic::Svg("code"),
                "Code editor",
                Some(Screen::Office(app_office::Ox::List(3))),
            ),
            (Ic::Person, "Contacts", Some(Screen::Contacts)),
            (Ic::Calc, "Calculator", Some(Screen::Calc)),
            (
                Ic::Svg("cal"),
                "Calendar",
                Some(Screen::Sys(app_sys::Sx::Calendar)),
            ),
            (
                Ic::Globe,
                "World clock",
                Some(Screen::Sys(app_sys::Sx::World)),
            ),
            (
                Ic::Sliders,
                "Unit converter",
                Some(Screen::Sys(app_sys::Sx::Units)),
            ),
            (Ic::Search, "Search", Some(Screen::Sys(app_sys::Sx::Search))),
            (
                Ic::Svg("tools"),
                "Tools",
                Some(Screen::More(app_more::Mx::Tools)),
            ),
            (
                Ic::Heart,
                "Device care",
                Some(Screen::More(app_more::Mx::Care)),
            ),
            (Ic::Clock, "Clock", Some(Screen::Clock)),
            (Ic::Folder, "Files", Some(Screen::Files)),
            (Ic::Apps, "Ubuntu apps", Some(Screen::UbuntuApps)),
            (Ic::Shield, "Privacy", Some(Screen::Privacy)),
            (Ic::Svg("pulse"), "Monitor", Some(Screen::Monitor)),
            (Ic::List, "Services", Some(Screen::Services)),
            (Ic::Info, "Help", Some(Screen::Help(None))),
            (Ic::Power, "Power", None),
        ]
    }

    /// Klassischer Startbildschirm (Uhr + volles App-Raster) – für kleine/quer liegende Bildschirme.
    pub(super) fn s_home_classic(&mut self, ui: &mut Ui, area: Rect) {
        let dt = sys::from_unix(sys::unix_now(), self.set.tz_min);
        // Hintergrundbild (aus der Galerie gesetzt): füllt den Schirm, darüber ein dunkler Schleier für die Lesbarkeit
        if self.wall.is_none() {
            let p = format!("{}/wallpaper.jpg", sys::data_dir());
            self.wall = Some(tools::decode(&p, (ui.w.max(ui.h) as u32).max(720)).ok());
        }
        let has_wall = matches!(self.wall, Some(Some(_)));
        if let Some(Some(p)) = &self.wall {
            let k = (ui.w / p.w as f32).max(ui.h / p.h as f32);
            let (w, h) = (p.w as f32 * k, p.h as f32 * k);
            ui.cv.blit_rgba(
                &p.rgba,
                p.w,
                p.h,
                Rect::new((ui.w - w) / 2.0, (ui.h - h) / 2.0, w, h),
            );
            ui.cv.veil(
                Rect::new(0.0, 0.0, ui.w, ui.h),
                rgb(0).alpha(0.42),
                rgb(0).alpha(0.78),
            );
        }
        let (c, cs) = if has_wall {
            (rgb(0xFFFFFF), rgb(0xFFFFFF).alpha(0.78))
        } else {
            (ui.th.text, ui.th.sub)
        };
        let wide = ui.w > ui.h;
        // Wenig Höhe (Querformat, kurze Handys): Uhr kompakter, damit alle Apps sichtbar bleiben
        let compact = wide || area.h < ui.dp(600.0);
        // Marke KM Code Craft oben (im Hochformat, ohne Hintergrundbild)
        if !compact && !has_wall {
            let lg = brand::logo();
            let lw = (ui.w * 0.36).min(ui.dp(150.0));
            let lh = lw * lg.h as f32 / lg.w as f32;
            ui.cv
                .image(&lg, (ui.w - lw) / 2.0, area.y + ui.dp(12.0), lw, lh);
        }
        // Ein/Aus oben in der Ecke
        let px = if ui.rtl {
            ui.dp(30.0)
        } else {
            ui.w - ui.dp(30.0)
        };
        if ui.icon_btn(
            px,
            area.y + ui.dp(26.0),
            ui.dp(17.0),
            Ic::Power,
            c.alpha(0.08),
            cs,
        ) {
            self.power_dialog = true;
            ui.dirty = true;
        }
        // Gelernter Schnellzugriff und Mobilfunk-Leiste gemeinsam nur, wenn darunter Raster und Dock noch passen
        let has_freq = !self.frequent().is_empty();
        let both = !compact && has_freq && area.h >= ui.dp(688.0);
        let show_radio = !compact && (!has_freq || both);
        let (clock_y, clock_h, clock_sz, date_y, chips_y, grid_y) = if compact {
            (2.0, 58.0, 46.0, 58.0, 0.0, 92.0)
        } else {
            (
                78.0,
                90.0,
                70.0,
                166.0,
                204.0,
                if both { 296.0 } else { 256.0 },
            )
        };
        ui.label(
            Rect::new(0.0, area.y + ui.dp(clock_y), ui.w, ui.dp(clock_h)),
            &self.clock_str(dt.h, dt.mi),
            clock_sz,
            c,
            true,
            Al::Center,
        );
        let date = format!("{}, {:02}.{:02}.{}", self.t(DAYS[dt.wd]), dt.d, dt.mo, dt.y);
        ui.label(
            Rect::new(
                0.0,
                area.y + ui.dp(date_y),
                ui.w,
                ui.dp(if compact { 24.0 } else { 28.0 }),
            ),
            &date,
            if compact { 14.0 } else { 15.5 },
            cs,
            false,
            Al::Center,
        );

        // Apps: (Symbol, Name, Farbton, Ziel)
        let browser_home = sys::memo("browser_home", 3000, || {
            std::fs::read_to_string(format!("{}/browser-home", sys::data_dir()))
                .map(|s| s.trim().to_string())
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "about:blank".into())
        });
        let dock: [(Ic, &'static str, u32, Screen); 4] = [
            (Ic::Phone, "Phone", 0x22C55E, Screen::Phone),
            (Ic::Chat, "Messages", 0x3B82F6, Screen::Messages),
            (Ic::Globe, "Browser", 0x14B8A6, Screen::Web(browser_home)),
            (Ic::Camera, "Camera", 0x64748B, Screen::Camera),
        ];
        let apps: [(Ic, &'static str, u32, Screen); 16] = [
            (
                Ic::Spark,
                "Agent",
                ACCENTS[self.set.accent % ACCENTS.len()],
                Screen::Agent,
            ),
            (Ic::Image, "Gallery", 0xF472B6, Screen::Gallery),
            (Ic::Clock, "Clock", 0x60A5FA, Screen::Clock),
            (Ic::Calc, "Calculator", 0xF59E0B, Screen::Calc),
            (Ic::Folder, "Files", 0xFBBF24, Screen::Files),
            (Ic::Note, "Notes", 0xFACC15, Screen::Notes),
            (Ic::Person, "Contacts", 0x34D399, Screen::Contacts),
            (Ic::Gear, "Settings", 0x94A3B8, Screen::Settings),
            (
                Ic::Chat,
                "WhatsApp",
                0x25D366,
                Screen::WebApp("whatsapp".into()),
            ),
            (Ic::Send, "Telegram", 0x38BDF8, Screen::Telegram),
            (
                Ic::Play,
                "YouTube",
                0xF43F5E,
                Screen::WebApp("youtube".into()),
            ),
            (Ic::Music, "Spotify", 0x1ED760, Screen::Spotify),
            (Ic::Box, "Ultar Store", 0xA78BFA, Screen::StoreCloud),
            (Ic::Term, "Terminal", 0x4ADE80, Screen::Terminal),
            (Ic::Chip, "Monitor", 0xFB7185, Screen::Monitor),
            (Ic::Shield, "Privacy", 0x2DD4BF, Screen::Privacy),
        ];
        // Gelernt: die meistgenutzten Apps als Schnellzugriff unter dem Datum (ab drei Starts)
        let freq = self.frequent();
        if !compact && !freq.is_empty() {
            let m = ui.dp(16.0);
            let w = (ui.w - 2.0 * m) / freq.len() as f32;
            for (i, name) in freq.iter().enumerate() {
                let Some((_, key, _, target)) = apps
                    .iter()
                    .chain(dock.iter())
                    .find(|a| a.1 == name.as_str())
                    .cloned()
                else {
                    continue;
                };
                let col = if ui.rtl { freq.len() - 1 - i } else { i };
                let r = Rect::new(m + col as f32 * w, area.y + ui.dp(chips_y), w, ui.dp(40.0))
                    .inset(ui.dp(3.0));
                if ui.button(r, self.t(key), Btn::Plain) {
                    self.count_launch(key);
                    self.go(target);
                    ui.dirty = true;
                }
            }
        }
        // Dock: die vier wichtigsten Apps auf einer Glasleiste unten
        let m = ui.dp(14.0);
        let dock_h = ui.dp(if compact { 76.0 } else { 84.0 });
        let dw = (ui.w - 2.0 * m).min(ui.dp(440.0));
        let dr = Rect::new(
            (ui.w - dw) / 2.0,
            area.bottom() - dock_h - ui.dp(8.0),
            dw,
            dock_h,
        );
        ui.cv.rrect(dr, ui.dp(26.0), c.alpha(0.10));
        ui.cv.rrect(
            dr.inset(1.0),
            ui.dp(25.0),
            if has_wall {
                rgb(0x101216).alpha(0.55)
            } else {
                ui.th.card.alpha(0.92)
            },
        );
        let ds = (dock_h - ui.dp(24.0)).min(ui.dp(58.0));
        let mut go_to: Option<(&'static str, Screen)> = None;
        for (i, (ic, name, hue, target)) in dock.iter().enumerate() {
            let col = if ui.rtl { 3 - i } else { i };
            let cx = dr.x + dr.w * (col as f32 + 0.5) / 4.0;
            let ir = Rect::new(cx - ds / 2.0, dr.cy() - ds / 2.0, ds, ds);
            let hit = Rect::new(cx - dr.w / 8.0, dr.y, dr.w / 4.0, dr.h);
            let hue = rgb(*hue);
            let held = ui.held(hit);
            // Ultar Noir: neutrale Glasfläche, nur das Telefon grün
            let phone = *name == "Phone";
            let base = if phone {
                rgb(0x173A24)
            } else if has_wall {
                rgb(0x1C1F26)
            } else {
                ui.th.press
            };
            let base = if held { base.mix(rgb(0), 0.3) } else { base };
            let _ = hue;
            ui.cv.rrect(ir, ds * 0.32, base);
            ui.cv.rrect(
                Rect::new(ir.x, ir.y, ir.w, ir.h * 0.45),
                ds * 0.32,
                rgb(0xFFFFFF).alpha(0.04),
            );
            ui.icon(
                *ic,
                ir.cx(),
                ir.cy(),
                ds * 0.46,
                if phone {
                    ui.th.ok
                } else if has_wall {
                    rgb(0xFFFFFF)
                } else {
                    ui.th.text
                },
            );
            if ui.tap(hit) {
                go_to = Some((name, target.clone()));
            }
        }
        // Raster: Spalten nach Breite (eine Kachel braucht rund 88 dp), Zeilenhöhe nach verfügbarer Höhe
        let m = ui.dp(16.0);
        let cols = (((ui.w - 2.0 * m) / ui.dp(88.0)) as usize)
            .clamp(4, 8)
            .min(apps.len());
        let tw = (ui.w - 2.0 * m) / cols as f32;
        let y0 = area.y + ui.dp(grid_y);
        // Verbindungsleiste (Mobilfunk) zwischen Datum und App-Raster
        if show_radio {
            let cr = if both {
                Rect::new(m, y0 - ui.dp(46.0), ui.w - 2.0 * m, ui.dp(38.0))
            } else {
                Rect::new(m, y0 - ui.dp(50.0), ui.w - 2.0 * m, ui.dp(42.0))
            };
            self.home_radio_card(ui, cr);
        }
        let rows = apps.len().div_ceil(cols);
        let th = ((dr.y - y0 - ui.dp(8.0)) / rows as f32).clamp(ui.dp(70.0), ui.dp(104.0));
        // Symbolgröße folgt der Zeilenhöhe, damit die Beschriftung nie abgeschnitten wird
        let icon_s = (th - ui.dp(36.0)).clamp(ui.dp(38.0), ui.dp(58.0));
        ui.set_clip(Some(Rect::new(
            0.0,
            y0 - ui.dp(4.0),
            ui.w,
            (dr.y - y0).max(0.0),
        )));
        for (i, (ic, name, hue, target)) in apps.iter().enumerate() {
            let col = i % cols;
            let col = if ui.rtl { cols - 1 - col } else { col };
            let cell = Rect::new(m + col as f32 * tw, y0 + (i / cols) as f32 * th, tw, th);
            let s = icon_s;
            let ir = Rect::new(cell.cx() - s / 2.0, cell.y + ui.dp(4.0), s, s);
            // „Glas"-Kachel im Farbton der App: dunkle Fläche, feine Lichtkante, leuchtendes Symbol
            let hue = rgb(*hue);
            let held = ui.held(cell);
            let rad = s * 0.3;
            let base = if has_wall { rgb(0x14161B) } else { ui.th.card };
            let tile = base;
            let tile = if held { tile.mix(rgb(0), 0.25) } else { tile };
            if !has_wall {
                ui.cv.soft_shadow(ir, rad, rgb(0));
            }
            ui.cv.rrect(
                ir,
                rad,
                if has_wall {
                    rgb(0xFFFFFF).alpha(0.12)
                } else {
                    ui.th.line
                },
            );
            ui.cv.rrect(ir.inset(1.0), rad - 1.0, tile);
            ui.cv.rrect(
                Rect::new(ir.x + 1.0, ir.y + 1.0, ir.w - 2.0, ir.h * 0.45),
                rad - 1.0,
                rgb(0xFFFFFF).alpha(0.05),
            );
            let gold = matches!(ic, Ic::Spark | Ic::Box);
            ui.icon(
                *ic,
                ir.cx(),
                ir.cy(),
                s * 0.46,
                if gold {
                    ui.th.accent
                } else if has_wall {
                    rgb(0xFFFFFF)
                } else {
                    ui.th.text.mix(hue, 0.0)
                },
            );
            ui.label(
                Rect::new(
                    cell.x + ui.dp(2.0),
                    ir.bottom() + ui.dp(4.0),
                    cell.w - ui.dp(4.0),
                    ui.dp(22.0),
                ),
                self.t(name),
                12.0,
                if has_wall { c } else { cs.mix(c, 0.45) },
                false,
                Al::Center,
            );
            if ui.tap(cell) {
                go_to = Some((name, target.clone()));
            }
        }
        ui.set_clip(None);
        if let Some((name, target)) = go_to {
            self.count_launch(name);
            self.go(target);
            ui.dirty = true;
        }
    }

    fn s_settings(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, "");
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let net = sys::net_status()
            .into_iter()
            .find(|n| n.state == "verbunden")
            .map(|n| n.ip)
            .unwrap_or_default();
        let nmiss = hw::missing(&self.hw).len();
        let acct = sys::memo("cloud_num", 3000, ultar_cloud::my_number).unwrap_or_else(|| {
            if sys::memo("cloud_tok", 3000, || ultar_cloud::token().is_some()) {
                self.t("Signed in").into()
            } else {
                String::new()
            }
        });
        let rs = self.radio();
        let mobile = if rs.in_service() {
            format!(
                "{} · {}",
                if rs.operator.is_empty() {
                    rs.plmn.clone()
                } else {
                    rs.operator.clone()
                },
                app_radio::rat_short(&rs.rat)
            )
        } else {
            String::new()
        };
        let mut items: Vec<(Ic, &'static str, String, Screen)> = vec![
            (Ic::Wifi, "Network", net, Screen::Net),
            (Ic::Tower, "Mobile network", mobile, Screen::Radio),
            (
                Ic::Shield,
                "Privacy",
                self.t(if self.set.adblock {
                    "Ads blocked"
                } else {
                    "Off"
                })
                .into(),
                Screen::Privacy,
            ),
            (
                Ic::Shield,
                "Security",
                if sys::memo("udata_enc", 30_000, ultar_secure::udata::is_encrypted) {
                    self.t("Encrypted").into()
                } else {
                    String::new()
                },
                Screen::Security,
            ),
            (Ic::Person, "Account", acct, Screen::Account),
            (
                Ic::Lock,
                "VPN",
                if crate::sys::vpn_status().up {
                    self.t("Connected").into()
                } else {
                    String::new()
                },
                Screen::Vpn,
            ),
            (
                Ic::Chip,
                "Device cluster",
                app_cloud::verbund_kurz(self),
                Screen::Verbund,
            ),
            (
                Ic::Box,
                "Cells & containers",
                app_zellen::kurz(self),
                Screen::Zellen,
            ),
            (Ic::Box, "Ultar Store", String::new(), Screen::StoreCloud),
            (Ic::Info, "System update", String::new(), Screen::Updates),
            (
                Ic::Sun,
                "Display",
                format!("{} %", (self.set.brightness * 100.0).round()),
                Screen::Display,
            ),
            (
                Ic::Speaker,
                "Sound",
                if self.set.sound {
                    format!("{} %", (self.set.volume * 100.0).round())
                } else {
                    self.t("Off").to_string()
                },
                Screen::Sound,
            ),
            (
                Ic::Lang,
                "Language",
                match self.set.lang.as_str() {
                    "ar" => "العربية",
                    "en" => "English",
                    _ => "Deutsch",
                }
                .into(),
                Screen::Lang,
            ),
            (
                Ic::Clock,
                "Date & time",
                tz_label(self.set.tz_min),
                Screen::Time,
            ),
            (Ic::Disk, "Storage", String::new(), Screen::Storage),
            (Ic::List, "Services", String::new(), Screen::Services),
            (
                Ic::Chip,
                "Hardware & drivers",
                if nmiss > 0 {
                    format!("{nmiss} {}", self.t("without driver"))
                } else {
                    String::new()
                },
                Screen::Hardware,
            ),
            (
                Ic::Bt,
                "Bluetooth",
                if app_bt::enabled() {
                    self.t("On").into()
                } else {
                    String::new()
                },
                Screen::Bluetooth,
            ),
            (Ic::Sun, "Sensors", String::new(), Screen::Sensors),
            (Ic::Globe, "Location (GPS)", String::new(), Screen::Gnss),
            (Ic::Spark, "Local agent", String::new(), Screen::AgentSet),
            (Ic::Apps, "Ubuntu apps", String::new(), Screen::UbuntuApps),
            (Ic::Info, "System", "Ultar OS".into(), Screen::System),
            (Ic::Info, "Help", String::new(), Screen::Help(None)),
        ];
        // Geräte-Akkus (Kopfhörer, kabellose Maus/Tastatur …): als eigene Zeile, wenn vorhanden
        let akkus = sys::geraete_akkus();
        if !akkus.is_empty() {
            let txt = akkus
                .iter()
                .map(|(n, p)| format!("{n} {p} %"))
                .collect::<Vec<_>>()
                .join(" · ");
            items.insert(2, (Ic::Bt, "Connected devices", txt, Screen::Bluetooth));
        }
        // Mobilfunk nur mit Modem/SIM; PC (Laptop/Server): keine Handy-Sensoren, Ortung nur mit Empfänger
        let mobil = self.mobilfunk_vorhanden();
        let pc = matches!(
            ultar_hal_system::current_edition(),
            ultar_profile::Edition::Desktop | ultar_profile::Edition::Server
        ) || self.desk.an;
        let gnss = ["/dev/gnss0", "/dev/ttyACM0", "/run/ultar/gnss.nmea"]
            .iter()
            .any(|p| std::path::Path::new(p).exists());
        items.retain(|(_, k, _, _)| match *k {
            "Mobile network" => mobil,
            "Sensors" => !pc,
            "Location (GPS)" => !pc || gnss,
            _ => true,
        });
        let extra = self.sys_settings_items();
        let at = items.len() - 1;
        items.splice(at..at, extra);
        items.insert(
            items.len() - 1,
            (
                Ic::Sliders,
                "Modes & automation",
                String::new(),
                Screen::More(app_more::Mx::Modes),
            ),
        );
        items.insert(
            items.len() - 1,
            (
                Ic::Heart,
                "Device care",
                String::new(),
                Screen::More(app_more::Mx::Care),
            ),
        );
        items.insert(
            items.len() - 1,
            (
                Ic::Clock,
                "Screen time",
                String::new(),
                Screen::More(app_more::Mx::Usage),
            ),
        );
        // Wie in der Web-Demo: großer Titel, Suchfeld, Ultar-Konto-Karte, vier Gruppen mit farbigen Symbolen
        let c = ui.th.text;
        ui.label(
            Rect::new(ui.dp(22.0), y + ui.dp(6.0), ui.w - ui.dp(44.0), ui.dp(40.0)),
            self.t("Settings"),
            32.0,
            c,
            true,
            Al::Start,
        );
        y += ui.dp(58.0);
        let fr = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(48.0));
        if ui.visible(fr) {
            ui.field(fr, "set_q", &self.f("set_q"), self.t("Search settings"));
        }
        y += ui.dp(62.0);
        let q = self.f("set_q").trim().to_lowercase();
        if q.is_empty() {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(70.0));
            let held = ui.held(r);
            let ac = ui.th.accent;
            ui.cv.rrect(r, ui.dp(22.0), ac.mix(ui.th.card, 0.65));
            ui.cv.rrect(
                r.inset(1.0),
                ui.dp(21.0),
                if held { ui.th.press } else { ui.th.card },
            );
            ui.cv.rrect(r.inset(1.0), ui.dp(21.0), ac.alpha(0.08));
            let ax = if ui.rtl {
                r.right() - ui.dp(14.0) - ui.dp(24.0)
            } else {
                r.x + ui.dp(14.0) + ui.dp(24.0)
            };
            ui.cv
                .circle(ax, r.cy(), ui.dp(24.0), ui.th.accent2.alpha(0.4));
            ui.cv.circle(ax, r.cy(), ui.dp(23.0), rgb(0x121108));
            let mk = brand::mark();
            let mw = ui.dp(34.0);
            let mh = mw * mk.h as f32 / mk.w.max(1) as f32;
            ui.cv.image(&mk, ax - mw / 2.0, r.cy() - mh / 2.0, mw, mh);
            let tx = if ui.rtl {
                r.x + ui.dp(40.0)
            } else {
                r.x + ui.dp(76.0)
            };
            let tw = r.w - ui.dp(116.0);
            ui.label(
                Rect::new(tx, r.y + ui.dp(14.0), tw, ui.dp(22.0)),
                self.t("Ultar account"),
                16.0,
                c,
                true,
                Al::Start,
            );
            let num = ultar_cloud::my_number().unwrap_or_else(|| "UOS-—".into());
            let cs = ui.th.sub;
            ui.label(
                Rect::new(tx, r.y + ui.dp(38.0), tw, ui.dp(18.0)),
                &num,
                12.5,
                cs,
                false,
                Al::Start,
            );
            let chx = if ui.rtl {
                r.x + ui.dp(22.0)
            } else {
                r.right() - ui.dp(22.0)
            };
            ui.icon(Ic::Chevron, chx, r.cy(), ui.dp(16.0), cs.alpha(0.6));
            if ui.tap(r) {
                self.go(Screen::Account);
                ui.dirty = true;
            }
            y += r.h + ui.dp(16.0);
        }
        // Gruppen wie in der Demo
        let group = |name: &str| -> usize {
            match name {
                "Network" | "Mobile network" | "VPN" | "Device cluster" | "Bluetooth"
                | "Network tools" | "Location (GPS)" => 0,
                "Display"
                | "Sound"
                | "Night light & brightness"
                | "Sound profiles"
                | "Battery & performance"
                | "Storage"
                | "Sensors"
                | "Hardware & drivers"
                | "Device care" => 1,
                "Privacy" | "Security" | "Screen lock" | "Local agent" | "Ubuntu apps"
                | "Ultar Store" | "Account" => 2,
                _ => 3,
            }
        };
        let color = |name: &str| -> u32 {
            match name {
                "Network" | "Bluetooth" | "Network tools" | "Backup & reset" => 0x1E3A5F,
                "Mobile network" | "Battery & performance" | "Device care" => 0x173A24,
                "VPN" | "Device cluster" | "Local agent" | "System update" | "Ultar Store" => {
                    0x4A421C
                }
                "Privacy" | "Security" | "Screen lock" => 0x4A1D1D,
                "Ubuntu apps" => 0x5A2A1C,
                _ => 0x2A2D35,
            }
        };
        let heads = ["Connections", "Device", "Protection & apps", "System"];
        let mut shown = 0;
        for (g, head) in heads.iter().enumerate() {
            let part: Vec<_> = items
                .iter()
                .filter(|it| {
                    it.1 != "Account"
                        && group(it.1) == g
                        && (q.is_empty()
                            || self.t(it.1).to_lowercase().contains(&q)
                            || it.1.to_lowercase().contains(&q))
                })
                .cloned()
                .collect();
            if part.is_empty() {
                continue;
            }
            ui.section(&mut y, self.t(head));
            ui.kb.row_end.set(-1.0);
            for (ic, name, val, target) in part {
                shown += 1;
                ui.kb.row_bg.set(Some(rgb(color(name))));
                if name == "VPN" && !val.is_empty() {
                    ui.kb.row_gold.set(true);
                }
                if ui.row(&mut y, Some(ic), self.t(name), None, Some(&val), true) {
                    self.fields.remove("set_q");
                    self.focus = None;
                    self.focus_reset = true;
                    self.go(target);
                    ui.dirty = true;
                }
            }
            y += ui.dp(14.0);
        }
        if shown == 0 {
            let cs = ui.th.sub;
            ui.label(
                Rect::new(0.0, y + ui.dp(30.0), ui.w, ui.dp(22.0)),
                self.t("Nothing found"),
                14.0,
                cs,
                false,
                Al::Center,
            );
            y += ui.dp(80.0);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn field_row(
        &mut self,
        ui: &mut Ui,
        y: &mut f32,
        id: &'static str,
        label: &'static str,
        hint: &str,
    ) {
        self.caption(ui, y, self.t(label));
        let r = Rect::new(ui.dp(16.0), *y, ui.w - ui.dp(32.0), ui.dp(52.0));
        if ui.visible(r) {
            ui.field(r, id, &self.f(id), hint);
        }
        *y += ui.dp(60.0);
    }

    fn s_net(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Network"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        ui.section(&mut y, self.t("Wired network"));
        let ifs = sys::net_status();
        if ifs.is_empty() {
            self.caption(ui, &mut y, self.t("No network device found"));
        }
        for n in ifs.iter().filter(|n| !n.wifi) {
            let st = match n.state.as_str() {
                "verbunden" => self.t("Connected"),
                "kein-kabel" => self.t("No cable"),
                "dhcp" => self.t("Connecting…"),
                _ => self.t("Error"),
            };
            ui.row(
                &mut y,
                Some(Ic::Globe),
                &n.name,
                Some(if n.ip.is_empty() { &n.mac } else { &n.ip }),
                Some(st),
                false,
            );
            if !n.ip.is_empty() {
                ui.row(&mut y, None, self.t("Gateway"), None, Some(&n.gw), false);
                ui.row(
                    &mut y,
                    None,
                    self.t("DNS server"),
                    None,
                    Some(&n.dns),
                    false,
                );
                ui.row(&mut y, None, "MAC", None, Some(&n.mac), false);
            }
        }
        let mut dhcp = self.net.dhcp;
        if ui.toggle_row(
            &mut y,
            self.t("Automatic (DHCP)"),
            Some(self.t("Address is assigned by the router")),
            &mut dhcp,
        ) {
            self.net.dhcp = dhcp;
            if dhcp {
                self.apply_net();
            }
        }
        if !self.net.dhcp {
            self.field_row(ui, &mut y, "ip", "IP address", "192.168.1.50");
            self.field_row(ui, &mut y, "prefix", "Prefix length", "24");
            self.field_row(ui, &mut y, "gw", "Gateway", "192.168.1.1");
            self.field_row(ui, &mut y, "dns", "DNS server", "1.1.1.1");
            let r = Rect::new(ui.dp(16.0), y + ui.dp(4.0), ui.w - ui.dp(32.0), ui.dp(50.0));
            if ui.button(r, self.t("Apply"), Btn::Primary) {
                let ok = self.f("ip").trim().parse::<std::net::Ipv4Addr>().is_ok()
                    && self.f("prefix").trim().parse::<u8>().is_ok_and(|p| p <= 32)
                    && (self.f("gw").trim().is_empty()
                        || self.f("gw").trim().parse::<std::net::Ipv4Addr>().is_ok())
                    && self
                        .f("dns")
                        .split([',', ' '])
                        .filter(|s| !s.is_empty())
                        .all(|d| d.parse::<std::net::Ipv4Addr>().is_ok());
                if ok {
                    self.net.address = self.f("ip").trim().into();
                    self.net.prefix = self.f("prefix").trim().parse().unwrap_or(24);
                    self.net.gateway = self.f("gw").trim().into();
                    self.net.dns = self.f("dns").trim().into();
                    self.apply_net();
                } else {
                    self.toast(self.t("Invalid input").into());
                }
                ui.dirty = true;
            }
            y += ui.dp(66.0);
        }
        ui.section(&mut y, self.t("Wi-Fi"));
        let c = ui.th.sub;
        if sys::wifi_iface().is_none() {
            y += ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                self.t("No Wi-Fi device found"),
                14.0,
                c,
            ) + ui.dp(20.0);
        } else if !sys::wifi_tools() {
            y += ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                self.t("Wi-Fi programs are missing. Install the packages wpasupplicant and iw."),
                14.0,
                c,
            ) + ui.dp(20.0);
        } else {
            let cur = sys::wifi_ssid();
            let wst = sys::wifi_state();
            if let Some(n) = ifs.iter().find(|n| n.wifi) {
                let st = match (wst.as_deref(), n.state.as_str()) {
                    (_, "verbunden") => self.t("Connected"),
                    (_, "wlan-start-fehler") => self.t("Wi-Fi could not be started"),
                    (Some("INTERFACE_DISABLED"), _) => self.t("Wi-Fi is off"),
                    (Some("COMPLETED"), _) => self.t("Getting address…"),
                    (
                        Some(
                            "AUTHENTICATING" | "ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE"
                            | "GROUP_HANDSHAKE",
                        ),
                        _,
                    )
                    | (_, "dhcp") => self.t("Connecting…"),
                    (_, "wlan-start") => self.t("Starting…"),
                    (_, "wlan-bereit" | "kein-kabel" | "wlan-getrennt") | (Some(_), _) => {
                        self.t("Not connected")
                    }
                    _ => self.t("Error"),
                };
                let st = if self.wifi_connecting.is_some() {
                    self.t("Connecting…")
                } else {
                    st
                };
                let title = self
                    .wifi_connecting
                    .clone()
                    .or_else(|| cur.clone())
                    .unwrap_or_else(|| n.name.clone());
                ui.row(
                    &mut y,
                    Some(Ic::Globe),
                    &title,
                    Some(if n.ip.is_empty() { &n.mac } else { &n.ip }),
                    Some(st),
                    false,
                );
            }
            let bw = (ui.w - ui.dp(32.0)) / 2.0;
            let old = ui.enabled;
            ui.enabled = old && !self.wifi_scanning;
            let label = if self.wifi_scanning {
                self.t("Searching…")
            } else {
                self.t("Search networks")
            };
            if ui.button(
                Rect::new(
                    ui.dp(12.0),
                    y,
                    if cur.is_some() {
                        bw
                    } else {
                        ui.w - ui.dp(24.0)
                    },
                    ui.dp(48.0),
                ),
                label,
                Btn::Primary,
            ) {
                self.wifi_scanning = true;
                sys::wifi_scan(&self.bg);
                ui.dirty = true;
            }
            ui.enabled = old;
            if let Some(c) = &cur {
                if ui.button(
                    Rect::new(ui.dp(20.0) + bw, y, bw, ui.dp(48.0)),
                    self.t("Forget network"),
                    Btn::Plain,
                ) {
                    sys::wifi_forget(c);
                    self.wifi_saved.retain(|n| n != c);
                    ui.dirty = true;
                }
            }
            y += ui.dp(58.0);
            let mut open = None;
            let connected = wst.as_deref() == Some("COMPLETED");
            for (ssid, sig, sec) in &self.wifi {
                let q = match *sig {
                    s if s >= -55 => self.t("Excellent"),
                    s if s >= -67 => self.t("Good"),
                    s if s >= -78 => self.t("Fair"),
                    _ => self.t("Weak"),
                };
                let mut sub = format!(
                    "{q} ({sig} dBm) · {}",
                    if *sec {
                        self.t("secured")
                    } else {
                        self.t("open")
                    }
                );
                if self.wifi_saved.contains(ssid) {
                    sub += &format!(" · {}", self.t("Saved network"));
                }
                let right = if connected && cur.as_deref() == Some(ssid.as_str()) {
                    Some(self.t("Connected"))
                } else {
                    None
                };
                if ui.row(&mut y, None, ssid, Some(&sub), right, true) {
                    open = Some((ssid.clone(), *sec));
                }
            }
            if let Some((n, sec)) = open {
                if self.wifi_saved.contains(&n) || !sec {
                    // gespeichert oder offen: direkt verbinden
                    self.wifi_connecting = Some(n.clone());
                    sys::wifi_connect(&self.bg, n, None);
                } else {
                    self.go(Screen::WifiPw(n, sec));
                }
                ui.dirty = true;
            }
            y += ui.dp(12.0);
        }
        let mobil = self.mobilfunk_vorhanden();
        ui.section(
            &mut y,
            self.t(if mobil {
                "Mobile network & Bluetooth"
            } else {
                "Bluetooth"
            }),
        );
        if mobil
            && ui.row(
                &mut y,
                Some(Ic::Globe),
                self.t("Mobile network"),
                None,
                None,
                true,
            )
        {
            self.go(Screen::Radio);
        }
        if ui.row(
            &mut y,
            Some(Ic::Wifi),
            self.t("Bluetooth"),
            None,
            None,
            true,
        ) {
            self.go(Screen::Bluetooth);
        }
        let _ = c;
        y += ui.dp(20.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn apply_net(&mut self) {
        self.focus = None;
        self.focus_reset = true;
        match sys::save("net.toml", &self.net) {
            Ok(()) => {
                sys::ctl("restart net");
                self.toast(self.t("Saved").into());
            }
            Err(e) => self.toast(format!("{}: {e}", self.t("Could not save"))),
        }
    }

    fn s_display(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Display"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        ui.section(&mut y, self.t("Screen"));
        // Helligkeit
        let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(92.0));
        ui.card(r);
        let (c, cs) = (ui.th.text, ui.th.sub);
        let lr = Rect::new(
            r.x + ui.dp(14.0),
            r.y + ui.dp(8.0),
            r.w - ui.dp(28.0),
            ui.dp(30.0),
        );
        ui.label(lr, self.t("Brightness"), 16.0, c, false, Al::Start);
        ui.label(
            lr,
            &format!("{} %", (self.set.brightness * 100.0).round()),
            14.0,
            cs,
            false,
            Al::End,
        );
        let mut v = (self.set.brightness - 0.1) / 0.9;
        let sr = Rect::new(
            r.x + ui.dp(10.0),
            r.y + ui.dp(44.0),
            r.w - ui.dp(20.0),
            ui.dp(40.0),
        );
        if ui.slider("bright", sr, &mut v) {
            self.set.brightness = 0.1 + 0.9 * v;
            self.apply_brightness();
        }
        if ui.p.released && ui.p.capture == Some("bright") {
            self.save();
        }
        y += r.h + ui.dp(8.0);

        self.caption(ui, &mut y, self.t("Screen off after"));
        const TO: [u32; 4] = [30, 60, 300, 0];
        let never = self.t("Never");
        let sel = TO
            .iter()
            .position(|t| *t == self.set.timeout_s)
            .unwrap_or(3);
        if let Some(i) = ui.chips(&mut y, &["30 s", "1 min", "5 min", never], sel) {
            self.set.timeout_s = TO[i];
            self.save();
        }
        self.caption(ui, &mut y, self.t("Rotation"));
        if let Some(i) = ui.chips(
            &mut y,
            &["0°", "90°", "180°", "270°"],
            (self.set.rotation / 90) as usize % 4,
        ) {
            self.set.rotation = i as u16 * 90;
            self.save();
        }
        self.caption(ui, &mut y, self.t("Size"));
        const SC: [f32; 4] = [0.85, 1.0, 1.15, 1.3];
        let sel = SC
            .iter()
            .position(|s| (*s - self.set.scale).abs() < 0.01)
            .unwrap_or(1);
        if let Some(i) = ui.chips(&mut y, &["85 %", "100 %", "115 %", "130 %"], sel) {
            self.set.scale = SC[i];
            self.save();
        }
        ui.section(&mut y, self.t("Appearance"));
        let mut dark = self.set.dark;
        if ui.toggle_row(&mut y, self.t("Dark mode"), None, &mut dark) {
            self.set.dark = dark;
            self.save();
        }
        let mut hb = self.set.help_btn;
        if ui.toggle_row(
            &mut y,
            self.t("Help button in the title bar"),
            Some(self.t("The (i) button opens the help for each page")),
            &mut hb,
        ) {
            self.set.help_btn = hb;
            self.save();
        }
        y += ui.dp(10.0);
        // Akzentfarbe
        let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(96.0));
        if ui.visible(r) {
            ui.card(r);
            ui.label(
                Rect::new(
                    r.x + ui.dp(14.0),
                    r.y + ui.dp(8.0),
                    r.w - ui.dp(28.0),
                    ui.dp(30.0),
                ),
                self.t("Accent color"),
                16.0,
                c,
                false,
                Al::Start,
            );
            let cw = (r.w - ui.dp(28.0)) / ACCENTS.len() as f32;
            for (i, a) in ACCENTS.iter().enumerate() {
                let cx = r.x + ui.dp(14.0) + cw * (i as f32 + 0.5);
                let cy = r.y + ui.dp(64.0);
                ui.cv.circle(cx, cy, ui.dp(17.0), rgb(*a));
                if i == self.set.accent {
                    ui.icon(Ic::Check, cx, cy, ui.dp(18.0), rgb(0xFFFFFF));
                }
                if ui.tap(Rect::new(cx - cw / 2.0, cy - ui.dp(24.0), cw, ui.dp(48.0)))
                    && i != self.set.accent
                {
                    self.set.accent = i;
                    self.save();
                    ui.dirty = true;
                }
            }
        }
        y += r.h + ui.dp(8.0);
        let res = format!("{} × {}", self.disp.0, self.disp.1);
        ui.row(
            &mut y,
            None,
            self.t("Resolution"),
            Some(&self.disp.2.clone()),
            Some(&res),
            false,
        );
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_lang(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Language"));
        let mut y = area.y;
        for (code, name) in [("de", "Deutsch"), ("en", "English"), ("ar", "العربية")] {
            let y0 = y;
            let tapped = ui.row(&mut y, None, name, None, None, false);
            if self.set.lang == code {
                let m = ui.dp(12.0) + ui.dp(26.0);
                let cx = if ui.rtl { m } else { ui.w - m };
                let a = ui.th.accent;
                ui.icon(Ic::Check, cx, y0 + ui.dp(28.0), ui.dp(20.0), a);
            } else if tapped {
                self.set.lang = code.into();
                self.save();
                ui.dirty = true;
            }
        }
    }

    fn s_time(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Date & time"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let now = sys::unix_now();
        let mut dt = sys::from_unix(now, self.set.tz_min);
        let (c, cs) = (ui.th.text, ui.th.sub);
        ui.label(
            Rect::new(0.0, y, ui.w, ui.dp(60.0)),
            &format!("{:02}:{:02}:{:02}", dt.h, dt.mi, dt.s),
            40.0,
            c,
            true,
            Al::Center,
        );
        y += ui.dp(60.0);
        ui.label(
            Rect::new(0.0, y, ui.w, ui.dp(28.0)),
            &format!("{}, {:02}.{:02}.{}", self.t(DAYS[dt.wd]), dt.d, dt.mo, dt.y),
            15.0,
            cs,
            false,
            Al::Center,
        );
        y += ui.dp(40.0);
        let d = ui.stepper(&mut y, self.t("Time zone"), &tz_label(self.set.tz_min));
        if d != 0 {
            self.set.tz_min = (self.set.tz_min + d * 30).clamp(-720, 840);
            self.save();
        }
        ui.section(&mut y, self.t("Set time"));
        let mut changed = false;
        let d = ui.stepper(&mut y, self.t("Hour"), &format!("{:02}", dt.h));
        if d != 0 {
            dt.h = (dt.h + d as i64).rem_euclid(24);
            changed = true;
        }
        let d = ui.stepper(&mut y, self.t("Minute"), &format!("{:02}", dt.mi));
        if d != 0 {
            dt.mi = (dt.mi + d as i64).rem_euclid(60);
            changed = true;
        }
        let d = ui.stepper(&mut y, self.t("Day"), &format!("{:02}", dt.d));
        if d != 0 {
            let n = sys::days_in_month(dt.y, dt.mo);
            dt.d = (dt.d - 1 + d as i64).rem_euclid(n) + 1;
            changed = true;
        }
        let d = ui.stepper(&mut y, self.t("Month"), &format!("{:02}", dt.mo));
        if d != 0 {
            dt.mo = (dt.mo - 1 + d as i64).rem_euclid(12) + 1;
            changed = true;
        }
        let d = ui.stepper(&mut y, self.t("Year"), &dt.y.to_string());
        if d != 0 {
            dt.y = (dt.y + d as i64).clamp(2024, 2099);
            changed = true;
        }
        if changed {
            dt.d = dt.d.min(sys::days_in_month(dt.y, dt.mo));
            if let Err(e) = sys::set_time(sys::to_unix(&dt, self.set.tz_min)) {
                self.toast(format!("{}: {e}", self.t("Error")));
            }
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_storage(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Storage"));
        let mut y = area.y;
        let (mt, ma) = sys::meminfo();
        let items = [
            ("Memory (RAM)", Some((mt, ma))),
            ("System partition", sys::disk("/")),
            ("Ubuntu area", sys::disk("/compat")),
            ("User data", sys::disk("/data")),
        ];
        for (name, v) in items {
            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(84.0));
            ui.card(r);
            let (c, cs) = (ui.th.text, ui.th.sub);
            let lr = Rect::new(
                r.x + ui.dp(14.0),
                r.y + ui.dp(10.0),
                r.w - ui.dp(28.0),
                ui.dp(28.0),
            );
            ui.label(lr, self.t(name), 16.0, c, false, Al::Start);
            match v {
                Some((total, free)) if total > 0 => {
                    let s = format!(
                        "{} {} {} {}",
                        sys::human(free),
                        self.t("free"),
                        self.t("of"),
                        sys::human(total)
                    );
                    ui.label(lr, &s, 13.0, cs, false, Al::End);
                    ui.progress(
                        Rect::new(
                            r.x + ui.dp(14.0),
                            r.y + ui.dp(54.0),
                            r.w - ui.dp(28.0),
                            ui.dp(10.0),
                        ),
                        1.0 - free as f32 / total as f32,
                    );
                }
                _ => ui.label(lr, self.t("not available"), 13.0, cs, false, Al::End),
            }
            y += r.h + ui.dp(8.0);
        }
    }

    fn s_services(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Services"));
        if self
            .svc_at
            .map_or(true, |t| t.elapsed() > Duration::from_millis(900))
        {
            self.svcs = sys::services();
            self.svc_at = Some(Instant::now());
        }
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        for s in self.svcs.clone() {
            let y0 = y;
            let running = s.state == "läuft";
            let state = match (self.set.lang.as_str(), s.state.as_str()) {
                ("en", "läuft") => "running",
                ("en", "gestoppt") => "stopped",
                ("en", "fertig") => "finished",
                ("ar", "läuft") => "يعمل",
                ("ar", "gestoppt") => "متوقف",
                ("ar", "fertig") => "انتهى",
                (_, o) => o,
            };
            let sub = if running {
                format!("{state} · PID {}", s.pid)
            } else {
                state.to_string()
            };
            ui.row(&mut y, None, &s.name, Some(&sub), None, false);
            if s.name == "ui" {
                continue; // die Oberfläche selbst nicht stoppen
            }
            let bw = ui.dp(104.0);
            let bx = if ui.rtl {
                ui.dp(24.0)
            } else {
                ui.w - ui.dp(24.0) - bw
            };
            let br = Rect::new(bx, y0 + ui.dp(12.0), bw, ui.dp(44.0));
            let (label, kind, cmd) = if running {
                (self.t("Stop"), Btn::Danger, "stop")
            } else {
                (self.t("Start"), Btn::Primary, "start")
            };
            if ui.button(br, label, kind) {
                sys::ctl(&format!("{cmd} {}", s.name));
                self.svc_at = None;
                ui.dirty = true;
            }
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_system(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("System"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        self.caption(ui, &mut y, self.t("Device name"));
        let bw = ui.dp(110.0);
        let fw = ui.w - ui.dp(24.0) - bw - ui.dp(8.0);
        let (fx, bx) = if ui.rtl {
            (ui.dp(12.0) + bw + ui.dp(8.0), ui.dp(12.0))
        } else {
            (ui.dp(12.0), ui.dp(12.0) + fw + ui.dp(8.0))
        };
        ui.field(
            Rect::new(fx, y, fw, ui.dp(48.0)),
            "host",
            &self.f("host"),
            "ultar",
        );
        if ui.button(
            Rect::new(bx, y, bw, ui.dp(46.0)),
            self.t("Apply"),
            Btn::Primary,
        ) {
            let h = self.f("host").trim().to_string();
            let ok = !h.is_empty()
                && h.len() <= 63
                && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !h.starts_with('-');
            if ok && nix::unistd::sethostname(&h).is_ok() {
                self.set.hostname = h;
                self.save();
                self.toast(self.t("Saved").into());
            } else {
                self.toast(self.t("Invalid input").into());
            }
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
        }
        y += ui.dp(64.0);
        ui.section(&mut y, self.t("Device"));
        let (mt, _) = sys::meminfo();
        let up = sys::uptime_s();
        let rows = [
            ("Version", format!("Ultar OS {}", env!("CARGO_PKG_VERSION"))),
            (
                "Edition",
                ultar_hal_system::current_edition()
                    .label(
                        ultar_profile::msg::Lang::parse(self.lang())
                            .unwrap_or(ultar_profile::msg::Lang::Ar),
                    )
                    .to_string(),
            ),
            ("Kernel", sys::read_trim("/proc/sys/kernel/osrelease")),
            ("Processor", sys::cpu()),
            ("Memory (RAM)", sys::human(mt)),
            (
                "Uptime",
                format!("{} h {:02} min", up / 3600, up % 3600 / 60),
            ),
        ];
        for (k, v) in rows {
            ui.row(&mut y, None, self.t(k), None, Some(&v), false);
        }
        {
            let rss = sys::read_trim("/proc/self/statm")
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                * 4096;
            let v = format!(
                "{:.0} ms · {:.1} ms · {}",
                self.perf.0,
                self.perf.2,
                sys::human(rss)
            );
            ui.row(
                &mut y,
                None,
                self.t("Diagnostics"),
                Some(self.t("Start · frame time · memory of the interface")),
                Some(&v),
                false,
            );
        }
        if let Some(b) = sys::battery_info() {
            let st = match b.status.as_str() {
                "Charging" => self.t("Charging"),
                "Full" => self.t("Full"),
                _ => self.t("On battery"),
            };
            let mut detail: Vec<String> = Vec::new();
            if let Some(t) = b.temp_c {
                detail.push(format!("{t:.0} °C"));
            }
            if let Some(v) = b.volt {
                detail.push(format!("{v:.2} V"));
            }
            if !b.health.is_empty() {
                detail.push(b.health.clone());
            }
            let sub = detail.join(" · ");
            ui.row(
                &mut y,
                None,
                self.t("Battery"),
                if sub.is_empty() { None } else { Some(&sub) },
                Some(&format!("{} % · {st}", b.percent)),
                false,
            );
        } else if let Some((cap, chg)) = sys::battery() {
            let st = if chg {
                self.t("Charging")
            } else {
                self.t("On battery")
            };
            ui.row(
                &mut y,
                None,
                self.t("Battery"),
                None,
                Some(&format!("{cap} % · {st}")),
                false,
            );
        } else {
            ui.row(
                &mut y,
                None,
                self.t("Battery"),
                None,
                Some(self.t("Not detected")),
                false,
            );
        }
        if ui.row(
            &mut y,
            Some(Ic::Box),
            self.t("System update"),
            None,
            Some(crate::update::VERSION),
            true,
        ) {
            self.go(Screen::Update);
            ui.dirty = true;
        }
        // Ultar-Cloud: eindeutige Rufnummer
        if let Some(num) = ultar_cloud::my_number() {
            ui.section(&mut y, self.t("Ultar number"));
            ui.row(
                &mut y,
                Some(Ic::Bot),
                self.t("Your number"),
                Some(self.t("Calls between Ultar devices")),
                Some(&num),
                false,
            );
        }
        // Hersteller / Entwickler – Markenblock mit Logo
        ui.section(&mut y, self.t("Manufacturer"));
        {
            let lg = brand::logo();
            let lw = (ui.w * 0.5).min(ui.dp(210.0));
            let lh = lw * lg.h as f32 / lg.w as f32;
            ui.cv.image(&lg, (ui.w - lw) / 2.0, y + ui.dp(4.0), lw, lh);
            y += lh + ui.dp(14.0);
        }
        ui.row(
            &mut y,
            None,
            self.t("Brand"),
            None,
            Some("KM Code Craft"),
            false,
        );
        // Entwicklername voll (lateinisch + arabisch) als Unterzeile, nicht gekürzt im Wertfeld
        ui.row(
            &mut y,
            None,
            self.t("Developer"),
            Some("Mohamad Anas Kara Mohamad · محمد أنس قرة محمد"),
            None,
            false,
        );
        ui.row(
            &mut y,
            None,
            self.t("Website"),
            None,
            Some("code-craft.tech"),
            false,
        );
        let r = Rect::new(ui.dp(16.0), y + ui.dp(6.0), ui.w - ui.dp(32.0), ui.dp(50.0));
        if ui.button(r, self.t("Power"), Btn::Plain) {
            self.power_dialog = true;
            ui.dirty = true;
        }
        y += ui.dp(62.0);
        let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(50.0));
        if ui.button(r, self.t("Run setup again"), Btn::Plain) {
            self.set.setup_done = false;
            self.save();
            self.stack = vec![(Screen::Setup(0), 0.0)];
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
        }
        y += ui.dp(70.0);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_store(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Packages"));
        let mut y = area.y;
        if !sys::compat_ready() {
            let c = ui.th.danger;
            ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                self.t("Ubuntu area not available"),
                15.0,
                c,
            );
            return;
        }
        let inst_tab = self.t("Installed");
        if let Some(i) = ui.chips(
            &mut y,
            &["KM Code Craft", "Ubuntu", inst_tab],
            self.store_tab,
        ) {
            self.store_tab = i;
            *sc = 0.0;
            self.focus = None;
            self.focus_reset = true;
        }
        if self.store_tab == 0 {
            if !sys::online() {
                let r = Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(26.0));
                let c = ui.th.danger;
                ui.label(
                    r,
                    self.t("No internet connection"),
                    13.0,
                    c,
                    false,
                    Al::Start,
                );
                y += ui.dp(28.0);
            }
            let list = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
            return self.s_apps(ui, list, sc);
        }
        if self.store_tab == 2 {
            let list = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
            return self.s_installed(ui, list, sc);
        }
        let bw = ui.dp(104.0);
        let fw = ui.w - ui.dp(24.0) - bw - ui.dp(8.0);
        let (fx, bx) = if ui.rtl {
            (ui.dp(12.0) + bw + ui.dp(8.0), ui.dp(12.0))
        } else {
            (ui.dp(12.0), ui.dp(12.0) + fw + ui.dp(8.0))
        };
        ui.field(
            Rect::new(fx, y, fw, ui.dp(48.0)),
            "q",
            &self.f("q"),
            self.t("Search packages"),
        );
        if ui.button(
            Rect::new(bx, y, bw, ui.dp(46.0)),
            self.t("Search"),
            Btn::Primary,
        ) {
            self.do_search();
            ui.dirty = true;
        }
        y += ui.dp(60.0);
        let online = sys::online();
        let old = ui.enabled;
        ui.enabled = old && online && !self.job_running();
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Update package list"),
            Btn::Plain,
        ) {
            self.start_job(self.t("Update package list").into(), &["apt", "update"]);
            ui.dirty = true;
        }
        ui.enabled = old;
        y += ui.dp(54.0);
        if !online {
            let r = Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(26.0));
            let c = ui.th.danger;
            ui.label(
                r,
                self.t("No internet connection"),
                13.0,
                c,
                false,
                Al::Start,
            );
            y += ui.dp(28.0);
        }
        let list = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
        ui.scroll_begin(list, sc);
        let mut ly = list.y - *sc + ui.dp(4.0);
        if self.searching {
            self.caption(ui, &mut ly, self.t("Searching…"));
        } else if self.searched && self.results.is_empty() {
            self.caption(ui, &mut ly, self.t("No results"));
        }
        let inst = self.t("Installed");
        let mut open = None;
        for (n, d) in &self.results {
            let v = self.installed.contains(n).then_some(inst);
            // Paketnamen und Beschreibungen sind Latein -> immer von links
            if ui.row(&mut ly, None, n, Some(d), v, true) {
                open = Some((n.clone(), d.clone()));
            }
        }
        if let Some((n, d)) = open {
            self.go(Screen::Pkg(n, d));
            ui.dirty = true;
        }
        ui.scroll_end(list, ly + *sc - list.y, sc);
    }

    fn s_pkg(&mut self, ui: &mut Ui, area: Rect, name: &str, desc: &str) {
        let area = self.header(ui, area, name);
        let mut y = area.y;
        let c = ui.th.sub;
        y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), desc, 15.0, c) + ui.dp(16.0);
        let installed = self.installed.contains(name);
        ui.row(
            &mut y,
            Some(Ic::Box),
            "Status",
            None,
            Some(if installed {
                self.t("Installed")
            } else {
                self.t("Not installed")
            }),
            false,
        );
        let r = Rect::new(ui.dp(16.0), y + ui.dp(8.0), ui.w - ui.dp(32.0), ui.dp(52.0));
        let old = ui.enabled;
        ui.enabled =
            old && !self.job_running() && sys::valid_pkg(name) && (installed || sys::online());
        if installed {
            if ui.button(r, self.t("Remove"), Btn::Danger) {
                self.remove_pkg(name);
                ui.dirty = true;
            }
        } else if ui.button(r, self.t("Install"), Btn::Primary) {
            self.start_job(
                format!("{}: {name}", self.t("Install")),
                &["apt", "install", "--no-install-recommends", name],
            );
            if !self.user_pkgs.list.iter().any(|p| p == name) {
                self.user_pkgs.list.push(name.to_string());
                let _ = sys::save("user-pkgs.toml", &self.user_pkgs);
            }
            ui.dirty = true;
        }
        ui.enabled = old;
        if !installed && !sys::online() {
            let c = ui.th.danger;
            ui.label(
                Rect::new(
                    ui.dp(22.0),
                    r.bottom() + ui.dp(8.0),
                    ui.w - ui.dp(44.0),
                    ui.dp(26.0),
                ),
                self.t("No internet connection"),
                13.0,
                c,
                false,
                Al::Start,
            );
        }
    }

    /// Ubuntu-Paket entfernen (mit nicht mehr gebrauchten Abhängigkeiten) und aus der Merkliste streichen.
    fn remove_pkg(&mut self, name: &str) {
        self.start_job(
            format!("{}: {name}", self.t("Remove")),
            &["apt", "remove", "--autoremove", name],
        );
        self.user_pkgs.list.retain(|p| p != name);
        let _ = sys::save("user-pkgs.toml", &self.user_pkgs);
    }

    /// Reiter „Installiert“: alles, was der Nutzer selbst installiert hat, mit einem Tippen entfernbar.
    fn s_installed(&mut self, ui: &mut Ui, list: Rect, sc: &mut f32) {
        ui.scroll_begin(list, sc);
        let mut y = list.y - *sc + ui.dp(4.0);
        let mut act_app: Option<apps::App> = None;
        let mut act_pkg: Option<String> = None;
        let mut n = 0;
        let bw = ui.dp(112.0);
        let busy = self.job_running();
        for a in self
            .apps
            .clone()
            .into_iter()
            .filter(|a| self.installed.contains(&a.package))
        {
            let y0 = y;
            ui.row(
                &mut y,
                Some(Ic::Box),
                &a.name,
                Some("KM Code Craft"),
                None,
                false,
            );
            let bx = if ui.rtl {
                ui.dp(24.0)
            } else {
                ui.w - ui.dp(24.0) - bw
            };
            let old = ui.enabled;
            ui.enabled = old && !busy;
            if ui.button(
                Rect::new(bx, y0 + ui.dp(12.0), bw, ui.dp(44.0)),
                self.t("Remove"),
                Btn::Danger,
            ) {
                act_app = Some(a.clone());
            }
            ui.enabled = old;
            n += 1;
        }
        for p in self
            .user_pkgs
            .list
            .clone()
            .into_iter()
            .filter(|p| self.installed.contains(p))
        {
            let y0 = y;
            ui.row(&mut y, Some(Ic::Box), &p, Some("Ubuntu"), None, false);
            let bx = if ui.rtl {
                ui.dp(24.0)
            } else {
                ui.w - ui.dp(24.0) - bw
            };
            let old = ui.enabled;
            ui.enabled = old && !busy;
            if ui.button(
                Rect::new(bx, y0 + ui.dp(12.0), bw, ui.dp(44.0)),
                self.t("Remove"),
                Btn::Danger,
            ) {
                act_pkg = Some(p.clone());
            }
            ui.enabled = old;
            n += 1;
        }
        if n == 0 {
            self.caption(ui, &mut y, self.t("Nothing installed yet"));
        }
        ui.scroll_end(list, y + *sc - list.y, sc);
        if let Some(a) = act_app {
            self.job = Some(Job {
                title: format!("{}: {}", self.t("Remove"), a.name),
                lines: Vec::new(),
                done: None,
            });
            apps::remove(&self.bg, a);
            self.go(Screen::Job);
            ui.dirty = true;
        }
        if let Some(p) = act_pkg {
            self.remove_pkg(&p);
            ui.dirty = true;
        }
    }

    /// Protokoll von unten nach oben zeichnen (neueste Zeile unten).
    fn log(&mut self, ui: &mut Ui, r: Rect, lines: &[String]) {
        ui.card(r);
        let inner = r.inset(ui.dp(12.0));
        ui.set_clip(Some(inner));
        let c = ui.th.text;
        let mut y = inner.bottom();
        for l in lines.iter().rev() {
            let sz = ui.dp(13.0);
            let h = ui.tx.measure(l, sz, false, inner.w).1.max(sz);
            y -= h;
            if y + h < inner.y {
                break;
            }
            ui.tx.draw(ui.cv, l, inner.x, y, sz, c, false, inner.w);
        }
        ui.set_clip(None);
    }

    fn s_job(&mut self, ui: &mut Ui, area: Rect) {
        let Some(j) = &self.job else { return };
        let (title, done, lines) = (j.title.clone(), j.done, j.lines.clone());
        let area = self.header(ui, area, &title);
        let (txt, col) = match done {
            None => (self.t("Working…").to_string(), ui.th.accent),
            Some(0) => (self.t("Done").to_string(), ui.th.ok),
            Some(c) => (format!("{} (Code {c})", self.t("Failed")), ui.th.danger),
        };
        ui.label(
            Rect::new(ui.dp(22.0), area.y, ui.w - ui.dp(44.0), ui.dp(30.0)),
            &txt,
            16.0,
            col,
            true,
            Al::Start,
        );
        let r = Rect::new(
            ui.dp(12.0),
            area.y + ui.dp(38.0),
            ui.w - ui.dp(24.0),
            (area.h - ui.dp(50.0)).max(0.0),
        );
        self.log(ui, r, &lines);
    }

    /// Terminal wie in der Web-Demo: schwarze Fläche mit Ausgabe und Eingabezeile, Befehls-Pillen unten
    fn s_terminal(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("Terminal"));
        if std::mem::take(&mut self.hdr_tap) {
            self.term.clear();
            ui.dirty = true;
        }
        if self.term.is_empty() {
            let k = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
            let kern = if std::env::var("ULTAR_KERN").is_ok() || k.trim().is_empty() {
                "Ultar-Kern".to_string()
            } else {
                format!("Linux {}", k.trim())
            };
            self.term.push(format!(
                "Ultar OS {} ({kern} {}) · KM Code Craft",
                crate::update::VERSION,
                std::env::consts::ARCH
            ));
            self.term
                .push(self.t("Type \"help\" for commands.").to_string());
        }
        let chips_h = ui.dp(52.0);
        let tr = Rect::new(0.0, area.y, ui.w, (area.h - chips_h).max(ui.dp(60.0)));
        ui.cv.rect(tr, rgb(0x07080A));
        let pad = ui.dp(12.0);
        let fs = 12.5;
        let lh = ui.dp(19.0);
        let maxw = tr.w - 2.0 * pad;
        // Zeilen umbrechen (zeichenweise, damit lange Pfade nicht abgeschnitten werden)
        let mut wrapped: Vec<(String, bool)> = Vec::new();
        for l in &self.term {
            let cmd = l.starts_with("$ ");
            let mut cur = String::new();
            let mut cw = 0.0;
            for ch in l.chars() {
                // Breite je Zeichen (aus dem Schrift-Cache) aufsummieren statt jedes Präfix neu zu setzen
                let w = ui.text_w(ch.encode_utf8(&mut [0u8; 4]), fs, false);
                if cw + w > maxw && !cur.is_empty() {
                    wrapped.push((std::mem::take(&mut cur), cmd));
                    cw = 0.0;
                }
                cur.push(ch);
                cw += w;
            }
            wrapped.push((cur, cmd));
        }
        let rows = ((tr.h - 2.0 * pad) / lh).floor().max(1.0) as usize;
        let show = rows.saturating_sub(1);
        let start = wrapped.len().saturating_sub(show);
        let old = ui.rtl;
        ui.rtl = false;
        let mut y = tr.y + pad;
        for (t, cmd) in &wrapped[start..] {
            // Zeilen, die arabisch beginnen, rechtsbündig (wie ein RTL-Terminal)
            let rtl = t.chars().find(|c| c.is_alphabetic()).is_some_and(|c| {
                ('\u{0590}'..='\u{08FF}').contains(&c) || ('\u{FB1D}'..='\u{FEFC}').contains(&c)
            });
            ui.label(
                Rect::new(tr.x + pad, y, maxw, lh),
                t,
                fs,
                if *cmd { rgb(0xF0E8A0) } else { rgb(0xCFE8C8) },
                false,
                if rtl { Al::End } else { Al::Start },
            );
            y += lh;
        }
        // Eingabezeile: ultar@a30:~$ Befehl▌
        let mut x = tr.x + pad;
        let wer = format!(
            "ultar@{}",
            if self.set.hostname.is_empty() {
                "ultar"
            } else {
                self.set.hostname.as_str()
            }
        );
        for (part, col) in [
            (wer.as_str(), 0xD8C870u32),
            (":", 0xCFE8C8),
            ("~", 0x7AB8FF),
            ("$ ", 0xCFE8C8),
        ] {
            ui.label(
                Rect::new(x, y, maxw, lh),
                part,
                fs,
                rgb(col),
                false,
                Al::Start,
            );
            x += ui.text_w(part, fs, false);
        }
        let focused = self.focus == Some("cmd");
        let cmd = self.f("cmd");
        let shown = if focused { format!("{cmd}▌") } else { cmd };
        ui.label(
            Rect::new(x, y, tr.right() - pad - x, lh),
            &shown,
            fs,
            rgb(0xFFFFFF),
            false,
            Al::Start,
        );
        ui.rtl = old;
        if ui.tap(tr) && !focused {
            self.focus = Some("cmd");
            self.focus_reset = true;
            ui.dirty = true;
        }
        // Befehls-Pillen
        let cmds = [
            "help",
            "neofetch",
            "ultar-ctl status",
            "ls",
            "df",
            "apt install htop",
            "top",
        ];
        let mut px = ui.dp(10.0);
        let py = tr.bottom() + ui.dp(10.0);
        let old = ui.rtl;
        ui.rtl = false;
        for cmd in cmds {
            let w = ui.text_w(cmd, 13.0, false) + ui.dp(26.0);
            if px + w > ui.w - ui.dp(6.0) {
                break;
            }
            let (_, t) = ui.pill(px, py, cmd, false);
            if t && !self.term_busy {
                self.fields.insert("cmd", cmd.to_string());
                self.run_cmd();
                ui.dirty = true;
            }
            px += w + ui.dp(6.0);
        }
        ui.rtl = old;
    }

    fn s_update(&mut self, ui: &mut Ui, area: Rect) {
        let area = self.header(ui, area, self.t("System update"));
        let mut y = area.y;
        ui.row(
            &mut y,
            None,
            self.t("Installed version"),
            None,
            Some(crate::update::VERSION),
            false,
        );
        let old = ui.enabled;
        ui.enabled = old && !self.upd_checking && sys::online() && sys::compat_ready();
        let label = if self.upd_checking {
            self.t("Searching…")
        } else {
            self.t("Check for updates")
        };
        if ui.button(
            Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(46.0)),
            label,
            Btn::Primary,
        ) {
            self.upd_checking = true;
            self.upd = None;
            crate::update::check(&self.bg, self.apps_cfg.github_token.clone());
            ui.dirty = true;
        }
        ui.enabled = old;
        y += ui.dp(60.0);
        if !sys::online() {
            let c = ui.th.danger;
            ui.label(
                Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(26.0)),
                self.t("No internet connection"),
                13.0,
                c,
                false,
                Al::Start,
            );
            y += ui.dp(30.0);
        }
        let Some(r) = self.upd.clone() else { return };
        if !r.error.is_empty() {
            let c = ui.th.danger;
            let hint = if self.apps_cfg.github_token.is_empty() {
                self.t("The project is private: enter a GitHub token under Packages.")
            } else {
                ""
            };
            y += ui.para(
                ui.dp(22.0),
                y,
                ui.w - ui.dp(44.0),
                &format!("{} {hint}", r.error),
                14.0,
                c,
            );
            let _ = y;
            return;
        }
        ui.row(
            &mut y,
            None,
            self.t("Latest version"),
            None,
            Some(&r.tag),
            false,
        );
        if !r.newer {
            let c = ui.th.ok;
            ui.label(
                Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(30.0)),
                self.t("The system is up to date."),
                15.0,
                c,
                true,
                Al::Start,
            );
            return;
        }
        let old = ui.enabled;
        ui.enabled = old && !self.job_running();
        if ui.button(
            Rect::new(ui.dp(16.0), y + ui.dp(4.0), ui.w - ui.dp(32.0), ui.dp(46.0)),
            self.t("Download and install"),
            Btn::Primary,
        ) {
            self.job = Some(Job {
                title: format!("{} {}", self.t("System update"), r.tag),
                lines: Vec::new(),
                done: None,
            });
            crate::update::install(&self.bg, r, self.apps_cfg.github_token.clone());
            self.go(Screen::Job);
            ui.dirty = true;
        }
        ui.enabled = old;
    }

    fn s_wifi_pw(&mut self, ui: &mut Ui, area: Rect, ssid: &str, secured: bool) {
        let area = self.header(ui, area, ssid);
        let mut y = area.y;
        if secured {
            self.field_row(ui, &mut y, "wpw", "Password", "");
        }
        let r = Rect::new(ui.dp(16.0), y + ui.dp(6.0), ui.w - ui.dp(32.0), ui.dp(52.0));
        if ui.button(r, self.t("Connect"), Btn::Primary) {
            let pw = if secured {
                self.f("wpw")
            } else {
                String::new()
            };
            if secured && pw.is_empty() {
                self.toast(self.t("Invalid input").into());
            } else {
                match crate::wifi::check_pw(&pw) {
                    Ok(()) => {
                        self.wifi_connecting = Some(ssid.to_string());
                        sys::wifi_connect(&self.bg, ssid.to_string(), Some(pw));
                        self.fields.insert("wpw", String::new());
                        self.back();
                    }
                    Err(e) => self.toast(format!("{}: {e}", self.t("Invalid input"))),
                }
            }
            ui.dirty = true;
        }
    }

    fn s_apps(&mut self, ui: &mut Ui, list: Rect, sc: &mut f32) {
        ui.scroll_begin(list, sc);
        let mut y = list.y - *sc + ui.dp(4.0);
        let inst = self.t("Installed");
        let mut act: Option<(apps::App, bool)> = None;
        for a in self.apps.clone() {
            let installed = self.installed.contains(&a.package);
            let y0 = y;
            let m = ui.dp(12.0);
            let r = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(132.0));
            y += r.h + ui.dp(8.0);
            if !ui.visible(r) {
                continue;
            }
            ui.card(r);
            let (c, cs) = (ui.th.text, ui.th.sub);
            let mk = crate::brand::mark();
            let s = ui.dp(40.0);
            ui.cv.rrect(
                Rect::new(r.x + ui.dp(14.0), y0 + ui.dp(14.0), s, s),
                ui.dp(10.0),
                crate::brand::BOOT_BG,
            );
            ui.cv.image(
                &mk,
                r.x + ui.dp(18.0),
                y0 + ui.dp(19.0),
                s - ui.dp(8.0),
                (s - ui.dp(8.0)) * mk.h as f32 / mk.w as f32,
            );
            let tx = r.x + ui.dp(66.0);
            ui.label(
                Rect::new(tx, y0 + ui.dp(10.0), r.w - ui.dp(80.0), ui.dp(26.0)),
                &a.name,
                16.0,
                c,
                true,
                Al::Start,
            );
            let src = if !a.url.is_empty() {
                "URL".to_string()
            } else {
                format!("GitHub · {}", a.github)
            };
            ui.label(
                Rect::new(tx, y0 + ui.dp(34.0), r.w - ui.dp(80.0), ui.dp(20.0)),
                &if installed {
                    format!("{inst} · {src}")
                } else {
                    src
                },
                12.0,
                cs,
                false,
                Al::Start,
            );
            if !installed {
                ui.para(
                    r.x + ui.dp(14.0),
                    y0 + ui.dp(60.0),
                    r.w - ui.dp(150.0),
                    &a.description,
                    12.0,
                    cs,
                );
            }
            let br = Rect::new(
                r.right() - ui.dp(126.0),
                y0 + ui.dp(74.0),
                ui.dp(112.0),
                ui.dp(44.0),
            );
            let old = ui.enabled;
            ui.enabled = old && !self.job_running() && (installed || sys::online());
            if installed {
                if ui.button(br, self.t("Remove"), Btn::Danger) {
                    act = Some((a.clone(), false));
                }
                // neueste Fassung aus dem Release darüber installieren
                let ur = Rect::new(br.x - br.w - ui.dp(8.0), br.y, br.w, br.h);
                ui.enabled = old && !self.job_running() && sys::online();
                if ui.button(ur, self.t("Update"), Btn::Plain) {
                    act = Some((a.clone(), true));
                }
            } else if ui.button(br, self.t("Install"), Btn::Primary) {
                act = Some((a.clone(), true));
            }
            ui.enabled = old;
        }
        if self.apps.is_empty() {
            self.caption(ui, &mut y, self.t("No results"));
        }
        // Zugang für private GitHub-Projekte
        self.caption(ui, &mut y, self.t("GitHub token (for private projects)"));
        let bw = ui.dp(110.0);
        let fw = ui.w - ui.dp(24.0) - bw - ui.dp(8.0);
        let r = Rect::new(ui.dp(12.0), y, fw, ui.dp(52.0));
        if ui.visible(r) {
            let shown = self.f("ghtok");
            let masked = if self.focus == Some("ghtok") || shown.len() < 8 {
                shown.clone()
            } else {
                format!("{}…{}", &shown[..4], "•".repeat(6))
            };
            ui.field(r, "ghtok", &masked, "ghp_…");
            if ui.button(
                Rect::new(ui.dp(12.0) + fw + ui.dp(8.0), y, bw, ui.dp(46.0)),
                self.t("Apply"),
                Btn::Primary,
            ) {
                self.apps_cfg.github_token = shown.trim().to_string();
                match sys::save("apps-auth.toml", &self.apps_cfg) {
                    Ok(()) => self.toast(self.t("Saved").into()),
                    Err(e) => self.toast(format!("{}: {e}", self.t("Could not save"))),
                }
                self.focus = None;
                self.focus_reset = true;
                ui.dirty = true;
            }
        }
        y += ui.dp(64.0);
        ui.scroll_end(list, y + *sc - list.y, sc);
        if let Some((a, install)) = act {
            let title = format!(
                "{}: {}",
                self.t(if install { "Install" } else { "Remove" }),
                a.name
            );
            self.job = Some(Job {
                title,
                lines: Vec::new(),
                done: None,
            });
            if install {
                apps::install(&self.bg, a, self.apps_cfg.github_token.clone());
            } else {
                apps::remove(&self.bg, a);
            }
            self.go(Screen::Job);
            ui.dirty = true;
        }
    }

    fn hw_rows(&mut self, ui: &mut Ui, y: &mut f32, only_missing: bool) {
        let devs = self.hw.clone();
        for d in devs
            .iter()
            .filter(|d| !only_missing || (d.driver.is_none() && !d.passive))
        {
            let v = match &d.driver {
                Some(n) => n.clone(),
                None if d.passive => "–".into(),
                None => self.t("no driver").to_string(),
            };
            ui.row(
                y,
                Some(Ic::Chip),
                self.t(d.kind),
                Some(&format!("{} {}", d.bus, d.id)),
                Some(&v),
                false,
            );
        }
    }

    fn driver_button(&mut self, ui: &mut Ui, y: &mut f32) {
        let r = Rect::new(ui.dp(16.0), *y, ui.w - ui.dp(32.0), ui.dp(50.0));
        let old = ui.enabled;
        let online = sys::online();
        ui.enabled = old && online && sys::compat_ready() && !self.job_running();
        if ui.button(r, self.t("Search for drivers online"), Btn::Primary) {
            self.start_driver_search();
            ui.dirty = true;
        }
        ui.enabled = old;
        *y += ui.dp(58.0);
        if !online {
            let c = ui.th.danger;
            ui.label(
                Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(26.0)),
                self.t("No internet connection"),
                13.0,
                c,
                false,
                Al::Start,
            );
            *y += ui.dp(28.0);
        }
    }

    fn hw_summary(&mut self) -> String {
        let miss = hw::missing(&self.hw).len();
        if miss == 0 {
            format!(
                "{} {} · {}",
                self.hw.len(),
                self.t("devices detected"),
                self.t("All devices have a driver")
            )
        } else {
            format!(
                "{} {} · {miss} {}",
                self.hw.len(),
                self.t("devices detected"),
                self.t("without driver")
            )
        }
    }

    fn s_gnss(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Location (GPS)"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        if !crate::privacy::allowed(&self.set, crate::privacy::Cap::Location) {
            self.caption(ui, &mut y, self.t("Turned off in privacy settings"));
            ui.scroll_end(area, y + *sc - area.y, sc);
            return;
        }
        let _ = crate::gnss::read_fix(&mut self.gnss);
        let lang = self.set.lang.clone();
        let st = self.gnss.clone();
        crate::gnss::render_gnss(ui, &mut y, &st, |k| crate::i18n::tr(&lang, k).to_string());
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn s_hardware(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Hardware & drivers"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let sum = self.hw_summary();
        self.caption(ui, &mut y, &sum);
        self.driver_button(ui, &mut y);
        if let Some(j) = &self.job {
            if j.title == self.t("Driver search") {
                let (lines, done) = (j.lines.clone(), j.done);
                let (txt, col) = match done {
                    None => (self.t("Working…"), ui.th.accent),
                    Some(0) => (self.t("Done"), ui.th.ok),
                    Some(_) => (self.t("Failed"), ui.th.danger),
                };
                ui.label(
                    Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(26.0)),
                    txt,
                    14.0,
                    col,
                    true,
                    Al::Start,
                );
                y += ui.dp(30.0);
                let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(190.0));
                if ui.visible(r) {
                    let clip = ui.clip;
                    self.log(ui, r, &lines);
                    ui.set_clip(clip);
                }
                y += ui.dp(198.0);
            }
        }
        self.selftest_rows(ui, &mut y);
        self.hw_rows(ui, &mut y, false);
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    /// Selbsttest-Bericht: pro Komponente OK/Warnung/Fehler mit Detail.
    fn selftest_rows(&mut self, ui: &mut Ui, y: &mut f32) {
        use crate::selftest::Stat;
        if self.selftest.is_empty() {
            return;
        }
        // Abschnittsüberschrift
        let ok = self.selftest.iter().filter(|c| c.stat == Stat::Ok).count();
        let n = self.selftest.len();
        ui.label(
            Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(24.0)),
            &format!("{} · {ok}/{n} OK", self.t("Component test")),
            14.0,
            ui.th.accent,
            true,
            Al::Start,
        );
        *y += ui.dp(30.0);
        let checks = self.selftest.clone();
        for c in &checks {
            let (sym, col) = match c.stat {
                Stat::Ok => ("OK", ui.th.ok),
                Stat::Warn => ("!", ui.th.accent),
                Stat::Fail => ("X", ui.th.danger),
                Stat::Info => ("i", ui.th.sub),
            };
            let r = Rect::new(ui.dp(16.0), *y, ui.w - ui.dp(32.0), ui.dp(60.0));
            if ui.visible(r) {
                ui.card(r);
                // farbiger Statuspunkt links
                let dot = Rect::new(
                    r.x + ui.dp(12.0),
                    r.cy() - ui.dp(13.0),
                    ui.dp(26.0),
                    ui.dp(26.0),
                );
                ui.cv.rrect(dot, ui.dp(13.0), col);
                ui.label(dot, sym, 11.0, ui.th.bg, true, Al::Center);
                // Titel + Detail
                let tx = if ui.rtl {
                    r.x + ui.dp(14.0)
                } else {
                    r.x + ui.dp(48.0)
                };
                let tw = r.w - ui.dp(62.0);
                ui.label(
                    Rect::new(tx, r.y + ui.dp(9.0), tw, ui.dp(22.0)),
                    self.t(c.comp),
                    15.0,
                    ui.th.text,
                    true,
                    Al::Start,
                );
                ui.label(
                    Rect::new(tx, r.y + ui.dp(32.0), tw, ui.dp(20.0)),
                    &c.detail,
                    12.0,
                    ui.th.sub,
                    false,
                    Al::Start,
                );
            }
            *y += ui.dp(66.0);
        }
        *y += ui.dp(8.0);
    }

    fn agent_status(&mut self, ui: &mut Ui, y: &mut f32) {
        let (msg, col) = if !self.agent.enabled {
            (self.t("Agent is disabled"), ui.th.sub)
        } else if self.agent.backend == "claude" {
            if self.agent.api_key.trim().is_empty() {
                (
                    self.t("Enter your Anthropic API key to use Claude."),
                    ui.th.danger,
                )
            } else if !sys::online() {
                (self.t("No internet connection"), ui.th.danger)
            } else {
                (
                    self.t("Claude is used over the internet (Anthropic API)."),
                    ui.th.ok,
                )
            }
        } else if self.agent_up == Some(true) {
            (self.t("Language model ready"), ui.th.ok)
        } else if !agent::model_installed() && self.agent.endpoint.starts_with("127.") {
            (self.t("No language model installed yet."), ui.th.danger)
        } else {
            (self.t("Language model not reachable"), ui.th.danger)
        };
        *y += ui.para(ui.dp(22.0), *y, ui.w - ui.dp(44.0), msg, 14.0, col) + ui.dp(10.0);
    }

    fn agent_toggles(&mut self, ui: &mut Ui, y: &mut f32) {
        let mut en = self.agent.enabled;
        if ui.toggle_row(y, self.t("Enable agent"), None, &mut en) {
            self.agent.enabled = en;
            self.save_agent();
            self.check_agent();
        }
        let mut cf = self.agent.confirm;
        if ui.toggle_row(
            y,
            self.t("Ask before risky actions"),
            Some(
                self.t("Calls and services. Root commands, security, camera and files always ask."),
            ),
            &mut cf,
        ) {
            self.agent.confirm = cf;
            self.save_agent();
        }
    }

    fn s_agent_set(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Local agent"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let c = ui.th.sub;
        y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("The agent can operate the whole device for you: settings, packages, services and commands. It runs on the device itself, without cloud."), 14.0, c) + ui.dp(14.0);
        self.agent_toggles(ui, &mut y);
        self.caption(ui, &mut y, self.t("Language model"));
        let local = self.t("On the device");
        if let Some(i) = ui.chips(
            &mut y,
            &[local, "Claude"],
            if self.agent.backend == "claude" { 1 } else { 0 },
        ) {
            self.agent.backend = if i == 1 { "claude" } else { "local" }.into();
            self.agent_up = None;
            self.save_agent();
            self.check_agent();
        }
        let claude = self.agent.backend == "claude";
        if claude {
            self.field_row(ui, &mut y, "akey", "Anthropic API key", "sk-ant-…");
            self.caption(ui, &mut y, self.t("Model"));
        } else {
            self.caption(ui, &mut y, self.t("Model server address"));
        }
        let bw = ui.dp(110.0);
        let fw = ui.w - ui.dp(24.0) - bw - ui.dp(8.0);
        if claude {
            ui.field(
                Rect::new(ui.dp(12.0), y, fw, ui.dp(48.0)),
                "amodel",
                &self.f("amodel"),
                "claude-haiku-5-5",
            );
        } else {
            ui.field(
                Rect::new(ui.dp(12.0), y, fw, ui.dp(48.0)),
                "ep",
                &self.f("ep"),
                "127.0.0.1:8080",
            );
        }
        if ui.button(
            Rect::new(ui.dp(12.0) + fw + ui.dp(8.0), y, bw, ui.dp(46.0)),
            self.t("Apply"),
            Btn::Primary,
        ) {
            let ep = self.f("ep").trim().to_string();
            if claude {
                self.agent.api_key = self.f("akey").trim().to_string();
                let m = self.f("amodel").trim().to_string();
                if !m.is_empty() {
                    self.agent.claude_model = m;
                }
                self.agent_up = None;
                self.save_agent();
                self.check_agent();
                self.toast(self.t("Saved").into());
            } else if ep.parse::<std::net::SocketAddr>().is_ok() {
                self.agent.endpoint = ep;
                self.agent_up = None;
                self.save_agent();
                self.check_agent();
                self.toast(self.t("Saved").into());
            } else {
                self.toast(self.t("Invalid input").into());
            }
            self.focus = None;
            self.focus_reset = true;
            ui.dirty = true;
        }
        y += ui.dp(64.0);
        self.agent_status(ui, &mut y);
        if !claude {
            self.agent_model_rows(ui, &mut y);
        }
        ui.scroll_end(area, y + *sc - area.y, sc);
    }

    fn d_confirm(&mut self, ui: &mut Ui) {
        let Some(call) = self.pending.clone() else {
            return;
        };
        ui.set_clip(None);
        ui.cv
            .rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0).alpha(0.55));
        ui.enabled = true;
        let w = (ui.w - ui.dp(48.0)).min(ui.dp(440.0));
        let desc = call.describe();
        let th = ui.tx.measure(&desc, ui.dp(15.0), false, w - ui.dp(40.0)).1;
        let risk = call.risk().map(|r| self.t(r).to_string());
        let extra = if risk.is_some() { ui.dp(28.0) } else { 0.0 };
        let h = ui.dp(150.0) + th + extra;
        let r = Rect::new((ui.w - w) / 2.0, (ui.h - h) / 2.0, w, h);
        ui.cv.rrect(r, ui.dp(20.0), ui.th.card);
        let (c, cs) = (ui.th.text, ui.th.sub);
        // Art des Eingriffs (rot bei Root/Sicherheit/Kamera/Dateien), darunter was genau ausgeführt wird
        if let Some(rk) = &risk {
            let col = if call.always_confirm() {
                ui.th.danger
            } else {
                ui.th.accent
            };
            ui.label(
                Rect::new(
                    r.x + ui.dp(20.0),
                    r.y + ui.dp(12.0),
                    r.w - ui.dp(40.0),
                    ui.dp(30.0),
                ),
                rk,
                16.0,
                col,
                true,
                Al::Start,
            );
        }
        ui.label(
            Rect::new(
                r.x + ui.dp(20.0),
                r.y + ui.dp(12.0) + extra,
                r.w - ui.dp(40.0),
                ui.dp(36.0),
            ),
            self.t("The agent wants to run:"),
            15.0,
            cs,
            false,
            Al::Start,
        );
        ui.tx.draw(
            ui.cv,
            &desc,
            r.x + ui.dp(20.0),
            r.y + ui.dp(54.0) + extra,
            ui.dp(15.0),
            c,
            false,
            w - ui.dp(40.0),
        );
        let bw = (r.w - ui.dp(52.0)) / 2.0;
        let by = r.bottom() - ui.dp(66.0);
        let deny = Rect::new(r.x + ui.dp(20.0), by, bw, ui.dp(48.0));
        let allow = Rect::new(deny.right() + ui.dp(12.0), by, bw, ui.dp(48.0));
        let held = ui.held(deny);
        ui.cv
            .rrect(deny, ui.dp(12.0), if held { ui.th.press } else { ui.th.bg });
        ui.label(deny, self.t("Deny"), 15.0, c, true, Al::Center);
        if ui.tap(deny) {
            self.deny();
            ui.dirty = true;
        } else if ui.button(allow, self.t("Allow"), Btn::Primary) {
            self.pending = None;
            self.exec(call);
            ui.dirty = true;
        }
    }

    /// Ersteinrichtung: Sprache -> Netzwerk -> Treiber -> Agent -> Fertig
    fn s_setup(&mut self, ui: &mut Ui, area: Rect, step: u8, sc: &mut f32) {
        let bh = ui.dp(76.0);
        let body = Rect::new(area.x, area.y, area.w, (area.h - bh).max(0.0));
        let (c, cs) = (ui.th.text, ui.th.sub);
        let mut next_label = self.t("Next");
        let mut can_next = true;
        if step == 0 {
            // Sprache: jede Sprache in ihrer eigenen Schrift, Arabisch zuerst
            let l = brand::logo();
            let lw = ui.dp(210.0).min(ui.w * 0.6);
            let lh = lw * l.h as f32 / l.w as f32;
            let top = body.y + (body.h * 0.08).min(ui.dp(60.0));
            if ui.th.bg.r < 0x80 {
                ui.cv.image(&l, (ui.w - lw) / 2.0, top, lw, lh);
            } else {
                // Logo ist für dunklen Grund gezeichnet
                let pad = ui.dp(16.0);
                ui.cv.rrect(
                    Rect::new(
                        (ui.w - lw) / 2.0 - pad,
                        top - pad,
                        lw + 2.0 * pad,
                        lh + 2.0 * pad,
                    ),
                    ui.dp(18.0),
                    brand::BOOT_BG,
                );
                ui.cv.image(&l, (ui.w - lw) / 2.0, top, lw, lh);
            }
            let mut y = top + lh + ui.dp(26.0);
            ui.label(
                Rect::new(0.0, y, ui.w, ui.dp(40.0)),
                "مرحبًا · Welcome · Willkommen",
                20.0,
                c,
                true,
                Al::Center,
            );
            y += ui.dp(44.0);
            ui.label(
                Rect::new(0.0, y, ui.w, ui.dp(28.0)),
                "اختر لغتك · Choose your language · Sprache wählen",
                13.0,
                cs,
                false,
                Al::Center,
            );
            y += ui.dp(44.0);
            for (code, name) in [("ar", "العربية"), ("en", "English"), ("de", "Deutsch")] {
                let r = Rect::new(ui.dp(32.0), y, ui.w - ui.dp(64.0), ui.dp(58.0));
                if ui.button(
                    r,
                    name,
                    if self.set.lang == code {
                        Btn::Primary
                    } else {
                        Btn::Plain
                    },
                ) {
                    self.set.lang = code.into();
                    self.save();
                    self.go_step(1);
                    ui.dirty = true;
                }
                y += ui.dp(68.0);
            }
            can_next = false;
        } else {
            let titles = [
                "",
                "Network",
                "Hardware & drivers",
                "Local agent",
                "You are all set",
            ];
            let inner = self.header(ui, body, self.t(titles[step as usize]));
            ui.scroll_begin(inner, sc);
            let mut y = inner.y - *sc;
            match step {
                1 => {
                    y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("Connect the device to the internet to search for drivers and install packages."), 14.0, cs) + ui.dp(14.0);
                    let ifs = sys::net_status();
                    if ifs.is_empty() {
                        self.caption(ui, &mut y, self.t("No network device found"));
                    }
                    for n in &ifs {
                        let st = match n.state.as_str() {
                            "verbunden" => self.t("Connected"),
                            "kein-kabel" => self.t("No cable"),
                            "dhcp" => self.t("Connecting…"),
                            _ => self.t("Error"),
                        };
                        ui.row(
                            &mut y,
                            Some(Ic::Globe),
                            &n.name,
                            Some(if n.ip.is_empty() { &n.mac } else { &n.ip }),
                            Some(st),
                            false,
                        );
                    }
                    if !sys::online() {
                        next_label = self.t("Skip");
                    }
                }
                2 => {
                    let sum = self.hw_summary();
                    self.caption(ui, &mut y, &sum);
                    self.hw_rows(ui, &mut y, true);
                    if !hw::missing(&self.hw).is_empty()
                        || self
                            .job
                            .as_ref()
                            .is_some_and(|j| j.title == self.t("Driver search"))
                    {
                        self.driver_button(ui, &mut y);
                    }
                    if let Some(j) = &self.job {
                        if j.title == self.t("Driver search") {
                            let (lines, done) = (j.lines.clone(), j.done);
                            let (txt, col) = match done {
                                None => (self.t("Working…"), ui.th.accent),
                                Some(0) => (self.t("Done"), ui.th.ok),
                                Some(_) => (self.t("Failed"), ui.th.danger),
                            };
                            ui.label(
                                Rect::new(ui.dp(22.0), y, ui.w - ui.dp(44.0), ui.dp(26.0)),
                                txt,
                                14.0,
                                col,
                                true,
                                Al::Start,
                            );
                            y += ui.dp(30.0);
                            let r = Rect::new(ui.dp(16.0), y, ui.w - ui.dp(32.0), ui.dp(220.0));
                            if ui.visible(r) {
                                let clip = ui.clip;
                                self.log(ui, r, &lines);
                                ui.set_clip(clip);
                            }
                            y += ui.dp(228.0);
                        }
                    }
                    can_next = !self.job_running();
                }
                3 => {
                    y += ui.para(ui.dp(22.0), y, ui.w - ui.dp(44.0), self.t("The agent can operate the whole device for you: settings, packages, services and commands. It runs on the device itself, without cloud."), 14.0, cs) + ui.dp(14.0);
                    self.agent_toggles(ui, &mut y);
                    if self.agent.enabled {
                        self.agent_status(ui, &mut y);
                    }
                }
                _ => {
                    let m = brand::mark();
                    let mw = ui.dp(120.0);
                    let mh = mw * m.h as f32 / m.w as f32;
                    ui.cv.image(&m, (ui.w - mw) / 2.0, y + ui.dp(10.0), mw, mh);
                    y += mh + ui.dp(30.0);
                    y += ui.para(
                        ui.dp(22.0),
                        y,
                        ui.w - ui.dp(44.0),
                        self.t("You can change everything later in the settings."),
                        15.0,
                        cs,
                    );
                    next_label = self.t("Finish");
                }
            }
            ui.scroll_end(inner, y + *sc - inner.y, sc);
        }
        // Fußzeile
        ui.set_clip(None);
        let fy = area.bottom() - bh + ui.dp(12.0);
        let bw = (ui.w - ui.dp(36.0)) / 2.0;
        let (rb, rn) = if ui.rtl {
            (
                Rect::new(ui.dp(24.0) + bw, fy, bw, ui.dp(52.0)),
                Rect::new(ui.dp(12.0), fy, bw, ui.dp(52.0)),
            )
        } else {
            (
                Rect::new(ui.dp(12.0), fy, bw, ui.dp(52.0)),
                Rect::new(ui.dp(24.0) + bw, fy, bw, ui.dp(52.0)),
            )
        };
        if step > 0 && ui.button(rb, self.t("Back"), Btn::Plain) {
            self.go_step(step - 1);
            ui.dirty = true;
        }
        if step > 0 {
            let old = ui.enabled;
            ui.enabled = old && can_next;
            if ui.button(rn, next_label, Btn::Primary) {
                if step >= 4 {
                    self.set.setup_done = true;
                    self.save();
                    self.stack = vec![(Screen::Home, 0.0)];
                    self.focus = None;
                    self.focus_reset = true;
                } else {
                    self.go_step(step + 1);
                }
                ui.dirty = true;
            }
            ui.enabled = old;
        }
        // Fortschrittspunkte
        for i in 0..5 {
            let x = ui.w / 2.0 + (i as f32 - 2.0) * ui.dp(14.0);
            let col = if i == step as i32 {
                ui.th.accent
            } else {
                ui.th.line
            };
            ui.cv
                .circle(x, area.bottom() - bh + ui.dp(2.0), ui.dp(3.5), col);
        }
    }

    fn go_step(&mut self, n: u8) {
        self.stack.clear();
        self.stack.push((Screen::Home, 0.0)); // Platzhalter, wird durch go() überdeckt
        self.go(Screen::Setup(n));
        self.stack.remove(0);
    }

    /// Kopf-Hinweis: zeigt die neueste Benachrichtigung kurz oben; Tippen öffnet das Panel.
    fn d_heads(&mut self, ui: &mut Ui) {
        let show = match &self.heads {
            Some((_, at)) => at.elapsed() < Duration::from_secs(4),
            None => false,
        };
        if !show {
            if self.heads.is_some()
                && self
                    .heads
                    .as_ref()
                    .map(|(_, a)| a.elapsed() >= Duration::from_secs(4))
                    .unwrap_or(false)
            {
                self.heads = None;
            }
            return;
        }
        let (title, body, call) = {
            let (n, _) = self.heads.as_ref().unwrap();
            (n.title.clone(), n.body.clone(), n.kind == "call")
        };
        ui.set_clip(None);
        let m = ui.dp(12.0);
        let h = if body.is_empty() {
            ui.dp(56.0)
        } else {
            ui.dp(72.0)
        };
        let r = Rect::new(m, ui.dp(34.0), ui.w - 2.0 * m, h);
        ui.card(r);
        let accent = if call { ui.th.ok } else { ui.th.accent };
        ui.cv
            .rrect(Rect::new(r.x, r.y, ui.dp(4.0), r.h), ui.dp(2.0), accent);
        let c = ui.th.text;
        ui.label(
            Rect::new(
                r.x + ui.dp(16.0),
                r.y + ui.dp(8.0),
                r.w - ui.dp(28.0),
                ui.dp(24.0),
            ),
            &title,
            15.0,
            c,
            true,
            Al::Start,
        );
        if !body.is_empty() {
            ui.label(
                Rect::new(
                    r.x + ui.dp(16.0),
                    r.y + ui.dp(34.0),
                    r.w - ui.dp(28.0),
                    ui.dp(22.0),
                ),
                &body,
                13.0,
                ui.th.sub,
                false,
                Al::Start,
            );
        }
        if ui.tap(r) {
            self.heads = None;
            self.shade = true;
        }
        ui.dirty = true; // Ablauf animieren/entfernen
    }

    /// Schnelleinstellungen + Benachrichtigungen (Wisch von oben).
    fn d_power(&mut self, ui: &mut Ui, active: bool) {
        ui.set_clip(None);
        ui.cv
            .rect(Rect::new(0.0, 0.0, ui.w, ui.h), rgb(0).alpha(0.55));
        ui.enabled = active;
        let w = (ui.w - ui.dp(48.0)).min(ui.dp(420.0));
        let h = ui.dp(248.0);
        let r = Rect::new((ui.w - w) / 2.0, (ui.h - h) / 2.0, w, h);
        ui.cv.rrect(r, ui.dp(20.0), ui.th.card);
        let c = ui.th.text;
        ui.label(
            Rect::new(r.x, r.y + ui.dp(10.0), r.w, ui.dp(44.0)),
            self.t("Power"),
            20.0,
            c,
            true,
            Al::Center,
        );
        let b = |i: f32| {
            Rect::new(
                r.x + ui.dp(16.0),
                r.y + ui.dp(60.0) + i * ui.dp(60.0),
                r.w - ui.dp(32.0),
                ui.dp(50.0),
            )
        };
        let (b0, b1, b2) = (b(0.0), b(1.0), b(2.0));
        if ui.button(b0, self.t("Restart"), Btn::Primary) {
            self.toast(self.t("Restart").into());
            sys::ctl("reboot");
            self.power_dialog = false;
        }
        if ui.button(b1, self.t("Power off"), Btn::Danger) {
            self.toast(self.t("Power off").into());
            sys::ctl("poweroff");
            self.power_dialog = false;
        }
        // Abbrechen: eigene Fläche, damit sie sich vom Kartenhintergrund abhebt
        let held = ui.held(b2);
        ui.cv
            .rrect(b2, ui.dp(12.0), if held { ui.th.press } else { ui.th.bg });
        ui.label(b2, self.t("Cancel"), 15.0, c, true, Al::Center);
        if ui.tap(b2) || (active && ui.p.released && !ui.p.moved && !r.contains(ui.p.px, ui.p.py)) {
            self.power_dialog = false;
        }
        if !self.power_dialog {
            ui.dirty = true;
        }
    }
}

fn tz_label(m: i32) -> String {
    format!(
        "UTC{}{:02}:{:02}",
        if m < 0 { '−' } else { '+' },
        m.abs() / 60,
        m.abs() % 60
    )
}

/// Benachrichtigungen lesen: eine je Zeile, Format "Titel\tText" (Text optional), neueste zuerst.
/// Quellen: /run/ultar/notify.log (flüchtig) und /data/ultar/notify.log. Dienste/Apps hängen an.
fn read_notifications() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in ["/run/ultar/notify.log", "/data/ultar/notify.log"] {
        if let Ok(s) = std::fs::read_to_string(p) {
            for line in s.lines().rev() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Some(n) = ultar_notify::parse_line(line) {
                    out.push((n.title, n.body));
                    if out.len() >= 10 {
                        return out;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod flow_tests {
    //! Abläufe über die echte Zeichen- und Eingabeschleife, ohne Gerät (Probebild statt Kamera).
    use super::*;
    use crate::gfx::Canvas;
    use crate::text::Text;
    use crate::ui::Ptr;

    fn draw(app: &mut App, cv: &mut Canvas, tx: &mut Text, p: &mut Ptr) {
        let (w, h) = (cv.w as f32, cv.h as f32);
        let mut ui = Ui {
            th: crate::ui::theme(app.set.dark, app.set.accent),
            k: crate::ui::scale_for(w, h, app.set.scale),
            w,
            h,
            cv,
            tx,
            p,
            rtl: false,
            enabled: true,
            focus: None,
            dirty: false,
            clip: None,
            links: Vec::new(),
            kb: Default::default(),
            seen: None,
        };
        app.draw(&mut ui);
    }

    /// What a failed flow assertion needs to explain itself (screen, focus, last message, env).
    fn state_of(app: &App) -> String {
        format!(
            "screen={:?} focus={:?} toast={:?} dry={:?} media={:?} data={:?} cores={:?}",
            app.stack.last().map(|s| &s.0),
            app.focus,
            app.toast.as_ref().map(|t| &t.0),
            std::env::var("ULTAR_DRY").ok(),
            std::env::var("ULTAR_MEDIA").ok(),
            std::env::var("ULTAR_DATA").ok(),
            std::thread::available_parallelism().ok()
        )
    }

    fn tap(app: &mut App, cv: &mut Canvas, tx: &mut Text, p: &mut Ptr, x: f32, y: f32) {
        (p.x, p.y, p.px, p.py) = (x, y, x, y);
        (p.down, p.pressed, p.released, p.moved) = (true, true, false, false);
        draw(app, cv, tx, p);
        (p.down, p.pressed, p.released) = (false, false, true);
        draw(app, cv, tx, p);
        p.released = false;
    }

    /// Waits up to 20 s: saving and camera capture run on worker threads, and shared CI runners are slow.
    fn wait_for(mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..800 {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[test]
    fn kamera_galerie_agent() {
        let _env = crate::sys::test_env_guard();
        let root = std::env::temp_dir().join(format!("ultar-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ultar")).unwrap();
        std::env::set_var("ULTAR_DRY", "1");
        // phone flows: the build machine must not pick the desktop surface by its own hardware
        std::env::set_var("ULTAR_OBERFLAECHE", "handy");
        std::env::set_var("ULTAR_MEDIA", &root);
        std::env::set_var("ULTAR_DATA", root.join("ultar"));
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_rfd, wfd) =
            nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC)
                .unwrap();
        let mut app = App::new(
            Bg {
                tx,
                wake: std::sync::Arc::new(wfd),
            },
            (720, 1520, "test".into()),
        );
        app.debug_open("camera", "de");
        let mut trail = vec![format!("open: {}", state_of(&app))];
        let (mut cv, mut text, mut p) = (Canvas::new(720, 1520), Text::new(), Ptr::default());
        draw(&mut app, &mut cv, &mut text, &mut p);
        trail.push(format!("first frame: {}", state_of(&app)));
        let cam_dir = app_gallery::camera_dir();
        let count = |ext: &str| {
            std::fs::read_dir(&cam_dir)
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.file_name().to_string_lossy().ends_with(ext))
                        .count()
                })
                .unwrap_or(0)
        };

        // --- Foto: Auslöser antippen -> JPEG im Kamera-Ordner, in voller Größe des Suchers
        let shutter = (360.0, 1302.0);
        tap(&mut app, &mut cv, &mut text, &mut p, shutter.0, shutter.1);
        trail.push(format!("after shutter: {}", state_of(&app)));
        assert!(
            wait_for(|| count(".jpg") == 1),
            "Foto wurde nicht gespeichert: {} | dir={:?} | {}",
            state_of(&app),
            std::fs::read_dir(&cam_dir)
                .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()),
            trail.join(" → ")
        );
        draw(&mut app, &mut cv, &mut text, &mut p);
        assert!(!app.cam.busy);
        let jpg = std::fs::read_dir(&cam_dir)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().ends_with(".jpg"))
            .unwrap()
            .path();
        assert_eq!(image::image_dimensions(&jpg).unwrap(), (480, 640));

        // --- Quadrat + 2-fach-Zoom: Ausschnitt 240x240
        app.cam.mode = 1;
        app.cam.zoom = 2.0;
        assert!(app.capture_photo());
        assert!(wait_for(|| count(".jpg") == 2));
        draw(&mut app, &mut cv, &mut text, &mut p);
        let dims: Vec<(u32, u32)> = std::fs::read_dir(&cam_dir)
            .unwrap()
            .flatten()
            .filter_map(|e| image::image_dimensions(e.path()).ok())
            .collect();
        assert!(dims.contains(&(240, 240)), "{dims:?}");

        // --- Video: starten, ein paar Bilder, stoppen -> abspielbares AVI
        app.cam.mode = 2;
        app.cam.zoom = 1.0;
        draw(&mut app, &mut cv, &mut text, &mut p);
        tap(&mut app, &mut cv, &mut text, &mut p, shutter.0, shutter.1);
        assert!(app.cam.rec.is_some(), "Aufnahme läuft nicht");
        for _ in 0..8 {
            draw(&mut app, &mut cv, &mut text, &mut p);
            std::thread::sleep(Duration::from_millis(40));
        }
        tap(&mut app, &mut cv, &mut text, &mut p, shutter.0, shutter.1);
        assert!(app.cam.rec.is_none());
        assert!(
            wait_for(|| {
                draw(&mut app, &mut cv, &mut text, &mut p);
                !app.cam.busy && count(".avi") == 1
            }),
            "Video wurde nicht gespeichert"
        );
        let avi = std::fs::read_dir(&cam_dir)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().ends_with(".avi"))
            .unwrap()
            .path();
        let mut rd = crate::mjpeg::Reader::open(avi.to_str().unwrap()).unwrap();
        assert!(
            rd.index.len() >= 2 && rd.frame(0).is_some(),
            "Video ohne Bilder"
        );

        // --- Galerie zeigt alle drei; Vorschaubilder entstehen im Hintergrund
        app.debug_open("gallery", "de");
        draw(&mut app, &mut cv, &mut text, &mut p);
        assert!(
            app.cam.worker.is_none() && app.cam.last_frame.is_none(),
            "Kamera wurde beim Verlassen nicht freigegeben"
        );
        assert_eq!(app.gal.items.len(), 3);
        assert!(
            wait_for(|| {
                draw(&mut app, &mut cv, &mut text, &mut p);
                !app.gal.pending()
            }),
            "Vorschaubilder fehlen"
        );
        assert!(
            std::fs::read_dir(root.join("ultar/cache/thumbs"))
                .unwrap()
                .count()
                >= 3
        );

        // --- Betrachter: Foto und Video zeichnen, Favorit setzen, blättern, löschen
        let list: Vec<String> = app.gal.items.iter().map(|i| i.path.clone()).collect();
        app.open_photo(list[0].clone(), list.clone());
        draw(&mut app, &mut cv, &mut text, &mut p);
        // Herz unten links (erste von fünf bzw. drei Aktionen)
        let n_act = if crate::mjpeg::is_video(&list[0]) {
            3.0
        } else {
            5.0
        };
        tap(
            &mut app,
            &mut cv,
            &mut text,
            &mut p,
            720.0 / n_act / 2.0,
            1520.0 - 104.0 - 84.0 - 8.0,
        );
        assert_eq!(app.gal.cfg.favorites, vec![list[0].clone()]);
        // nach links wischen -> nächstes Bild
        (p.x, p.y, p.px, p.py, p.down, p.pressed, p.moved) =
            (500.0, 700.0, 500.0, 700.0, true, true, false);
        draw(&mut app, &mut cv, &mut text, &mut p);
        (p.x, p.pressed, p.moved) = (200.0, false, true);
        draw(&mut app, &mut cv, &mut text, &mut p);
        (p.down, p.released) = (false, true);
        draw(&mut app, &mut cv, &mut text, &mut p);
        (p.released, p.moved) = (false, false);
        assert!(
            matches!(app.stack.last(), Some((Screen::Photo(x), _)) if *x == list[1]),
            "Wischen blättert nicht"
        );
        for path in &list {
            app.open_photo(path.clone(), list.clone());
            draw(&mut app, &mut cv, &mut text, &mut p);
            app.back();
        }

        // --- Agent bedient die Apps
        let call = |tool: &str, args: serde_json::Value| Call {
            tool: tool.into(),
            args,
        };
        assert!(app
            .exec_apps(&call(
                "set_alarm",
                serde_json::json!({"hour": 7, "minute": 30})
            ))
            .unwrap()
            .starts_with("ok"));
        assert!(app
            .tl
            .alarms
            .list
            .iter()
            .any(|a| a.h == 7 && a.m == 30 && a.on));
        assert!(app
            .exec_apps(&call("set_alarm", serde_json::json!({"hour": 25})))
            .unwrap()
            .starts_with("error"));
        assert!(app
            .exec_apps(&call(
                "add_note",
                serde_json::json!({"text": "Milch kaufen"})
            ))
            .unwrap()
            .starts_with("ok"));
        assert!(app
            .exec_apps(&call("list_notes", serde_json::json!({})))
            .unwrap()
            .contains("Milch"));
        assert!(app
            .exec_apps(&call(
                "add_contact",
                serde_json::json!({"name": "Lina", "number": "+49 30 1"})
            ))
            .unwrap()
            .starts_with("ok"));
        assert!(app
            .exec_apps(&call("find_contact", serde_json::json!({"name": "lin"})))
            .unwrap()
            .contains("+49 30 1"));
        assert!(app
            .exec_apps(&call("start_timer", serde_json::json!({"minutes": 5})))
            .unwrap()
            .starts_with("ok"));
        assert!(app.tl.timer_end.is_some());
        assert!(app
            .exec_apps(&call(
                "read_file",
                serde_json::json!({"path": "/data/ultar/wifi.conf"})
            ))
            .unwrap()
            .starts_with("error"));
        assert!(
            app.exec_apps(&call("open_app", serde_json::json!({"name": "notes"})))
                .unwrap()
                == "ok"
        );
        assert!(matches!(app.stack.last(), Some((Screen::Notes, _))));
        assert!(app
            .exec_apps(&call("take_photo", serde_json::json!({})))
            .unwrap()
            .starts_with("ok"));
        assert!(
            matches!(app.stack.last(), Some((Screen::Camera, _))) && app.cam.countdown.is_some()
        );
        assert!(
            app.exec_apps(&call("get_status", serde_json::json!({})))
                .is_none(),
            "bekannte System-Werkzeuge bleiben beim alten Weg"
        );
        // jede neue App zeichnet ohne Absturz, auch auf Arabisch und im Querformat
        let mut wide = Canvas::new(1280, 800);
        for lang in ["de", "ar", "en"] {
            for s in [
                "home",
                "camera",
                "gallery",
                "photo",
                "agent",
                "agentchat",
                "agentset",
                "notes",
                "contacts",
                "ubuntu",
                "phone",
                "calc",
                "clock",
                "files",
            ] {
                app.debug_open(s, lang);
                draw(&mut app, &mut cv, &mut text, &mut p);
                draw(&mut app, &mut wide, &mut text, &mut p);
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn buero_apps() {
        let _env = crate::sys::test_env_guard();
        use ultar_office as uo;
        let root = std::env::temp_dir().join(format!("ultar-buero-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ultar")).unwrap();
        std::env::set_var("ULTAR_DRY", "1");
        // phone flows: the build machine must not pick the desktop surface by its own hardware
        std::env::set_var("ULTAR_OBERFLAECHE", "handy");
        std::env::set_var("ULTAR_MEDIA", &root);
        std::env::set_var("ULTAR_DATA", root.join("ultar"));
        let p = |n: &str| root.join(n).to_string_lossy().into_owned();
        uo::create(&p("brief.docx"), "# Brief\n\nErster Absatz").unwrap();
        uo::create(&p("zahlen.xlsx"), "5,7\n").unwrap();
        std::fs::write(p("main.py"), "def f():\n    return 1\n").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_rfd, wfd) =
            nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC)
                .unwrap();
        let mut app = App::new(
            Bg {
                tx,
                wake: std::sync::Arc::new(wfd),
            },
            (720, 1520, "test".into()),
        );
        let (mut cv, mut text, mut pt) = (Canvas::new(720, 1520), Text::new(), Ptr::default());

        // --- Dokument: Absatz bearbeiten, Enter teilt, Verlassen speichert als .docx
        app.debug_open(&format!("office:{}", p("brief.docx")), "de");
        draw(&mut app, &mut cv, &mut text, &mut pt);
        app.doc_select(1);
        for c in " – geändert".chars() {
            app.on_key(Key::Char(c));
        }
        app.on_key(Key::Enter);
        for c in "Zweiter".chars() {
            app.on_key(Key::Char(c));
        }
        draw(&mut app, &mut cv, &mut text, &mut pt);
        app.home();
        let want = "Erster Absatz – geändert\nZweiter";
        let saved = || {
            uo::read_doc(&p("brief.docx"))
                .map(|d| d.to_text())
                .unwrap_or_default()
        };
        assert!(
            wait_for(|| saved().contains(want)),
            "{} – {}",
            saved(),
            state_of(&app)
        );

        // --- Tabelle: Formel in B1, Enter übernimmt und geht nach unten
        app.debug_open(&format!("office:{}", p("zahlen.xlsx")), "de");
        draw(&mut app, &mut cv, &mut text, &mut pt);
        app.office.sheet.as_mut().unwrap().sel = (0, 2);
        app.office.sheet.as_mut().unwrap().editing = true;
        app.focus = Some("office_cell");
        app.fields.insert("office_cell", "=SUM(A1:B1)*2".into());
        app.on_key(Key::Enter);
        assert_eq!(app.office.sheet.as_ref().unwrap().sel, (1, 2));
        draw(&mut app, &mut cv, &mut text, &mut pt);
        app.focus = None;
        app.home();
        let cell = || uo::sheet_get(&p("zahlen.xlsx"), "C1").unwrap_or_default();
        assert!(wait_for(|| cell() == "24 [=SUM(A1:B1)*2]"), "{}", cell());

        // --- Code: tippen mit automatischer Klammer, Suchen/Ersetzen, Speichern beim Verlassen
        app.debug_open(&format!("office:{}", p("main.py")), "de");
        draw(&mut app, &mut cv, &mut text, &mut pt);
        // Antippen im Editor setzt die Schreibmarke und öffnet die Tastatur (Fokus bleibt über das Bild hinaus)
        tap(&mut app, &mut cv, &mut text, &mut pt, 400.0, 240.0);
        draw(&mut app, &mut cv, &mut text, &mut pt);
        assert_eq!(app.focus, Some("office_ed"));
        assert!(
            app.office.code.as_ref().unwrap().tabs[0].buf.cur > 0,
            "Antippen traf den Editor nicht"
        );
        {
            let t = &mut app.office.code.as_mut().unwrap().tabs[0];
            t.buf.cur = t.buf.text.len();
        }
        for c in "print(".chars() {
            app.on_key(Key::Char(c));
        }
        for c in "f()".chars() {
            app.on_key(Key::Char(c));
        }
        app.on_key(Key::Char(')'));
        draw(&mut app, &mut cv, &mut text, &mut pt);
        assert_eq!(
            app.office.code.as_ref().unwrap().tabs[0].buf.text,
            "def f():\n    return 1\nprint(f())"
        );
        app.fields.insert("code_find", "return 1".into());
        app.office_find_enter("code_find");
        assert_eq!(
            app.office.code.as_ref().unwrap().tabs[0]
                .buf
                .selected_text(),
            "return 1"
        );
        // Agent sieht, was offen ist; Dateien außerhalb von /data bleiben gesperrt
        let call = |tool: &str, args: serde_json::Value| crate::agent::Call {
            tool: tool.into(),
            args,
        };
        let cur = app
            .exec_apps(&call("doc_current", serde_json::json!({})))
            .unwrap();
        assert!(
            cur.contains("main.py") && cur.contains("selection: return 1"),
            "{cur}"
        );
        assert!(app
            .exec_apps(&call("doc_read", serde_json::json!({"path": p("main.py")})))
            .unwrap()
            .starts_with("error"));
        assert!(app
            .exec_apps(&call(
                "doc_create",
                serde_json::json!({"path": "/data/ultar/x.docx", "content": "x"})
            ))
            .unwrap()
            .starts_with("error"));
        app.home();
        assert_eq!(
            std::fs::read_to_string(p("main.py")).unwrap(),
            "def f():\n    return 1\nprint(f())"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn system_assistent() {
        let _env = crate::sys::test_env_guard();
        let root = std::env::temp_dir().join(format!("ultar-jarvis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("ultar")).unwrap();
        std::env::set_var("ULTAR_DRY", "1");
        // phone flows: the build machine must not pick the desktop surface by its own hardware
        std::env::set_var("ULTAR_OBERFLAECHE", "handy");
        std::env::set_var("ULTAR_MEDIA", &root);
        std::env::set_var("ULTAR_DATA", root.join("ultar"));
        // ein Bild-Modell „installiert" (Projektor-Datei vorhanden)
        std::env::set_var("ULTAR_MODEL_DIR", root.join("models"));
        std::fs::write(root.join("models/mmproj-qwen2.5-vl-3b.gguf"), b"GGUF").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_rfd, wfd) =
            nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC)
                .unwrap();
        let mut app = App::new(
            Bg {
                tx,
                wake: std::sync::Arc::new(wfd),
            },
            (720, 1520, "test".into()),
        );
        app.agent.enabled = true;
        app.agent.endpoint = "127.0.0.1:9".into(); // nicht erreichbar – Antworten werden hier nachgestellt
        let (mut cv, mut text, mut p) = (Canvas::new(720, 1520), Text::new(), Ptr::default());
        // Nutzer sieht den Rechner → Assistent merkt sich Text und Bild
        app.debug_open("calc", "en");
        draw(&mut app, &mut cv, &mut text, &mut p);
        assert!(app.jv.view.is_some());
        assert!(
            app.jv.view_text.iter().any(|t| t.contains("Calculator")),
            "{:?}",
            app.jv.view_text
        );
        app.go(Screen::Agent);
        draw(&mut app, &mut cv, &mut text, &mut p);
        // der Assistent selbst wird nicht als „Bildschirm" gemerkt
        assert!(app.jv.view_text.iter().any(|t| t.contains("Calculator")));
        let reply = |app: &mut App, s: &str| app.on_msg(Msg::AgentReply(Ok(s.to_string())));
        let last = |app: &App| {
            app.history
                .last()
                .map(|m| m.to_string())
                .unwrap_or_default()
        };

        // sehen (Text)
        reply(&mut app, r#"{"tool":"see_screen","args":{}}"#);
        assert!(
            last(&app).contains("TOOL_RESULT") && last(&app).contains("Calculator"),
            "{}",
            last(&app)
        );
        // sehen (Bild) – Bildschirmfoto geht als Bild an das Modell
        reply(&mut app, r#"{"tool":"look_screen","args":{}}"#);
        assert!(crate::agent::has_image(app.history.last().unwrap()));
        // nach der Antwort ist das Bild aus dem Verlauf genommen
        reply(&mut app, r#"{"answer":"You are using the calculator."}"#);
        assert!(!app.history.iter().any(crate::agent::has_image));
        assert_eq!(app.chat.last().unwrap().1, "You are using the calculator.");

        // Root-Befehl: immer erst bestätigen; ablehnen → Modell erfährt es
        app.agent.confirm = false;
        app.steps = 0;
        reply(
            &mut app,
            r#"{"tool":"run_command","args":{"command":"id"}}"#,
        );
        assert!(app.pending.is_some(), "Root-Befehl ohne Rückfrage");
        app.deny();
        assert!(last(&app).contains("denied"));
        // Geheimnisse lesen → Rückfrage; normale Systemdateien frei
        reply(
            &mut app,
            r#"{"tool":"read_any","args":{"path":"/data/ultar/wifi.conf"}}"#,
        );
        assert!(app.pending.take().is_some());
        reply(
            &mut app,
            r#"{"tool":"read_any","args":{"path":"/proc/version"}}"#,
        );
        assert!(app.pending.is_none() && last(&app).contains("TOOL_RESULT"));
        // Sicherheit/Kamera → Rückfrage
        for t in [
            r#"{"tool":"set_privacy","args":{"group":"camera","on":false}}"#,
            r#"{"tool":"look_at_user","args":{}}"#,
            r#"{"tool":"kill_process","args":{"pid":"1"}}"#,
        ] {
            app.steps = 0;
            reply(&mut app, t);
            assert!(app.pending.take().is_some(), "{t}");
        }
        // Systemüberblick frei
        app.steps = 0;
        reply(&mut app, r#"{"tool":"system_info","args":{}}"#);
        assert!(last(&app).contains("kernel"));
        reply(&mut app, r#"{"tool":"network_connections","args":{}}"#);
        assert!(last(&app).contains("LISTENING"));
        reply(&mut app, r#"{"tool":"model_status","args":{}}"#);
        assert!(last(&app).contains("recommended chat model"));

        // Mitbringen: Foto + Textdatei → eine Nachricht mit Bild und Dateiinhalt
        let img = root.join("foto.png");
        image::save_buffer(
            &img,
            &[200u8; 3 * 4 * 4],
            4,
            4,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        let txt = root.join("notiz.txt");
        std::fs::write(&txt, "Einkaufsliste: Brot").unwrap();
        app.jv.attach = vec![
            img.to_string_lossy().into_owned(),
            txt.to_string_lossy().into_owned(),
        ];
        app.agent_busy = false;
        app.fields.insert("chat", "Was ist das?".into());
        app.send_chat();
        let m = app.history.last().unwrap().clone();
        assert!(crate::agent::has_image(&m));
        assert!(m.to_string().contains("Einkaufsliste: Brot"));
        assert!(app.chat.last().unwrap().1.contains("📎 foto.png"));
        assert!(app.jv.attach.is_empty());

        // Echte Runde gegen den Modell-Dienst (Test-Engine), wenn gebaut: Bild kommt beim Modell an
        let bin = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/release/ultar-llm"
        );
        if std::path::Path::new(bin).exists() {
            let mut child = std::process::Command::new(bin)
                .args([
                    "serve",
                    "--model",
                    "mock:",
                    "--listen",
                    "127.0.0.1:18181",
                    "--socket",
                    "",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let mut ok = false;
            for _ in 0..50 {
                if crate::agent::reachable("127.0.0.1:18181") {
                    ok = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let r = if ok {
                crate::agent::complete("127.0.0.1:18181", &[m])
            } else {
                Err("not reachable".into())
            };
            let _ = child.kill();
            assert!(
                r.as_deref().unwrap_or("").contains("mock saw 1 image"),
                "{r:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
