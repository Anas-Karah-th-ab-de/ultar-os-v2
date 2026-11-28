# ultar-voiced – Sprachsteuerung „Hey KM“ für den Helfer

Stand: Paket `voice` (Punkt C8). Der Dienst `ultar-voiced` (Nutzer 842) lässt die Person den
Helfer (`ultar-agentd`) mit der Stimme bedienen: Weckwort sagen, Anfrage sprechen, die Antwort
wird vorgelesen. Arabisch zuerst, dann Englisch und Deutsch. Alles läuft auf dem Gerät.

## Grundsätze

- **Aus, bis die Person es einschaltet.** Ohne Einschalten (`voice.set_enabled`) fragt der
  Dienst den Mikrofonstrom nicht einmal an (`audio.capture` wird nicht gerufen) und verwirft
  jeden Block von `audio.mic` ungelesen. Das zeigt der Test
  `it_is_off_and_deaf_until_the_person_switches_it_on_and_trains_the_word`.
- **Vor dem Weckwort wird nichts aufgenommen.** Der Ton läuft nur durch einen kurzen
  Äußerungspuffer (höchstens 3 s) und wird beim nächsten Sprechpause verworfen. Erst nach dem
  Weckwort entsteht eine Aufnahme der Anfrage (höchstens 15 s); sie wird nach der Weitergabe
  verworfen. Es gibt keinen Pfad, der Ton auf die Platte schreibt.
- **Nur das Training bleibt.** Gespeichert werden die Merkmalsfolgen der trainierten Wörter
  (Weckwort, Geheimwort) und der Stimmabdruck (24 Zahlen) in `/data/ultar/voice` (Modus 0600,
  CRC, atomar). Keine Aufnahmen, keine Texte.
- **Nichts verlässt das Gerät.** Spracherkennung und Sprachausgabe sind lokale Prozesse.
  Der Erkennungsserver bindet nur an `127.0.0.1`. Seine Ausgabe wird nicht protokolliert (sie
  enthielte das Gesagte).

## Ablauf einer Anfrage

```
audio.mic ─► Vad (10-ms-Schritte, anpassbare Rauschgrenze) ─► Segmenter (Pause 300 ms)
          ─► Weckwort (DTW über MFCC)  ─► Signalton (audio.play)
          ─► Aufnahme (Ende nach 1,2 s Stille, Start-Timeout 6 s, höchstens 15 s)
          ─► Sprecherprüfung (Stimmabdruck)  ─► Whisper (whisper-server, 127.0.0.1)
          ─► agent.ask_voice  ─► eSpeak NG ─► audio.play
```

Die Zeit wird in Abtastwerten gezählt; die Zustandsmaschine (`pipeline.rs`) entscheidet nur,
der Dienst (`service.rs`) führt aus.

### Gesperrter Bildschirm

Die Sperre meldet `voice.set_locked`. Solange gesperrt ist, weckt **nur das Geheimwort**; das
Weckwort „Hey KM“ ist taub. Die Anfrage geht als `agent.ask_voice { locked: true }` an den
Helfer; dieser öffnet dann nur harmlose Lese-Werkzeuge (`Limits.locked`, siehe
`ultar-agentd/src/tools.rs`, ohne `see_screen`, `notifications`, `settings_list`,
`setting_get`, `what_did_you_do`, `devices`, `storage`). Nichts wird verändert.
Tests: `a_locked_screen_leaves_only_harmless_looking_open` (Einheit),
`a_spoken_request_acts_but_on_a_locked_screen_nothing_is_changed` (Prozess, agentd) und
`the_word_the_request_the_answer_and_the_lock_and_the_stranger` (Prozess, voiced).

### Fremde Stimme

Passt die Anfrage nicht zum Stimmabdruck, wird nichts weitergegeben. Der Dienst sagt in der
Sprache der Person „Du bist nicht der Standardnutzer.“ (ar/en/de) und bittet um Bestätigung
mit Fingerabdruck oder Gesicht. Bestätigt die Sperre/Anmeldung (`voice.owner_confirmed`)
innerhalb von 60 s, gilt die **nächste** Anfrage einmal ohne Stimmprüfung.

## Entscheidungen und Lizenzen

| Aufgabe | Wahl | Lizenz | Wie eingebunden |
|---|---|---|---|
| Spracherkennung | whisper.cpp 1.9.6 (`whisper-server`, CPU, SSE4.2) | MIT | eigener Prozess, nur bei Bedarf gestartet, nach 120 s Leerlauf beendet |
| Modelle | `ggml-tiny/base/small/medium.bin` (Whisper-Gewichte von OpenAI) | MIT | Dateien in `/data/ultar/voice/models`, kommen mit dem Offline-Update, nicht im Paket |
| Sprachausgabe | eSpeak NG 1.52.0 | GPL-3.0-or-later | eigener Prozess (CLI), Text über stdin, WAV über stdout; nicht gelinkt |
| Weckwort / Geheimwort | eigene Vorlagen-Erkennung (MFCC + DTW), in Rust | eigen | im Dienst |
| Stimmabdruck | eigener Mittelwert/Streuung der MFCC, in Rust | eigen | im Dienst |

Gründe:

- **whisper.cpp** läuft ohne Python und ohne GPU, hat ein kleines HTTP-Programm (`/inference`)
  und versteht Arabisch. Der Dienst spricht es mit einem minimalen eigenen HTTP/1.1-Client
  (`http.rs`), keine neue Rust-Abhängigkeit.
- **eSpeak NG** ist klein, ohne Modelldateien (Daten im Paket), mit arabischer Stimme
  (`espeak-ng --voices=ar`). Er klingt synthetisch; für Antworten des Helfers genügt das. Die
  GPL gilt für den Prozess und das Paket, nicht für Ultar-Code: es wird nicht gelinkt, die
  Schnittstelle ist die Befehlszeile. Wer eine andere Lizenz braucht, tauscht das Paket
  (`Speaker`-Trait in `engine.rs`).
- **Weckwort ohne Fremdbibliothek.** Es gibt keine zugelassene Weckwort-Bibliothek im
  Arbeitsbereich; neue Crates sind nur im Notfall erlaubt. Daher vergleicht der Dienst die
  Aussprache der Person mit ihren eigenen Aufnahmen (Dynamic Time Warping über mittelwertfreie
  MFCC c1..c12). Der Schwellwert entsteht aus dem Abstand der drei Trainingsaufnahmen
  (Faktor 1,35, Untergrenze 1,0).

## Pakete (`tools/ultar-paketbau/pakete/`)

- `whisper/paket.toml`: ninja 1.13.2, cmake 4.5.1 als Werkzeug-Teile, dann `whisper-cpp` 1.9.6
  (nur CPU, Ziele `whisper-server` und `whisper-cli`). Prüfungen: `whisper-server --help`
  enthält `--inference-path`, `whisper-cli --help` enthält `--language`.
- `espeak-ng/paket.toml`: eSpeak NG 1.52.0 (ohne MBROLA, Sonic, PCAudio, SpeechPlayer, Async).
  Prüfungen: `--voices=ar` nennt „Arabic“, `-v ar --stdout hello` liefert „RIFF“.

Beide Pakete werden am Zielpräfix (`/pkg/whisper`, `/pkg/espeak-ng`) gebaut und sind mit
`sha256` der Quellen fixiert.

## Bus-Vertrag (`ultar-bus/src/messages/voice.rs`)

| Name | Art | Zweck |
|---|---|---|
| `voice.state` | Thema, gehalten | Schalter, Phase, Training (je 3 Aufnahmen), Sprache, Modell(e), Fehler |
| `voice.set_enabled/set_language/set_model` | Aufruf | Einstellungen (Einstellungs-App) |
| `voice.enroll/forget/test` | Aufruf | Training, Vergessen, Test des Weckworts |
| `voice.set_locked` | Aufruf | die Sperre meldet den Zustand |
| `voice.owner_confirmed` | Aufruf | Fingerabdruck/Gesicht hat die Person bestätigt |
| `audio.capture` | Aufruf an audiod | Mikrofonstrom an/aus |
| `audio.mic` | Thema (nicht gehalten) | PCM16-Blöcke, 16 kHz mono |
| `audio.play` | Aufruf an audiod | Ton abspielen; Antwort, wenn fertig |
| `agent.ask_voice` | Aufruf an agentd | gesprochene Anfrage (`text`, `locked`) |

Das Training sendet **nie** Ton über den Bus: der Dienst nimmt die Aufnahmen selbst vom
Mikrofonstrom, die Einstellungen zählen nur mit.

## Einstellungen

Einstellungen > Assistent > „Sprachsteuerung“: Schalter (aus), Sprache (ar/en/de), Modell
(nur installierte), Training von Weckwort, Geheimwort (gesperrter Bildschirm) und Stimme
(je 3 Aufnahmen, einzeln vergessbar), Test-Knopf, Datenschutz-Hinweis, Hinweis „andere Stimme
abgelehnt: mit Fingerabdruck oder Gesicht bestätigen“. Texte in ar/en/de
(`app.settings.voice.*`), Klick-Tests in `apps/ultar-settings/tests/voice.rs`.

## Was bewiesen ist

Alle Tests laufen ohne Mikrofon:

- Einheit (`cargo test -p ultar-voiced --lib`, 42 Tests): VAD, Segmentierung, MFCC, DTW, Vorlagen,
  Zustandsmaschine (Weckwort, Anfrage, Sperre, Fremdstimme, Training, Test), HTTP-Client,
  Whisper-Antwort, eSpeak-Aufruf, Texte in drei Sprachen, Speicherdatei (0600, defekte Datei
  → Standard).
- Prozess (`tests/process.rs`): das echte `ultar-voiced` am Bus mit dem **echten `ultar-audiod`**
  auf einer Fake-Karte (zwei Dateien: Aufnahme rein, Wiedergabe raus) und einem Stand-in für
  agent, einem Stand-in-Whisper-Server (HTTP) und einem Stand-in-eSpeak: aus bis zum
  Einschalten; Training; Anfrage bis zur Antwort; anderes Wort weckt nichts; gesperrt nur
  Geheimwort und `locked: true`; Fremdstimme abgelehnt und gesprochen, dann
  `owner_confirmed`; Ausschalten stoppt den Strom.
- Echte eSpeak-Sprache (`tests/fixtures.rs`, Aufnahmen mit dem gebauten Paket erzeugt,
  16 kHz): dasselbe Wort wird erkannt (Abstand 4,4 und 4,6 bei Schwelle 7,0), andere Wörter,
  auch das ähnliche „Hey Kim“ (14,1), und andere Stimmen (17,3 und 22,2) nicht. Stimmabdruck:
  die eigene Stimme (19,8 bei Schwelle 40,0) wird angenommen, weibliche und kindliche Stimme
  (41,9 und 47,4) werden abgelehnt.

- Die echten Programme, einmal von Hand auf dem Testserver mit dem Code des Dienstes
  gefahren (kein Dauertest, weil er die Pakete unter `/pkg` und ein Modell braucht): der
  gebaute `whisper-server` mit dem Modell `tiny` schrieb die eSpeak-Aufnahme „Turn the wifi
  off“ als „Turn the Wi-Fi off.“ ab (etwa 2,6 s auf einem ausgelasteten Server, zweimal
  gleich); eSpeak NG sprach Arabisch, Englisch und Deutsch und lieferte WAV in einen Pipe mit
  unbekannter Länge (größte Längenangabe im Kopf); der Dienst liest das (Test
  `a_streamed_wav_of_unknown_length_runs_to_the_end`, der Stand-in-eSpeak der Prozesstests
  schreibt dasselbe Format). Arabische Erkennung ist damit nicht gemessen.

## Messung der Spracherkennung (Whisper small und base, Arabisch zuerst)

Dauertest `tests/real_speech.rs`: eSpeak NG spricht neun typische Anfragen (fünf Arabisch, zwei
Englisch, zwei Deutsch; Arabisch zusätzlich mit Vokalzeichen), `whisper-server` schreibt sie ab,
der Test gibt Wortgenauigkeit (1 minus Wortabstand durch Wortzahl, nach Normalisierung) und Zeit je
Aufnahme aus. Er tut nichts, solange `ULTAR_WHISPER` nicht auf das `whisper-server`-Programm zeigt
(dazu `ULTAR_WHISPER_MODELS`, `ULTAR_ESPEAK`, optional `ULTAR_WHISPER_MODEL_NAMES`, Standard
`small,base`); der Aufruf steht im Kopf der Datei. Gemessen auf dem Testserver .18 (16 Kerne, mit
`nice -n 10`, 4 Threads, Modelle `ggml-small.bin` und `ggml-base.bin`, mehrsprachig):

| Modell | Arabisch | Arabisch mit Vokalzeichen | Englisch | Deutsch | Zeit je Aufnahme |
|--------|----------|---------------------------|----------|---------|------------------|
| small  | 0 %      | 0 %                       | 75 %     | 38 %    | 16,5 s           |
| base   | 0 %      | 0 %                       | 38 %     | 12 %    | 4,1 s            |

Einzelwerte (Anfrage, gehört), small: „افتح الإعدادات“ gehört als „وقتها“; „كم الساعة“ als
„كيلومي تسأل“; „أطفئ الواي فاي“ als „فالويفة“; Englisch „Turn the wifi off“ als „Turn the Wi-Fi
on.“ (75 %). Alle 14 Zeilen stehen in der Ausgabe des Tests (`--nocapture`).

**Was das heißt, ehrlich:**
- Mit der eSpeak-Stimme versteht auch `small` kein einziges arabisches Wort vollständig (0 %).
  Die gehörten Wörter klingen oft ähnlich („كمي أسارة“ für „كم الساعة“): das Modell hört etwas
  Ähnliches, aber die Stimme ist zu künstlich. Dass die Vokalzeichen nichts ändern, spricht
  dafür, dass die Grenze die eSpeak-Stimme ist, nicht allein das Modell. **Das sagt deshalb nicht,
  wie gut Whisper echte arabische Menschenstimmen erkennt; das ist weiterhin ungemessen** und
  braucht aufgenommene Sprecher (Hochdeutsch/Hocharabisch und Dialekt, Mikrofon, Raum).
- Sicher ist nur der Rang: `tiny` und `base` sind für Arabisch ungeeignet (schon Englisch fällt bei
  `base` auf 38 %); `small` ist die untere Grenze. Empfehlung: Standard `small` (so ist es: ohne
  Wahl nimmt der Dienst `small`, dann `base`, dann `tiny`), für Arabisch nicht darunter, und wo der
  Speicher reicht `medium` (1,5 GB; nicht gemessen) – Settings zeigt dazu den Hinweis „Für Arabisch
  Small oder größer“.
- Die Zeit ist ein ernstes Problem: etwa 16,5 s je kurzer Anfrage mit `small` auf 4 Threads eines
  PC-Servers (Whisper rechnet immer ein 30-s-Fenster), `base` etwa 4 s. Auf einem Telefon wäre es
  langsamer. Ein Sprachbefehl mit `small` ist so für den Alltag zu träge. Unprobierte Wege, in der
  Reihenfolge der Erwartung: mehr Threads, quantisierte Modelle (zum Beispiel `small` mit
  5-Bit-Gewichten, etwa ein Drittel der Größe), iGPU/NPU (Paket `IGPU-NPU`). Bis das gemessen ist,
  ist die Arabisch-Erkennung des Pakets nicht alltagstauglich nachgewiesen.

## Was nicht bewiesen ist (ehrlich)

1. **Keine echten Menschenstimmen.** Alle Aufnahmen sind Synthese oder Töne. Wie gut das
   Weckwort bei echten Stimmen, Räumen, Mikrofonen und Lärm funktioniert, ist ungemessen und
   braucht ein Mikrofon mit Personen. Erwartung: brauchbar für eine Person in ruhiger
   Umgebung, anfällig für Falsch-Alarm und Verpasst in Lärm.
2. **Der Stimmabdruck ist ein schwacher Test, kein Schloss.** Er trennt deutlich verschiedene
   Stimmen, nicht ähnliche: in den Aufnahmen wird eine zweite männliche eSpeak-Stimme
   angenommen (26,4 bei Schwelle 40,0). Er ersetzt weder Passwort noch Fingerabdruck; die
   Bestätigung `voice.owner_confirmed` ist dafür da. Das Geheimwort ist ebenfalls kein
   Geheimnis gegen Mithörer: wer es mithört und ähnlich spricht, weckt den Helfer; die Sperre
   lässt dann nur harmloses Lesen zu.
3. **Mikrofonstrom: nur mit Fake-Karte bewiesen.** `audio.capture`, `audio.mic` und
   `audio.play` bedient jetzt der echte `ultar-audiod` (`docs/integration/ultar-audiod.md`);
   der Prozess-Test läuft gegen ihn mit einer Fake-Karte. Mit einem echten Mikrofon ist es
   nicht gelaufen (Kernel: keine Aufnahme-Abtastrate getrennt von der Wiedergabe, siehe dort).
   Die Anfrage `audio.capture` ist eine Leihe von 60 s; voiced erneuert sie alle 20 s.
4. **Fingerabdruck/Gesicht.** Es gibt keinen Bus-Dienst für Gesicht; die Bestätigung ist als
   Aufruf `voice.owner_confirmed` vorbereitet. Dass Sperre/Anmeldung ihn nach einer
   Bestätigung ruft, ist offen (anderes Paket). `voice.set_locked` muss die Sperre ebenfalls
   noch rufen.
5. **Modelle.** Die Whisper-Modelle sind nicht im Paket; sie kommen mit dem Offline-Update
   nach `/data/ultar/voice/models`. Das Paket `whisper` ist gebaut und geprüft (siehe unten),
   aber die Erkennungsqualität mit echter arabischer Sprache ist nicht gemessen (mit eSpeak-
   Stimme erkennt auch `small` kein arabisches Wort, siehe die Messung oben). Das Modell
   `small` (466 MB, etwa 1 GB RAM) ist der Standard und für Arabisch die Untergrenze; `tiny`
   und `base` taugen dafür nicht. Auch die Zeit (etwa 16 s je Anfrage) ist offen.
6. **Auslieferung.** `build-a30.sh` und `make-vmware.sh` bauen `ultar-voiced` mit und legen
   Programm und `voiced.toml` ins Abbild (der Dienst startet, ist aber aus, bis die Person ihn
   in den Einstellungen einschaltet). Die Pakete `whisper` und `espeak-ng` und die Modelle
   kommen noch nicht ins Abbild; ohne sie meldet der Dienst „keine Spracherkennung“ bzw.
   „keine Sprachausgabe“.

## SCOPE-EXTRA (über die erlaubte Liste hinaus angefasst)

- `rootfs/etc/ultar/bus-policy.toml`: Regel `system.voiced`, `app.settings` darf `voice.*`,
  `system.audiod` darf `audio.mic` veröffentlichen.
- `rootfs/etc/ultar/ids.toml`: Nutzer `voiced = 842` und Mitgliedschaften.
- `rootfs/etc/ultar/services/bus.toml`, `logd.toml`: Sockel und Gruppe 842.
- `system/core/ultar-busd/src/service_files_tests.rs`: die Tabellen kennen 842/`voiced`.
- `system/services/ultar-agentd/tests/process.rs`: der Prozess-Test für `agent.ask_voice`.
- `apps/ultar-settings/src/**`: außer der Assistent-Seite die Verdrahtung (Port, Zustand, Aktion).
