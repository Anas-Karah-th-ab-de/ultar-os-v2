# Ultar OS v2 – Stand

Stand 28.11.2026 · alle Zahlen gemessen in dieser Sitzung (Bau-Rechner: 2 Kerne, QEMU ohne KVM)

## Fremde Apps, Stufe P1: Wayland-Brücke (28.11.2026, Zweig feat/fa-p1-wayland)

| Baustein | Stand |
|---|---|
| `ultar-wayland` | Wayland-Server in reinem Rust (ohne libwayland, statisch baubar) für **eine** App-Zelle; gibt Fenster als gewöhnlicher Client an `ultar-compositor` weiter (Fenster, Unterflächen, Menüs, Tastatur ar/en/de, Zeiger, Touch, Zwischenablage, Maximieren/Vollbild/Minimieren, Schließen). Grenzen je Verbindung und gegenüber dem Compositor; falsche Anfragen beenden nur die eine Verbindung |
| Geprüft | `tools/kern-test/wayland-e2e.sh` → `WAYLAND: ALLES OK`: weston-simple-shm, weston-terminal, foot, gtk4-demo, Qt 6, SDL3 mit Bildschirmfotos; Fuzz-Test (200 Verbindungen); auf dem Ultar-Kern in QEMU `WAYLAND-QEMU: OK` |
| Offen | GTK 4 zeigt zwei Titelleisten ohne Recht `CLIENT_HEADER`; keine GPU-Puffer, kein Xwayland, keine Eingabemethode – siehe `docs/integration/ultar-wayland.md` |

## Systemauslastung auf dem Schreibtisch, Logo ohne Platte (28.11.2026)

| Baustein | Stand |
|---|---|
| Systemanzeige | Karte oben in der Ecke gegenüber den Symbolen: Prozessor, Arbeitsspeicher (belegt von gesamt), Grafik, Datenverkehr (↓/↑), je mit einer Minute Verlauf als Balken (Upload als Linie); alle 2 s neu gelesen und an Ort und Stelle geändert (kein Neuaufbau, Doppelklick bleibt sicher); nicht Messbares zeigt „–“ |
| Einstellung | `desktop.metrics.enabled` (Standard an, Einstellungen › Arbeitsfläche & Leiste) und „Systemauslastung anzeigen“ im Rechtsklick-Menü des Schreibtischs |
| Ultar-Kern | `/proc/stat` zählt Arbeits- und Leerlaufzeit aller Kerne (vorher stand dort nur die Zeit seit dem Start – die Last wäre immer 100 %), `/proc/net/dev` mit dem Datenverkehr der Netzwerkkarten, `/proc/ultar/gpu_busy_ns` aus den Auftragszeiten der Intel-GT; QEMU: `kerntest` ALLES OK, Zähler steigen |
| Logo | das Firmenlogo (transparenter Hintergrund) steht in beiden Themen ohne dunkle Platte; auf dem Schreibtisch ruhig (dunkel halbe, hell drei Viertel Stärke) |
| Geprüft | `tests/metrics.rs` (Werte in der Sprache, Ecke, Bilder), Bildvergleiche aller Flächen neu |

## Rechtsklick auf den Schreibtisch, App-Verknüpfungen und Schreibtisch-Ordner (28.11.2026)

| Baustein | Stand |
|---|---|
| Rechtsklick auf den Hintergrund | Menü am Zeiger (öffnet zur Endseite, kippt an Rand und Leiste): Neue Verknüpfung…, Neuer Ordner (Files legt ihn im Schreibtisch-Ordner an, Name zum Tippen), Einfügen (gesperrt: Files hält seine Zwischenablage je Fenster), große/mittlere/kleine Symbole, Schreibtischsymbole anzeigen, Nach Name sortieren, Hintergrund ändern, Anzeigeeinstellungen, Schreibtisch anpassen, Im Terminal öffnen |
| Symbole auf dem Schreibtisch | in dieser Reihenfolge: Persönlicher Ordner, eingehängte Laufwerke, App-Verknüpfungen, Dateien und Ordner des Schreibtisch-Ordners (Bereich `desktop`, Ordner zuerst, alle 2 s neu gelesen, versteckte Namen und Verweise ausgelassen); Klick wählt, Doppelklick öffnet (Ordner/Laufwerk in Files, Datei mit ihrer Standard-App über `launcher.resolve`, keine App → Hinweis) |
| Rechtsklick auf ein Symbol | Verknüpfung: Öffnen, Umbenennen (Dialog), An Leiste anheften/lösen, Vom Schreibtisch entfernen, Eigenschaften; Datei/Ordner: Öffnen, In Dateien zeigen, Umbenennen, In den Papierkorb, Eigenschaften – die Änderung macht Files (`files://desktop/<name>?action=rename|trash|properties`, mit Rückgängig); Laufwerk: Öffnen, Auswerfen; Persönlicher Ordner: Öffnen, Eigenschaften |
| Verknüpfungen anlegen | „Neue Verknüpfung…“ (App-Raster wählt die App), im App-Raster „Zum Schreibtisch hinzufügen“/„Vom Schreibtisch entfernen“, oder eine App aus dem App-Raster auf den Schreibtisch ziehen (während des Ziehens zeigt der Hintergrund blass, wo sie hinkommt) |
| Gespeichert | Verknüpfungen je Person neben der Hülle (`<state>/<person>/desktop.json`), beim Start gelesen; nicht installierte Apps werden nicht gezeigt |
| Rechte | die Hülle (816) ist in der Gruppe `userfiles` und liest nur den Bereich `desktop` (`--desk-folder`); Bus-Recht `launcher.resolve` |
| Einstellungen | `desktop.icons.enabled`, `.size`, `.start_corner` wirken sofort |
| Werkzeugkasten | `BrickKind::DeskIcon`, `Ui::set_brick` (Auswahl ohne Neuaufbau, damit der Doppelklick gelingt), `HostEvent::Menu` trägt den Zeigerpunkt |
| Geprüft | `tests/desk.rs`, `tests/desk_items.rs` (Klicks, Menüs, Reihenfolge, Ecken, Arabisch, Ziehen), Files `tests/desk_actions.rs`, CLICKS `desk.icon` jetzt in der Hülle geprüft; Bildvergleiche aller Flächen neu (der Schreibtisch zeigt jetzt den persönlichen Ordner wie Tafel „Main“) |

## Logo, Ein/Aus und Einstellungen, die sofort wirken (28.11.2026)

Befund des Eigentümers in VMware (v2 gegen v1): das Firmenlogo fehlte an mehreren Stellen, besonders bei der
Installation; der Ein/Aus-Knopf fehlte; Änderungen in den Einstellungen wurden nicht übernommen. Ursache des
dritten Punkts: viele Einstellungen hatten keinen Abnehmer – u. a. wurde die bei der Installation gewählte
Sprache nirgends angewandt, Apps bekamen weder Sprache noch Thema, Uhr und Sperrbildschirm zeigten UTC.

| Baustein | Stand |
|---|---|
| Logo | `BrickKind::Logo` (Bitmap aus `ultar-brand`, flächengemittelt verkleinert, dunkle Platte im hellen Thema); Installer: Willkommen, Schreiben, Fertig; Einstellungen › Info; Sperrbildschirm; Desktop-Hintergrund „ultar-noir“ (Logo ruhig in der Mitte wie in v1); Kern: Startbild und Text-Installer |
| Ein/Aus | im App-Raster (Startmenü) unten am Ende, öffnet das Ein/Aus-Menü (Ruhezustand, Neustart, Ausschalten, Abmelden, Benutzer wechseln) |
| Sprache, Thema, Akzent, Textgröße | sofort und ohne Neustart: Leiste und Blätter, Sperrbildschirm, Systemdialoge, Bildschirmtastatur, Einstellungen-Fenster, Titelleisten des Compositors (`Request::ConfigureLook` über displayd), jede neu gestartete App (Launcher übergibt `ULTAR_LANG`/`ULTAR_THEME`/`ULTAR_ACCENT`/`ULTAR_FONT_SIZE`) |
| Uhr | Zeitzone mit Sommerzeit (`ultar_i18n::tz`), 12/24 h, Sekunden, Wochentag, Datum, Hidschri – Leiste und Sperrbildschirm |
| Leiste | Symbole mittig/am Anfang, Papierkorb, Laufwerke, Akku-Prozent |
| Ehrlichkeit | Einstellungen ohne Abnehmer in acht der ersten Bereiche zeigen jetzt „In dieser Version noch nicht verfügbar“ statt nur einen Wert zu speichern (`docs/integration/settings-wiring.md` §2) |

## Welle B (28.11.2026) – SSH auf den Ultar-Kern, USB-C-Netz, Doku-Abgleich

| Baustein | Stand |
|---|---|
| **SSH auf den Ultar-Kern** | `ultar-sshd` (reines Rust) läuft auf dem eigenen Kern: Schlüssel-Anmeldung, Befehle, Exit-Code, scp/sftp, fremder Schlüssel und Kennwort abgewiesen, parallele Sitzungen – `ssh-e2e.sh` (`SSH: ALLES OK`) in `ci.sh`. Server-/Desktop-Ausgabe ohne OpenSSH aus dem Ubuntu-Bereich, ein Dienst (`entwickler`), Server standardmäßig an (nur mit Schlüssel); Web-Verwaltung schreibt Schlüssel dorthin, wo `ultar-sshd` liest. Offen: Empfangstempo ~0,3 MB/s (`docs/integration/ssh.md`) |
| **USB-C-Netz** | RNDIS-Empfang behoben (Ursache: eigener RNDIS-`SET` mit 28 statt 32 Byte, nicht QEMU); DHCP, DNS, TCP, ping über `usb0`; Abziehen gibt Adresse und Standardroute ab, erneutes Anstecken (`usb1`) übernimmt sie – `usb-net-e2e.sh` in `ci.sh`. CDC-ECM-Treiber da, braucht einen echten Dongle |
| Doku-Abgleich | `docs/review/ABGLEICH.md`: jeder offene Punkt gegen den Code geprüft; STATUS, OFFENE-BEFUNDE, DRIVER-TARGETS, PLAN berichtigt |
| TLB-Abgleich | Entwurf + halbe Umsetzung auf `wip/tlb-shootdown`, nicht übernommen |

## Treiber-Welle A (28.11.2026) – Netzwerk-Fundament und DMA-Schutz

| Baustein | Stand |
|---|---|
| **MSI/MSI-X** (`msi.rs`) | Gemeinsamer Rahmen: Vektoren 0x90–0xEF, je Vektor ein Handler (bekommt seinen Vektor), Einzel-MSI und MSI-X-Tabelle in einem BAR. 96 IDT-Tore aus einer Einsprungtabelle. Fundament für Unterbrechungen je Warteschlange (10G, NVMe) statt reinem Abfragen |
| **10G-Netz (E610)** | `gen-ixgbe` richtet MSI-X für RxQ0 ein (Auto-Maske + Auto-Quittung + EITR-Drossel → kein Sturm). **Am echten E610-Port des Beelink über VFIO belegt**: Treiber bindet, Firmware 7.8.18, Kabel erkannt, und nach dem Boot zeigt `netz` „10G MSI-X-Unterbrechungen: 1" (die im Treiberstart bei IF=0 angestoßene Schleifentest-Unterbrechung wird nach `sti` zugestellt) → MSI-X-Zustellung auf echter Hardware bewiesen. Datenpfad pollt weiter; der IRQ ist die Grundlage für einen blockierenden Empfang |
| **IOMMU (VT-d)** (`iommu.rs`) | DMA-Schutz Stufe 1: DMAR gelesen, Einheit(en) an, nutzbarer RAM 1:1 (2-MiB-Seiten), sonst nichts. **In QEMU (`-device intel-iommu`) geprüft**: „VT-d 1.0: 1/1 Einheit(en) an", virtio-blk/-net arbeiten durch die IOMMU, DHCP holt eine Adresse, keine vIOMMU-Fehler. Eigene Domäne je Gerät und Prüfung auf der echten 2-Einheiten-Hardware offen (`docs/integration/iommu.md`) |
| **USB-Netz (RNDIS)** (`gen_xhci/rndis.rs`) | `NetDev` über USB-Bulk mit bewaffneten Bulk-IN-Puffern, für USB-C-Tethering → SSH. TX ok; RX in QEMU unbestätigt (QEMU-`usb-net` ist Full-Speed, bulk 64 → dci 5 schließt nie ab), am echten Dongle zu prüfen (`docs/integration/usb-net.md`) |
| **SD-Leser (SDHCI)** (`gen_sdhci.rs`) | Controller-Reset/Takt/Strom, Karten-Init (CMD0/8/ACMD41/2/3/9/7/16), CSD-Kapazität (v1/v2), PIO-Lesen, `BlockDev`. **Bring-up auf echtem O2 Micro (VFIO) belegt**; Lesen mit eingelegter Karte offen |
| **Bluetooth (Intel Read Version)** (`gen_xhci/bt.rs`) | HCI 0xFC05, TLV-Auswertung (CNVi/CNVr, Image-Typ), Firmware-Dateiname `ibt-<cnvi>-<cnvr>.sfi`. **Auf echtem BE200 belegt** (operational). Firmware-Download am Live-Server vorbereitet (`docs/integration/bt-intel.md`) |

Offen in Welle A: NVMe am echten Gerät (die Crucial-Platten tragen Linux → nur beim Direktstart prüfbar),
E610-Verbindung mit Gegenstelle und Durchsatz (braucht ein Kabel zur Gegenstelle), VT-d auf echter Hardware,
RNDIS-RX und SD-Lesen am echten Gerät.

**Live-Vollendung vorbereitet** (`docs/LIVE-VOLLENDUNG.md`): Handbuch für die nur am laufenden Ultar-Server
prüfbaren Teile – BT-Firmware-Download (Secure-Send-Offsets/Opcodes, Intel-Reset, DDC), WLAN W1–W4, Xe X1–X3,
plus S9-Direktstart, NVMe-Schreibtest, VT-d/SD auf echter HW. `scripts/build-pc-image.sh` legt die Intel-BT-Firmware
(`ibt-*.sfi/.ddc`) entpackt nach `/lib/firmware/intel` (Kern liest über `fs::read_all`).


## Installation auf dem Ultar-Kern vollständig geprüft (26.11.2026)

| Baustein | Stand |
|---|---|
| Grafische Installation A–Z | `installer-e2e.sh` grün in QEMU: alle 16 Schritte per Tastatur (inkl. Konto), Schreiben, **Neustart** aus dem letzten Schritt, Erststart der installierten Platte allein mit Konto. Läuft in `ci.sh` und als eigener GitHub-Job `installer` |
| Neustart nach der Installation | „Neu starten“ ruft `init.power` (Reboot); ultar-init beendet alle Dienste geordnet und startet die Maschine neu |
| Erststart übernimmt alle Antworten (28.11.2026) | `ultar-firstboot` legt neben dem Konto Sprache, Tastatur, Zeitzone, Barrierefreiheit und Gerätenamen im Speicher des Einstellungsdienstes an (geprüft gegen das Schema, nach Geltungsbereich in `system.toml`/`user-default.toml`, Vorhandenes bleibt) und die gewählten Komponenten als `apps.install_selection` – der Store zeigt auf „Entdecken“, wie viele davon fehlen, und markiert ihre Karten („Bei der Installation gewählt“); Ausgabe/Codecs zusätzlich in `/data/ultar/install/selection.toml`; settingsd startet im Installationsabbild nach dem Erststart; `installer-e2e` prüft es |
| Kern: Unix-Sockel | Verbindungen in der Warteschlange eines verschwundenen Lauschers lesen Dateiende (vorher ewiges Warten); `SO_RCVTIMEO` gilt für read/recvfrom/recvmsg – beides hielt ultar-init beim Herunterfahren fest |
| Kern: AVX | XSAVE: CR4.OSXSAVE, XCR0 = x87/SSE/AVX; YMM-Register bleiben über Planerwechsel, Signale und fork erhalten (`avxtest`). Programme (glibc, Graviola-TLS, llama.cpp) sehen AVX jetzt als nutzbar |

Datenverschlüsselung (2026-11-28): Installer schreibt LUKS2 (ultar-luks, nur RustCrypto), der Kern entschlüsselt
`ultar-data` (aes-xts-plain64), grafischer Entsperr-Bildschirm beim Start (`ultar-unlock`, Konsole als Rückfall);
geprüft gegen cryptsetup/dm-crypt. Datenpartition füllt bei „ganze Platte“ die Platte, das ext4 wächst mit
(`ultar-ext4grow`, auch verschlüsselt). Offen bleiben: TPM,
WLAN-Treiber, Anzeige über die Intel-Grafik (Rendern/PRIME), Bluetooth/Ton/Touchpad auf echter Hardware, Start direkt
auf dem Beelink. Details: `docs/integration/installer-boot.md`, `docs/integration/kernel-xsave-unix-sockets.md`.

## Umsetzung Welle 4 (23.11.2026) – gebaut, eingebunden, unabhängig geprüft

| Baustein | Stand |
|---|---|
| Kern: Geräte und Treiber | Geräte- und Treiberliste im Linux-Format (`/sys/bus/...`), Binden/Lösen mit echtem Abbau wo möglich, Ereignisse über netlink, Prüf- und Zustandswerte; `ultar-devd` liest sie statt des Kern-Protokolls (D1 erledigt) |
| Kern: Prozesse und Wecken | vfork gibt den Elternprozess beim exec frei, `shutdown` weckt blockierte Leser, Signale erreichen jeden Faden, ganze Zeilen je Prozessorkern, keine Speicherreste nach einer Prozess-Lawine |
| Startlader A/B | eigener UEFI-Lader mit Slot-Tabelle und Startzähler, zwei Kern- und zwei System-Slots, Rückfall nach misslungenen Versuchsstarts, Stromausfall geprüft; Updater schreibt über Roh-Partitionen (K4 erledigt) |
| ext4 | Extent-Bäume, ehrliche Fehler bei fsync/close statt stiller Datenverlust |
| ultar-init | startet Dienste nach Abhängigkeit und Bereitschaft, als eigene Benutzer, in Speichergruppen, mit nur den nötigen Rechten; Ausgaben in den Protokolldienst; geordnetes Herunterfahren; `uinit` (I1 erledigt) |
| Dienste | jede ausgelieferte Dienstdatei nennt Benutzer und Begründung je Recht; „alle Rechte“ wird abgelehnt; Eingabegeräte nur über die Gruppe `seat` |
| Updater | HTTPS rein in Rust (ohne C-Bibliothek), statischer ARM64-Bau wieder in der Prüfung |
| **Compositor** | eigener `ultar-compositor`: Fenster anderer Prozesse, Software-Zusammensetzung nach Schadensbereichen (1080p unter 4 ms), Eingabe-Lenkung, Dekoration aus den Gestaltungswerten (Arabisch gespiegelt), Sperre schließt sicher, sichere Taste, gekennzeichnete Systemdialoge; läuft auf dem Ultar-Kern mit zwei App-Prozessen |
| Baukasten | Runde 4–6: Dialog dimmt das vorhandene Bild, Mehrfachauswahl, Ablegen, Mittelklick, Tabellen für Bildschirmleser, Kennwortfeld abgesichert, Knoten-Kennungen mit Generation, A/B-Messung in der Prüfung |
| Alte Oberfläche | Ursache der gelegentlichen GitHub-Fehler gefunden: Testrechner wurden als Desktop erkannt |
| Kern: Grafik und Fenster | memfd mit Siegeln, geteilter Speicher zwischen Prozessen, Übergabe von Deskriptoren, Bildspeicher mit vblank-Raster, Eingabegeräte für USB-Tastatur, -Maus und -Tablet mit Einstecken/Abziehen, Besitzer und Rechte auf Gerätedateien; der Compositor nutzt auf dem Ultar-Kern geteilten Speicher |
| **Echte Hardware** | Beelink (KVM, 16 Kerne): Stufe 1 mit allen 10 Prüfungen bestanden, inkl. Geräte-Schnittstellen, Prozess-Start, Fenster-Kern und Speichergrenzen |

Prüfrunden: Compositor 2 Runden (zuerst 3 kritisch / 8 schwer, dann 2 kritisch / 7 schwer – alles behoben), Baukasten Runden 4–6.
Rund **2 450 automatische Tests** ohne Fehler; Kern-Prüfung mit 20 Prüfungen inkl. A/B-Start, ext4 nach hartem Stopp, Init, Speichergrenzen.

## Umsetzung Welle 3 (23.11.2026) – gebaut, eingebunden, zweimal unabhängig geprüft

| Baustein | Stand |
|---|---|
| Kern: Speicher | ein Speicher-Vorrat statt fester Halde; jede Anforderung kann fehlschlagen statt Panik; Gruppen system/session/apps/cells mit Grenzen (cgroup2-artig unter `/sys/fs/cgroup`), dynamischer Schutz aus der Speichergröße; `grenztest` 130 Prüfungen |
| Kern: Kennungen und Rechte | Benutzer-/Gruppenkennungen, echte Dateirechte und Besitzer, Unix-Sockel als Datei mit Rechteprüfung, `SO_PEERCRED`; `rechtetest` 102 Prüfungen |
| Dienste | `ultar-service` (Signale, Grenzen, Bereitschaft, Geheimnisse nur aus Datei/Deskriptor), `ultar-sys` (einziger `unsafe`-Teil), `ultar-busd`, `ultar-logd` + `ulog` (begrenzt, faire Aufnahme, Hash-Kette); Bus, Einstellungen und devd laufen darauf |
| Geräte-Identität | Ed25519-Geräteschlüssel, Kennung aus dem Schlüssel, Anmeldung und Anrufe per Schlüsselnachweis, Wiederherstellung mit Wartezeit und Warnung auf allen Geräten, neue Geräte erst nach Freigabe |
| System-Updates | `ultar-update` / `uupdate`: signiertes Manifest mit Schlüsselwechsel, Schutz vor älterer Fassung, A/B mit Rückfall, Offline-Paket; Stromausfall an 29 Stellen geprüft |
| Schriften und Baukasten | Noto Naskh/Kufi/Sans Arabic/Mono (OFL) mit echten Stärken 400/500/600/700, 27 Bausteine (u. a. Tabelle, Baum, Seitenleiste, Kopfleiste, Hinweis, Kennwortfeld, Ziehen), vollständiger Zugänglichkeits-Baum, 641 Referenzbilder; Dialog öffnen 3–5 ms (Budget 8 ms) |
| Entwurf | Runde 3: 26 Tafeln aus denselben Gestaltungswerten wie der Baukasten; Runde 2: 23 Tafeln aus den Gestaltungswerten, Klickliste mit 65 Festlegungen (DESKTOP-SPEC §13) |
| **Echte Hardware** | **Beelink GTi15 Ultra (KVM, echter Prozessor, 16 Kerne, echte SMBIOS): Stufe 1 bestanden** – alle Geräte mit Treiber, Ring-3-Programme, Bus, Ausbruchs- und Speichergrenzen-Prüfung ohne Fehler |

Zusammen **rund 2 250 automatische Tests** ohne Fehler (Arbeitsbereich 1 872, Server 290, Ende-zu-Ende 21, Oberfläche 74), Kern-Prüfung mit 2 und 4 Kernen grün.
**Nicht produktionsreif:** offen sind die Kern-Schnittstellen für devd (D1), die Nachprüfung der Baukasten-Korrekturen, der Startlader für A/B-Updates (K4) und `ultar-init` auf dem neuen Dienst-Rahmen. Noch keine sichtbare neue Oberfläche.
Noch kein Start ohne KVM direkt auf der Hardware (Stufe 3).

## Umsetzung Welle 2 (23.11.2026) – gebaut, eingebunden, unabhängig geprüft

| Baustein | Stand |
|---|---|
| Kern: Abschottung Stufe 1 | Mount- und PID-Namensräume, Rechte je Prozess; Ausbruchs-Tests in der Kern-Prüfung (QEMU) grün |
| `platform/ultar-ui` | Baukasten: Gestaltungswerte aus `design/tokens.toml`, Zeichnen, arabischer Textsatz mit Leserichtung, 8 Bausteine mit allen Zuständen, Bildvergleich mit 297 Referenzbildern; ganzer Bildschirm 1920×1080 in 3,6–5,7 ms |
| `system/core/ultar-settingsd` | Einstellungsdienst mit 111 Einstellungen in 10 Bereichen, „Behalten oder zurück“, Profile je Bildschirm-Kombination; Abfrage 34 µs |
| `system/core/ultar-devd` | läuft als Dienst (Start 2–3 ms), Bus-Dienst `devices`, `udevctl`; Treiberwechsel nur, wo der Kern es kann |
| Server + Gerät | Store und Updates ohne Konto, Berechtigungen für Premium und gesperrte Kanäle, Schutz der offenen Endpunkte |
| Prüfung | drei unabhängige Prüfer: 1 kritischer und 19 schwere Befunde, davon 1 kritischer und 15 schwere behoben – Rest in `docs/review/OFFENE-BEFUNDE.md` |

Zusammen jetzt **rund 1 400 automatische Tests** ohne Fehler (Arbeitsbereich, Server, Ende-zu-Ende), Kern-Prüfung grün.
**Nicht produktionsreif:** siehe „Offen – sperrt den Einsatz“ in `docs/review/OFFENE-BEFUNDE.md`. Noch keine sichtbare
neue Oberfläche: Compositor, Desktop-Hülle und Apps sind nicht gebaut; nichts lief auf echter Hardware.

## Fertig und geprüft

| Was | Nachweis |
|---|---|
| Sieben-Bereiche-Struktur, gesamter Code des Vorgängers übernommen | Workspace baut; **842 Tests bestanden, 0 fehlgeschlagen** (2 bewusst übersprungen) |
| Einheitliche Formatierung des gesamten Codes | `cargo fmt --all --check` sauber |
| Neue Bausteine (i18n, profile, hal, hal-system, bus, driverpkg, devd, check) | zusammen rund 650 Tests; `clippy -D warnings` sauber; in diesen Bausteinen keine Regelverletzung |
| Ultar-Kern baut und startet | QEMU (q35, UEFI): bereit nach **145 ms**, alle Geräte mit Treiber, Ring-3-Prüfprogramm „ALLES OK“ |
| Systembus läuft **auf dem Ultar-Kern** | statisches Prüfprogramm im Kern in QEMU: „BUS SELFCHECK OK“ |
| Systembus Leistung (Linux, 2 geteilte Kerne) | Anfrage hin und zurück 33–40 µs (Median), 165–180 Tsd. Ereignisse/s, im Leerlauf kein Aufwachen |
| Treiber-Depot | 6 signierte Pakete + signierter Index gebaut; Zuordnung für beide Testrechner geprüft |
| Regelprüfung mit Sperrklinke | 5 602 Altlasten des übernommenen Codes erfasst; die Zahl darf nur sinken |

## Beim Übernehmen gefundene und behobene Fehler

- Der Browser `ultar-web` ließ sich auf dem alten Stand nicht mehr bauen (fehlende Abhängigkeiten und Module).
- Drei Testfehler: Bildschirm-Maßstab (Test passte nicht mehr zum Entwurf mit 390 Punkten Breite) und
  Bildschirmübertragung im Verbund (Überlauf im Testmuster); dazu liefen Tests der Oberfläche, die gemeinsame Umgebungsvariablen ändern, gegeneinander (fiel erst auf GitHub auf) – jetzt nacheinander.
- Die alte Geräteart-Erkennung (`ultar-edition`) ist durch `ultar-profile` + `ultar-hal-system` ersetzt.

## Noch nicht gemacht (ehrlich, Stand 28.11.2026)

| Phase | Stand |
|---|---|
| P2 | TEILWEISE: `ultar-i18n` mit Katalog (ar/en/de) ist da; `ultar-sprache` existiert noch und wird in wenigen CLI-/Legacy-Stellen aufgerufen; nicht alle alten Oberflächentexte umgestellt |
| P2b | OFFEN: Kern enthält weiter `install.rs`, `startseite.rs`, `shell.rs` (`kernel/ultar-kern/src/main.rs`); daneben existiert `apps/ultar-installer-ui`, der Kern-Installer läuft aber weiter |
| P3 | TEILWEISE: `ultar-hal` + Backends (gfx, linux, netlink, system) vorhanden; die alte Hülle `platform/ultar-shell` greift weiter direkt auf `/sys`/`/dev` zu |
| P4 | ERLEDIGT: `ultar-devd` läuft als Dienst (user 804), Bus-Dienst `devices`, Treiberwahl/Gesundheitsprüfung/Rückfall/Quarantäne (`system/core/ultar-devd/daemon.rs`) |
| P5/P6 | ERLEDIGT (Desktop): `ultar-compositor` (user 811), Hülle `ultar-desktop` (user 816, Leiste/Menüs/Schnelleinstellungen), Sperre `ultar-lock`, Systemdialoge `ultar-sysdialog`, Apps als eigene Prozesse über `ultar-launcher` |
| P7 | TEILWEISE: signiertes Treiber-Depot + Index vorhanden; ladbare Kern-Module (`.uko`) fehlen, Treiber weiter einkompiliert |
| P8–P11 | P8 Installation A–Z ERLEDIGT (QEMU); Verschlüsselung gesperrt; P9/P10 TEILWEISE; P11 ARM-Port OFFEN |
| Treiber | siehe „Treiber-Welle A“ oben; fehlen u. a. WLAN (kein Treiber im Kern), ACPI-AML/Thermik, TPM, Kamera, Touch (I2C-HID), Stufe 4 |
| Umbenennung | OFFEN: deutsche Bezeichner weiter vorhanden (`ultar-verbund`, `ultar-zellen`, …) |

## Nicht geprüft

- Echte Hardware: nur Beelink unter KVM (Stufe 1). ThinkPad und A30 nicht; kein Start direkt auf der Hardware.
- Die GitHub-Prüfung (`.github/workflows/ci.yml`) läuft auf GitHub: Format, Regeln, Tests, statischer ARM64-Bau der neuen Bausteine und Kern-Start in QEMU.
- ARM64: nur gebaut (statisch, in der GitHub-Prüfung), nicht ausgeführt; `ultar-llm` (llama.cpp) wurde nicht gebaut.
- Arabische Texte sind nicht von einem Muttersprachler gegengelesen.
- Zielwerte in `tools/budgets.toml` sind Vorgaben, keine Messwerte.

## Entscheidungen der Bausteine, die noch bestätigt werden sollten

Siehe `docs/integration/*.md`, jeweils letzter Abschnitt (u. a.: ohne gesetzte Sprache ist Arabisch Standard;
ein erzwungener Modus des Nutzers gilt auch auf Servern; Handgerät ab 178 mm Diagonale zählt als Tablet;
`ultar-busd` startet nur mit Rechte-Datei).
