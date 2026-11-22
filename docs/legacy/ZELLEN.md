# Zellen & Container (`zelle`)

Apps, Windows-Programme (.exe über Wine) und Container laufen in **Zellen**: abgeschottet, mit eigenem Profil.
Einstellungen → **Zellen & Container** oder Befehlszeile `zelle`. Getestet mit `sudo tools/zellen-test.sh` (29/29).

## Was eine Zelle bietet

| Funktion | Umsetzung |
|---|---|
| Eigener Speicher, wächst nach Bedarf | `/data/zelle` = eigener Ordner; kein fester Block, wächst solange Platz frei ist (optional Grenze `speicher.max_gb`) |
| Kein Zugriff auf anderen Speicher | eigener Mount-Namensraum: `/data`, `/home`, `/root`, `/media`, `/mnt`, `/srv`, `/var/log`, `/run/user` leer; eigene Benutzerkennung (60000+), `no_new_privs` |
| RAM dynamisch oder begrenzt | cgroup v2: `speicher.ram_mb` (0 = dynamisch), `ram_garantiert_mb` |
| Keine Standort-/Geräteinfo | kein GPS-/ttyACM-Gerät, neutrale `/proc/cpuinfo` („Ultar Prozessor“), eigene machine-id, Rechnername „ultar“, DMI/Firmware/Akku/USB/Bluetooth in `/sys` leer; Kamera/Mikrofon/USB nur wenn erlaubt |
| Netz frei / aus / nur über VPN | eigener Netz-Namensraum; `netz.art = ueber` + VPN-Zelle oder `wireguard` direkt in der Zelle |
| Killswitch je App | Weiterleitung nur zur Schnittstelle der VPN-Zelle (nftables), Richtlinien-Route mit `unreachable`-Rückfall; VPN-Zelle leitet nur durch ihren Tunnel. Tunnel weg oder VPN-Zelle gestoppt → **kein** Verkehr (getestet) |
| Protokoll | Wächter (transparenter Proxy in der Zelle): Ziel, DNS-/SNI-/Host-Name, Bytes hoch/runter je Verbindung; mit `uploads` zusätzlich Anfang jedes unverschlüsselten Uploads (z. B. `POST /…` + Inhalt). Systemweit: `zelle systemprotokoll` (Verbindungen aller Programme mit Programmname) |
| Verknüpfen | `zelle verknuepfen a b`: gemeinsamer Ordner `/data/gemeinsam/<andere>`, gegenseitig beim Namen erreichbar; nicht verknüpfte Zellen erreichen sich nicht |
| Empfehlung statt doppelt | vor jeder Installation prüft Ultar Apps, Zellen und Container auf dieselbe Familie (Browser, Datenbank, …) und schlägt vor, das Vorhandene in einer neuen Zelle zu nutzen bzw. zu verknüpfen – „Trotzdem doppelt installieren“ bleibt möglich |

## Beispiel: zweiter, privater Browser nur über VPN

```sh
zelle neu vpn --vpn-zelle wg0 -- wg-quick up /data/zelle/wg0.conf   # oder: --netz wireguard --wg-conf …
zelle neu browser-privat --app firefox --netz ueber:vpn --uploads     # Standort/Geräteinfo standardmäßig gesperrt
zelle start vpn &  zelle start browser-privat
zelle protokoll browser-privat                                         # was wohin gesendet wurde
```
Für ein Windows-Programm (`ChromeSetup.exe`) gilt dasselbe: `uapp install` legt die App an, `--app <id>` startet sie
in der Zelle mit eigenem Wine-Präfix (Kopie, Original bleibt unberührt).

## Grafikkarten (`zelle gpu`)

- `auto` (Vorgabe): jede Zelle bekommt die Karte mit dem meisten freien Grafikspeicher.
- `alle`: alle Karten zusammen – Speicher und Rechenleistung werden für **KI/Rechenaufgaben** summiert
  (`CUDA_VISIBLE_DEVICES`/`HIP_VISIBLE_DEVICES` mit allen Karten, `LLAMA_ARG_TENSOR_SPLIT` nach Speicher, z. B. 16:8).
- `aus` oder Kartennummer (manuell). Jede Zelle kann die Vorgabe überschreiben (`gpu = "1"`).
- Ehrlich: **Spiele und Oberfläche** nutzen immer eine Karte. Zwei Karten zum Zeichnen zu „einer“ zu verschmelzen
  kann kein heutiger Treiber (SLI/CrossFire sind eingestellt); verteilt wird nach Bedarf, summiert nur bei Rechenarbeit.

## Kiri statt Docker

[Kiri](https://github.com/libracoder/kiri) (Rust, Apache-2.0) ist eine Docker-kompatible Laufzeit ohne VM unter Linux.
`zelle kiri einrichten` holt die aktuelle Version aus dem offiziellen Feed, prüft die SHA-256-Summe und legt den Dienst
`kiri` an (manuell zu starten). Ultar spricht die Docker-Engine-API (`/run/kiri/docker.sock`, sonst `/run/docker.sock`):
`zelle kiri container`, `zelle verknuepfen-container web datenbank` (gemeinsames Netz + Volume), Empfehlung bei
doppelten Abbildern (`postgres:17` → vorhandenen „datenbank“ verknüpfen).

## Grenzen (ehrlich)

- HTTPS-Inhalte sind verschlüsselt: protokolliert werden Ziel, Name (SNI) und Größe, nicht der Inhalt.
- Anderer UDP-Verkehr außer DNS ist in Zellen mit Protokoll gesperrt (QUIC fällt auf TCP zurück).
- Zellen brauchen den Linux-Kern (Namensräume, nftables, cgroup v2). Der eigene Ultar-Kern hat keine Namensräume –
  dort laufen Apps ohne Zellen.
- Kiri wurde in der Testumgebung nicht ausgeführt (nur gegen eine nachgebildete Docker-API geprüft).
