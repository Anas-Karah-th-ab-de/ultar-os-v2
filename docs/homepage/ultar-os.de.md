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

## Was als Nächstes kommt

Vollständige Treiber für die zwei Testrechner (Grafik, WLAN, Ton), eine neue Oberfläche, in der jede App in
einem eigenen abgeschotteten Prozess läuft, und ein Installer auf dem Niveau der bekannten Desktop-Systeme.

KM Code Craft · Mohamad Anas Kara Mohamad · info@code-craft.tech
