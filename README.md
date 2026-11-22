# Ultar OS

<div dir="rtl">

**نظام تشغيل مستقل من KM Code Craft** — نواة خاصة مكتوبة بلغة Rust، العربية أولاً، للهاتف والحاسوب والخادم.

</div>

Ultar OS is an independent operating system by **KM Code Craft** with its own kernel written in Rust
(the *Ultar kernel*), Arabic as its first language, and one code base for phone, desktop/laptop and server.

> **Stand 22.11.2026 – Neuaufbau (v2) in Arbeit.** Dieses Repository ordnet Ultar OS neu in sieben klar
> getrennte Bereiche. Der Grundbau ist fertig und geprüft, die Oberfläche und die Dienste stammen noch aus dem
> Vorgänger-Repository und werden phasenweise umgebaut. Was fertig ist und was nicht: [docs/STATUS.md](docs/STATUS.md).

## Aufbau – sieben Bereiche

| # | Bereich | Ordner | Inhalt |
|---|---|---|---|
| 1 | Kernel | `kernel/` | Ultar-Kern: Speicher, Prozesse, Systemaufrufe, Dateisysteme, Netz, Geräte-Busse |
| 2 | Treiber | `drivers/` | signierte Treiberpakete `.utr`, Treiber-Depot für das Installationsmedium |
| 3 | Systemkern | `system/core/`, `system/hal/` | Start, Geräteprofil, Systembus, Sprache, Pakete, Treiberwahl, Hardware-Schnittstelle |
| 4 | Systemdienste | `system/services/` | Netz, Ton, Mobilfunk, VPN, Verbund, Zellen, Sprachmodell … |
| 5 | Plattform | `platform/` | Oberfläche, Tastatur, Fensterverwaltung |
| 6 | System-Apps | `apps/` | Browser, Büro … |
| 7 | Fremd-Apps | `external/` | Pakete für Dienste Dritter |

Abhängigkeiten zeigen nur nach unten; das prüft `ultar-check` bei jeder Änderung.
Plan, Regeln und Fahrplan: [docs/PLAN.md](docs/PLAN.md).

## Grundsätze

- **Eigener Kernel zuerst.** Desktop und Server laufen nur auf dem Ultar-Kern; keine neue Linux-Abhängigkeit.
- **Arabisch zuerst.** Jeder Text entsteht zuerst arabisch, Layout von rechts nach links; Englisch und Deutsch folgen.
- **Fähigkeiten statt Geräteklassen.** Das System richtet sich nach dem, was das Gerät kann (Touch, Akku, Modem,
  zweiter Bildschirm) – Desktop-Modus auch auf Handy und Tablet.
- **Treiber im Installationsmedium.** Signierte Pakete, Auswahl mit Rückfall, ohne Internet installierbar.
- **Leistung ist messbar.** Zielwerte je Gerät als Schranke (Start, Bildzeit, Speicher).
- **Sauberer Code.** Englische Bezeichner, keine Abstürze durch `unwrap`, Texte nur über Schlüssel.

## Neu in v2 (fertig und geprüft)

| Baustein | Aufgabe |
|---|---|
| `system/core/ultar-i18n` | eine Sprachquelle für alles: ar/en/de, arabische Mehrzahl, Hijri-Datum, arabische Suche und Sortierung |
| `system/core/ultar-profile` | Geräteprofil: Bauform, Fähigkeiten, Modus (mobil/Desktop/ohne Bildschirm), sichtbare Einstellungen |
| `system/hal/ultar-hal` | Hardware-Schnittstelle (16 Geräteklassen) mit vollständiger Test-Attrappe |
| `system/core/ultar-bus` | Systembus: Anfragen, Ereignisse, Rechte je Teilnehmer – ohne Polling |
| `system/core/ultar-driverpkg` | Treiberpaket `.utr`, Signaturprüfung, Depot, Zuordnung Gerät → Treiber (Werkzeug `udrv`) |
| `system/core/ultar-devd` | Treiberwahl zur Laufzeit: probieren, prüfen, Rückfall, Quarantäne, merken |
| `tools/ultar-check` | Regelprüfung des Repositories (Bereiche, Hardwarepfade, Texte, Größe, `unsafe` …) |

## Bauen und prüfen

```bash
cargo test --workspace --exclude ultar-llm          # alle Tests des Systems
cargo run -p ultar-check -- all                     # Regeln des Repositories
tools/kern-test/ci.sh                               # Ultar-Kern bauen, in QEMU starten, Treiber + Bus prüfen
drivers/depot/build.sh --test-key                   # Treiber-Depot mit Testschlüssel bauen
```

Der Kern-Test braucht `rustc-1.97 cargo-1.97 rust-1.97-src qemu-system-x86 ovmf mtools dosfstools` (Ubuntu 24.04).

## Geräte

- Testrechner: Lenovo ThinkPad P1 Gen 3 und Beelink GTi15 Ultra – [docs/DRIVER-TARGETS.md](docs/DRIVER-TARGETS.md)
- Samsung Galaxy A30 (Übergang mit Samsung-Kernel): [devices/a30/](devices/a30/)

## Weitere Dokumente

[Plan](docs/PLAN.md) · [Stand](docs/STATUS.md) · [Treiberziele](docs/DRIVER-TARGETS.md) ·
[Schnittstellen der neuen Bausteine](docs/integration/) · [Texte für die Webseite](docs/homepage/) ·
[Dokumente des Vorgängers](docs/legacy/)

© KM Code Craft · Mohamad Anas Kara Mohamad · [code-craft.tech](https://code-craft.tech)
