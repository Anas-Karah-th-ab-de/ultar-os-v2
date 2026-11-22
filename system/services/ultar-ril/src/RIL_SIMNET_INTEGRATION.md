# Integration RIL-2 (SIM + Netz) – für den Koordinator

Neue Dateien in `system/services/ultar-ril/src/`: `ipc.rs`, `sim.rs`, `net.rs`, `mock.rs` (nur Tests).
Kein Gerätezugriff, keine gemeinsamen Dateien angefasst, kein git-push.

## Crate zusammenführen mit RIL-1
RIL-1 liefert `link.rs` (Transport `UmtsLink`) im selben Crate. `SipcFrame` + `trait ModemLink`
sind in beiden Paketen **identisch** definiert (verlangt durch das Briefing). Beim Merge:

1. **Eine** kanonische Definition behalten. Empfehlung: `SipcFrame` + `ModemLink` in `ipc.rs`
   lassen und in `link.rs` ersetzen durch `use crate::ipc::{ModemLink, SipcFrame};`
   (oder umgekehrt – nur nicht doppelt).
2. `system/services/ultar-ril/src/lib.rs` (vom Koordinator, falls RIL-1 keine passende liefert):
   ```rust
   pub mod ipc;
   pub mod link;   // RIL-1
   pub mod sim;
   pub mod net;
   #[cfg(test)]
   mod mock;
   pub use ipc::{ModemLink, SipcFrame, Ril, IpcMessage};
   ```
3. `system/services/ultar-ril/Cargo.toml`: nur std nötig (kein Fremdcrate). Workspace-root-`Cargo.toml`
   um `"system/services/ultar-ril"` als member ergänzen (macht der Koordinator).

Verifiziert: `cargo test` (Host) 9/9 grün; `cargo build --target aarch64 -Zbuild-std` statisch grün.

## Nutzung in ultar-shell (app_phone / Netz-Einstellungen)
Sobald der CP gebootet ist (Subpaket Modem, ultar-modem --arm → ONLINE):
```rust
use ultar_ril::{Ril};
use ultar_ril::link::UmtsLink;           // RIL-1
use ultar_ril::sim; use ultar_ril::net;

let link = UmtsLink::open("/dev/umts_ipc0")?;   // RIL-1
let mut ril = Ril::new(link);
net::power_on(&mut ril, 3000)?;                  // Modem in Normalbetrieb + Attach
let sim = sim::sim_status(&mut ril, 2000)?;      // Ready/PinRequired/Absent…
let imsi = sim::imsi(&mut ril, 2000)?;
let st = net::network_state(&mut ril, 2000)?;    // reg, rat, plmn, signal
let nets = net::scan_networks(&mut ril, 15000)?; // für manuelle Netzwahl
net::select_network(&mut ril, Some(("26201", net::Rat::Umts)), 30000)?;
```
Das ersetzt das Stub-`TelephonyBackend` in `app_phone.rs`: ein echtes Backend baut auf diesen
Funktionen auf (Anruf/SMS kommen über die CALL/SMS-Gruppen – eigenes Folgepaket).
Für die vom Nutzer gewünschte **manuelle Funkturm-/Netzwahl**: `scan_networks` + `select_network(manual)`.

## WICHTIG – noch am Gerät zu bestätigen (VERIFY)
- NET-/DISP-/PWR-Kommando-IDs: net.h/disp.h/pwr.h waren beim Bau nicht abrufbar. SEC (0x05xx)
  und MISC (0x0Axx) sind aus den Headern bestätigt. NET (0x08xx)/DISP (0x07xx)/PWR (0x01xx)
  gegen libsamsung-ipc `include/net.h`,`disp.h`,`pwr.h` bzw. am Gerät prüfen (Werte in ipc.rs
  mit `// VERIFY` markiert).
- Rumpf-Layouts (Reg-Status-Feldposition, RSSI→dBm, PLMN-Listen-Eintragsgröße) sind defensiv
  geparst; mit echten CP-Antworten (riltest-Hexdump aus RIL-1) feinjustieren.
- Mehrfach-Frames: große Antworten (PLMN-Liste) müssen vom Transport (RIL-1) zu EINER FMT-
  Nachricht reassembliert werden, bevor `decode()` greift.
