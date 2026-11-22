# Auftrag der Treiber-Werkstatt: Treiber „gen-hda“ für den Ultar-Kern

Datei: `kernel/ultar-kern/src/drivers/gen_hda.rs` (eigener Rust-Code, `no_std` + `alloc`, kein Linux-Code).

## Norm
High Definition Audio Specification 1.0a (Intel): 3 Register, 4.4 CORB/RIRB, 4.5 Ströme, 7 Codec-Verben

## Schnittstelle
`driver::Driver` (`name`, `origin = "werkstatt"`, `matches`, `probe`), Abfragebetrieb ohne Unterbrechungen.

## Prüfung (QEMU)
-device ich9-intel-hda -device hda-duplex,audiodev=a -audiodev wav,id=a,path=ton.wav · `ton 880 500` → WAV enthält 880 Hz

## Status
ausgeschrieben und geprüft am 22.11.2026 (siehe `docs/TREIBER.md`).
