# Telegram auf Ultar OS (A30) – nativer MTProto-Client

## Weg & Begründung
Telegram stellt sein Protokoll **MTProto** offiziell und öffentlich bereit; ein eigener Client ist
ausdrücklich vorgesehen (kein Reverse Engineering, kein Umgehen von Schutz). Gewählt: **grammers**
(reine-Rust-MTProto-Implementierung) statt TDLib (C++, sehr schwer statisch/cross für aarch64).

Vorteile grammers: kein openssl/kein C (eigene Krypto in Rust) → **statischer aarch64-Build mit
build-std funktioniert** (verifiziert, ~Minuten, Lib baut sauber). tokio nur mit `rt`/`time`.

## Architektur
- Crate `crates/ultar-telegram`: Trait `TgBackend` (login/chats/messages/send/pump), `MockTg`
  (ohne Netz, für UI) und `GrammersTg` (Feature `real`): läuft in einem eigenen Thread mit Tokio,
  tauscht über Kanäle + Arc<Mutex> Zustand mit der Oberfläche. Die synchrone Ultar-UI bleibt
  unblockiert.
- Modul `crates/ultar-shell/src/app_telegram.rs`: Login (Nummer → Code → ggf. 2FA), Chatliste,
  Chatansicht (Sprechblasen), Senden. Dunkel/Gold, RTL-fähig.

## Einrichtung (Nutzer)
1. Auf https://my.telegram.org → „API development tools" eine **api_id** (Zahl) + **api_hash**
   anlegen (kostenlos, einmalig).
2. Am Gerät: `/data/ultar/telegram.conf` — Zeile 1 = api_id, Zeile 2 = api_hash.
3. In der App: Telefonnummer (mit Landesvorwahl) → „Code anfordern" → Code aus der Telegram-App
   eingeben → fertig. Sitzung wird in `/data/ultar/telegram.session` gespeichert (kein erneuter Login).

## Gerätebau (mit echtem Netz)
`cargo build -p ultar-shell --features telegram-real --release --target aarch64-unknown-linux-gnu -Z build-std=std,panic_abort`
(RUSTC_BOOTSTRAP=1, RUSTFLAGS="-C target-feature=+crt-static"). Ohne das Feature → Mock.

## Gerätetest
1. ultar-shell mit `telegram-real` auf das Gerät (Hot-Deploy oder Flash).
2. `/data/ultar/telegram.conf` anlegen, WLAN aktiv.
3. Telegram-Kachel öffnen → anmelden → Chatliste sollte laden; Nachricht an „Gespeicherte
   Nachrichten" senden → erscheint.

## Offene Punkte / Grenzen
- **2FA-Passwort** (`check_password`) ist im Backend als TODO markiert (grammers-API je nach Version);
  für Konten ohne 2FA nicht nötig.
- Medien (Fotos/Sprachnachrichten) noch nicht; aktuell Text. Senden/Empfangen von Text + Chatliste stehen.
- Erststart lädt die TL-Schema-Typen (grammers-tl-gen, Build-Zeit) – nur Build, nicht Laufzeit.
- WhatsApp ist bewusst NICHT auf diesem Weg: WhatsApp hat keine offene Client-API; ein eigener
  WhatsApp-Client hieße inoffizielles Protokoll-Reverse-Engineering – anderer, rechtlich heikler
  Weg, hier nicht umgesetzt.
