# Ultar A30 – Bluetooth (eigener Host)

Stand 19.11.2026. Code: `crates/ultar-bt` (Bibliothek + Werkzeug), Oberfläche `crates/ultar-shell/src/app_bt.rs`.

## Lage
Der Kernel des A30 hat kein Bluetooth (kein BlueZ, `CONFIG_SCSC_BT_BLUEZ` aus, kein rfkill). Der Chip
(Samsung SCSC „Maxwell", zusammen mit WLAN) erscheint nur als Zeichengerät **/dev/scsc_h4_0**: rohe HCI-Pakete im
H4-Format (1 Byte Typ: 1 Befehl, 2 ACL, 3 SCO, 4 Ereignis). Öffnen startet den Bluetooth-Dienst im Chip
(Konfiguration `/vendor/etc/wifi/mx140_bt.hcf`), Schließen stoppt ihn.

## Was läuft (am Gerät bestätigt)
`ultar-bt scan 12`:
- Chip startet, Reset, Ereignismasken, Adresse und Version gelesen (HCI 5.0, Hersteller 0x0075 Samsung).
- Suche klassisch (Inquiry, mit Empfangsstärke und Namen) und Low Energy (aktiv) gleichzeitig – ein LE-Gerät
  in der Nähe wurde gefunden.
- Ausgabe je Gerät: `ADRESSE<TAB>le|br<TAB>RSSI<TAB>KLASSE<TAB>NAME`, zusätzlich `/run/ultar/bt/scan.tsv`.

Oberfläche: Einstellungen → Bluetooth (auch über Netzwerk): Schalter, eigene Adresse, „Geräte suchen", Liste
mit Art (aus der Geräteklasse) und Empfang. Schalter ohne rfkill: Datei `/data/ultar/bt.on`.

## Offen (nächste Pakete, in dieser Reihenfolge)
1. Dauerdienst statt Einzelaufruf (Chip bleibt an, sichtbar/verbindbar, Namen nachfragen).
2. Verbindungen: ACL, L2CAP, Koppeln (SSP/„Just Works" + Zahlenvergleich, LE-Sicherheit), Schlüssel speichern.
3. Profile: Tastatur/Maus (HID/HOGP) zuerst, dann Audio (A2DP + AVDTP + SBC, Weg über ABOX), Freisprechen.
4. Feste Geräteadresse aus EFS (`bluetooth/bt_addr`) statt der Ersatzadresse des Chips.
