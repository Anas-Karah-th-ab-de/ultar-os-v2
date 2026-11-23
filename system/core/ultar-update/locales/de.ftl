# ultar-update – Deutsch

update.state.idle = Das System ist aktuell
update.state.checking = Suche nach Aktualisierungen …
update.state.available = Eine Systemaktualisierung ist verfügbar
update.state.downloading = Aktualisierung wird geladen …
update.state.paused = Das Laden ist unterbrochen und kann fortgesetzt werden
update.state.staged = Die Aktualisierung ist bereit zur Installation
update.state.applying = Aktualisierung wird installiert …
update.state.reboot_required = Gerät neu starten, um die Aktualisierung abzuschließen
update.state.verifying = Die neue Fassung wird nach dem Neustart geprüft …

update.error.io = Lesen vom oder Schreiben auf den Speicher ist fehlgeschlagen
update.error.malformed = Die Aktualisierungsdatei ist beschädigt oder keine Ultar-Systemaktualisierung
update.error.no_trusted_keys = Auf dem Gerät fehlen vertraute Schlüssel zur Prüfung von Aktualisierungen
update.error.unknown_key = Die Aktualisierung ist mit einem Schlüssel signiert, dem dieses Gerät nicht vertraut
update.error.bad_signature = Die Signatur der Aktualisierung stimmt nicht; es wurde nichts installiert
update.error.expired = Diese Aktualisierung ist abgelaufen; bitte nach einer neueren suchen
update.error.wrong_arch = Diese Aktualisierung ist für einen anderen Prozessortyp gebaut
update.error.wrong_edition = Diese Aktualisierung ist für eine andere Ausgabe des Systems gebaut
update.error.wrong_device = Diese Aktualisierung ist nicht für dieses Gerät bestimmt
update.error.wrong_channel = Diese Aktualisierung gehört zu einem anderen Kanal als dem gewählten
update.error.payload_set = Der Inhalt der Aktualisierung passt nicht zur Aufteilung dieses Geräts
update.error.rollback_refused = Die Aktualisierung wurde abgelehnt, weil sie älter ist als die Sicherheitsstufe des Geräts
update.error.needs_intermediate = Zuerst muss eine Zwischenfassung installiert werden
update.error.up_to_date = Diese Fassung ist bereits installiert
update.error.downgrade_refused = Diese Fassung ist älter als die laufende
update.error.hash_mismatch = Die geladenen Daten passen nicht zur signierten Aktualisierung; sie wurden gelöscht und werden neu geladen
update.error.read_back = Der Speicher hat die Daten nicht so behalten, wie sie geschrieben wurden; das laufende System ist unverändert
update.error.size_exceeded = Die Aktualisierungsdatei ist größer als ihre signierte Größe; sie wurde abgelehnt
update.error.truncated = Das Laden wurde vor dem Ende abgebrochen und kann fortgesetzt werden
update.error.rotation_refused = Die neuen Aktualisierungsschlüssel konnten nicht angenommen werden
update.error.state_corrupt = Die auf dem Gerät gespeicherten Schutzdaten sind beschädigt; Aktualisierungen sind zur Sicherheit angehalten
update.error.no_space = Nicht genug freier Speicher zum Laden der Aktualisierung
update.error.not_staged = Keine geladene Aktualisierung ist bereit zur Installation
update.error.busy = Es läuft bereits ein Aktualisierungsvorgang
update.error.cancelled = Der Vorgang wurde abgebrochen
update.error.interrupted = Der Vorgang wurde unterbrochen
update.error.no_bootable_slot = Es gibt keine startbare Systemfassung
update.error.boot_control = Die Startdaten konnten nicht gelesen werden
update.error.previous_unavailable = Es gibt keine vorige Fassung, die gestartet werden kann
update.error.no_update = Für dieses Gerät gibt es keine Aktualisierung
update.error.metered = Die Verbindung ist volumenbegrenzt; das Laden darüber erlauben oder ein anderes Netz verwenden
update.error.permission_denied = Für diese Aktion sind Verwalterrechte nötig
update.error.auth_required = Für diesen Kanal ist eine Anmeldung nötig
update.error.not_entitled = Ihr Konto hat keinen Zugang zu diesem Kanal
update.error.unknown_channel = Der Aktualisierungskanal existiert auf dem Server nicht
update.error.rate_limited = Der Server ist ausgelastet; bitte später erneut versuchen
update.error.network = Der Aktualisierungsserver ist nicht erreichbar
update.error.unhealthy = Die neue Fassung hat nicht richtig funktioniert; ein Neustart kehrt zur vorigen Fassung zurück
update.error.invalid_argument = Ungültiger Wert
update.error.boot_failed = Die neue Fassung ist nicht gestartet; das Gerät ist von selbst zur vorigen Fassung zurückgekehrt
update.error.tls_unavailable = Der Prozessor dieses Geräts kann die verschlüsselte Verbindung zum Aktualisierungsserver nicht herstellen; bitte eine Aktualisierung vom USB-Stick verwenden

update.history.staged = Aktualisierung geladen und geprüft
update.history.applied = Aktualisierung installiert, wartet auf Neustart
update.history.installed = Systemaktualisierung abgeschlossen
update.history.rolled_back = Das System ist zur vorigen Fassung zurückgekehrt
update.history.failed = Der Aktualisierungsvorgang ist fehlgeschlagen
update.history.previous_selected = Vorige Fassung für den nächsten Start gewählt
update.history.cancelled = Aktualisierung abgebrochen
update.history.keys_rotated = Prüfschlüssel für Aktualisierungen ersetzt

update.notes.generic = Verbesserungen und Korrekturen für das System

update.cli.usage = Aufruf: uupdate [--socket PFAD] [--lang ar|en|de] [--no-wait] BEFEHL
        Befehle: status · check · download [--metered] · apply · cancel · history [N]
        policy · policy set SCHLÜSSEL=WERT… · offline PFAD [--allow-downgrade] · previous
        verify DATEI.uup --keys ORDNER [--state ORDNER]
update.cli.current = Laufende Fassung: {version} (Slot {slot})
update.cli.channel = Kanal: {channel}
update.cli.pending = Wartende Fassung: {version}
update.cli.previous = Vorige Fassung: {version} (startbar)
update.cli.previous_none = Keine vorige Fassung startbar
update.cli.floor = Sicherheitsstufe: {index}
update.cli.critical = Wichtige Sicherheitsaktualisierung
update.cli.progress = {percent} %
update.cli.accepted = Auftrag angenommen
update.cli.no_bus = Der Aktualisierungsdienst ist nicht erreichbar
update.cli.timeout = Der Aktualisierungsdienst hat nicht rechtzeitig geantwortet
update.cli.verify_ok = Die Datei ist unversehrt und signiert: Fassung {version}
update.cli.history_empty = Der Verlauf ist leer
update.cli.history_line = {time} · {what} · {version}
update.cli.policy.automatic = Automatische Aktualisierung: {value}
update.cli.policy.window = Zeitfenster (UTC): {start}–{end}
update.cli.policy.window_any = Zeitfenster: jederzeit
update.cli.policy.metered = Laden über volumenbegrenzte Verbindungen: {value}
update.cli.policy.rate = Geschwindigkeitsgrenze: {rate} KiB/s
update.cli.policy.rate_unlimited = Geschwindigkeitsgrenze: keine
update.cli.yes = ja
update.cli.no = nein

update.daemon.usage = Aufruf: ultar-updated --config DATEI [--socket PFAD] [--log-socket PFAD] [--lang ar|en|de]
update.daemon.config = Die Konfigurationsdatei ist ungültig (Zeile {line}): {detail}
update.daemon.bus = Verbindung zum Systembus fehlgeschlagen
update.daemon.ready = Aktualisierungsdienst bereit
update.daemon.hardening_failed = Der Aktualisierungsdienst konnte sich beim Start nicht einschränken: {detail}

update.sign.usage = Aufruf: uupdate-sign BEFEHL [Optionen]
        keygen --secret DATEI --public DATEI
        manifest --out DATEI --version X.Y.Z --rollback-index N --channel NAME --edition NAME --arch NAME
                 --devices LISTE --min-version X.Y.Z --notes-key SCHLÜSSEL --created SEKUNDEN --expires SEKUNDEN [--critical]
                 --payload ROLLE=DATEI[:VERITY-WURZEL]…
        sign --manifest DATEI --key GEHEIMDATEI --out DATEI
        rotate --serial N --created SEKUNDEN --key GEHEIMDATEI --out DATEI --public DATEI…
        bundle --signed DATEI --out DATEI.uup [--rotation DATEI]… --payload ROLLE=DATEI…
        verify --bundle DATEI.uup --keys ORDNER
update.sign.wrote = Geschrieben: {path}
update.sign.verified = Signatur gültig: Fassung {version}, Rückfall-Index {index}
update.sign.key_id = Schlüsselkennung: {id}
update.sign.failed = Fehlgeschlagen: {detail}
