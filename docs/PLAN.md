# Ultar OS – Umbauplan: Struktur, Trennung, Treiber im Installationsmedium

KM Code Craft · Stand 22.11.2026 · Grundlage: Repo `ultar-os` (main, 14f037c) · Maßstab: Ubuntu 26.04 LTS Desktop

> Ziel: Ultar OS wird ein sauber geschichtetes System nach Clean Code, das auf jedem Gerät mindestens so gut
> funktioniert wie Ubuntu 26.04 Desktop – mit klarer Trennung *gemeinsam / Handy / Desktop*, Treibern im
> Installationsmedium und einer dynamischen Anpassung von Treibern, System und Einstellungen an das Gerät.
> Der Desktop-Modus für Handy und Tablet bleibt erhalten.

---

## 1. Befund – wo das System heute steht

Gemessen am Code (nicht geschätzt):

| Problem | Befund im Repo | Folge |
|---|---|---|
| **Gott-Objekt Oberfläche** | `ultar-shell`: 40 198 Zeilen in 71 Dateien. `app.rs` 4 303 Zeilen, `struct App` ~107 Zeilen Felder, `enum Screen` ~86 Varianten, 397 `Screen::`-Stellen allein in `app.rs`. Alle Apps sind Untermodule von `app` und greifen direkt auf den Zustand zu. | Jede Änderung kann alles brechen; keine App ist einzeln testbar, austauschbar oder als Prozess abschottbar. Ein Absturz einer App = Absturz der ganzen Oberfläche. |
| **Gerätecode in der Oberfläche** | A30-Spezifisches (DECON, ABOX/COD3035/SMA1301, fimc-is, `/dev/umts_ipc0`, rfkill-Ersatz) steckt in 12 Shell-Dateien; 85 rohe `/sys`- bzw. `/dev`-Pfade in der Shell. | Jedes neue Gerät verändert die Oberfläche. Kein Hardware-Abstraktions-Layer (HAL). |
| **Zwei Desktops** | `app_desktop.rs` (in der Shell, Software-Fenster) **und** `crates/ultar-wm` (12 791 Zeilen, Smithay, eigenes Workspace, läuft im Ubuntu-Bereich mit Mesa). | Doppelte Arbeit, zwei Verhalten, zwei Fehlerquellen; `ultar-wm` widerspricht dem Ziel „ohne Linux/Ubuntu“. |
| **Zwei Treiberwelten ohne gemeinsames Modell** | Linux-Weg: `hw.rs` doppelt (`ultar-init` und `ultar-shell`), `coldplug` als Shell-Einzeiler, Module per **apt online** aus dem Ubuntu-Bereich. Ultar-Kern: Treiber **fest einkompiliert** (`drivers/mod.rs`, Liste `ALL`). | Kein Treiberpaket, kein Treiber-Depot, keine Offline-Treiber im Installationsmedium, kein Wechsel/Rückfall eines Treibers zur Laufzeit. |
| **Ausgabe statt Fähigkeit** | `ultar_edition::current()` an 4 Stellen der Shell entscheidet Desktop vs. Handy; daneben `self.desk.an`. | Tablet mit Tastatur, Handy am Monitor, Laptop mit Touch passen nicht in „phone/desktop/server“. |
| **Drei Übersetzungssysteme** | `ultar-shell/i18n.rs` (1 472 Z.), `ultar-sprache` (übersetzt *deutsche Ausgabetexte* per Tabelle), eigenes `t()` im Kern-Installer. | Fehlende/gemischte Sprachreste (bereits in Ultar Flash aufgefallen), Arabisch nicht überall gleich gut. |
| **Benennung ohne Bedeutung** | `sys`, `sysx`, `sysy`, `sysd`; `app_more`, `app_extra`, `app_modern`, `tools`. | Niemand findet etwas ohne Suche. |
| **Flacher Workspace** | 29 Crates nebeneinander ohne Schichten; die Shell hängt direkt an Telegram, Cloud, Office, LLM, Radio … | Ein 6–7 MB-Monolith, lange Bauzeiten, kein Teilbau je Ausgabe. |
| **Qualitätssicherung** | 196 Tests gesamt (70 in der Shell), 174 `unwrap()` in der Shell, CI nur für Release/WM/llama – **kein CI für Bauen + Testen des Haupt-Workspace**. | Rückschritte fallen erst am Gerät auf. |
| **Kernel enthält Nicht-Kernel-Teile** | Im Ultar-Kern liegen der grafische Installer (`install.rs`, 1 466 Z.), die Startseite/Oberfläche (`startseite.rs`, 485 Z.), die Bedien-Konsole (`shell.rs`, 644 Z.), Sprachtexte und Verbund-Logik. | Jeder Fehler im Installer oder in der Startseite läuft mit Kernel-Rechten; der Kern ist nicht klein und nicht prüfbar. |
| **Keine Grenze System ↔ App** | Systemfunktionen (PIN, Sicherung, Energie), System-Apps (Telefon, Kamera) und Fremddienste (Telegram, Spotify) liegen im selben Prozess `ultar-shell`. | Eine App hat dieselben Rechte wie die Systemoberfläche. |
| **Binärdateien im Git** | `bins/` 44 MB (A30-/Desktop-Programme), `.git` 50 MB. | Repo wächst mit jedem Bau; Herkunft der Binaries nicht nachvollziehbar. |

Was **gut** ist und erhalten bleibt: `ultar-init` (Profil „erst probieren, dann merken“), signiertes Paketformat `.uap`,
`ultar-edition`-Erkennung über SMBIOS/DMI, Treiber-Trait + Werkstatt im Ultar-Kern, Datenschutz-Zentrale, statischer Bau.

---

## 2. Maßstab Ubuntu 26.04 Desktop – „mindestens so gut“

Ultar muss nicht gleich aussehen, aber in jedem Bereich mindestens gleichwertig sein.

| Bereich | Ubuntu 26.04 | Ultar heute | Ultar Ziel (Abnahme) |
|---|---|---|---|
| Installer | geführter Installer: Sprache, Tastatur, Netz, Treiber/Drittsoftware, Platte, Verschlüsselung (TPM oder Passphrase), Konto, Zeitzone | Kern-Installer: Sprache → Platte → Bestätigen → Kopieren | gleiche Schritte + **Treiber-Schritt offline aus dem Medium**, Ar/En/De vollständig, RTL, Touch + Tastatur |
| Treiber | „Zusätzliche Treiber“, OEM/HWE-Treiber automatisch, `fwupd` für Firmware | apt online / einkompiliert | **Treiber-Zentrale**: Gerät → aktiver Treiber → Alternativen → Wechsel/Rückfall; Firmware-Updates offline (USB) und online |
| Verschlüsselung | TPM-gestützte Vollverschlüsselung im Installer, sonst LUKS | `ultar-secure` (LUKS2, dm-verity), nicht im Installer | im Installer wählbar: TPM (wo vorhanden) / Passphrase; Verity-Root immer |
| Anzeige | nur Wayland, gebrochene Skalierung, VRR, HDR, mehrere Monitore | Software-Rendering, ganzzahliger Maßstab, ein Bildschirm | ein Compositor, gebrochene Skalierung (DPI aus EDID/Panel), mehrere Monitore, Hotplug |
| Einstellungen | GNOME-Einstellungen: Panels je nach Hardware | feste Listen, Panels auch ohne Hardware | Panels **dynamisch aus den Fähigkeiten** (kein Mobilfunk-Panel ohne Modem usw.) |
| Barrierefreiheit | Bildschirmleser, Vergrößerung, hoher Kontrast | fehlt | Bildschirmleser (Ar/En), große Schrift, hoher Kontrast – Pflicht für „mindestens gleich gut“ |
| Speichersichere Basis | sudo-rs, uutils-coreutils | alles Rust | beibehalten; `unsafe` nur in HAL-Backends, dokumentiert |
| Updates | apt + Rückfall über Kernel-Auswahl | Abbild + `.alt`, kein automatischer Rückfall | A/B-Slots mit automatischem Rückfall, offline per USB und online |
| Rettung | Recovery-Modus | Notausstieg (Laut+Leise), Rettungseintrag | einheitlicher Rettungsmodus auf allen Geräten |

---

## 3. Zielarchitektur

### 3.0 Die sieben Bereiche – was ist Kernel, was System, was App

Jeder Teil des Systems gehört zu **genau einem** Bereich. Der Bereich legt fest: Rechte, erlaubte Schnittstellen,
Ordner im Repo und wie der Teil aktualisiert wird. Was nicht eindeutig zuzuordnen ist, wird geteilt.

| # | Bereich | Was gehört hinein | Rechte / Laufort | Spricht nur über | Ordner | Update |
|---|---|---|---|---|---|---|
| 1 | **Kernel** | Speicher, Prozesse, Zeitplanung, Systemaufrufe, Dateisysteme, Netz-Stapel, Brandmauer, Geräte-Busse (PCI/USB/ACPI), Treiber-Schnittstelle. **Sonst nichts.** | Ring 0 | Systemaufrufe, `/proc/ultar`, `/dev` | `kernel/` | Kern-Abbild (A/B) |
| 2 | **Treiber** | je Gerät ein Treiber + Firmware, als Paket `.utr` | Ring 0 (Kern-Modul) bzw. HAL-Backend im Nutzerraum | Kern-Treiber-ABI / HAL-Traits | `treiber/` | einzeln, signiert |
| 3 | **Systemkern** (Basis, ohne die nichts startet) | `stage0`, `init`, `devd`, `ultar-bus`, HAL, `settingsd`, `pkg`, `update`, `secure`, Profil, i18n, Protokoll | root, je Dienst eigene cgroup + minimale Rechte | `ultar-bus`, HAL-Traits | `system/kern/`, `system/hal/` | System-Abbild (A/B, Verity) |
| 4 | **Systemdienste** (zuschaltbar je Gerät/Ausgabe) | net, audio, power, radio/ril, bt, gnss, notify, vpn, cloud, call, verbund, zellen, llm, sshd, admin, compat | eigener Benutzer je Dienst, nur benötigte Fähigkeiten | `ultar-bus` | `system/dienste/` | System-Abbild; je Dienst abschaltbar |
| 5 | **Plattform** (Oberflächen-Unterbau) | Compositor, `ultar-ui`, Tastatur/IME, shell-mobile, shell-desktop, Sperrbildschirm, Schnelleinstellungen | Nutzer „ui“, kein root | `ultar-bus`, Wayland | `plattform/` | System-Abbild |
| 6 | **System-Apps** (mitgeliefert, von KM Code Craft) | Einstellungen, Installer, Telefon, Nachrichten, Kamera, Galerie, Dateien, Browser, Büro, Uhr, Rechner, Kalender, Kontakte, Notizen, Store, Monitor, Helfer/Agent, Terminal | Nutzer, in Zelle, Rechte laut Manifest | `ultar-ui` + `ultar-bus`-Client (öffentliche App-API) | `apps/` | einzeln als `.uap` – **ohne** Systemupdate |
| 7 | **Fremd-Apps** | `.uap` von Dritten, `.deb`, `.apk`, `.exe/.msi`, Flatpak, Telegram, Spotify, Web-Apps | Nutzer, strenge Zelle, Netz nur laut Profil | App-API bzw. Kompatibilitätsschicht | nicht im Repo (Store) | Store |

**Harte Grenzen (per CI geprüft):**
- **Kernel ↔ alles andere:** nur Systemaufrufe und Gerätedateien. Im Kernel gibt es keine Oberfläche, keinen Installer,
  keine Übersetzungstexte für Nutzer, keine Verbund-Logik (nur der harte Ein/Aus-Schalter und die Paketfilter bleiben).
  → `install.rs`, `startseite.rs` und die Bedien-Konsole ziehen aus dem Kern in Bereich 6 (Programme in Ring 3);
  im Kern bleibt nur eine serielle Not-Konsole für die Fehlersuche.
- **Systemkern ↔ Dienste ↔ Plattform ↔ Apps:** nur `ultar-bus` mit festen, versionierten Nachrichten. Kein Dienst liest
  Dateien eines anderen, keine App ruft Programme des Systems direkt auf.
- **Apps ↔ Hardware:** nie direkt. App fragt Dienst (z. B. Kamera-Dienst), Dienst nutzt HAL, HAL nutzt Treiber.
- **System-Apps haben keine Sonderwege:** sie nutzen dieselbe App-API wie Fremd-Apps, nur mit mehr erlaubten Rechten
  im Manifest (z. B. Telefon darf `radio.anruf`). So ist die API vollständig und jede System-App austauschbar.
- **Einstellungen ist eine App**, die Logik dahinter (`settingsd`) ist Systemkern. Gleiches Muster überall:
  *Oberfläche = App, Zustand und Regeln = Dienst.*

**Einordnung der heutigen Teile:**

| Bereich | heutige Crates / Dateien |
|---|---|
| 1 Kernel | `kernel/ultar-kern` ohne `install.rs`, `startseite.rs`, `shell.rs` (Bedienteil), `sprache.rs`; `kernel/samsung-a30` (Linux-Gerätekern) |
| 2 Treiber | `kernel/ultar-kern/src/drivers/*`, Werkstatt-Baupläne; Linux-Module/Firmware aus dem Ubuntu-Bereich; A30-Teile aus der Shell (`decon.rs` …) als HAL-Backend |
| 3 Systemkern | `ultar-init`, `ultar-ctl`, `ultar-edition`, `ultar-pkg`, `ultar-apps` (Installer-Logik), `ultar-secure` (+stage0), `hw.rs`×2, `update.rs`, `sys.rs`-Einstellungen, `ultar-sprache`→i18n |
| 4 Systemdienste | `ultar-net`, `-audio`, `-modem`, `-ril`, `-radio`, `-bt`, `-notify`, `-vpn`, `-cloud`, `-call`, `-verbund`, `-zellen`, `-llm`, `-face`, `-sshd`, `-admin`, `-compat`; aus der Shell: `sysd.rs`, `privacy.rs`, `gnss.rs`, `sensors.rs`, `wifi.rs`, `voice.rs`, `fp.rs` |
| 5 Plattform | `ultar-wm`, `ultar-ime`; aus der Shell: `gfx`, `ui`, `text`, `kbd`, `display`, `input`, `app_layout`, `app_desktop`, `quick`, Sperrbildschirm aus `app_sys` |
| 6 System-Apps | `ultar-web`, `ultar-office`; aus der Shell: alle `app_*`, `tools.rs`, `monitor.rs`, `scan.rs`, `agent`/`mind`/`app_buddy`/`app_jarvis`; aus dem Kern: Installer, Startseite |
| 7 Fremd-Apps | `ultar-telegram`, `app_spotify`, Web-Apps (WhatsApp/YouTube) → raus aus der Shell, als `.uap` über den Store |

### 3.0b Schichten

Abhängigkeiten zeigen **nur nach unten**. Keine Schicht kennt eine darüberliegende.

```
 L7  Apps            apps/*            (Telefon, Kamera, Galerie, Dateien, Browser, Büro, Einstellungen …)
 L6  Erlebnisse      shell-mobile · shell-desktop      (zwei Modi auf demselben Compositor)
 L5  Oberflächen-    ultar-ui          (Zeichnen, Text, Widgets, Theme, Layout-Klassen, i18n, Barrierefreiheit)
     Baukasten
 L4  Compositor      ultar-compositor  (Fenster, Bildschirme, Eingabe, Skalierung – nativ, ohne Ubuntu)
 L3  Systemdienste   devd · net · audio · power · radio/ril · notify · settingsd · pkg · update · verbund · llm
                     verbunden über ultar-bus (ein typisiertes IPC statt Dateien in /run + Polling)
 L2  HAL             ultar-hal (Traits) + Backends: generic-linux · ultar-kern · exynos7885 (A30) · qcom-… (OP15)
 L1  Kern            Linux (Gerätekern) ODER Ultar-Kern – beide hinter derselben Linux-Systemaufruf-Schnittstelle
 L0  Startkette      stage0 · dm-verity · A/B-Slots · Startschalter (bekannter Fall wird gemerkt)
```

### 3.1 HAL – Hardware nur an einer Stelle
`system/hal/ultar-hal` definiert Traits je Geräteklasse; jedes Backend implementiert sie. Oberfläche und Apps sehen
**nie** einen `/sys`- oder `/dev`-Pfad.

| Trait | Inhalt | Backends heute vorhanden (wird verschoben) |
|---|---|---|
| `Display` | Modi, Puffer, Helligkeit, Drehung, EDID/DPI, Hotplug | DRM/KMS, fbdev (generic), DECON+ION (`decon.rs` → exynos7885), GOP/Bochs (ultar-kern) |
| `Input` | Touch, Tasten, Maus, Tastatur, Gesten-Rohdaten | evdev (generic), PS/2 + xHCI-HID (ultar-kern) |
| `Audio` | Ausgänge/Eingänge, Lautstärke, Routen | ALSA (generic), ABOX/COD3035/SMA1301 (exynos7885), HDA (ultar-kern) |
| `Power` | Akku, Laden, Thermik, Leistungsprofile | power_supply (generic), Samsung-Knoten (exynos7885) |
| `Radio` | Modem, SIM, Netz, Daten, USSD | modem_v1/SIPC5 (exynos7885, aus `ultar-modem`/`ultar-ril`) |
| `Wifi`, `Bluetooth` | Scan, Verbinden, Schalter | nl80211 (generic), SCSC (exynos7885) |
| `Camera`, `Sensors`, `Gnss`, `Vibrator`, `Led` | … | v4l2/IIO (generic), fimc-is/Kepler/timed_output (exynos7885) |

Die Wahl des Backends trifft **devd** über das Geräteprofil (Abschnitt 5) – das bewährte Prinzip aus `ultar-init/hw.rs`
(„alle Fälle probieren, den funktionierenden merken“) wird hierhin verallgemeinert, und `hw.rs` gibt es nur noch einmal.

### 3.2 Ein Compositor, zwei Erlebnisse
- **Entscheidung (Empfehlung):** ein nativer Compositor `ultar-compositor` in Rust (Wayland-Protokoll über die reinen
  Rust-Teile von Smithay; Zeichnen über eigenen Software-Renderer, GPU-Renderer als HAL-Backend wo vorhanden).
  `app_desktop.rs` und `ultar-wm` gehen darin auf – es bleibt **eine** Fensterlogik.
- **shell-mobile** (Startbildschirm, Gesten, Schnelleinstellungen, Vollbild-Apps) und **shell-desktop** (Leiste, Startmenü,
  freie Fenster, Kacheln) sind zwei Erlebnisse auf demselben Compositor, mit denselben Apps.
- **Desktop-Modus für Handy und Tablet bleibt**: Wechsel automatisch (externer Bildschirm, Tastatur + Maus, Tablet quer
  mit Tastatur) oder per Schnellschalter; laufende Apps bleiben offen, nur die Anordnung wechselt.
- Grafische Ubuntu-/Windows-Programme laufen als Wayland-Clients (Xwayland nur auf Wunsch) im selben Compositor.

### 3.3 Apps als eigene Bausteine
```rust
pub trait UltarApp {
    fn manifest(&self) -> &AppManifest;          // id, Name (i18n-Schlüssel), Symbol, benötigte Fähigkeiten, Formfaktoren
    fn update(&mut self, ev: &Event, ctx: &mut Ctx) -> Action;
    fn view(&self, ui: &mut Ui, layout: LayoutClass);   // kompakt | mittel | weit
}
```
- Jede App ist ein eigenes Crate unter `apps/`, nur abhängig von `ultar-ui`, `ultar-bus`-Client und `ultar-hal`-**Traits**.
- Stufe 1: Apps werden noch in denselben Prozess gelinkt (Register statt `enum Screen`). Stufe 2: Apps als eigene Prozesse
  in Zellen (Absturz einer App ≠ Absturz der Oberfläche, Rechte je App).
- `Screen`-Riesen-Enum und `struct App` lösen sich auf in: `Router` (Navigation), `AppRegistry`, je App eigener Zustand.

### 3.4 Eine Sprache für alles
`system/kern/ultar-i18n`: Schlüssel + Übersetzungsdateien (`i18n/ar.ftl`, `en.ftl`, `de.ftl`, Fluent-Format), gemeinsam für
Kern-Installer, Shell, Apps, CLI-Werkzeuge und Ultar Flash. Arabisch ist Referenzsprache; der Bau **scheitert**, wenn ein
Schlüssel in einer Sprache fehlt. Ziffern 0–9 überall. `ultar-sprache` (Übersetzung von Ausgabetexten) entfällt.

---

### 3.4b Arabisch überall – Arabisch ist die erste Sprache des Systems
Arabisch ist nicht eine Übersetzung unter dreien, sondern die Sprache, in der Ultar OS entworfen und geprüft wird.
Englisch und Deutsch folgen.

**Wo Arabisch sichtbar sein muss (jeder Bereich aus §3.0):**

| Bereich | Arabisch heißt hier |
|---|---|
| Startkette / Kernel | Startbild, Startmenü, Rettungsmodus, Fehlermeldungen beim Start und Not-Konsole auf Arabisch (arabische Schrift im Kern vorhanden: `arabic16.psf`); Texte kommen aus derselben i18n-Quelle, nicht aus eigenem Code im Kern |
| Installer / Ultar Flash | vollständig Arabisch, RTL, Arabisch als vorausgewählte Sprache |
| Treiber & Hardware | Gerätenamen, Zustände und Fehlermeldungen übersetzt (Klassen-Namen aus i18n, nicht roh aus dem Treiber) |
| Systemdienste | Benachrichtigungen, Protokoll-Meldungen für Nutzer, Web-Verwaltung `ultar-admin`, SSH-Begrüßung |
| Plattform | RTL als Grundrichtung des Layouts: Leiste, Menüs, Fensterknöpfe, Gesten (Zurück-Wisch), Listen, Regler gespiegelt |
| Apps | jede App vollständig Arabisch; ohne arabische Übersetzung keine Aufnahme in den Store für System-Apps |
| Werkzeuge (CLI) | `ultar-ctl`, `uapp`, `upkg`, `zelle` … Ausgaben und Hilfe auf Arabisch; Befehlsnamen bleiben lateinisch, arabische Kurzbefehle als Zusatz möglich |
| Agent / Helfer / Sprache | versteht und antwortet zuerst Arabisch (Hocharabisch + Dialekte), Weckwort, Spracherkennung und Sprachausgabe Arabisch; Modell-Empfehlung bevorzugt Modelle mit gutem Arabisch |
| Doku & Hilfe | Hilfe-System und Benutzer-Doku zuerst Arabisch; Fehlermeldungen mit arabischem Hinweis „was tun“ |

**Technik, die das trägt (in `ultar-i18n` und `ultar-ui`):**
- **Layout in logischen Richtungen:** `start/ende` statt `links/rechts` im ganzen UI-Baukasten; RTL ist dann automatisch
  richtig. `links/rechts` im UI-Code ist verboten (CI-Prüfung), außer für echte physische Richtungen.
- **Textsatz:** Buchstabenformung, Ligaturen, Tashkeel, Kashida, gemischter Text (BiDi) Arabisch/Latein/Zahlen in jeder
  Textstelle – auch Terminal, Editor, Browser, Büro-Apps, PDF. Hochwertige arabische Schrift als Systemschrift
  (Noto Naskh/Kufi bzw. eigene Markenschrift), Zeilenhöhe für arabische Zeichen ausgelegt.
- **Zahlen und Formate:** Ziffern 0–9 (Entscheidung steht), Datum gregorianisch + Hijri, Woche ab Samstag wählbar,
  Zeit von Damaskus als Voreinstellung, Währung/Einheiten, Mehrzahlformen des Arabischen (6 Formen) über Fluent.
- **Eingabe:** arabische Tastatur (Gboard-Belegung) als Voreinstellung, Wortvorschläge, Sprachdiktat; Suche findet
  unabhängig von Tashkeel, Hamza-/Alef-Formen und ة/ه, ي/ى.
- **Sortieren und Suchen** nach arabischer Reihenfolge (Kontakte, Dateien, Apps).
- **Sprachausgabe/Bildschirmleser** auf Arabisch (Teil der Barrierefreiheit).
- **Namen:** Apps, Einstellungen und Systemordner haben arabische Anzeigenamen (Dateipfade bleiben lateinisch, damit
  Programme kompatibel bleiben).

**Durchsetzung:**
- Arabisch ist die **Referenzsprache**: ein neuer Text wird zuerst arabisch angelegt; fehlt `ar`, scheitert der Bau.
- Bildschirmtests laufen **immer zuerst in Arabisch** (RTL) – in 3 Größen; Prüfung auf abgeschnittenen Text,
  falsche Richtung, lateinische Reste.
- CI-Suche nach fest eingebauten Texten (deutsch/englisch) außerhalb der `.ftl`-Dateien – auch im Kern, in Skripten
  und in Ultar Flash.
- Abnahme je Phase: ein kompletter Durchlauf (Installation → Einrichtung → jede System-App) nur in Arabisch.

### 3.5 Ultar-Kern zuerst
Die Regeln sollen den eigenen Kernel voranbringen, nicht nur Ordnung schaffen.

- **Kern-zuerst-Regel:** Jede neue Systemfunktion wird gegen die HAL-Traits gebaut und muss auf dem Ultar-Kern laufen
  (`hal-ultar-kern`). Linux ist Rückfall für Hardware, deren Treiber noch fehlen (A30, OP15) – kein Ziel.
- **Keine neue Linux-Abhängigkeit:** kein neuer Aufruf von `modprobe`, `apt`, `iw`, `nft`, `cryptsetup` usw. in
  Systemkern, Diensten oder Plattform. Bestehende werden hinter HAL-/Dienst-Schnittstellen gekapselt und stehen auf
  einer Abbau-Liste (CI zählt sie; die Zahl darf nur sinken).
- **Paritätstabelle** (`docs/KERN-PARITAET.md`, von der CI erzeugt): je HAL-Trait und Systemaufruf – Linux ✔ / Ultar-Kern
  ✔ / fehlt. Jede Phase muss die Lücke verkleinern; Desktop und Server sind die ersten Ausgaben, die **nur** mit dem
  Ultar-Kern ausgeliefert werden (Zielrechner: eigener PC, Beelink GTi15, VMware).
- **Treiber-Werkstatt als Fließband:** Gerät ohne Treiber → Bauplan → erzeugter Treiber → Prüfung in QEMU/am Gerät →
  `.utr` ins Depot. Reihenfolge der fehlenden Treiber nach Zielrechnern: Intel-/AMD-Grafik, WLAN, Realtek-Ethernet,
  USB-Hubs, virtio-gpu/-input, Bluetooth, ALSA-Schnittstelle für `gen-hda`.
- **Keine „Grenze“:** Für Geräte ohne offene Beschreibung gilt „beobachten (Ubuntu/Windows) – verstehen – selbst
  bauen“, als letzter Bauteil (siehe `docs/DRIVER-TARGETS.md`, Stufe 4).
- **CI startet jede Änderung auf dem Ultar-Kern** (QEMU bis zur Oberfläche), nicht nur auf Linux.

### 3.6 Leistung – das Beste aus der Hardware holen
Leistung ist eine Anforderung mit Messwert, kein Nachgedanke. Jede Phase hat Budgets; überschreitet ein Merge sie,
scheitert die CI.

| Messwert | Budget Handy (A30) | Budget PC/Laptop |
|---|---|---|
| Kaltstart bis bedienbare Oberfläche | ≤ 6 s | ≤ 3 s |
| Bildzeit Oberfläche (99 %) | ≤ 16 ms (60 Hz) | ≤ 8 ms (120 Hz, wo Panel es kann) |
| Reaktion auf Berührung/Klick | ≤ 50 ms | ≤ 30 ms |
| App-Start (System-App, warm) | ≤ 300 ms | ≤ 150 ms |
| RAM nach Start mit Oberfläche | ≤ 150 MB | ≤ 300 MB |
| Aufwachen im Leerlauf | ≤ 1×/min je Dienst | ebenso |

Wie das erreicht wird – fest in der Architektur:

- **GPU statt Software, wo vorhanden:** Compositor und `ultar-ui` zeichnen über ein Renderer-Trait der HAL
  (GPU-Backend; Software nur als Rückfall). Nur veränderte Bereiche neu zeichnen, Puffer direkt aufs Panel
  (kein Kopieren), Bildwiederholrate dem Panel folgend (VRR).
- **Alle Kerne und Spezialchips nutzen:** `devd` meldet Topologie (große/kleine Kerne, Cache, NUMA), SIMD-Stufen
  (NEON/SVE, AVX2/AVX-512), GPU, NPU. Zeitplanung legt Oberfläche auf schnelle Kerne, Hintergrund auf sparsame;
  Rechenkerne (Bild, Ton, Sprachmodell, Krypto) wählen zur Laufzeit den schnellsten Weg. Agent/LLM nutzt NPU/GPU.
- **Ereignisgesteuert, kein Polling:** `ultar-bus` ersetzt Dateien in `/run` + Zeitschleifen (heute z. B.
  Benachrichtigungen alle 400 ms). Datenübergabe zwischen Dienst und App über geteilten Speicher statt Kopien
  (Kamerabilder, Ton, Fensterpuffer).
- **Schneller Start:** Dienste parallel und bei Bedarf starten (Socket-/Bus-Aktivierung), gemerktes Geräteprofil
  lädt nur die bekannten Treiber, System-Abbild read-only und komprimiert (zstd), Apps vorgewärmt.
- **Energie = Leistung:** Leistungsprofile aus der HAL (Frequenz, Thermik, Ladezustand); „Leistung“, „Ausgewogen“,
  „Sparen“ – automatisch nach Last und Temperatur, auf PC inkl. TDP-Grenzen.
- **Kern-Vorteile ausspielen:** Auf dem Ultar-Kern eigene schnelle Wege, die Linux so nicht bietet – z. B. direkter
  Bildpuffer-Tausch Compositor↔Kern, asynchrone Ein-/Ausgabe als Standard, Treiber ohne Zwischenschichten.
- **Messen statt schätzen:** eingebaute Spuren (`ultar-log` mit Zeitstempeln je Bild, Start, IPC), Bildschirm
  „Diagnose“ zeigt sie; Messläufe in CI (QEMU) und nächtlich am A30 und Beelink, Verlauf als Kurve im Repo.
- **Bau:** Profil-geführte Optimierung (PGO) + LTO für Compositor, `ultar-ui`, Kern; Zielprozessor je Geräteprofil
  (z. B. `x86-64-v3` für PC), weiterhin statisch.

### 3.7 Abschottung im Ultar-Kern: Namensräume, Gruppen, Profile mit Rechten

Voraussetzung für „jede App ein eigener Prozess in einer Zelle“ (P5), für Zellen/Container und für die Rechte je App.
Der Kern bekommt drei Bausteine, die zusammen **eine** Sache bilden – die Zelle:

| Baustein | Was er trennt/begrenzt | Umfang |
|---|---|---|
| **Namensräume** (was ein Prozess *sieht*) | Dateibaum (mount) · Prozessliste (pid) · Netz (net: eigene Schnittstellen, Routen, Brandmauer) · Rechnername (uts) · IPC/gemeinsamer Speicher (ipc) · Benutzer-Nummern (user) · Geräte (dev: nur freigegebene Gerätedateien) | Linux-verträgliche Aufrufe `clone`/`unshare`/`setns`, damit vorhandene Werkzeuge (Container, Kiri) laufen |
| **Gruppen** (wie viel ein Prozess *verbraucht*) | Prozessor-Anteil und Kerne (schnell/sparsam) · Speicher-Obergrenze · Ein-/Ausgabe · Zahl der Prozesse · Grafikkarte/NPU-Anteil · Netz-Bandbreite · Energie (im Hintergrund einfrieren) | **neu gruppiert** statt Linux-Nachbau: ein Baum mit vier festen Ästen – `system` (Kern-Dienste) · `session` (Oberfläche, aktive App bevorzugt) · `apps` (je App eine Gruppe; Vordergrund/Hintergrund/eingefroren) · `cells` (Container, Fremdsysteme). Die Zeitplanung kennt die Äste: Oberfläche und aktive App gehen immer vor. Linux-Sicht (`cgroup2`-Dateien) als Übersetzung für fremde Programme. |
| **Rechte** (was ein Prozess *darf*) | einzelne Fähigkeiten statt „root oder nicht“ (Netz öffnen, Geräte, Zeit stellen, Module laden …) · Filter für Systemaufrufe · Dateizugriff nur auf freigegebene Pfade | Standard: nichts erlaubt; jede Erlaubnis kommt aus dem Profil |

**Profile mit Rechten (später, Schnittstelle wird jetzt mitgeplant):** ein Profil ist ein benannter, signierter Satz aus
Namensräumen + Gruppengrenzen + Rechten. Der Kern setzt es beim Start des Prozesses durch; eine App kann ihr Profil nie
selbst erweitern.

| Profil | Für |
|---|---|
| `app.standard` | Fremd-Apps: eigener Dateibereich, Netz nur nach Erlaubnis, keine Geräte |
| `app.system` | System-Apps von KM Code Craft: zusätzlich benannte Bus-Dienste (z. B. Telefon → `radio.call`) |
| `service.<name>` | je Systemdienst genau die nötigen Rechte (z. B. `service.net`: Netz verwalten, sonst nichts) |
| `cell.linux` · `cell.windows` · `cell.android` | Fremdsysteme: eigenes Netz, eigener Dateibaum, kein Zugriff auf Nutzerdaten außer Freigabe-Ordner |
| `user.admin` · `user.standard` · `user.child` · `user.guest` · `user.work` | Benutzer-Profile: was ein Mensch darf (Apps installieren, Einstellungen ändern, Zeiten, Arbeits-/Privatbereich getrennt) |

Reihenfolge im Bau: mount + pid + Rechte-Grundlage (reicht für P5) → net + Gruppen-Baum → user/ipc/uts/dev →
Profile als Dateien + Verwaltung in den Einstellungen („Benutzer“, „Apps → Rechte“, „Zellen“). Jede Stufe mit
Ausbruchs-Tests (ein Prozess versucht, aus seiner Zelle zu sehen, zu schreiben, zu senden) in der Kern-Prüfung.

## 4. Trennung: gemeinsam · Handy · Desktop · Server

Gesteuert über **Fähigkeiten**, nicht über die Ausgabe. Die Ausgabe (`phone/desktop/server`) bleibt nur als
Voreinstellung für Bau und Dienstauswahl.

```rust
pub struct Geraeteprofil {
    pub formfaktor: Formfaktor,            // Handy | Tablet | Laptop | Desktop | Server | Platine
    pub faehigkeiten: Faehigkeiten,        // touch, tastatur, maus, akku, modem, gnss, kamera, sensoren,
                                           // externe_anzeige, mehrere_anzeigen, tpm, npu, gpu …
    pub modus: Modus,                      // Mobil | Desktop | Kopflos (Server)
}
```

| Baustein | Alle Geräte | nur Handy/Tablet | nur Desktop/Laptop | nur Server |
|---|---|---|---|---|
| Startkette, init, devd, HAL-Kern, bus, settingsd, pkg, update, Sicherheit, Zellen, Verbund, notify, i18n | ✔ | | | |
| Compositor + ultar-ui | ✔ (außer Server) | | | – |
| shell-mobile | | ✔ | Tablet-Modus bei Touch-Laptop | |
| shell-desktop | | ✔ als Desktop-Modus | ✔ | |
| Radio/RIL, SIM, Anrufe, SMS, USSD, GNSS, Sensoren, Näherung, Vibration, Fingerabdruck | | ✔ | (nur wenn Fähigkeit vorhanden) | |
| Kamera, Galerie, Dateien, Browser, Büro, Agent, Store | ✔ | | | |
| Mehrere Monitore, Fenster frei, Maus-Zeiger, Startmenü | | im Desktop-Modus | ✔ | |
| Akku/Energieprofile | wenn `akku` | ✔ | Laptop | |
| ultar-admin (Web), SSH, Podman | | | auf Wunsch | ✔ |

Regel im Code: Eine App/ein Panel erklärt im Manifest, welche Fähigkeiten es braucht; fehlt eine, erscheint es nicht.
`if edition == …` ist außerhalb von `ultar-edition` verboten (CI-Prüfung).

---

## 5. Treiber: im Installationsmedium, dynamisch zur Laufzeit

### 5.1 Treiberpaket `.utr` (Ultar-Treiber)
Gleicher Aufbau wie `.uap`, **Signaturpflicht** (Ed25519, eingebrannte Schlüssel – unsigniert wird nie geladen):

```toml
# ultar-treiber.toml
id        = "gen-intel-eth"
version   = "1.2.0"
klasse    = "netz"                       # anzeige | eingabe | ton | netz | wlan | bt | speicher | modem | kamera …
kern      = ["ultar-kern-abi-1", "linux-6.8.0-*"]   # für welchen Kern
herkunft  = "werkstatt"                  # hand | werkstatt | linux-modul | hersteller
rang      = 50                           # höher = bevorzugt bei mehreren Kandidaten
[passt]                                  # Zuordnung zur Hardware
pci       = ["8086:10d3", "8086:15b8"]
pci_klasse= []
usb       = []
dt        = []                           # Gerätebaum „compatible“ (ARM)
acpi      = []
dmi       = []                           # Modell-Feinabstimmung
[inhalt]                                 # was im Paket liegt
modul     = "treiber/gen_intel_eth.uko"  # Ultar-Kern-Modul ODER Linux .ko
firmware  = []
hal       = ""                           # optionales HAL-Backend (Userspace) + Konfiguration (z. B. Mixer-Routen)
```

### 5.2 Treiber-Depot im Installationsmedium
```
ULTAR-INSTALL/
  treiber/<arch>/index.toml          # Zuordnungstabelle aller Pakete (schnell durchsuchbar, signiert)
  treiber/<arch>/*.utr               # alle Treiber + Firmware, offline
```
- Das Medium funktioniert **ohne Internet** (passt zum USB-Update-Weg und zu Syrien).
- Online ist nur Zusatz: neuere Treiber vom Ultar-Server, gleiche Signaturpflicht.

### 5.3 Ablauf im Installer
1. Hardware erfassen (PCI/USB/ACPI/DMI/Gerätebaum) → **Geräteprofil**.
2. Abgleich mit `index.toml` → je Gerät die Kandidaten nach Rang.
3. Live-Probe im Installer: bester Kandidat wird geladen und geprüft (Bild da? Netz da? Ton-Test?), sonst nächster.
4. Anzeige „Erkannte Hardware“ (wie Ubuntus Treiber-Schritt): Gerät · Treiber · Zustand · Alternativen; Geräte ohne Treiber →
   „Bauplan für die Werkstatt anlegen“.
5. Auf die Zielplatte kommen die **gewählten** Treiber + das Profil; Option „alle Treiber mitnehmen“ für tragbare Installationen.

### 5.4 Zur Laufzeit – `ultar-devd`
- Ersetzt `hw.rs` (init + shell), `coldplug`, `drivers`-Dienst und die Treibersuche der Shell.
- Ereignisse (Hotplug/uevent bzw. Kern-Geräteereignisse) → Zuordnung → Kandidat laden → **Gesundheitsprüfung** →
  bei Fehler nächster Kandidat, fehlerhafter Treiber kommt in Quarantäne (mit Begründung im Protokoll).
- Ergebnis wird gemerkt (bewährtes Profilprinzip): nächster Start lädt direkt den bekannten Treiber.
- Meldet über `ultar-bus` die **Fähigkeiten** (z. B. `modem=da`, `anzeigen=2`, `dpi=401`) an alle Dienste.

### 5.5 Ultar-Kern: von einkompiliert zu ladbar
- **Stufe A (sofort):** alle Treiber bleiben einkompiliert, aber Aktivierung nur noch über Profil/Rang (`ALL` wird Register
  mit Metadaten wie `.utr`). Werkstatt erzeugt zusätzlich das Manifest.
- **Stufe B:** ladbare Kern-Module (`.uko`, verschiebbares ELF, stabile Kern-Treiber-ABI v1, signiert) aus dem Depot.
  Kern-Abbild wird klein, neue Hardware braucht keinen neuen Kern.

### 5.6 Danach: System und Einstellungen passen sich an
`settingsd` bildet aus Profil + Fähigkeiten die Voreinstellungen und die sichtbaren Panels:

| Erkannt | Anpassung |
|---|---|
| Panel-DPI / EDID | Skalierung (gebrochen), Schriftgröße, Layout-Klasse |
| Akku | Energieprofile, Akku-Panel, Statusleiste |
| Modem + SIM | Mobilfunk-Panel, Telefon/SMS, USSD |
| zweiter Bildschirm / Tastatur+Maus | Angebot Desktop-Modus (oder automatisch, je Einstellung) |
| GPU / NPU | Renderer-Backend, Modellwahl für den Agenten |
| TPM | Verschlüsselungsoption mit TPM |
| CPU/Thermik (z. B. Beelink TDP) | Leistungsprofile |

Einstellungs-Panel **„Treiber & Hardware“**: Geräteliste, aktiver Treiber, Alternativen, Wechseln, Zurückrollen,
Firmware aktualisieren (USB/online), Werkstatt-Bauplan exportieren.

---

## 6. Ziel-Struktur des Repos

```
ultar-os/
  Cargo.toml                 # Workspace; Ordner = Bereich aus §3.0
  kernel/                    # Bereich 1 – nur Kernel
    ultar-kern/              #   Kern (Ring 0), eigene Abhängigkeiten, kein Zugriff auf system/ oder apps/
    linux/                   #   Konfiguration der Linux-Gerätekerne (a30 …)
  treiber/                   # Bereich 2
    kern/                    #   Ultar-Kern-Treiber, je Treiber ein Ordner (Quelle + Manifest)
    linux/                   #   Listen der Linux-Module/Firmware je Gerät
    werkstatt/               #   Baupläne, Aufträge, Erzeuger
    depot/                   #   Bau der .utr + index.toml fürs Installationsmedium
  system/
    kern/                    # Bereich 3 – Systemkern
      ultar-init, ultar-stage0, ultar-devd, ultar-bus, ultar-profil (war ultar-edition), ultar-settingsd,
      ultar-pkg (+ ultar-apps), ultar-update, ultar-secure, ultar-i18n, ultar-log, ultar-ctl
    hal/                     # Bereich 3 – Hardware-Abstraktion
      ultar-hal (Traits), hal-generic-linux, hal-ultar-kern, hal-exynos7885 (A30), hal-qcom-… (OP15, später)
    dienste/                 # Bereich 4 – Systemdienste
      net, audio, power, radio (modem+ril), bt, gnss, sensoren, kamera, notify, vpn, cloud, call,
      verbund, zellen, llm, face, sshd, admin, compat
  plattform/                 # Bereich 5
    ultar-ui, ultar-compositor, ultar-ime, shell-mobile, shell-desktop, app-api (öffentliche Schnittstelle für Apps)
  apps/                      # Bereich 6 – System-Apps, jede ein eigenes Crate, jede als .uap baubar
    einstellungen, installer, telefon, nachrichten, kamera, galerie, dateien, browser, buero, terminal,
    agent, helfer, uhr, rechner, kalender, kontakte, notizen, store, monitor, werkzeuge
  extern/                    # Bereich 7 – Pakete für Fremddienste (nicht im System-Abbild): telegram, spotify, web-apps
  geraete/     a30/, op15/, beelink-gti15/, vmware/, qemu/   # Profile, Boot-Wege, Gerätedoku (statt A30-* in docs/)
  editions/    phone/, desktop/, server/                    # war rootfs-editions/
  rootfs/      nur noch gemeinsame Dateien
  server/      ultraos-server
  tools/       pc/, flash/, chaos/, entwickler/
  docs/        architektur/, adr/, benutzer/, entwickler/
```
`bins/` verlässt das Git: Binaries werden in CI gebaut und als Release-Anhang (mit SHA256SUMS) veröffentlicht.

**Umzug der Shell-Dateien (Beispiele):**

| heute | Ziel |
|---|---|
| `decon.rs`, `camera.rs` (Samsung-Teil), `mic.rs`, A30-Teile aus `audio.rs`/`quick.rs`/`app_bt.rs` | `hal-exynos7885` |
| `display.rs`, `input.rs`, `sensors.rs`, `gnss.rs`, `wifi.rs`, `camera.rs` (v4l2) | `hal-generic-linux` |
| `hw.rs` (Shell) + `ultar-init/hw.rs` + `selftest.rs` | `system/kern/ultar-devd` |
| `gfx.rs`, `ui.rs`, `text.rs`, `kbd.rs`, `icons_svg.rs`, `brand.rs` | `plattform/ultar-ui` |
| `sys.rs` / `sysx.rs` / `sysy.rs` / `sysd.rs` | aufgeteilt nach Bedeutung: `zeit`, `energie`, `sicherung`, `gebetszeiten`, `einheiten` … (Logik in Diensten oder App-Crates) |
| `app_more.rs`, `app_extra.rs`, `app_modern.rs`, `tools.rs` | je App ein Crate (`werkzeuge`, `notizen`, `kontakte`, `uhr`, `rechner`, `dateien` …) |
| `app_desktop.rs` + `crates/ultar-wm` | `plattform/ultar-compositor` + `plattform/shell-desktop` |
| `i18n.rs` + `ultar-sprache` + Installer-`t()` | `system/kern/ultar-i18n` + `i18n/*.ftl` |

---

## 7. Clean-Code-Regeln (verbindlich, per CI geprüft)

0. **Bereichsregel (§3.0):** jedes Crate liegt in genau einem Bereichsordner; erlaubt sind nur diese Abhängigkeiten:
   `apps → plattform/app-api`, `plattform → system/kern (bus, i18n, profil)`, `dienste → system/kern + hal-Traits`,
   `hal-Backends → ultar-hal`, `kernel → nichts aus dem Repo`. Alles andere lässt die CI scheitern.
0b. **Kern-zuerst und Leistungsbudgets (§3.5, §3.6):** keine neue Linux-Abhängigkeit; kein Merge, der ein Budget reißt
   oder die Kern-Parität verschlechtert.
0c. **Arabisch zuerst (§3.4b):** jeder Text zuerst arabisch, Layout nur in `start/ende`, kein fest eingebauter Text.
1. **Schichtregel:** Abhängigkeiten nur nach unten (Prüfskript über `cargo metadata`). Apps kennen keine HAL-Backends.
2. **Keine Hardwarepfade außerhalb der HAL:** `"/sys/"`, `"/dev/"` nur in `system/hal/*` und `treiber/` (Prüfskript).
3. **Keine Ausgabe-Abfragen außerhalb von `ultar-profil`:** Verhalten hängt an Fähigkeiten.
4. **Größe:** Datei ≤ 600 Zeilen (Warnung ab 400), Funktion ≤ 60 Zeilen, kein Typ mit > 15 Feldern.
5. **Fehler:** `clippy::unwrap_used`/`expect_used` verboten außerhalb von Tests; Fehlertypen je Crate; nie `panic` im Dienst.
6. **Benennung nach Bedeutung**, keine `x/y/more/extra`-Namen. Eine Sprache für Bezeichner (Entscheidung offen, siehe §9).
7. **Texte nur über i18n-Schlüssel**; fehlende Übersetzung = Baufehler.
8. **`unsafe` nur in HAL/Kern**, jeder Block mit `// SICHERHEIT:`-Begründung.
9. **Tests:** jede Logik-Datei mit Tests; jede App mit Bildschirmtest (`--shot`) in 3 Größen × 3 Sprachen (Bildvergleich).
10. **ADR** für jede Architekturentscheidung (`docs/adr/NNNN-titel.md`).

**CI-Pipeline (neu, für main):** `fmt` → `clippy -D warnings` → Tests → Schicht-/Pfad-Prüfung → aarch64-Bau **statisch**
(prüft `+crt-static`, die A30-Lektion) → Größenbudget je Binary → QEMU-Start bis Oberfläche + Bildvergleich →
Kern-Test (`wsl-x11.py geraete`-Äquivalent in QEMU).

---

## 8. Fahrplan – in dieser Reihenfolge, das A30 bleibt immer lauffähig

| Phase | Inhalt | Abnahme |
|---|---|---|
| **P0 Sicherung & Messbasis** | CI für main; Messwerte festhalten (Startzeit, RAM, Bildzeit, Binärgröße, Testzahl); Bildschirm-Referenzbilder; `bins/` → Releases | CI grün; Messbericht als Referenz |
| **P0b Leistung & Kern-Parität messbar** | Messläufe (Start, Bildzeit, Reaktion, RAM) in CI auf Linux **und** Ultar-Kern; Paritätstabelle; Abbau-Liste der Linux-Aufrufe | Budgets aus §3.6 als CI-Schranke aktiv; Tabelle erzeugt |
| **P1 Repo-Struktur** | Crates in Schicht-Ordner verschieben, `rootfs-editions` → `editions`, Gerätedoku → `geraete/`; **kein Verhaltenswechsel** | gleiches Abbild bit-gleich bzw. gleiches Verhalten in QEMU + A30 |
| **P2 i18n vereinheitlichen** | `ultar-i18n` + `.ftl`; Shell, Installer, CLI umstellen; `ultar-sprache` entfernen | Prüfung „alle Schlüssel in ar/en/de“ grün; keine deutschen Reste in Ar |
| **P2b Kernel entschlacken** | Installer, Startseite und Bedien-Konsole aus dem Ultar-Kern in eigene Ring-3-Programme (`apps/installer`, Plattform); im Kern bleibt nur die serielle Not-Konsole | Kern enthält keine Oberfläche/Nutzertexte mehr; Installation in QEMU/VMware läuft wie vorher |
| **P2c Abschottung im Kern** | Namensräume (mount, pid), Rechte-Grundlage, Gruppen-Baum – §3.7 | Ausbruchs-Tests grün in QEMU; ein Testprogramm läuft in einer Zelle und sieht nur seinen Bereich |
| **P3 HAL herausziehen** | Traits + `hal-generic-linux` + `hal-exynos7885` + `hal-ultar-kern`; Shell ohne `/sys`/`/dev` | Pfad-Prüfung grün; A30: Anzeige, Touch, Akku, WLAN, Ton wie vorher |
| **P4 devd + Profil** | ein `hw`, Fähigkeiten über `ultar-bus`, `coldplug`/`drivers` ersetzt | QEMU, VMware, Beelink, A30 erkennen alles wie vorher; Treiberwechsel/Rückfall funktioniert |
| **P5 Oberfläche zerlegen + Apps als Prozesse** | `ultar-ui`, `app-api`, Compositor-Kern, jede System-App als eigener Prozess in einer Zelle; `ultar-shell`-Monolith aufgelöst | alle Bildschirmtests gleich; Absturz einer App lässt die Oberfläche stehen; Rechte je App greifen |
| **P6 Compositor + Modi** | `ultar-compositor`, shell-mobile/-desktop, Desktop-Modus am Handy/Tablet, mehrere Monitore, gebrochene Skalierung; `ultar-wm` und `app_desktop` aufgelöst | Laptop (VMware/Beelink) mit 2 Monitoren; A30 Modus-Wechsel ohne App-Verlust |
| **P7 Treiberpakete + Depot** | `.utr`, Signatur, `index.toml`, Werkstatt erzeugt Pakete; Ultar-Kern Stufe A | Installation offline auf Beelink + VMware mit allen Treibern aus dem Medium |
| **P8 Installer auf Ubuntu-Niveau** | Schritte Sprache · Tastatur · Netz · **Treiber** · Platte · Verschlüsselung (TPM/Passphrase) · Konto · Zeitzone · Zusammenfassung; gleicher Ablauf in Ultar Flash für Handys | Installation in < 10 min, Ar/En/De, Touch + Tastatur |
| **P9 Dynamische Einstellungen** | `settingsd`, Panels aus Fähigkeiten, Panel „Treiber & Hardware“, Firmware-Updates | kein Panel ohne Hardware; Skalierung stimmt auf A30/Laptop/4K |
| **P11 Ultar-Kern auf ARM64** | Port + Handy-Treiber über die Werkstatt (siehe §9) | Handy startet ohne Linux bis zur Oberfläche; danach Funk, Ton, Kamera |
| **P10 Gleichwertigkeit** | Barrierefreiheit, A/B-Updates mit Rückfall, Rettungsmodus, Ladbare Kern-Module (Stufe B) | Prüfliste §2 vollständig grün |

Aufwand grob: P0–P2 klein, P3–P5 mittel (größter Umbau, aber mechanisch), P6–P10 groß. Jede Phase endet mit Merge auf main
und lauffähigem A30 – kein langer Parallelzweig.

---

## 9. Entscheidungen (getroffen am 22.11.2026)

| # | Frage | Entscheidung | Folge im Plan |
|---|---|---|---|
| 1 | Compositor | **nativer `ultar-compositor`** in Rust | `ultar-wm` (Smithay im Ubuntu-Bereich) und `app_desktop.rs` werden in P6 aufgelöst; kein Mesa/Ubuntu als Pflicht. GPU-Zeichnen kommt über das Renderer-Trait der HAL. |
| 2 | Sprache der Bezeichner | **zuerst Englisch**, danach auch Arabisch | Alle Bezeichner (Crates, Module, Typen, Funktionen, Dateien, Bus-Nachrichten) werden englisch – Umbenennung der deutschen Namen in P1 (`geraet`→`device`, `werkstatt`→`workshop`, `zellen`→`cells`, `verbund`→`mesh` …). Stufe 2 Arabisch: Doc-Kommentare, Entwickler-Doku, Fehlertexte und ein Glossar Englisch↔Arabisch für jeden Fachbegriff; arabische Namen für CLI-Befehle und Einstellungs-Schlüssel als Alias. |
| 3 | Apps als eigene Prozesse | **schon in P5** | P5 liefert `app-api` über `ultar-bus`, jede System-App als eigener Prozess in einer Zelle mit Rechten laut Manifest; Fenster über den Compositor. Deshalb wird der Compositor-Kern (Fenster eines Fremdprozesses anzeigen) von P6 nach **P5 vorgezogen**; P6 behält Desktop-Modus, mehrere Monitore, Skalierung. |
| 4 | Linux-Weg | **nur noch Ultar-Kern** | Linux-Module werden **nicht** als `.utr` paketiert; `.utr` enthält nur Ultar-Kern-Treiber (`kern = ["ultar-kern-abi-1"]`). `hal-generic-linux`, `coldplug`, apt-Treibersuche und der Ubuntu-Bereich als Treiberquelle entfallen für Desktop/Server sofort. |

**Was Entscheidung 4 für die Handys bedeutet (ehrlich):** Der Ultar-Kern läuft heute nur auf x86_64 (UEFI). Für A30 und
OP15 gibt es noch keinen ARM-Port und keine Treiber (Anzeige, Touch, Modem, WLAN, Kamera). Deshalb:

- **Desktop, Laptop, Server:** ab jetzt ausschließlich Ultar-Kern.
- **Handy:** der Samsung-Linux-Kern bleibt als **eingefrorene Übergangslösung** hinter `hal-exynos7885` – es wird dort
  nichts Neues mehr gebaut, nur lauffähig gehalten, bis der Ultar-Kern das Gerät trägt.
- **Neue Phase P11 – Ultar-Kern auf ARM64:** Port (Start über Gerätebaum, GIC, Zeitgeber, MMU), dann Treiber je Gerät
  über die Werkstatt: Anzeige/Touch → Speicher → USB → WLAN → Ton → Modem → Kamera. Erstes Ziel ist das Gerät mit der
  besten offenen Dokumentation (Vorschlag: zuerst QEMU-`virt` und eine Entwicklungsplatine, dann das Handy).
  Modem- und Kamera-Firmware der Hersteller bleiben geschlossene Blobs, die der eigene Treiber nur lädt und anspricht.

Wenn „nur noch Ultar-Kern“ auch heißen soll, dass das A30 **sofort** ohne Linux laufen muss, ist das A30 bis zum Ende
von P11 nicht benutzbar – dann bitte Bescheid geben, der Fahrplan würde sich ändern (P11 vor P3).
