# ultar-pdf – der PDF-Leser

`apps/ultar-pdf` zeigt PDF-Dateien des Nutzers: fortlaufende Seiten, Zoom, Suche mit Arabisch-
Normalisierung, Text markieren und kopieren, Vorschaubilder und Gliederung. Der Leser läuft als
Compositor-Client (`UiWindow`) in einer Zelle, liest nur Dateien aus den Orten des Nutzers
(`files://<ort>/<pfad>[#page=N]`) und hat kein Netz. Das PDF wird mit hayro (reines Rust) über
`ultar-office` (`pdf.rs`) gelesen und gezeichnet.

## Öffnen

| Weg | Verhalten |
|---|---|
| Pfad-Argument | `ultar-pdf files://documents/a.pdf#page=3` (auch `$ULTAR_OPEN_URI`, vom Launcher gesetzt) |
| Pfadfeld (Strg+O) | Pfad oder URI tippen, darunter die zuletzt geöffneten Dateien (`app:data`, höchstens 12) |
| Dateien-App | Das Manifest `[open]` nennt das Schema `files`; Dateien öffnet `.pdf` damit |

Hinweis zur Standard-App: `Kind::Documents` hat im Launcher den Standard `documents`. Wer den
Leser für `.pdf` einsetzen will, wählt ihn in den Einstellungen (wie beim Editor); es gibt keine
stille Umstellung.

## Aufbau

| Modul | Aufgabe |
|---|---|
| `state`, `layout` | reine Logik: Zoom (50 bis 400 %), Einpassen, Drehen, Seitengeometrie, Lesestelle |
| `text`, `search` | Wörter einer Seite mit Rahmen, Suche seitenweise, Treffer „n von m“ |
| `doc`, `worker`, `cache` | das Dokument, sein Render-Thread und der Bildspeicher mit Byte-Grenze |
| `app`, `app_open`, `app_search` | Zustand des Fensters: Ansicht, Öffnen, Suche |
| `build`, `paint`, `raster` | Widget-Baum, Seitenbild (BGRX in `PixelArea`), Hervorhebungen |
| `input`, `keys`, `touch` | Tasten, Zeiger, Wischen und Zwei-Finger-Zoom |
| `action` | die Handlungen des Helfers (APP-RULES §9) |
| `place`, `service`, `probe` | Orte und Zuletzt-Liste, Mesh-Meldung, Zugriffe für Tests |

Das Fenster wartet nie auf den Worker. Es schickt einen Wunsch (`Want`: sichtbare Seiten zuerst,
die mittlere zuerst, dann die Nachbarn, Texte, Vorschaubilder); ein neuer Wunsch ersetzt den alten,
sodass beim schnellen Blättern nur gezeichnet wird, was noch jemand sieht. Antworten holt
`App::poll` jede Runde ab. Solange etwas aussteht, fragt die Schleife alle 30 ms, sonst jede
Sekunde. Ein beschädigtes Dokument, das im Parser in Panik gerät, wird als „beschädigt“ gemeldet,
nicht als Absturz.

### Speicher

- Seitenbilder: höchstens 40 MB (`PAGE_BUDGET`), Vorschaubilder höchstens 8 MB; Verdrängung nach
  zuletzt benutzt. Das Manifest erlaubt 120 MB für die ganze App.
- Ein Seitenbild hat höchstens 4 Megapixel (`MAX_PAGE_PIXELS`); bei stärkerem Zoom wird das Bild
  gedehnt gezeichnet (unschärfer), bis ein neuer Ausschnitt gerendert wäre. Es gibt keine Kacheln.
- Der Leser verlangt nie mehr Bilder, als der Speicher zusammen hält (`App::update_wants`):
  sonst würden Seiten gerendert, die sofort wieder verdrängt werden. Diesen Fehler (Dauerschleife
  bei 400 % Zoom, der Worker rendert endlos) hat der Test `perf.rs` gefunden und behoben.
- Seitentexte: bis 96 im Fenster, 128 im Worker.
- Dateien über 512 MB werden nicht geöffnet.

## Bedienung

| Aktion | Tasten / Geste |
|---|---|
| Zoom | Strg+Plus, Strg+Minus, Strg+0 (100 %), Strg+1 (Seitenbreite), Strg+2 (ganze Seite), Kneifen |
| Drehen | Strg+R |
| Blättern | Bild auf/ab, Leertaste (mit Umschalt zurück), Pfeile, Mausrad, Wischen; Strg+Pos1/Ende |
| Seitenfeld | Zahl tippen, Enter (auch arabisch-indische Ziffern) |
| Suche | Strg+F, Enter / F3 weiter, Umschalt+Enter zurück, Esc schließt; „n von m“ im Suchfeld |
| Markieren | Ziehen über den Text; Strg+A alles auf der Seite; Strg+C kopiert, Esc hebt auf |
| Öffnen | Strg+O |

Die Seite bleibt immer weiß, auch im dunklen Thema; nur die Oberfläche spiegelt sich in RTL
(das Panel mit Seiten und Gliederung liegt auf der Startseite, im Arabischen rechts). Das Panel
gibt es nur in der Klasse Wide (ab 1024 px), Compact und Medium haben die ganze Breite für die Seite.

## Zustände

Kein Datei, Laden, beschädigt, Passwort nötig, nicht unterstützt (Verschlüsselung, die hayro nicht
liest), leere Suche („keine Treffer“) – jeder mit Satz und nächstem Schritt in `ar`, `en`, `de`
(`locales/*.ftl`). Das Passwortfenster öffnet dieselbe Datei erneut mit dem Passwort; ein falsches
Passwort fragt noch einmal.

## Handlungen des Helfers (`[agent]`)

`open`, `go_to_page`, `search`, `copy_text` – alle frei (sie zeigen oder lesen nur, was der Nutzer
von Hand auch kann), `confirm = []`. Jede zeigt eine Meldung („Der Helfer …“). Umgesetzt in
`App::agent(AgentAction)` mit den Antworten `Done`, `NotOpen`, `OutOfRange { pages }`, `Invalid`.
Offen: Ein Bus-Vertrag, der den Helfer an diese Methoden bindet, existiert noch nicht; die Namen
stehen im Manifest und in `action::NAMES`, die Bindung folgt mit dem Agent-Dienst.

## Mesh (B4)

`service::report` meldet über `ultar-bus` (`SetActivity`) das offene Dokument und die Seite
(`files://<ort>/<pfad>#page=N`), damit „auf dem anderen Gerät weitermachen“ dieselbe Seite öffnet.
Ohne offenes Dokument (kein Datei, beschädigt, Passwort) wird nichts gemeldet.

## Änderung außerhalb der App (SCOPE-EXTRA)

`platform/ultar-surface/src/ui_app_input.rs`: neue Warteschlange `take_touch_events()`. Das
Fenster legt jede Berührung (unten, bewegt, oben, abgebrochen) zusätzlich zur Behandlung durch den
Widget-Baum ab; der Leser braucht sie für Wischen und Zwei-Finger-Zoom auf seinem eigenen Bild.
Die Warteschlange ist begrenzt (`MAX_UNHANDLED`), bestehende Apps sind nicht betroffen.
Änderung in `ultar-office/src/pdf.rs`: `OpenError::Unsupported`, `outline()`, `render_limited()`.

## Tests

| Datei | Inhalt |
|---|---|
| `src/*.rs` (Unit) | 39 Tests: Zoom, Geometrie, Cache, Suche, Berührung, Orte, Tasten, Raster |
| `tests/logic.rs` | Fixtures: Öffnen, Gliederung (arabisch), lateinische und arabische Suche (mit/ohne Tashkeel), Passwort, beschädigt, fehlend, Zoomgrenzen, Drehen, `#page=N` |
| `tests/flow.rs` | Ereignisschleife: Zoom-Tasten, Seitenfeld, Strg+F tippen/weiter/zurück/„n von m“, Ziehen-Markieren und Strg+C, Kneifen/Wischen, Mausrad, Handlungen des Helfers, Passwortfenster, Öffnen/Zuletzt |
| `tests/window.rs` | kopflos gerendert: ar/en × dunkel/hell × Compact/Medium/Wide, Seite weiß, Spiegelung nur der Oberfläche, nichts ragt heraus, alle Zustände |
| `tests/perf.rs` | 300 Seiten: Öffnen und Blättern; 500 Seiten: Speichergrenze; Mausrad/Zoom; Suche in 300 Seiten |
| `tests/fixtures/` | `mixed.pdf` (2 Seiten, Noto Naskh + DejaVu, ToUnicode, Presentation Forms, arabische Gliederung), `plain3.pdf`, `locked.pdf` (RC4-40, Passwort `ultar`), `broken.pdf`; alle unter 50 kB, erzeugt von `tests/make_fixtures.py` |

Zeitbudget (APP-RULES §8, Release-Bau): Klick/Taste in 50 ms, Start warm bis zum ersten Bild
300 ms. Gemessen mit `cargo test --release -p ultar-pdf --test perf` auf dem Entwicklungs-PC
(WSL, geteilt mit anderen Aufgaben): 300 Seiten, Öffnen bis erstes Bild 78 ms; 400 Seitenwechsel
im Mittel 9,5 ms, schlimmster 17,6 ms; Mausrad schlimmster 11,4 ms, Zoomschritt 9,3 ms;
500 Seiten: Öffnen 36 ms (alle vier Tests grün gegen die 50-ms-Grenze). Im Debug-Bau (`cargo test`) gelten wegen der ungeoptimierten Pixel-
schleifen großzügige Grenzen (5 s statt 50 ms, die Maschine ist geteilt), siehe Kopf von `perf.rs`.

## Grenzen

- Kein Kachel-Rendern: Bei sehr starkem Zoom wird das 4-Megapixel-Bild gedehnt.
- Auswahl nur innerhalb einer Seite, wortweise; keine Formularfelder, keine Anmerkungen.
- Der Zustand „nicht unterstützt“ ist als Fall der Fehlerkennung im Code und in der Oberfläche
  umgesetzt, aber ohne eigenes Fixture getestet (hayro öffnet RC4 und AES; ein nicht lesbares
  Verfahren ließ sich ohne Fremdwerkzeug nicht in unter 50 kB erzeugen).
- Pixel-genaue Referenzbilder (Snapshots) sind nicht eingecheckt; die Fenster-Tests prüfen
  Eigenschaften (weiße Seite, Spiegelung, Grenzen), keine Bilddiffs.
