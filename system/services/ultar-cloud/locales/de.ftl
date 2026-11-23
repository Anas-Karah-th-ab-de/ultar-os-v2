# ultar-cloud – Meldungen zu Store, Updates und Konto. Referenz: ar.ftl.

# Store
cloud.store.sign_in_required = Melde dich an, um diese App zu erhalten.
cloud.store.not_included = Diese App ist in deinem Zugang nicht enthalten.
cloud.store.not_found = Diese App gibt es im Store nicht.
cloud.store.state.free = Frei
cloud.store.state.premium_sign_in = Premium – Anmeldung nötig
cloud.store.state.premium_not_included = Premium – nicht in deinem Zugang enthalten
cloud.store.state.unlocked = Für dich freigeschaltet
cloud.store.publisher.verified = Geprüfter Herausgeber
cloud.store.empty = Keine passenden Apps.
cloud.store.installed = {name} Version {version} wurde installiert.

# System-Updates
cloud.update.sign_in_required = Melde dich an, um diesen Update-Kanal zu nutzen.
cloud.update.not_included = Dieser Update-Kanal ist in deinem Zugang nicht enthalten.
cloud.update.not_found = Diesen Update-Kanal gibt es nicht.
cloud.update.available = Ein Update ist verfügbar: Version {version} (Kanal {channel}).
cloud.update.none = Das System ist aktuell.

# Allgemeine Fehler
cloud.error.auth_required = Dafür musst du dich anmelden.
cloud.error.not_entitled = Dein Konto hat darauf keinen Zugriff.
cloud.error.not_found = Der gewünschte Eintrag existiert nicht.
cloud.error.rate_limited = {seconds ->
    [one] Zu viele Anfragen. Versuche es in {seconds} Sekunde erneut.
   *[other] Zu viele Anfragen. Versuche es in {seconds} Sekunden erneut.
}
cloud.error.invalid_request = Der Server hat die Anfrage als ungültig abgelehnt.
cloud.error.invalid_credentials = Benutzername oder Kennwort ist falsch.
cloud.error.username_taken = Dieser Benutzername ist bereits vergeben.
cloud.error.server = Der Server hat einen Fehler gemeldet. Versuche es später erneut.
cloud.error.offline = Der Server ist nicht erreichbar. Prüfe deine Internetverbindung.
cloud.error.invalid_response = Die Antwort des Servers konnte nicht gelesen werden.
cloud.error.io = Das Schreiben auf den Speicher ist fehlgeschlagen.
cloud.error.integrity = Die heruntergeladene Datei ist beschädigt oder unvollständig. Versuche es erneut.
cloud.error.package_rejected = Das Paket wurde abgelehnt: Die Signatur fehlt oder ist nicht vertrauenswürdig. Es wurde nichts installiert.
cloud.error.not_signed_in = Du bist auf diesem Gerät nicht angemeldet.
cloud.error.vpn = Die VPN-Schlüssel konnten nicht vorbereitet werden.

# Geräte-Identität
cloud.device.challenge_rejected = Der Server hat die Sicherheitsprüfung nicht angenommen. Versuche es erneut.
cloud.device.signature_rejected = Der Server hat den Identitätsnachweis dieses Geräts nicht angenommen.
cloud.device.taken = Dieses Gerät ist bereits bei einem anderen Konto registriert.
cloud.device.retired = Dieses Gerät wurde aus dem Konto entfernt. Setze die Geräte-Identität zurück, um es neu zu registrieren; es erhält dann eine neue Nummer.
cloud.device.limit = Dein Konto hat die Höchstzahl an Geräten erreicht. Entferne zuerst ein Gerät.
cloud.device.unknown = Der Server kennt dieses Gerät nicht.
cloud.device.update_required = Aktualisiere das System auf diesem Gerät, bevor du es registrierst.
cloud.device.legacy_not_eligible = Die frühere Registrierung konnte von diesem Gerät nicht übernommen werden.
cloud.device.cooldown_active = {seconds ->
    [one] Die Nummer kann in {seconds} Sekunde umgezogen werden.
   *[other] Die Nummer kann in {seconds} Sekunden umgezogen werden.
}
cloud.device.recovery_pending = Für dieses Konto wartet bereits ein Geräte-Ersatz.
cloud.device.recovery_unknown = Es wartet kein Geräte-Ersatz.
cloud.device.server_mismatch = Die Sicherheitsprüfung nannte einen anderen Server oder ein anderes Konto. Es wurde nichts signiert.
cloud.device.key_unusable = Der Geräteschlüssel kann nicht verwendet werden. Er wurde nicht verändert.
cloud.device.not_registered = Dieses Gerät ist noch nicht registriert.
cloud.device.not_approved = {seconds ->
    [one] Dieses Gerät wurde vor Kurzem hinzugefügt. Es darf deine anderen Geräte in {seconds} Sekunde verwalten – oder sofort, wenn eines davon es bestätigt.
   *[other] Dieses Gerät wurde vor Kurzem hinzugefügt. Es darf deine anderen Geräte in {seconds} Sekunden verwalten – oder sofort, wenn eines davon es bestätigt.
}
cloud.device.recovery_locked = {seconds ->
    [one] Diese frühere Registrierung kann noch nicht ersetzt werden. Aktualisiere jenes Gerät oder versuche es in {seconds} Sekunde erneut.
   *[other] Diese frühere Registrierung kann noch nicht ersetzt werden. Aktualisiere jenes Gerät oder versuche es in {seconds} Sekunden erneut.
}
cloud.device.approved = Das Gerät wurde bestätigt.
cloud.device.state.waiting = vor Kurzem hinzugefügt – darf andere Geräte noch nicht verwalten

# Sicherheitshinweise
cloud.alert.title = Sicherheitshinweis
cloud.alert.recovery_pending = {hours ->
    [0] Jemand mit deinem Kennwort will die Nummer des Geräts {device} auf ein anderes Gerät umziehen. Der Umzug ist jetzt möglich.
    [one] Jemand mit deinem Kennwort will die Nummer des Geräts {device} auf ein anderes Gerät umziehen. Der Umzug ist in {hours} Stunde möglich.
   *[other] Jemand mit deinem Kennwort will die Nummer des Geräts {device} auf ein anderes Gerät umziehen. Der Umzug ist in {hours} Stunden möglich.
}
cloud.alert.not_me = Das war ich nicht – abbrechen
cloud.alert.recovery_cancelled = Der Umzug der Nummer wurde abgebrochen. Ändere jetzt dein Kennwort.
cloud.alert.cannot_cancel = Dieses Gerät wurde vor Kurzem hinzugefügt und kann den Umzug nicht abbrechen. Nimm eines deiner anderen Geräte oder ändere dein Kennwort.
cloud.alert.none = Es gibt nichts zu melden.
cloud.alert.event.device_added = Deinem Konto wurde ein Gerät hinzugefügt: {device}
cloud.alert.event.device_removed = Aus deinem Konto wurde ein Gerät entfernt: {device}
cloud.alert.event.device_replaced = Eine Nummer wurde auf ein neues Gerät umgezogen: {device}
cloud.alert.event.device_upgraded = Eine frühere Registrierung wurde von Gerät {device} übernommen
cloud.alert.event.device_approved = Ein Gerät darf jetzt deine Geräte verwalten: {device}
cloud.alert.event.recovery_started = Mit deinem Kennwort wurde der Umzug der Nummer von Gerät {device} beantragt.
cloud.alert.event.recovery_cancelled = Ein beantragter Umzug einer Nummer wurde abgebrochen.
cloud.alert.event.recovery_finished = Nach der Wartezeit wurde eine Nummer auf Gerät {device} umgezogen.
cloud.alert.event.password_changed = Das Kennwort deines Kontos wurde geändert.
cloud.device.state.active = registriert
cloud.device.state.legacy = vor Einführung der Geräteschlüssel registriert – dieses Gerät aktualisieren
cloud.device.this_device = dieses Gerät
cloud.device.none = Es sind keine Geräte registriert.
cloud.device.removed = Das Gerät wurde entfernt.
cloud.device.renamed = Das Gerät wurde umbenannt.
cloud.device.reset = Die Geräte-Identität wurde gelöscht. Registriere das Gerät neu, um eine neue Nummer zu erhalten.

# Befehlszeile
cloud.cli.signed_in = Angemeldet.
cloud.cli.number = Ultar-Nummer: {number}
cloud.cli.vpn_ready = Das VPN ist eingerichtet.
cloud.cli.server = Server: {server}
cloud.cli.account.signed_in = Konto: angemeldet
cloud.cli.account.signed_out = Konto: abgemeldet (Store und Updates gehen ohne Konto)
cloud.cli.unknown_command = Unbekannter Befehl: {command}
cloud.cli.usage = Aufruf: ucloud register|login <Benutzer> [--password-file <Datei>] (Kennwort über die Standardeingabe) · number · devices · device-rename <Kennung> <Name> · device-remove <Kennung> · device-approve <Kennung> · device-reset · alerts · not-me · vpn · store [Suche] · app <Kennung> · install <Kennung> · update [Kanal] · whoami
