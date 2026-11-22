//! Ultar-Verbund – Ultar-Geräte zu einem gemeinsamen Rechner im eigenen Netz verbinden.
//!
//! * **Entdecken**: Geräte melden sich per UDP-Rundruf (Port 47800) im selben Netz.
//! * **Koppeln**: per 6-stelligem Code (Gerät zu Gerät) oder automatisch über dasselbe Ultar-Konto.
//! * **Sicher**: jede Verbindung ist Noise_XX (X25519, ChaCha20-Poly1305, BLAKE2s) – auch über das Relais
//!   ultar.code-craft.tech, das nur verschlüsselte Bytes weiterreicht.
//! * **Steuern**: Befehle auf einem gekoppelten Gerät ausführen (nur mit Rolle „voll“).
//! * **Bündeln**: Aufgaben auf alle freien Kerne im Verbund verteilen, mit Wiederholung bei Ausfällen.
//! * **Ein/Aus**: der Nutzer schaltet den Verbund an oder aus (`ultar-verbund an|aus`, Einstellungen,
//!   auf dem Ultar-Kern zusätzlich `/proc/ultar/verbund`).

pub mod bildschirm;
pub mod chaos;
pub mod config;
pub mod dienst;
pub mod entdeckung;
pub mod ident;
pub mod kanal;
pub mod konto;
pub mod mining;
pub mod planer;
pub mod proto;
pub mod relay;
pub mod ressourcen;
pub mod ton;
pub mod weitergeben;
pub mod web;

pub const UDP_PORT: u16 = 47800;
pub const TCP_PORT: u16 = 47801;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Datenordner: ULTAR_VERBUND_DIR, sonst /data/ultar/verbund, sonst ~/.ultar-verbund
pub fn datenordner() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ULTAR_VERBUND_DIR") {
        return d.into();
    }
    if std::path::Path::new("/data/ultar").is_dir() {
        return "/data/ultar/verbund".into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    std::path::Path::new(&home).join(".ultar-verbund")
}

pub fn jetzt_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
