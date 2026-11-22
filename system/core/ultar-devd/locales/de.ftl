# ultar-devd / udevctl – Deutsch.
# Positionsargumente von Bus-Texten heißen der Reihe nach {a}, {b}.

devd.phase.detected = erkannt
devd.phase.probing = wird geprüft
devd.phase.active = läuft
devd.phase.no-driver = kein Treiber
devd.phase.failed = läuft nicht
devd.phase.removed = entfernt

devd.reason.no-package = Im Depot gibt es keinen Treiber, der zu diesem Gerät passt.
devd.reason.kernel-only = Der Kern betreibt das Gerät mit seinem Treiber „{a}“, im Depot gibt es dafür aber kein Treiberpaket.
devd.reason.not-loadable = Das Treiberpaket braucht ein ladbares Kernmodul; das unterstützt dieser Kern noch nicht.
devd.reason.not-bound = Der Kern hat seinen Treiber „{a}“ beim Start nicht an dieses Gerät gebunden; im laufenden Betrieb lässt er sich nicht aktivieren.
devd.reason.package-missing = Die Datei des Treiberpakets ist auf diesem System nicht vorhanden.
devd.reason.rejected-detail = Das Treiberpaket wurde abgelehnt: {a}
devd.reason.probe-failed = Der Kerntreiber konnte das Gerät nicht einrichten: {a}
devd.reason.load.package-rejected = Das Treiberpaket wurde abgelehnt, weil Signatur oder Inhalt nicht unversehrt sind.
devd.reason.load.incompatible-kernel = Der Treiber passt nicht zum laufenden Kern.
devd.reason.load.init-failed = Der Treiber konnte das Gerät nicht einrichten.
devd.reason.load.timeout = Zeitüberschreitung beim Laden des Treibers.
devd.reason.load.unsupported = Dieser Treiber lässt sich mit dem laufenden Kern nicht verwenden.
devd.reason.health.no-response = Das Gerät antwortet mit diesem Treiber nicht.
devd.reason.health.self-test-failed = Der Selbsttest des Geräts ist mit diesem Treiber fehlgeschlagen.
devd.reason.health.timeout = Zeitüberschreitung bei der Geräteprüfung.
devd.reason.health.unstable = Das Gerät arbeitet mit diesem Treiber nur zeitweise.
devd.reason.unknown = Für dieses Gerät funktioniert kein Treiber.
devd.detail.wrong-package = die Datei enthält einen anderen Treiber oder eine andere Version als im Index steht
devd.detail.bad-activation-record = der Aktivierungseintrag im Paket ist ungültig

devd.error.unknown-device = Unter der Adresse {a} gibt es kein Gerät.
devd.error.not-a-candidate = Der Treiber {b} passt nicht zum Gerät {a} oder ist nicht im Depot.
devd.error.quarantined = Der Treiber {b} ist für das Gerät {a} in Quarantäne. Zuerst die Quarantäne aufheben.
devd.error.already-active = Der Treiber {a} ist bereits in Verwendung.
devd.error.nothing-to-roll-back = Für das Gerät {a} gibt es keinen früheren Treiber.
devd.error.wrong-phase = Nicht möglich, solange das Gerät {a} in diesem Zustand ist: {b}.
devd.error.unsupported = Das Gerät {a} lässt sich nicht auf den Treiber {b} umstellen: Der laufende Kern bietet keine Möglichkeit, seine eingebauten Treiber im Betrieb zu aktivieren oder abzuschalten.
devd.error.transition = Interner Fehler im Gerätedienst.
devd.error.bad-request = Die Anfrage ist ungültig.

devd.notice.keys-missing = Es gibt keine vertrauenswürdigen Schlüssel; kein Treiber kann geprüft werden.
devd.notice.keys-unreadable = Die Schlüsseldatei {a} war nicht lesbar und wurde übergangen.
devd.notice.depot-unreadable = Das Treiber-Depot ist nicht verwendbar: {a}
devd.notice.hardware-unreadable = Die Geräteliste war nicht lesbar: {a}
devd.notice.state-recovered = Die Zustandsdatei war beschädigt ({a}); der Dienst ist mit leerem Zustand gestartet und hat die beschädigte Datei aufbewahrt.
devd.notice.state-unavailable = Die Auswahl konnte nicht gespeichert werden ({a}); sie gilt bis zum Herunterfahren.
devd.notice.publish-failed = Der Gerätezustand konnte nicht auf dem Systembus veröffentlicht werden; andere Dienste kennen die letzten Änderungen eventuell nicht.

devd.store.problem.header = Dateikopf fehlt oder ist ungültig
devd.store.problem.length = Datei ist abgeschnitten
devd.store.problem.checksum = Prüfsumme stimmt nicht
devd.store.problem.body = Inhalt ist ungültig
devd.store.problem.version = Datei stammt von einer neueren Version
devd.store.problem.too-large = Datei ist größer als erlaubt
devd.store.unavailable = Speicherort nicht verfügbar
devd.store.corrupt = gespeicherter Inhalt nicht verständlich
devd.store.write-failed = Schreiben fehlgeschlagen

hal.hardware.no-source = weder Kern noch System liefern eine Gerätebeschreibung
hal.hardware.io = die Gerätebeschreibung war nicht lesbar
hal.hardware.too-many-devices = mehr Geräte als plausibel
hal.hardware.note.log-wrapped = Das Kernprotokoll ist übergelaufen; einige Geräte fehlen möglicherweise.
hal.hardware.note.bound-only = Der Kern zeigt nur Geräte mit eingebautem Treiber; andere Geräte sind nicht sichtbar.
hal.hardware.note.class-incomplete = Die Klasse einiger Geräte ist nicht genau bekannt; Treiber, die nach Klasse zugeordnet werden, werden eventuell nicht gefunden.
hal.hardware.note.pci-count = Die vom Kern gemeldete Geräteanzahl weicht von der Zahl der beschriebenen Geräte ab.

devd.capability.no-driver = Die Hardware ist vorhanden, aber kein Treiber funktioniert dafür.

devd.start.unknown-kernel = Der laufende Kern ließ sich nicht bestimmen. --kernel-abi verwenden.
devd.start.hardware = Die Systembeschreibung war nicht lesbar: {a}

devd.cli.usage = Aufruf: udevctl [--bus SOCKET] [--lang ar|en|de] list | switch <Adresse> <Treiber> <Version> | rollback <Adresse> | rescan [Adresse] | clear-quarantine [Adresse [Treiber Version]] | shutdown
devd.cli.daemon-usage = Aufruf: ultar-devd --depot <Ordner> --keys <Ordner> --state <Ordner> --bus <Socket> [--root <Ordner>] [--kernel-abi <Name>] [--once] [--lang ar|en|de]
devd.cli.bus-failed = Der Gerätedienst ist nicht erreichbar: {a}
devd.cli.no-devices = Keine Geräte.
devd.cli.device = {address}  {id}  [{phase}]
devd.cli.label = Bezeichnung: {a}
devd.cli.driver = Treiber: {a} {b}
devd.cli.alternative = Alternative: {a} {b}
devd.cli.alternative-quarantined = Alternative in Quarantäne: {a} {b} ({c})
devd.cli.previous = vorher: {a}
devd.cli.reason = Grund: {a}
devd.cli.notice = Hinweis: {a}
devd.cli.kernel-switch-unsupported = Hinweis: Der laufende Kern kann seine eingebauten Treiber im Betrieb nicht umschalten.
devd.cli.done = Erledigt.
devd.cli.restored = Der gewünschte Treiber hat nicht funktioniert; der vorherige wurde wiederhergestellt.
devd.cli.refused = Abgelehnt: {a}
devd.cli.released = {count ->
    [zero] Keine Quarantäne aufzuheben.
    [one] Eine Quarantäne aufgehoben.
   *[other] {count} Quarantänen aufgehoben.
}
devd.cli.added = neu: {a}
devd.cli.removed = verschwunden: {a}
devd.cli.shutdown = Der Gerätedienst wurde beendet.

devd.daemon.started = Gerätedienst läuft: {devices} Geräte, davon {active} mit funktionierendem Treiber.
devd.daemon.stopped = Der Gerätedienst wurde beendet.
devd.daemon.bus-lost = Die Verbindung zum Systembus ist abgebrochen: {a}
