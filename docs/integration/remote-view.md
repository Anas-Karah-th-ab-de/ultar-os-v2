# Fernansicht (Verbund-Punkt C1)

Den Bildschirm eines anderen eigenen Geräts sehen und bedienen. Das angesehene Gerät (die
**Quelle**) entscheidet: Nur ein gekoppeltes Gerät mit dem Recht „Dieses Gerät steuern“
(`Right::Control`, nach dem Koppeln aus) darf fragen, und die Person an der Quelle sagt
vorher im Systemdialog Ja. Ohne dieses Ja wird nichts aufgenommen und nichts übertragen.

## Teile

| Teil | Ort | Aufgabe |
|---|---|---|
| Bildschirmdienst | `system/services/ultar-screend` | liest den Bildschirm vom Compositor, schickt nur geänderte Rechtecke, spielt Zeiger und Tasten des Betrachters ein |
| Verbund | `system/services/ultar-mesh` (`view.rs`, `viewbus.rs`, `codec.rs`) | Sitzung, Rahmen, Flusssteuerung, Rechte, Journal |
| Vertrag | `system/core/ultar-bus/src/messages/mesh_view.rs` | Aufrufe, Themen, Eingabewörter |
| Betrachter | `apps/ultar-remote` | Fenster: Geräteauswahl, Bild, Werkzeugleiste, Zustände |

`ultar-screend` läuft als eigener Benutzer (843) mit genau zwei Rechten: `ui.capture` beim
Compositor (`compositor-rights.toml`) und dem Kernrecht `input_inject` für virtuelle
Eingabegeräte. Er startet nie von selbst, sondern erst, wenn der Verbund eine Sitzung meldet.

## Ablauf

1. Betrachter ruft `mesh.view_start` (Gerät). Der Verbund sendet `ViewStart`.
2. Die Quelle prüft: gekoppelt, `Control` erlaubt, keine andere Sitzung. Sonst `ViewRefuse`.
3. Der Verbund der Quelle fragt die Person (`sysdialog.permission`, Bildschirm teilen,
   Name des Betrachters als Datum). Nach 8 s ohne Antwort oder bei „Nein“: `ViewRefuse`.
4. Bei Ja veröffentlicht er `mesh.view_serve` (`Begin`). `ultar-screend` nimmt den
   Bildschirm auf, ruft `mesh.view_ready` (Größe) und zeigt eine Benachrichtigung mit
   Knopf „Beenden“. Der Verbund sendet `ViewAccept`.
5. Bildstücke: `mesh.view_picture` → `ViewFrame` (Rechteck, QOI-artig verlustfrei kodiert).
   Der Betrachter bestätigt mit `ViewAck`.
6. Eingabe: `mesh.view_input` → `ViewInput` → `ServeEvent::Input` → virtuelle Geräte.
7. Ende: `ViewStop` von jeder Seite, Sperrbildschirm (`locked`), Verbindungsverlust,
   Entzug des Rechts `Control` oder Entkoppeln.

## Bild

Der Differ teilt den Bildschirm in 64-Pixel-Kacheln und fasst bis zu vier nebeneinander
liegende zu einem Rechteck zusammen (höchstens 64×256 Pixel, bis `MAX_PIXELS` = 128×128
Pixel je Rahmen nach der Zerlegung durch den Verbund). Eine kachel gilt erst als bekannt,
wenn der Verbund das Rechteck **angenommen** hat; ein verworfenes Rechteck wird daher von
selbst wiederholt. Das Tempo ist höchstens 30 Bilder/s, im Leerlauf bis 250 ms, bei
langsamer Verbindung bis 500 ms (`pace.rs`).

## Flusssteuerung

Höchstens `WINDOW = 4` Rahmen sind unbestätigt unterwegs. Ist das Fenster voll, nimmt der
Verbund das Rechteck nicht an (`PictureTaken { taken: false }`); `ultar-screend` puffert
nichts und versucht es beim nächsten Blick erneut, mit größerem Abstand. Der Speicher
bleibt damit unabhängig von der Verbindung begrenzt. Der Betrachter prüft jedes Stück
gegen die angenommene Bildgröße; der Dekoder lehnt Köpfe über `MAX_PIXELS` ab.

## Sicherheit

* Rechte: `Control` für Rahmen und Eingabe, geprüft bei jedem Rahmen, nicht nur beim Start.
* Nur `system.screend` darf `view_ready`, `view_picture`, `view_locked` rufen; Stopp darf
  die Shell oder `system.screend` (Knopf der Benachrichtigung). Regeln in `bus-policy.toml`.
* **Sperrbildschirm:** Eingabe wird erst eingespielt, nachdem das erste Stück einer frischen
  Aufnahme ankam. Liefert der Compositor nichts (Sperre: kein `ui.capture`-Bild), gilt das
  nach 2 s als gesperrt: Sitzung endet mit `locked`, nichts wird getippt.
* Beim Ende lässt der Einspieler gehaltene Tasten und Tasten los.
* Journal: `mesh.view_ask`, `mesh.view_incoming`, `mesh.view_serving`,
  `mesh.view_end_viewer`, `mesh.view_end_source`, `screend.begin`, `screend.end`.
* Bildschirminhalt und Eingabe laufen nur durch den verschlüsselten Verbundkanal.

## Betrachter (`ultar-remote`)

Geräteauswahl aus `mesh.state` (gekoppelt und erreichbar), Warten auf das Ja (90 s),
Bild eingepasst mit Rändern (weich oder scharf), Zeiger, Rad und Tastatur zum anderen
Gerät, Werkzeugleiste: Beenden, Vollbild (F11 ist lokal), „Tasten senden“ (Strg+Alt+Entf
u. a.), Bildqualität. Zustände: Auswahl, Warten, Läuft, Abgelehnt, Beendet, Gesperrt,
Verbindung verloren, jeweils mit „Nochmal versuchen“. Arabisch zuerst, dazu en und de.

Zeigerbewegungen werden pro Blick zu einer zusammengefasst; Tasten laufen als
Evdev-Codes für ein US-Layout (nur ASCII; arabische Buchstaben haben dort keinen Code und
werden verworfen).

## Handoff

Für Geräte ohne Anzeige ist die Stufe `RemoteView` in den Übergabe-Angeboten freigeschaltet.

## Tests

* `ultar-mesh`: Codec (Rundlauf, Größe, feindlicher Kopf), Sitzungszustand, Rechte,
  Entzug, Verbindungsverlust (`tests/view.rs`).
* `ultar-screend`: Differ, Takt, Einspieler, Engine (Sperre, Wiederholung, Reihenfolge),
  zwei Verbundknoten Ende-zu-Ende (`tests/e2e.rs`), echter Compositor (`tests/compositor.rs`).
* `ultar-remote`: Tasten, Skalierung und Zeigerabbildung, Fenster in allen Zuständen,
  ar/en × dunkel/hell, Katalog-Gleichheit, Klicks.
* Gebaut und getestet auf dem Testrechner 192.168.100.18; `ultar-check all` ohne neue
  Verstöße.

## Offen

* Der Bildschirm wird je Blick vollständig aufgenommen (der Compositor liefert noch keine
  Schadensliste); der Differ spart nur die Übertragung.
* Nur der ganze Bildschirm, keine einzelnen Fenster.
* Das Ja nutzt den vorhandenen Dialog „Bildschirm teilen“ ohne eigenen Text.
* Die Shell liest `mesh.view_state`, zeichnet aber noch keinen farbigen Rahmen; bis dahin
  zeigt die Benachrichtigung mit „Beenden“-Knopf, wer zusieht.
* Ändert sich die Bildschirmgröße, endet die Sitzung.
* Sperre wird über das Ausbleiben der Aufnahme erkannt, nicht über `session.state`.
* Der Prozess-Test mit echtem Bus, Compositor, Verbund und Dienst auf zwei Geräten fehlt;
  Eingabe in `/dev/uinput` ist nur mit Aufzeichnern getestet.
* Die App ist noch nicht in den Starterrastern, den VM-Abbildern und dem Test
  `app_manifests.rs` eingetragen.
