# Unbenutzter Code, der bewusst stehen bleibt

KM Code Craft · Ultar OS v2 · Stand 25.11.2026

Der Bau meldete 49 Warnungen außerhalb des Kerns (`ultar-shell` 37, `ultar-audio` 7 und Doppelte im Testbau).
Zwei waren echte Kleinigkeiten und sind behoben (`mut` ohne Grund; Funktionszeiger direkt in eine Zahl
gewandelt). Der Rest ist **Code, den nichts aufruft**. Er wurde nicht gelöscht – es sind zum Teil ganze
Bildschirme und Hardware-Wege, die offenbar noch angeschlossen werden sollen –, sondern jede Stelle einzeln
markiert (`#[allow(dead_code)]` mit Grund). So bleibt der Bau still und jede Stelle ist auffindbar:

    grep -rn "see docs/OFFEN-WARNUNGEN.md" platform system

Wer eine Stelle anschließt oder endgültig streicht, nimmt die Markierung weg. Neue Warnungen fallen wieder auf.

## Alte Hülle (`platform/ultar-shell`)

| Was | Datei |
|---|---|
| Kontakte: Liste und Bearbeiten (`s_contacts`, `s_contact_edit`, `initial`) | `app_extra.rs` |
| Verbindungs-Karte, große Kachel, Raten-Anzeige (`conn_card`, `big_tile`, `Rates::sample`, `rate`, `DOCK`) | `app_layout.rs` |
| Zweite Reihe und kleine Kachel der Schnelleinstellungen (`shade_row2`, `small_tile`), Radio (`shade_radio`) | `app_more.rs`, `app_radio.rs` |
| Benachrichtigungen lesen (`read_notifications`), Feld `spotify`, Feld `note` | `app.rs`, `app_spotify.rs` |
| Alter (`age`), `t_pub`, `MONTHS`, `DAYS_SHORT`, Feld `hist_tab` | `app_modern.rs`, `app_gallery.rs`, `app_sys.rs` |
| Taste `Helper` der Bildschirmtastatur, Symbol `Star`, Prüfstufe `Info` | `kbd.rs`, `ui.rs`, `selftest.rs` |
| Datenschutz (`Cap::key`, `blocked_count`), Sensoren (`sensors_open`, Feld `present`), `Profile::forget`, `scan::FULL`, Feld `url` | `privacy.rs`, `sensors.rs`, `voice.rs`, `scan.rs`, `linkcheck.rs` |

Dazu sechs Stellen, die nur die laufende Hülle benutzt und der Testbau nicht (`help::export`, `sysd`): dort
`#[cfg_attr(test, allow(dead_code))]`.

## Ton (`system/services/ultar-audio`)

`Codec::enable_mic`, `StubCodec`, `Ctl::has`, `Ctl::set_frac`, `Mixer::each`, `CTL_READ`, `PCM_SW_PARAMS`,
`Dev::modem_path` – Wege für Mikrofon, Mischer und Modem, die noch niemand ruft.

## Kern (`kernel/ultar-kern`)

Der Kern meldete 68 Warnungen. Hier lässt er sich nicht bauen (keine Rust-Quellen für `build-std`), geprüft
wird diese Änderung deshalb **nur vom Kern-Lauf der CI**.

Behoben (ohne das Verhalten zu ändern):

- 17-mal Funktion direkt in eine Zahl gewandelt → erst in einen Zeiger (`arch.rs`, `apic.rs`, `proc.rs`, `trap.rs`),
- zwei unnötige `unsafe` um `__cpuid` (`arch.rs`, `ps2maus.rs`), ein unbenutzter Wiederexport (`net.rs`),
  ein Anfangswert, den niemand liest (`gen_xhci.rs`), die Variable `TRAMP` (`apic.rs`),
- Systemaufruf 204 stand zweimal da; der zweite Eintrag (in der Liste „tut nichts“) war nie erreichbar.

Markiert wie oben, nicht gelöscht: `arm_deadline`, `reboot_hw`, `absolut`, `verworfen`, `Canvas::cursor`,
`write32x`, `PciDev::w32x`, `Shared::refs`, `SIGTERM`, `BS_ALLOCATE_PAGES`, `MRAD`, `Wait::Pipe` und die Felder
`UsbDev::speed`, `Fat::part`, `Inode::mtime`, `PciDev::driver`. Die Nvidia-Schnittstellen (`drivers/nvidia`)
ruft noch niemand; dort gilt die Erlaubnis für das ganze Modul.

### Eine Warnung bleibt mit Absicht: `getpeername` (Systemaufruf 52) tut nichts

`syscall.rs` führt 52 in der Liste „tut nichts, meldet Erfolg“ und weiter unten noch einmal mit der echten
Antwort (`51 | 52 => sys_sockname(…)`). Der erste Eintrag gewinnt: Ein Programm, das fragt, mit wem es
verbunden ist, bekommt Erfolg und einen unbeschriebenen Puffer. Das ist vermutlich ein Fehler. Es zu
berichtigen ändert das Verhalten des Kerns für alle Programme und gehört dem, der den Kern prüfen kann – darum
steht die Warnung noch da.

Die C-Prüfprogramme (`kerntest.c`, `ereignis.c`, `lauscher.c`, `nettest.c`) melden ignorierte Rückgabewerte
und zwei mögliche Kürzungen bei `snprintf`; nicht angefasst.
