# Der Helfer (schwebender Begleiter)

Eine kleine Figur, die über **jeder** Seite der Oberfläche liegt. Code: `crates/ultar-shell/src/app_buddy.rs`,
Link-Prüfung: `crates/ultar-shell/src/linkcheck.rs`.

## Bedienung

| Geste | Wirkung |
|---|---|
| antippen | Feld öffnet/schließt sich |
| ziehen | Figur an eine andere Stelle setzen (gleitet an den nächsten Rand, Platz wird gemerkt) |
| „Wegschicken" | Figur verschwindet; am Rand bleibt eine schmale Lasche |
| Lasche antippen | Helfer kommt zurück |

Die Figur zeigt eine Zahl, wenn auf der sichtbaren Seite Web-Adressen stehen (die Oberfläche merkt sich beim
Zeichnen alle Texte, die wie eine Adresse aussehen). Im Feld stehen diese Links mit einem Knopf „Prüfen".
Ins Eingabefeld kann man auch selbst einen Link einfügen oder eine Frage schreiben; Fragen gehen an den
Agenten, sobald er eingerichtet ist.

## Link-Prüfung in der abgeschotteten Box

1. **Adresse ansehen** (ohne Netz): unverschlüsselt, Zahlen-Adresse, `@`-Trick, täuschend ähnliche Buchstaben
   (Punycode), Endungen mit hohem Betrugsanteil, Kurzlink-Dienste, nachgeahmte Marken
   (z. B. `paypal.com.secure-login.xyz`, `paypa1-…`), Programmdateien (`.apk`, `.exe` …), Lockwörter.
2. **Link verfolgen** in einem eigenen Prozess `ultar-shell --linkprobe <adresse>`:
   - läuft als Benutzer `nobody`, darf keine Datei schreiben, keine Prozesse starten, höchstens 256 MB
     und wenige Sekunden laufen (`setrlimit`, `alarm`, `PR_SET_NO_NEW_PRIVS`), bekommt keine Umgebungsvariablen;
   - holt nur die **Kopfzeilen** (HEAD) über unverschlüsseltes HTTP, folgt bis zu 6 Weiterleitungen,
     führt nichts aus und speichert nichts;
   - greift nie auf Adressen im eigenen Netz oder auf das Gerät selbst zu;
   - endet an der ersten https-Station: der Name der Zielseite ist dann bekannt, ohne sie zu laden.
3. **Urteil**: „Sieht sicher aus" / „Vorsicht" / „Wahrscheinlich gefährlich" mit den Gründen. Das Ziel der
   Weiterleitung zählt: ein Kurzlink auf eine unauffällige https-Seite ist in Ordnung, einer auf eine
   Nachahmung ist gefährlich.

Grenzen: Es ist eine Einschätzung aus Adresse und Weiterleitungen, keine Prüfung des Seiteninhalts und kein
Abgleich mit Sperrlisten. https-Seiten werden nicht geladen (kein TLS in der Box).

## Bewegung und Verhalten

Der Helfer steht nicht starr an seinem Platz (`app_buddy.rs`, Abschnitt „Figur"):

- **Gleiten und Laufen**: Jede Ortsänderung (zum Feld, an den Rand nach dem Ziehen, zum Anruf in die Mitte)
  wird weich gefahren; unterwegs macht er kleine Schritte, der Schatten bleibt am Boden.
- **Fühler** auf dem Kopf schwingt der Bewegung hinterher; die Spitze leuchtet, wenn er etwas zu sagen hat.
- **Blick**: zur Berührung, in Laufrichtung, beim Prüfen nach oben; nach einem Ausflug sieht er sich um.
  Er atmet, blinzelt, ist nachts müde.
- **Ausflüge**: etwa alle ein bis zwei Minuten läuft er kurz an eine andere Stelle oder hüpft – nicht über
  Kamera/Bildern, nicht beim Tippen, nicht bei gesperrtem Gerät. Die Oberfläche wird dafür nicht extra
  geweckt (Takt der Minutenaktualisierung), es kostet also keinen Akku im Leerlauf.
- **Reaktionen**: Sprung beim Antippen, nach einer erledigten Anweisung und bei einer neuen Erinnerung;
  Begrüßung mit der nächsten Erinnerung, wenn der Bildschirm länger als 30 Minuten aus war.

## Erinnerungen und Termine: er ruft an

Anlegen im Feld des Helfers (Schicht 1, ohne Sprachmodell, de/en/ar):

    Erinnere mich in 10 Minuten an Mama anrufen
    Erinnere mich um 17:30 an den Zahnarzt        (Uhrzeit vorbei → morgen)
    Termin um 14 Uhr Besprechung
    remind me at 9 to take my pills      ·      ذكرني بعد ٥ دقائق الاتصال بأحمد
    meine Erinnerungen      ·      lösche alle Erinnerungen

Gespeichert in `/data/ultar/reminders.tsv` (`Zeit<TAB>Versuche<TAB>Text`), übersteht Neustarts.
Termine aus der Kalender-App (mit Erinnerung) sagt er ebenfalls an.

Ist etwas fällig, **ruft er an**: Bildschirm geht an (auch über dem Sperrbildschirm), er kommt in die
Mitte, wird groß, klingelt mit eigener Tonfolge und vibriert (Ruhemodus/Profil werden beachtet), die Seite
dahinter ist abgedunkelt und gesperrt. Karte mit dem Text und zwei Antworten: **Fertig** oder **In 10 Min**.
Ohne Antwort nach 45 s: zweimal nach je 5 Minuten erneut, dann Benachrichtigung „Verpasste Erinnerung".
Während eines Telefonats wartet er bis zum Auflegen.

Genauigkeit: bei eingeschaltetem Bildschirm sofort, bei ausgeschaltetem zur vollen Minute.

## Vorstellung und Führung

Beim ersten Start (und auf „Rundgang" / „Wer bist du?" / «من أنت») stellt er sich vor –
„Ich bin KM Code Craft, dein persönlicher Assistent. Ich arbeite auf deinem Gerät, behalte deine Daten
hier und halte sie sicher." – und führt in acht Stationen durch das System: Er geht jeweils an die Stelle,
um die es geht (oberer Rand = Schnelleinstellungen, unten = Leiste …), daneben die Karte mit „Weiter" /
„Überspringen". Gemerkt wird das im vierten Feld von `/data/ultar/buddy`.

## Er kennt die Hilfeseiten

Fragen wie „Wie funktioniert der Scanner?" beantwortet er aus `help.rs` (dieselben Texte wie das
Hilfe-Zentrum, in der eingestellten Sprache) und bietet „Hilfeseite öffnen" an. Neue Themen dort:
`scanner`, `helper`. Wer eine neue Funktion baut, trägt sie in `help.rs` ein – dann kennt der Helfer sie.

## Sprache: Weckwort, geheimes Wort, eigene Befehle

Alles im Rust-Kern, ohne Netz und ohne Sprachmodell (`mic.rs`, `voice.rs`, `app_voice.rs`).
Es erkennt **eingelernte Wörter und Sätze des Besitzers** – keine freie Spracherkennung, keine Sprachausgabe
(Antworten kommen als Karte in der Mitte und als kurzer Ton).

Einrichten im Feld des Helfers:

    Sprache einrichten                       Weckwort „Hey KM" 3×, dann den Vorlesesatz 2× (Stimmbild)
    geheimes Wort                            3× sprechen – damit hört er auch bei gesperrtem Bildschirm
    Lerne Sprachbefehl Taschenlampe an       3× sprechen – der Text ist die Anweisung, die er dann ausführt
    Sprache aus                              hört nicht mehr zu

Benutzen: „Hey KM" (entsperrt) oder das geheime Wort (immer) → Ton und „Ich höre …" → innerhalb von 8 s
einen eingelernten Befehl sagen. Gesperrt führt er nur Unkritisches aus (Uhrzeit, Erinnerungen, harmlose
Schalter), sonst „Bitte erst entsperren".

Fremde Stimme: Passt das Wort, aber nicht die Stimme (Klangfarbe und Tonhöhe), weist er ab – „Du bist nicht
mein Besitzer". Ist es doch der Besitzer (Erkältung, heiser), weist er sich am Sperrbildschirm mit
**Fingerabdruck, Gesicht oder PIN** aus; danach hört der Helfer 8 s zu. Ist eine Sperre eingerichtet, zeigt
er dafür von selbst den Sperrbildschirm.

Grenzen, ehrlich:
- Die Stimmprüfung ist Bequemlichkeit, kein Schloss – eine gute Aufnahme kann sie täuschen.
- Schwellen stammen aus der Streuung der eingesprochenen Muster; mit echten Stimmen noch nicht abgestimmt.
  Jede gehörte Äußerung schreibt ihre Messwerte nach `/run/ultar/voice.log` (Abstand je Satz, Stimme, Tonhöhe).
- Lauschen heißt: Mikrofon und Tonchip bleiben an. Ohne geheimes Wort nur bei eingeschaltetem, entsperrtem
  Bildschirm; mit geheimem Wort dauernd (kostet Akku).
- Dateien: `/data/ultar/voice.tsv` (Muster), `/data/ultar/voice.on` (eingeschaltet).

Mikrofon (A30): MIC1 → COD3035-ADC → UAIF0 → NSRC0 → WDMA1 = `/dev/snd/pcmC0D9c`, 48 kHz Stereo, in der
Oberfläche auf 16 kHz Mono gebracht. Stolperstein: Der Puffer muss ein ganzes Vielfaches der Periode sein
(960 × 8 Rahmen), sonst läuft der Lesezeiger davon und man liest alten Pufferinhalt statt Ton.
Prüfen: `ultar-shell --mic 3000 /data/ultar/mic.wav` (Pegel je Viertelsekunde), Regler ansehen/setzen und
roh aufnehmen mit `tools/a30-mic/micprobe`.

## Prüfen über die Tür

    echo "buddy de"      > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # Feld offen
    echo "buddycheck de" > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # ersten Link der Seite prüfen
    echo "buddyaway de"  > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # weggeschickt
    echo "buddytour de"  > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # Führung (Station aus /run/ultar/buddy.say)
    echo "buddycall de"  > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # Anruf zeigen (Text aus /run/ultar/buddy.say)
    echo "buddysay de"   > /run/ultar/shell.goto; kill -USR1 $(pidof ultar-shell)   # Text aus /run/ultar/buddy.say ausführen
    ultar-shell --linkprobe http://bit.ly/…                                          # Box direkt

Tests: `cargo test -p ultar-shell linkcheck::` und `mind::`
