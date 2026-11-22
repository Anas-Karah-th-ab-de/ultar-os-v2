# Ultar Flash (Windows)

Sichert ein Android-Handy, flasht Ultar OS und stellt das Original wieder her.
**Stand: Logik mit Geräte-Attrappen getestet (`test/run-tests.sh`, 79 Prüfungen). Nicht an einem echten Handy und nicht unter Windows getestet.**

## Vorbereitung
1. `UltarFlash-<version>.msi` installieren (oder `UltarFlash.exe` in einen Ordner legen). Dann einen Ordner `tools`
   anlegen – neben der .exe oder unter `Dokumente\UltarFlash\tools` – mit:
   - `adb.exe`, `fastboot.exe` (+ die beiden `Adb*.dll`) aus den Android „platform-tools“
   - für Samsung zusätzlich `heimdall.exe` (Heimdall) und dessen USB-Treiber (Zadig)
2. Am Handy: Entwickleroptionen → USB-Debugging einschalten.

## Geführter Ablauf
„Geführter Ablauf starten“ nimmt den Nutzer an die Hand: lädt bei Bedarf adb/fastboot von Google, zeigt Schritt für
Schritt, was am Handy zu tun ist (Entwickleroptionen, USB-Debugging, OEM-Entsperrung, Freigabe bestätigen), erkennt
selbst, wann ein Schritt erledigt ist, und sichert dann Treiber und – mit Root – die Partitionen.

## Einzelschritte
| Schritt | Was passiert | Voraussetzung |
|---|---|---|
| Gerät erkennen | Modell, Zustand, Akku, Root, Bootloader-Sperre | USB-Debugging |
| Treiber & Infos sichern | Hersteller-Firmware und -Treiber (`/vendor/firmware`, `/vendor/lib/modules`, WLAN-/Bluetooth-Dateien), Kernel-Konfiguration, Geräteliste | keine (ohne Root sind einzelne Dateien nicht lesbar) |
| Vollsicherung | Partitionen boot, recovery, dtb(o), vbmeta, **efs/sec_efs (IMEI, Funkdaten)**, param, modem … als Abbild; jede Datei wird gegen die Prüfsumme auf dem Gerät geprüft | Root oder Custom-Recovery (z. B. TWRP) |
| Ultar OS flashen | schreibt die Abbilder aus einem Ultar-Paket | geprüfte Vollsicherung dieses Geräts, entsperrter Bootloader, Akku ≥ 50 %, Paket passt zum Gerät, Modell zur Bestätigung eintippen |
| Original wiederherstellen | schreibt die gesicherten Partitionen zurück | Sicherungsordner, Gerät im Download-/Fastboot-Modus erreichbar |

## Schreiben auf Samsung-Geräte
Ultar Flash schreibt selbst über den COM-Port, den Windows für den Download-Modus anlegt (Odin/Loke-Protokoll,
`Loke.cs`) – ohne heimdall und ohne Treibertausch. Voraussetzung: der Samsung-USB-Treiber (bzw. der serielle
Standardtreiber von Windows). Ablauf in einer Sitzung: Partitionstabelle lesen (noch nichts geschrieben), prüfen,
Abbilder senden, Neustart. **Experimentell: bisher nur gegen eine Geräte-Attrappe geprüft.**
Mit der Umgebungsvariable `ULTARFLASH_USE_HEIMDALL=1` wird stattdessen heimdall benutzt.

## Sicherheitsregeln (fest eingebaut)
- Geschrieben werden nur: boot, recovery, dtbo, dtb, vbmeta, system, vendor, super, userdata, cache.
  **Bootloader, Partitionstabelle, Modem und EFS werden nie geschrieben** – so bleibt der Download-/Fastboot-Modus
  immer erreichbar und das Zurückschreiben möglich.
- Ohne geprüfte Sicherung jeder betroffenen Partition (bei Samsung zusätzlich EFS) wird nicht geflasht.
- Beschädigte Sicherungen oder Abbilder (Prüfsumme) werden abgelehnt.

## Ultar-Paket selbst bauen („Ultar-Paket bauen …“)
Ultar Flash baut das Paket aus dem **Original-Boot-Abbild des Geräts**: Der Kernel des Herstellers mit allen Treibern
bleibt, nur das Start-Ramdisk wird durch Ultar OS ersetzt (`ultar-initramfs-aarch64.cpio.gz`, liegt neben der .exe).
Quelle für das Original: die Samsung-Firmware des Geräts (`AP_….tar.md5`), eine `boot.img` oder – mit Root – die Vollsicherung.

Dabei passiert:
- `skip_initramfs` im Kernel wird umbenannt (ältere Geräte übergehen sonst das Ramdisk),
- `vbmeta` bekommt die Prüfung abgeschaltet (sonst lehnt der Bootloader das geänderte Abbild ab),
- die unveränderten Originale werden als Wiederherstellungs-Sicherung abgelegt,
- zusätzlich entsteht `ultar-<gerät>-odin-AP.tar` für Odin, falls Heimdall das Gerät nicht bedienen kann.

### Galaxy A30 (SM-A305F) – Reihenfolge
1. Firmware passend zum Stand des Handys besorgen (Build-Nummer steht in `device.txt`, z. B. `A305FDDS6CWA3`) und die AP-Datei entpacken.
2. Bootloader entsperren (**löscht alle Daten, löst Knox aus**): Entwickleroptionen → OEM-Entsperrung; Handy aus;
   Lauter + Leiser halten und USB-Kabel anstecken; Lauter lange drücken und bestätigen. Danach Android neu einrichten,
   einmal mit dem Internet verbinden, USB-Debugging wieder einschalten.
3. In Ultar Flash „Testgerät – darf verloren gehen“ anhaken, „Ultar-Paket bauen …“ → AP-Datei wählen.
4. „Ultar OS flashen …“ → Paketordner bestätigen, Modellbezeichnung eintippen.
5. Zurück zu Android: „Original wiederherstellen …“ oder die komplette Firmware mit Odin flashen.

Erkennungszeichen beim Start: ein **goldener Balken oben** heißt, der Kernel hat Ultar OS gestartet; danach sollte das
Logo erscheinen. **Dieser Weg ist an keinem echten Gerät erprobt.** Samsungs Kernel-Schutz (Knox/RKP/DEFEX) kann den
Start verhindern; dann bleibt der Bildschirm dunkel oder das Handy startet neu – in dem Fall über den Download-Modus
(Leiser + Lauter + Kabel) wiederherstellen. WLAN, Mobilfunk und Akkuanzeige sind auf dem Handy noch nicht eingerichtet.

## Ultar-Paket (Aufbau)
Ordner mit `ultar-package.txt`:
```
device=a30,a30dd
image.boot=boot.img
sha256.boot=<prüfsumme>
```
Für das Galaxy A30 gibt es noch kein Paket – der Port auf den Samsung-Kernel steht aus. Bis dahin sind
Sicherung und Treiberauslese nutzbar; die gesicherten Treiber sind die Grundlage für den Port.

## Grenzen
- Die Vollsicherung braucht Root oder eine Custom-Recovery. Ein unverändertes Samsung-Handy lässt sich im
  Download-Modus nicht auslesen.
- Bootloader entsperren löscht alle Daten und löst bei Samsung Knox dauerhaft aus (Garantie, manche Apps).
- EFS wird gesichert, aber nicht automatisch zurückgeschrieben.

## Updates
Ultar Flash sieht beim Start in den GitHub-Releases des Projekts nach einer neueren Version, lädt die MSI,
prüft sie gegen `SHA256SUMS` und startet die Installation. Solange das Projekt privat ist, im Feld
„GitHub-Token“ ein Token mit Lesezugriff eintragen.

## Kommandozeile
`UltarFlash.exe --cli detect | drivers <ordner> | full <ordner> [--large] | check <paket> <sicherung> | flash <paket> <sicherung> --yes <modell> | restore <sicherung> --yes <modell> | update [--download]`
