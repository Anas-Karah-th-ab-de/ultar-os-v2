# Integration: ultar-ril (RIL-Transport)

Neues Crate `system/services/ultar-ril` (eigenständig, keine gemeinsamen Dateien angefasst).

## Workspace aufnehmen (Koordinator)
In der Workspace-root `Cargo.toml` die `members`-Liste ergänzen:
```toml
members = [..., "system/services/ultar-modem", "system/services/ultar-ril"]
```
Kein `[workspace]`-Eintrag in system/services/ultar-ril/Cargo.toml (die ist bewusst sauber; zum Einzel-Bauen
wurde nur in einer Wegwerf-Kopie temporär `[workspace]` angehängt).

## Verwendung durch RIL-2 / RIL-3
Beide programmieren gegen das Trait `ultar_ril::link::ModemLink` (nicht gegen `UmtsLink` direkt),
damit sie mit `ultar_ril::link::MockLink` ohne Gerät testbar bleiben:
```rust
use ultar_ril::{ModemLink, UmtsLink, CH_FMT_0};
let mut link = UmtsLink::open()?;            // oder ein &mut dyn ModemLink
link.send(CH_FMT_0, &ipc_command_bytes)?;    // RIL-2 baut ipc_command_bytes
while let Some(frame) = link.recv(1000)? { /* ipc-Antwort parsen (RIL-2) */ }
```

## Build / Test
- Host-Unit-Tests: `cargo test -p ultar-ril` (SIPC5-Roundtrip, Padding, Node-Mapping, Mock) → 5 grün.
- Statisch aarch64: `RUSTC_BOOTSTRAP=1 RUSTFLAGS="-C target-feature=+crt-static" cargo build -p ultar-ril --target aarch64-unknown-linux-gnu --release -Z build-std=std,panic_abort`.
- Binary `ultar-ril` (Werkzeug) → /mnt/user-data/outputs/ultar-ril (Modi --probe/--send/--nodes).

## Schnittstelle (stabil halten – RIL-2/RIL-3 hängen daran)
```rust
pub struct SipcFrame { pub channel: u8, pub payload: Vec<u8> }
pub trait ModemLink {
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()>;
    fn recv(&mut self, timeout_ms: u32) -> std::io::Result<Option<SipcFrame>>;
}
```
Kanalkonstanten: `CH_FMT_0=235`, `CH_FMT_1=236`, `CH_RFS_0=245`, `CH_ROUTER=25`, `CH_PDP_0=10`.
