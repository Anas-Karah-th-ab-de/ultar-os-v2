# Integration: Telegram-App (nativer MTProto-Client)

Neue Teile (nichts Gemeinsames angefasst):
- Crate `external/ultar-telegram/` (grammers-MTProto, Feature `real`; Standard = Mock).
- Modul `platform/ultar-shell/src/app_telegram.rs` (Untermodul von `app`).

Verifiziert: ultar-shell baut mit Mock (`cargo build -p ultar-shell`), und das echte Backend
baut **statisch für aarch64** (`cargo build --release --features real --target aarch64-unknown-linux-gnu -Z build-std=std,panic_abort`, RUSTFLAGS=+crt-static). grammers ist reines Rust (kein openssl/C).

## 1. Workspace-root `Cargo.toml`
`members`-Liste um `"external/ultar-telegram"` ergänzen.

## 2. `platform/ultar-shell/Cargo.toml`
```toml
[dependencies]
ultar-telegram = { path = "../ultar-telegram" }

[features]
# Für den Gerätebau einschalten -> echtes Telegram-Netz:
telegram-real = ["ultar-telegram/real"]
```
Gerätebau dann: `cargo build -p ultar-shell --features telegram-real --release --target aarch64-unknown-linux-gnu -Z build-std=std,panic_abort` (RUSTFLAGS=+crt-static). Ohne das Feature läuft der Mock.

## 3. `platform/ultar-shell/src/app.rs`
Nach `#[path = "monitor.rs"] mod monitor;`:
```rust
#[path = "app_telegram.rs"]
mod app_telegram;
```
In `struct App` nach `mon: monitor::Mon,`:
```rust
    tg: app_telegram::TelegramState,
```
In `App::new` (im `Self { … }`) nach `mon: monitor::Mon::default(),`:
```rust
            tg: Default::default(),
```
`enum Screen` (nach `Web(String),`):
```rust
    Telegram,
```
Dispatch (nach `Screen::Web(u) => self.s_web(ui, area, u),`):
```rust
            Screen::Telegram => self.s_telegram(ui, area),
```
Routing-`match` (bei "calc" => …):
```rust
            "telegram" => Screen::Telegram,
```
Startbildschirm-Kachel in `s_home` (es gibt schon eine Telegram-Web-Kachel – diese ersetzen
oder zusätzlich): `(Ic::Globe, "Telegram", Some(Screen::Telegram)),`. ACHTUNG: Die apps-Array-
Größe `[(Ic, &'static str, Option<Screen>); N]` um die Anzahl neuer Kacheln erhöhen.
`needs_tick()` um `Screen::Telegram` erweitern (damit Updates live reinkommen):
```rust
    || matches!(self.stack.last().map(|s| &s.0), Some(Screen::Telegram))
```

## 4. `i18n.rs` (en, de, ar) – fehlende Schlüssel
```rust
    ("Phone number (with country code)", "Telefonnummer (mit Landesvorwahl)", "رقم الهاتف (مع رمز الدولة)"),
    ("Request code", "Code anfordern", "طلب الرمز"),
    ("Enter the code", "Code eingeben", "أدخل الرمز"),
    ("Two-step password", "Zwei-Schritt-Passwort", "كلمة مرور بخطوتين"),
    ("Sign in", "Anmelden", "تسجيل الدخول"),
    ("No chats", "Keine Chats", "لا محادثات"),
    ("Set your Telegram api_id/api_hash (from my.telegram.org) in /data/ultar/telegram.conf", "Trage deine Telegram api_id/api_hash (von my.telegram.org) in /data/ultar/telegram.conf ein", "ضع api_id/api_hash الخاص بتيليجرام (من my.telegram.org) في /data/ultar/telegram.conf"),
```
(Bereits vorhanden/wiederverwendet: "Telegram","Back","Send","Message","Working…".)

## 5. Nutzer-Einrichtung am Gerät
`/data/ultar/telegram.conf`: Zeile 1 = api_id (Zahl), Zeile 2 = api_hash. Kostenlos auf
https://my.telegram.org (API development tools). Session wird in /data/ultar/telegram.session gespeichert.
