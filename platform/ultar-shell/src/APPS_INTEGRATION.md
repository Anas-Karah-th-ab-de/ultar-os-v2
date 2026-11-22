# Integration der Kern-Apps (Telefon, Nachrichten, Kamera)

Neue, eigenständige Module (Untermodule von `app`, wie `tools.rs`/`monitor.rs`):
`app_phone.rs`, `app_messages.rs`, `app_camera.rs`.

Verifiziert mit `cargo check -p ultar-shell` (Host) in einer Wegwerf-Kopie: kompiliert sauber,
**0 Warnungen/Fehler in diesen drei Dateien**. Unten die exakten Einfügungen – alle in `app.rs`,
plus i18n und Startbildschirm-Kacheln. (Ich habe app.rs/main.rs/i18n.rs/apps.rs NICHT angefasst.)

## 1. `app.rs` – Modul-Deklarationen
Direkt nach `#[path = "monitor.rs"] mod monitor;` einfügen:
```rust
#[path = "app_phone.rs"]
mod app_phone;
#[path = "app_messages.rs"]
mod app_messages;
#[path = "app_camera.rs"]
mod app_camera;
```

## 2. `app.rs` – Felder in `struct App`
Nach `mon: monitor::Mon,` einfügen:
```rust
    phone: app_phone::PhoneState,
    msg: app_messages::MessagesState,
    cam: app_camera::CameraState,
```

## 3. `app.rs` – Initialisierung in `App::new` (im `Self { … }`)
Nach `mon: monitor::Mon::default(),` einfügen:
```rust
            phone: Default::default(),
            msg: Default::default(),
            cam: Default::default(),
```

## 4. `app.rs` – `enum Screen` (neue Varianten)
Vor der schließenden Klammer von `pub enum Screen { … }` (nach `Web(String),`) einfügen:
```rust
    Phone,
    Messages,
    MessageNew,
    Camera,
```

## 5. `app.rs` – Dispatch (im großen `match scr { … }`)
Nach `Screen::Web(u) => self.s_web(ui, area, u),` einfügen:
```rust
            Screen::Phone => self.s_phone(ui, area),
            Screen::Messages => self.s_messages(ui, area, &mut sc),
            Screen::MessageNew => self.s_message_new(ui, area),
            Screen::Camera => self.s_camera(ui, area),
```

## 6. `app.rs` – Startbildschirm-Kacheln (in `s_home`, Liste `(Ic, "Name", Some(Screen::…))`)
Z. B. direkt vor `(Ic::Calc, "Calculator", …)` einfügen (eigene Icons optional; hier vorhandene genutzt):
```rust
            (Ic::Bot, "Phone", Some(Screen::Phone)),
            (Ic::Term, "Messages", Some(Screen::Messages)),
            (Ic::Image, "Camera", Some(Screen::Camera)),
```
Schöner wären eigene Icons `Ic::Phone`, `Ic::Message`, `Ic::Camera` (ui.rs Ic-Enum + `icon()`-Zeichnung) – optional.

## 7. `app.rs` – Routen-Namen (im `match s { "calc" => Screen::Calc, … }`)
Einfügen:
```rust
            "phone" => Screen::Phone,
            "messages" => Screen::Messages,
            "camera" => Screen::Camera,
```

## 8. `i18n.rs` – Zeichenketten (en, de, ar)
Fehlende Schlüssel in die `T`-Tabelle aufnehmen (vorhandene wie "Back","Send","Camera","Number","Message" nicht doppeln):
```rust
    ("Phone", "Telefon", "الهاتف"),
    ("Messages", "Nachrichten", "الرسائل"),
    ("New message", "Neue Nachricht", "رسالة جديدة"),
    ("No messages", "Keine Nachrichten", "لا رسائل"),
    ("Keypad", "Tastenfeld", "لوحة الأرقام"),
    ("Recents", "Verlauf", "السجل"),
    ("Call", "Anrufen", "اتصال"),
    ("End call", "Auflegen", "إنهاء"),
    ("In call", "Im Gespräch", "في مكالمة"),
    ("Dialing…", "Wählt…", "جارٍ الاتصال…"),
    ("Incoming call", "Eingehender Anruf", "مكالمة واردة"),
    ("Incoming", "Eingehend", "وارد"),
    ("Outgoing", "Ausgehend", "صادر"),
    ("Missed", "Verpasst", "فائتة"),
    ("Answer", "Annehmen", "ردّ"),
    ("Decline", "Ablehnen", "رفض"),
    ("No recent calls", "Keine Anrufe", "لا مكالمات"),
    ("No mobile network yet (modem/SIM not active)", "Noch kein Mobilfunk (Modem/SIM nicht aktiv)", "لا شبكة خلوية بعد (المودم/الشريحة غير مفعّل)"),
    ("Enter number and message", "Nummer und Nachricht eingeben", "أدخل الرقم والرسالة"),
    ("Camera off — tap Start", "Kamera aus – Start antippen", "الكاميرا متوقفة — اضغط ابدأ"),
    ("Starting camera…", "Kamera startet…", "تشغيل الكاميرا…"),
    ("No camera found", "Keine Kamera gefunden", "لم يُعثر على كاميرا"),
    ("Capture", "Auslösen", "التقاط"),
    ("Photo saved", "Foto gespeichert", "تم حفظ الصورة"),
    ("Saved:", "Gespeichert:", "محفوظ:"),
    ("Start", "Start", "ابدأ"),
    ("Stop", "Stopp", "إيقاف"),
    ("Front", "Front", "أمامية"),
    ("Back", "Zurück", "رجوع"),
```
(Bereits vorhanden und wiederverwendet: "Back", "Send", "Camera", "Number", "Message".)

## Hinweise
- **Telefonie/SMS** laufen über `trait TelephonyBackend` / `trait SmsBackend` mit Stub-Backend
  (kein echtes Netz: `ready()` = false, Oberfläche zeigt den Hinweis). Sobald `crate::ultar-modem`
  einen RIL liefert, nur ein echtes Backend implementieren und in `PhoneState::default` /
  `MessagesState::default` einsetzen – die Oberfläche bleibt unverändert.
- **Kamera**: `app_camera.rs` nutzt `crate::camera`. `Camera::open()`/`find_capture_node()` können
  auf Exynos beim Öffnen blockieren (ISP-Pipeline). Darum wird nur auf Antippen geöffnet und
  bevorzugt ein in `/data/ultar/camera.node` hinterlegter, bekannter Knoten benutzt. Für robusten
  Dauerbetrieb das Öffnen/Capturen in einen Thread mit Timeout verlagern (gehört ins camera-Modul).
  Fotos landen in `/data/ultar/media` → erscheinen in der vorhandenen Galerie (MEDIA_DIRS).
  Optional: nach dem Auslösen `self.tl.pics_scanned = false;` setzen, damit die Galerie neu scannt.
