# Integration RIL-3 (Mobile Daten)

Datei: `system/services/ultar-ril/src/pdp.rs` (neu). Gehört in das Crate `ultar-ril` (gemeinsam mit RIL-1
`sipc5`/Transport und RIL-2 `ipc`/`sim`/`net`).

## Zusammenführen (Koordinator)
1. In `system/services/ultar-ril/src/lib.rs` ergänzen: `pub mod pdp;`
   (lib.rs und Cargo.toml verwaltet RIL-1 — diese Datei NICHT von mehreren Agenten anlegen lassen.)
2. In `pdp.rs` das **lokale** `pub mod link { … }` + `pub use link::…` ENTFERNEN und durch den echten
   Transport ersetzen: `use crate::sipc5::{ModemLink, SipcFrame};` (bzw. finaler Pfad/Name von RIL-1).
   Die Trait-Signatur ist identisch vereinbart:
   ```rust
   pub struct SipcFrame { pub channel: u8, pub payload: Vec<u8> }
   pub trait ModemLink {
       fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()>;
       fn recv(&mut self, timeout_ms: u32) -> std::io::Result<Option<SipcFrame>>;
   }
   ```
3. Das `fmt`-Modul (FMT-Message-Encode/-Decode) in `pdp.rs` ist minimal und nur für GPRS. Wenn RIL-2
   eine allgemeine `ipc.rs` liefert (gleicher Header: len|mseq|aseq|group|index|type), `fmt` durch
   RIL-2s Encoder ersetzen und nur die GPRS-Body-Builder (`build_define_pdp`, `build_pdp_context`,
   `parse_ip_config`) behalten.

## Cargo
`ultar-ril` braucht `libc` (für die rmnet-ioctls). In der Crate-Cargo.toml:
```toml
[dependencies]
libc = "0.2"
```
Workspace-root-Cargo.toml: `system/services/ultar-ril` als member aufnehmen (macht der Koordinator zentral).

## Nutzung in der Oberfläche (ultar-shell Netz-Einstellungen)
- Umschalter „Mobile Daten": ruft `pdp::activate_data(link, cid=1, apn, user, pass, true)` /
  `pdp::deactivate_data(link, 1)`. `link` ist die offene `UmtsLink` (RIL-1), gemeinsam mit RIL-2.
- APN-Eingabefeld (+ optional Benutzer/Passwort) in den Netz-Einstellungen; Standard-cid = 1 (rmnet0).
- Statusanzeige: `pdp::list_data_ifaces()` + die aktive `DataSession` (IP/DNS).
- Datenschutz: nur aktivieren, wenn `privacy::allowed(set, Cap::Network)`.

## Reihenfolge im Stack
CP-Boot (ultar-modem) → RIL-1 Transport offen → RIL-2 SIM lesbar + Netz registriert → **RIL-3 PDP aktivieren**.
Ohne Registrierung (RIL-2) schlägt die PDP-Aktivierung mit fail_cause ab.
