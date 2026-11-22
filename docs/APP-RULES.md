# Ultar OS – Regeln für die App-Entwicklung

KM Code Craft · Stand 23.11.2026 · gilt für Repo `ultar-os-v2` · Grundlage: `claude/UMBAU-PLAN.md` (§3.0, §3.3–3.7, §7), `claude/DESKTOP-ENTWURF.md`, `design/tokens.toml`

> Diese Regeln sind verbindlich für **jede** App: System-Apps von KM Code Craft (Bereich 6, `apps/`) und Fremd-Apps (Bereich 7, `external/`, Store).
> Was mit „MUSS“ markiert ist, prüft die CI (`ultar-check`) oder die Abnahme. Eine App, die eine MUSS-Regel bricht, kommt nicht auf `main` und nicht in den Store.

---

## 1. Grundsatz in fünf Sätzen

1. **Oberfläche = App, Zustand und Regeln = Dienst.** Eine App zeigt an und nimmt Eingaben an; alles, was Hardware, Netz, Konten oder Systemzustand betrifft, macht ein Systemdienst über `ultar-bus`.
2. **Eine App ist ein eigener Prozess in einer eigenen Zelle** mit genau den Rechten aus ihrem Manifest – nicht mehr.
3. **System-Apps haben keine Sonderwege.** Sie nutzen dieselbe öffentliche App-API wie Fremd-Apps, nur mit mehr erlaubten Rechten.
4. **Arabisch zuerst.** Jeder Text entsteht zuerst arabisch, jedes Layout denkt in `start/end`, jeder Bildschirmtest läuft zuerst in RTL.
5. **Jeder Klick funktioniert zu 100 %.** Die Umsetzung sieht genauso aus wie der Entwurf; keine Funktion ohne Sinn, kein Element ohne Zustand.

---

## 2. Aufbau einer App (MUSS)

Jede App ist ein eigenes Crate und als eigenes `.uap` baubar.

```
apps/<app-id>/                     # System-App; Fremd-App: external/<app-id>/ bzw. eigenes Repo
  Cargo.toml                       # Name: ultar-app-<app-id>
  ultar-app.toml                   # Manifest (§3)
  locales/ar.ftl  en.ftl  de.ftl   # alle Texte; ar ist Referenz
  assets/                          # Symbol (SVG, aus dem Symbolsatz), keine Rasterbilder für UI-Elemente
  src/
    main.rs                        # nur Start: Bus verbinden, App registrieren, Schleife
    state.rs                       # Zustand der App (reine Daten, serialisierbar)
    action.rs                      # alle Aktionen (enum), auch die für den Agenten (§9)
    update.rs                      # Ereignis → Zustand + Aktionen (reine Logik, testbar ohne Bildschirm)
    view/                          # je Fläche eine Datei: compact.rs, medium.rs, wide.rs oder Teilflächen
    service.rs                     # einziger Ort mit Bus-Aufrufen (Client der Dienste)
  tests/
    logic.rs                       # Logik-Tests
    flow.rs                        # Ablauftests über echte Ereignisschleife
    shots/                         # Referenzbilder (§11)
```

- `app-id`: Kleinbuchstaben, englisch, Bindestrich (`camera`, `file-manager`, `office-sheet`). Herausgeber-Präfix für Fremd-Apps: `<publisher>.<app>`.
- Die App implementiert den Vertrag aus `platform/app-api`:

```rust
pub trait UltarApp {
    fn manifest(&self) -> &AppManifest;
    fn update(&mut self, ev: &Event, ctx: &mut Ctx) -> Vec<Action>;  // keine Ein-/Ausgabe hier
    fn view(&self, ui: &mut Ui, layout: LayoutClass);                // compact | medium | wide
    fn save_state(&self) -> StateBlob;                               // für Wiederherstellung (§8)
    fn restore_state(&mut self, blob: StateBlob);
}
```

---

## 3. Manifest `ultar-app.toml` (MUSS)

```toml
id          = "camera"
version     = "1.4.0"                 # SemVer; jede Store-Version höher als die vorige
publisher   = "km-code-craft"
kind        = "native"                # native | web | compat-deb | compat-exe | compat-apk
min_os      = "2.0"
category    = "media"                 # Store-Kategorie
name        = "app.camera.name"       # i18n-Schlüssel, nie Klartext
description = "app.camera.desc"
icon        = "assets/icon.svg"

[form]                                # wo die App erscheint
factors     = ["phone", "tablet", "laptop", "desktop"]   # server nur für kopflose Werkzeuge
modes       = ["mobile", "desktop"]                       # beide = Moduswechsel ohne Verlust (§7)

[needs]                               # Fähigkeiten des Geräts; fehlt eine → App nicht sichtbar
required    = ["camera"]
optional    = ["flash", "gnss", "microphone"]

[permissions]                         # Rechte; Standard: nichts
bus         = ["camera.capture", "media.store", "notify.post"]
files       = ["media:pictures:rw"]   # benannte Bereiche, nie rohe Pfade
network     = "none"                  # none | ask | vpn-only | allowed
background  = false                   # darf im Hintergrund laufen?

[agent]                               # was der Agent/Helfer mit der App tun darf (§9)
actions     = ["take_photo", "open_last"]
confirm     = ["delete_media"]

[limits]                              # Gruppen-Grenzen der Zelle
memory_mb   = 120
cpu         = "foreground"            # foreground | background | frozen-when-hidden
```

- Profil der Zelle: System-Apps `app.system`, Fremd-Apps `app.standard` (UMBAU-PLAN §3.7). Eine App kann ihr Profil nie selbst erweitern.
- Jedes Recht braucht im Store-Eintrag eine arabische Begründung „wofür“.
- **Offen:** `ultar-pkg::Manifest` hat heute nur `permissions`/`services` als Liste. Die Felder `form`, `needs`, `agent`, `limits` müssen ergänzt werden, bevor die erste App nach diesen Regeln paketiert wird.

---

## 4. Erlaubte Abhängigkeiten (MUSS, `ultar-check layers`)

| Eine App darf nutzen | Eine App darf NICHT nutzen |
|---|---|
| `platform/app-api`, `platform/ultar-ui` | `platform/ultar-shell`, Compositor-Interna |
| `system/core/ultar-bus` (nur Client), `ultar-i18n`, `ultar-profile` (nur lesen) | Systemdienste direkt als Crate (`ultar-net`, `ultar-modem` …) |
| reine Rust-Bibliotheken aus crates.io nach Prüfung (§10) | `ultar-hal`-Backends, `drivers/`, `kernel/` |
| | andere Apps (Austausch nur über Bus oder Teilen-Dialog) |

- **Keine Hardwarepfade:** `/sys/`, `/dev/`, `/proc/` sind in Apps verboten (`ultar-check paths`).
- **Keine Ausgabe-Abfragen:** `if edition == …` verboten; die App fragt Fähigkeiten (`ultar-check edition`).
- **Keine Systemprogramme starten:** kein `Command::new("ip" | "rfkill" | "apt" …)`; dafür gibt es Dienste (`ultar-check linux-calls`).
- **Keine eigenen Dateipfade:** Daten nur über benannte Bereiche (`app:data`, `app:cache`, `media:*`, `documents`, `share`) aus der App-API.

---

## 5. Daten, Speicherung, Datenschutz

- Eigene Daten nur in `app:data` (wird beim Deinstallieren gelöscht, auf Wunsch gesichert) und `app:cache` (darf das System jederzeit leeren).
- Nutzerdateien (Fotos, Dokumente) gehören dem Nutzer, nicht der App: Zugriff nur über `media:*`/`documents` mit Recht oder über die Dateiauswahl des Systems (einmalige Freigabe).
- **Fortlaufend speichern**, kein „Speichern“-Zwang bei einfachen Inhalten (Notizen, Einstellungen); bei Dokumenten zusätzlich Autosicherung + Wiederherstellen nach Absturz.
- Speicherformat versioniert (`format_version`), Migration beim Update getestet.
- Kamera, Mikrofon, Standort, Kontakte, Netz: werden **beim ersten Gebrauch** gefragt (nicht beim Start), gehorchen der Datenschutz-Zentrale (Hauptschalter aus = sofort aus) und zeigen in der Statusleiste einen Hinweis, solange sie aktiv sind.
- Keine Telemetrie, keine Werbung, keine Tracker – auch nicht in Fremd-Apps aus dem Store.
- Geheimnisse (Kennwörter, Schlüssel, Token) nur über den Schlüsselbund-Dienst, nie in `app:data`.
- **Keine eigene Kryptografie**; nur geprüfte Bausteine über Systemdienste.

---

## 6. Oberfläche und Gestaltung (MUSS)

- **Nur `ultar-ui`-Bausteine** (Knopf, Liste, Karte, Feld, Schalter, Dialog, Leiste …). Eigene Widgets nur, wenn der Baukasten sie nicht hat – dann zuerst in `ultar-ui` aufnehmen, nicht in der App bauen.
- **Alle Werte aus `design/tokens.toml`**: Abstände `4/8/12/16/24/32/48`, Radius `8/12/rund`, Schriftgrößen `12–32`, Farben über Theme-Rollen. Kein fest eingebauter Farb-, Größen- oder Radiuswert in einer App.
- Mindestgröße Ziel: **44 pt Touch, 32 pt Zeiger**.
- Hell und dunkel (Standard dunkel) und Akzentfarbe des Systems; Kamera/Betrachter dürfen immer dunkel sein.
- **Drei Layoutklassen** `compact` (Handy hoch) · `medium` (Handy quer, kleines Tablet) · `wide` (Tablet, Laptop, Desktop) – jede Fläche muss in allen dreien ohne abgeschnittenen Text funktionieren.
- **Jeder Bildschirm hat alle Zustände entworfen:** leer · lädt · Inhalt · Fehler (mit „was tun“) · kein Recht (mit Knopf zur Freigabe) · offline · Gerätefähigkeit fehlt.
- **Eingabefelder bleiben über der Bildschirmtastatur sichtbar** (Fläche scrollt mit).
- Gefährliche Aktionen (löschen, senden, bezahlen, anrufen) mit Bestätigung oder Rückgängig; Löschen bevorzugt Papierkorb.
- Kein Element ohne Funktion, keine Funktion ohne sichtbaren Zustand (gedrückt, aktiv, deaktiviert mit Grund).
- **Symbole** nur aus dem Systemsatz (SVG); App-Symbol nach Symbol-Vorlage (Raster, Ränder, Farbe).
- Barrierefreiheit: jedes Element hat eine Beschriftung für den Bildschirmleser (i18n-Schlüssel), Reihenfolge der Fokusbewegung logisch, hoher Kontrast und große Schrift brechen das Layout nicht.

---

## 7. Gerätearten und Modi

- Sichtbarkeit nur über `[needs]` – eine App ohne Kamera auf einem Gerät ohne Kamera erscheint nicht; optionale Fähigkeiten blenden Teilfunktionen aus statt Fehler zu zeigen.
- **Mobil-Modus:** Vollbild, Zurück-Geste (in RTL gespiegelt), untere Leisten für Hauptaktionen, langer Druck = Kontextmenü.
- **Desktop-Modus:** freies Fenster mit Mindest- und Lieblingsgröße, Titelleiste vom Compositor (Fensterknöpfe am Ende), Menü/Kontextmenü mit Rechtsklick, Tastenkürzel (Strg+C/V/Z/F/S, Entf, F2 …), Ziehen und Ablegen von/nach Dateien, mehrere Fenster derselben App wo sinnvoll, Zeiger-Hover-Zustände.
- **Moduswechsel ohne Verlust:** Handy an Monitor oder Tablet mit Tastatur → App bleibt offen, Zustand bleibt, nur Layoutklasse wechselt.
- Mehrere Bildschirme und gebrochene Skalierung: die App zeichnet in logischen Punkten, nie in Pixeln.
- **Verbund:** wenn sinnvoll, Zustand übertragbar („auf dem PC weiterarbeiten“) über `save_state`/`restore_state` – kein App-eigener Netzweg dafür.

---

## 8. Leistung und Robustheit (MUSS, Budgets in `tools/budgets.toml`)

| Messwert | Handy | PC/Laptop |
|---|---|---|
| App-Start warm (bis erstes Bild mit Inhalt) | ≤ 300 ms | ≤ 150 ms |
| App-Start kalt | ≤ 800 ms | ≤ 400 ms |
| Bildzeit 99 % | ≤ 16 ms | ≤ 8 ms |
| Reaktion auf Tippen/Klick | ≤ 50 ms | ≤ 30 ms |
| RAM im Leerlauf (System-App) | ≤ 40 MB | ≤ 60 MB |
| Aufwachen im Hintergrund | 0 (eingefroren), außer Recht `background` |

- **Ereignisgesteuert, kein Polling.** Daten kommen per Abo über `ultar-bus`; Zeitschleifen nur für Animation, die nur läuft, solange sie sichtbar ist.
- **Nichts Blockierendes im UI-Faden:** Dateien, Netz, Bildverarbeitung, Modelle in eigenen Fäden/Aufgaben; die Oberfläche bleibt immer bedienbar (A30-Lektion: Kameraknoten haben die Oberfläche eingefroren).
- Jeder Dienstaufruf mit Zeitlimit; Fehler `ServiceBusy`/Zeitüberschreitung → sichtbarer Zustand, kein Hängen.
- Nur veränderte Bereiche neu zeichnen; große Listen virtualisiert; Vorschaubilder im Cache.
- Große Daten (Kamerabild, Ton, Fensterpuffer) über geteilten Speicher, nicht über Kopien im Bus.
- **Kein `unwrap`/`expect`/`panic`** außerhalb von Tests (`ultar-check unwrap`); jede App hat einen eigenen Fehlertyp mit i18n-Schlüssel.
- Absturz der App darf nichts anderes beschädigen: Zustand wird bei jeder Änderung sparsam gesichert, nach Neustart wiederhergestellt.
- Statisch gebaut (`+crt-static`), Größenbudget je App im Budget-File.

---

## 9. Schnittstelle zum Agenten/Helfer

- Jede App, die der Agent bedienen darf, meldet ihre **Aktionen** (aus `action.rs`) mit Name, Parametern und arabischer/englischer Beschreibung über den Bus an.
- Drei Stufen: **frei** (öffnen, anzeigen, suchen) · **bestätigen** (senden, anrufen, löschen, kaufen, Einstellungen ändern) · **nie** (Kennwörter, Schlüssel, Zahlungsdaten).
- Der Agent bekommt nur, was die App über die Aktion herausgibt – keinen Zugriff auf `app:data`.
- Jede vom Agenten ausgelöste Aktion ist für den Nutzer sichtbar (Hinweis + Verlauf).

---

## 10. Code-Regeln (MUSS)

- Bezeichner **Englisch** (Crates, Module, Typen, Funktionen, Dateien, Bus-Nachrichten); Kommentare zur Begründung, Entwickler-Doku zusätzlich Arabisch.
- `#![forbid(unsafe_code)]` in jeder App.
- `cargo fmt`, `clippy -D warnings` sauber.
- Datei ≤ 600 Zeilen (Warnung ab 400), Funktion ≤ 60 Zeilen, Typ ≤ 15 Felder, keine Namen wie `more`, `extra`, `misc`, `utils2`.
- **Kein Text im Code:** jede Zeichenkette für Menschen über `tr.t("app.<id>.…")`; Schlüssel immer mit App-Namensraum (`ultar-check text`).
- Keine `left/right` im UI-Code, nur `Side::Start/End` (`ultar-check logical-layout`).
- Ziffern immer 0–9 (`numbers::to_western_digits` bei Eingabe), Datum über `datefmt` (gregorianisch + Hijri), Suche über `arabic::normalize_for_search`, Sortierung über `collation`.
- Fremde Bibliotheken: reines Rust bevorzugt, Lizenz kompatibel, gepflegt, keine Netzaufrufe im Hintergrund; neue Abhängigkeit = Eintrag in `docs/adr/`.

---

## 11. Prüfung und Abnahme

### 11.1 Automatisch (CI)
- Logik-Tests für `update.rs` (jede Aktion, jeder Zustand).
- Ablauftests über die echte Ereignisschleife (wie `flow_tests::kamera_galerie_agent`).
- **Bildschirmtests:** jede Fläche × 3 Layoutklassen × 3 Sprachen (ar zuerst) × hell/dunkel, Bildvergleich gegen `tests/shots/`. Prüfung auf abgeschnittenen Text, lateinische Reste in Arabisch, falsche Richtung.
- i18n-Prüfung: alle Schlüssel in ar/en/de, Platzhalter gleich, Mehrzahlformen vollständig.
- Start in QEMU auf dem **Ultar-Kern** bis zur App (Kern-zuerst).
- Budgets (§8) als Schranke.

### 11.2 Rollen (jede Funktion geht durch alle)
| Rolle | prüft |
|---|---|
| Entwickler | Regeln §2–§10, Tests grün |
| Designer | Umsetzung = Entwurf (Abstände, Ausrichtung, Zustände, Symbole), 2–3-fach angesehen |
| Tester | jeder Klick, jede Geste, jeder Fehlerfall, Moduswechsel, Absturz + Wiederherstellung |
| Arabisch-Prüfer | Sprache, RTL, Textsatz, Ziffern, Datum, Suche |
| Sicherheit | Rechte minimal, keine Datenwege an Diensten vorbei, Agent-Stufen |
| Leistung | Messwerte am Gerät (A30, Beelink, PC) gegen Budget |

### 11.3 Fertig („Definition of Done“)
- [ ] Entwurf in `DESKTOP-ENTWURF`/App-Spezifikation vorhanden und bestätigt
- [ ] Manifest vollständig, Rechte begründet
- [ ] alle Zustände (§6) umgesetzt und im Bildschirmtest
- [ ] Mobil- und Desktop-Modus, 3 Layoutklassen, hell/dunkel
- [ ] ar/en/de vollständig, kompletter Durchlauf nur auf Arabisch bestanden
- [ ] Budgets eingehalten (Messbericht)
- [ ] Agent-Aktionen gemeldet und Stufen gesetzt (falls zutreffend)
- [ ] `ultar-check all` grün, keine neuen Altlasten
- [ ] auf mindestens einem echten Gerät getestet (sonst ausdrücklich „nicht am Gerät geprüft“ vermerkt)
- [ ] Hilfe-Text (ar zuerst) und Store-Text fertig, Info-Text für die Homepage wo sinnvoll
- [ ] `.uap` signiert gebaut, Update von Vorversion getestet

---

## 12. Ablauf für eine neue App

1. **Entwurf:** Zweck, Flächen, Zustände, Aktionen, Rechte, Gerätearten → Abschnitt im Entwurf, Bestätigung durch den Nutzer.
2. **ADR**, falls neue Abhängigkeit, neuer Bus-Dienst oder neues Recht nötig.
3. **Dienstseite zuerst:** fehlt eine Fähigkeit im Bus (z. B. `camera.capture`), wird sie im Dienst gebaut – nie in der App umgangen.
4. Gerüst nach §2, Manifest, Texte zuerst `ar.ftl`.
5. Logik + Tests, dann Ansichten, dann Bildschirmtests.
6. Rollenprüfung (§11.2), Korrekturen, erneute Prüfung.
7. Signieren (`upkg-sign`), in den Store (System-Apps zusätzlich im Abbild, aber einzeln aktualisierbar), Merge auf `main`.

---

## 13. Fremd-Apps (Bereich 7)

- Gleiche App-API, Profil `app.standard`, strengere Zelle; Netz nur laut Manifest und Nutzerfreigabe.
- Store-Aufnahme nur signiert, mit Prüfung von Manifest, Rechten und Arabisch-Übersetzung (für hervorgehobene Apps Pflicht).
- Kompatibilitäts-Apps (`.deb`, `.exe/.msi`, `.apk`, Web-Apps) laufen in ihrer Fremdsystem-Zelle (`cell.linux` / `cell.windows` / `cell.android`), Austausch nur über den Freigabe-Ordner. Sie gelten nicht als Ultar-Apps und unterliegen §6–§11 nicht.
- Offizielle Wege statt Nachbau geschützter Protokolle (WhatsApp Web, Telegram über MTProto-Bibliothek, Spotify Connect).

---

## 14. Was für die Durchsetzung noch fehlt

| Fehlt | Wo | Wann |
|---|---|---|
| `platform/app-api` und `platform/ultar-ui` als Crates | Plattform | P5 |
| Manifest-Felder `form`, `needs`, `agent`, `limits`, benannte Dateibereiche | `system/core/ultar-pkg` | vor erster neuer App |
| Prüfregeln `app-manifest` (Felder, Rechte begründet), `forbid-unsafe` in `apps/`, `tokens` (keine festen Werte), `app-i18n-namespace` | `tools/ultar-check` | mit P5 |
| Bildschirmtest-Werkzeug für getrennte App-Prozesse (heute nur `ultar-shell --shot`) | `tools/` | P5 |
| Bus-Verträge der Dienste, die Apps brauchen (`camera.*`, `media.*`, `radio.*`, `files.*`, Schlüsselbund) | `system/services` | P3–P5, je App vorher |
| Bestehende Apps in `platform/ultar-shell` (`app_*.rs`) einzeln nach diesen Regeln herauslösen | `apps/` | P5 |
