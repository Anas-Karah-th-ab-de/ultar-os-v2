# ultraos-server — Deployment (ultar.code-craft.tech)

Rust/Axum-Dienst für den **öffentlichen Store-Katalog und System-Updates (ohne Konto)**, Konten,
Rufnummern, Anruf-Signalisierung, Zugänge für Premium-Apps und geschützte Update-Kanäle,
WireGuard-Provisionierung und das **Ultar-Verbund-Relais**.

Alle Endpunkte mit Parametern, Antworten und Fehlercodes: **[API.md](API.md)**.

**Schnellweg:** `sudo server/ultraos-server/deploy/install.sh` (baut, installiert systemd-Dienst, Token-Schlüssel,
Caddy) – oder als Container: `cd server/ultraos-server/deploy && docker compose up -d`.
Alle Dateien liegen in `deploy/` (ultraos.service, ultraos-admin, Caddyfile, install.sh, Dockerfile, docker-compose.yml).

> Stand der Prüfung: Server und Verwaltungsbefehle sind durch automatische Tests abgedeckt
> (siehe INTEGRATION.md). Die Dateien in `deploy/` (systemd, Caddy, Docker, `ultraos-admin`) wurden
> angepasst, konnten in der Bauumgebung aber **nicht ausgeführt** werden – zuerst auf einem
> Testserver einspielen.

## 1. Bauen
```bash
cd server/ultraos-server
cargo build --release          # Binary: target/release/ultraos-server
cargo test                     # startet den echten Server auf einem freien Port
```

## 2. Umgebungsvariablen
| Variable | Zweck | Beispiel / Standard |
|---|---|---|
| `ULTRAOS_BIND` | Lauschadresse (hinter Caddy) | `127.0.0.1:8080` |
| `ULTRAOS_LOG` | Protokoll-Stufe `error`/`warn`/`info`/`debug`/`off`. Release-Server: `warn` (keine Zeile je Anfrage, keine Konto-/Geräte-Ereignisse; Anrufe werden nie protokolliert) | `info` |
| `ULTRAOS_DB` | SQLite-Datei | `/var/lib/ultraos/ultraos.sqlite` |
| `ULTRAOS_FILES` | Dateiablage für Pakete und System-Abbilder (Dateiname = SHA-256 des Inhalts) | `/var/lib/ultraos/files` |
| `ULTRAOS_SECRET` | 32-Byte Token-Schlüssel (hex, **fest setzen!**). Gesetzt, aber fehlerhaft → Server startet nicht. | `openssl rand -hex 32` |
| `ULTRAOS_WG_PUBKEY` | öffentlicher WireGuard-Serverschlüssel | `…=` |
| `ULTRAOS_WG_ENDPOINT` | WG-Endpunkt | `ultar.code-craft.tech:51820` |
| `ULTRAOS_TURN_SECRET` | coturn `static-auth-secret` (eigenes Geheimnis, **nicht** der Token-Schlüssel) | leer = TURN aus |
| `ULTRAOS_TURN_HOST` | öffentlicher TURN/STUN-Host (nicht über den CDN-Proxy) | `ultar.code-craft.tech` |
| `ULTRAOS_TRUSTED_PROXIES` | Adressen/Netze der vorgeschalteten Proxys, deren `X-Forwarded-For` geglaubt wird (Komma-getrennt, auch `a.b.c.d/nn`) | `127.0.0.1,::1` |
| `ULTRAOS_LIMIT_GENERAL` | Anfragen je Client: `Stoß,je_Minute` | `120,120` |
| `ULTRAOS_LIMIT_DOWNLOAD` | Downloads je Client | `30,20` |
| `ULTRAOS_LIMIT_AUTH` | Registrierung/Anmeldung je Client | `10,6` |
| `ULTRAOS_SERVER_NAME` | Name des Servers, wie ihn die Geräte aus ihrer Server-Adresse ableiten (Rechnername, bei abweichendem Port `name:port`, klein geschrieben). Steht in jeder Geräte-Signatur; stimmt er nicht, lehnen die Geräte jede Signatur ab (kein Gerät kann sich registrieren). | `ultar.code-craft.tech` |
| `ULTRAOS_MAX_DEVICES` | Geräte je Konto (1–100) | `10` |
| `ULTRAOS_DEVICE_COOLDOWN_SECS` | Wartezeit beim Ersatz eines verlorenen Geräts **und** Zeit, nach der ein nachträglich hinzugefügtes Gerät ohne Bestätigung andere Geräte verwalten darf (mindestens 3600) | `259200` (72 h) |
| `ULTRAOS_PASSWORD_FAILURES` | Fehlversuche des Kennworts je Konto **und Client-Adresse** (IPv6: /64) in 15 Minuten (Anmeldung, Kennwort ändern, Geräte-Ersatz zusammen; 1–1000). Danach wird diese Adresse für dieses Konto bis zum Ende des Zeitfensters abgewiesen. Je Konto insgesamt (alle Adressen): 10 Fehlversuche frei, danach wartet jede Prüfung 1–8 s, ab 200 wird abgewiesen – nicht für Anfragen mit dem Token eines verwaltenden Geräts. | `20` |
| `ULTRAOS_MAX_CONNECTIONS` | gleichzeitig offene Verbindungen des Prozesses (16–1000000). Ist die Grenze erreicht, wird für eine neue Verbindung die älteste **untätige** geschlossen (keine laufende Anfrage, keine laufende Antwort); sind alle tätig, wird die neue geschlossen. | `4096` |
| `ULTRAOS_MAX_CONNECTIONS_PER_PEER` | Verbindungen je Client-Adresse (IPv6: /64), gleiche Regel; Adressen aus `ULTRAOS_TRUSTED_PROXIES` sind ausgenommen (hinter Caddy kommt jede Verbindung von Caddy) | `64` |

Fehlerhafte Werte bei den Proxys und Grenzen werden im Protokoll gemeldet (`ignored_trusted_proxy`,
`ignored_limit`) und nicht übernommen.

## 3. systemd — `/etc/systemd/system/ultraos.service` (vollständig gehärtet in `deploy/ultraos.service`)
```ini
[Service]
Environment=ULTRAOS_BIND=127.0.0.1:8080
Environment=ULTRAOS_DB=/var/lib/ultraos/ultraos.sqlite
Environment=ULTRAOS_FILES=/var/lib/ultraos/files
EnvironmentFile=/etc/ultraos/secret.env
ExecStart=/usr/local/bin/ultraos-server
Restart=always
DynamicUser=yes
StateDirectory=ultraos
```
Protokoll: strukturierte Zeilen (`event=… key=value`) auf stderr → journald. Es werden **keine
Client-Adressen, Benutzernamen oder Suchbegriffe** protokolliert (nur Methode, Pfad ohne
Query, Klasse, Status).

## 4. TLS-Terminierung — Caddy (`deploy/Caddyfile`)
Caddy holt das Zertifikat automatisch (ACME) und begrenzt Kopfzeilen, Body-Größe und langsame
Verbindungen, bevor sie den Dienst erreichen. WebSockets (`/v1/signal`, `/v1/verbund/relais`)
werden durchgereicht.

**Wer schützt wovor – hinter Caddy:** Alle Verbindungen zum Dienst kommen von Caddy (vertrauter
Proxy, daher ohne Grenze je Adresse). Langsame und stumme Clients enden an Caddy: `read_header 10s`,
`read_body 30s`, `idle 2m`, Kopf ≤ 16 KB, Rumpf ≤ 64 KB. `max_conns_per_host 2048` + `1024` im
`reverse_proxy` halten Caddys Verbindungen zum Dienst unter `ULTRAOS_MAX_CONNECTIONS` (4096). Die
Anzahl gleichzeitiger Verbindungen **eines** Clients zu Caddy begrenzt Caddy selbst nicht (dafür
bräuchte es ein Zusatzmodul oder die Firewall, z. B. `nft … ct count over 64`); die
Anfragegrenzen je Client-Adresse wirken im Dienst über `X-Forwarded-For`. Die Beispiel-Dateien
`deploy/Caddyfile` und `deploy/Caddyfile.docker` werden vom Test `tests/deploy_config.rs` auf diese
Werte geprüft (Datei-Prüfung; ein echter Caddy läuft im Test nicht). Ohne Proxy gelten die
Grenzen des Dienstes selbst (§ „Feste Grenzen“ und die Tabelle oben).

**Client-Adresse für die Anfragegrenzen** – wichtig, sonst teilen sich alle Nutzer ein Kontingent:
- Caddy direkt am Internet: Standard passt (Caddy setzt `X-Forwarded-For`, der Dienst vertraut
  nur `127.0.0.1`/`::1`).
- Docker: Caddy hat eine Adresse aus dem Docker-Netz → in `docker-compose.yml` ist
  `ULTRAOS_TRUSTED_PROXIES` auf die privaten Netze gesetzt (der Dienst hat keinen veröffentlichten Port).
- **Cloudflare (oder ein anderes CDN) vor Caddy:** dann sieht Caddy nur die CDN-Adresse. In Caddy
  `header_up X-Forwarded-For {http.request.header.CF-Connecting-IP}` im `reverse_proxy` setzen
  **und** den Ursprung per Firewall nur für die CDN-Netze öffnen (sonst kann jeder den Kopf fälschen).
  Der Dienst wertet immer nur den **letzten** Eintrag von `X-Forwarded-For` aus.

## 5. TURN/STUN — coturn `/etc/turnserver.conf`
```
listening-port=3478
tls-listening-port=5349
use-auth-secret
static-auth-secret=<derselbe Wert wie ULTRAOS_TURN_SECRET>
realm=ultar.code-craft.tech
# nichts protokollieren: keine Adressen, keine Benutzer, keine Sitzungen
log-file=/dev/null
no-stdout-log
```
(Server gibt kurzlebige Zugangsdaten über `/v1/turn-cred` aus; der Benutzer ist `<Ablauf>:<Zufall>`,
nie das Konto. Im Container zusätzlich `logging: driver: none`.)

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

## 7. Store und Updates ohne Konto (Entscheidung vom 22.11.2026)
| Was | Zugang |
|---|---|
| Katalog, Suche, Kategorien, App-Seite | offen |
| Download **kostenloser** Pakete | offen |
| `update/latest` + Abbild der **offenen** Kanäle (`stable`, `beta`) | offen |
| **Premium**-Pakete | Token **und** Zugang `app:<Kennung>` |
| **Geschützte** Kanäle (`preview`, `lts`) | Token **und** Zugang `channel:<Name>` |
| Rufnummer, Signalisierung, TURN, VPN, Verbund | Konto (wie bisher) |

Der Server ist **nie** der Vertrauensanker: Pakete und Abbilder sind vom Herausgeber signiert und
werden am Gerät geprüft (`ultar_pkg`, dm-verity-Root-Hash + Signatur). Ein kompromittierter Server
kann nichts Unsigniertes einschleusen; er kann höchstens Dateien vorenthalten oder ältere, echt
signierte Versionen anbieten (siehe INTEGRATION.md, Bedrohungsbetrachtung).

## 8. Datenbank-Migration (vorhandene Installation)
Beim Start wird die Datei **an Ort und Stelle** auf das aktuelle Schema gebracht (`PRAGMA user_version`,
eine Transaktion; bricht sie ab, bleibt das alte Schema unverändert). Vorhandene Konten, Geräte,
Nummern, VPN-Peers, Store-Einträge und Releases bleiben erhalten:
- alte Store-Einträge sind kostenlos, Kategorie `other`, ihre Datei bleibt am bisherigen Ort (`url`) –
  der Download-Endpunkt leitet dorthin um (307);
- alte Releases gelten für `arch=any`, `edition=any`; Kanäle, die schon Releases hatten, bleiben offen;
- neue Kanäle `preview` und `lts` werden geschützt angelegt.

**Vor dem Update sichern** (ein Zurück auf das alte Binary ist nach der Migration nicht vorgesehen):
```bash
systemctl stop ultraos
sqlite3 /var/lib/ultraos/ultraos.sqlite ".backup '/root/ultraos-vor-update.sqlite'"
./install.sh
```
Eine Datenbank eines **neueren** Servers wird nicht angefasst (Start bricht mit Meldung ab).

## 9. Verwaltung: Veröffentlichen und Zugänge (`ultraos-server admin …`)
Es gibt **keine** Verwaltungs-Schnittstelle über HTTP. Befehle laufen auf dem Server und arbeiten
direkt auf Datenbank und Dateiablage (auch während der Dienst läuft). Mit systemd über den Wrapper
`ultraos-admin` (führt den Befehl als Benutzer des Dienstes aus), im Container mit
`docker compose exec ultraos ultraos-server admin …`. Ausgaben auf Arabisch/Englisch/Deutsch (`LANG`).

```bash
# App veröffentlichen (Paket vorher mit upkg-sign signieren)
cat > /srv/ultraos-incoming/sheets.json <<'EOF'
{"id":"com.kmcc.sheets","name":"KM Sheets","version":"1.1.0",
 "description":"Tabellen","category":"office","publisher":"KM Code Craft",
 "premium":false,"permissions":["files"],"file_types":["xlsx","csv"],
 "installed_size":600000,"changelog":"schneller"}
EOF
ultraos-admin publish-app /srv/ultraos-incoming/sheets.json /srv/ultraos-incoming/sheets.uap

ultraos-admin publisher "KM Code Craft" verified      # „geprüfter Herausgeber“
ultraos-admin premium com.kmcc.pro on|off             # nur für Pakete in der Dateiablage

# System-Abbild veröffentlichen
echo '{"channel":"stable","version":"0.7.0","arch":"aarch64","edition":"phone","roothash":"<hex>","sig":"<base64>"}' > rel.json
ultraos-admin publish-release rel.json ultar-0.7.0.img
ultraos-admin channel preview gated|open              # Kanal anlegen / schützen / öffnen

# Zugänge (keine Bezahllogik – ein Zugang existiert, weil er hier eingetragen wurde)
ultraos-admin grant  <benutzer> app com.kmcc.pro
ultraos-admin grant  <benutzer> channel preview
ultraos-admin revoke <benutzer> app com.kmcc.pro
ultraos-admin entitlements <benutzer>
```
Regeln: veröffentlichte Versionen sind unveränderlich (gleiche Version erneut → Fehler); unbekannte
Felder in der JSON-Beschreibung werden abgelehnt; jede Änderung steht in der Tabelle `audit`.
Der Server öffnet oder prüft das Paket nicht – Signaturprüfung ist Sache des Geräts.

## 10. Ultar-Verbund-Relais
Der Server sieht nur Chiffretext (Noise_XX zwischen den Geräten, Ende-zu-Ende). Er leitet nur innerhalb eines
Kontos weiter, begrenzt Puffer (512 Rahmen je Verbindung, 70 KB je Rahmen, 64 Verbindungen je Konto) und meldet
„unerreichbar“, statt zu stauen. Ein Neustart des Servers trennt nur – die Geräte verbinden sich selbst neu.

## 11. Schutz der öffentlichen Endpunkte (eingebaut)
- Anfragegrenzen je Client (Token-Bucket im Speicher, höchstens 50 000 Einträge; IPv6 je /64). Adressen stehen
  nur im Arbeitsspeicher und werden gelöscht, sobald das Kontingent wieder voll ist (spätestens Fenster + 10 s).
- Größen: Body ≤ 16 KiB, Anfrageziel ≤ 2048 Byte, Suchtext ≤ 100 Zeichen, Seite ≤ 100 Einträge.
- Downloads: nur über App-Kennung/Release-Nummer; Dateinamen stammen aus der Datenbank (SHA-256), nie vom Client.
- `ETag`/`If-None-Match` und `Range`/`If-Range` für Katalog und Dateien (Fortsetzen abgebrochener Downloads).

## 12. Vor Produktion noch zu tun (nicht erledigt, nicht behauptet)
- `ULTRAOS_SECRET` fest und geheim (sonst sind alle Token nach Neustart ungültig).
- TLS-Terminierung mit der neuen Caddy-Konfiguration auf dem Testserver prüfen (Zeitlimits, Body-Grenze,
  `Range`/`ETag` durch den Proxy, WebSockets).
- **Sicherungen:** tägliches `sqlite3 … ".backup"` + Dateiablage, verschlüsselt außer Haus; Rückspielen üben.
- **Überwachung:** Dienst-Zustand (`/v1/health`), 5xx- und 429-Rate aus dem Protokoll, Plattenplatz, Zertifikatsablauf.
- **Externer Penetrationstest** (Auth, Signalisierung, Relais, öffentliche Endpunkte). Der Code ist getestet,
  aber **nicht** extern geprüft oder auditiert.
- Signaturschlüssel (ed25519) für Store/OTA offline halten; nur öffentliche Schlüssel aufs Gerät.
- Bandbreiten-/Verbindungsgrenzen je Client am Proxy oder per Firewall (der Dienst zählt Anfragen, nicht Bytes).
- Lasttest mit realistischen Abbildgrößen.

## 13. App-Store: signierte Pakete (.uap)
Alle Apps sind signiert (Ed25519); das Gerät lehnt Unsigniertes ab. Ablauf:
1. Einmalig Herausgeber-Schlüssel erzeugen: `upkg-keygen` → privaten Teil geheim auf dem Signier-
   rechner halten, öffentlichen Teil ins Gerät-Image (`/etc/ultar/trusted-keys/kmcodecraft.ed25519`).
2. App paketieren + signieren: `upkg-sign <manifest.toml> <ordner> <app.uap> <privkey-datei>`.
3. Mit `ultraos-admin publish-app` veröffentlichen (§9). Format-Details: docs/A30-APP-PAKET.md.

## Feste Grenzen des Prozesses (ohne Einstellung)

- Der Kopf einer Anfrage muss binnen 10 s vollständig sein, sonst wird die Verbindung geschlossen.
- Eine Anfrage (Rumpf und Bearbeitung) muss binnen 30 s beantwortet sein, sonst `408 request_timeout`
  und die Verbindung wird geschlossen. WebSockets und Downloads sind davon nicht betroffen.
- Diese Grenzen gelten unabhängig vom vorgeschalteten Proxy; dessen Zeitgrenzen sollten nicht
  größer sein (Beispiel-Caddyfile: `read_header 10s`, `read_body 30s`).
- `ULTRAOS_LIMIT_AUTH` gilt jetzt auch für `/v1/account/password` und `/v1/device/recover/start`.
