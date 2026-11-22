# Ultar OS — Sicherheit, Cloud-Dienste & Netz-Relay (Produktions-Architektur)

Stand 19.11.2026. Drei Bausteine, vom Nutzer einzeln wählbar, standardmäßig **aus**:

1. **Geräteverschlüsselung + verifizierter Start** (Daten unlesbar ohne Passwort; System manipulationssicher).
2. **Ultar-Cloud** (`ultar.code-craft.tech`): eindeutige Rufnummer, Anrufe/Video zwischen Ultar-Geräten, App-Store, OTA-Updates.
3. **Netz-Relay / VPN**: gesamter Internetverkehr läuft über einen eigenen Server (nichts geht direkt raus), Login per Nutzername/Passwort.

Leitsatz: **keine selbstgebaute Kryptografie.** Es werden ausschließlich geprüfte Standard-Bausteine
orchestriert — LUKS2/dm-crypt, dm-verity, WireGuard, TLS 1.3 (rustls), Argon2id, WebRTC (DTLS-SRTP).
Das ist der einzige ehrliche Weg zu „militärisch sicher".

---

## 1. Geräteverschlüsselung + Verified Boot

### 1.1 Zwei Zonen
- **System/Root (read-only, verifiziert, NICHT verschlüsselt):** alles, was zum Start nötig ist
  (Kernel-Userspace, ultar-init, ultar-shell, Bibliotheken, rootfs). Wird bei **jedem Start per
  dm-verity gegen einen signierten Root-Hash geprüft**. Kein Schreibzugriff → kann nicht heimlich
  verändert werden; eine einzige geänderte Kachelbyte bricht die Prüfung.
- **Daten (/data, mmcblk0p32, read-write, verschlüsselt):** alles Persönliche. **LUKS2**,
  `aes-xts-plain64` 512-bit, Schlüssel aus Passwort per **Argon2id** (hohe Kosten). Ohne Passwort
  ist die Partition reiner Zufall — kein Weg ran, auch nicht per Chip-Auslesen.

### 1.2 Startkette (Vertrauenswurzel)
```
Stock-Bootloader → Stock-Kernel (vbmeta leer) → /init = ultar-stage0 (winzig, statisch)
   ├─ liest Root-Hash aus /ultar/verity.roothash (+ Signatur verity.sig, ed25519)
   ├─ richtet dm-verity über die System-Partition ein (device-mapper ioctls)
   │     Hash-Baum liegt als separate Partition/Anhang; Abgleich gegen Root-Hash
   ├─ Hash falsch  → Start verweigert, rote Meldung „System verändert — Start abgebrochen"
   ├─ Hash ok      → mountet /dev/mapper/ultar-root read-only, switch_root
   └─ exec /sbin/ultar-init  (der echte PID 1)
ultar-init:
   ├─ /data entschlüsseln:
   │     Verschlüsselung aus  → /data normal mounten
   │     Verschlüsselung an   → Passwort-Abfrage (ultar-shell Früh-UI) → cryptsetup luksOpen
   │                            → mount /dev/mapper/ultar-data /data
   └─ weiter wie bisher (Watchdog, Dienste, Shell)
```

Ehrlich: Bei **entsperrtem Bootloader + leerem vbmeta** gibt es keine Hardware-Vertrauenswurzel.
stage0 selbst ist die Wurzel und liegt in einem nicht-verifizierten Bereich. Verity ist damit
**manipulations-*evident*** (jede Änderung am System fällt auf und blockt den Start), aber ein
Angreifer mit vollem Flash-Zugriff könnte stage0 + Root-Hash gemeinsam tauschen. Der echte,
harte Schutz liegt deshalb in der **Datenverschlüsselung** (LUKS2): die Nutzerdaten bleiben ohne
Passwort unlesbar, egal was mit dem System passiert. Für einen echten HW-Root-of-Trust müsste der
Bootloader wieder gesperrt und ein eigener, signierter vbmeta/AVB-Schlüssel hinterlegt werden
(nur auf Geräten möglich, die eigene AVB-Keys erlauben — A30 nicht ab Werk).

### 1.3 Werkzeuge / Umsetzung
- `ultar-stage0` (neue statische Mini-Binary): dm-verity-Setup per `/dev/mapper/control`-ioctls
  (DM_DEV_CREATE/DM_TABLE_LOAD, target „verity"), switch_root, exec. Kein externes Tool nötig.
- `/data`-Krypto: statisches **cryptsetup** (LUKS2) in `rootfs/usr/lib/ultar/`; Rust-Manager
  `ultar-secure::udata` ruft es auf (format/open/close/change-key). Kein eigener Krypto-Code.
- Bau-Zeit (WSL): `mkverity.sh` erzeugt per `veritysetup format` den Hash-Baum + Root-Hash,
  signiert ihn (ed25519), legt beides ins Image. Einbindung in `UltarFlash/build_image*.sh`.
- Shell: Einstellungen → **Sicherheit**: Schalter „Geräteverschlüsselung", Passwort setzen/ändern,
  Status Verified Boot (Root-Hash, letzte Prüfung).

---

## 2. Ultar-Cloud — `ultar.code-craft.tech`

Ein Rust/Axum-Server (`server/ultraos-server/`). TLS 1.3 (rustls, Zertifikat via Caddy/ACME davor).
Speicher: SQLite (sqlx) — leicht auf Postgres umstellbar. Passwörter **Argon2id**. Sitzungen per
signiertem Token (PASETO v4 / HMAC). Rate-Limiting, Audit-Log.

### 2.1 Konto & eindeutige Nummer
- Registrierung: Nutzername + Passwort → Konto. Jedes Ultar-Gerät meldet beim ersten Start seine
  **Geräte-ID** (Hash aus Seriennummer + erzeugtem Schlüsselpaar) an `/v1/device/register`.
- Server vergibt eine **eindeutige Ultar-Nummer** im Format `UOS-XXXX-XXXX` (bzw. als E.164-artige
  interne Nummer `+999 ULTAR …`). Die Nummer ↔ Konto/Gerät-Zuordnung liegt in der Tabelle `numbers`.
  Kollisionsfrei per Sequenz + Prüfziffer. Die Nummer erscheint in Systeminfo und der Telefon-App.

### 2.2 Anrufe & Video (Gerät ↔ Gerät)
- **Signalisierung:** WebSocket `/v1/signal` (nach Token-Auth). Austausch von SDP-Offer/Answer +
  ICE-Kandidaten zwischen zwei Ultar-Nummern. Server ist nur Vermittler, sieht **keine** Medien.
- **Medien:** WebRTC, Ende-zu-Ende per **DTLS-SRTP** verschlüsselt. NAT-Durchstoß per **STUN**;
  wenn direkt nicht möglich, **TURN-Relay** (coturn auf demselben Server, Zugangsdaten kurzlebig
  per `/v1/turn-cred`). Audio Opus, Video VP8/H264.
- Gerät: `ultar-call` (webrtc-rs) spricht Signalisierung + Medien; Telefon-App bekommt einen
  Internet-Anruf-Pfad (klingeln, annehmen, auflegen, Video an/aus) neben dem späteren Mobilfunk.

### 2.3 App-Store
- `/v1/store/apps` (Liste: id, name, version, größe, signatur-hash, icon), `/v1/store/app/{id}`
  (signiertes Paket). Pakete **ed25519-signiert**; Gerät prüft Signatur vor Installation.
- Gerät: `ultar-cloud::store` + Store-Oberfläche (nutzt vorhandene Paket-/Install-Logik).
  [[universal-pos-erp]] u. a. hier einstellbar.

### 2.4 OTA-Updates
- `/v1/update/channel/{stable|beta}` → neueste Version + URL + Root-Hash + Signatur des neuen
  System-Images. Gerät (`ultar-cloud::update`) lädt, prüft Signatur **und** Verity-Root-Hash,
  schreibt in die inaktive System-Partition (A/B-Slot-Prinzip; sonst in `/data/ota` + Flagge),
  bootet neu. So bleibt die Kette aus Teil 1 (verifizierter Start) auch nach Update intakt.

### 2.5 Server-Deployment (Doku)
- Caddy (automatisches TLS für `ultar.code-craft.tech`) → reverse-proxy auf `ultraos-server`.
- coturn (STUN/TURN) auf 3478/5349, Zugangsdaten rotierend.
- WireGuard-Server (Teil 3) `wg0`, nftables-NAT.
- systemd-Units, Backups der SQLite-DB, fail2ban/Rate-Limit.

---

## 3. Netz-Relay / VPN — „nichts fliegt direkt raus"

- **WireGuard.** Der Server ist der einzige WG-Endpunkt; das Gerät bekommt pro Konto einen Peer
  (`AllowedIPs = 0.0.0.0/0, ::/0`) → **sämtlicher** Verkehr (auch DNS) läuft durch den Server ins
  Internet. Keine App kann daran vorbei (Killswitch, s. u.).
- **Provisionierung:** Gerät erzeugt lokal ein WG-Schlüsselpaar (privater Schlüssel verlässt das
  Gerät nie), sendet den **öffentlichen** Schlüssel + Konto-Login an `/v1/vpn/peer`; Server legt
  den Peer an und liefert Server-Pubkey, Endpunkt, zugewiesene interne IP, DNS. Login = Konto
  (Nutzername/Passwort) aus Teil 2.
- **Killswitch:** nftables-Default-Policy **drop**; erlaubt ist nur Verkehr über `wg0` (+ Handshake
  zum Server-Endpunkt). Bricht der Tunnel ab, geht nichts ungeschützt raus.
- **Gerät:** `ultar-vpn` — WG per Kernel-Modul falls vorhanden, sonst **boringtun** (geprüfter
  Rust-Userspace-WG über /dev/net/tun). bin `uvpn` (up/down/status). Einstellungen → **VPN**:
  Schalter, Serveradresse, Konto-Login, Status (verbunden, IP, Durchsatz).
- **Server:** siehe 2.5; `/v1/vpn/peer` provisioniert, `wg0` + NAT leiten weiter.

---

## 4. Zusammenspiel & Datenschutz
- Alle drei Teile hängen am **Datenschutz-/Sicherheits-Kontrollzentrum** (bestehend, `privacy.rs`):
  ein Hauptschalter deckt Netz/VPN/Cloud mit ab.
- Standard: alles **aus**. Der Nutzer entscheidet pro Funktion. Konto-Login ist für Cloud **und**
  VPN nötig (ein Konto).
- Kein Google/Android-Dienst, keine stillen Verbindungen. Die Android-Laufzeit (falls genutzt)
  bleibt isoliert und kann über denselben VPN/Killswitch gezwungen werden.

## 5. Offene Härtung (ehrlich, vor „produktionsreif")
- Sicherheits-Audit/Pentest der Server-Endpunkte + Signalisierung.
- A/B-Slots fürs System für ausfallsichere OTA (sonst Recovery-Pfad nötig).
- HW-Root-of-Trust nur mit gesperrtem Bootloader + eigenem AVB-Key (A30-Grenze, s. 1.2).
- Lasttest TURN/Signalisierung; DB-Backups + Monitoring produktiv aufsetzen.
- Alles auf echtem A30 testen (Gerät war offline) — Reihenfolge in A30-TESTPLAN.md ergänzt.
