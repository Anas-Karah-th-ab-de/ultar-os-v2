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
| `llmcloud.state` | Thema, gehalten: `configured`, `verified`, `provider`, `endpoint`, `model`, Liste der Anbieter und der Modelle | Helfer, Einstellungen |
| `llmcloud.set_key`, `llmcloud.clear_key`, `llmcloud.configure` | Aufruf | nur die Einstellungen-App |
| `llmcloud.complete` | Aufruf | nur der Helfer |

Kein Aufruf gibt den Schlüssel heraus. `SetKey` druckt sich nie aus (kein Schlüssel im Protokoll).
`Saved.refusal` sagt, warum nicht gespeichert wurde: `Invalid`, `KeyRefused`, `Storage`.

## Schlüssel (`src/key.rs`)

Versiegelt abgelegt: `Version ‖ Nonce ‖ AES-256-GCM`. Der Siegel-Schlüssel ist
HMAC-SHA256(Geheimnis, "ultar-llm-cloud key v1"); das Geheimnis (32 zufällige Bytes) liegt in einem
**zweiten** Ordner (`--seal`, `/data/ultar/llm-cloud-seal`), der Schlüssel in `/data/ultar/llm-cloud`
(beide Benutzer 837, Modus 0600, geschrieben neben der alten Datei und dann umbenannt). Angenommen wird nur
druckbarer Text ohne Leerzeichen bis 512 Zeichen.

**Grenze, ehrlich:** Das schützt gegen die Kopie eines Ordners, einer Sicherung des Dienst-Ordners und gegen
zufälliges Mitlesen. Es schützt **nicht** gegen jemanden, der beide Ordner lesen kann (root, ausgebaute
Platte ohne Plattenverschlüsselung). Mehr geht erst mit einem Schlüsselspeicher in der Hardware (TPM/TEE);
den gibt es im System noch nicht. Dann wird nur die Herkunft des Geheimnisses getauscht (Version im Kopf der
Datei).

## Prüfung des Schlüssels

`set_key` fragt zuerst die Modell-Liste des Anbieters ab. Lehnt der Anbieter ab (401/403), wird **nichts**
gespeichert (`KeyRefused`). Ist der Anbieter nicht erreichbar, wird gespeichert und als „nicht geprüft“
gezeigt; beim Start und bei der nächsten Antwort wird neu geprüft. Ein 401 beim Denken setzt „nicht geprüft“.

## Anbieter (`src/provider.rs`)

Merkmal `Provider` (`id`, `name`, `endpoint`, `small`, `models`, `complete`). Zwei Anbieter:

- **Claude**: `POST <Adresse>/v1/messages`, `GET <Adresse>/v1/models`; Kopfzeilen `Authorization: Bearer`,
  `anthropic-version: 2026-10-21`, `content-type`; Körper `model`, `max_tokens`, `system`, `messages`.
  Antwort: Text aller `text`-Blöcke. Stand der Dokumentation des Anbieters: 25.11.2026.
- **OpenAI-kompatibel** (`compatible`): `…/chat/completions`, `…/models`; die Person trägt die Adresse ihres
  Servers ein. Deckt auch einen eigenen Server im Netz ab.

Adressen nur `https`; `http` nur an das eigene Gerät (Schleife). Wechselt Anbieter oder Adresse, wird der
Schlüssel gelöscht – er geht nie an einen anderen Empfänger als den, für den er eingegeben wurde.

Das Modell ist **nicht mehr fest**: Die Liste kommt vom Anbieter, die Person wählt in den Einstellungen;
ohne Wahl nimmt der Dienst das kleine Modell des Anbieters, wenn die Liste es nennt, sonst das erste.
Optionen: `--state`, `--seal`, `--claude-endpoint`, `--claude-api-version`. Wahl in `config.json`.

## Oberfläche

Einstellungen › Assistent, Tafeln `AssistantCloud`, `AssistantCloudLight`, `AssistantCloudKey`
(`design/boards/b10.py`): Karte „Womit der Assistent denkt“ (die vier Stufen, Marke „wird gerade benutzt“),
darunter „Cloud-Modell, als letzter Weg“: Anbieter, Adresse (nur wo nötig), Schlüssel (an / nicht geprüft /
keiner), Modell, was das Gerät verlässt, Kosten-Hinweis („der Anbieter rechnet jede Anfrage über dein Konto
ab; das System fügt nichts hinzu und sieht die Rechnung nicht“), „Schlüssel vergessen“ (mit Rückfrage).
Fehler im Dialog: ungültig, vom Anbieter abgelehnt. Der Abschnitt fehlt auf Geräten ohne diesen Dienst.

## Tests

Dienst: 5 + 2 (Versiegelung: falsches Geheimnis und veränderte Datei öffnen nicht; Antwortform beider
Anbieter; Adress-Regel; echter Prozess gegen einen Ersatz-Anbieter: ohne Schlüssel keine Anfrage, abgelehnter
Schlüssel wird nicht gespeichert, der Helfer kann keinen Schlüssel setzen, die Einstellungen können nicht
denken lassen). Helfer: Reihenfolge und echter Prozess. Einstellungen: Schlüssel-Dialog, Anbieter- und
Modellwahl, mit Klicks.

## Offen

- **Nicht gegen den echten Anbieter geprüft** (kein Schlüssel hier). Modellname ist kein Risiko mehr (Liste
  vom Anbieter); Version der Schnittstelle per Option änderbar.
- Schlüsselspeicher in der Hardware, sobald das System einen hat (siehe Grenze oben).
- Tafeln sind gebaut, aber vom Eigentümer noch nicht freigegeben. Handy fehlt.
- Kein Zähler für verbrauchte Anfragen (nur der Hinweis).
