# Auftrag der Treiber-Werkstatt: Treiber „gen-bochs-vga“ für den Ultar-Kern

Datei: `kernel/ultar-kern/src/drivers/gen_bochs_vga.rs` (eigener Rust-Code, `no_std` + `alloc`, kein Linux-Code).

## Norm
Bochs VBE-DISPI (QEMU docs/specs/standard-vga): Register über MMIO BAR2+0x500, Bildspeicher BAR0

## Schnittstelle
`driver::Driver` (`name`, `origin = "werkstatt"`, `matches`, `probe`), Abfragebetrieb ohne Unterbrechungen.

## Prüfung (QEMU)
-device VGA,xres=1280,yres=800,vgamem_mb=32 · `anzeige 1920x1080` → Bildschirmfoto 1920×1080

## Status
ausgeschrieben und geprüft am 22.11.2026 (siehe `docs/TREIBER.md`).
