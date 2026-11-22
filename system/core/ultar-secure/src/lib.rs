//! Ultar-Sicherheit: Datenverschlüsselung (LUKS2) + verifizierter Start (dm-verity).
//!
//! Grundsatz: **keine eigene Kryptografie**. Diese Crate ruft nur geprüfte Standard-Werkzeuge
//! (cryptsetup/veritysetup) auf und kümmert sich um Reihenfolge, Zustand und Einbindung.

pub mod udata;
pub mod verity;

/// Pfade der statischen Werkzeuge im Image (rootfs/usr/lib/ultar/).
pub const CRYPTSETUP: &str = "/usr/lib/ultar/cryptsetup";
pub const VERITYSETUP: &str = "/usr/lib/ultar/veritysetup";

/// Rohe Partition, die /data trägt (A30: userdata = mmcblk0p32). Per Datei überschreibbar.
pub fn data_partition() -> String {
    std::fs::read_to_string("/data/ultar/data-part.conf")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/dev/block/mmcblk0p32".to_string())
}

/// Name des entschlüsselten Mapper-Geräts.
pub const DATA_MAP: &str = "ultar-data";
