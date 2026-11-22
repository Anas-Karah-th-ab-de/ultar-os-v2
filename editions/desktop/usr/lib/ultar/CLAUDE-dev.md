# Arbeitsplatz auf Ultar OS (KM Code Craft)

Du läufst in Claude Code auf **Ultar OS, Desktop-Ausgabe** – im Ubuntu-Bereich (Ubuntu 24.04) über dem Rust-Kern.

## Werkzeuge des Systems
- `ultar-ctl status | start <dienst> | stop <dienst> | restart <dienst> | reboot` – Dienste des Rust-Kerns (ultar-init)
- `uapp install <datei>` – Apps installieren (.uap, .deb, .apk, .exe, .msi, AppImage, Flatpak); `uapp list | run | remove`
- `/data/ultar/` – Einstellungen, Apps, Protokolle des Kerns liegen unter `/run/ultar/log/`
- Persönlicher Ordner: `/data/ultar/home`, Projekte: `/data/ultar/dev`

## Apps bauen und testen
| Ziel | Bauen | Testen |
|---|---|---|
| Ultar OS / Linux | `cargo build --release` (Rust), gcc/clang | direkt hier; Ultar-Abbilder mit `qemu-system-x86_64` |
| Windows | `cargo build --target x86_64-pc-windows-gnu`, `x86_64-w64-mingw32-gcc`; MSI mit `wixl` | `uapp install app.exe` bzw. `wine app.exe` |
| Android | Android SDK unter `$ANDROID_HOME` (Gradle/`sdkmanager`), Rust `aarch64-linux-android` | `uapp install app.apk` (Waydroid), `adb` |

Werkzeuge fehlen? `sh /data/ultar/claude/claude-setup.sh dev`
