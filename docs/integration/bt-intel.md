# Bluetooth Intel – Firmware und Bringup (`drivers/gen_xhci/bt.rs`)

Stand 28.11.2026.

Der USB-HCI-Host (`bt.rs`) stellt jeden Bluetooth-Controller über `/dev/ultar/hci` bereit (Ereignis-,
ACL-, Befehls-Endpunkte). Intel-Controller (z. B. BE200, `8087:0036`) starten jedoch im **Bootloader**
und brauchen zuerst ihre **Firmware**, bevor sie funktionieren.

## Stand

| Stufe | Inhalt | Stand |
|---|---|---|
| Erkennung | HCI „Intel Read Version" (TLV, 0xFC05/0xFF) auf EP0 senden, Command-Complete lesen, TLVs parsen (CNVI/CNVR_TOP, CNVI_BT-Variante, Image-Typ), Firmware-Name `ibt-<cnvi>-<cnvr>.sfi` bilden | ✅ **am echten BE200 (USB-Durchreichung) belegt**: „Variante 0x1c, Modus operativ" – das in-Kernel-Vendor-HCI funktioniert auf echter Hardware |
| Firmware-Ladung | `.sfi` in Teilen über „Intel Secure Send" (0xFC09, Typen CSS/Daten/Signatur/Schlüssel) laden, Download-Ereignis abwarten, „Intel Reset" (0xFC01) zum Start der operativen Firmware, `.ddc`-Konfiguration (0xFC8B) | ⬜ offen |

## Was die Ladung noch braucht (und warum nicht in der Cloud prüfbar)

1. **Bootloader-Modus**: Der Controller lädt die Firmware nur im Bootloader-Modus. QEMUs `usb-host`
   setzt den Controller nicht kalt zurück (die im RAM liegende Firmware bleibt → „operativ"). Ein
   echter Kaltreset/Strom-aus am Gerät ist nötig, um den Bootloader-Modus und damit den korrekten
   Lade-Firmware-Namen (am Beelink `ibt-0040-0041.sfi`) zu sehen. Im operativen Modus meldet die
   Version andere CNVi/CNVr (hier 0291) – das ist der Build der laufenden Firmware, nicht der Lade-Name.
2. **Firmware im Abbild**: Die `.sfi`/`.ddc` (geschlossen, ~500 KB) müssen ins Ultar-Abbild (nur geladen,
   nicht verändert) – entweder beigelegt oder vom Installer vom Gerät geholt.
3. **Secure-Send-Ablauf** (nach btintel als Beschreibung): CSS-Version bei Offset 8 (RSA 0x00010000 /
   Hybrid 0x00069700); je nach Variante RSA-, ECDSA- oder Hybrid-Kopf senden (Secure-Send-Typen 0x00
   Kopf, 0x03 öffentl. Schlüssel, 0x02 Signatur, 0x04/0x05 LMS), dann die Nutzlast (HCI-Befehle,
   4-Byte-ausgerichtet) als Typ-0x01-Fragmente; Nutzlast-Offset 644 (RSA) / 964 (RSA+ECDSA) / 2116
   (Hybrid). Danach „Intel Reset" mit dem Boot-Parameter aus `Write Boot Params` (0xFC0E) im `.sfi`.
4. **Neu-Enumeration**: Nach dem Reset meldet sich das USB-Gerät neu an – der Treiber muss das Gerät
   danach erneut aufnehmen.

Das Paaren selbst braucht zusätzlich ein zweites Bluetooth-Gerät.
