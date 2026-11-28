# Vollendung auf einem live laufenden Ultar-Server

Stand 28.11.2026. Diese Anleitung sammelt alles, was **nur an echter Hardware** fertig und geprüft werden
kann, in genau der Reihenfolge, in der es eine Sitzung an einem **live laufenden Ultar-Server** (Ultar OS
direkt auf dem Beelink, nicht über VFIO/QEMU) abarbeitet. Jeder Punkt nennt: Stand, die restlichen
Schritte und die genauen Befehle/Prüfungen.

Warum live: VFIO/QEMU aus der Cloud kann die geschlossenen-Firmware- und Ausgabe-Geräte nicht
voll prüfen – der BT-Controller kommt über USB-Durchreichung nicht in den Bootloader-Modus, WLAN
braucht Funk + AP, die Xe-Anzeige braucht einen Monitor. Alles davor (Reset, Register, Erkennung) ist
bereits über VFIO belegt (siehe die jeweiligen `docs/integration/*`).

## 0. Voraussetzung: Ultar OS live auf dem Beelink (S9)

1. Server-Abbild bauen: `EDITION=server bash scripts/build-pc-image.sh` → `out/ultar-os-server-x86_64.img`.
   Das Skript legt die vorhandene WLAN-/BT-Firmware aus `/lib/firmware` mit ins Abbild (siehe §1b, §3).
2. Auf einen USB-Stick schreiben, am Beelink davon starten (einmal **Freigabe des Nutzers**, Server ist
   dabei vom Netz). Ziel: Stufe 1 + der eigene Kern bootet auf echter Hardware (nicht nur KVM).
3. Ab hier laufen alle folgenden Punkte **auf dem Gerät** (Kern-Shell bzw. der lokale Agent), nicht über VFIO.

## 1. Bluetooth Intel (BE200) – Firmware-Ladung fertig

**Stand:** Erkennung (Read Version TLV) am echten BE200 belegt (`docs/integration/bt-intel.md`): Variante
0x1c, Modus wird erkannt. Offen: die Firmware-Ladung.

**Restliche Schritte (im Bootloader-Modus, d. h. nach einem Kaltstart des Geräts):**
1. `.sfi`/`.ddc` müssen im Abbild unter `/lib/firmware/intel/` liegen (dekomprimiert). Das Abbild-Skript
   legt sie bei (§1b). Erwartet am Beelink: `ibt-0040-0041.sfi`, `ibt-0040-0041.ddc`.
2. Die Firmware-Ladung an `bt.rs` anschließen (Funktion `download_intel_fw`, Gerüst vorbereitet):
   - Nur wenn `img == 1` (Bootloader). Firmware mit `fs::read_all("/lib/firmware/intel/<name>")` lesen –
     **nach** dem Einhängen des Dateisystems (nicht schon bei der USB-Erkennung; Reihenfolge beachten,
     ggf. einen verzögerten Init-Schritt nach `fs`-Mount).
   - Secure Send (0xFC09), Fragmenttypen: 0x00 Kopf (CSS), 0x03 öffentl. Schlüssel, 0x02 Signatur,
     0x04 LMS-Signatur, 0x05 LMS-Schlüssel, 0x01 Daten. BE200 (Variante 0x1c, RSA+ECDSA):
     - RSA: CSS 128 B @0 (0x00), Pubkey 256 B @128 (0x03), Signatur 256 B @388 (0x02).
     - ECDSA: CSS 128 B @644 (0x00), Pubkey 96 B @772 (0x03), Signatur 96 B @868 (0x02).
     - Nutzlast ab Offset 964: HCI-Befehlsköpfe (2 B Opcode, 1 B Länge) laufen, bis die Summe durch 4
       teilbar ist, dann den Block als **ein** Typ-0x01-Fragment senden.
   - Auf das Download-Fertig-Ereignis warten (Intel-Vendor-Ereignis), dann **Intel Reset** (0xFC01,
     `struct intel_reset` 8 B: reset_type, patch_enable, ddc_reload, boot_option, boot_param LE32) mit
     dem `boot_param` aus dem `Write Boot Params`-Befehl (0xFC0E) in der Nutzlast. Boot-Ereignis abwarten.
   - `.ddc` über 0xFC8B-Datensätze laden (`[len][id LE16][value]`, Länge = len+1).
   - Danach meldet sich das USB-Gerät **neu an** – der Treiber nimmt es erneut auf, Read Version sollte
     nun „operativ" melden.
3. **Prüfen (live):** `hilfe`/Geräte in der Kern-Shell → `[bt] … operativ`; dann ein zweites BT-Gerät
   suchen/paaren (braucht ein Gegenstück).

## 2. WLAN Intel (BE200, `gl-c0-fm-c0`) – das große Paket

**Stand:** ⬜. Firmware am Gerät vorhanden (`/lib/firmware/iwlwifi-gl-c0-fm-c0-*.ucode`, `…pnvm`). Dies ist
das größte Einzelpaket: geschlossene Firmware **plus** eigener 802.11-Stapel (WPA2/WPA3). Eigene Welle.

**Restliche Schritte (gestaffelt, je Stufe am Gerät prüfbar):**
1. **W1 Bringup/VFIO:** PCI (ac:00.0 8086:272b), Reset, MAC/CSR, Firmware (`.ucode` TLV-Format) in den
   Gerätespeicher laden, PNVM, ALIVE-Ereignis, NVM lesen. (Teilweise schon über VFIO prüfbar.)
2. **W2 MAC/Queues:** TX/RX-Ringe, Befehls-/Antwortkanal (host command), Interrupt (MSI-X), Kalibrierung.
3. **W3 802.11:** Scannen (aktive Antwort-Liste), Verbindung zu einem AP, Authentifizierung.
4. **W4 WPA2/WPA3:** 4-Wege-Handschlag, Schlüssel (CCMP/GCMP) – an die `NetDev`-Schnittstelle binden.
5. **Prüfen (live):** `netz`/`dhcp` über WLAN an einem **echten AP**; „verbindet sich" nur so belegbar.

Hinweis: W3/W4 sind ein eigener 802.11-Stapel – realistisch mehrere Sitzungen. Linux `iwlwifi`/`mac80211`
dienen als Beschreibung (kein GPL-Code), die Firmware wird nur geladen.

## 3. Xe-Anzeige (Intel Arc 140T) – Bild auf echtem Monitor

**Stand:** Rechnen/3D über den GT ist fertig und am Beelink belegt (`docs/integration/intel-gt.md`, G4/G5:
Modell läuft auf der GPU). Die **Anzeige** (Modus setzen, Bild ausgeben) fehlt; heute Bild über den
UEFI-GOP-Bildspeicher. Der Anzeige-Treiber-Bauplan liegt in `drivers/kernel/intel-display/`.

**Restliche Schritte (am Beelink mit Monitor):**
1. **X1:** Anzeige-Register übernehmen (GOP/Firmware-Zustand ablösen), Taktquellen, PLLs.
2. **X2:** Ausgang erkennen (DP/HDMI über den PHY), EDID lesen, Modus setzen (Pipe/Plane/Transcoder).
3. **X3:** Bildspeicher (Plane) auf den Compositor umlenken; Helligkeit; Schlafmodus; Hotplug.
4. **Prüfen (live):** ein Testbild/Compositor am angeschlossenen Monitor sichtbar – das ist der einzige
   echte Beleg. Darum ist X ohne Monitor am headless-Beelink nicht verifizierbar.

## 4. Weitere live abzuschließende Punkte (bereits begonnen)

- **S10 iGPU beim normalen Start:** heute startet der GT nur nach dem Selbsttest; beim normalen Boot in
  Betrieb nehmen und den Modell-Dienst anbinden. Am Gerät nach dem Direktstart (S9) prüfen.
- **NVMe-Schreibtest:** die Crucial-Platten tragen Linux; der Schreibweg ist erst beim Direktstart (S9)
  gefahrlos prüfbar (eigene Platte/Partition).
- **VT-d auf echter Hardware:** die 2 IOMMU-Einheiten des Beelink erst beim Direktstart (nicht über die
  geschachtelte Gast-VT-d) prüfen.
- **SD-Karte lesen:** `gen-sdhci` ist am echten O2 Micro hochgefahren (Reset/Fähigkeiten/Erkennung
  belegt); mit **eingelegter Karte** den Lesepfad (CMD17/18) und danach das Schreiben prüfen.

## Reihenfolge der Live-Sitzung

S9 (Ultar live auf dem Beelink) → BT-Firmware fertig → SD mit Karte → NVMe-Schreibtest + VT-d live →
S10 iGPU beim Start → WLAN W1→W4 → Xe X1→X3. Jeder Punkt wird sofort am Gerät bestätigt.
