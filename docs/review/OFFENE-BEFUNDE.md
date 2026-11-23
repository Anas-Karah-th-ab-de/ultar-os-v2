# Unabhängige Prüfung Runde 1 – Befunde und Stand

Stand 23.11.2026 · Drei Prüfer, die den Code nicht geschrieben haben, haben versucht, ihn zu brechen.
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
| Server | Kennwort-Hashing ohne Obergrenze (Speicher), VPN-Adressen von einem Konto aufbrauchbar, Token gelöschter Konten weiter gültig und auf das nächste Konto übertragbar, Anmelde-Zeit verriet Konten | schwer |

## Offen – sperrt den Einsatz

| # | Bereich | Befund | Was nötig ist |
|---|---|---|---|
| S15 | Server/Gerät | Geräte-Kennung ohne Schlüsselnachweis: wer sie zuerst sendet, besitzt sie; ohne Android-Seriennummer haben alle Geräte dieselbe Kennung | Geräteschlüssel erzeugen, Kennung = SHA-256 des Schlüssels, signierte Server-Aufgabe |
| S16 | Gerät | System-Abbilder vom Server werden am Gerät noch nicht geprüft (kein Updater vorhanden) | Prüfung von Wurzel-Hash und Signatur + Schutz vor älterer Fassung, bevor ein Updater diesen Weg nutzt |
| D1 | Kern ↔ devd | Der Kern bietet keine Geräte-/Treiber-Liste und kein Binden/Lösen; devd liest das Protokoll | sechs Kern-Schnittstellen (docs/integration/ultar-devd-daemon.md §2) |
| B | Dienste | kein Dienst behandelt Signale, kein Protokolldienst, Vermittler ohne Kennung des Gegenübers (`SO_PEERCRED`), beibehaltene Themen nach Neustart nicht erneut gesendet | Dienst-Rahmen: Start-Reihenfolge über `ultar-init`, Stop-Signal, Protokoll, Kennung aus dem Kern |
| U | Baukasten | Entwurfsbilder und Gestaltungswerte widersprechen sich an 20 Stellen (13-px-Schrift, 36 hohe Elemente, Schalter 44×24 …); keine fette arabische Schrift, Naskh/Kufi fehlen | Runde 2 der Bilder aus `design/tokens.toml` erzeugen; Schriften beschaffen (Lizenz prüfen) |

## Offen – leichter (vor Freigabe der jeweiligen Stufe)

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
