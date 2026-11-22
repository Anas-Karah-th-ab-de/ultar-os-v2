# Ultar OS – Ausgaben: Handy, Desktop/Laptop, Server

KM Code Craft · ein Kern (ultar-init, ultar-shell, ultar-compat …), drei Ausgaben.

| Ausgabe | Geräte | Oberfläche | Start |
|---|---|---|---|
| `phone` | Galaxy A30, OnePlus, Tablets | ultar-shell (Touch) | Ultar Flash |
| `desktop` | Laptops, PCs (x86_64) | ultar-shell, danach automatisch Ultar-Desktop (ultar-wm) | USB-Stick (UEFI), dann auf Platte installieren |
| `server` | Server, Mini-PCs, VMs | keine; Web-Verwaltung `ultar-admin` (HTTPS 9443) + SSH | USB-Stick/Abbild (UEFI) |

Die Ausgabe steht in `/etc/ultar/edition` (vom Bau geschrieben) und lässt sich mit dem Kernel-Parameter
`ultar.edition=desktop|server|phone` überschreiben (`crates/ultar-edition`).

## Was sich je Ausgabe unterscheidet

- **Dienste:** Jeder Dienst kann `editions = [...]` tragen. `ultar-init` lädt Dienste anderer Ausgaben gar nicht erst
  (kein Modem, RIL, GPS, Fingerabdruck auf PC/Server; keine Oberfläche auf dem Server). Das spart Speicher, Startzeit und
  Angriffsfläche. Ausnahme: ausdrücklich mit `ultar.start=<name>` verlangt.
- **Konfiguration:** `rootfs-editions/pc/` (Desktop und Server) und `rootfs-editions/<ausgabe>/` liegen beim Bau über `rootfs/`.
- **Datenpartition:** Auf dem Handy die größte eMMC-Partition, auf PC/Server **nur** die Partition `ultar-data`
  (nie eine fremde Partition einer vorhandenen Windows-/Linux-Installation).
- **Ubuntu-Bereich:** `scripts/make-compat.sh PROFILE=phone|desktop|server` – siehe unten.

| Dienst | phone | desktop | server |
|---|---|---|---|
| `ui` (ultar-shell) | ✔ | ✔ | – |
| `modem`, `ril`, `gnss`, `fingerabdruck` | ✔ | – | – |
| `coldplug` (Treiber aller Geräte laden) | – | ✔ | ✔ |
| `grow-data` (Datenpartition beim 1. Start vergrößern) | – | ✔ | ✔ |
| `sshd` (nur Schlüssel) | – | auf Wunsch | ✔ |
| `admin` (Web-Verwaltung) | – | auf Wunsch, nur 127.0.0.1 | ✔, aus dem Netz |
| `net`, `drivers`, `llm` … | ✔ | ✔ | ✔ |

## Apps aller Formate installieren – `uapp`

`crates/ultar-apps` (Programm `uapp`, im Kernsystem und im Ubuntu-Bereich unter `/usr/local/bin/uapp`).

| Format | Weg | Abschottung |
|---|---|---|
| `.uap` (KM Code Craft) | `ultar-pkg`, Signaturpflicht (Ed25519, eingebrannte Schlüssel) | – (geprüft) |
| `.deb` | `apt-get install` im Ubuntu-Bereich (Abhängigkeiten automatisch), Architektur wird geprüft | Ubuntu-Bereich |
| `.rpm` | über `alien` → `.deb` (wenn installiert) | Ubuntu-Bereich |
| `.exe`, `.msi` | Wine im Ubuntu-Bereich, **eigener Wine-Bereich je App** (`/data/ultar/wine/<id>`) | kein Laufwerk Z: (= ganzes Dateisystem), Austausch nur über D: = `/data/ultar/share` |
| `.apk` | PC: Waydroid · Handy: abgeschottete Android-Umgebung (Standard AUS) | Android-Container |
| AppImage | eigener Ordner, Start ohne FUSE (`APPIMAGE_EXTRACT_AND_RUN`) | – |
| `.flatpak`, `.flatpakref` | Flatpak (Laufzeiten von Flathub) | Flatpak-Sandbox |
| `.snap`, `.xapk` | abgelehnt mit Begründung (Snap braucht snapd/systemd; Split-APKs noch nicht) | |

```
uapp install Setup.exe            # Windows-Installer (Fenster im Desktop)
uapp install --silent Paket.msi   # MSI ohne Rückfragen
uapp install App.apk              # Android (PC: Waydroid; sonst vorgemerkt bis die Umgebung läuft)
uapp list | uapp run <id> | uapp remove <id> | uapp android-sync | uapp policy
```

- **Erkennung am Inhalt** (Magic-Bytes), nicht nur an der Endung: eine umbenannte `.exe` wird als Windows-Programm erkannt,
  ein Word-Dokument mit `.msi`-Ähnlichkeit nicht. Paketname einer APK wird aus dem binären `AndroidManifest.xml` gelesen,
  dazu die enthaltenen Prozessor-Architekturen (Hinweis, wenn eine Übersetzungsschicht nötig ist).
- **Windows-Programme:** Installer laufen in ihrem eigenen Wine-Bereich; die Startmenü-Verknüpfungen, die der Installer
  anlegt, werden übernommen. Ein Programm ohne Installer wird als eigenständiges Programm abgelegt.
- **Verzeichnis:** `/data/ultar/apps.d/<id>.toml` (Format, Prüfsumme, Startbefehl), Startmenü-Einträge
  `/data/ultar/launchers/*.desktop` – die Touch-Oberfläche (Pakete → Ubuntu-Programme) und der Ultar-Desktop zeigen sie an
  und starten über `uapp run <id>`.
- **Richtlinie** `/etc/ultar/apps-policy.toml` (Nutzer: `/data/ultar/apps-policy.toml`): fremde Formate erlauben/sperren,
  einzelne Formate sperren (z. B. `["exe", "msi"]` für Firmengeräte), Windows-Abschottung.
- **Oberfläche:** In *Dateien* eine App-Datei antippen → Installation mit Protokoll. Windows-Installer laufen im
  Fenstersystem, damit ihre Fenster sichtbar sind. Im Desktop: Doppelklick (MIME-Zuordnung `ultar-install.desktop`).
- **Web-Verwaltung:** Apps → Datei hochladen (Ziehen und Ablegen), Liste mit Entfernen.

**Getestet** (Ubuntu 24.04 x86_64, Wine 9.0, echter Durchlauf über `ultar-compat`): selbst gebauter Windows-Installer
(legt `C:\Program Files\KMDemo` und eine Startmenü-Verknüpfung an) → installiert, Verknüpfung übernommen, Start über
`uapp run` gibt „KM Demo läuft“ aus; MSI (mit wixl gebaut, Verknüpfung im Startmenü) mit `--silent` → installiert und startet;
eigenständige `.exe` → abgelegt und startet; `.deb` mit `.desktop`-Datei → installiert, gestartet, entfernt; Laufwerk Z:
fehlt im Wine-Bereich. APK-Erkennung und Paketname mit einem erzeugten APK (Unit-Test).
**Nicht getestet:** APK in echtem Waydroid (braucht Binder-Kernel und das Android-Abbild), Flatpak, AppImage mit echtem
Programm, Windows-Programme mit grafischer Oberfläche (Testumgebung ohne Bildschirm), ARM-Geräte (Windows-Programme dort
erst mit Hangover/FEX).

## Claude auf dem Desktop – Arbeiten und Entwickeln

Im Startmenü des Ultar-Desktops (und in der Touch-Oberfläche unter Pakete → Ubuntu-Programme):

- **Claude Code** – öffnet ein Terminal mit Claude Code im Arbeitsordner `/data/ultar/dev`. Beim ersten Start wird der
  offizielle Installer von Anthropic ausgeführt; Anmeldung mit dem **eigenen Claude-Abo** (Pro/Max) über `/login` im Browser.
  Claude Code ist mit dem Kern verbunden: `ultar-ctl` (Dienste, Neustart) und `uapp` (Apps installieren) stehen im
  Ubuntu-Bereich zur Verfügung; `/data/ultar/dev/CLAUDE.md` beschreibt das System für Claude.
- **Claude** – claude.ai als eigenes Fenster mit dauerhafter Anmeldung (Chats, Projekte, Artefakte wie in der Windows-App).
- **Entwickeln für alle Plattformen** (`claude-setup.sh dev`): Rust mit Zielen Windows/ARM/Android, MinGW + `wixl` (EXE/MSI),
  Test mit Wine; Android-SDK, Test mit Waydroid; QEMU für Ultar-Abbilder.

Ehrlich: Die Windows-App „Claude Desktop“ gibt es von Anthropic nicht für Linux. Ultar nutzt deshalb die offiziellen Wege
mit demselben Abo (Claude Code + claude.ai). Die Windows-App über Wine (`uapp install Claude-Setup-x64.exe`) ist möglich,
aber ungetestet und nicht empfohlen.

## Web-Verwaltung – `ultar-admin`

`crates/ultar-admin`, HTTPS auf Port 9443 (Server: aus dem Netz, Desktop: nur `127.0.0.1`, auf Wunsch).
Arabisch/Englisch/Deutsch (RTL), Ziffern immer 0–9.

- **Übersicht:** Prozessor, Speicher, Laufzeit, Kernel, Akku, Netzwerkadressen, Datenträger.
- **Dienste:** Zustand aller Dienste von ultar-init, Start/Stopp/Neustart, Protokoll ansehen.
- **Apps:** Datei hochladen und mit `uapp` installieren (alle Formate), entfernen; Ubuntu-Pakete aus den Paketquellen
  (z. B. `nginx`, `postgresql`) installieren/entfernen.
- **System:** Neustart, Ausschalten, Kennwort ändern, SSH-Schlüssel verwalten.

Sicherheit: nur HTTPS (Zertifikat wird beim ersten Start erzeugt, **SHA-256-Fingerabdruck auf der Konsole** zum Vergleichen),
Kennwort als Argon2id-Hash (`/data/ultar/admin/admin.toml`), zufälliges **Anfangskennwort** auf der Konsole und in
`/data/ultar/admin/initial-password` (wird beim Ändern gelöscht; die Oberfläche mahnt das Ändern an), Sitzungs-Cookie
`HttpOnly; Secure; SameSite=Strict`, jede Änderung braucht zusätzlich den Kopf `X-Ultar: 1` (kein Cross-Site-Angriff
über Formulare), Sperre nach 5 Fehlversuchen (wachsend ab 30 s), strenge Content-Security-Policy ohne Inline-Skripte,
nur gültige Dienst-/Paketnamen, SSH nur mit Schlüssel (Passwort-Anmeldung aus).

## PC-/Server-Abbild bauen

```bash
sudo apt install debootstrap gdisk dosfstools mtools e2fsprogs zstd cpio systemd-boot-efi   # Bau-Rechner Ubuntu 24.04
scripts/package-ultar-wm.sh                    # optional: Ultar-Desktop als .deb (wird mit eingebaut)
EDITION=desktop scripts/build-pc-image.sh      # → out/ultar-desktop-x86_64.img
EDITION=server  scripts/build-pc-image.sh      # → out/ultar-server-x86_64.img
```

Abbild auf einen USB-Stick schreiben (`dd`, Rufus, balenaEtcher) und per UEFI starten. Bootmenü (systemd-boot):
*Ultar OS*, *sichere Grafik* (`nomodeset`), *Rettung* (Konsole). Mit `ukify` (Paket `systemd-ukify`) entsteht zusätzlich ein
UKI mit dem arabischen Startbild als Startbildschirm.

**Aufbau:** GPT mit `ULTAR-EFI` (FAT32: systemd-boot, Kernel, Ultar-Startabbild), `ultar-compat` (ext4, Ubuntu 24.04 mit
Kernel-Modulen/Firmware des **gleichen** Kernels) und `ultar-data` (ext4, wächst beim ersten Start auf den freien Platz).
Treiber für Platten/USB (NVMe, AHCI, USB-Speicher, eMMC, VMD …) liegen im Startabbild; alle anderen lädt `coldplug`
nach den Gerätekennungen aus dem Ubuntu-Bereich (Grafik, Netzwerk, WLAN, Ton …).

**Auf die interne Platte installieren** (vom USB-Stick gestartet, Rettungskonsole oder SSH):
```
/usr/lib/ultar/install-to-disk.sh                       # Ziele anzeigen
/usr/lib/ultar/install-to-disk.sh /dev/nvme0n1 --yes-erase-nvme0n1
```
Kopiert Start- und Ubuntu-Partition, legt eine neue Datenpartition an. Danach USB-Stick abziehen.

## Ubuntu-Bereich je Ausgabe (`scripts/make-compat.sh`)

| PROFILE | Inhalt |
|---|---|
| `phone` | WLAN-Anmeldung, curl, kmod (wie bisher) |
| `desktop` | + Linux-Kernel, Module, Firmware, Xwayland, Weston, Mesa (auch 32 Bit), Noto-Schriften, **Wine 32/64 Bit**, **Flatpak**, **Waydroid** (aus repo.waydro.id), LXC, ultar-wm (falls gebaut) |
| `server` | + Linux-Kernel, Module, Firmware, OpenSSH, nftables, **Podman**, Werkzeuge |

## Grenzen (ehrlich)

- **Windows-Programme:** Wine deckt viele Programme ab, nicht alle (Kopierschutz, Kernel-Treiber, manche .NET-/Anti-Cheat-
  Programme laufen nicht). Auf ARM-Geräten fehlt die x86-Übersetzung noch.
- **Android-Apps auf dem PC:** Waydroid lädt beim ersten Einrichten das Android-Abbild (~800 MB, Internet nötig); Apps nur
  für ARM brauchen libndk/libhoudini in Waydroid. Google-Dienste gibt es bewusst nicht.
- **Startkette:** systemd-boot ist nicht mit Microsofts Schlüssel signiert – Secure Boot im BIOS ausschalten oder eigenen
  Schlüssel eintragen (folgt mit eigenem Signierschlüssel).
- **Gleiche Partitionsnamen:** Während USB-Stick und installierte Platte gleichzeitig stecken, gibt es `ultar-compat`/`ultar-data`
  doppelt – nach der Installation den Stick abziehen.
- **Server-Konsole:** keine lokale Anmeldung mit Kennwort; Verwaltung über die Web-Oberfläche und SSH (Schlüssel), Notfall über
  den Bootmenüeintrag *Rettung*.
