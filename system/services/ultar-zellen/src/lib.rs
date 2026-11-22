//! Ultar-Zellen – Apps und Container abgeschottet, verknüpfbar und nachvollziehbar.
//!
//! Eine **Zelle** ist eine App (nativ, Ubuntu, Windows über Wine, AppImage …) in eigenem Raum:
//! * **Speicher:** eigener Ordner, wächst dynamisch, solange Platz ist – kein Zugriff auf anderen Speicher;
//!   Arbeitsspeicher dynamisch oder fest (cgroup v2).
//! * **Netz:** frei · aus · über eine VPN-Zelle (Killswitch: ohne VPN kein Verkehr) · eigenes WireGuard.
//! * **Privatsphäre:** kein Standort (kein GNSS, keine Ortungsdienste), keine Geräteinfo (eigene Geräte-ID,
//!   neutraler Prozessor-/Rechnername, keine DMI-/Firmware-Daten), Kamera/Mikrofon nur auf Wunsch.
//! * **Protokoll:** jede Verbindung, jeder DNS-Name, gesendete/empfangene Bytes, Klartext-Uploads (HTTP) –
//!   damit sichtbar wird, wo etwas abfließt.
//! * **Verknüpfen:** gemeinsamer Ordner und gegenseitiges Aufrufen (Netz) zwischen Zellen; vor neuen
//!   Installationen schlägt das System vor, Vorhandenes zu verknüpfen statt doppelt zu installieren.
//! * **Grafikkarten:** automatisch nach Bedarf, alle gemeinsam (Rechenlast verteilt) oder fest gewählt.
//! * **Container:** Kiri (Rust, Docker-kompatibel) als schlanker Docker-Ersatz.
//!
//! Läuft auf den Linux-Ausgaben von Ultar OS (Namensräume, cgroups, nftables). Der Ultar-Kern kennt noch keine
//! Namensräume – dort laufen Apps ohne Zelle.

pub mod empfehlung;
pub mod gpu;
pub mod kiri;
pub mod netz;
pub mod profil;
pub mod regeln;
pub mod waechter;
pub mod zelle;

use std::path::PathBuf;

/// Wurzel aller Zellen: ULTAR_ZELLEN, sonst /data/ultar/zellen
pub fn wurzel() -> PathBuf {
    std::env::var("ULTAR_ZELLEN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| "/data/ultar/zellen".into())
}

pub fn jetzt() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Befehl ausführen, Fehler mit Ausgabe
pub fn befehl(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| format!("{prog}: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(format!(
            "{prog} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

/// stabiler kleiner Hash (Netz-Adressen, Schnittstellennamen, Benutzerkennungen)
pub fn kurzhash(s: &str) -> u32 {
    use sha2::Digest;
    let h = sha2::Sha256::digest(s.as_bytes());
    u32::from_le_bytes([h[0], h[1], h[2], h[3]])
}
