# ultar-llm-bus – das Sprachmodell am Systembus

KM Code Craft · Ultar OS v2 · Stand 24.11.2026 · Bereich 4 (Systemdienste)

## Wozu

Helfer (`ultar-agentd`) und Verbund (`ultar-mesh`) sprechen nur über den Bus. Der Modell-Dienst
`ultar-llm` hat eine eigene Buchse (`/run/ultar/llm/llm.sock`, HTTP mit JSON) und entscheidet selbst,
womit er rechnet (Prozessor, iGPU, NPU). Dieser Dienst verbindet beide, ohne `ultar-llm` zu ändern.

## Vertrag (`ultar_bus::messages::llm`)

| Name | Art | Inhalt |
|---|---|---|
| `llm.state` | Thema, gehalten | `ready`, `model`, `class` (klein, mittel, groß) |
| `llm.complete` | Aufruf | `system`, `prompt`, `max_tokens` → `ok`, `text` |

## Verhalten

- Alle 10 s fragt der Dienst `/health` und `/v1/models`. Bereit heißt: der Modell-Dienst antwortet
  und ein Modell ist da. Es zählt das geladene Modell, sonst das größte vorhandene.
- Klasse nach Dateigröße: bis 1,7 GB klein, bis 5,8 GB mittel, darüber groß.
- `llm.complete` geht als `/v1/chat/completions` weiter, höchstens 110 s; Antwort höchstens 16 KiB.
- Läuft der Modell-Dienst nicht, heißt der Zustand „kein Modell“ und jeder Aufruf endet mit `ok = false`.

## Im Abbild

Benutzer 835 ohne Kernrechte, `rootfs/etc/ultar/services/llmbus.toml`, Bus-Identität `system.llmbus`
(darf `llm` anbieten und `llm.state` veröffentlichen).

## Tests

5: Größen zu Klassen, HTTP-Antwort, fehlender Modell-Dienst; als echter Prozess am Testbus gegen
einen Ersatz der Buchse (Zustand, weitergereichter Aufruf, ohne Modell-Dienst).

## Offen

Siehe `docs/OFFEN-VERBUND-HELFER.md` (Gerät in die Klasse einrechnen, Start des Modell-Dienstes, Lauf
gegen den echten Dienst).

## Nachtrag 25.11.2026: der Modell-Dienst läuft, wenn die Person ihn eingeschaltet hat

- Entscheidung des Eigentümers: Der Modell-Dienst ist aktiv, wenn die Person es aktiviert hat.
- Schalter: `assistant.enabled` (Einstellungen › Assistent). Voreinstellung jetzt **aus**.
- `ultar-llm-bus` liest die Einstellung beim Start (`settings.get`) und folgt `settings.changed`; es sagt dem
  Dienst-Verwalter `init.switch { service: "llm", on }`.
- `init.switch` ist neu und eng. Es erhöht kein Recht:
  - Die Dienstdatei nennt **die eine** System-Identität, die schalten darf (`switch = "system.llmbus"`).
    Der Dienst-Verwalter prüft den Aufrufer selbst dagegen, zusätzlich zur Bus-Regel. Für jeden anderen
    Aufrufer – auch den Verwalter – und für jeden anderen Dienst heißt die Antwort „unbekannt“.
  - Eine Dienstdatei mit `switch` wird nur angenommen, wenn der Dienst auf Anforderung startet
    (`manual = true`), als eigener Benutzer läuft und **kein** Kernrecht hat. Apps, Platzhalter und leere
    Namen sind als Schalter nicht erlaubt.
  - Der Aufruf startet oder beendet nur; er setzt den Absturz-Zähler nicht zurück (anders als `init.control`),
    und ein Dienst lässt sich höchstens alle 2 s schalten.
  - Jeder Schaltvorgang und jede Ablehnung steht mit Aufrufer im Protokoll des Dienst-Verwalters.
- Aus: der Modell-Dienst wird beendet, `llm.state` meldet „kein Modell”, der Helfer fällt auf ein verliehenes
  Modell oder die kleinste Hilfe zurück.
- Tests: 6 (neu: Schalter als echter Prozess); im Dienst-Verwalter: wer schalten darf, und welche Dienstdatei
  `switch` überhaupt haben darf.

## Nachtrag 25.11.2026 (2): die Modellgröße der Person (`assistant.model`)

- Der Dienst liest `assistant.model` beim Start (`settings.get`) und folgt `settings.changed`.
- Gewählt wird das größte installierte Modell der gewählten Klasse (klein/mittel/groß, dieselben
  Größenklassen wie `llm.state`); es bekommt das `model`-Feld jeder Weitergabe und steht in `llm.state`.
- Ist keine Modell der Klasse installiert, bleibt die eigene Wahl des Modell-Dienstes (das geladene,
  sonst das größte installierte); der Zustand meldet weiterhin, was wirklich da ist.
- `assistant.model_place` bleibt ungeknüpft: `ultar-llm` liest seine Modellordner nur beim Start aus
  seiner Konfiguration; es gibt keinen Aufruf, zur Laufzeit einen Ordner hinzuzufügen, und llm-bus
  darf diese Konfiguration nicht schreiben (siehe `docs/integration/settings-wiring.md`).
- Tests: 8 (neu: Klassenwahl, Modellgröße als echter Prozess mit Ersatz-Buchse).
