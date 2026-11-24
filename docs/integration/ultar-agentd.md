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
- Hilfe-Themen (`src/help.rs`, Texte `agent.help.<thema>` in drei Sprachen): pairing, continue, files, model,
  wifi, bluetooth, updates, helper. Ohne Modell beantwortet der Helfer Fragen dazu („wie …“, „كيف …“);
  einem Modell dient das Werkzeug `help`.
- Die kleinste Hilfe versteht zusätzlich: Bluetooth an/aus, Flugmodus an/aus, Bildschirmfoto, Sperren.
- Tests: 22.
