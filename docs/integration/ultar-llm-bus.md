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
- `init.switch` ist neu und eng: kein Verwalter-Recht, wirkt nur auf Dienste, deren Dienstdatei `switch = true`
  sagt (heute nur `llm.toml`). Jeder andere Dienst ist dafür unbekannt. „Schon an“ und „schon aus“ gelten als erledigt.
- Aus: der Modell-Dienst wird beendet, `llm.state` meldet „kein Modell“, der Helfer fällt auf ein verliehenes
  Modell oder die kleinste Hilfe zurück.
- Tests: 6 (neu: Schalter als echter Prozess); im Dienst-Verwalter: nur markierte Dienste lassen sich schalten.
