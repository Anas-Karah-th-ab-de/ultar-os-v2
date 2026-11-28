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

## Sicherheit P0 – behoben am 28.11.2026

| # | Befund | Korrektur | Nachweis |
|---|---|---|---|
| S-8 | `noexec` vom Ultar-Kern ignoriert | Einhängungen tragen `read_only` und `no_exec`; `MS_NOEXEC` bei mount/remount, ein bind behält, was die Quelle verbietet; `execve` (Programm, Skript, Interpreter) → `EACCES`, der ELF-Lader wird nie von dort gelesen, `mmap(PROT_EXEC)` einer solchen Datei → `EPERM`; `/proc/mounts` zeigt `noexec` | `zelltest` (11 neue Prüfungen in der Zelle), QEMU: zelltest, kerntest, rechtetest ALLES OK |
| S-4 | `uinput`-Geräte sahen wie Hardware aus | der Kern meldet für jedes `uinput`-Gerät `BUS_VIRTUAL`, egal was der Dienst angibt, und liefert `device/id/bustype` wie Linux; devd und die Sitz-Regel des HAL erkennen den virtuellen Bus auch ohne Verknüpfung | `bttest uinput`, `geraetetest` (QEMU ALLES OK), devd-Test mit dem Ultar-Aufbau |
| S-7 | System-UI-Rechte als App-Berechtigung | `ui.shell`, `ui.input-method`, `ui.lock-screen`, `ui.system-dialog` vergibt kein Profil mehr (auch nicht `app.system`); der Compositor nimmt aus der Datei des Starters nur `ui.capture`, `ui.overlay`, `ui.client-header` | Tests `not_even_a_system_app_gets_the_system_ui`, `the_launchers_file_never_gives_the_system_ui` |
| S-5 | Anruf-Bildschirm bildschirmfüllend über der PIN-Eingabe | gesperrt nur ein Band oben (30 %), darunter gehören Bild und Berührung der Sperre; Sperren/Entsperren meldet die Größe neu | `call_screen_is_a_band_above_the_lock_…` (Tipp unter dem Band erreicht nur die Sperre) |
| S-10 | firstboot `.users.tmp` folgte Verknüpfungen | alte Datei entfernen, `O_EXCL` + `O_NOFOLLOW`, `fchown` am offenen Deskriptor | Installer-Tests |
| S-12 | `window_action` während der Sperre | abgelehnt (`Permission`) | `no_window_action_under_the_lock` |
| R2-3, S-9 | Gruppen-Ende per pid (Wettlauf), je App statt je Rolle, 5 s Warten unter der Sperre | Ultar-Kern hat `cgroup.kill` (ganze Untergruppe unter einer Sperre der Prozesstabelle, nicht für Wurzel und feste Äste); jede Rolle eigene Gruppe `apps/<app>/<rolle>`; hartes Ende nach dem Loslassen der Sperre; `ultar-init` (`kill_all`) wirkt jetzt auch auf dem Ultar-Kern | `grenztest grenzen` (QEMU ALLES OK), Starter-Tests |
| R2-2 | Signierer nur 64 Bit des Schlüssel-Hashs | voller SHA-256 als Signierer (Zustandsformat 3, Format 2 wird gelesen; Kurz-Kennung nur noch einmal gegen den eigenen Schlüssel) | `only_the_same_signer_replaces_…`, `format_2_is_read_…` |
| R2-4 | Telefon-Link `org.ultar.settings` ungültig, `app.phone` ohne `launcher.start` | Kennung `settings`, ohne URI (Einstellungen haben noch kein Schema), Regel und Manifest um `launcher.start` ergänzt | Telefon-Tests |
| R2-5 | langsamer Start blockiert den Busfaden | `start` läuft auf vier dauerhaften Start-Fäden (Stufe 1 endet mit ihrem Faden), bei voller Schlange wie bisher | Starter-Tests |
| neu | `mprotect` ohne Wirkung (kein RELRO, kein W^X, keine Schutzseiten) | setzt Schreiben/Ausführen genau, auch für reservierte Bereiche; Kopie einer `noexec`-Datei wird nie ausführbar (Kennzeichen in der Seitentabelle, bleibt über `fork`), geteilte Seiten bekommen kein neues Recht (`EACCES`); `PROT_NONE` bleibt lesbar (x86) | neuer `schutztest` im CI-Tor (QEMU ALLES OK), glibc-Programme und Fäden laufen |
| S-11 | beliebige HCI-Herstellerbefehle | OGF `0x3F` → `EPERM`; die Test-Gegenstelle nutzt einen Normbefehl | `bttest hci`, `bt-e2e` (BT ALLES OK) |
| S-13 | `--input-ack` im ausgelieferten Compositor | nur mit Merkmal `test-hooks` (desktop-e2e baut es), Datei ohne Verknüpfung folgen, 0600 | Compositor-Tests mit und ohne Merkmal |
| S-15 | `ui.client-header` | nur `app.system` kann es bekommen (Starter-Profil), seit S-7 geprüft | Profil-Tests |
| Kern | `clone(CLONE_THREAD)` ohne `CLONE_VM` | `EINVAL` wie Linux | `schutztest` |

Noch offen dazu: der Anruf-Bildschirm erscheint weiterhin ohne Bindung an ein „klingelt“-Ereignis von `ril`/`calld`; `mprotect` ändert Rechte nur auf dem eigenen Kern, ohne Abgleich der anderen Prozessorkerne (wie schon `munmap`); Bluetooth-Tastaturen über `btd` werden jetzt wie alle virtuellen Geräte abgewiesen, bis devd/Compositor sie über eine feste Kennung des Erzeugers zulassen und Sicherheitstasten nur von Hardware nehmen.

## Welle 8 – offen nach der Integration (Stand 24.11.2026)

Behoben in `integ/wave8`: S-1 (Sperre nur durch den Besitzer der Sitzung), S-2 (kein offener „Zwilling“ eines
gesicherten WLANs), S-3 (Rechte-Bit 64 und Opcode 30/41 doppelt belegt → Anruf-Bildschirm 128, eigene Kopfzeile
256, Opcode 42), S-6 (PIN nach 10 Fehlversuchen gesperrt bis zum Kennwort; unlesbare Zählerdatei = Wartezeit für
alle), S-16, L-1…L-11 und R2-1 des App-Starters, Kennungs-Kollision 813 (calld → 829). Berichte:
`docs/review/launcher-runde-2.md`, `docs/review/security-runde-welle-8.md`, `docs/review/integration-welle-8.md`.

| # | Bereich | Befund | Was nötig ist |
|---|---|---|---|

Leichter: S-14
(`ultar.profile` unter Linux aus der Bootzeile) · der Starter kann
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
