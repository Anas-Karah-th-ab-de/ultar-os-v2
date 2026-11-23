# Unabhängige Prüfungen – Befunde und Stand

Stand 23.11.2026, nach Runde 3 und 4 · Drei Prüfer, die den Code nicht geschrieben haben, haben versucht, ihn zu brechen.
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
| D1 | Kern ↔ devd | Der Kern bietet keine Geräte-/Treiber-Liste und kein Binden/Lösen; devd liest das Protokoll | sechs Kern-Schnittstellen (docs/integration/ultar-devd-daemon.md §2) |
| K4 | Kern/Start | Updater kann auf dem Ultar-Kern noch nichts installieren: kein Lader mit Slot-Wahl und Startzähler, nur eine Wurzel-Partition, FAT nur lesbar | Änderungen K1–K6 in `docs/SYSTEM-UPDATE.md` |
| I1 | init | `ultar-init` nutzt die Bereitschafts-Meldung nicht, wechselt keine Benutzer, setzt Sitzung/Apps nicht in ihre Gruppen | init auf den Dienst-Rahmen umstellen |

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

## Geprüft und in Ordnung

Kern: kein Ausbruch über Pfade, Prozessnummern, Dateideskriptoren oder Rechte (117 Angriffsversuche) ·
Server: kein Zugriff auf Premium-Pakete oder gesperrte Kanäle ohne Berechtigung (71 Varianten), keine SQL-Einschleusung,
kein Absturz bei kaputten Anfragen · Gerät: unsignierte, fremd signierte oder veränderte Pakete nie installiert; Token nie
an einen fremden Rechner gesendet · Einstellungsdienst: kein Nicht-Verwalter kann Verwalter-Einstellungen ändern;
111 Einstellungen × 32 feindliche Werte ohne Datenverlust · Bus: Richtlinien-Leser gegen echtes TOML mit 400 000 Texten
ohne sicherheitsrelevante Abweichung · Baukasten: arabischer Textsatz und Leserichtung an 81 Zeichenketten geprüft, kein
Fehler in der Reihenfolge.
