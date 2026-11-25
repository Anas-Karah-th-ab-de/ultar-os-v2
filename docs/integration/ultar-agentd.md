# ultar-agentd – integration notes

KM Code Craft · Ultar OS v2 · 2026-11-24 · the local helper (area 4) · contract `messages/agent.rs`

The person asks in words; the helper answers and does what was asked **through the system bus only**.
It replaces the helper of v1, which ran inside the old surface with root rights and ran commands itself.

## Where it thinks (`src/brain.rs`)

| Device | Brain | What it can do |
|---|---|---|
| no model, no paired device lends one | `Minimal` | fixed commands in Arabic, English and German (`src/minimal.rs`, words in `src/msg.rs`); everything else gets the text that says what it understands |
| no model (or a small one), a paired device lends a better one | `Mesh(device)` | the lender's model thinks (`mesh.think`), the tools run **here** |
| own model at least as good as any lender's | `Local` | `llm.complete` |

A model that fails or does not keep to the answer form leaves the smallest help, and the answer says so
(`Answer::brain`). `agent.state` (retained) tells the surface which brain is in use and who lends.
How a model computes (processor, graphics chip, neural chip) is the model service's matter: the helper
only reads `llm.state { ready, class }`.

## What it can do (`src/tools.rs`, `src/toolbox.rs`)

Ten tools, each one a typed bus call: `list_apps`, `open_app`, `set_volume`, `mute`, `wifi`, `devices`,
`continue_on` and `send_file` (the mesh: the other device still asks its person), `restart`, `power_off`.
A tool that is not in the list does not exist – a name a model invents is answered "not allowed" and
never run. At most six tools per request.

Dangerous tools (`restart`, `power_off`) first ask the person in the system dialog
(`sysdialog.authenticate`); without "authorized" nothing happens. The smallest help never uses them.

## The model's side (`src/model.rs`, `src/session.rs`)

The model gets the rules and the tool list and answers with one JSON object: `{"tool":…,"args":{…}}` or
`{"say":…}`. After a tool it is told the result. Its words reach the person as the one verbatim argument
of the catalogue text `agent.says`; every other answer is a catalogue key (`locales/{ar,en,de}.ftl`).

## Tests (19)

Unit: brain choice, tools, fixed commands in three languages, reading model answers, the session (tools
step by step, invented tool, failing model, a model that never stops). Process (`tests/process.rs`, the
real binary with stand-ins on a test bus): smallest help · lent model · own model and the hand-over to the
lender when it becomes unusable · nothing dangerous without confirmation · texts in three languages.

## Wiring it in (not done)

- `rootfs/etc/ultar/services/agentd.toml` (own user, no kernel right, no folder), `ids.toml`, bus and log
  listeners.
- `bus-policy.toml`: `system.agentd` registers `agent`, publishes `agent.state`, subscribes `llm.state`,
  `mesh.state`, calls exactly the methods of its tools (see `POLICY` in `tests/process.rs`), `mesh.models`,
  `mesh.think`, `llm.complete`, `sysdialog.authenticate`. The shell and the helper app call `agent.ask`.
- The model service must offer the contract `messages/llm.rs` (`llm.state`, `llm.complete`).

Open points of the helper and of the mesh are collected in `docs/OFFEN-VERBUND-HELFER.md`.

## Nachtrag 24.11.2026: anderes Gerät, Abbild

- Werkzeug `ask_device { device, request }`: bittet den Helfer eines gekoppelten Geräts (`mesh.act`).
  Gelungen ist der Schritt nur, wenn dort jeder Schritt gelang.
- Das eigene Modell kommt über `ultar-llm-bus` (`docs/integration/ultar-llm-bus.md`).
- Im Abbild: Benutzer 834, `services/agentd.toml`, Regeln in `bus-policy.toml`.
- Tests: 20 (neu: der Helfer lässt den Helfer eines anderen Geräts handeln).

## Nachtrag 25.11.2026: mehr Werkzeuge, Hilfe-Themen

- Werkzeuge (18): neu `bluetooth`, `airplane`, `media` (play_pause, next, previous), `screenshot`, `lock`,
  `help`, `suspend` (mit Bestätigung der Person).
- Hilfe-Themen (`src/help.rs`, Texte `agent.help.<thema>` in drei Sprachen unter `ultar-bus/locales/agent/`): pairing, continue, files, model,
  wifi, bluetooth, updates, helper. Ohne Modell beantwortet der Helfer Fragen dazu („wie …“, „كيف …“);
  einem Modell dient das Werkzeug `help`.
- Die kleinste Hilfe versteht zusätzlich: Bluetooth an/aus, Flugmodus an/aus, Bildschirmfoto, Sperren.
- Tests: 22.

## Nachtrag 25.11.2026 (2): erreichbar, zusammen geprüft

- Die Hülle bietet in der Suche der Übersicht als letzten Treffer „Den Helfer fragen: …“ (sobald `agent.state`
  gesehen wurde, ab drei Zeichen). Die Antwort erscheint als Meldung der Hülle; die Texte `agent.*` lädt die
  Hülle aus `ultar_bus::messages::agent::LOCALES`.
- `open_app` nennt dem Starter die einteilige Kennung (`files` statt `org.ultar.files`).
- `tests/together.rs`: zwei Geräte, echte Dienste, ausgelieferte Regeln. Tests gesamt: 23.

## Nachtrag 25.11.2026 (3): der Helfer erreicht das ganze System – und man sieht es

Entscheidung des Eigentümers: Der Helfer soll **alles im System erreichen**, Gefährliches bestätigt die Person;
was er erreicht und tut, soll **sichtbar** sein (eigene Fläche und Seite „Was der Helfer darf“).

### Bereiche und Werkzeuge (33, `src/tools.rs`)

Jedes Werkzeug gehört zu einem Bereich (`ultar_bus::messages::agent::Area`).

| Bereich | Werkzeuge (★ = die Person bestätigt vorher) |
|---|---|
| Apps | `list_apps`, `open_app`, `close_app`★ |
| Einstellungen | `settings_panels`, `settings_list`, `setting_get`, `setting_set` (heikle Bereiche ★) |
| Netz | `network_status`, `wifi`, `bluetooth`, `airplane` |
| Ton | `sound_status`, `set_volume`, `mute`, `media` |
| Bildschirm | `screenshot`, `lock` |
| Benachrichtigungen | `notifications`, `dismiss_notification` |
| Speicher | `storage`, `eject` |
| Andere Geräte | `devices`, `continue_on`, `send_file`, `ask_device` |
| Energie | `battery`, `suspend`★, `sign_out`★, `restart`★, `power_off`★ |
| Hilfe | `help`, `what_can_you_do`, `what_did_you_do` |

Die breiteste Tür sind die Einstellungen: über `settings.list/get/set` erreicht der Helfer **jede** Einstellung
des Systems (281 Schlüssel). Darum hat sie die meisten Regeln (`src/toolbox_system.rs`):

- **Nie die eigenen Schalter:** Schlüssel `assistant.*` lehnt der Helfer ab – er kann sich weder einschalten
  noch einen Bereich öffnen noch seinen Verlauf verlängern.
- **Nie Verwalter-Einstellungen:** Der Helfer ist am Bus kein Verwalter; `settingsd` lehnt sie ohnehin ab, der
  Helfer fragt gar nicht erst.
- **Heikle Bereiche erst nach Bestätigung** (Systemdialog): `privacy`, `account`, `sharing`, `users`, `updates`,
  `backup`, `mesh`, `cells`, `emergency`.
- **Nur Werte, die die Einstellung annimmt** (Art, Bereich, Auswahl aus `settings.list`).
- Einstellungen mit „Behalten?“ (z. B. Auflösung) fragt das System wie bei jeder anderen Änderung.

### Was die Person schließen kann

- `assistant.act` (Vorgabe **aus**, Entscheidung des Eigentümers): Bis die Person es einschaltet, sieht der Helfer
  nur nach und erklärt. Erreicht er den Einstellungsdienst nicht, bleibt es dabei (er handelt nie auf Verdacht) und
  fragt alle 5 s erneut. Ohne Modell sagt er, wo man es einschaltet (`agent.act_off`).
- `assistant.reach.<bereich>` (neun Schalter, Vorgabe an): ein geschlossener Bereich lehnt jedes Werkzeug darin
  ab – mit und ohne Modell; das Modell bekommt dessen Werkzeuge gar nicht erst genannt.
- `assistant.history_days`: wie lange die Liste bleibt; 0 = nichts behalten.

Der Helfer folgt `settings.changed` sofort. Zustand am Bus: `agent.reach` (gehalten).

### Was er getan hat

`agent.activity` (gehalten, höchstens 50 Einträge): Anfrage, womit gedacht, Schritte mit Bereich, Einzelheit
(`key=appearance.dark, value=true`) und Ergebnis, Antwort. Liegt in `/data/ultar/agentd/activity.json` (Benutzer
834) – die einzige Datei des Helfers. `agent.clear_activity` darf nur die Einstellungen-App.

### Oberfläche

- Einstellungen › Assistent: die Schalter (allgemeine Liste des Bereichs) und die Karte „Was der Assistent getan
  hat“ mit „Alles vergessen“ (`apps/ultar-settings/src/view/helper.rs`). Tafeln `AssistantReach`, `AssistantDid`.
- Eigene Fläche des Helfers (Blatt der Hülle, Super+H): Tafeln `HelperSheet`, `HelperConfirm` – **nicht gebaut**,
  wartet auf Freigabe.

### Fehler der früheren Arbeit, hier gefunden und behoben

Medientasten, Schlafen, Neustart und Ausschalten gingen im echten System **nie**: `ultar-mediad` und
`ultar-powerd` nehmen nur Aufrufer an, die die Regeln als Teil des Systems kennen (`media.system`,
`power.system`); die Tests liefen gegen Ersatz-Dienste, die das nicht prüfen. Der Helfer hat jetzt
`media.system`, `power.system`, `session.system`, `notify.system`; welche Aufrufe er machen darf, bleibt die
schmale Liste seiner Regel. Ein Test liest die ausgelieferten Regeln und hält das fest
(`tests/reach.rs`, `the_shipped_rules_let_the_services_accept_the_helper`).

### Was „alles“ heute nicht einschließt

- **Dateien** lesen, suchen, verschieben: die Bereiche der Person erreicht nur der Starter (OFFEN B6).
- **Apps installieren, Aktualisierungen, Treiber**: diese Dienste haben keinen Vertrag in `ultar-bus/messages`.
- **Konten, Benutzer, WLAN-Netz mit Passwort beitreten, VPN**: bewusst nicht – Geheimnisse gehen nicht durch ein Modell.
- **Bildschirm sehen** (`assistant.see_screen`), Sprache, Zwischenablage.
- Ohne Modell sagt der Helfer Zustände (Akku, Speicher, Geräte) als rohe englische Zeile.

### Tests

Helfer: 21 + 7 + 3 + 1 (Regeln der Einstellungen, Werte, Liste über Neustart, geschlossener Bereich mit und ohne
Modell, „nur nachsehen“; echter Prozess: Einstellung ändern, eigene und Verwalter-Schlüssel abgelehnt, heikler
Bereich nur nach Bestätigung, Bereich schließen, Liste lesen und vergessen). Einstellungen: `tests/helper.rs` (3).
Nicht geprüft: gegen die echten Dienste `settingsd`, `powerd`, `mediad`, `notifyd`, `volumed`, `sessiond`.
