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

# Befehlszeile
cloud.cli.signed_in = Angemeldet.
cloud.cli.number = Ultar-Nummer: {number}
cloud.cli.vpn_ready = Das VPN ist eingerichtet.
cloud.cli.server = Server: {server}
cloud.cli.account.signed_in = Konto: angemeldet
cloud.cli.account.signed_out = Konto: abgemeldet (Store und Updates gehen ohne Konto)
cloud.cli.unknown_command = Unbekannter Befehl: {command}
cloud.cli.usage = Aufruf: ucloud register|login <Benutzer> <Kennwort> · number · vpn · store [Suche] · app <Kennung> · install <Kennung> · update [Kanal] · whoami
