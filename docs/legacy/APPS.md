# Ultar OS – Apps (Stand 19.11.2026, Neuentwicklung)

Alle Apps sind nativer Rust-Code in `crates/ultar-shell` (Untermodule von `app`), ohne VM und ohne
Fenstersystem. Sprachen: Arabisch (Rechts-nach-links), Englisch, Deutsch. Jede App passt sich an
Hochformat, Querformat und Tablets an und folgt hell/dunkel sowie der Akzentfarbe.

Geprüft wurde ohne Gerät: Bildschirmfotos (`ultar-shell --shot`) in 720×1520 und 1280×800, auf
Deutsch und Arabisch, hell und dunkel, sowie ein Ablauftest über die echte Zeichen- und Eingabeschleife
(`app::flow_tests::kamera_galerie_agent`). **Auf dem A30 selbst ist dieser Stand noch nicht gelaufen.**

## Startbildschirm
- Uhr und Datum, darunter die gelernten meistgenutzten Apps (ab drei Starts).
- Raster mit 16 Apps; jede App hat ein eigenes Symbol und einen eigenen Farbton.
- **Dock** unten: Telefon, Nachrichten, Browser, Kamera.
- Ein/Aus oben in der Ecke. „Dienste" liegt jetzt nur noch in den Einstellungen.
- **Hintergrundbild**: in der Galerie ein Foto öffnen → Symbol „Hintergrund". Liegt als
  `/data/ultar/wallpaper.jpg`; zum Entfernen die Datei löschen.

## Kamera (`app_camera.rs`)
| Funktion | Stand |
|---|---|
| Sucher im Vollbild, startet von selbst, gibt Sensor und Licht beim Verlassen und bei Bildschirm aus frei | getestet (Probebild) |
| Foto, Quadrat, Video | getestet (Probebild) |
| Selbstauslöser 3 s / 10 s, Raster (Drittel), Zoom 1× / 2× / 4× (digital) | getestet (Probebild) |
| Licht (Taschenlampen-LED), Front-/Rückkamera, Bild drehen (je Kamera gemerkt) | gebaut, braucht das Gerät |
| Vorschau der letzten Aufnahme → öffnet den Betrachter | getestet |

- Fotos: JPEG (Qualität 93) nach `/data/DCIM/Camera/IMG_JJJJMMTT_HHMMSS.jpg`; gespeichert wird genau
  der sichtbare Ausschnitt (Modus, Zoom, Spiegelung der Frontkamera).
- Video: Motion-JPEG im AVI-Behälter (`mjpeg.rs`), `VID_….avi`. Spielt in VLC, mpv, ffmpeg, Windows.
  Gegenprobe mit `ffprobe`: Codec mjpeg, Bildzahl und Bildrate stimmen. **Kein Ton** (Mikrofonweg fehlt noch).
- Speichern und Videokodierung laufen in eigenen Threads; die Oberfläche bleibt bedienbar.
- Auflösung ist die des Kameratreibers (derzeit 640×480 angefordert) – höhere Auflösung, Autofokus,
  Belichtung und HDR hängen am ISP des Geräts (`camera.rs`), nicht an der App.
- Frontkamera: Der Geräteknoten gehört in `/data/ultar/camera-front.node`. Ohne ihn zeigt „Wechseln"
  weiter die Rückkamera (Hinweis erscheint).
- Sensor steht auf dem Kopf oder quer? Oben rechts „Drehen" antippen – wird je Kamera in
  `/data/ultar/camera.toml` gemerkt.

## Galerie (`app_gallery.rs`)
- Reiter **Fotos** (nach Datum: Heute, Gestern, Monat), **Alben** (Ordner), **Favoriten**, **Videos**.
- Quellen: `/data/DCIM`, `/data/Pictures`, `/data/media`, `/data/Download`, `/data/ultar/media`
  (PNG, JPEG, AVI/Motion-JPEG).
- Vorschaubilder entstehen in einem Hintergrund-Thread und liegen danach unter
  `/data/ultar/cache/thumbs` – beim nächsten Öffnen sind sie sofort da.
- Langer Druck = Mehrfachauswahl, dann Löschen (zweites Tippen bestätigt).
- **Betrachter** (Vollbild): wischen = blättern, nach unten wischen = schließen, tippen = Leisten
  ein/aus, doppelt tippen = vergrößern und verschieben. Aktionen: Favorit, Drehen, als Hintergrund
  setzen, Details (Name, Ordner, Größe, Auflösung, Dauer), Löschen.
- Videos: Abspielen/Pause, Fortschritt, Zeit.
- Die Dateien-App öffnet Bilder und Videos im selben Betrachter.
- Offen: Zwei-Finger-Zoom (die Eingabe kennt bisher einen Finger), Bearbeiten (Zuschneiden, Filter),
  Teilen.

## Agent (`app_agent.rs`, `agent.rs`)
- Gespräch mit Sprechblasen, Werkzeugschritte als kleine Zeilen, Vorschläge beim leeren Gespräch,
  „Neues Gespräch", Zustand (bereit / nicht erreichbar) im Kopf.
- **Lokales Sprachmodell**: llama.cpp (`llama-server`) im Ubuntu-Bereich, nur auf `127.0.0.1:8080`
  erreichbar – nichts verlässt das Gerät. Alternativ Claude über die Anthropic-API (wie bisher).
- **Einrichtung**: Einstellungen → Lokaler Agent → Modell antippen. Das Skript
  `rootfs/usr/lib/ultar/agent-install.sh` nimmt den ersten Weg, der klappt:
  1. offline: Ordner `ultar-agent/` auf einem USB-Stick (`llama-server` oder `llama-*.tar.gz`, dazu `*.gguf`),
  2. fertiges llama.cpp aus den Veröffentlichungen von `ggml-org/llama.cpp` (`…-bin-ubuntu-arm64.tar.gz`),
  3. llama.cpp auf dem Gerät bauen (`-DGGML_NATIVE=OFF`), falls das fertige Programm auf dem
     Prozessor nicht läuft – das wird mit `llama-server --version` geprüft.
- Modelle (GGUF, Q4_K_M): Qwen3 0.6B (ab 2 GB RAM), **Qwen3 1.7B** (ab 3 GB, für das A30 empfohlen),
  Qwen3 4B (ab 6 GB). Empfehlung nach Arbeitsspeicher. Adressen in `agent.rs` (`MODELS`),
  überschreibbar in `/data/ultar/agent/models.conf` (`kennung=adresse`).
- Dienst `llm` läuft jetzt mit `compat = true` (Ubuntu-Laufzeit), höchstens 4 Kerne.
- **Neue Werkzeuge** – der Agent bedient die Apps: `open_app`, `open_url`, `take_photo`, `set_alarm`,
  `start_timer`, `toggle` (WLAN, Bluetooth, mobile Daten, Flugmodus, Taschenlampe, VPN), `add_note`,
  `list_notes`, `add_contact`, `find_contact`, `call`, `list_files`, `read_file`, `notify`; dazu wie
  bisher Einstellungen, Dienste, Ubuntu-Pakete (apt), Befehle, Neustart.
- Sicherheit: `read_file`/`list_files` nur unter `/data`, **nicht** unter `/data/ultar` (Schlüssel,
  WLAN-Kennwörter, Konten). Anrufe, Befehle, Pakete, Ausschalten fragen vorher nach.
- **Nicht getestet**: ein echtes Modell auf dem A30 (Geschwindigkeit, Speicher, Qualität der
  Werkzeugaufrufe), der Download-Weg (die Testumgebung erreicht GitHub-Releases/Hugging Face nicht).
  Die Adressen für 0.6B und 4B folgen dem Namensmuster des geprüften 1.7B-Eintrags.

## Notizen und Kontakte (`app_extra.rs`)
- Notizen: Karten im Raster, anheften, fortlaufendes Speichern (kein Speichern-Knopf), Löschen.
  Daten: `/data/ultar/notes.toml`.
- Kontakte: Liste mit Anfangsbuchstabe, direkt anrufen, anlegen/bearbeiten/löschen.
  Daten: `/data/ultar/contacts.toml`. Das Telefon zeigt den Namen zur getippten Nummer.

## Telefon, Rechner, Uhr
- Telefon: runde Tasten mit Buchstaben, grüner Anrufknopf, langer Druck auf 0 = „+", Kontakte-Knopf.
- Rechner: Rechenzeichen im Akzentton, „=" gefüllt, „C" rot, Löschtaste als Symbol.
- Uhr: Wecker mit Papierkorb neben dem Schalter.

## Ubuntu-Programme (`app_extra.rs`, `rootfs/usr/lib/ultar/app-session.sh`)
- Einstellungen → Ubuntu-Programme: listet grafische Programme aus installierten Ubuntu-Paketen
  (`/compat/usr/share/applications/*.desktop`) und startet sie im Vollbild im selben Fenstersystem
  (Weston) wie den Browser. Zurück mit der Ein/Aus-Taste.
- Voraussetzung wie beim Browser: Weston im Ubuntu-Bereich und ein Bildschirm über DRM/KMS.
  **Auf dem A30 (Exynos-DECON ohne DRM) läuft das Fenstersystem noch nicht** – dort also weder Browser
  noch Ubuntu-Programme mit Fenster. Kommandozeilen-Pakete und der Agent sind davon nicht betroffen.
- Nicht getestet (braucht ein Abbild mit Ubuntu-Bereich und Weston in QEMU).

## Gestaltung
- 25 neue Symbole in `ui.rs` (Kamera, Telefon, Nachricht, Abspielen, Notiz, Person, Senden, Herz,
  Papierkorb, Funke, …), runder Symbol-Knopf (`icon_btn`), Pille (`pill`).
- `gfx.rs`: `blit_xrgb` (Kamerabild mit Ausschnitt und Spiegelung ohne Zwischenkopie),
  `round_corners`, `veil`.
- Kamera und Betrachter sind immer dunkel (Vollbild auf Schwarz), unabhängig vom Farbschema.
- Bewegte Inhalte (Kamerabild, Video, Selbstauslöser) zeichnet die Hauptschleife mit rund 30 Bildern/s
  (`App::animating`), sonst schläft die Oberfläche wie bisher.

## Prüfen
```bash
cargo test -p ultar-shell                       # 13 Tests, darunter der Ablauftest
ULTAR_DATA=/tmp/ud target/debug/ultar-shell --shot 720 1520 camera de /tmp/kamera.bmp
#   Bildschirme: home camera gallery photo agent agentchat agentset notes contacts ubuntu phone calc clock …
ULTAR_MEDIA=/tmp/medien …                       # Nutzerbereich umlenken (Standard /data)
```

## Offen
- Auf dem A30 testen: Kamera-Knoten und Drehung, Taschenlampe, Geschwindigkeit des Suchers.
- Ton im Video, höhere Kamera-Auflösung, Zwei-Finger-Zoom.
- Kalender, Musik-/Videoplayer für andere Formate, Sprachaufnahme, Wetter, Sperrbildschirm mit PIN.
