//! ultar-telegram — nativer Telegram-Client für Ultar OS über das offizielle MTProto-Protokoll
//! (Bibliothek `grammers`, reines Rust). Kein inoffizielles Reverse Engineering: Telegram
//! stellt MTProto offen bereit; der Nutzer registriert kostenlos eine `api_id`/`api_hash`
//! auf https://my.telegram.org und meldet sich mit seiner Telefonnummer + Code an.
//!
//! Architektur: Die Oberfläche (ultar-shell, synchron/immediate-mode) spricht nur das
//! `TgBackend`-Trait. Das echte Backend (`grammers_backend`) läuft in einem eigenen Thread mit
//! einer Tokio-Laufzeit und tauscht über Kanäle Befehle/Ereignisse aus. Ein `MockTg` erlaubt
//! das Entwickeln/Testen der Oberfläche ohne Netz.

#[cfg(feature = "real")]
pub mod grammers_backend;

/// Anmeldefortschritt.
#[derive(Clone, PartialEq, Debug)]
pub enum LoginStage {
    /// Telefonnummer eingeben.
    NeedPhone,
    /// Code aus der Telegram-App/SMS eingeben.
    NeedCode,
    /// Zwei-Faktor-Passwort nötig.
    NeedPassword,
    /// Angemeldet.
    Ready,
    /// Verbindet / arbeitet.
    Busy,
    /// Fehler mit Meldung.
    Error(String),
}

/// Kurzfassung eines Chats für die Liste.
#[derive(Clone, Debug, Default)]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub last: String,
    pub unread: u32,
}

/// Eine Nachricht in der Chatansicht.
#[derive(Clone, Debug, Default)]
pub struct Msg {
    /// true = von uns gesendet (ausgehend).
    pub out: bool,
    pub text: String,
    /// Uhrzeit/Datum als kurzer Text (vom Backend formatiert).
    pub when: String,
    pub sender: String,
}

/// Schnittstelle, die die Oberfläche nutzt. Alle Methoden sind nicht-blockierend:
/// Befehle werden gesendet, Zustand wird aus dem zwischengespeicherten Stand gelesen.
pub trait TgBackend: Send {
    /// Verbindung aufbauen (api_id/api_hash vom Nutzer, Sitzung wird unter `session_path` gespeichert).
    fn connect(&mut self, api_id: i32, api_hash: &str, session_path: &str);
    /// Login-Code an die Telefonnummer anfordern.
    fn request_code(&mut self, phone: &str);
    /// Eingegebenen Code abschicken.
    fn submit_code(&mut self, code: &str);
    /// Zwei-Faktor-Passwort abschicken (falls NeedPassword).
    fn submit_password(&mut self, password: &str);
    /// Aktueller Anmeldestand.
    fn stage(&self) -> LoginStage;
    /// Chatliste (zwischengespeichert).
    fn chats(&self) -> Vec<ChatSummary>;
    /// Einen Chat öffnen (lädt Verlauf asynchron; Rückgabe = aktueller Stand).
    fn open(&mut self, chat_id: i64);
    /// Nachrichten des zuletzt geöffneten Chats.
    fn messages(&self) -> Vec<Msg>;
    /// Nachricht senden.
    fn send(&mut self, chat_id: i64, text: &str);
    /// In jedem UI-Tick aufrufen: holt neue Ereignisse (nicht blockierend).
    fn pump(&mut self);
    /// true, wenn sich seit dem letzten pump etwas geändert hat (für Neuzeichnen).
    fn dirty(&mut self) -> bool;
}

// ----------------- Mock-Backend (ohne Netz, für UI-Entwicklung) -----------------

pub struct MockTg {
    stage: LoginStage,
    chats: Vec<ChatSummary>,
    msgs: Vec<Msg>,
    open_id: i64,
    dirty: bool,
}

impl Default for MockTg {
    fn default() -> Self {
        Self { stage: LoginStage::NeedPhone, chats: Vec::new(), msgs: Vec::new(), open_id: 0, dirty: true }
    }
}

impl TgBackend for MockTg {
    fn connect(&mut self, _api_id: i32, _api_hash: &str, _session_path: &str) {
        self.stage = LoginStage::NeedPhone;
        self.dirty = true;
    }
    fn request_code(&mut self, phone: &str) {
        if phone.len() >= 5 {
            self.stage = LoginStage::NeedCode;
        } else {
            self.stage = LoginStage::Error("Nummer zu kurz".into());
        }
        self.dirty = true;
    }
    fn submit_code(&mut self, code: &str) {
        if code.len() >= 4 {
            self.stage = LoginStage::Ready;
            self.chats = vec![
                ChatSummary { id: 1, title: "Gespeicherte Nachrichten".into(), last: "Willkommen bei Ultar".into(), unread: 0 },
                ChatSummary { id: 2, title: "Mohamad".into(), last: "Test ✓".into(), unread: 2 },
            ];
        } else {
            self.stage = LoginStage::Error("Code ungültig".into());
        }
        self.dirty = true;
    }
    fn submit_password(&mut self, _password: &str) {
        self.stage = LoginStage::Ready;
        self.dirty = true;
    }
    fn stage(&self) -> LoginStage {
        self.stage.clone()
    }
    fn chats(&self) -> Vec<ChatSummary> {
        self.chats.clone()
    }
    fn open(&mut self, chat_id: i64) {
        self.open_id = chat_id;
        self.msgs = vec![
            Msg { out: false, text: "Hallo von Telegram".into(), when: "12:00".into(), sender: "Mohamad".into() },
            Msg { out: true, text: "Läuft auf Ultar OS 🎉".into(), when: "12:01".into(), sender: String::new() },
        ];
        self.dirty = true;
    }
    fn messages(&self) -> Vec<Msg> {
        self.msgs.clone()
    }
    fn send(&mut self, _chat_id: i64, text: &str) {
        if !text.is_empty() {
            self.msgs.push(Msg { out: true, text: text.to_string(), when: String::new(), sender: String::new() });
            self.dirty = true;
        }
    }
    fn pump(&mut self) {}
    fn dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }
}
