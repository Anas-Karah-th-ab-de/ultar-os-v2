# Ultar-App-Paket `.uap` — Format & Signaturpflicht

Eigenes Paketformat für den Ultar-Store. **Harte Regel: ohne gültige Signatur keine Installation.**
Jede App ist mit dem privaten Schlüssel von KM Code Craft (oder einem freigegebenen Herausgeber)
signiert; das Gerät prüft gegen die eingebrannten öffentlichen Schlüssel. Ein kompromittierter Server
kann **nichts** Unsigniertes einschleusen — die Prüfung ist geräteseitig und serverunabhängig.

Umgesetzt im Crate `ultar-pkg` (getestet: gültig → OK, fremder Schlüssel → abgelehnt, manipuliert → abgelehnt).

## Aufbau (`.uap` = äußeres tar, unkomprimiert)
| Eintrag | Inhalt |
|---|---|
| `ultar-manifest.toml` | Metadaten (siehe unten) |
| `ultar-payload.tar.gz` | die eigentlichen Dateien der App |
| `ultar-sig.ed25519` | `<keyid>\n<base64-Signatur>` |

**Signierte Nachricht** (kanonisch, eindeutig):
```
ultar-pkg-v1\n<sha256(manifest)>\n<sha256(payload.tar.gz)>\n
```
Signatur = Ed25519 über diese Nachricht. `keyid` = erste 16 Hex-Zeichen von `sha256(pubkey)`.

## Manifest (`ultar-manifest.toml`)
```toml
id = "com.kmcc.pos"          # eindeutige Kennung
name = "KM POS Server"
version = "1.2.0"
arch = "aarch64"             # oder "any"
kind = "compat-deb"          # native | compat-deb | data
exec = "/usr/bin/km-server"  # optional (native)
services = ["kmpos"]         # optional: Dienstnamen (ultar-ctl reload)
permissions = ["network"]    # optional: an Datenschutzzentrum koppelbar
min_os = "0.6.0"             # optional
publisher = "KM Code Craft"
```
`kind`:
- **native** — Payload-Dateien sind unter `usr/`, `etc/` … gewurzelt und werden nach `/` entpackt.
- **compat-deb** — Payload enthält ein `.deb`; es wird im Ubuntu-Bereich installiert (`ultar-compat dpkg -i`).
- **data** — App lebt unter `/data/ultar/apps/<id>/` (keine Systemänderung).

## Werkzeuge
| Werkzeug | Ort | Zweck |
|---|---|---|
| `upkg-keygen` | Server/Entwickler | Herausgeber-Schlüsselpaar erzeugen |
| `upkg-sign <manifest> <ordner> <out.uap> <privkey>` | Server/Entwickler | packen + signieren |
| `upkg info\|verify\|install <datei.uap>` | Gerät | prüfen/installieren (lehnt unsigniert ab) |

## Schlüsselverwaltung
- **Privater Schlüssel**: nur auf dem Signier-/Server-Rechner, offline halten. Niemals aufs Gerät.
- **Öffentliche Schlüssel**: ins Image unter `/etc/ultar/trusted-keys/<name>.ed25519` (hex, eine Zeile).
  Mehrere erlaubt (z. B. KM Code Craft + geprüfte Partner). Fehlt jeder Schlüssel → alles abgelehnt.
- Schlüsselwechsel/Widerruf: alten Pubkey aus `trusted-keys` entfernen (per OTA-Update).

## Installationsablauf am Gerät
1. Store lädt `<app>.uap` vom Server (über das Konto).
2. `ultar-pkg::install` prüft die Signatur gegen `/etc/ultar/trusted-keys`.
3. Ungültig/fehlt/unbekannter Herausgeber → **Abbruch, keine Datei wird geschrieben**.
4. Gültig → Payload je `kind` entpacken, Dienste übernehmen (`ultar-ctl reload`), Vermerk unter
   `/data/ultar/installed/<id>.toml`.

## Server/Store
Der Server (`ultraos-server`) hostet die `.uap`-Dateien und listet sie unter `/v1/store/apps`
(id, name, version, size, sha256, url). Die Vertrauensentscheidung trifft allein das Gerät anhand der
Signatur im Paket. Neue App veröffentlichen: `upkg-sign` → `.uap` hochladen → Zeile in `store_apps`.
