# Servo als volle Engine des Ultar Browsers (`external/servo-host`)

KM Code Craft · Ultar OS v2 · Stand 28.11.2026 · gehört zu `docs/integration/ultar-web.md`

## Kurz

Der Browser (`apps/ultar-web`) zeigt Seiten jetzt mit **Servo** (HTML, CSS, JavaScript mit
SpiderMonkey-JIT, WebRender): ein eigener Prozess `ultar-web-engine`, den der Browser startet und über
das vorhandene Protokoll (`apps/ultar-web/src/ipc`, Version 1) steuert. Bilder kommen als geteilter
Speicher (memfd + `SCM_RIGHTS`), nur geänderte Zeilen. Servo kommt unverändert von crates.io
(`servo` 0.8.0, MPL-2.0).

**Belegt (28.11.2026, Builder 192.168.100.12, Linux, ohne Bildschirm):** `examples/shot` als Browser-Seite
→ Host → Tor-Port → Internet: `https://example.org/` geladen und gezeichnet in 11 s (davon Tor-Aufbau),
Titel, Adresse, sichere Verbindung und Fortschritt kommen als Ereignisse; Arabisch richtig geformt und
von rechts nach links (Bild `shot1.png` im Build-Ordner). Host bereit nach 35–80 ms.

## Aufbau

| Teil | Datei | Aufgabe |
|---|---|---|
| Start | `src/main.rs`, `src/args.rs` | nur die Optionen, die `HostConfig::args` schreibt; ohne `--proxy` startet er nicht (nichts geht ohne Tor hinaus) |
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

## Bauen und prüfen

```text
scripts/build-servo-engine.sh                      # ultar-web-engine (Release, ~25 min beim ersten Mal)
scripts/build-servo-engine.sh --shot ADRESSE OUT.png  # dazu eine Seite über Tor als Bild
scripts/build-servo-engine.sh --media              # mit Ton und Video (GStreamer)
```

Builder: clang, python3, cmake, Entwicklerpakete für fontconfig/freetype/EGL (für den Bau; das Ultar-Bündel
bringt seine eigenen Bibliotheken, siehe offen). Ziel-Ordner `TARGET` (Standard `/root/ultar-servo/target`).

## Offen (ehrlich)

1. **Zelle der Seite:** Der Host baut die Zelle (`--cell-*`) noch nicht und verweigert deshalb den Start mit
   Zelle – der Browser zeigt dann den Lese-Modus. Entwicklung: `ultar-web --no-cell`. Nötig: Zelle wie der
   Starter (`system/core/ultar-launcher/src/cell`) und ein Kern-Recht „nur Loopback“, damit die Engine selbst
   nie am Tor-Port vorbei ins Netz kommt (WebSockets verbindet Servo heute direkt – in der Zelle ohne Netz
   schlagen sie dann fehl, statt am Tor vorbei zu gehen).
2. **Ultar-Kern:** nur unter Linux belegt. Bündel für `/usr/lib/ultar/web`: glibc, libstdc++, zlib,
   fontconfig, freetype, expat und Mesa mit EGL + llvmpipe/iris aus eigenen Paketen (`tools/ultar-paketbau`).
3. **Ton und Video:** Merkmal `media` (GStreamer) – Bau und Prüfung laufen; auf dem Ultar-Kern braucht es
   eine GStreamer-Tonausgabe nach `/dev/ultar/pcm`. YouTube & Co. brauchen MSE, das Servo nicht hat.
4. Auswahllisten (`<select>`), Datei-Auswahl, Dialoge (`alert`), Kontextmenüs der Seite: der Host
   beantwortet sie noch mit dem Standard; der Browser braucht dafür eigene Protokoll-Nachrichten.
5. Downloads, Zähler des Tracking-Schutzes, Zwischenablage, Schriften für CJK im Bündel.
