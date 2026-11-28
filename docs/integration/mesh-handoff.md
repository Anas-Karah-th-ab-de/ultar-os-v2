# „Auf anderem Gerät weitermachen“ (Mesh-Übergabe, A5/A6)

Stand 2026-11-28. Die Oberflächen auf Desktop und Telefon, ihre Bus-Aufrufe, die Fehlerzustände und die
Tests. Dienst und Kopplung im Allgemeinen: `docs/integration/ultar-mesh.md`; Bilder und Texte:
`docs/ui/mesh.md`; Tafeln: `design/boards/project/{MeshSend,MeshIncoming,MeshStates,CompactMeshSend,
CompactMeshIncoming}.dc.html`.

## 1. Ablauf

```
App (Telefon)  --mesh.set_activity-->  ultar-mesh A  ~~ Noise ~~  ultar-mesh B  --sysdialog.handoff--> Dialog (Rechner)
Shell (Telefon) --mesh.handoff_send-->        |                         |        <-- Annehmen / Ablehnen
Shell  <-- mesh.handoff_progress (Asked, Accepted, Opened | Declined | Expired | Failed)
                                              ultar-mesh B --launcher.start--> App öffnet an derselben Stelle
                                              ultar-mesh B --mesh.received--> Shell (Rechner): Hinweis „Öffnen“
```

* Vor der Antwort der Person wird nichts geschrieben und nichts geöffnet. Keine Antwort in 60 s = abgelehnt.
* Mit einem Ziel (`HandoffSend { target: Some(..) }`) geht das Angebot sofort ab; ohne Ziel antwortet der
  Dienst mit `Offered`, `Choose(Liste)`, `NoTarget` oder `NoActivity`.
* Die empfangende Seite fragt über den Systemdialog (`sysdialog.handoff`, `AskHandoff`); der Dialog
  antwortet mit `HandoffDecision { accept }`.

## 2. Oberflächen

| Oberfläche | Tafel | Wo | Eintritt |
|---|---|---|---|
| Auswahl am Rechner | `MeshSend` | `platform/ultar-desktop/src/view/mesh_send.rs`, `shell/handoff.rs` | Eintrag „Auf anderem Gerät weitermachen“ im Menü der aktiven App, **Super+Umschalt+M** (Super+M allein tut nichts); der Pfeil der Kachel öffnet die Geräteliste (`qs.mesh.list`) |
| Frage am Rechner | `MeshIncoming` | `platform/ultar-desktop/src/dialog/handoff.rs` | Systemdialog |
| Hinweise am Rechner | `MeshStates` | `view/mesh_toasts.rs`, `app/desktop/mesh_toasts.rs`, `model/mesh.rs` | Toolkit-Toasts mit Abbrechen / Erneut versuchen / Öffnen |
| Geräteliste (Kachel-Pfeil) | – | `view/mesh_list.rs` | Seite `QuickPage::Mesh` im Schnellmenü |
| Auswahl am Telefon „Weitergeben“ | `CompactMeshSend` | `platform/ultar-mobile/src/view/mesh.rs`, `model/mesh.rs` | Karte der App in den letzten Apps **halten und nach oben wischen** (`drag_click`), Knopf „Auf anderem Gerät weitermachen“ der Karte, Pfeil der Kachel (App im Vordergrund) |
| Frage am Telefon „Annehmen?“ | `CompactMeshIncoming` | `platform/ultar-mobile/src/dialog/handoff.rs` | Systemdialog des Telefons |
| Hinweise am Telefon | – | `view/mesh.rs` (`notice`) | Karte über der Navigationsleiste mit Knopf und Schließen |

## 3. Bus-Aufrufe (nichts nachgebaut)

| Aufruf / Thema | Wer | Wofür |
|---|---|---|
| `mesh.set_activity` | Apps, Shell ersatzweise | was gerade offen ist (App, Titel, Ort, Datei) |
| `mesh.handoff_send { target }` | Shell Rechner und Telefon | Angebot abschicken |
| `mesh.offer_cancel { offer }` | beide Shells | wartendes Angebot zurücknehmen (Knopf „Abbrechen“ des Hinweises) |
| `mesh.handoff_progress` (Thema) | beide Shells | Ende eines eigenen Angebots → Hinweis |
| `mesh.state` (Thema, gehalten) | beide Shells | Mesh an, gekoppelte und erreichbare Geräte |
| `mesh.received` (Thema, **neu**) | `ultar-mesh` → Shells | eine Datei ist im Eingang angekommen: `FileReceived { offer, from_name, title, uri }`; die Shell zeigt „Öffnen“ |
| `sysdialog.handoff` | `ultar-mesh` → Dialogdienst | die Frage beim Empfänger |

`mesh.received` ist die einzige Ergänzung am Dienst (SCOPE-EXTRA): Vertrag in
`system/core/ultar-bus/src/messages/mesh_received.rs` (nur additiv, `mesh.rs` unberührt), Auslöser in
`system/services/ultar-mesh/src/arrived.rs` (eine Funktion, drei Hakenzeilen in `lib.rs` und `service.rs`),
Veröffentlichungsrecht in `rootfs/etc/ultar/bus-policy.toml` (`system.mesh`). Es wird nur für eine Datei ohne
App gesendet (Stufe „nur Datei“); eine fortgesetzte Aktivität öffnet sich selbst.

## 4. Fehlerzustände (Tafel `MeshStates`)

| Zustand | Hinweis | Knopf | Aufruf |
|---|---|---|---|
| wartet auf Antwort | „Warte auf die Antwort von …“ | **Abbrechen** | `mesh.offer_cancel` |
| geöffnet | „… hat es geöffnet“ | – | – |
| abgelehnt | „… hat abgelehnt“ | – | – |
| niemand hat geantwortet (60 s) | „Keine Antwort von …“ | **Erneut versuchen** | `mesh.handoff_send` an dasselbe Gerät |
| Verbindung verloren / Fehler | „Nicht gesendet …“ | **Erneut versuchen** | `mesh.handoff_send` |
| Datei angekommen | „… hat … geschickt“ | **Öffnen** | `launcher.start` (Dateien, `uri`) |
| kein Gerät erreichbar | „Kein Gerät kann das jetzt annehmen“ | – | – |
| nichts im Vordergrund | „Nichts zum Weitermachen“ | – | – |

Das Wartehinweis-Fenster am Telefon dauert 65 s (die Frage dort endet nach 60 s), die übrigen 6 s; am Rechner
10 s (ein wartender Hinweis bleibt, bis das Ende kommt).

## 5. Tests

| Test | Was er beweist |
|---|---|
| `platform/ultar-desktop/tests/mesh.rs` | Kachel, Super+Umschalt+M, Auswahl, Geräteliste |
| `platform/ultar-desktop/tests/mesh_conformance.rs` | Tafeln `MeshSend`, `MeshIncoming`, `MeshStates` (Name, Kasten ±2 px, Farbe ±6, Zustand), Arabisch/Englisch, dunkel/hell |
| `platform/ultar-desktop/src/model/mesh.rs` (Unit) | Hinweise, Knöpfe, Ablauf |
| `platform/ultar-mobile/tests/clicks.rs` | Kachel, Karte, Halten + Wischen, Auswahl, Zurück, jeder Hinweis-Knopf |
| `platform/ultar-mobile/tests/mesh_conformance.rs` | Tafeln `CompactMeshSend`, `CompactMeshIncoming` |
| `platform/ultar-mobile/tests/mesh_e2e.rs` | echter Compositor + Bus mit Stellvertreter-Mesh |
| `system/services/ultar-mesh/tests/handoff.rs` | `mesh.received` für eine angekommene Datei |
| `system/services/ultar-mesh/tests/handoff_flow.rs` | zwei echte `ultar-mesh`-Dienste, je ein Bus: Geste am Telefon → Angebot → echte Dialog-Ansicht des Rechners, Annehmen gedrückt → Start derselben App an derselben Stelle (`#line=3`) → Hinweis „geöffnet“ am Telefon |

Die Konformanz-Tests sind in `tools/design-conformance/run.sh` eingetragen; die Tafel-Listen kommen aus
`tools/design-conformance/make_board_manifest.py` (`design/conformance/boards/*.tsv`).

## 6. Benannte Abweichungen (offen beim Eigentümer)

1. **Gerätekarten:** `ultar-ui` hat kein Karten-Layout mit Gerätesymbol (Polster 8, Haken am Ende); die Karten
   sind 69/67/67 statt 61,6/59,6/59,6 px hoch. Die Tests nennen genau diese Kästen (Rechner 5, Telefon 4).
   Behebung: kleines, rein additives Layout im Werkzeugkasten – Entscheidung steht aus.
2. **Touch-Ziele:** die Knöpfe am Telefon sind ≥ 44 px hoch (Tafel: 32 px in 44-px-Zeile).
3. **Einstellungsseite** der Tafel `MeshStates` (`mesh.toggle`, `mesh.pair`, „Port nicht zu öffnen“ → Erneut):
   gehört `apps/ultar-settings`, nicht diesem Paket.
4. Das Telefon hat noch kein App-Menü auf dem Startbildschirm (langes Drücken, `mobile.app`); die Geste und der
   Knopf der Karte in den letzten Apps decken den Eintritt ab.
