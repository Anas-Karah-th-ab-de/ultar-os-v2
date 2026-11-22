# WhatsApp / YouTube / Spotify als Apps (offizielle Wege)

Grundsatz: **keine Protokoll-Nachbauten, keine DRM-Umgehung.** Nachgebaute WhatsApp-Clients
werden gesperrt (Konto weg); DRM-Umgehung bricht bei jedem Update und ist rechtlich heikel.
Darum offizielle Clients/Player – stabil und sicher.

## Web-App-Framework (neu): `rootfs/usr/lib/ultar/webapp.sh`
Wie `web-session.sh`, aber mit **eigenem, dauerhaftem Profil je App** unter `/data/ultar/webapps/<slug>`.
Der Slug wird aus der Domain abgeleitet (web.whatsapp.com→whatsapp, m.youtube.com→youtube,
open.spotify.com→spotify, web.telegram.org→telegram). Dadurch bleiben die Logins **getrennt und erhalten**,
statt ein gemeinsames Browserprofil. Epiphany läuft im `--application-mode` (App-Auftritt, Vollbild über
Weston), mobiler oder Desktop-User-Agent je App.

Voraussetzung: der Ubuntu-Web-Bereich (weston + epiphany) muss vorhanden sein – wie beim bestehenden
Browser (noch nicht standardmäßig geflasht; `sys::compat_ready()`/`s_web` prüfen das bereits).

### Je App
- **WhatsApp** → `https://web.whatsapp.com` – offizieller, Ende-zu-Ende-verschlüsselter Web-Client.
  Login per QR-Code mit dem Handy. Desktop-UA (WhatsApp Web verweigert mobile UAs).
- **YouTube** → `https://m.youtube.com` – offizielle mobile Seite. Mobiler UA.
- **Spotify (Web)** → `https://open.spotify.com` – offizieller Web-Player. **Braucht Widevine (EME)**;
  WebKitGTK/Epiphany hat das oft nicht → Wiedergabe kann ausbleiben. Dann librespot (siehe unten).

## Spotify nativ: librespot (Connect-Gerät)
`librespot` ist ein offener Spotify-Client (Rust, [librespot-org/librespot](https://github.com/librespot-org/librespot),
crates.io 0.7.x). Läuft als **Spotify-Connect-Gerät „Ultar"**; gesteuert aus der offiziellen Spotify-App
(selbes WLAN, Zeroconf – **kein Passwort** im Gerät) oder dem Web-Player. **Braucht Spotify Premium.**

Starter (neu): `rootfs/usr/lib/ultar/librespot.sh` (erwartet Binary `/usr/lib/ultar/librespot`).
UI (neu): `crates/ultar-shell/src/app_spotify.rs` – Web-Player öffnen, Connect-Gerät starten/stoppen, Status.

### librespot für aarch64 statisch bauen (Build-Rezept, nicht im Repo gebaut – groß + Netz nötig)
```
# im Sandbox/Cross-Umfeld, aarch64, statisch:
git clone https://github.com/librespot-org/librespot
cd librespot
# minimal: nur ALSA-Backend, kein Audio-Cache-Zwang
cargo build --release --target aarch64-unknown-linux-gnu \
  --no-default-features --features "alsa-backend" \
  # statisch wie der Rest von Ultar:
  #   RUSTFLAGS="-C target-feature=+crt-static"  (braucht statisches alsa-lib; sonst dynamisch + libasound mitliefern)
```
Hinweis: Das ALSA-Backend verlangt `libasound`. Für ein voll statisches Binary entweder `libasound` statisch
einbinden oder librespot dynamisch bauen und `libasound.so` nach /usr/lib/ultar mitliefern. Audio funktioniert
erst, wenn der A30-Ton läuft (ABOX-Calliope-Firmware + SMA1301 „Power Up" – siehe Audio-Paket).
Ergebnis-Binary → `/usr/lib/ultar/librespot`.

## Telegram
Separat als **nativer Client (grammers/MTProto, offizielle API)** umgesetzt – siehe docs/A30-TELEGRAM.md
(eigenes Paket). Alternativ bis dahin Web über webapp.sh (`https://web.telegram.org/a/`).

## Grenzen (ehrlich)
- WhatsApp: nur offizielles Web. Ein eigener WhatsApp-Protokoll-Client wird NICHT gebaut (Sperrgefahr).
- YouTube/Spotify: kein DRM-Bypass, kein Ad-Stripping über das offizielle Angebot hinaus (der System-Adblock
  im Browser bleibt davon unberührt).
- Alles Web-basierte braucht den Ubuntu-Web-Bereich (weston/epiphany). librespot und Spotify-Web brauchen
  funktionierenden Ton.
