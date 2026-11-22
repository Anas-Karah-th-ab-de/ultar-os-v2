# ultraos-server — Deployment (ultar.code-craft.tech)

Rust/Axum-Dienst für Konten, Rufnummern, Anruf-Signalisierung, Store, OTA, WireGuard-Provisionierung
und das **Ultar-Verbund-Relais** (Geräte desselben Kontos verbinden sich darüber, wenn sie nicht im selben Netz sind).

**Schnellweg:** `sudo server/ultraos-server/deploy/install.sh` (baut, installiert systemd-Dienst, Token-Schlüssel,
Caddy) – oder als Container: `cd server/ultraos-server/deploy && docker compose up -d`.
Alle Dateien liegen in `deploy/` (ultraos.service, Caddyfile, install.sh, Dockerfile, docker-compose.yml).

## 1. Bauen
```bash
cd server/ultraos-server
cargo build --release          # Binary: target/release/ultraos-server
```

## 2. Umgebungsvariablen
| Variable | Zweck | Beispiel |
|---|---|---|
| `ULTRAOS_BIND` | Lauschadresse (hinter Caddy) | `127.0.0.1:8080` |
| `ULTRAOS_DB` | SQLite-Datei | `/var/lib/ultraos/ultraos.sqlite` |
| `ULTRAOS_SECRET` | 32-Byte Token-Schlüssel (hex, **fest setzen!**) | `openssl rand -hex 32` |
| `ULTRAOS_WG_PUBKEY` | öffentlicher WireGuard-Serverschlüssel | `…=` |
| `ULTRAOS_WG_ENDPOINT` | WG-Endpunkt | `ultar.code-craft.tech:51820` |

## 3. systemd — `/etc/systemd/system/ultraos.service` (vollständig gehärtet in `deploy/ultraos.service`)
```ini
[Unit]
Description=Ultar-Cloud Server
After=network.target
[Service]
Environment=ULTRAOS_BIND=127.0.0.1:8080
Environment=ULTRAOS_DB=/var/lib/ultraos/ultraos.sqlite
EnvironmentFile=/etc/ultraos/secret.env
ExecStart=/usr/local/bin/ultraos-server
Restart=always
DynamicUser=yes
StateDirectory=ultraos
[Install]
WantedBy=multi-user.target
```

## 4. TLS-Terminierung — Caddy `/etc/caddy/Caddyfile`
```
ultar.code-craft.tech {
    reverse_proxy 127.0.0.1:8080
}
```
Caddy holt das Zertifikat automatisch (ACME). WebSocket `/v1/signal` wird mit durchgereicht.

## 5. TURN/STUN — coturn `/etc/turnserver.conf`
```
listening-port=3478
tls-listening-port=5349
use-auth-secret
static-auth-secret=<derselbe wie ULTRAOS_SECRET, als Klartext-Secret>
realm=ultar.code-craft.tech
```
(Server gibt kurzlebige Zugangsdaten über `/v1/turn-cred` aus.)

## 6. WireGuard-Server `/etc/wireguard/wg0.conf`
```
[Interface]
Address = 10.80.0.1/16
ListenPort = 51820
PrivateKey = <server-privkey>
PostUp   = nft add table ip ultraos; nft 'add chain ip ultraos nat { type nat hook postrouting priority 100 ; }'; nft add rule ip ultraos nat oifname "eth0" masquerade
PostDown = nft delete table ip ultraos
```
`ULTRAOS_WG_PUBKEY` = `wg pubkey < server-privkey`. Peers legt der Server bei `/v1/vpn/peer` in die DB;
zusätzlich produktiv dort `wg set wg0 peer <pubkey> allowed-ips <ip>` ausführen (oder periodischer
Sync-Dienst, der die DB nach `wg0` schreibt).

## 7. API (v1)
| Methode | Pfad | Auth | Zweck |
|---|---|---|---|
| GET | `/v1/health` | — | Lebenszeichen |
| POST | `/v1/account/register` | — | Konto anlegen → Token |
| POST | `/v1/account/login` | — | Login → Token |
| POST | `/v1/device/register` | Bearer | Gerät + **eindeutige Nummer** |
| GET (WS) | `/v1/signal` | im 1. Frame | Anruf-/Video-Signalisierung |
| GET | `/v1/turn-cred` | Bearer | kurzlebige TURN-Zugangsdaten |
| GET | `/v1/store/apps` | Bearer | App-Liste (signiert) |
| GET | `/v1/update/latest` | Bearer | neuestes System-Image (Kanal via `x-ultar-channel`) |
| POST | `/v1/vpn/peer` | Bearer | WireGuard-Peer provisionieren |
| POST | `/v1/verbund/geraet` | Bearer | Verbund-Gerät (Noise-Schlüssel) dem Konto zuordnen; fremder Schlüssel → 409 |
| POST | `/v1/verbund/geraet/entfernen` | Bearer | Gerät aus dem Konto-Verbund entfernen |
| GET | `/v1/verbund/geraete` | Bearer | Geräte des Kontos (+ online) |
| GET (WS) | `/v1/verbund/relais` | im 1. Frame | Relais: Binärrahmen nur zwischen Geräten desselben Kontos, Inhalt Noise-verschlüsselt |
| GET | `/v1/verbund/statistik` | — | Verbindungen, weitergeleitete/verworfene Rahmen |

### Ultar-Verbund-Relais
Der Server sieht nur Chiffretext (Noise_XX zwischen den Geräten, Ende-zu-Ende). Er leitet nur innerhalb eines
Kontos weiter, begrenzt Puffer (512 Rahmen je Verbindung, 70 KB je Rahmen, 64 Verbindungen je Konto) und meldet
„unerreichbar“, statt zu stauen. Ein Neustart des Servers trennt nur – die Geräte verbinden sich selbst neu,
laufende Aufgaben werden auf andere Geräte umgeplant (siehe docs/VERBUND.md, Chaos-Tests).

## 8. Härtung vor Produktion
- `ULTRAOS_SECRET` fest und geheim (sonst sind alle Token nach Neustart ungültig).
- Rate-Limiting/fail2ban auf `/account/*`.
- DB-Backups (SQLite WAL) + Monitoring.
- Signaturschlüssel (ed25519) für Store/OTA offline halten; nur öffentliche Schlüssel aufs Gerät.
- Pentest der Signalisierung und Auth.

## 9. App-Store: signierte Pakete (.uap)
Alle Apps sind signiert (Ed25519); das Gerät lehnt Unsigniertes ab. Ablauf:
1. Einmalig Herausgeber-Schlüssel erzeugen: `upkg-keygen` → privaten Teil geheim auf dem Signier-
   rechner halten, öffentlichen Teil ins Gerät-Image (`/etc/ultar/trusted-keys/kmcodecraft.ed25519`).
2. App paketieren + signieren: `upkg-sign <manifest.toml> <ordner> <app.uap> <privkey-datei>`.
3. `<app.uap>` auf den Server legen (z. B. unter `/var/lib/ultraos/apps/`, ausgeliefert über
   einen statischen Pfad hinter Caddy), Zeile in `store_apps` eintragen (id, name, version, size,
   sha256, sig, url). `url` zeigt auf die .uap. Format-Details: docs/A30-APP-PAKET.md.
