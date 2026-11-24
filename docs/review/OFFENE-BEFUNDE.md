# Unabhängige Prüfungen – Befunde und Stand

Stand 23.11.2026, nach Welle 4 · Drei Prüfer, die den Code nicht geschrieben haben, haben versucht, ihn zu brechen.
Jeder Befund ist durch einen Test oder durch Lesen belegt. **Kein Teil ist damit „produktionsreif“** – die offenen
Punkte unten sperren den Einsatz.

## Behoben (Korrektur + Test im Repo)

| Bereich | Befund | Schwere |
|---|---|---|
| Kern | `mmap(MAP_FIXED)` nahm beliebige Adressen an: eine Zelle ohne Rechte konnte Kern-Speicher überschreiben und den Rechner anhalten | **kritisch** |
| Kern | `munmap` mit riesiger Länge fror den Rechner ein | schwer |
| Kern | `ftruncate`/`lseek`+`write`/`F_DUPFD` mit Riesenwerten → Kern-Panik | schwer |
| Kern | Zelle konnte Dateien unter `/proc` und `/dev` im gemeinsamen Baum anlegen (Kanal zwischen Zellen) | schwer |
| Kern | Ausgaben von Nutzerprogrammen landeten im Kern-Protokoll, das `ultar-devd` als Quelle liest (Geister-Geräte, Treiber-Quarantäne fälschbar) | schwer |
| Bus | 24 stumme Verbindungen auf dem App-Sockel sperrten Systemdienste aus; Handshake-Frist je Lesevorgang | schwer |
| Bus | ein nicht lesender Abonnent ließ den Vermittler um 275 MiB wachsen (jetzt Grenze in Bytes) | schwer |
| Einstellungsdienst | startete nicht, wenn das Journal nach Absturz nicht schreibbar war | schwer |
| Baukasten | deaktivierte Bausteine zu blass (Deckkraft doppelt), Zeichenfehler bei offener Auswahlliste, Pfeiltasten im Textfeld bei gemischter Richtung vertauscht, Auslassungspunkte am falschen Ende | schwer |
| Gerät | Server konnte eine ältere signierte App-Fassung unterschieben; Download ohne eigene Obergrenze | schwer |
| Kern | K5: Speichermangel war eine Kern-Panik – eine Zelle ohne Rechte hielt den Rechner an (Speicher, tmpfs, Pipe, Prozess-Lawine). Jetzt: fehlbare Speicheranforderung, Vorrat des Kerns, begrenzte Pipe, tmpfs-Kontingent, Gruppen mit Grenzen und dynamischem Schutz (`docs/integration/kernel-memory-groups.md`); Prüfung `grenztest` (130 Prüfungen, 512 MiB und 2 GiB) in `ci.sh` | **kritisch** |
| Server/Gerät | S15: Geräte-Kennung ohne Schlüsselnachweis. Jetzt Ed25519-Geräteschlüssel, Kennung aus dem Schlüssel, Nachweis per Aufgabe; Runde 3 (5 schwere) und Runde 4 (2 schwere) gefunden und behoben – Übernahme der Nummer per Kennwort, Entfernen fremder Geräte, Weiterleitung über fremden Server, alte Sockets, Kennwort-Raten, Aussperren des Besitzers, Füllen des Aufgabenspeichers (`docs/DEVICE-IDENTITY.md`, `identity-runde-3.md`) | **kritisch** |
| Gerät | S16: kein Updater. Jetzt `ultar-update`: signiertes Manifest, Schutz vor älterer Fassung, A/B mit Rückfall, Offline-Paket `.uup`; Runde 3 (3 schwere, 5 leichte) behoben (`docs/SYSTEM-UPDATE.md`, `update-runde-3.md`) | **kritisch** |
| Dienste | B: Dienst-Rahmen `ultar-service` (Signale, Grenzen, Bereitschaft, Kennung des Gegenübers), Protokolldienst `ultar-logd`, Vermittler `ultar-busd`; Runde 3 (1 kritischer: Vermittler durch stumme Verbindungen beendbar, 6 schwere) behoben (`services-runde-3.md`) | **kritisch** |
| Kern | R3-K1: Kern hatte keine Benutzerkennungen, keine echten Dateirechte, Sockel ohne Rechteprüfung, kein `SO_PEERCRED`. Jetzt alles vorhanden; Prüfung `rechtetest` (102 Prüfungen) und ext4-Gegenprüfung in `ci.sh`; Wettlauf im Speichermangel-Abbruch bei 4 Kernen behoben | schwer |
| Baukasten | U3: Runde 3 – Dialog sprengte das Zeitbudget, Listen nicht virtuell, Zugänglichkeits-Baum unvollständig, Zustandsgrafik im hellen Thema zu kontrastarm, Menüs am Telefon, fehlende Bausteine; 27 Abweichungen Bild ↔ Baukasten. Alles behoben, Tafeln folgen jetzt dem Baukasten (`ui-runde-3.md`); Nachprüfung durch einen zweiten Prüfer steht noch aus | schwer |
| Oberfläche (alt) | Ein langsames Bild machte aus einem Tipp einen langen Druck (Tipp ging verloren) | schwer |
| Server | Kennwort-Hashing ohne Obergrenze (Speicher), VPN-Adressen von einem Konto aufbrauchbar, Token gelöschter Konten weiter gültig und auf das nächste Konto übertragbar, Anmelde-Zeit verriet Konten | schwer |

## Offen – sperrt den Einsatz

| # | Bereich | Befund | Was nötig ist |
|---|---|---|---|
| A1 | Apps | Apps haben noch keine eigenen Benutzerkennungen; es gibt noch keinen App-Starter | Starter, der jede App als eigenen Benutzer in ihrer Zelle startet und ihre Rechte einträgt |
| S1 | Oberfläche | Desktop-Hülle (Leiste, Startmenü, Schnelleinstellungen), Sperrbildschirm, Systemdialoge und Apps sind noch nicht gebaut; die Sicherheits-Tasten wirken erst mit einem Sperrbildschirm | Desktop-Hülle als Clients des Compositors (Welle 5) |
| G1 | Kern/Grafik | Nur der Bildspeicher der Firmware: ein Bildschirm, vblank nur nachgebildet (60-Hz-Raster), kein Touch-Treiber (USB-Tastatur, -Maus und -Tablet gehen jetzt) | Grafiktreiber (virtio-gpu, Intel), Touch |
| R1 | Root-Dienste | 17 Dienste laufen noch als root (je mit Begründung) | aufteilen in kleine Helfer oder Bus-Aufrufe |
| H1 | Hardware | kein Start ohne KVM direkt auf dem Beelink; ThinkPad nicht getestet; A/B-Start nur in QEMU geprüft | Freigabe und Einschaltknopf vor Ort |
| K6 | Start | kein Zähler gegen ältere Fassung in einer EFI-Variable, kein Hardware-Wachhund nach dem Start | eigene Untersuchung (Firmware-Dienste stürzten in OVMF ab) |

## Offen – leichter (vor Freigabe der jeweiligen Stufe)

Kern: kein `SCM_CREDENTIALS`; fsync schickt keinen Cache-Flush an NVMe/AHCI; Set-ID-Bits beim Start ignoriert ·
Updater: Daemon noch ohne Dienst-Rahmen; nach fehlgeschlagener Gesundheitsprüfung startet niemand neu · statischer ARM64-Bau des Updaters braucht TLS ohne C-Bibliothek ·
Server: Proxy-Grenzen (Caddy) nur als Konfigurationsdatei geprüft ·
Tests: zwei Ablauf-Tests der alten Oberfläche scheitern auf GitHub gelegentlich (Ursache offen, Fehlermeldung zeigt jetzt den Zustand); `ultar-logd` `after_sigkill` einmal gescheitert, nicht nachstellbar · Bildzeit nur auf dem Bau-Rechner gemessen, nicht auf ruhiger Hardware ·
Kern (Rest von K5): einige große Kern-Puffer werden noch ohne Rückweg angefordert (Kopie der epoll-Liste, `/proc`-Listen und
Wecktabelle des Planers je Fadenzahl, Lesen des Bildspeichers, Metadaten der Wirts-Datenträger) – begrenzt und aus dem
Vorrat bedient, Liste in `docs/integration/kernel-memory-groups.md`; kein Prozessor-Anteil je Gruppe; nicht auf echter
Hardware geprüft ·
Bus: Grenzen je Kennung (beibehaltene Themen, Dienstnamen), vorhandene Datei am Sockel-Pfad nicht blind ersetzen ·
Einstellungsdienst: Bestätigung nach Fristablauf, Bestätigung nennt nicht die Änderung, `--allow-all` neben Richtlinie,
Budget für App-Schlüssel, Bildschirm-Kennungen von der Meldereihenfolge abhängig, keine Sperre gegen zweite Instanz ·
devd: Index ohne Seriennummer (alter signierter Index wird angenommen), widerrufener Schlüssel wirkt erst nach Neustart,
`devices.shutdown` für jeden erlaubten Aufrufer · Kern: `clone(CLONE_THREAD)` ohne `CLONE_VM`, `/proc/meminfo` zeigt den
ganzen Rechner · Server: Benutzernamen nicht normalisiert, keine Grenze für Geräte je Konto, Signalisierung ohne
Ratenbegrenzung · Baukasten: 10 kleinere Verhaltensfehler (Schieber mit NaN, Zeilenumbruch-Zeichen im Textfeld …).

Die vollständigen Berichte mit Fundstellen: `docs/review/ultar-ui-runde-1.md`; die Angriffs-Programme liegen unter
`tools/kern-test/attacks/` und in den Testordnern (`*review*`, `*sec_review*`).

## Welle 8 – offen nach der Integration (Stand 24.11.2026)

Behoben in `integ/wave8`: S-1 (Sperre nur durch den Besitzer der Sitzung), S-2 (kein offener „Zwilling“ eines
gesicherten WLANs), S-3 (Rechte-Bit 64 und Opcode 30/41 doppelt belegt → Anruf-Bildschirm 128, eigene Kopfzeile
256, Opcode 42), S-6 (PIN nach 10 Fehlversuchen gesperrt bis zum Kennwort; unlesbare Zählerdatei = Wartezeit für
alle), S-16, L-1…L-11 und R2-1 des App-Starters, Kennungs-Kollision 813 (calld → 829). Berichte:
`docs/review/launcher-runde-2.md`, `docs/review/security-runde-welle-8.md`, `docs/review/integration-welle-8.md`.

| # | Bereich | Befund | Was nötig ist |
|---|---|---|---|
| S-4 | Kern/Compositor | `/dev/ultar/uinput`-Geräte sehen wie Hardware aus (sysfs-Verzeichnis statt Link), Name/Bus/Hersteller frei wählbar | Kern kennzeichnet virtuelle Geräte unfälschbar; Sicherheitstasten nur von Hardware |
| S-5 | Compositor | gesperrt liegt der Anruf-Bildschirm noch bildschirmfüllend über der PIN-Eingabe und bekommt Zeiger/Berührung (entsperrt jetzt unter Systemdialogen) | gesperrt nur ein Band, Eingabebereich der Sperre hat Vorrang; nur bei klingelndem Anruf |
| S-7 | Starter | `ui.lock-screen`, `ui.shell`, `ui.system-dialog` sind App-Berechtigungen (System-Apps per Dialog erteilbar) | diese Rechte nur über die feste Rechtedatei |
| S-8 | Kern | `noexec` wird vom Ultar-Kern ignoriert (App startet Heruntergeladenes) | `noexec` im Kern |
| R2-2…R2-5 | Starter | 64-Bit-Signaturkennung; Gruppen-Ende ohne `cgroup.kill` (pid-Wettlauf, je App statt je Rolle, 5 s unter Sperre); Telefon-Link `org.ultar.settings` ist keine Starter-Kennung, `app.phone` ohne `launcher.start`; ein langsamer Start blockiert den Busfaden | siehe `launcher-runde-2.md` |

Leichter: S-9 (Gruppen-Ende per pid), S-10 (firstboot `.users.tmp` ohne `O_NOFOLLOW|O_EXCL`), S-11 (beliebige
HCI-Herstellerbefehle), S-12 (`window_action` während der Sperre), S-13 (`--input-ack` nur für Tests), S-14
(`ultar.profile` unter Linux aus der Bootzeile), S-15 (`ui.client-header` nur für System-Apps) · der Starter kann
`ui.call-screen`/`ui.client-header`/`ui.output-config` nicht erteilen · „Im Ordner zeigen“ des Browsers ruft
`ultar-files --select`, das Dateien-App noch nicht kennt · boringtun/ring für aarch64 ungeprüft · ultar-check auf
main 7744e78 schon mit 20 Befunden (Kern-Texte, `unsafe` ohne SAFETY in pci.rs) · Arbeitsbereich-clippy über die
CI-Auswahl hinaus scheitert an Altlasten-Crates · ultar-cloud: zwei Tests unter umask 002 · Installer-E2E nur
mit `INSTALLER_E2E=1`.

## Geprüft und in Ordnung

Kern: kein Ausbruch über Pfade, Prozessnummern, Dateideskriptoren oder Rechte (117 Angriffsversuche) ·
Server: kein Zugriff auf Premium-Pakete oder gesperrte Kanäle ohne Berechtigung (71 Varianten), keine SQL-Einschleusung,
kein Absturz bei kaputten Anfragen · Gerät: unsignierte, fremd signierte oder veränderte Pakete nie installiert; Token nie
an einen fremden Rechner gesendet · Einstellungsdienst: kein Nicht-Verwalter kann Verwalter-Einstellungen ändern;
111 Einstellungen × 32 feindliche Werte ohne Datenverlust · Bus: Richtlinien-Leser gegen echtes TOML mit 400 000 Texten
ohne sicherheitsrelevante Abweichung · Baukasten: arabischer Textsatz und Leserichtung an 81 Zeichenketten geprüft, kein
Fehler in der Reihenfolge.
