//! Nachrichten im verschlüsselten Kanal (JSON).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Aufgabe {
    /// eindeutig je Auftraggeber: dieselbe Kennung wird nie zweimal ausgeführt (Wiederholungen liefern das Ergebnis)
    pub id: String,
    pub befehl: Vec<String>,
    #[serde(default)]
    pub eingabe: String,
    #[serde(default)]
    pub zeit_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Ergebnis {
    pub id: String,
    pub code: i32,
    pub ausgabe: String,
    pub fehler: String,
    pub dauer_ms: u64,
    pub knoten: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Zustand {
    pub kennung: String,
    pub name: String,
    pub ausgabe: String,
    pub kerne: usize,
    pub frei: usize,
    pub last: f32,
    pub ram_frei_mb: u64,
    pub aktiv: bool,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum Nachricht {
    Zustand,
    ZustandAntwort(Zustand),
    /// Aufgabe rechnen (Bündeln)
    Rechnen(Aufgabe),
    /// Befehl ausführen (Steuern, nur Rolle „voll“)
    Steuern(Aufgabe),
    Ergebnis(Ergebnis),
    /// beschäftigt: später erneut versuchen
    Belegt,
    /// Lebenszeichen während einer langen Aufgabe (alle 2 s) – bleibt es aus, gilt das Gerät als weg
    Laeuft,
    Fehler {
        text: String,
    },
    /// Kopplung: HMAC(code, pk_initiator ‖ pk_antwort) und Name
    Koppeln {
        name: String,
        mac: String,
    },
    KoppelnOk {
        name: String,
        mac: String,
    },
    /// allgemeine Bestätigung
    Ok {
        text: String,
    },
    // ---- Bildschirm teilen und fernbedienen ----
    /// Bildschirm teilen beginnen (startet bei Geräten ohne Bildschirm einen virtuellen)
    Teilen,
    /// Eingaben abliefern und das nächste Bild holen (wartet bis `warte_ms` auf eine Änderung)
    Fern {
        eingaben: Vec<String>,
        warte_ms: u32,
        voll: bool,
    },
    /// Kachelpaket (base64), leer = unverändert
    Bild {
        w: u32,
        h: u32,
        nr: u64,
        daten: String,
    },
    /// einzelner Steuerbefehl an die Oberfläche: hell/drehen/bildschirm/oeffne/helfer
    Aktion {
        zeile: String,
    },
    // ---- Weitergeben ----
    /// Datei übergeben (base64); `oeffnen` = dort sofort anzeigen
    Datei {
        name: String,
        daten: String,
        oeffnen: bool,
    },
    /// „zeig meinen Bildschirm/meine App bei dir“: das Ziel öffnet die Ansicht des Absenders
    Ansehen,
    // ---- Ton ----
    TonHoeren,
    TonWeiter,
    TonDaten {
        pcm: String,
    },
    // ---- Mining ----
    /// Mining-Einstellungen setzen (None = nur Status) und ggf. einrichten
    Mining {
        konfig: Option<serde_json::Value>,
        einrichten: bool,
    },
    MiningStatus {
        status: serde_json::Value,
    },
}
