# Integration: Web-Apps (WhatsApp/YouTube) + Spotify

Neue Dateien (nichts Geteiltes angefasst):
- `rootfs/usr/lib/ultar/webapp.sh` – Web-App-Starter mit eigenem Profil je App.
- `rootfs/usr/lib/ultar/librespot.sh` – Spotify-Connect-Starter (librespot).
- `platform/ultar-shell/src/app_spotify.rs` – Spotify-Screen (Submodul von `app`, host-`cargo check` grün: 0 Fehler).
- `docs/A30-WEBAPPS.md`.

## 1. main.rs – Web-Apps mit eigenem Profil
In `fn web_session(...)` den Starter auf `webapp.sh` umstellen (gibt jeder App ein eigenes, dauerhaftes
Login-Profil; Slug wird aus der URL-Domain abgeleitet):
```rust
// vorher: let _ = std::fs::copy("/usr/lib/ultar/web-session.sh", script);
let _ = std::fs::copy("/usr/lib/ultar/webapp.sh", script);
```
(web-session.sh kann als Rückfall bleiben.) WhatsApp/YouTube/Telegram-Kacheln existieren bereits als
`Screen::Web(url)` in `s_home` und profitieren sofort.

## 2. app.rs – Spotify-App
`#[path = "app_camera.rs"] mod app_camera;` → danach:
```rust
#[path = "app_spotify.rs"]
mod app_spotify;
```
`struct App`: nach `cam: app_camera::CameraState,`:
```rust
    spotify: app_spotify::SpotifyState,
```
`App::new`: nach `cam: Default::default(),`:
```rust
            spotify: Default::default(),
```
`enum Screen` (nach `Camera,`):
```rust
    Spotify,
```
Dispatch (nach `Screen::Camera => self.s_camera(ui, area),`):
```rust
            Screen::Spotify => self.s_spotify(ui, area, &mut sc),
```
Home-Kachel in `s_home` (neben WhatsApp/YouTube; Array-Länge +1 anpassen):
```rust
            (Ic::Globe, "Spotify", Some(Screen::Spotify)),
```
Routing-String (bei `"camera" => Screen::Camera,`):
```rust
            "spotify" => Screen::Spotify,
```

## 3. i18n.rs – Zeichenketten (en, de, ar)
```rust
    ("Spotify", "Spotify", "سبوتيفاي"),
    ("Web player", "Web-Player", "مشغّل الويب"),
    ("Opens open.spotify.com in the browser. Playback needs Widevine.", "Öffnet open.spotify.com im Browser. Wiedergabe braucht Widevine.", "يفتح open.spotify.com في المتصفّح. التشغيل يحتاج Widevine."),
    ("Open in browser", "Im Browser öffnen", "فتح في المتصفّح"),
    ("Spotify Connect (librespot)", "Spotify Connect (librespot)", "سبوتيفاي كونكت (librespot)"),
    ("librespot is not installed yet (see docs).", "librespot ist noch nicht installiert (siehe Doku).", "librespot غير مثبّت بعد (راجع المستندات)."),
    ("Runs as the Connect device \"Ultar\". Needs Spotify Premium. Control it from the Spotify app or the web player.", "Läuft als Connect-Gerät \"Ultar\". Braucht Spotify Premium. Steuerung über die Spotify-App oder den Web-Player.", "يعمل كجهاز Connect باسم \"Ultar\". يحتاج سبوتيفاي بريميوم. تحكّم من تطبيق سبوتيفاي أو مشغّل الويب."),
    ("Start Connect device", "Connect-Gerät starten", "تشغيل جهاز Connect"),
    ("Stop Connect device", "Connect-Gerät stoppen", "إيقاف جهاز Connect"),
    ("Connect device is running", "Connect-Gerät läuft", "جهاز Connect يعمل"),
    ("Connect device is off", "Connect-Gerät aus", "جهاز Connect متوقّف"),
    ("Starting…", "Startet…", "جارٍ التشغيل…"),
    ("Stopped", "Gestoppt", "متوقّف"),
```
("Turned off in privacy settings" ist bereits vorhanden.)

## 4. Flashen
`webapp.sh` und `librespot.sh` liegen in `rootfs/usr/lib/ultar/` und kommen beim nächsten Image-Bau
automatisch nach `/usr/lib/ultar/`. librespot-Binary separat bauen (docs/A30-WEBAPPS.md) und nach
`/usr/lib/ultar/librespot` legen.
