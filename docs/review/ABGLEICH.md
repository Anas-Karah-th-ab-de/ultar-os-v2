# Abgleich Dokument ↔ Code

Stand 28.11.2026 · Basis: `main` 44a4ae62. Diese Datei prüft jeden offenen/markierten Punkt der
Projekt-Dokumente gegen den Code und die Git-Geschichte (`git log --since=2026-11-20`). Urteile:

- **ERLEDIGT** = im Code vorhanden und durch Test/Log belegt (Beleg nennt Datei/Test).
- **TEILWEISE** = ein Teil steht, der Rest fehlt (genannt).
- **OFFEN** = im Code bestätigt nicht vorhanden.
- **VERALTET** = Punkt betrifft etwas, das es so nicht mehr gibt (Begründung).

Zwei Pakete werden parallel gebaut und sind hier **in Arbeit (28.11.2026)** notiert, nicht abschließend
bewertet: SSH in den Ultar-Kern (ultar-sshd) und USB-C-Netz (RNDIS-Empfang + CDC-ECM).

Nicht neu geprüft: `docs/OFFEN-WARNUNGEN.md` (Stand 25.11.2026) ist eine Momentaufnahme absichtlich
stehengelassenen, markierten Totcodes – eine Neuprüfung braucht den Kern-/CI-Lauf; seit dem 25.11.2026 gab es
keine Änderungen an den dortigen Diensten (Kommunikation/Helfer), `docs/OFFEN-VERBUND-HELFER.md` bleibt inhaltlich
gültig. `docs/LIVE-VOLLENDUNG.md` und `docs/OFFEN-PLAN.md` sind bereits auf 28.11.2026.

---

## Kern

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| TCP im Kern | (ältere Plan-Notizen) „der Kern hat kein TCP" | AF_INET mit UDP **und TCP, Client und Server**: Drei-Wege-Handshake, Folgenummern, ACK, Wiederholung, FIN/RST, Annahme-Warteschlange | `kernel/ultar-kern/src/socket.rs` (968 Z., `tcp_in`/`tcp_out`, Z. 1–2) | ERLEDIGT (seit 24.11.2026) |
| Unix-Sockel | Welle-4-Reste: „ein verschwundener Lauscher = ewiges Warten" | Leseende bei fehlendem Lauscher, `SO_RCVTIMEO` wirkt, `SO_PEERCRED` vorhanden, Listener-Abbau ordentlich | `kernel/ultar-kern/src/uds.rs` (Z. 183 ff.), `cred.rs`; Commits 1191ea37/Dok. | ERLEDIGT |
| `SCM_CREDENTIALS` | OFFENE-BEFUNDE „Offen – leichter": fehlt | nur `SO_PEERCRED` (getsockopt); keine `SCM_CREDENTIALS`-Hilfsdaten in `sendmsg`/`recvmsg` | `grep SCM_CRED` im Kern: 0 Treffer | OFFEN |
| AVX/XSAVE | STATUS „nicht fertig" (älter) | `CR4.OSXSAVE`, XCR0 = x87/SSE/AVX, YMM bleibt über Planer/Signal/fork erhalten | `avxtest`; STATUS 26.11.2026 | ERLEDIGT |
| `mprotect`, `CLONE_THREAD`, `cgroup.kill`, noexec | diverse P0-Befunde | alle umgesetzt: `mprotect` setzt Rechte, `CLONE_THREAD` ohne `CLONE_VM` = EINVAL, `cgroup.kill` je Rolle, `MS_NOEXEC` | Commits 172bd72b, 91d51bfd, c98bc411, 8ed113c2; `schutztest`, `grenztest` | ERLEDIGT |
| fsync → NVMe/AHCI-Flush | OFFENE-BEFUNDE „Offen – leichter" | `BlockDev::flush` im NVMe-Treiber vorhanden | `kernel/ultar-kern/src/drivers/nvme.rs:212` | TEILWEISE (NVMe ja; AHCI nicht nachgewiesen) |
| TLB-Shootdown für `mprotect`/`munmap` | OFFEN-PLAN §2 | Rechte nur am eigenen Kern, kein Abgleich der anderen Kerne | kein IPC/Shootdown-Pfad | OFFEN |
| IOMMU/VT-d | STATUS Welle A | DMA-Schutz Stufe 1: DMAR gelesen, RAM 1:1, in QEMU geprüft; eigene Domäne je Gerät + echte 2 Einheiten offen | `kernel/ultar-kern/src/iommu.rs`; `docs/integration/iommu.md` | TEILWEISE |
| Installer/Startseite/Bedien-Konsole aus dem Kern (P2b) | PLAN P2b; STATUS „Noch nicht gemacht" | Kern enthält weiter `install.rs`, `startseite.rs`, `shell.rs` und ruft sie | `kernel/ultar-kern/src/main.rs:34,39,58` (`mod install/startseite/shell`) | OFFEN (TEILWEISE: `apps/ultar-installer-ui` existiert, Kern-Installer läuft aber weiter) |

## Treiber

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| MSI/MSI-X | Stufe 1 🔧 | Rahmen `msi.rs`, Vektoren 0x90–0xEF; E610 nutzt RxQ0-MSI-X, am echten E610 belegt (`netz` → „10G MSI-X-Unterbrechungen: 1") | `kernel/ultar-kern/src/msi.rs`; `gen_ixgbe.rs` | ERLEDIGT |
| E610 10 GbE | „MSI-X offen" (DRIVER-TARGETS) | MSI-X RxQ0 belegt; Verbindung mit Gegenstelle + Durchsatz offen (braucht Kabel) | Commits bda3bc7a, 7c9a95a8 | TEILWEISE |
| USB-Netz RNDIS | „RX unbestätigt" | `NetDev` über USB-Bulk, bewaffnete Bulk-IN-Puffer; TX ok, RX in QEMU dci-5-hängt (Full-Speed) | `drivers/kernel/…/gen_xhci/rndis.rs` | **in Arbeit (28.11.2026)** – RX/CDC-ECM-NCM |
| SDHCI | 🔧 | Controller-Reset/Init/CSD/BlockDev; Bring-up echtem O2 Micro belegt, Lesen mit Karte offen | `gen_sdhci.rs` | TEILWEISE |
| Bluetooth Intel | 🔧 | HCI 0xFC05 + TLV, Firmware-Name; Erkennung echtem BE200 belegt, Firmware-Download offen (nur Live) | `gen_xhci/bt.rs` | TEILWEISE |
| NVMe | „am echten Gerät prüfen" | Norm-Treiber + `flush`; echter Schreibtest erst beim Direktstart | `nvme.rs` | TEILWEISE |
| WLAN (W1–W4) | ⬜ | **kein** WLAN-Treiber im Kern; kein 802.11-Stapel | `grep -i wifi/wlan` → nur Nutzertexte | OFFEN |
| HD Audio | 🔧 `gen-hda` erweitern | `gen-hda` vorhanden, Codec-Wege unvollständig | `gen_hda.rs` | TEILWEISE |
| TPM 2.0, SMBus/Sensoren, ACPI-AML, Energie (HWP/C/RAPL) | ⬜/🔧 | TPM nicht begonnen; `gen_smbus.rs` Grundlage, AML-Interpreter ⬜ | `kernel/ultar-kern/src/drivers/` | OFFEN (SMBus TEILWEISE) |
| Intel-Grafik/GT, NPU | eigenes Paket (nicht Teil dieses Abgleichs) | wie in `docs/integration/intel-gt.md`/`intel-npu.md` | – | (anderes Team) |

## Sicherheit

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| S-4 … S-15, R2-2 … R2-5, `mprotect`, `CLONE_THREAD` | „behoben 28.11.2026" | im Code + Tests | OFFENE-BEFUNDE §„Sicherheit P0"; Commits 172bd72b … 91d51bfd | ERLEDIGT |
| Anruf-Bildschirm an „klingelt" von `calld`/`ril` | OFFEN-PLAN §2 | Bildschirm erscheint ohne Bindung an Klingel-Ereignis | – | OFFEN |
| BT-Tastaturen über `btd` | OFFEN-PLAN §2 | virtuelle Geräte werden abgewiesen, feste Erzeuger-Kennung fehlt | – | OFFEN |
| `ui.client-header` (S-15) | – | nur `app.system` | Profil-/Starter-Tests | ERLEDIGT |
| devd-Index ohne Seriennummer, S-14, Bus-Grenzen je Kennung, Server-Härtung | „leicht, offen" | nicht umgesetzt | – | OFFEN |

## Dienste

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| `ultar-devd` Dienst + Kern/Bus (P4) | STATUS „nur Logik + Tests" | laufender Daemon: aufzählen → auflösen → verwalten → Bus-Dienst `devices`, Gesundheitsprüfung/Rückfall/Quarantäne, eigener Benutzer 804 | `system/core/ultar-devd/daemon.rs`; `rootfs/etc/ultar/services/devd.toml` | ERLEDIGT |
| Bus/Init/Logd | Welle 3/4 | `ultar-busd`, `ultar-init`, `ultar-logd` laufen | STATUS; `rootfs/etc/ultar/services/bus.toml` | ERLEDIGT |
| R1 Root-Dienste | OFFENE-BEFUNDE „17 Dienste" (OFFEN-PLAN „19") | **26** Dienstdateien ohne `user`-Zeile (rootfs + editions), Liste unten | Zählung dieser Sitzung | TEILWEISE (Zahl korrigieren) |

**R1 — die 26 Dienstdateien ohne `user`-Zeile** (`rootfs/etc/ultar/services/*.toml` + `editions/*/etc/ultar/services/*.toml`):

- Nur-Test: `selftest`, `tontest`, `webtest`, `wifitest`
- By design Verwalter: `shell`, `entwickler`
- Alte Oberfläche (hängt an `ui`): `ui` (`/usr/bin/ultar-shell`), `verbund`
- Handy-Hersteller-HAL (Android-Binaries erwarten root): `gnss`, `fingerabdruck`, `modem`, `ril`
- Einmal-/Aufräumläufe: `drivers`, `compat-bind`, `firstboot`, `netzregeln`
- Echt aufzuteilen: `net`, `seat`, `llm-compat`
- Editionsdateien: `admin` (desktop, server), `sshd` (desktop, server, OpenSSH-compat), `claude-prep` (desktop), `coldplug`, `grow-data` (pc)

## Oberfläche

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| Compositor (P5/P6) | STATUS „Compositor fehlt" | nativer `ultar-compositor`, user 811, phone+desktop; Fenster fremder Prozesse, Schadensflächen, Eingabe, Dekoration, Sperre | `platform/ultar-compositor/`; `compositor.toml` | ERLEDIGT |
| Desktop-Hülle (S1) | OFFENE-BEFUNDE „nicht gebaut" | `ultar-desktop` (user 816, `ui.shell`): Leiste (`shell/bar.rs`), Menüs (`shell/menus.rs`), Schnelleinstellungen (`model/quick.rs`), Sperre (`lock/`), Systemdialoge (`dialog/`) | `platform/ultar-desktop/`; `desktop.toml`, `lock.toml` (814), `sysdialog.toml` (815) | ERLEDIGT |
| Sperrbildschirm + Systemdialoge | „wirken erst mit Sperrbildschirm" | eigene Dienste `ultar-lock`, `ultar-sysdialog` | `lock.toml`, `sysdialog.toml` | ERLEDIGT |
| G1 Touch-Treiber | OFFENE-BEFUNDE „kein Touch" | USB-Tastatur/-Maus/-Tablet gehen; **kein** I2C-HID/Touch-Treiber für Laptop-Panel | `ps2kbd.rs`, `gen_xhci` HID | TEILWEISE |
| Eine alte Hülle | PLAN „Gott-Objekt" | alte `ultar-shell` existiert weiter (läuft als `ui`-Dienst, root); daneben neue Desktop-/Mobile-Hülle | `platform/ultar-shell/`, `ui.toml` | TEILWEISE (Altlast noch da) |
| P3 HAL | STATUS „kein echtes HAL-Backend, Oberfläche 393 Stellen direkt" | `ultar-hal` + Backends (gfx, linux, netlink, system) existieren; alte Hülle greift aber weiterhin direkt auf `/sys`/`/dev` zu | `system/hal/ultar-hal*`; `grep /sys\|/dev` in `platform/ultar-shell` | TEILWEISE |

## Apps

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| App-Starter (A1) | OFFENE-BEFUNDE „kein Starter" | `ultar-launcher` (user 805): jede App eigener Benutzer (20000–49999) in eigener Zelle (mount+pid, read-only System-Sicht, privates /tmp, Daten 0700, Gruppe `apps/<app>`), Rechte nur aus signiertem Manifest | `system/core/ultar-launcher/src/{spawn,cell,manifest,rightsfile}.rs`; `launcher.toml` | ERLEDIGT |
| Apps als eigene Prozesse | P5 „fehlt" | System-Apps als eigene Crates + Prozesse | `apps/` (settings, files, terminal, web, office, phone, store …) | ERLEDIGT |
| i18n (P2) | STATUS „1400 Texte nicht umgestellt, `ultar-sprache` existiert noch" | `ultar-i18n` (catalog, ara/en/de, RTL) vorhanden; `ultar-sprache` liegt noch und wird in wenigen CLI/Legacy-Stellen aufgerufen | `system/core/ultar-i18n/`; `system/core/ultar-sprache/` | TEILWEISE |
| Offenes je App melden (B4) | OFFEN-VERBUND: Browser/Editor/Büro fehlen | Mesh merkt je App; Browser ohne Bus, Büro fehlt | `apps/ultar-web`, `apps/ultar-office` | TEILWEISE |
| „Im Ordner zeigen" (`ultar-files --select`) | Welle-8-Rest | Dateien-App kennt `--select` nicht | – | OFFEN |

## Installer / Update

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| Installation A–Z | „nur Kern-Installer" | `installer-e2e.sh` grün: 16 Schritte, Neustart, Erststart | `tools/`/`ci.sh`, GitHub-Job `installer` | ERLEDIGT |
| Erststart übernimmt alle Antworten | – | `ultar-firstboot`: Konto/Sprache/Tastatur/Zeitzone/Barrierefreiheit/Gerätename/Komponenten | Commits 4fad4f74, 55a0119c | ERLEDIGT |
| Verschlüsselung (LUKS2/XTS) | „gesperrt" | nicht im Installer; braucht Entscheidung | – | OFFEN (Inhaber-Entscheidung) |
| Updater | A/B, signiertes Manifest | `ultar-update`/`uupdate` vorhanden | STATUS „Fertig und geprüft" | ERLEDIGT (Reste: Daemon ohne Dienst-Rahmen, ARM64-TLS) |
| VMware-Installationsmaschine | – | `UltarOS-v2-installer.zip` + Workflow „Installer image" | Commit 49af9c92 | ERLEDIGT |

## Verbund / Helfer

| Punkt | Stand 25.11.2026 | Code | Urteil |
|---|---|---|---|
| A1–A4, B1, B2, C7, C9, C10, C11, C12, C13, B4-Teil | erledigt (durchgestrichen) | wie dokumentiert | ERLEDIGT |
| A5 Kachel „Auf anderem Gerät weitermachen" | gebaut, weicht von Tafel ab | Dialog/Bestätigung fehlen | TEILWEISE |
| A6 Handy „Weitergeben"-Blatt | fehlt | – | OFFEN |
| B3 Konto-Ticket für Verbund | nicht begonnen | – | OFFEN |
| B5 Zwischenablage über Geräte | nicht begonnen | Compositor kennt keine Verbund-Rolle | OFFEN |
| B6 Dateien im Abbild gehen nicht | nicht begonnen | – | OFFEN |
| B7 Rechenaufgaben in Zelle | nicht begonnen | – | OFFEN |
| C1 Fernansicht, C2 Ton/Anrufe, C4 Identitätsbindung, C5 Auto-Vertrauen, C6 Tafeln, C8 Sprache | offen | – | OFFEN |

## Handy

| Punkt | Stand | Code | Urteil |
|---|---|---|---|
| A30-Abbild (Linux-Kern) | läuft (DECON, Images) | `kernel/samsung-a30`, `ultar-mobile`, phone-Edition | ERLEDIGT (Übergang) |
| ARM64-Port des Ultar-Kerns (P11) | fehlt | Kern nur x86_64/UEFI | OFFEN |
| Arabisch gegengelesen | offen | – | OFFEN |

## Server / Store

| Punkt | Dokument sagt | Code sagt | Beleg | Urteil |
|---|---|---|---|---|
| SSH-Server | „schon vorhanden, läuft auf Ultar-Kern" | `ultar-sshd`-Crate (russh, nur Schlüssel, SFTP/scp) vorhanden; ausgeliefertes `sshd.toml` nutzt aber OpenSSH im Compat-Bereich, natives `ultar-sshd` nicht als Dienstdatei verdrahtet | `system/services/ultar-sshd/src/{main,sftp,steuern}.rs`; `editions/*/sshd.toml` | **in Arbeit (28.11.2026)** |
| Store | Install-Auswahl markiert, Suche im Überblick | `ultar-store`; `apps.install_selection` | Commits 06cc206a, e886c6e9 | ERLEDIGT |
| Konto-Server-Ticket (B3) | offen | – | – | OFFEN |

---

## Echte offene Punkte nach Priorität

**P0 (sperrt Einsatz/Sicherheit):**
1. RNDIS-Empfang + „ssh über USB-C" Ende-zu-Ende – `drivers/kernel/…/gen_xhci/rndis.rs`, `system/services/ultar-sshd` *(in Arbeit)*.
2. Start direkt auf dem Beelink (S9) – Freigabe; schaltet NVMe/VT-d/BT/WLAN/Xe auf echter HW frei.
3. Verschlüsselung im Installer (LUKS2/XTS oder TPM) – Inhaber-Entscheidung.

**P1 (nötig für den Betrieb):**
4. R1 Root-Dienste aufteilen (`net`, `netzregeln`, `drivers`, `seat`, `ril`) – `rootfs/etc/ultar/services/`.
5. TLB-Shootdown für `mprotect`/`munmap` – `kernel/ultar-kern`.
6. ACPI-AML-Interpreter, Energie (HWP/C/RAPL), TPM 2.0 + Hardware-Wachhund.
7. WLAN-Stack (W1–W4) + BT-Firmware-Download (Live, `docs/LIVE-VOLLENDUNG.md`).

**P2 (Ausbau):**
8. i18n-Umstellung vollenden (`ultar-sprache` entfernen), P2b den Kern entschlacken (Installer/Startseite/Konsole aus dem Kern).
9. G1 Touch-Treiber (I2C-HID), Intel-Anzeige (X1–X3), HD-Audio-Codecs, SCM_CREDENTIALS.
10. Verbund/Helfer-Reste: A5/A6, B3–B7, C1/C2/C4/C5/C6/C8.

**P3 (Erkundung/später):**
11. ARM64-Port (P11), Stufe-4-Treiber (NVIDIA, DisplayLink, Fingerabdruck), NPU-Inferenz.
12. Arabische Texte durch Muttersprachler gegengelesen; deutsche Bezeichner vereinheitlichen.