# Ultar-Verbund – alle Ultar-Geräte als ein Rechner

Handy, Laptop, Server und Entwicklungsplatinen mit Ultar OS schließen sich auf Wunsch zu einem **Verbund**
zusammen: Sie finden sich im selben Netz selbst, helfen sich gegenseitig, bündeln Rechenleistung für Aufgaben,
die eines allein nicht schafft, und lassen sich voneinander aus steuern. Außerhalb des eigenen Netzes laufen
dieselben Verbindungen Ende-zu-Ende verschlüsselt über **ultar.code-craft.tech** (derselbe Server wie für
Anrufe). Der Nutzer schaltet den Verbund an oder aus – Standard nach der Installation: **aus**.

## Ebenen

| Ebene | Was | Wo |
|---|---|---|
| Ultar-Kern | harter Schalter `/proc/ultar/verbund` (an/aus); bei AUS verwirft der Kern alle Pakete der Verbund-Ports (UDP 47800, TCP 47801) ein- und ausgehend; Geräte-Erkennung im Kern; Konsole `verbund`, `ipfest` | `kernel/ultar-kern/src/verbund.rs` |
| Dienst | `ultar-verbund dienst`: Meldungen, Noise-Kanäle, Kopplung, Aufgaben ausführen, Relais-Verbindung, Zustand für Oberfläche | `crates/ultar-verbund`, `rootfs/etc/ultar/services/verbund.toml` |
| Oberfläche | Einstellungen → Geräteverbund: an/aus, Steuern/Rechnen erlauben, Geräteliste, Kopplungscode | `crates/ultar-shell/src/app_cloud.rs` |
| Befehlszeile | `ultar-verbund an|aus|status|geraete|koppeln|ausfuehren|verteilt|bench` | `crates/ultar-verbund/src/bin` |
| Server | Geräteliste je Konto, Relais (WebSocket) nur zwischen Geräten desselben Kontos | `server/ultraos-server/src/verbund.rs`, `deploy/` |
| Tests | Chaos-Engineering (Namespaces, Abstürze, Netztrennung, Relais-Ausfall, gestörte Leitungen) und Ultar-Kern in QEMU | `tools/verbund-chaos/` |

## Ablauf

1. **Einschalten:** `ultar-verbund an` (oder Schalter in den Einstellungen). Auf dem Ultar-Kern wird
   `/proc/ultar/verbund` mitgeschaltet.
2. **Entdecken:** alle 2 s Meldung per UDP-Rundruf (Port 47800): Kennung `UV-xxxx-xxxx`, Name, Ausgabe
   (Handy/Laptop/Server/Platine), freie Kerne. Netze ohne Rundruf: `ultar-verbund ziel <ip>`.
3. **Vertrauen:**
   * **Konto:** Geräte desselben Ultar-Kontos (`ultar-verbund konto anmelden <name>`) vertrauen sich automatisch.
   * **Code:** Gerät A `ultar-verbund koppeln` zeigt einen 6-stelligen Code (5 min, 5 Fehlversuche),
     Gerät B `ultar-verbund koppeln <A> <code>`. Beide Seiten beweisen per HMAC-SHA256(Code, Schlüssel A‖B),
     dass sie den Code kennen – ein Mithörer erfährt ihn nicht.
4. **Verbindung:** direkt per TCP 47801, sonst über das Relais. Immer **Noise_XX_25519_ChaChaPoly_BLAKE2s**:
   jede Seite beweist ihren statischen Schlüssel; nicht gekoppelte Geräte werden abgewiesen.
5. **Steuern:** `ultar-verbund ausfuehren <gerät> -- befehl …` (nur Rolle „voll“, abschaltbar).
6. **Bündeln:** `ultar-verbund verteilt -n 1000 -- programm {}` bzw. `--zeilen` (eine Aufgabe je Eingabezeile).
   Jedes Gerät bekommt so viele Arbeiter wie freie Kerne.

## Bildschirm teilen, fernbedienen – auch Geräte ohne Bildschirm

* `ultar-verbund anzeigen <gerät>` öffnet die **Fernansicht im Browser** (jeder Browser; auf Ultar-Geräten
  in den Einstellungen → Geräteverbund → „Bildschirm ansehen“). Maus, Berührung, **langes Drücken**, Wischen,
  Mausrad, Tastatur, Einfügen, Dateien per Ziehen/Ablegen – alles geht zurück an das Gerät.
  Zugriff nur mit dem Schlüssel in der Adresse.
* **Gerät ohne Bildschirm** (Server, Platine): der Dienst startet die Oberfläche als **virtuellen Bildschirm**
  (`ultar-shell --virtuell 1280x800`, Größe über `/etc/ultar/virtuell`) und beendet ihn 90 s nach dem letzten
  Zusehen. Findet `ultar-shell` beim Start keine Anzeige, läuft sie ebenfalls virtuell weiter und prüft alle
  30 s, ob doch ein Bildschirm kommt.
* **Anzeige steuern:** `ultar-verbund bildschirm <gerät> hell 60 | drehen 90 | an | aus | oeffne settings`.
* **Bildschirmfoto:** `ultar-verbund foto <gerät> bild.bmp`.
* Übertragung: 64×64-Kacheln, nur geänderte, RGB + LZ4, im Noise-Kanal (direkt oder über das Relais).
* Das geteilte Gerät zeigt einen Hinweis „Bildschirm wird geteilt“; abschaltbar (`erlauben bildschirm aus`).

## Weitergeben: lange drücken + nach oben wischen

* Irgendwo **lange halten (0,6 s), dann nach oben wischen** → „Weitergeben an …“ mit den gekoppelten Geräten.
* **Offenes Bild/offene Datei:** wird übertragen, am Ziel unter `/data/Empfangen` abgelegt und **sofort geöffnet**
  – dort weiterarbeiten (`ultar-verbund weitergeben <gerät> <datei>`).
* **App:** das Ziel öffnet die **Fernansicht dieses Geräts** – die App läuft weiter, wo sie installiert ist
  (z. B. Handy), wird aber am Ziel (PC) gesehen und bedient. Genauso umgekehrt (PC → Handy).

## Stimme und Ton

* **Sprachbefehle an andere Geräte:** Der Helfer versteht „auf <Gerät>: <Anweisung>“ („on …“, „على …“) und
  reicht sie an den Helfer des Geräts weiter – auch eingelernte Sprachbefehle („Hey KM“) können so z. B.
  den Server ohne Bildschirm steuern.
* **Mithören:** `ultar-verbund hoeren <gerät>` spielt das Mikrofon des Geräts hier ab.
* **Durchsage:** `ultar-verbund sprechen <gerät>` – hier sprechen, dort abspielen.
* Format 16 kHz Mono; Geräte über `ultar-shell --mic-roh/--ton-roh`, sonst ALSA; abschaltbar (`erlauben ton aus`).

## Mining im Leerlauf

* Ungenutzte Geräte schürfen **Monero (XMR, RandomX auf dem Prozessor)** mit XMRig.
* **Ein Klick** (Einstellungen → Geräteverbund → „Auf allen Geräten einrichten“ bzw. `ultar-verbund mining alle`):
  Wallet und Pool auf alle gekoppelten Geräte, XMRig je Gerät einrichten (x86_64: aktuelles Release von GitHub,
  SHA-256 geprüft; ARM: Ubuntu-Paket).
* Läuft **nur**, wenn das Gerät ruht: keine Bedienung seit `leerlauf_min` (Standard 5), keine Verbund-Aufgabe,
  am Strom, unter `max_temp_c` (75 °C, 5 °C Abstand). Jede Bedienung, Verbund-Aufgabe, Akku oder Hitze stoppt
  ihn sofort; niedrigste Priorität (`nice 19`), Standard 75 % der Kerne.
* **Ethereum** ist seit 15.09.2022 Proof-of-Stake und nicht mehr minebar. Für andere Münzen mit GPU-Minern
  (z. B. Ethereum Classic mit lolMiner) gibt es `waehrung = "eigen"` + `eigener_befehl` in `mining.json`.
* Handys werden dabei warm und verbrauchen Strom – deshalb sind „nur am Strom“ und das Temperaturlimit Standard.

## Robustheit (geprüft mit Chaos-Engineering)

* **Idempotent:** jede Aufgabe hat eine Kennung; Wiederholungen beim selben Gerät liefern das gespeicherte
  Ergebnis, statt neu zu rechnen. Ergebnis: **genau ein** Ergebnis je Aufgabe (das erste gewinnt).
* **Lebenszeichen:** während einer Aufgabe alle 2 s `Laeuft`; 8 s Stille = Gerät weg → Aufgabe zurück in die
  Warteschlange, Gerät nach 4 Fehlern in Folge aus dem Lauf genommen.
* **Nachzügler:** wenn die Warteschlange leer ist, werden Aufgaben, die > 3 s laufen, ein zweites Mal gestartet.
* **Ersatz:** fallen alle anderen Geräte aus, rechnet das eigene Gerät den Rest.
* **Belegt:** ein volles Gerät antwortet „Belegt“ statt zu stauen.
* **Relais:** begrenzte Puffer, „unerreichbar“ statt Stau, automatische Neuverbindung mit Backoff.

`tools/verbund-chaos/chaos.sh` (root) baut 4 Geräte im Heimnetz + 1 Gerät in einem fremden Netz + Relais-Server
und prüft bei jedem Lauf unabhängig nach (SHA-256-Kette in Python), dass **alle Ergebnisse korrekt und genau
einmal** vorhanden sind:

| Szenario | Störung |
|---|---|
| entdeckung | Heimnetz per Rundruf, fremdes Netz über Relais, Konto-Vertrauen |
| grundlauf | ohne Störung |
| steuern | Befehl auf Gerät im fremden Netz über Relais |
| kopplung | falscher Code abgewiesen, richtiger gekoppelt |
| absturz | `kill -9` zweier Geräte mitten im Lauf (Dienst startet neu) |
| trennung | Netzkabel zweier Geräte 4 s gezogen |
| relais_weg | Relais-Server stürzt ab und startet neu |
| stoerung | eingebautes Chaos: Verbindungsabbrüche, verfälschte Bytes, Verzögerungen, Abstürze während Aufgaben |
| ausschalten | Nutzer schaltet ein Gerät mitten im Lauf aus; danach unsichtbar |
| totalausfall | alle anderen Geräte weg → eigenes Gerät rechnet allein fertig |
| entkoppelt / rateschutz / fremdkonto / ohne_konto / e2e | Sicherheit: Vertrauen entziehen, Code-Raten, fremdes Konto, ohne Token, kein Klartext auf dem Server |

`tools/verbund-chaos/fern-test.sh` (26 Prüfungen): virtueller Bildschirm auf einem Gerät ohne Bildschirm,
Foto, Öffnen/Zurück/Drehen aus der Ferne, Browser-Ansicht (Schlüssel, Kacheln, Eingaben, Datei, Aktion),
Weitergeben von Bild und App, Mithören/Durchsage bitgenau, Mining startet nur im Leerlauf und stoppt bei
Bedienung und Verbund-Aufgaben, Ethereum ehrlich abgelehnt, fremdes Gerät/abgeschaltetes Teilen abgewiesen,
Oberfläche abgeschossen → Neustart, Netz getrennt → Ansicht verbindet sich selbst neu.

`tools/verbund-chaos/live-test.sh` prüft gegen den **echten Server ultar.code-craft.tech** (zwei Geräte, die
sich nur über das Relais sehen): Konto, Vertrauen, Steuern, 40 Aufgaben, 300-KB-Datei, Absturz mitten im Lauf.

`tools/verbund-chaos/kern-test.sh` startet den **Ultar-Kern** in QEMU (tap-Netz) neben einem Linux-Gerät:
Kern-Erkennung, `ultar-verbund`-Dienst auf dem Ultar-Kern, Kopplung, verteiltes Rechnen auf den Ultar-Kern,
Steuern und harter Kern-Schalter AUS.

Eingebautes Chaos für eigene Tests: `ULTAR_VERBUND_CHAOS="abbruch=0.02,kaputt=0.02,verzoegerung=25,absturz=0.03"`.

## Dateien und Ports

* Daten: `/data/ultar/verbund` (sonst `~/.ultar-verbund`, `ULTAR_VERBUND_DIR`): `schluessel` (0600),
  `verbund.json` (Einstellungen, Partner), `zustand.json` (für Oberfläche/CLI), `koppeln.json` (offener Code).
* Ports: UDP 47800 (Meldungen), TCP 47801 (Noise-Kanal); Relais `wss://ultar.code-craft.tech/v1/verbund/relais`.

## Grenzen

* Ausgeführt werden Programme, die auf dem Zielgerät vorhanden sind (gleiche Architektur bzw. Skripte).
  Verschiedene Architekturen (Handy aarch64, Server x86_64) rechnen gemeinsam, wenn das Programm auf beiden liegt.
* Steuern heißt: Befehle mit den Rechten des Dienstes – deshalb nur gekoppelte Geräte mit Rolle „voll“,
  und auf jedem Gerät einzeln abschaltbar.
