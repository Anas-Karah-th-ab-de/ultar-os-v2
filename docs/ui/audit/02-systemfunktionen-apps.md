# Audit: Systemfunktionen und Apps – altes System gegen DESKTOP-SPEC §5–9

Stand 22.11.2026 · nur gelesen · Pfade relativ zu `/home/claude/ultar-os-v2` · `shell/` = `platform/ultar-shell/src/` · `leg/` = `docs/legacy/`
Spalte 3: **ja** = im Entwurf benannt · **teilw.** = Oberbegriff da, genannte Einstellungen fehlen · **fehlt**.
Nicht geprüft (anderer Prüfer): Konten, Telefonie, Sperre/PIN/Fingerabdruck/Gesicht, Verschlüsselung.
„(unsicher)“ = nur in der Unterlage gefunden oder Zuordnung im Entwurf nicht eindeutig.

## A. Systemfunktionen

| # | Funktion/Einstellung | Wo im alten System (Datei) | Im Entwurf vorhanden? | Vorschlag: Bereich/Fläche + genaue Einstellungen/Elemente |
|---|---|---|---|---|
| 1 | Automatische Zeit (SNTP, HTTP-Ersatz), Abgleich nach Start/alle 12 h | shell/sysx.rs `auto_time`, sysd.rs `sync_time`; leg/SYSTEM-FUNKTIONEN.md #6–7 | ja §8 Datum & Zeit „automatisch (Netz)“ | – |
| 2 | Zeitserver wählbar, „Letzter Abgleich“, „Jetzt abgleichen“ | sysx.rs `ntp_server`, `last_sync`; app_sys.rs („Synchronize with a time server“, „Last sync“) | fehlt | Einstellungen › Datum & Zeit: Zeile „Zeitserver“ (Text, Standard pool.ntp.org), Info „Letzter Abgleich: …“, Aktion „Jetzt abgleichen“ |
| 3 | Zeitzone (29 Städte, Regeln), Sommerzeit automatisch, fester Versatz von Hand | sysx.rs `zone`; app_sys.rs („Manual (fixed offset)“, „summer time“) | teilw. §8 (Zeitzone automatisch/Wahl) | Datum & Zeit: Wahl „Fester Versatz (UTC±hh:mm)“ als letzter Eintrag der Zonenliste; Info „Sommerzeit: gilt/gilt nicht“ |
| 4 | Zeit von Hand stellen (Stunde, Minute, Tag, Monat, Jahr) | shell/app.rs `s_time` | teilw. §8 (nur „automatisch“) | Datum & Zeit: bei „automatisch“ aus → Felder Datum + Uhrzeit, Aktion „Übernehmen“ |
| 5 | 12-/24-Stunden-Format | sysx.rs `h12` | ja §8 Datum & Zeit, §6 Schritt 13 | – |
| 6 | Energiesparmodus + automatisch ab x %, aus beim Laden | sysx.rs `saver`, `auto_saver`, `saver_at` | ja §8 Energie | – |
| 7 | Leistungsprofil Sparend/Ausgewogen/Leistung | sysx.rs `perf` | ja §8 Energie, §5.1 Kachel | – |
| 8 | Akku-Details: Temperatur, Zustand, Spannung, wärmstes Bauteil | app_sys.rs (Battery) | teilw. §8 („Akku-Stand, Verlauf, Zustand“) | Energie › Akku: Info-Zeilen Temperatur, Spannung, Zustand (%), Ladezyklen falls vorhanden |
| 9 | Hinweise „Akku schwach“ 15 %/5 %, „Wird geladen“, „Akku voll“ | sysd.rs; leg/SYSTEM-FUNKTIONEN.md #13–14 | fehlt | Energie: Schalter „Hinweis bei niedrigem Akku“ (Schwelle 15 %/5 % fest), als System-Meldung „wichtig“ in §5.3 Benachrichtigungen aufnehmen |
| 10 | Hitzewarnung ab 70 °C | sysd.rs; SYSTEM-FUNKTIONEN.md #16 | fehlt | Hülle §5.3 Rückmeldungen: Systemmeldung „Gerät zu warm“ (nicht abschaltbar); Wert im Systemmonitor |
| 11 | Ladehinweis bei 80 % | sysy.rs `charge_alert` | ja (besser) §8 Energie „Ladegrenze (80 %)“ | Hinweis entfällt, wo Ladegrenze die Hardware kann; sonst Schalter „Bei 80 % erinnern“ in Energie |
| 12 | Akku-Verlauf 24 h | app_more.rs `Mx::BattHist`; sysy.rs | ja §8 Energie „Verlauf“ | – |
| 13 | Restlaufzeit / Zeit bis voll | app_more.rs („Lasts about“, „Full in about“) | fehlt | Energie › Akku: Info „Reicht noch etwa …“ / „Voll in etwa …“; dieselbe Zeile im Akku-Fuß der Schnelleinstellungen |
| 14 | Beim Laden wach bleiben | sysx.rs `stay_awake_charging` (Entwickler) | fehlt | System/Info › Entwickleroptionen: Schalter „Beim Laden wach bleiben“ |
| 15 | Wach halten 30 min (Schnellschalter) | app_more.rs `caffeine`; leg/SYSTEM-FUNKTIONEN-2.md #31 | fehlt | Schnelleinstellungen §5.1: Kachel „Wach halten“ (30 min · 1 h · bis aus), Statussymbol solange an |
| 16 | Bildschirm aus nach … | app.rs `s_display` („Screen off after“) | ja §8 Energie | – |
| 17 | Nachtlicht: an, Stärke, Zeitplan von–bis | sysx.rs `night*`; app_sys.rs `Sx::DisplayX` | ja §8 Anzeige, §5.1 Kachel | – |
| 18 | Automatische Helligkeit (Lichtsensor) | sysx.rs `auto_bright` | fehlt | Anzeige › je Bildschirm „Helligkeit“: Schalter „Automatisch (Lichtsensor)“, Bedingung Sensor; Schieber bleibt als Grundwert |
| 19 | Hoher Kontrast | sysx.rs `contrast` | ja §8 Barrierefreiheit | – |
| 20 | Dunkelmodus, Akzentfarbe, Größe, Drehung, Auflösung | app.rs `s_display` | ja §8 Aussehen/Anzeige | – |
| 21 | Automatisches Design (hell am Tag, Zeiten Tag ab/Nacht ab) | sysy.rs `auto_theme`, `day_from/to` | ja §8 Aussehen „automatisch (Sonnenstand)“ | Ergänzen: Wahl „nach Sonnenstand · eigene Zeiten (von/bis)“ – Sonnenstand braucht Ort |
| 22 | Tonprofile Normal / Vibration / Lautlos | sysx.rs `profile`; app_sys.rs `Sx::SoundX` | fehlt | Ton (Bedingung Vibrationsmotor): Auswahl „Klingeln · Vibrieren · Lautlos“; in Schnelleinstellungen als Zustand der Ton-Kachel (kompakt) |
| 23 | Vibration bei Berührung | sysx.rs `haptic` | fehlt | Ton › „Vibration“ (Bedingung Motor): Schalter „Bei Berührung“, „Bei Benachrichtigungen“ |
| 24 | Systemklänge an, Lautstärke, Testton | shell/tools.rs `s_sound` | ja §8 Ton | – |
| 25 | Nicht stören + Zeitplan; Anrufe/Wecker/Erinnerungen kommen durch | sysx.rs `dnd*`; app_sys.rs | teilw. §8 Benachrichtigungen (Zeitplan ja, Ausnahmen nicht) | Benachrichtigungen › Nicht stören: Liste „Darf durch“: Wecker (fest an), Erinnerungen, Anrufe (alle · Favoriten · keine), als „wichtig“ markierte Apps |
| 26 | Benachrichtigungsverlauf (300 Einträge), „Verlauf löschen“ | sysd.rs `history`; app_sys.rs `Sx::NotifHist` | fehlt (§5.1 nur aktuelle Liste) | Blatt Datum & Zeit: unter der Liste „Verlauf“ → Liste nach Tag, Suche, „Verlauf löschen“; Benachrichtigungen: Schalter „Verlauf führen“ + Dauer (7/30 Tage) |
| 27 | Benachrichtigungen „Alle löschen“, Kopf-Hinweis, Dringlichkeit low/normal/high | shell/notify.rs; devices/a30/docs/A30-BENACHRICHTIGUNGEN.md | ja §5.1, §5.3 | – |
| 28 | Verstecktes WLAN hinzufügen | app_sys.rs `Sx::HiddenWifi` | ja §8 WLAN | – |
| 29 | WLAN-Details (Signal, Frequenz, IP, Gateway, DNS, MAC, Sicherheit) | app_sys.rs `Sx::NetTools` | teilw. §8 WLAN (Stärke, Sicherheit) | WLAN › verbundenes Netz › „Details“: Signal dBm, Band/Kanal, Geschwindigkeit, IPv4/IPv6, Gateway, DNS, Geräteadresse, „Kopieren“ |
| 30 | Eigene DNS-Server systemweit | sysx.rs `dns` | teilw. §8 (DNS je Netz/Kabel) | Netzwerk: Zeile „DNS für alle Netze“ (automatisch · eigene Adressen · verschlüsselt später); je Netz überschreibbar |
| 31 | Netzdiagnose: Router-Ping, Internet-Ping, DNS, HTTPS | app_sys.rs („Diagnostics“) | fehlt | Netzwerk › „Verbindung prüfen“: vier Zeilen (Router · Internet · Namensauflösung · HTTPS) je Haken/Fehler + Zeit, Aktion „Erneut prüfen“, „Bericht kopieren“ |
| 32 | Monatliches Datenlimit, Warnung 80 %/100 % | sysx.rs `data_limit_mb`; sysd.rs | teilw. §8 Mobilfunk „Verbrauch + Limit“ | Auch für als „getaktet“ markierte WLAN-Netze: WLAN › bekanntes Netz › „Monatslimit“ + Warnschwelle |
| 33 | Datenverkehr je Verbindung und je Tag, Upload hervorgehoben | shell/monitor.rs (Reiter „Data traffic“) | ja §9 Systemmonitor | Ergänzen: Ansicht „je Tag“ (30 Tage) im Reiter Netz |
| 34 | Systemprotokolle Kern / Dienste, Aktualisieren | app_sys.rs `Sx::Logs` | ja §7.1, §9 Protokolle (ohne Muss-Liste) | App Protokolle, Muss: Quellen Kern · Dienste · Apps · Rettung, Filter Stufe/Zeit, Suche, „Speichern/Teilen“ |
| 35 | Sicherung (Einstellungen, Notizen, Kontakte, Kalender, Wecker; ohne Kennwörter) + Wiederherstellen | app_sys.rs `Sx::Backup`; sysx.rs | ja §8 Sicherung, §9 | „was“ ausschreiben: Einstellungen · App-Daten je App · persönliche Ordner |
| 36 | Sicherungs-Erinnerung nach 30 Tagen | sysy.rs `backup_remind` | teilw. §8 Sicherung „Zeitplan“ | Sicherung: Schalter „Erinnern, wenn letzte Sicherung älter als 30 Tage“ (für Ziel USB ohne Zeitplan) |
| 37 | Werkseinstellungen, dreifache Bestätigung, Fotos bleiben | app_sys.rs („Factory reset“) | ja §8 System/Info „Zurücksetzen (Einstellungen / alles löschen)“ | Dritte Stufe ergänzen: „System zurücksetzen, persönliche Dateien behalten“ |
| 38 | Speicher bereinigen (Zwischenspeicher, alte Programmstände) | app_sys.rs („Free up storage“), app_more.rs `Mx::StorageX` | ja §8 Speicher „Aufräumen“ | – |
| 39 | Speicher-Analyse größte Ordner/Dateien | app_more.rs `Mx::StorageX` | ja §8 Speicher, §9 Belegung | – |
| 40 | Warnung „Speicher fast voll“ | sysy.rs `storage_warn` | fehlt | Speicher: Schalter „Warnen bei wenig Platz“ (Schwelle 10 %/5 GB); Meldung mit „Aufräumen“ |
| 41 | Entwickleroptionen: Berührungen zeigen, Bildrate/Zeichenzeit | sysx.rs `show_touches`, `show_fps` | teilw. §8 System/Info (Leistungsanzeige ja, Berührungen nein) | Entwickleroptionen: Schalter „Berührungen/Zeiger anzeigen“ |
| 42 | Neustart in Download-Modus / Recovery | app_sys.rs („Restart into“) | fehlt | Entwickleroptionen (nur Handy): Aktion „Neustart in Flash-Modus“ / „in Rettung“ mit Rückfrage; PC: Ein/Aus-Menü mit Umschalt = „Neustart in Rettung/Firmware“ |
| 43 | Entwicklerzugang SSH: an/aus, Adresse, Geräteschlüssel, erlaubte Schlüssel | app_sys.rs („Developer access (SSH)“) | teilw. §8 Freigaben „SSH“, Entwickleroptionen „Fern-Shell“ – doppelt angelegt | Ein Ort: Freigaben › SSH (an, Netz-Auswahl, Fingerabdruck des Geräteschlüssels, Liste erlaubter Schlüssel + „Schlüssel hinzufügen“); Entwickleroptionen verweist nur |
| 44 | Diagnose der Oberfläche (Startzeit, Bildzeit, Speicher) | app.rs `s_system` („Diagnostics“) | teilw. (Leistungsanzeige) | Entwickleroptionen › Leistungsanzeige: Werte Start bis erstes Bild, Bildzeit, RAM der Hülle |
| 45 | Dienste ansehen, starten/stoppen | app.rs `s_services` | fehlt (nur Web-Verwaltung der Komponente Server) | Systemmonitor: Reiter „Dienste“ (Zustand, Neustart; Stopp nur mit Bestätigung, Systemdienste geschützt) |
| 46 | Gerätename ändern | app.rs `s_system` | ja §8 System/Info, Freigaben | Nur ein Ort: System/Info; Freigaben zeigt ihn an |
| 47 | Ersteinrichtung erneut ausführen | app.rs `s_system` („Run setup again“) | fehlt | System/Info: Aktion „Ersteinrichtung erneut starten“ |
| 48 | Globale Suche: Apps, Einstellungen, Kontakte, Notizen, arabisch tolerant | app_sys.rs `Sx::Search`; sysx.rs | teilw. §5.1 Suche (Kontakte, Notizen fehlen) | Suche: Quellen „Kontakte“, „Notizen“, „Kalender“, „Hilfe“ ergänzen; in §8 Suche abschaltbar |
| 49 | Zuletzt verwendete Apps, „Alle schließen“ | app_more.rs `Mx::Recents` | ja §5.5, §5.1 Übersicht | Übersicht (kompakt): Knopf „Alle schließen“ |
| 50 | App anheften (Bildschirm-Fixierung; lösen mit Start halten, mit PIN gesperrt) | app_more.rs `pinned`; SYSTEM-FUNKTIONEN-2.md #23 | fehlt | Übersicht: Fenstermenü „Fixieren“; Datenschutz & Sicherheit › Bildschirmsperre: Schalter „App-Fixierung“, „Zum Lösen entsperren“; Statussymbol |
| 51 | Zwischenablage mit Verlauf, nicht bei Kennwortfeldern, „Alle löschen“ | app_more.rs `Mx::Clipboard`; sysy.rs `clip` | ja §5.3 (Löschen nicht genannt) | Zwischenablage-Blatt: „Alle löschen“, Eintrag löschen; Einstellung „Verlauf führen“ + Größe in Datenschutz & Sicherheit |
| 52 | Nutzungszeit je App (heute/7 Tage), Zähler Bildschirm an/entsperrt | app_more.rs `Mx::Usage`; sysy.rs `Stats` | fehlt (nur Kinderschutz-Zeiten) | Neuer Bereich „Nutzungszeit“ in Einstellungen: heute/7 Tage je App, Bildschirm an, Entsperrungen, Schalter „Erfassen“ (lokal), „Daten löschen“; Kinderschutz nutzt dieselben Daten |
| 53 | Pausen-Erinnerung nach langer Bildschirmzeit | sysy.rs `break_min` | fehlt | Nutzungszeit: „Pausen-Erinnerung nach“ (aus · 30 · 60 · 90 min) |
| 54 | Gerätepflege: Zustandswert + Hinweise mit Sprung zur Lösung | app_more.rs `Mx::Care`; sysy.rs | fehlt | System/Info: Karte „Zustand“ oben (Speicher, Akku, Updates, Sicherung, Treiber) – je Zeile Sprung zur Einstellung; kein eigener Punktwert nötig |
| 55 | Laufende Programme beenden (Systemprogramme geschützt) | app_more.rs `Mx::Tasks`; monitor.rs Reiter „Processes“ | ja §9 Systemmonitor | – |
| 56 | Schlafmodus (Nicht stören + Nachtlicht + gedimmt, stellt wieder her) | sysy.rs `sleep_mode`, `sleep_prev` | fehlt | Benachrichtigungen › Nicht stören: Zeitplan-Art „Schlafen“ mit Schaltern „Nachtlicht“, „Bildschirm dimmen“; Kachel „Nicht stören“ lang drücken = Modus wählen |
| 57 | Geplanter Neustart (täglich/Wochentag, Uhrzeit; nie im Anruf) | sysy.rs `restart_on/at/day` | fehlt | Energie: „Geplanter Neustart“ (aus · täglich · Wochentag + Uhrzeit), Hinweis 5 min vorher mit „Verschieben“ |
| 58 | Taschen-Schutz (Näherungssensor) | sysy.rs `pocket` | fehlt | Anzeige (Bedingung Näherungssensor): Schalter „In der Tasche nicht aufwecken“ |
| 59 | Stündliches Zeitsignal (Vibration) | sysy.rs `hourly_chime` | fehlt | Uhr › Einstellungen der App: „Stundensignal“ (aus · Ton · Vibration) – niedrige Priorität, Übernahme entscheiden |
| 60 | Besitzerinfo auf dem Sperrbildschirm | sysy.rs `owner_info` | fehlt | Datenschutz & Sicherheit › Bildschirmsperre: Text „Nachricht auf dem Sperrbildschirm“ |
| 61 | Sperrliste für Anrufer | app_more.rs `Mx::Blocked`; sysy.rs `blocked` | fehlt in §8 Mobilfunk | Komponente Telefon › Einstellungen der App „Gesperrte Nummern“ (Liste, hinzufügen, entsperren); nicht in Systemeinstellungen |
| 62 | Notfall: Notruf, Notfallkontakt, medizinische Angaben, ohne Entsperren | app_more.rs `Mx::Emergency`; sysy.rs `emergency_*`, `medical` | fehlt (§5.3 nur „Notfall-Ausschalten“) | Neuer Bereich „Notfall“ in Einstellungen: Notrufnummer, Notfallkontakte, medizinische Angaben (Blutgruppe, Allergien, Medikamente), Schalter „Auf Sperrbildschirm zeigen“; Sperrbildschirm + Ein/Aus-Menü: Knopf „Notfall“ |
| 63 | SOS-Lichtsignal (Morse, Kameralicht) | app_more.rs `Mx::Sos`; sysy.rs | fehlt | Fläche „Notfall“ (aus #62): Aktion „SOS-Licht“; Bedingung Kameralicht |
| 64 | Taschenlampe (Schnellschalter) | shell/quick.rs `set_torch`; A30-SCHNELLEINSTELLUNGEN.md | fehlt | Schnelleinstellungen §5.1: Kachel „Taschenlampe“ (Bedingung LED), auch auf dem Sperrbildschirm |
| 65 | Bildschirmlicht (weiß, volle Helligkeit) | app_more.rs `Mx::Light` | fehlt | Nur ohne LED: Kachel „Taschenlampe“ nutzt dann den Bildschirm – keine eigene App |
| 66 | Datenschutz-Hauptschalter als Schnellschalter | quick.rs; A30-SCHNELLEINSTELLUNGEN.md | teilw. §8 Datenschutz (Schalter ja, Kachel nein) | Schnelleinstellungen: Kacheln „Kamera“, „Mikrofon“, „Standort“ (je Fähigkeit) bzw. eine Kachel „Alles sperren“ |
| 67 | Bildschirmfoto per Kachel mit 3 s Verzögerung; Tasten Leiser+Ein/Aus | app_more.rs `shot_at`; SYSTEM-FUNKTIONEN.md #22 | teilw. §5.3 (Verzögerung, Handy-Tasten fehlen) | Bildschirmfoto-Fläche: Wahl „Verzögerung 0 · 3 · 10 s“; §5.5: Tastenkombination Leiser+Ein/Aus |
| 68 | Lautstärke-Anzeige bei Tasten | app_sys.rs `vol_shown` | ja §5.3 Rückmeldungen | – |
| 69 | Statussymbole Wecker, Fokus, wach halten, angeheftet | SYSTEM-FUNKTIONEN-2.md #25 | teilw. §5.1 Status | Status: Symbole „Wecker gestellt“, „Wach halten“, „Fixiert“ ergänzen (Regel „nur wenn sie etwas sagen“ bleibt) |
| 70 | „Auf einen Blick“ am Start (Fokus, nächste Erinnerung, nächster Wecker, sonst Hijri) | SYSTEM-FUNKTIONEN-2.md #26; app_layout.rs | fehlt (§5.5 beschreibt keinen Startbildschirm) | §5.5 ergänzen: Startbildschirm kompakt mit Zeile „Als Nächstes“; Desktop: dieselbe Zeile im Blatt Datum & Zeit und auf dem Sperrbildschirm |
| 71 | Startbildschirm Handy: meistgenutzte Apps, Verbindungskarte, Hintergrundbild | app_layout.rs; leg/APPS.md | fehlt (§5.5 nur Zuordnung der Flächen) | §5.5 um eigene Beschreibung „Start (kompakt)“ erweitern: Uhr, „Als Nächstes“, Favoritenzeile, App-Raster, Suche |
| 72 | Hardware-Selbsttest (Speicher, Anzeige, Tasten, Ton, Kameras, Modem/SIM, WLAN, Bluetooth, Sensoren, Akku, Vibration) | shell/selftest.rs; app.rs `s_hardware` | ja §8 Treiber & Hardware „Selbsttest“ | Prüfliste ausschreiben wie alt; Ergebnis OK/Warnung/Fehler + „Bericht speichern“ |
| 73 | Treibersuche | app.rs `s_hardware` („Driver search“) | ja §8 Treiber & Hardware | – |
| 74 | Notausstieg: Laut+Leise 2 s/6 s/10 s (Oberfläche neu · Dienste neu · Neustart), `ultar-ctl rescue` | devices/a30/docs/A30-NOTAUSSTIEG.md (Code laut Unterlage `ultar-init/src/rescue.rs`) | fehlt (§5.2 nur „Fenster reagiert nicht“) | §5.3 neue Fläche „Notausstieg“: Handy Laut+Leise halten (3 Stufen, Vibration als Rückmeldung), PC Strg+Alt+Entf 2 s halten = Hülle neu starten, 6 s = Dienste, 10 s = Neustart; danach Meldung; Hilfeseite; nicht abschaltbar |
| 75 | Wachhund der Oberfläche (startet hängende Hülle neu) | A30-NOTAUSSTIEG.md §2 | fehlt (kein sichtbares Element; unsicher ob Entwurfsthema) | In §5.3 als Verhalten nennen: Meldung „Oberfläche wurde neu gestartet“ + Eintrag in Protokolle |
| 76 | Sensoren: Liste erkannter Sensoren mit Zustand | shell/sensors.rs `s_sensors` | teilw. §8 Datenschutz (nur Schalter) | Treiber & Hardware › Geräteliste: Gruppe „Sensoren“ mit Live-Wert je Sensor (Bedingung Sensoren) |
| 77 | Standort/GNSS: Fix-Art, Breite/Länge, Höhe, Genauigkeit, Tempo, Zeit, Satelliten | shell/gnss.rs | teilw. §8 Datenschutz „Standort“ (nur Schalter + App-Recht) | Datenschutz & Sicherheit › Standort: Schalter, Apps mit Zugriff (zuletzt benutzt), „Genauigkeit/Quelle“ (GNSS · Netz), Fläche „Satelliten-Status“ (Fix, Anzahl, Genauigkeit) unter Treiber & Hardware |
| 78 | Systemweite Werbe-/Tracker-Sperre (Hosts-Liste), Liste aktualisieren, Zähler | shell/tools.rs `adblock_apply`, `s_privacy` | teilw. §9 Browser (nur im Browser) | Netzwerk › „Werbe- und Tracker-Sperre (alle Apps)“: Schalter, Listenstand, „Liste aktualisieren“, Ausnahmen; Zähler in Datenschutz-Zentrale |
| 79 | Hilfe-Knopf (i) in jeder Titelzeile, abschaltbar | app.rs `s_display` („Help button in the title bar“) | fehlt | §5.1 App-Menü: Eintrag „Hilfe zu dieser Seite“ (F1); in Einstellungen je Bereich Symbol Kreis-i → Hilfeseite |
| 80 | Titel antippen = an den Anfang der Liste | SYSTEM-FUNKTIONEN-2.md #46 | fehlt | §5.5 als Geste nennen (Statusleiste antippen = Anfang); Desktop: Pos1 |

## B. Apps

| # | Funktion/Einstellung | Wo im alten System (Datei) | Im Entwurf vorhanden? | Vorschlag |
|---|---|---|---|---|
| 81 | Rechner: Standard, Reiter Einheiten, Reiter Zahlensysteme | shell/tools.rs `calc_tab`, `s_calc` | ja §9 Rechner (einfach/wissenschaftlich/Programmierer/Umrechnen) | – |
| 82 | Rechner-Verlauf | app_sys.rs `Sx::CalcHist` | fehlt | Rechner: Seitenleiste/Blatt „Verlauf“ (antippen übernimmt, „Verlauf löschen“) |
| 83 | Einheitenrechner (Länge, Gewicht, Temperatur, Fläche, Volumen, Tempo, Daten) | app_sys.rs `Sx::Units`; sysx.rs | ja §9 Rechner „Umrechnen“, §5.1 Suche | Gruppen wie alt ausschreiben |
| 84 | Währungsrechner mit eigenen Kursen (ohne Netz) | app_more.rs `Mx::Currency`; sysy.rs `rates` | fehlt | Rechner › Umrechnen › Gruppe „Währung“: eigene Kurse bearbeiten; bei Netz „Kurse holen“ mit Datum |
| 85 | Prozent-/Trinkgeldrechner, Ratenrechner, Datumsrechner | app_more.rs `Mx::Percent`, `Loan`, `DateCalc` | fehlt | Rechner: Modus-Liste um „Prozent“, „Kredit“, „Datum“ erweitern (ein Modus-Menü, keine eigenen Apps) |
| 86 | Zahlensysteme Dez/Hex/Bin/Okt | app_more.rs `Mx::Bases`; tools.rs Reiter | ja §9 „Programmierer“ | – |
| 87 | Würfel, Münze, Zufallszahl | app_more.rs `Mx::Dice` | fehlt | Nicht ins Grundsystem; falls gewünscht Store-App. Entscheidung nötig |
| 88 | Großanzeige (Text so groß wie möglich) | app_more.rs `Mx::BigText` | fehlt | Nicht ins Grundsystem; Entscheidung nötig |
| 89 | Uhr: Wecker (Mo–Fr/Wochenende/einmal), Schlummern 5 min, klingelt bei Bildschirm aus | tools.rs `Alarm`, `s_clock`; app_sys.rs `snooze` | ja §9 Uhr | Muss ergänzen: Wiederholung je Wochentag, Schlummern, Ton je Wecker, weckt aus Bereitschaft |
| 90 | Weltuhr, Stadt hinzufügen | app_sys.rs `Sx::World`; tools.rs Reiter „World“ | ja §9 Uhr | – |
| 91 | Timer (Pause/Fortsetzen), Stoppuhr mit Runden | tools.rs | ja §9 Uhr | – |
| 92 | Countdown „Tage bis“ | app_more.rs `Mx::Countdown`; sysy.rs `countdowns` | fehlt | Kalender (leicht): Termin-Eigenschaft „Tage zählen“ → Anzeige im Blatt Datum & Zeit; keine eigene Fläche |
| 93 | Fokus-Timer (Arbeit / 5 min Pause, schaltet Nicht stören) | app_more.rs `Mx::Focus`; sysy.rs `focus_min` | fehlt | Uhr: Reiter „Fokus“ (Dauer, Pause, Schalter „Nicht stören dabei“); Statussymbol |
| 94 | Erinnerungen (in 10 min … 24 h; klingeln bei Bildschirm aus; Helfer „ruft an“) | app_more.rs `Mx::Reminders`; sysy.rs `reminders`; app_buddy.rs (`reminders.tsv`) | fehlt | Uhr: Reiter „Erinnerungen“ (Text, Zeit/in …, Wiederholung, „Fertig · In 10 min“); Helfer legt nur dort an |
| 95 | Aufgabenliste | app_more.rs `Mx::Todo`; sysy.rs `todos` | fehlt | Grund-App „Notizen“ (#101): Notiz-Art „Liste mit Haken“; keine eigene App |
| 96 | Kalender: Monat, Termine, ganztägig, Erinnerung x min vorher | app_sys.rs `Sx::Calendar`, `EventEdit` | ja §9 Kalender (leicht), §5.1 Blatt | Muss ausschreiben: Termin anlegen/ändern, ganztägig, Erinnerung, Wiederholung |
| 97 | Hijri-Datum (rechnerisch, ±1 Tag), eigene Seite | app_more.rs `Mx::Hijri`; sysy.rs | ja §5.1, §8 Leiste/Region, §9 Kalender | Region & Sprache › Kalender: „Hijri-Korrektur −2…+2 Tage“; Umrechnung Datum ↔ Hijri im Kalender |
| 98 | Gebetszeiten (16 Städte, 4 Methoden, nächste Zeit) | app_more.rs `Mx::Prayer`; sysy.rs `city`, `method` | fehlt | Uhr: Reiter „Gebetszeiten“ (Ort: Liste/Standort/Koordinaten, Methode MWL · Ägypten · Umm al-Qura · Karatschi, Hinweis je Zeit an/aus, Korrektur ±min); nächste Zeit im Blatt Datum & Zeit; abschaltbar in Region & Sprache |
| 99 | Qibla-Richtung | app_more.rs `Mx::Qibla` | fehlt | Uhr › Gebetszeiten: Knopf „Qibla“ (Kompassnadel bei Magnetsensor, sonst Gradzahl + Karte) |
| 100 | Kompass (Korrekturwert), Wasserwaage | app_more.rs `Mx::Compass`, `Level`; sysy.rs `compass_offset` | fehlt | Grund-App „Kompass“ nur bei Sensor (Bedingung Magnet-/Beschleunigungssensor): Reiter Kompass (Kalibrieren, Korrektur) · Wasserwaage |
| 101 | Notizen (Karten, anheften, fortlaufend speichern) | shell/app_extra.rs `Notes` | fehlt (§7.1/§9 nur Texteditor) | Neue Grund-App „Notizen“ in §7.1/§9: Liste/Raster, anheften, Suche, Listen mit Haken, Teilen/Export, in Sicherung |
| 102 | Kontakte (anlegen/bearbeiten/löschen, direkt anrufen) | app_extra.rs `Contacts` | ja §7.2 Kommunikation | Komponente „Telefon“ muss Kontakte als Abhängigkeit nennen (Regel §7.2) |
| 103 | Dateien: Kopieren/Verschieben/Umbenennen/Löschen, neuer Ordner, Sortierung, USB-Stick | tools.rs `s_files`, `paste`, `rename`, `fsort` | ja §9 Dateien | – |
| 104 | Dateien: App-Datei antippen → Installation | tools.rs `installable` | ja §7.3 „Datei installieren“ | – |
| 105 | Dateien: „Dem Assistenten geben“ | tools.rs:2232; leg/SYSTEM-ASSISTENT.md | fehlt | Dateien-Kontextmenü: „An Helfer geben“ (nur mit Komponente Helfer) |
| 106 | Galerie: Fotos nach Datum, Alben, Favoriten, Videos, Mehrfachauswahl, Details, drehen, als Hintergrund | shell/app_gallery.rs | teilw. §9 Bilder (Betrachter) + §7.2 Medien „Foto-Verwaltung“ | Medien-Komponente ausschreiben: Reiter Fotos · Alben · Favoriten · Videos, Mehrfachauswahl, Details; Grund-App Bilder: Blättern im Ordner, Favorit |
| 107 | Kamera: Foto, Quadrat, Video, Selbstauslöser 3/10 s, Raster, Zoom 1/2/4×, Licht, Front/Rück, Bild drehen je Kamera | shell/app_camera.rs; leg/APPS.md | teilw. §9 (Kamera ohne Muss-Liste) | §9 Kamera, Muss: Modi Foto · Video · Scan, Selbstauslöser, Raster, Zoom, Licht, Kamera wechseln, Auflösung/Format, Speicherort, letzte Aufnahme; „Bild drehen“ → Treiber & Hardware |
| 108 | Dokumentenscanner per Kamera: Blatt finden, Ecken ziehen, Farbe/Grau/SW, mehrseitiges PDF | shell/scan.rs; app_camera.rs (Modus „Scan“) | teilw. §9 „Scannen“ (Gegenstück Flachbett-Scanner) | App Scannen: Quellen „Scanner“ und „Kamera“; Kamera-Weg: Ecken prüfen, Filter, Seiten ordnen, „Als PDF speichern“; Kamera-Modus „Scan“ öffnet dieselbe Fläche |
| 109 | Systemmonitor: Last (CPU-Kerne, RAM, GPU/NPU, Temperatur), Datenverkehr, Prozesse | shell/monitor.rs | ja §9 Systemmonitor | NPU/KI-Chip und Temperatur ergänzen |
| 110 | PDF-Leser (Zoom, Suche, Lesemodus) | shell/app_office.rs `Ox::Pdf`; apps/ultar-office/src/pdf.rs | ja §9 Dokumente | „Lesemodus (Text der Seite)“ ergänzen |
| 111 | Dokumente (.docx/.md/.txt), Tabellen (.xlsx/.csv, Formeln) | app_office.rs; apps/ultar-office | ja §7.2 Büro | – |
| 112 | Code-Editor (21 Sprachen, Reiter, Regex-Suche, Kodierungen, Zeilenende) | app_office.rs `Ox::Code`; ultar-office/src/code.rs | ja §9 Texteditor + §7.2 Coder | Grenze festlegen: Texteditor = eine Datei/Reiter; Coder = Projekte – Zeilenende-Umstellung in Texteditor aufnehmen |
| 113 | Befehlszeile `udoc` (lesen, erzeugen, umwandeln) | apps/ultar-office/src/bin; leg/BUERO-APPS.md | fehlt (unsicher ob nutzersichtbar) | In Komponente Büro als Inhalt nennen |
| 114 | Browser | shell/app.rs `Screen::Web`; apps/ultar-web | ja §9, §11 | – |
| 115 | Terminal | app.rs `Screen::Terminal` | ja §9 | – |
| 116 | Ubuntu-Programme starten (Liste grafischer Programme) | app_extra.rs `desktop_apps` | ja §7.2 Linux-Programme, §5.1 App-Raster | Apps der Komponente erscheinen im App-Raster mit Quelle-Abzeichen; keine eigene Starter-Liste |
| 117 | Kennwort-Generator (Länge, Großbuchstaben, Ziffern, Symbole) | app_more.rs `Mx::Password` | fehlt | Kennwörter & Schlüssel: „Kennwort erzeugen“ (dieselben Optionen); im Systemdialog „Kennwort speichern“ wiederverwendet |
| 118 | Zähler mit Ziel 33/99/100/1000 und Vibration (Tasbih) | app_more.rs `Mx::Counter`; sysy.rs `counter*` | fehlt | Uhr › Gebetszeiten: Knopf „Zähler“ (Ziel wählbar, Vibration, Zurücksetzen) – oder Store-App; Entscheidung nötig |
| 119 | Hintergrundbild aus Galerie setzen | app_gallery.rs („Wallpaper set“) | ja §9 Bilder, §8 Aussehen | – |

## C. Helfer und System-Assistent

| # | Funktion/Einstellung | Wo im alten System (Datei) | Im Entwurf vorhanden? | Vorschlag |
|---|---|---|---|---|
| 120 | Schwebende Figur über jeder Seite: antippen, ziehen, „Wegschicken“, Lasche, Ausflüge | shell/app_buddy.rs; leg/HELFER.md | teilw. §5.3 (nur Knopf + Super+H), §7.2 „Helfer-Figur“ | §5.3 Helfer: Darstellung wählen – Einstellungen › Helfer: „Figur auf dem Bildschirm“ (an/aus), „Bewegt sich von selbst“ (an/aus), Lage merken; aus = nur Knopf/Kürzel |
| 121 | Link-Sicherheitsprüfung: Zahl an der Figur, „Prüfen“ in abgeschotteter Box, Urteil sicher/Vorsicht/gefährlich | shell/linkcheck.rs; app_buddy.rs; s_privacy („Check a link now“, „Links checked“) | fehlt | Systemdienst unabhängig vom Helfer: Kontextmenü jedes Links „Link prüfen“ (§5.3 Systemdialog mit Urteil + Gründen); Datenschutz & Sicherheit: Schalter „Links vor dem Öffnen einschätzen“; Browser nutzt denselben Dienst |
| 122 | Erinnerung per Satz anlegen (de/en/ar), Helfer „ruft an“: Vollbild, Fertig / In 10 Min, Wiederholung, „Verpasste Erinnerung“ | app_buddy.rs; shell/mind.rs | fehlt | Erinnerungen in Uhr (#94); fällige Erinnerung = Systemfläche wie Wecker (§5.3 ergänzen: „Wecker/Erinnerung klingelt“ mit Fertig · Später) |
| 123 | Befehle ohne Modell (Taschenlampe, WLAN, Helligkeit, Timer, Wecker, Notiz, App öffnen, Uhrzeit, Akku) | shell/mind.rs `understand` | teilw. §5.1 Suche „Befehle“ | Suche und Helfer teilen eine Befehlsliste; in §5.1 die Befehle aufzählen (Schalter, Timer, Wecker, Notiz, App öffnen) |
| 124 | Vorstellung + Rundgang in 8 Stationen, wiederholbar | app_buddy.rs; HELFER.md | teilw. §6 Schritt 15 (Einführung 5 Seiten) | Hilfe: Aktion „Rundgang starten“ (auch ohne Helfer); mit Helfer führt die Figur |
| 125 | Helfer beantwortet aus den Hilfeseiten, „Hilfeseite öffnen“ | app_buddy.rs; shell/help.rs `digest` | fehlt | §9 Hilfe: „Helfer nutzt die Hilfetexte als Wissen“; Antwort mit Knopf „Hilfeseite öffnen“ |
| 126 | Agent-Gespräch: Sprechblasen, Werkzeugschritte, Vorschläge, „Neues Gespräch“, Zustand | shell/app_agent.rs | teilw. §5.3 Helfer (Fläche nicht beschrieben) | Eigene Unterlage „Helfer-Fläche“: Eingabe, Verlauf, Werkzeugschritte einklappbar, Bestätigungs-Karte, „Neues Gespräch“, Zustand (bereit/lädt/kein Modell) |
| 127 | Sieht den Bildschirm: Text der Seite (`see_screen`), Bildschirmfoto an Bild-Modell (`look_screen`); Kennwortfelder verborgen | shell/app_jarvis.rs; leg/SYSTEM-ASSISTENT.md | teilw. §8 Helfer „was er sehen/tun darf“ | Einstellungen › Helfer › „Darf sehen“: Schalter Bildschirmtext · Bildschirmfoto · Kamera · Dateien (Ordnerwahl); Aufnahme-Symbol in der Leiste während des Sehens |
| 128 | Frontkamera auf Wunsch (`look_at_user`), immer mit Rückfrage | app_jarvis.rs | teilw. (wie #127) | Unter „Darf sehen › Kamera“: „jedes Mal fragen“ fest |
| 129 | Mitbringen: „+“ → aktueller Bildschirm, Foto aufnehmen, neueste Bilder/Dokumente | app_jarvis.rs („Current screen“, „Take a photo“) | fehlt | Helfer-Fläche: Knopf Plus → „Bildschirm“, „Foto“, „Datei wählen“ (Systemdialog) |
| 130 | Systemaktionen als root mit Bestätigung: Befehle, Prozesse beenden, Dateien schreiben/löschen, Datenschutz-Schalter, Pakete, Ausschalten, Geheimnisse lesen | app_jarvis.rs (Werkzeuge `run_command`, `kill_process`, `write_file`, `delete_path`, `set_privacy`, `read_any` …) | teilw. §8 Helfer „was er … tun darf“; §5.3 Systemdialog Bestätigung | Einstellungen › Helfer › „Darf tun“: Stufen je Gruppe (Apps bedienen · Einstellungen ändern · Dateien ändern · Pakete · Befehle als Verwalter) mit „nie · fragen“; Verwalter-Aktionen immer über Systemdialog mit Kennwort/Fingerabdruck; Protokoll der Aktionen in Datenschutz-Zentrale |
| 131 | Schalter „Vor riskanten Aktionen fragen“ | app.rs `s_agent_set`; SYSTEM-ASSISTENT.md | fehlt | Ersetzt durch Stufen aus #130 (kein „nie fragen“ für Riskantes) |
| 132 | Lese-/Systemwerkzeuge: Systeminfo, Prozesse, Netzverbindungen je Programm, Sicherheitsstatus, Protokolle | app_jarvis.rs | teilw. (wie #130) | In „Darf sehen“: Schalter „Systemzustand und Protokolle“ |
| 133 | Agent bedient Apps: App öffnen, Adresse öffnen, Foto, Wecker, Timer, Schalter, Notiz, Kontakt, Anruf, Dateien lesen | shell/agent.rs; leg/APPS.md | teilw. (wie #130) | Jede Grund-App meldet ihre Aktionen im Verzeichnis (§2.1) – dieselben wie „App-eigene Schnellaktionen“ im Dock |
| 134 | Agent in Büro-Apps: Funken-Knopf mit Frage zur offenen Datei; Werkzeuge `doc_*`, `sheet_*`, `pdf_search` | shell/app_office.rs; leg/BUERO-APPS.md | fehlt | App-Menü §5.1: Eintrag „Helfer zu diesem Fenster fragen“ (Super+H gibt Fenster mit); Komponente Büro liefert die Werkzeuge |
| 135 | Modell-Katalog, Empfehlung nach RAM/CPU/GPU, Laden mit Fortschritt (fortsetzbar, SHA-256) | app_jarvis.rs; system/services/ultar-llm/src/models.rs | ja §7.2 Helfer, §8 Helfer „Modell“, §6 Ersteinrichtung | – |
| 136 | Quelle wählbar Hugging Face / Ollama; beliebiges Modell per Kennung | app_jarvis.rs („Source“); ultar-llm `pull` | fehlt | Einstellungen › Helfer › Modell: „Quelle“ (Ultar-Katalog · Hugging Face · Ollama), „Eigenes Modell hinzufügen“ (Kennung/Adresse), Liste installierter Modelle mit Größe + Entfernen |
| 137 | Bild-Modell getrennt (lädt nur bei Bild) | app_jarvis.rs („Vision“); SYSTEM-ASSISTENT.md | fehlt | Helfer › Modell: zweite Zeile „Modell für Bilder“ (aus · Wahl) |
| 138 | Offline: .gguf-Datei kopieren / vom USB-Stick | app_jarvis.rs (Hinweistext); leg/APPS.md | teilw. §7.3 Offline-Quelle (Modelle nicht genannt) | Helfer › Modell: „Aus Datei hinzufügen“; Offline-Quelle des Store führt Modelle als Pakete |
| 139 | Claude über Anthropic-API (Schlüssel), Daten verlassen Gerät → Rückfrage | app.rs `s_agent_set` („Claude“, „Anthropic API key“) | fehlt | Helfer › Modell: Art „Im Netz (eigener Schlüssel)“ – Anbieter, Schlüssel (im Schlüsselbund), Warnzeile „Inhalte verlassen das Gerät“, Schalter „Vor dem Senden von Bildschirm/Dateien fragen“ (fest an) |
| 140 | Adresse des Modell-Servers; lokale OpenAI-Schnittstelle 127.0.0.1:8080 für andere Programme | app.rs `s_agent_set` („Model server address“); leg/LLM-DIENST.md | fehlt | System/Info › Entwickleroptionen: „Lokale KI-Schnittstelle für Apps“ (aus · nur dieser Rechner · im Netz mit Schlüssel) |
| 141 | Weckwort „Hey KM“ einlernen (3×) | shell/app_voice.rs; shell/voice.rs | ja §8 Helfer „Weckwort“ | Ablauf einlernen beschreiben; Mikrofon-Symbol in der Leiste solange gelauscht wird |
| 142 | Geheimes Wort: hört auch bei gesperrtem Bildschirm; gesperrt nur Unkritisches | app_voice.rs | fehlt | Helfer › Sprache: „Bei gesperrtem Bildschirm zuhören“ (aus · mit geheimem Wort), Liste „gesperrt erlaubt“ |
| 143 | Stimmbild: nur Stimme des Besitzers; sonst Ausweis per Fingerabdruck/Gesicht/PIN | app_voice.rs („You are not my owner…“) | teilw. §8 Helfer „Stimme“ (Bedeutung unklar: Ausgabe- oder Besitzerstimme) | Helfer › Sprache: „Nur auf meine Stimme hören“ (einlernen/löschen) getrennt von „Stimme der Ausgabe“ |
| 144 | Eigene Sprachbefehle einlernen („Lerne Sprachbefehl …“) | app_voice.rs; HELFER.md | fehlt | Helfer › Sprache: Liste „Eigene Sprachbefehle“ (hinzufügen: Aktion wählen, 3× sprechen; löschen) |
| 145 | Sprachsteuerung aus | app_voice.rs („Voice control is off“) | ja §8 Helfer an/aus | Eigener Schalter „Zuhören“ neben „Helfer an“ |
| 146 | Sprachbefehl an anderes Gerät („auf <Gerät>: …“) | leg/VERBUND.md (Stimme und Ton) | fehlt | Verbund › Rechte je Gerät: „Helfer-Anweisungen annehmen“ |

## D. Verbund, Zellen, App-Formate, Ausgaben

| # | Funktion/Einstellung | Wo im alten System (Datei) | Im Entwurf vorhanden? | Vorschlag |
|---|---|---|---|---|
| 147 | Verbund an/aus (harter Kern-Schalter) | shell/app_cloud.rs; system/services/ultar-verbund | ja §8 Verbund, §5.1 Kachel | – |
| 148 | Geräte finden, Liste (freie Kerne, gekoppelt, über Relais) | app_cloud.rs; ultar-verbund/src/entdeckung.rs | ja §8 Verbund | Zeile je Gerät: Art, Weg (direkt/Internet), Zustand |
| 149 | Koppeln mit 6-stelligem Code (5 min) oder automatisch über Konto | app_cloud.rs; leg/VERBUND.md | teilw. §8 („gekoppelte Geräte“, „über Internet“), §6 Ersteinrichtung | Verbund: Aktion „Gerät koppeln“ (Code zeigen/eingeben, QR), Schalter „Geräte meines Ultar-Kontos automatisch vertrauen“, „Entkoppeln“ |
| 150 | Gerät von Hand angeben (Netz ohne Rundruf) | VERBUND.md (`ziel <ip>`) | fehlt | Verbund › „Gerät koppeln“ › „Adresse eingeben“ |
| 151 | Rechte: Steuern, Rechnen, Bildschirm, Ton, Dateien annehmen | app_cloud.rs (5 Schalter); ultar-verbund-Befehl `erlauben` | teilw. §8 (sehen, steuern, Dateien, Rechenleistung – Ton fehlt) | Rechte je Gerät um „Mikrofon mithören / Durchsage“ ergänzen, Standard aus |
| 152 | Bildschirm ansehen/fernbedienen (auch im Browser, Schlüssel in Adresse) | app_cloud.rs („View screen“); ultar-verbund/src/web.rs, bildschirm.rs | ja §7.2 Verbund; doppelt mit Komponente „Fernzugriff“ | Abgrenzen: Verbund = Ultar↔Ultar; Fernzugriff = RDP/VNC. Browser-Ansicht als Schalter „Ansicht im Browser erlauben“ |
| 153 | Virtueller Bildschirm für Geräte ohne Anzeige | VERBUND.md; ultar-verbund/src/bildschirm.rs | fehlt | Verbund (Server-Ausgabe): Schalter „Virtuellen Bildschirm bereitstellen“ + Größe |
| 154 | Hinweis „Bildschirm wird geteilt“ | VERBUND.md | ja §5.1 Status „Aufnahme läuft“ | – |
| 155 | Weitergeben: lang halten + nach oben wischen; Datei wird am Ziel geöffnet | app.rs `Screen::Weitergeben`; ultar-verbund/src/weitergeben.rs | teilw. §7.2 „Dateien/Zwischenablage“ | Symbol „teilen“ (§4) › Ziel „Gerät im Verbund“; Geste nur §5.5; Fenstermenü §5.2: „An Gerät weitergeben“ |
| 156 | App weitergeben (Ziel sieht/bedient die laufende App per Fernansicht) | app_cloud.rs („This app – keep working on the other device“) | fehlt | Fenstermenü „Auf anderem Gerät fortsetzen“ → Ziel öffnet Fenster-Fernansicht; in §7.2 Verbund als Inhalt nennen |
| 157 | Rechenleistung bündeln (`verteilt`, Arbeiter je freiem Kern) | ultar-verbund/src/planer.rs; VERBUND.md | ja §8 Verbund „Leistung teilen (Grenzen, nur am Netzteil)“ | Grenzen ausschreiben: Kerne %, RAM, GPU ja/nein, nur am Netzteil, nur im Leerlauf, Temperaturgrenze |
| 158 | Mining im Leerlauf (Monero/XMRig): Wallet, „auf allen Geräten einrichten“, Leerlauf/Strom/Temperatur | app_cloud.rs („Mining while idle“); ultar-verbund/src/mining.rs | fehlt | Nicht in Verbund-Einstellungen; falls gewollt eigene Store-Komponente „Mining“ mit eigener Fläche (Wallet, Pool, Grenzen, Ertrag). Entscheidung nötig (Risiko: Store-/Markenbild) |
| 159 | Mithören / Durchsage zwischen Geräten | ultar-verbund/src/ton.rs; VERBUND.md | fehlt | Verbund › Gerät: Aktionen „Durchsage“, „Mithören“ (Ziel zeigt Aufnahme-Symbol, Recht aus #151) |
| 160 | Fernbefehle: Helligkeit, Drehen, An/Aus, App öffnen, Bildschirmfoto | VERBUND.md (`bildschirm`, `foto`) | teilw. Recht „steuern“ | Verbund › Gerät: Aktionen „Sperren“, „Bildschirm aus“, „Bildschirmfoto holen“ |
| 161 | Zellen-Liste, Zelle öffnen/löschen, Zustand „läuft“ | shell/app_zellen.rs; system/services/ultar-zellen | fehlt in §8 (nur Komponente §7.2) | Neuer Bereich „Zellen & Container“ (Komponente): Liste (Name, App, Netz, Speicher, läuft), Neu, Öffnen, Löschen |
| 162 | Je Zelle Netz: frei · aus · nur über VPN-Zelle, Killswitch | app_zellen.rs („Network“, „Only via“); ultar-zellen/src/netz.rs | teilw. §8 Netzwerk „VPN immer an mit Sperre“ (systemweit), Apps › Recht „Netz“ | Apps › je App › Netz: „frei · aus · nur über VPN <Wahl>“ (Sperre bei VPN-Ausfall fest); für Zellen dieselbe Zeile |
| 163 | Je Zelle Identität: Standort, Zeitzone, Sprache, die die App sieht | app_zellen.rs („Identity shown to the app“); ultar-zellen/src/profil.rs | fehlt | Zellen › Zelle › „Was die App sieht“: Standort (keiner · fest · echt), Zeitzone, Sprache, Gerätedaten neutral (fest an) |
| 164 | Protokoll je Zelle: Verbindungen (Ziel, Name, Bytes), Uploads | app_zellen.rs („Log connections“, „Log uploads“); ultar-zellen/src/waechter.rs | teilw. §8 Datenschutz „Upload-Protokoll je App“ | Datenschutz-Zentrale › Protokoll: Schalter je App „Verbindungen“, „Upload-Anfang“; Ansicht mit Filter, Export |
| 165 | Speicher je Zelle (wächst, Grenze), RAM dynamisch/begrenzt | app_zellen.rs; leg/ZELLEN.md | fehlt | Zellen › Zelle › „Grenzen“: Speicher max GB, RAM max/garantiert, Kerne |
| 166 | Zellen verknüpfen (gemeinsamer Ordner, Name erreichbar) | app_zellen.rs („Linked with“) | fehlt | Zellen › Zelle › „Verknüpft mit“ (Liste, hinzufügen, lösen) |
| 167 | Grafikkarten-Verteilung: Automatisch · Alle (für KI summiert) · Aus · Karte n; je Zelle überschreibbar | app.rs `Screen::Gpu`; ultar-zellen/src/gpu.rs | fehlt | Treiber & Hardware › Grafik (Bedingung >1 Karte): „Verteilung“ + Hinweis; je App/Zelle „Grafikkarte“ unter Apps › je App |
| 168 | Kiri (Docker-kompatibel): installieren, Dienst, Containerliste, Container verknüpfen | app_zellen.rs („Containers (Kiri / Docker)“); ultar-zellen/src/kiri.rs | teilw. §7.2 Container & Server | Zellen & Container: Reiter „Container“ (Liste, Start/Stopp, Abbild, verknüpfen) |
| 169 | Empfehlung vor Doppelinstallation („schon etwas Ähnliches – in neuer Zelle nutzen / verknüpfen / trotzdem“) | app.rs `Screen::Empfehlung`; ultar-zellen/src/empfehlung.rs | fehlt | Store › App-Seite: Hinweisleiste vor „Installieren“ mit drei Aktionen |
| 170 | Internet-Regeln: Gerät selbst offline, nur erlaubte Zellen/Dienste online; Netz neuer Zellen | app_zellen.rs („Internet rules“); ultar-zellen/src/regeln.rs | fehlt (§8 Brandmauer nur Profil/offene Dienste) | Netzwerk › Brandmauer: „Ausgehend“ (alle · nur erlaubte Apps), Liste erlaubter Apps/Systemdienste, „Standard für neue Apps“ |
| 171 | Hinweis: eigener Ultar-Kern hat laut Unterlage keine Namensräume – Zellen nur mit Linux-Kern | leg/ZELLEN.md „Grenzen“ | Widerspruch zu §7.2 („.exe/.msi in eigener Zelle“, „läuft als normale App in einer Zelle“) – unsicher, Stand evtl. überholt | Vor Freigabe klären; sonst sind #161–170 und Rechte je App nicht umsetzbar |
| 172 | Installation `.uap`, `.deb`, `.exe/.msi`, `.apk` | system/core/ultar-apps; leg/AUSGABEN.md | ja §7.2, §7.3 | – |
| 173 | `.rpm` (über alien), AppImage, Flatpak | ultar-apps; AUSGABEN.md; tools.rs `installable` | fehlt | §7.3 „Datei installieren“ + Quelle: AppImage und Flatpak unter Komponente Linux-Programme nennen; `.rpm` streichen oder nennen. Entscheidung |
| 174 | Ablehnung mit Begründung (`.snap`, `.xapk`) | AUSGABEN.md | fehlt | §7.3 Zustand „Format nicht unterstützt“ mit Grund + Alternative im Store |
| 175 | Format-Richtlinie: fremde Formate erlauben/sperren, einzelne Formate sperren | AUSGABEN.md (`apps-policy.toml`, `uapp policy`) | teilw. §8 Datenschutz „unsignierte Pakete erlauben“ | Datenschutz & Sicherheit › „App-Quellen“: Schalter je Quelle (Linux · Windows · Android · Datei), verwaltbar per Richtlinie |
| 176 | Ausgaben Handy / Desktop / Server (Server ohne Oberfläche, Web-Verwaltung + SSH) | editions/; AUSGABEN.md | teilw. §5.5, §7.2 („Server: Standard“) – Installer §6 hat keine Wahl | §6 Schritt 8 Umfang: vierte Wahl „Server (ohne Oberfläche)“ → zeigt danach Adresse + Fingerabdruck der Web-Verwaltung |
| 177 | Web-Verwaltung `ultar-admin` (Übersicht, Dienste, Apps, System; HTTPS 9443) | system/services/ultar-admin; AUSGABEN.md | teilw. §7.2 Container & Server – in §8 Freigaben nicht | Freigaben: Zeile „Web-Verwaltung“ (an, nur dieser Rechner/Netz, Kennwort ändern, Fingerabdruck) |
| 178 | Startmenü: sichere Grafik, Rettung | AUSGABEN.md | fehlt in §6 | §6 vor Schritt 1: Starteinträge „Ultar OS · Sichere Grafik · Rettung“ beschreiben |
| 179 | Claude Code / Claude als Starter auf dem Desktop | AUSGABEN.md (unsicher: nur Unterlage) | fehlt | Komponente Coder: wählbares Werkzeug „Claude Code“; kein Grundsystem |

## E. Tastatur, Zwischenablage, Hilfe, Schnelleinstellungen

| # | Funktion/Einstellung | Wo im alten System (Datei) | Im Entwurf vorhanden? | Vorschlag |
|---|---|---|---|---|
| 180 | Bildschirmtastatur ar/en/de, Sprachtaste, Leertaste mit Sprachname | platform/ultar-ime/src/lib.rs; shell/kbd.rs | ja §5.3, §8 Tastatur | – |
| 181 | Zahlenreihe an/aus, Tastenvorschau, Größe klein/normal/groß | kbd.rs `KbCfg` | fehlt | Tastatur › Bildschirmtastatur: Schalter „Zahlenreihe“, „Tastenvorschau“, Auswahl „Größe“, Schalter „Vibration/Ton beim Tippen“ |
| 182 | Werkzeugleiste: Zwischenablage, Tastatur-Einstellungen, Helfer (übernimmt Feldtext), Ausblenden | kbd.rs; leg/TASTATUR.md | fehlt | §5.3 Bildschirmtastatur ausschreiben: Leiste mit Vorschlägen + Symbolen Klemmbrett · Zahnrad · Helfer · Ausblenden |
| 183 | Feldart-Belegungen: PIN, Telefon, Zahl, E-Mail, Adresse, Aktionstaste Suchen/Senden | ultar-ime `purpose_for` | fehlt | §3 Baustein „Eingabefeld“: Eigenschaft „Art“; §5.3 nennt die Belegungen |
| 184 | Langer Druck: Varianten (Umlaute, Hamza/Alif, Taschkil) | ultar-ime; kbd.rs | fehlt | §5.3 Bildschirmtastatur: „Halten = Varianten“; §8 Tastatur „Sonderzeichen-Taste“ gilt nur für Hardware |
| 185 | Ziffern immer 0–9, eingefügte indische Ziffern in Ziffernfeldern umwandeln | ultar-ime `normalize_digits` | ja §3 Ziffern | – |
| 186 | „Einfügen/Kopieren“ über der Tastatur, Verlauf | kbd.rs (Zwischenablage-Feld); app_more.rs | ja §5.3 Zwischenablage (Zugang per Tastatur fehlt) | wie #182 |
| 187 | Hilfe-Zentrum: Themen je Bildschirm, 3 Sprachen, Artikelansicht | shell/help.rs; app.rs `Screen::Help` | ja §9 Hilfe | Gliederung: je Einstellungs-Bereich und Grund-App ein Thema (Kennung = Flächen-Kennung aus §2.1) |
| 188 | Hilfe als Wissensbasis exportieren (Markdown) für Agent | help.rs `export`, `digest` | fehlt (nicht nutzersichtbar) | Bauregel: Hilfetexte aus dem Verzeichnis erzeugen; kein UI |
| 189 | Schnellschalter WLAN, Bluetooth, Mobile Daten, Flugmodus, VPN, Nachtlicht, Nicht stören, Helligkeit | quick.rs; A30-SCHNELLEINSTELLUNGEN.md; app_layout.rs | ja §5.1 | – |
| 190 | Schnelleinstellungen passen sich niedriger Höhe an; „+N · Verlauf“ | SYSTEM-FUNKTIONEN-2.md Feinschliff | fehlt (Zustand kompakt/niedrig nicht beschrieben) | §5.1/§5.5: Verhalten bei wenig Höhe festlegen (Kacheln 2 Zeilen + Blättern) |
| 191 | Verbindungsweg / Datenrate (Down/Up/Signal) auf Start und im Panel | app_layout.rs `conn_card`, `Rates` | fehlt | Schnelleinstellungen: Netz-Kachel zeigt Rate ↓/↑ als Unterzeile; Verbindungsweg gehört zum Mobilfunk-Prüfer |

## F. Dopplungen im alten System → ein Ort im neuen

| Funktion | Doppelt in | Ein Ort im Entwurf |
|---|---|---|
| Datum & Zeit | app.rs `s_time` („Date & time“) und app_sys.rs `Sx::Time` („Time zone & sync“) | Einstellungen › Datum & Zeit |
| Anzeige | app.rs `s_display` und app_sys.rs `Sx::DisplayX` („Night light & brightness“) | Einstellungen › Anzeige (Nachtlicht, Helligkeit); Hell/Dunkel nur in Aussehen |
| Hell/Dunkel | `s_display` „Dark mode“ und app_more.rs Modi „Automatic theme“ | Aussehen: Hell · Dunkel · automatisch |
| Ton | tools.rs `s_sound` und app_sys.rs `Sx::SoundX` („Sound profiles“) | Einstellungen › Ton |
| Nicht stören | SoundX-Schalter, Schnellschalter, Schlafmodus, Fokus-Timer schalten je selbst | Ein Zustand im Benachrichtigungsdienst; Zeitpläne/Modi (Schlafen, Fokus) setzen ihn mit Grund, der angezeigt wird |
| Einheitenrechner | tools.rs Rechner-Reiter „Units“ und app_sys.rs `Sx::Units` | Rechner › Umrechnen |
| Zahlensysteme | tools.rs Rechner-Reiter „Number systems“ und app_more.rs `Mx::Bases` | Rechner › Programmierer |
| Rechner insgesamt | Rechner + Prozent + Kredit + Datum + Währung als Einzelflächen in „Werkzeuge“ | eine App Rechner mit Modus-Menü |
| Weltuhr | tools.rs Uhr-Reiter „World“ und app_sys.rs `Sx::World` | Uhr › Weltuhr |
| Erinnerungen | app_more.rs `Mx::Reminders` (more.toml) und app_buddy.rs (`reminders.tsv`) und Kalender-Erinnerung | Uhr › Erinnerungen (ein Speicher); Kalender-Termine erinnern über denselben Dienst; Helfer nur Eingabeweg |
| Timer | Uhr-Timer, Fokus-Timer, Countdown | Uhr (Timer, Fokus); Countdown im Kalender |
| Prozesse beenden | monitor.rs Reiter „Processes“ und app_more.rs `Mx::Tasks` | Systemmonitor |
| Speicher | app.rs `s_storage`, app_more.rs `Mx::StorageX`, app_sys.rs Backup „Free up storage“ | Einstellungen › Speicher (+ App Belegung für die Karte) |
| Akku | app_sys.rs `Sx::Battery` und app_more.rs `Mx::BattHist`; Akku auch in `s_system` | Einstellungen › Energie |
| Zwischenablage | app_more.rs `Mx::Clipboard` und Feld in kbd.rs; Dateien haben eigene (tools.rs `clip`) | Zwischenablage-Dienst + Blatt Super+V; Tastatur-Leiste öffnet dasselbe Blatt; Dateien nutzen den Dienst |
| Suche | Suchleiste am Start, `Sx::Search`, Filterfeld in Einstellungen | Suche der Übersicht; Einstellungen-Suche = dieselbe Quelle gefiltert |
| Agent/Helfer | app_agent.rs (Agent), app_jarvis.rs (System-Assistent), app_buddy.rs (Figur), `Screen::Helper`; Einstellungen `s_agent_set` und Modellseite in app_jarvis.rs | eine Helfer-Fläche (Super+H / Figur öffnet dieselbe); ein Bereich Einstellungen › Helfer |
| Modell-Dienst | `llm` (ultar-llm) und `llm-compat` (llama-server im Ubuntu-Bereich) | nur Dienst `llm` |
| Taschenlampe | Schnellschalter (quick.rs), Werkzeug „Torch“ (Bildschirmlicht), SOS, Kamera-Licht | Kachel Taschenlampe; SOS in Notfall; Kamera-Licht bleibt in Kamera |
| Hijri | `Mx::Hijri`, Gebetszeiten-Seite, „Auf einen Blick“ | Kalender (Anzeige + Umrechnung); Leiste/Blatt zeigen nur an |
| Bildbetrachter | tools.rs `Screen::Viewer` und app_gallery.rs `Screen::Photo` | Grund-App Bilder |
| Texteditor | Code-Editor (app_office.rs) und Textanzeige im Viewer (tools.rs) | Texteditor (Grund) / Coder (Komponente) |
| Bildschirmfoto | Tasten, Schnell-Kachel (3 s), Verbund `foto` | eine Bildschirmfoto-Fläche; Kachel und Taste rufen sie |
| Links prüfen | Helfer-Feld und Datenschutz-Zentrum („Check a link now“) | Systemdienst + Kontextmenü (#121) |
| App-Installation | Store (KM-Apps/Ubuntu-Pakete), Dateien → uapp, Ubuntu-Programme-Seite, Web-Verwaltung, Agent (apt) | Ultar Store (alle Wege enden dort, auch Datei/Helfer) |
| Updates | `Screen::Update`, `Screen::Updates`, `s_system` „System update“, Store | Einstellungen › Updates (System) + Store › Aktualisierungen (Apps) – wie §7.3/§8, gegenseitig nur verweisen |
| SSH | Entwickleroptionen „Developer access (SSH)“ und Dienst `sshd`/Web-Verwaltung | Freigaben › SSH |
| Bildschirm teilen | Verbund-Fernansicht und Komponente Fernzugriff (§7.2, neu) | siehe #152 |
| Gerätename | `s_system`; im Entwurf zusätzlich Freigaben „Rechnername“ | System/Info |
| Hardware-Selbsttest / Treiber | `Screen::Hardware`, Gerätepflege, Sensoren-Seite, GNSS-Seite | Treiber & Hardware |
| Benachrichtigungen löschen | Panel „Alle löschen“, Verlauf „Clear current“ / „Clear history“ | Blatt Datum & Zeit: „Alle löschen“; Verlauf: „Verlauf löschen“ |

## G. Zählung

- Geprüfte Funktionen: 191 (A 80 · B 39 · C 27 · D 33 · E 12).
- Im Entwurf: ja 60 · teilweise 42 · fehlt 88 · Widerspruch 1 (#171).
- Dopplungen: 31.
