//! Internet-Regeln des Geräts: das Gerät selbst offline, nur erlaubte Zellen und Systemdienste online.
//!
//! „Gerät aus“ sperrt den eigenen Verkehr des Systems (Paketquellen, Updates, Apps ohne Zelle), die
//! Weiterleitung für Zellen läuft weiter. Systemdienste auf der Erlaubt-Liste starten mit der
//! Gruppe [`NETZ_GID`] (ultar-init, nftables `meta skgid`) und dürfen hinaus – ihre Kindprozesse geben die Gruppe ab
//! (z. B. führt der Verbund Fernbefehle ohne Internet aus).

use crate::befehl;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Gruppe, deren Prozesse trotz „Gerät aus“ ins Internet dürfen
pub const NETZ_GID: u32 = 47900;
const TABELLE: &str = "ultar_geraet";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Regeln {
    /// Internet für das Gerät selbst (Systemdienste, Apps ohne Zelle)
    pub geraet: bool,
    /// Netz neuer Zellen: „frei“ oder „aus“
    pub vorgabe: String,
    /// Systemdienste, die auch bei „Gerät aus“ ins Internet dürfen (Namen wie in ultar-init)
    pub erlaubt: Vec<String>,
}

impl Default for Regeln {
    fn default() -> Self {
        Regeln {
            geraet: true,
            vorgabe: "frei".into(),
            erlaubt: Vec::new(),
        }
    }
}

fn datei() -> PathBuf {
    crate::wurzel().join("netz.toml")
}

/// Liste für ultar-init (eine Zeile je Dienst)
pub fn erlaubt_datei() -> PathBuf {
    std::env::var("ULTAR_NETZ_ERLAUBT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/ultar/netz-erlaubt"))
}

impl Regeln {
    pub fn laden() -> Regeln {
        std::fs::read_to_string(datei())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn speichern(&self) -> Result<(), String> {
        let _ = std::fs::create_dir_all(crate::wurzel());
        std::fs::write(
            datei(),
            toml::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let e = erlaubt_datei();
        if let Some(d) = e.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        std::fs::write(
            &e,
            self.erlaubt
                .iter()
                .map(|n| format!("{n}\n"))
                .collect::<String>(),
        )
        .map_err(|e| e.to_string())
    }

    /// nftables-Regeln setzen (idempotent)
    pub fn anwenden(&self) -> Result<(), String> {
        let _ = befehl("nft", &["delete", "table", "inet", TABELLE]);
        if self.geraet {
            return Ok(());
        }
        let r = format!(
            "table inet {TABELLE} {{
  chain aus {{
    type filter hook output priority 0; policy accept;
    oifname \"lo\" accept
    ct state established,related accept
    meta skgid {NETZ_GID} accept
    oifname \"zh*\" accept
    ip daddr 10.88.0.0/16 accept
    udp dport {{ 67, 68 }} accept
    meta l4proto ipv6-icmp accept
    counter drop
  }}
}}
"
        );
        crate::netz::nft(&r)
    }

    /// Darf ein Systemdienst hinaus? (für ultar-init: liest nur die einfache Liste)
    pub fn dienst_erlaubt(name: &str) -> bool {
        std::fs::read_to_string(erlaubt_datei())
            .map(|s| s.lines().any(|l| l.trim() == name))
            .unwrap_or(false)
    }
}
