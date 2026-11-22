# Ultar Web – Browser mit Rust-Kern, ohne Ubuntu (Stand 20.11.2026)

Ultar Web ersetzt den bisherigen Weg „Weston + GNOME Web im Ubuntu-Bereich“. Der Browser läuft direkt im
Kernsystem: kein `/compat`, kein Wayland, kein Weston. Engine ist WPE WebKit 2.54.3, angebunden über eine
**eigene WPEPlatform-Plattform in Rust**; Oberfläche, Tabs, Planung und Sicherheit sind Rust.

## Aufbau (wie Chrome)
| Chrome | Ultar Web | Datei |
|---|---|---|
| Browser-Prozess | `ultar-web` (Rust): Leisten, Tabs, Adresszeile, Lebenszyklus, Speicherwächter | `crates/ultar-web/src/browser.rs` |
| Renderer je Website | WebKit-Seitenprozess je Tab (eigener Prozess, Prozesswechsel bei Navigation) | WebKit |
| Network Service | ein `WPENetworkProcess` (HTTP/2, Cache, DNS-Vorabauflösung) | WebKit |
| Viz/GPU-Prozess | Compositor-Thread: setzt Seite + Leisten zusammen, gibt aus (DRM/DECON/fbdev) | `compositor.rs` |
| Plattform-Schicht | GObject-Unterklassen `UltarDisplay/View/Toplevel/Screen/InputMethod` in Rust | `platform.rs`, `ffi.rs` |
| Engine-Schnittstelle | `trait WebEngine`, heute `WpeTab` (Servo kann dieselbe Schnittstelle umsetzen) | `engine.rs` |

Bilder kommen als Shared-Memory-Puffer (Skia zeichnet auf der CPU, Kacheln parallel auf **allen Kernen**:
`WEBKIT_SKIA_CPU_PAINTING_THREADS = Kernzahl`). Der Compositor kopiert das neueste Bild, gibt den Puffer sofort
zurück (WebKit zeichnet schon das nächste) und überspringt veraltete Bilder.

## Ressourcen
- **Kerne:** schnelle/sparsame Kerne werden über `cpuinfo_max_freq` erkannt (A30: 2× A73 + 6× A53).
  Compositor- und Eingabe-Thread auf den schnellen Kernen, Netzwerkprozess auf den sparsamen mit niedriger
  Priorität, Seitenprozesse auf allen Kernen (bei Energiesparen nur sparsame). `sched.rs`
- **Speicher:** WebKit-Speicherdruck-Einstellungen (Grenze je Seitenprozess = RAM/3, 256–2048 MB);
  eigener Wächter alle 10 s: unter 12 % freiem RAM wird der am längsten ungenutzte Hintergrund-Tab
  **verworfen** (Verlauf + Vorschaubild bleiben, beim Antippen wird er wiederhergestellt).
- **Tab-Lebenszyklus:** aktiv → verborgen (WebKit drosselt Timer, kein Zeichnen) → nach 5 min „eingefroren“
  (Anzeige des Zustands; Tabs mit Ton bleiben wach) → verworfen bei Speichernot. Beim Neustart lädt nur
  der aktive Tab, die übrigen bleiben verworfen, bis sie geöffnet werden.
- **Netzwerk:** DNS-Vorabauflösung schon beim Tippen in der Adresszeile; Platten-Cache und JS-Bytecode-Cache
  im Profil; Back-Forward-Cache an. Bei Akku < 15 %, > 70 °C oder mobilen Daten: kein Vorladen.
- **Video:** GStreamer mit libav/dav1d-Software-Dekodern; der v4l2-Hardware-Dekoder (Exynos MFC) wird nur
  zugeschaltet, wenn sich seine Knoten in einem Testprozess binnen 1,5 s öffnen lassen.
- **Messung:** `ultar://leistung` – Kerne, Last je Kern, Prozesse mit RAM/CPU, Bildzeiten, Tab-Zustände.

## Sicherheit und Datenschutz
- Bildschirm und Eingabe werden als root geöffnet, danach gibt `ultar-web` alle Rechte ab (Benutzer/Gruppe
  9901, `no_new_privs`); alle WebKit-Prozesse erben das. Eigener Mount-Namensraum: `/data/ultar`
  (WLAN-Kennwörter, Schlüssel, Konten) und `/compat` sind verdeckt, nur das Profil bleibt sichtbar.
  Eigene cgroup mit Speichergrenze (75 % RAM). `sandbox.rs`
- Werbe-/Trackingschutz: EasyList + EasyPrivacy werden beim Paketbau von `ultar-web-filter` in
  WebKit-Content-Blocker-Regeln übersetzt; WebKit kompiliert sie einmal und blockiert Anfragen, bevor sie
  das Netz erreichen. Dazu Intelligent Tracking Prevention, keine Drittanbieter-Cookies, TLS-Fehler = Abbruch.
- Kamera, Mikrofon, Standort nur mit Nachfrage und nur, wenn das Datenschutzzentrum (`/run/ultar/privacy`)
  es erlaubt; Drittanbieter-Speicher und DRM werden abgelehnt.
- Getrennte, dauerhafte Profile je Web-App (`/data/ultar/web/whatsapp`, `…/youtube`, …), private Tabs ohne Spuren.

## Bedienung
Adresszeile oben (Antippen = Eingabe mit Vorschlägen aus dem Verlauf), Tab-Zähler, Menü (neuer/privater Tab,
Desktop-Version, Neu laden, Leistung, Schutzstatus, Schließen). Unten: Zurück, Vor, Start, Neu laden/Stopp,
neuer Tab. Bildschirmtastatur (Latein/Arabisch/Zeichen) erscheint, sobald ein Eingabefeld der Seite den Fokus
bekommt, und verkleinert die Seite wie bei Chrome. Wisch vom Rand = zurück, Ein/Aus = zurück ins System.
Auswahllisten, Dialoge (alert/confirm/prompt) und Berechtigungen erscheinen als eigene Leisten.

## Bauen
```bash
# 1. Laufzeit (einmalig, Stunden): Buildroot 2026.08.2 + br-ext (Paket ultar-wpewebkit 2.54.3)
make BR2_EXTERNAL=<br-ext> O=<out-aarch64> ultar_aarch64_defconfig && make …
# 2. Rust-Kern + Bündel + Filter
scripts/build-ultar-web.sh <out-aarch64> aarch64     # -> out/ultar-web-aarch64.tar.zst
# 3. Gerät: Bündel nach /usr/lib/ultar/web entpacken; ultar-shell startet es automatisch für Browser und Web-Apps
```
Ohne `ULTAR_WEB_SYSROOT` baut `cargo build --workspace` nur einen Platzhalter (kein Fehler auf Rechnern ohne Bündel).
Die Buildroot-Dateien liegen unter `tools/ultar-web-br/`.

## Grenzen (ehrlich)
- Die Engine ist C++ (WebKit). Der Kern, die Plattform-Schicht, Oberfläche und Planung sind Rust.
- Keine GPU auf dem A30-Kernel: Darstellung auf der CPU. Mehrfinger-Zoom fehlt (die Eingabe kennt einen Finger).
- HTTP/3 kann libsoup nicht; Vorabrendern ganzer Seiten bietet WebKit-WPE nicht an (nur DNS-Vorabauflösung).
- Ohne Seiten-PID je Tab werden Hintergrund-Tabs über WebKits Sichtbarkeit gedrosselt, nicht per cgroup-Freezer.
- Servo-Backend und Helfer-Box sind vorbereitet (Schnittstelle), aber nicht umgesetzt.
