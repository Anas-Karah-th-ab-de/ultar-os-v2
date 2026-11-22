//! Verifizierter Start über dm-verity (veritysetup). Konfiguration in /ultar/verity.conf:
//!   data_dev=/dev/block/mmcblk0p25
//!   hash_dev=/dev/block/mmcblk0pXX   (oder Datei)
//!   roothash=<hex>
//! Der Root-Hash ist zusätzlich ed25519-signiert (verity.sig); der öffentliche Schlüssel ist
//! ins Image gebrannt. stage0 prüft die Signatur, bevor es den Hash benutzt.

use crate::VERITYSETUP;
use std::process::Command;

pub struct VerityCfg {
    pub data_dev: String,
    pub hash_dev: String,
    pub roothash: String,
}

/// /ultar/verity.conf lesen. None = kein Verity bereitgestellt (dann Rückfall: Root ro direkt).
pub fn read_cfg(path: &str) -> Option<VerityCfg> {
    let s = std::fs::read_to_string(path).ok()?;
    let mut data_dev = None;
    let mut hash_dev = None;
    let mut roothash = None;
    for l in s.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("data_dev=") {
            data_dev = Some(v.trim().to_string());
        } else if let Some(v) = l.strip_prefix("hash_dev=") {
            hash_dev = Some(v.trim().to_string());
        } else if let Some(v) = l.strip_prefix("roothash=") {
            roothash = Some(v.trim().to_string());
        }
    }
    Some(VerityCfg {
        data_dev: data_dev?,
        hash_dev: hash_dev?,
        roothash: roothash?,
    })
}

/// dm-verity-Gerät öffnen: /dev/mapper/<name>. Schlägt fehl, wenn der Hash nicht passt.
pub fn open(cfg: &VerityCfg, name: &str) -> Result<String, String> {
    let st = Command::new(VERITYSETUP)
        .args(["open", &cfg.data_dev, name, &cfg.hash_dev, &cfg.roothash])
        .status()
        .map_err(|e| format!("{VERITYSETUP}: {e}"))?;
    if st.success() {
        Ok(format!("/dev/mapper/{name}"))
    } else {
        Err("verity open: Root-Hash passt nicht (System verändert)".into())
    }
}

/// Status des aktiven Verity-Geräts (für die Anzeige in den Einstellungen).
pub fn status(name: &str) -> Option<String> {
    let out = Command::new(VERITYSETUP).args(["status", name]).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
