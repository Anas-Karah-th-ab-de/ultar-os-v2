# Ultar – Kern-Apps (A30)

> **Stand 19.11.2026:** Kamera, Galerie, Agent, Startbildschirm, Telefon-Oberfläche, Rechner und Uhr wurden neu
> entwickelt; Notizen, Kontakte und Ubuntu-Programme kamen dazu. Aktuelle Beschreibung: [`APPS.md`](APPS.md).
> Der Text unten beschreibt den Stand davor (Telefon- und SMS-Backends gelten weiter).

Neu entwickelte, eigenständige Apps. Alle als Untermodule von `app` (wie `tools.rs`),
sauber gegen `crate::ui`/`app`/`sys`/`camera`. Integration: siehe
`crates/ultar-shell/src/APPS_INTEGRATION.md`. Mit `cargo check` verifiziert (0 Warnungen in den Modulen).

## Telefon (`app_phone.rs`)
- Wählfeld (0–9, *, #), Nummernanzeige, Löschen, Anrufen.
- Reiter **Tastenfeld** / **Verlauf** (Anrufliste mit Richtung, Rückruf).
- Anrufzustände: Wählt… / Im Gespräch / Eingehend (Annehmen/Ablehnen) / Auflegen.
- Funkteil hinter `trait TelephonyBackend` (dial/hangup/answer/state/log/ready). Aktuell `StubTelephony`
  (kein echtes Netz). Echtes Modem (crate `ultar-modem`, Shannon-CP) später als Backend einsetzen –
  Oberfläche unverändert.

## Nachrichten/SMS (`app_messages.rs`)
- Konversationsliste, Verlauf (Sprechblasen), Eingabezeile, „Neue Nachricht" (Nummer + Text).
- Hinter `trait SmsBackend` (threads/send/ready). `StubSms` speichert lokal in `/data/ultar/sms.json`.
- Später echtes SMS über das Modem-Backend.

## Kamera (`app_camera.rs`)
- Live-Vorschau über `crate::camera` (v4l2), Auslöser → PNG nach `/data/ultar/media` (in der Galerie sichtbar),
  Start/Stopp, Front/Back-Umschalter.
- Öffnet nur auf Antippen (Exynos-ISP-Knoten können beim `open()` blockieren); bevorzugt einen bekannten
  Knoten aus `/data/ultar/camera.node`. Für Dauerbetrieb Öffnen/Capture in Thread mit Timeout auslagern.

## Vorhandene Apps (geprüft, unverändert gelassen)
Rechner, Uhr (Wecker/Timer/Stoppuhr), Dateien, Galerie (zeigt MEDIA_DIRS inkl. `/data/ultar/media`),
Betrachter, Monitor, Terminal, Browser, Ton. Galerie und Dateien sind ausreichend; die Kamera speichert
bewusst in ein von der Galerie gescanntes Verzeichnis.

## Offen / für später
- Echte Telefonie/SMS sobald der RIL (SIPC5) auf dem Modem steht.
- Kamera-Dauerbetrieb mit threadbasiertem Öffnen + Timeout (gehört ins camera-Modul).
- Optional eigene Icons (Ic::Phone/Message/Camera) statt wiederverwendeter.
