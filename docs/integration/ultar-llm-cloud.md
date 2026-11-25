# ultar-llm-cloud – das Cloud-Modell als letzter Weg

KM Code Craft · Ultar OS v2 · Stand 25.11.2026 · Bereich 4 (Systemdienste)

## Entscheidung des Eigentümers

Ein Modell im Internet nur als **letzter Weg**, wenn kein lokales Modell möglich ist. Die Person gibt ihren
eigenen API-Schlüssel ein. Anbieter vorerst als Beispiel: Claude.

## Reihenfolge im Helfer (`ultar-agentd/src/brain.rs`)

1. Modell dieses Geräts (wenn der Assistent eingeschaltet ist und ein Modell da ist),
2. Modell eines gekoppelten Geräts, das es verleiht (auch ein besseres als das eigene),
3. Cloud-Modell – nur wenn 1 und 2 nicht gehen **und** ein Schlüssel gespeichert ist,
4. kleinste Hilfe (feste Befehle, Hilfe-Themen).

Ein noch so kleines Modell auf den eigenen Geräten geht vor. Antwortet die Cloud nicht (kein Netz, Schlüssel
abgelehnt), bleibt die kleinste Hilfe.

## Was das Gerät verlässt

Die Anfrage in den Worten der Person und die Liste der Werkzeuge des Helfers (mit Zwischenergebnissen der
Schritte). Keine Dateien, kein Bildschirm. Ohne Schlüssel wird nichts gesendet. Die Meldung der Hülle sagt im
Titel, wenn mit dem Cloud-Modell gedacht wurde.

## Vertrag (`ultar_bus::messages::llm_cloud`)

| Name | Art | Wer darf |
|---|---|---|
| `llmcloud.state` | Thema, gehalten: `configured`, `provider`, `model` | Helfer, Einstellungen |
| `llmcloud.set_key`, `llmcloud.clear_key` | Aufruf | nur die Einstellungen-App |
| `llmcloud.complete` | Aufruf | nur der Helfer |

Kein Aufruf gibt den Schlüssel heraus. `SetKey` druckt sich nie aus (kein Schlüssel im Protokoll).

## Schlüssel

Datei `key` im eigenen Ordner des Dienstes (`/data/ultar/llm-cloud`, Benutzer 837), Modus 0600, geschrieben
neben der alten und dann umbenannt. Angenommen wird nur druckbarer Text ohne Leerzeichen bis 512 Zeichen.
Nicht verschlüsselt abgelegt – offen, siehe unten.

## Anbieter (`src/provider.rs`)

Merkmal `Provider`; heute `Claude`: `POST <Adresse>/v1/messages` mit den Kopfzeilen `x-api-key`,
`anthropic-version`, `content-type`; Körper `model`, `max_tokens`, `system`, `messages`. Die Antwort ist der
Text aller `text`-Blöcke. 401/403 heißt „Schlüssel abgelehnt“. Voreinstellungen: Adresse
`https://api.anthropic.com`, Modell `claude-haiku-5-5`, Version `2026-10-21` – alle drei per Option änderbar
(`--endpoint`, `--model`, `--api-version`).

## Oberfläche

Einstellungen › Assistent, Abschnitt „Cloud-Modell, als letzter Weg“: Anbieter, Zustand, „Schlüssel eingeben“
(verdeckt), „Schlüssel vergessen“ (mit Rückfrage). Der Abschnitt fehlt auf Geräten ohne diesen Dienst.
Ohne Tafel gebaut.

## Tests

Dienst: 4 (Schlüssel-Ablage, Antwortform, echter Prozess gegen einen Ersatz-Anbieter: ohne Schlüssel keine
Anfrage, der Helfer kann keinen Schlüssel setzen, die Einstellungen können nicht denken lassen, abgelehnter
Schlüssel). Helfer: Reihenfolge und echter Prozess. Einstellungen: der Dialog, mit Klicks.

## Offen

- **Nicht gegen den echten Anbieter geprüft** (kein Schlüssel hier): Version der Schnittstelle und Modellname
  stammen aus der Dokumentation des Anbieters vom 25.11.2026.
- Schlüssel verschlüsselt ablegen (an die Geräte-Identität gebunden), statt nur mit Dateirechten.
- Weitere Anbieter, Auswahl des Modells in den Einstellungen, Kosten-Hinweis.
- Tafel für den Abschnitt; Handy.
