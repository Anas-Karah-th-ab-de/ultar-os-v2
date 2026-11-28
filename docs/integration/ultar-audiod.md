# ultar-audiod: der Tonserver (Mischer, Mikrofon, Anruf-Vorrang, Echo-Unterdrückung)

`ultar-audiod` ist neben Lautstärke, Stumm und Ausgang der **einzige Halter** des Kernel-Knotens
`/dev/ultar/pcm` (48 kHz, 16 bit, Stereo, Recht `sound`). Alle anderen Programme spielen und
nehmen über **einen eigenen Strom-Socket** auf; der Server mischt, senkt, verteilt das Mikrofon
und entfernt auf Wunsch das Echo. Kein anderer Dienst und keine App hält das Recht `sound`
(`system/core/ultar-init/tests/shipped_services.rs::only_the_sound_server_keeps_the_sound_right`).

## Aufbau

* **Ein Engine-Thread** (`src/engine.rs`) hält die Karte, nimmt Verbindungen an, mischt, liest das
  Mikrofon und schreibt an die Clients. Der Bus-Dienst (`src/service.rs`, `src/sound.rs`) ruft
  ihn über einen Kanal; Ereignisse (Mikrofon-Blöcke, Zustand, fertige Klänge) kommen über
  `Poster` zurück.
* **Karte** (`src/device.rs`): `NodeDevice` (der Kernel-Knoten, `O_NONBLOCK`), `FileDevice` (zwei
  Dateien, Entwicklung und Tests: `--play-file`, `--capture-file`). Fehlt die Karte, werden
  Ströme mit `NoDevice` abgelehnt (der Client läuft still); sie wird alle 2 s neu gesucht.
* **Taktung** über die Wanduhr: Schritte von 5 ms (beschäftigt) oder 20 ms (leer), die Warteschlange
  der Karte wird auf 10 ms (480 Frames) gehalten; die Füllung wird aus Geschriebenem minus
  Verstrichenem geschätzt, weil der Kernel keine Füllstand-Abfrage hat (siehe „Lücken des Kerns“).

## Strom-Protokoll (`system/core/ultar-audio-stream`, Crate `ultar-audio-stream`)

Ein Programm verbindet sich mit **seinem** Socket `<stream-dir>/<Identität>.sock`
(`/run/ultar/audio/system.calld.sock`, …). Der Socket ist die Identität, wie bei den Bus-Sockets
nicht wie bei den Bus-Sockets: `audiod` besitzt die Sockets (Modus `0666`, jeder darf verbinden) und
fragt beim `accept` die Peer-Credentials des Kernels (`SO_PEERCRED`, `ultar-service::peercred`).
Die Benutzer-ID des Peers muss die des Sockets sein (`system.calld` = 829, `system.mediad` = 824,
`rootfs/etc/ultar/ids.toml`), sonst wird die Verbindung ohne Antwort geschlossen
(`Stats::wrong_peer`). Meldet der Kernel keine Credentials (älterer Kern), wird jeder abgewiesen (fail closed).

| Byte | Hello (16 Byte, vom Client) |
|---|---|
| 0–3 | Magie `UAS1` |
| 4 | Klasse: 0 Medien, 1 Hinweis (Prompt), 2 Klingeln, 3 Anruf |
| 5 | Richtung: 0 spielen, 1 aufnehmen, 2 beides |
| 6 | Kanäle: 1 oder 2 |
| 7 | Echo-Unterdrückung (0/1, nur für die Aufnahme) |
| 8–11 | Abtastrate (8000 … 96000 Hz, little endian) |
| 12–13 | Lautstärke in Promille (1000 = unverändert, höchstens 2000) |
| 14–15 | 0 |

Antwort (8 Byte): `UASR`, Status (0 ok, 1 verweigert, 2 keine Karte, 3 falsches Hello, 4 zu viele
Ströme), 3 Byte 0. Danach trägt die Verbindung **rohes PCM** (16 bit, little endian, verschränkt)
im erklärten Format, in beide Richtungen. Der Server wandelt die Rate (gefensterter Sinc,
`src/resample.rs`, Verstärkung bei 0 Hz genau 1) und die Kanäle. Höchstens 16 Ströme.

Niemand hält niemanden auf: der Server liest einen Client nur, solange dessen Puffer Platz hat
(sonst „würde blockieren“, der Client wirft weg, wie am Kernel-Knoten); einem Aufnahme-Client, der
nicht liest, werden die **ältesten** 200 ms verworfen. Ein toter oder stehender Client wird
entfernt, die anderen laufen weiter (Test `a_client_that_dies_or_stops_reading_never_holds_up_the_others`).
Der Client (`client::Stream`) ist nicht blockierend, begrenzt seinen Rückstau auf 200 ms und zählt,
was er verwirft.

### Entscheidung: rohes PCM über den Socket, nicht ein Ring im gemeinsamen Speicher

Ein `memfd`-Ring braucht `SCM_RIGHTS` (Deskriptor über den Socket reichen), `mmap` und damit
`unsafe`-Code in jedem Client und im Server; die Dienste verbieten `unsafe`
(`#![forbid(unsafe_code)]`). Der Gewinn wäre ein Kopiervorgang weniger. Gemessen
(`system/core/ultar-audio-stream/tests/cost.rs`, Release-Build, ein Kern, Entwicklungsserver):
eine Sekunde 48-kHz-Stereo (1 920 000 Byte) über den Socket zu schieben und zu lesen kostet
**0,10 ms je Sekunde Ton**, also weit unter 1 % eines Kerns. Der Ring spart nichts, was zählt;
der Socket gibt dazu kostenlos Rückdruck, Lebenszeichen (EOF) und die Peer-Credentials.

## Mischer (`src/mix.rs`)

* Summe aller Ströme in 48 kHz Stereo mit `f32`, je Strom eine Lautstärke; die Summe läuft durch
  einen weichen Begrenzer (unverändert bis 0,8, danach `tanh` gegen, nie über 1,0): kein
  Überlauf, kein Knacken.
* **Ohne Knacken**: Anfang, Ende und Unterlauf werden über 96 Frames (2 ms) ein- und ausgeblendet.
  Ein Strom beginnt erst nach einem Polster von 20 ms; läuft er trocken, wächst sein Polster um
  10 ms je Unterlauf bis höchstens 80 ms (Test `a_stream_that_runs_dry_gets_a_bigger_cushion_and_never_clicks`:
  größter Sprung zwischen zwei Abtastwerten bleibt klein).
* Ein Strom endet erst, wenn sein Rest gespielt ist; der Platz je Strom ist begrenzt (100 ms Frames).

## Vorrang und Absenken

Nur **Medien** werden abgesenkt; Anruf, Klingeln und Hinweise nie. Es gilt die stärkste aktive Klasse
(aktiv: Ton im Puffer oder in den letzten 300 ms):

| Aktiv | Medien um | Nachweis |
|---|---|---|
| Anruf | −15 dB | `a_call_ducks_media_by_15_db_and_media_comes_back`; Prozess-Test: gemessen **−15,0 dB** |
| Klingeln (eingehender Anruf) | −12 dB | `a_ring_and_a_prompt_duck_media_less_than_a_call` |
| Hinweis (Signalton, gesprochene Antwort) | −9 dB | derselbe Test |

Das Absenken dauert 10 ms, das Zurückkehren 250 ms (kein Pumpen). Sprachsteuerung blockiert
niemanden: ihr Zuhören ist ein reines Aufnahme-Abonnement ohne eigenen Ton; ihre Hinweise sind die
Klasse „Hinweis“ und werden auch während eines Anrufs gespielt (nur Medien werden gesenkt).

## Rechte

Was ein Programm darf, steht in `rootfs/etc/ultar/bus-policy.toml` als Pseudo-Methoden der Regel
seiner Identität (`Policy::may_call(identität, "audio", methode)`), nicht in einer zweiten Liste:

| Pseudo-Methode | erlaubt |
|---|---|
| `audio.play` | Klassen Medien und Hinweis spielen |
| `audio.ring` | Klingeln spielen |
| `audio.call` | Anruf-Strom (Klasse Anruf) |
| `audio.capture` | **das Mikrofon**: Aufnahme-Strom oder Bus-Aufruf `audio.capture` |

Wer das Recht nicht hat, bekommt `Denied`. Ein reiner Wiedergabe-Client kann nicht aufnehmen, auch
nicht „im Duplex“. Aktuell: `system.calld` (`audio.call`, `audio.capture`), `system.mediad`
(`audio.play`), `system.voiced` (`audio.capture`, `audio.play`), `app.browser` (`audio.play`).
Beweis: `engine_tests::rights_decide_who_plays_what_and_who_records`,
`tests/sound_process.rs` (echter Prozess: `app.nobody` verweigert in allen Fällen, Medien-Client
kann nicht aufnehmen, Bus-Aufruf ohne Recht → `PermissionDenied`).

`--streams system.calld:829,system.mediad:824` nennt zu jeder Identität die Benutzer-ID; `audiod`
ist in **keiner** Gruppe der Clients (eine Gruppe von `calld`/`mediad` würde auch deren Bus- und
Log-Sockets (`0660`) öffnen). Beweis: `engine_tests::a_peer_with_another_user_id_is_not_the_identity_and_is_closed`,
`shipped_services::the_sound_server_knows_its_clients_by_user_id_and_shares_no_group_with_them`.

## Mikrofon über den Bus (für Sprachsteuerung)

Genau wie `voice.rs` es festlegt:

* `audio.capture {on}` ist eine **Leihe von 60 s**; `ultar-voiced` erneuert sie alle 20 s. Läuft sie
  ab (voiced stirbt), geht das Mikrofon aus.
* `audio.mic`: Blöcke zu 320 Abtastwerten, 16 kHz mono (nicht gehalten).
* `audio.mic_state` (**neu**, `MicState {in_use, users}`, gehalten): die Shell kann „Mikrofon in
  Gebrauch“ anzeigen; sie umfasst Bus-Leihen **und** Strom-Aufnahmen. Die Regel der Shell abonniert
  das Thema.
* `audio.play {samples, rate}`: ein Klang als Hinweis; die Antwort kommt, wenn er gespielt ist.

## Echo-Unterdrückung (`src/aec.rs`)

Reines Rust, ohne neue Bibliothek: partitionierter Block-Frequenzbereich-Filter (PBFDAF) bei 16 kHz,
Block 128, 16 Partitionen (**128 ms** Echoweg), eigene Radix-2-FFT. Nur für Ströme mit
`echo_cancel` (Anrufe). Als Gegenstelle dient, was die Karte tatsächlich gespielt hat (Spiegel der
Warteschlange), nicht, was die Clients schickten.

Doppelsprechen: Geigel-Kriterium (Mikrofon lauter als die Gegenstelle erklären kann) und Sprung des
Restes über das Dreifache seines Mittels frieren die Anpassung ein; nach 3 s Dauer gilt es als
geänderter Raum. Ist der Filter schlechter als nichts (Rest > 2× Mikrofon), wird er verworfen.

Messungen (synthetisches Echo, Test auf dem Entwicklungsserver):

| Fall | Ergebnis |
|---|---|
| Echo, 5 s Anlauf, ERLE | **52 dB** (Test fordert > 30 dB) |
| Durch die Engine (Raum mit zwei Reflexionen) nach 9 s | **> 20 dB** gefordert, gemessen 79,7 dB |
| Raumwechsel nach 4 s, 6 s später | **91,6 dB** (gefordert > 25 dB) |
| Doppelsprechen: Nahe Sprecherin über dem Rest | gefordert > 12 dB, Pausen der Nahen > 15 dB |
| Gegenstelle still | nichts wird verändert |

**Was sie nicht kann** (ehrlich):

* Nur **linear**: kein Restecho-Unterdrücker, kein Nichtlinearitäts-Modell (billige Lautsprecher
  verzerren; der Rest bleibt hörbar).
* **Keine Taktdrift-Ausgleichung** zwischen Lautsprecher und Mikrofon (eine echte Karte hat getrennte
  Takte); ohne sie verschlechtert sich der Wert über Minuten. Die Zuordnung von Gegenstelle und
  Mikrofon läuft über Warteschlangen; die Gegenstelle darf dem Mikrofon höchstens 16 ms voraus sein.
* Die Doppelsprech-Erkennung ist pegelbasiert und einfach; sie nimmt an, dass das Echo nicht lauter
  ist als die Gegenstelle.
* Echoweg über 128 ms (große Räume, Bluetooth-Lautsprecher mit Verzögerung) wird nicht erfasst.
* Alles ist mit **synthetischem** Echo gemessen, nicht mit einem echten Lautsprecher und Mikrofon.

## Latenz

Budget: Wiedergabe ≤ 40 ms, Aufnahme ≤ 30 ms. Gemessen wird nur, was Software verantwortet
(Engine mit Schritten von 5 ms in virtueller Zeit, Karte als Fake):

* **Wiedergabe**: vom Eintreffen der Bytes bis zum Stand in der Karten-Warteschlange
  **20,0 ms** (Polster 10 ms + Warteschlange 10 ms + Schritt 5 ms; Test
  `the_latency_of_a_stream_is_inside_the_budget`, fordert ≤ 40 ms, Start des Tons ≤ 4 Schritte).
* **Aufnahme**: höchstens ein Schritt (5 ms) plus die halbe Filterbreite des Umtasters (12
  Abtastwerte der niedrigeren Rate: 0,75 ms bei 16 kHz); mit Echo-Unterdrückung ein Block (8 ms).
  Das ist **berechnet, nicht gemessen**.
* Nicht enthalten: der Puffer des Treibers und der Wandler der echten Karte. Ob das Budget auf
  Hardware hält, ist **nicht bewiesen**.

## Clients

| Client | Strom | Änderung |
|---|---|---|
| `calld` (SIM, ID, Mesh-Relais) | Klasse Anruf, beides, 16 kHz mono, Echo-Unterdrückung | `src/pcm.rs`: `PcmAudio` ist ein Strom; `--audio SOCKET` (Standard `/run/ultar/audio/system.calld.sock`); Mesh-Gast: `StreamTap` (48 kHz mono, Anruf, Echo-Unterdrückung); `--mesh-modem-pcm` bleibt ein eigener Knoten (Modem, kein Mikrofon) |
| `mediad` (dort abspielen) | Medien, spielen, 48 kHz mono | `StreamSink`; `--there-sink SOCKET`; kein Recht `sound` mehr |
| `voiced` | Bus: `audio.capture`/`audio.mic`/`audio.play` | Leihe wird alle 20 s erneuert; Prozess-Test gegen den **echten** `audiod` |
| `servo-host` | Medien, spielen, 48 kHz Stereo | `src/pcm.rs`, `src/media/gst.rs`; `--audio-socket`; liegt der Socket nicht in einem Ordner der Zelle, wird die Zelle abgelehnt |
| `apps/ultar-web` | Medien, spielen | `src/media/sink.rs`; Manifest ohne Kernel-Recht |

Die **Einstellung Mono** (`accessibility.mono_audio`, vorher der Schalter `/dev/ultar/pcm-mono` in
calld) wirkt jetzt im Mischer von `audiod` (beide Kanäle in jeden Lautsprecher, für alle Ströme);
`audiod` liest und abonniert sie über den Bus. Der Kernel-Schalter wird nicht mehr benutzt.

## Tests (Beweise)

* `stream` (6 + Kostenmessung): Hello/Antwort, Client, Rückstau.
* `ultar-audiod --lib` (36): Mischer (Summe, Begrenzer, Lautstärke, viele Ströme, Absenken),
  Aufnahme (Verteilung an Clients mit eigener Rate), Rechte, Engine (Ströme mehrerer Programme,
  toter/stehender Client, kein Gerät, 16 Ströme, Latenz, Echo durch die Engine, Unregelmäßigkeit
  ohne Knacken), Echo-Unterdrückung (ERLE, Doppelsprechen, Raumwechsel).
* `tests/sound_process.rs`: der echte Prozess auf einer Fake-Karte; **Anruf, Medien und
  Sprachsteuerung gleichzeitig**, Rechte, Mikrofon-Zustand, Absenken gemessen mit einer
  Tonanalyse (−15,0 dB).
* `ultar-voiced/tests/process.rs`: echte `ultar-voiced`- und `ultar-audiod`-Prozesse.
* `ultar-call`, `ultar-mediad` (Stand-in-Server mit Hello und Antwort), `servo-host`, `ultar-web`:
  Strom-Sinks gegen einen Stand-in-Server; `ultar-init`: nur `audiod` hält `sound`.

## Lücken des Kerns (nicht angefasst, `kernel/ultar-kern`)

* `SO_PEERCRED` und Benutzer-IDs gibt es im Ultar-Kern (`cred.rs`, `uds.rs`, Option 17): die
  Prüfung von `audiod` gilt dort wie auf Linux, Anrufe, Medien und Sprachsteuerung behalten ihren
  Ton. Nur ein Peer, den der Kern im Pid-Namensraum von `audiod` nicht sieht (Prozess-ID 0),
  wird von `ultar_sys::peer_credentials` abgewiesen, auch mit der richtigen Benutzer-ID; Dienste
  im selben Namensraum sind nicht betroffen. Eine Seitenzelle mit eigenem Pid-Namensraum
  (Browser) braucht dafür noch eine Lösung (offener Punkt 3). Ein älterer Kern ohne die Option
  (vier Nullbytes) führt dazu, dass `audiod` jeden Client abweist (fail closed).
* `/dev/ultar/pcm` hat **keine getrennten Raten** für Aufnahme und Wiedergabe und **keine
  Füllstand-Abfrage**; `audiod` schätzt den Füllstand und geht von 48 kHz in beide Richtungen aus.
  Eine Abfrage (`FIONREAD`-artig) und ein Zähler der gespielten Frames würden die Schätzung und
  die Taktdrift der Echo-Unterdrückung ersetzen.
* Der Schalter `pcm-mono` wird nicht mehr gebraucht und kann entfallen.

## Offen und nicht bewiesen

1. **Echte Hardware.** Kein Ton wurde je auf einer echten Karte erzeugt, kein echtes Mikrofon
   aufgenommen; alles läuft mit einer Fake-Karte (Dateien) und echten Sockets/Bus/Prozessen.
2. **Linux/ALSA-Knoten** (`NodeDevice` ist nur der Ultar-Knoten): ein ALSA-PCM-Zugang in
   `ultar-hal-linux` ist **nicht gebaut**. Auf einem Linux-Entwicklungsrechner geht es nur mit
   `--play-file`/`--capture-file`.
3. **Browser-Zelle.** Die Seitenzelle bekommt den Socket nicht von selbst: der Browser/Launcher
   muss `--audio-socket` mitgeben und genau diesen Socket in die Zelle legen (nicht das ganze
   Verzeichnis, sonst sähe die Zelle die Sockets von `calld` und `mediad`); die Benutzer-ID von
   `app.browser` vergibt der Launcher bei der Installation (App-IDs 20000-49999). **`app.browser`
   fehlt deshalb in `--streams` von `audiod.toml`**, bis die ID aus dem Launcher/Bus verfügbar ist;
   der Browser spielt bis dahin keinen Ton über den Server.
4. **Echo-Unterdrückung**: Grenzen oben; kein Test mit echtem Raum.
5. **SIM-Sprachpfad**: weiterhin kein Modem-PCM-Pfad im System.
6. Die Rechte `ring` für Klingeltöne: kein Dienst spielt zurzeit das Klingeln über den Server
   (der Klingelton eines eingehenden Anrufs bleibt, wo er ist); die Klasse und ihr Absenken (−12 dB)
   sind bewiesen, die Verdrahtung des Klingeltons nicht.
