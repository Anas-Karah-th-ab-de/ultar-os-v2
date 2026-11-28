# Servo als volle Engine des Ultar Browsers (`external/servo-host`)

KM Code Craft · Ultar OS v2 · Stand 28.11.2026 · gehört zu `docs/integration/ultar-web.md`

## Kurz

Der Browser (`apps/ultar-web`) zeigt Seiten jetzt mit **Servo** (HTML, CSS, JavaScript mit
SpiderMonkey-JIT, WebRender): ein eigener Prozess `ultar-web-engine`, den der Browser startet und über
das vorhandene Protokoll (`apps/ultar-web/src/ipc`, Version 1) steuert. Bilder kommen als geteilter
Speicher (memfd + `SCM_RIGHTS`), nur geänderte Zeilen. Servo kommt unverändert von crates.io
(`servo` 0.8.0, MPL-2.0).

**Belegt (28.11.2026, Builder 192.168.100.12, Linux, ohne Bildschirm):**
- `examples/shot` als Browser-Seite → Host → Tor-Port → Internet: `https://example.org/` in 11 s (davon
  Tor-Aufbau), `https://ar.wikipedia.org/wiki/سوريا` in 14 s – Arabisch richtig geformt und von rechts nach
  links, Bilder (SVG, PNG), Tabellen. Titel, Adresse, sichere Verbindung und Fortschritt kommen als
  Ereignisse. Host bereit nach 35–80 ms.
- `examples/window`: das **ganze Browser-Fenster** (Tabs, Adressfeld, Schutz-Knopf, Seite) über Servo, ohne
  Bildschirm gezeichnet – Arabisch dunkel und Englisch hell (`win-ar-dark.png`, `win-en-light.png`).
  Dabei gefunden und behoben: das Adressfeld zeigte `%D8%AF…` statt der arabischen Buchstaben
  (`url::readable`, Richtungszeichen bleiben maskiert).
- Mit `--media` (GStreamer): `flower.mp4` (H.264) von MDN über Tor geladen und das Videobild gezeichnet.
  Ton auf dem Builder ohne Soundkarte nicht hörbar (ALSA/PipeWire fehlen dort).

## Aufbau

| Teil | Datei | Aufgabe |
|---|---|---|
| Start | `src/main.rs`, `src/args.rs` | nur die Optionen, die `HostConfig::args` schreibt; ohne Tor-Weg startet er nicht (`--proxy` bei `--no-cell`, sonst `--proxy-socket` in der Zelle: `src/cell/`, `src/relay.rs`) |
| Verbindung | `src/link.rs` | Befehle lesen (nie blockierend), Ereignisse senden, Bildpuffer mit `SCM_RIGHTS` |
| Engine | `src/host.rs` | ein Servo, ein Software-Zeichenkontext, eine Web-Ansicht je Tab (entsteht mit der ersten Adresse); zeichnet den sichtbaren Tab erst, wenn der Browser das letzte Bild gelesen hat (`FrameDone`) |
| Rückmeldungen | `src/delegate.rs` | Adresse, Verlauf, Titel, Laden, neue Fenster (werden Browser-Tabs), Rechte (Kamera, Mikrofon, Standort, Mitteilungen → Frage im Browser), Absturz; nur `http`, `https`, `blob` und `about:blank` als Seiten |
| Bilder | `src/frames.rs` | RGBA → BGRX, nur geänderte Zeilen schreiben, Schadensband melden |
| Eingabe | `src/input.rs` | Zeiger, Rad, Tasten (evdev → DOM `code`/`key`, Zeichen wie getippt – auch Arabisch) |
| Persona | `src/persona.rs`, `src/persona.js` | siehe unten |
| Aufwecken | `src/waker.rs` | Servo weckt die Schleife über eine Pipe |

Protokoll-Erweiterung im Browser: `Command::decode` (Host-Seite) mit Rundlauf-Test; `HostConfig` hat
`proxy`; der Host bekommt nur `TZ=UTC` und `LANG=en_US.UTF-8` als Umgebung.

## Anonymität (Vorgabe 26.11.2026)

- **Netz:** Servos Proxy-Einstellung zeigt auf den neuen HTTP-`CONNECT`-Port von `ultar-tor`
  (`Tor::http_proxy_url`, mit dem Geheimnis der App). Der Port weist dieses Gerät und das lokale Netz ab und
  geht nur über Tor hinaus, ein Tor-Kreis je Website. TLS bleibt zwischen Servo und der Seite.
  HTTP wird auf HTTPS gehoben (`.onion` bleibt HTTP).
- **Persona** wie beim Lese-Modus: Tor-Browser-Kennung (Firefox/Windows), `en-US`, UTC, Bildschirm
  1400×900, 4 Kerne, Plattform `Win32`; Uhr in 100-ms-Schritten (`performance.now`); Canvas lässt sich
  nicht auslesen (leeres Bild gleicher Größe); Standort nur der erfundene Ort der Sitzung (nach Erlaubnis).
- **Abgeschaltet:** WebRTC, Bluetooth, Gamepads, WebGPU, WebXR, Service Worker, Lesen der Zwischenablage,
  Entwicklerwerkzeuge.
- **Nichts bleibt:** Cookies, Speicher, Cache nur im Speicher (`temporary_storage`).

## Wichtiger Fund beim Bau

Arti (Tor) und Servo dürfen **nicht im selben Programm** gebaut werden: Arti schaltet in `derive_more` das
Merkmal `eq` ein; WebRender bindet `derive_more` mit `#[macro_use]` ein – dann ersetzt dessen `PartialEq`
das eingebaute, und WebRender bricht ab („constant of non-structural type“). Deshalb: `ultar-web` hat das
Merkmal `tor` (Standard, nur das Programm `ultar-web` braucht es); der Host nutzt die Bibliothek ohne Tor.
Für Tests liefert `ultar-tor-port` (neues Werkzeug in `platform/ultar-tor`) die Tor-Ports als eigener Prozess.

Zweiter Fund: `ultar-tor` brach beim ersten Tor-Verbindungsaufbau ab (rustls ohne Krypto-Anbieter). Jetzt
installiert `Tor::start` Graviola bzw. RustCrypto, wenn die App keinen gesetzt hat. Das betraf auch den
Lese-Modus des Browsers.

## Zelle

Der Host baut seine Zelle selbst, bevor Servo startet (`src/cell/`, zwei Stufen wie der Starter, ohne
`unsafe`): neue Namensräume für Mounts, Prozesse, IPC und **Netz** (nur ein privates Loopback), tmpfs-Wurzel
mit dem Bündel nur lesbar, Profil und Downloads schreibbar (nie ausführbar), `/dev`-Knoten, `/proc`, `/tmp`,
`pivot_root`, keine neuen Privilegien, alle Capabilities (Ultar: alle Rechte) abgelegt, Speichergrenze als
Gruppe. Die Zelle hat **kein Netz**: Servos Proxy zeigt auf einen Loopback-Port **in** der Zelle, ein
Relais (`src/relay.rs`) kopiert auf den **Unix-Socket** `--proxy-socket` (`ultar-tor`, dritte Front,
Modus 0600, ohne Geheimnis). Direkte Verbindungen (WebSockets in Servo 0.8) finden keine Route. Jeder
Fehler beendet den Host vor Servo; auf dem Ultar-Kern (kein `CLONE_NEWNET`) verweigert er den Start mit
Zelle. Belege, Befund zum Kern und Offenes: `docs/integration/servo-cell.md`.

```text
scripts/build-servo-engine.sh --shot ADRESSE OUT.png --cell   # eine Seite durch die Zelle und Tor (root)
ULTAR_WEB_ENGINE=…/ultar-web-engine cargo test --release --test cell   # Prüfung von innen (root)
```

## Bauen und prüfen

```text
scripts/build-servo-engine.sh                      # ultar-web-engine (Release, ~25 min beim ersten Mal)
scripts/build-servo-engine.sh --shot ADRESSE OUT.png  # dazu eine Seite über Tor als Bild
scripts/build-servo-engine.sh --media              # mit Ton und Video (GStreamer)
```

Builder: clang, python3, cmake, Entwicklerpakete für fontconfig/freetype/EGL (für den Bau; das Ultar-Bündel
bringt seine eigenen Bibliotheken, siehe offen). Ziel-Ordner `TARGET` (Standard `/root/ultar-servo/target`).

## Offen (ehrlich)

1. **Zelle der Seite:** gebaut (Abschnitt „Zelle“, `docs/integration/servo-cell.md`). Offen: auf dem
   **Ultar-Kern** ist sie nicht baubar (kein Netz-Namensraum) – der Host verweigert dort den Start mit Zelle;
   die Liste, was im Kern fehlt, steht in `servo-cell.md`. Entwicklung: `ultar-web --no-cell`.
2. **Ultar-Kern:** nur unter Linux belegt. Bündel für `/usr/lib/ultar/web`: glibc, libstdc++, zlib,
   fontconfig, freetype, expat und Mesa mit EGL + llvmpipe/iris aus eigenen Paketen (`tools/ultar-paketbau`).
3. **Ton und Video:** Merkmal `media` (GStreamer) gebaut, Video-Dekodierung belegt; auf dem Ultar-Kern braucht
   es eine GStreamer-Tonausgabe nach `/dev/ultar/pcm` (Anfang im Branch `wip/servobundle`: `src/pcm.rs`,
   `src/media.rs`) und GStreamer als eigene Pakete. YouTube & Co. brauchen MSE, das Servo nicht hat.
   Medien-Dokumente starten nicht von selbst (Klick auf Abspielen nötig).
4. Auswahllisten (`<select>`), Datei-Auswahl, Dialoge (`alert`), Kontextmenüs der Seite: der Host
   beantwortet sie noch mit dem Standard; der Browser braucht dafür eigene Protokoll-Nachrichten.
5. Downloads, Zähler des Tracking-Schutzes, Zwischenablage, Schriften für CJK im Bündel.
6. Wikipedia-Symbole (CSS `mask-image`) zeichnet Servo als schwarze Flächen – Grenze von Servo 0.8.

Zu 1 (Paket `servocell`, Branch `q/servocell`): umgesetzt wie unten im Abschnitt „Zelle“; der Ultar-Kern
lehnt `unshare(CLONE_NEWNET)` mit `EINVAL` ab, der Host verweigert dort den Start mit Zelle.
