# Apps aus anderen Systemen – Linux, Windows, Android

KM Code Craft · Ultar OS v2 · Plan vom 28.11.2026 · Auftrag des Eigentümers: „einfache Installation von Android-,
Windows- und Linux-Apps, damit der Nutzer seine alten Apps weiter verwenden kann – **die Sicherheit muss bestehen
bleiben**.“

## 1. Ziel für die Person

* Doppelklick auf `.deb`, `.exe`, `.msi` oder `.apk` in „Dateien“ (oder „Installieren“ im Store) → ein Systemdialog
  zeigt, was es ist, woher es kommt und was es darf → „Installieren“ → die App steht im App-Raster mit einem kleinen
  Herkunftszeichen (Linux · Windows · Android) und startet wie jede andere App.
* Entfernen über das App-Raster oder den Store – spurlos (Zelle, Daten, Einträge).
* Die Person sieht jederzeit unter **Einstellungen › Apps › Apps aus anderen Systemen**: Linux-Bereich (Größe,
  aktualisieren, zurücksetzen), jede fremde App mit ihren Rechten (Netz, Ordner) zum Ändern.

## 2. Sicherheit zuerst (gilt vor jeder Funktion)

Fremde Programme sind **nicht** von KM Code Craft geprüft. Deshalb bekommen sie das engste Profil `app.foreign`:

| Regel | Umsetzung |
|---|---|
| eigene Identität | eigene uid/gid aus 20000–49999 (Launcher), nie root, `no_new_privs`, alle Kern-Rechte weg |
| eigene Sicht | Mount- + PID-Namensraum; Laufzeit (Linux-Bereich) **nur lesend**, `nosuid`, `nodev`; eigenes `/home` (rw) |
| keine Systemdienste | **kein** Bus-Socket, keine Hilfsdienste, kein `/run` des Hosts; nur der eigene Wayland-Socket der Zelle |
| Netz | Standard **aus** (eigener Netz-Namensraum: nur eigenes `lo`). Mit Erlaubnis: **nur Internet** – kein Loopback des Hosts, keine Adressen des Geräts, kein LAN (außer ausdrücklich erlaubt) |
| Dateien | nur eigene Daten; Ordner der Person (Dokumente, Downloads …) nur, wenn sie sie im Dialog freigibt |
| Geräte | nur `null zero full random urandom tty`; kein `fb`, `event`, `dri`, Kamera, Mikrofon (später je Recht über Dienste) |
| Grenzen | Speicher- und Prozessgrenze je App (Speichergruppe), Ende der Zelle beendet alle ihre Prozesse |
| Installation | Datei der Person = **ungeprüft** → Warnung + Bestätigung mit Anmeldung (Systemdialog). Pakete aus den Ubuntu-Quellen prüft `apt` über deren Signaturen. Der Linux-Bereich wird **nur** in einer Installationszelle beschrieben, nie von einer App |
| Windows | eigenes Wine-Präfix je App; Laufwerk `Z:` (ganzes Dateisystem) entfernt; nur `C:` (eigene Daten) und freigegebene Ordner |
| Android | eigene Laufzeit je App, keine Google-Dienste, keine Geräte-IDs (neutral) |

Ohne diese Kern-Voraussetzungen startet **keine** fremde App (der Launcher verweigert, wie heute bei fehlenden
Plattform-Funktionen) – es gibt keinen „unsicheren Modus“.

## 3. Aufbau

```
 Dateien / Store / Einstellungen  ──bus──▶  ultar-foreign (Dienst, eigener Benutzer)
                                               │ prüft Datei, zeigt Dialog, baut Zelle, schreibt Launcher-Eintrag
                                               ▼
                                  Installationszelle (rw Linux-Bereich, Netz nur zu Paketquellen)
 App-Raster ──▶ ultar-launcher ──▶ App-Zelle (profil app.foreign)
                                    ├─ /            Linux-Bereich (nur lesend)
                                    ├─ /home/app    eigene Daten (rw)
                                    ├─ ultar-wayland  (Brücke, gleiche uid) ──eigenes Protokoll──▶ ultar-compositor
                                    └─ Programm (Linux) │ wine (Windows) │ ATL (Android)
```

* **Linux-Bereich**: *ein* gemeinsamer Ubuntu-26.04-Grundstock (glibc, apt/dpkg, GTK 3/4, Qt 6 Wayland, Mesa
  llvmpipe, Schriften inkl. Noto Arabic), als signierte Komponente „Linux-Programme“ ausgeliefert. Apps teilen den
  Grundstock lesend; jede App läuft in ihrer eigenen Zelle.
* **ultar-wayland**: Wayland-Server in Rust (reines Rust, ohne libwayland), *eine Instanz je App-Zelle* mit den
  Rechten der App; übersetzt `xdg_toplevel`/`wl_shm`/Eingabe/Zwischenablage auf das eigene Compositor-Protokoll.
  So sieht der Compositor jede fremde App als eigenen Client mit eigener uid.
* **Windows**: Wine (Wayland-Treiber) im Linux-Bereich; je App ein Präfix in ihren Daten; `.exe`/`.msi` läuft
  einmal als Installer in der App-Zelle, danach werden die Startmenü-Verknüpfungen zu App-Einträgen.
* **Android**: zuerst *Android Translation Layer* (ATL, läuft als Linux-Programm über Wayland, ohne Binder) – deckt
  einfache Apps ab. Volle Android-Laufzeit (Binder im Ultar-Kern) ist eine spätere Stufe.

## 4. Stufen

| Stufe | Inhalt | Prüfung (muss grün sein) |
|---|---|---|
| **P0 Kern** | Netz-Namensraum (eigenes `lo`, kein Host), Netz-Schranke „nur Internet“ (vererbbar, nicht rücknehmbar), `PR_SET_PDEATHSIG`, `nosuid` beachten, eingeschränkte Gerätesicht für Zellen | Angriffstests in QEMU: Zelle erreicht weder `127.0.0.1` des Hosts noch Geräte-Adressen noch Host-Sockets; Waise stirbt mit dem Launcher |
| **P1 Brücke** | `platform/ultar-wayland` | GTK4-, Qt6-, SDL- und `weston-simple-shm`-Fenster erscheinen, Tastatur (ar/en/de), Maus, Zwischenablage, Größe ändern, schließen; Fuzz der Anfragen |
| **P2 Linux** | Grundstock-Bau (`tools/foreign/`), Dienst `ultar-foreign` + Bus-Vertrag `messages/foreign.rs`, Launcher-Art `compat-deb`, Einträge aus `.desktop` | `.deb` (z. B. ein GTK-Editor) installieren → startet im Raster → entfernen; ohne Netz-Recht kein Netz |
| **P3 Oberfläche** | Dateien: Öffnen von `.deb/.exe/.msi/.apk` → Installationsdialog; Store-Quellen; Einstellungen-Seite; Herkunftszeichen | Klicktests ar/en/de, hell/dunkel |
| **P4 Windows** | Wine im Grundstock, Präfix je App, `.exe`/`.msi`, Verknüpfungen, `Z:` entfernt | ein echtes Windows-Programm installieren und starten |
| **P5 Android** | ATL bauen und im Grundstock ausliefern, `.apk` installieren | eine einfache `.apk` startet |
| **P6 Ausbau** | Ton (Brücke zu `ultar-audiod`), Xwayland für reine X11-Programme, GPU, Benachrichtigungen, Drucken | – |
| **P7 Webseite** | ehrlicher Stand auf ultar.code-craft.tech | – |

Reihenfolge: P0, P1, P2 parallel; P3–P5 danach. Jede Stufe: eigener Branch, Prüfung auf den Servern
(.8/.12/.18), dann `main`. Nichts davon wird als „fertig“ gemeldet, was nicht in QEMU auf dem Ultar-Kern lief.

## 5. Offene Entscheidungen

* LAN-Zugriff für fremde Apps mit Netz-Recht (Drucker, NAS): Vorschlag **aus**, eigener Schalter je App.
* Flatpak (Flathub) als zusätzliche Linux-Quelle: braucht Benutzer-Namensräume für `bwrap` – später prüfen.
