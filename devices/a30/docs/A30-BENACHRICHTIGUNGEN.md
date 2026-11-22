# Benachrichtigungen, Alarmieren & Näherungssensor

## Schnittstelle (Crate `ultar-notify`)
Jede App/jeder Dienst meldet Ereignisse hierüber; die Oberfläche zeigt und alarmiert.
Transport: eine Zeile je Benachrichtigung an `/run/ultar/notify.log`:
```
<ts>\t<urgency>\t<kind>\t<app>\t<title>\t<body>
```
- **urgency**: `low` | `normal` | `high` (high = sofort alarmieren + Bildschirm wecken, z. B. Anruf)
- **kind**: `call` | `sms` | `message` | `system` | `app`

**Aus Rust:** `ultar_notify::post(kind, app, title, body, Urgency::High)` oder `ultar_notify::info(app, title, body)`.
**Aus Dienst/Skript/Tür:** `unotify <kind> <app> <titel> [text] [--urgency high|normal|low]`.

## Alarmieren (auch in der Tasche)
Die Oberfläche liest neue Zeilen laufend (Wächter-Thread weckt die Ereignisschleife auch bei
Bildschirm aus, Latenz ≤ ~0,4 s) und reagiert je nach urgency:
| urgency | Ton | Vibration | Bildschirm |
|---|---|---|---|
| high (Anruf) | ja (Alarmton) | kräftig (600 ms) | weckt auf |
| normal | ja (Hinweiston) | kurz (nur bei Bildschirm aus) | nein |
| low | nein | nein | nein |
Ton nur, wenn in Einstellungen → Ton aktiv. Vibration über `/sys/.../vibrator`. Die neueste
Benachrichtigung erscheint kurz als **Kopf-Hinweis** oben (Tippen öffnet das Panel), alle im
Wisch-von-oben-Panel.

## Angebundene Apps
- **Telefon**: eingehender Anruf (Ultar-/SIM-Nummer) → `call/high` (automatisch aus dem Anrufzustand).
- **Uhr**: Wecker/Timer → `system/high`.
- **Store**: Installation fertig → `app/normal`. **Update**: Update verfügbar → `system/normal`.
- **Weitere** (Telegram, SMS, eigene Apps): `ultar_notify::post` bzw. `unotify` aufrufen — gleiche
  Oberfläche, gleiches Alarmieren.

## Näherungssensor bei Anrufen (Handy am Ohr)
Während ein Anruf läuft (`call_active`: klingelt/wählt/verbunden) prüft die Oberfläche den
Näherungssensor (`sys::proximity_near`, Samsung-Sensorhub oder IIO). **Verdeckt (am Ohr)** →
Bildschirm aus und Berührungen werden ignoriert (keine Fehleingabe mit der Wange); **frei** →
Bildschirm wieder an. Gilt für SIM-Anrufe und Ultar-Nummer-Anrufe. Für WhatsApp Web (eigener
Browserprozess) ist der Anrufzustand von außen nicht sichtbar — dort greift die Automatik nicht.
