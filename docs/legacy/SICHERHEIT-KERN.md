# Ultar-Kern: kleine Angriffsfläche, wenige Sicherheitsupdates

Ziel: Der Kern bietet von außen **nichts** an, was angegriffen werden kann, und verarbeitet fremde Daten nur in
wenigen, kleinen, geprüften Teilen. Dann gibt es auch wenig, das je ein Sicherheitsupdate braucht.

## Was von außen erreichbar ist

| Weg | Standard | Schutz |
|---|---|---|
| TCP/UDP-Ports | **alles zu** – auch wenn ein Programm lauscht | Brandmauer im Kern verwirft still (kein RST, kein ICMP): Scanner sehen nur „gefiltert“. Öffnen nur ausdrücklich: `/etc/ultar/firewall`, `/proc/ultar/firewall`, Konsole `firewall offen 22` |
| Antworten auf eigene Verbindungen | erlaubt | TCP nur zur bestehenden Verbindung (4-Tupel, zufällige Folgenummern), UDP nur von Gegenstellen, an die das Programm selbst gesendet hat |
| Ping | aus | `ping an` |
| Verbund (47800/47801) | aus | nur solange der Nutzer den Verbund einschaltet; Kanal Noise-verschlüsselt und gegenseitig authentisiert (im Dienst, nicht im Kern) |
| Ethernet/ARP/IPv4/UDP/TCP-Köpfe | werden gelesen | Rust mit Bereichsprüfung, Grenzen gegen Fluten, mit Angriffs-Fuzzing geprüft |

## Härtung im Kern

* **Prozessor:** SMEP (Kern führt nie Benutzercode aus), SMAP (Kern fasst Benutzerspeicher nie direkt an – nur über
  die Seitentabelle), UMIP, NX. Anzeige: `/proc/ultar/sicherheit`, Konsole `sicherheit`.
* **TCP:** Folgenummern und lokale Ports aus RDRAND (nicht vorhersagbar), RST nur mit passender Folgenummer
  (blind eingeschleuste Abbrüche wirkungslos), **SYN-Cookies** bei Überflutung (kein Zustand für gefälschte Absender),
  Grenzen: 4096 Sockets, 64 halboffene je Port, 256 KB Empfangspuffer je Verbindung.
* **ARP:** nur gelernt, was gefragt wurde oder uns direkt anspricht; höchstens 256 Einträge (gegen Fälschung und Flut).
* **Sendewarteschlange** begrenzt (1024); unbekannte Ethertypen, IPv6, Fragmente werden verworfen.
* **Programme:** ELF-Lader prüft Adressen und Größen; kaputte Programme werden abgelehnt oder sterben allein.

## Geprüft

| Test | Ergebnis |
|---|---|
| `tools/kern-test/angriff-test.sh` – Portscan aller 65 535 Ports bei lauschendem Dienst, UDP-/Ping-Proben, 15 000 kaputte Rahmen (12 Arten: Zufall, falsche IHL/Längen/Datenversatz, Fragmente, Riesenrahmen, Müll auf den Verbund-Port …), 10 000 gefälschte ARP, 2 × 10 000 SYN, 3 000 blinde RST | 11/11: nichts offen, nichts antwortet, kein Absturz, geöffneter Port bleibt trotz SYN-Flut erreichbar, Verbindung übersteht RST-Flut, Speicher stabil |
| `tools/kern-test/medien-fuzz.sh` – 12 Starts mit verfälschtem ext4 (Superblock, Gruppen, Inodes, Verzeichnisse), 40 verfälschte ELF-Programme | 2/2: kein Kernabsturz |
| `kerntest`, `ereignis`, `fadentest`, `blocktest`, bash/coreutils, Verbund (`kern-test.sh`) | mit SMEP/SMAP/UMIP unverändert grün |

## Was bleibt (ehrlich)

* **Kein Programm ist fehlerfrei** – ein seltenes Update kann nötig werden: für den Paketleser, die Datenträgerleser
  (ext4/FAT/USB-Sticks: nur bei physischem Zugriff), den ELF-Lader, neue Prozessor-Lücken.
* **Benutzertrennung fehlt noch:** auf dem Ultar-Kern laufen Programme wie root. Ein Schadprogramm müsste aber erst
  auf das Gerät kommen (Apps nur signiert, Netz standardmäßig zu). Nächster Schritt: Rechte je Programm.
* **Meltdown-artige Lücken alter Intel-Prozessoren** (vor 2018): der Kern ist im Adressraum der Programme eingeblendet
  (ohne Zugriffsrecht). Auf betroffenen CPUs hilft erst eine Trennung der Seitentabellen (KPTI) – noch offen.
* Programme im Benutzerbereich (Browser, Dienste) brauchen weiter eigene Updates – die Brandmauer hält sie aber
  vom Netz fern, solange sie keinen Port öffnen.
