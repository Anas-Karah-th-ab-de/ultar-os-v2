# Ultar OS – ein unabhängiges Betriebssystem, Arabisch zuerst

**Ultar OS** entsteht bei **KM Code Craft** von Grund auf: ein eigener Kernel in Rust, eine arabische
Oberfläche von rechts nach links und eine Codebasis für Handy, Rechner und Server.

## Was es unterscheidet

- **Eigener Kernel.** Der Ultar-Kern ist in Rust geschrieben und läuft ohne Linux. Im Emulator ist er nach rund 145 ms bereit.
- **Arabisch zuerst.** Jeder Text entsteht zuerst auf Arabisch: die sechs arabischen Mehrzahlformen, der
  Hijri-Kalender und eine Suche, die Vokalzeichen und Schreibvarianten übergeht. Ziffern sind immer 0–9.
- **Ein System für jedes Gerät.** Das System richtet sich nach dem, was das Gerät kann: Touch, Akku, Modem,
  zweiter Bildschirm. Den Desktop-Modus gibt es auch auf Handy und Tablet.
- **Treiber im Installationsmedium.** Signierte Treiberpakete werden automatisch gewählt; die Installation geht ohne Internet.
- **Datenschutz und Sicherheit.** Jeder Teil läuft mit den geringsten nötigen Rechten; kein unsignierter Treiber und keine unsignierte App wird installiert.
- **Sieben klar getrennte Bereiche:** Kernel, Treiber, Systemkern, Systemdienste, Plattform, System-Apps, Fremd-Apps.

## Wo wir stehen (22.11.2026)

Wir bauen das System auf einem neuen, geordneten Fundament neu auf. Das Fundament ist fertig und geprüft:
Sprachsystem, Geräteprofil, Hardware-Schnittstelle, Systembus und Treiberpakete. 842 automatische Tests laufen
ohne Fehler, und der Systembus läuft auf dem Ultar-Kern. Oberfläche und Dienste stammen noch aus der vorigen
Fassung und werden phasenweise neu gebaut.

Aktuelle Testgeräte: Laptop Lenovo ThinkPad P1, Mini-PC Beelink GTi15 Ultra und Handy Samsung Galaxy A30.

## Neu seit dem 24.11.2026: Geräteverbund und lokaler Helfer

- **Geräteverbund.** Die eigenen Geräte finden sich, werden einmal mit einem sechsstelligen Code gekoppelt
  und übergeben einander die Arbeit: Was auf einem Gerät offen ist, geht auf dem anderen weiter. Jede
  Verbindung ist Ende-zu-Ende verschlüsselt. Nichts kommt ungefragt an – das Zielgerät fragt immer nach.
  Gebaut sind der Dienst (auch über einen Vermittlungs-Server für Geräte in verschiedenen Netzen), die Seite
  „Verbund“ in den Einstellungen mit Koppeln und Rechten je Gerät, die Kachel in den Schnelleinstellungen,
  „Auf anderem Gerät weitermachen“ im Menü der App und die Frage am Zielgerät. Auf echten Geräten ist der
  Verbund noch nicht erprobt; die Handy-Oberfläche und die Fernansicht fehlen noch.
- **Lokaler Helfer.** Man fragt in Worten, der Helfer handelt über den Systembus – mit eigenen, engen Rechten
  und nie ohne Bestätigung bei Gefährlichem. Er passt sich dem Gerät an: Mit eigenem Sprachmodell denkt er
  selbst; ein schwaches Gerät nutzt das Modell eines gekoppelten Geräts, das es verleiht; ohne beides
  versteht er feste Befehle und beantwortet Fragen zum System auf Arabisch, Englisch und Deutsch. Er kann
  auch den Helfer eines gekoppelten Geräts bitten, dort zu handeln, wenn dieses es erlaubt. Der Dienst ist
  gebaut und getestet; eine eigene Oberfläche und die Sprachsteuerung fehlen noch.

## Was als Nächstes kommt

Vollständige Treiber für die zwei Testrechner (Grafik, WLAN, Ton), eine neue Oberfläche, in der jede App in
einem eigenen abgeschotteten Prozess läuft, und ein Installer auf dem Niveau der bekannten Desktop-Systeme.

KM Code Craft · Mohamad Anas Kara Mohamad · info@code-craft.tech
