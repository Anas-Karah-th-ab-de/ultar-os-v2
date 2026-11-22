# Auftrag der Treiber-Werkstatt: Treiber „gen-smbus“ für den Ultar-Kern

Datei: `kernel/ultar-kern/src/drivers/gen_smbus.rs` (eigener Rust-Code, `no_std` + `alloc`, kein Linux-Code).

## Norm
SMBus 2.0 + Intel ICH9-Datenblatt (D31:F3): HST_STS/HST_CNT/HST_CMD/XMIT_SLVA/HST_D0, HOSTC

## Schnittstelle
`driver::Driver` (`name`, `origin = "werkstatt"`, `matches`, `probe`), Abfragebetrieb ohne Unterbrechungen.

## Prüfung (QEMU)
-machine q35 · Startmeldung „8 Gerät(e) an 0x50–0x57 antworten“, `smbus 50 2`

## Status
ausgeschrieben und geprüft am 22.11.2026 (siehe `docs/TREIBER.md`).
