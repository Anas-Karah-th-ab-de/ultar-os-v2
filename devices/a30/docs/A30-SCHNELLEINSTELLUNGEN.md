# Schnelleinstellungen & Benachrichtigungen (Wisch von oben)

Vom oberen Bildschirmrand nach unten wischen öffnet das Panel (zurück/Griff/Wisch nach oben schließt).

## Inhalt
- **Uhrzeit** und **Akku**: Prozent + Zustand (Lädt / Voll / Akkubetrieb), grün beim Laden, rot unter 15 %.
  (Dieselbe Anzeige steht dauerhaft in der Statusleiste.)
- **Schnellschalter** (Kachel leuchtet gold = an): WLAN, Bluetooth, Mobile Daten, Flugmodus,
  Taschenlampe, VPN, Ton, Datenschutz.
- **Helligkeit**: Schieberegler (wirkt sofort, Hardware-Backlight oder Software).
- **Benachrichtigungen**: Liste, sonst „Keine Benachrichtigungen".

## Schalter-Backend (`crates/ultar-shell/src/quick.rs`)
| Schalter | Wirkung |
|---|---|
| WLAN | rfkill wlan + `ip link wlan0 up/down` |
| Bluetooth | rfkill bluetooth |
| Flugmodus | rfkill alle + mobile Daten aus |
| Mobile Daten | Flag `/run/ultar/mobiledata.on` + `ip link rmnet0` (greift, sobald Modem/RIL aktiv) |
| Taschenlampe | `/sys/class/leds/*torch*|*flash*/brightness` |
| VPN | `ultar-vpn` verbinden/trennen |
| Ton / Datenschutz | Einstellungen (sound / Datenschutz-Hauptschalter) |

## Benachrichtigungen einspeisen
Dienste/Apps hängen Zeilen an `/run/ultar/notify.log` (flüchtig) oder `/data/ultar/notify.log` an.
Format je Zeile: `Titel<TAB>Text` (Text optional). Neueste zuerst, bis zu 10 werden angezeigt.
```sh
printf 'Update verfügbar\tv0.6.1 bereit zum Installieren\n' >> /run/ultar/notify.log
```
