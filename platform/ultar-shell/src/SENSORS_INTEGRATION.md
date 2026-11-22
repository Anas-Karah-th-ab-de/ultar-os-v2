# Integration: Sensoren-Bildschirm

`sensors.rs` ist ein Untermodul von `app` (wie `monitor.rs`). Es nutzt nur die
**öffentlichen/ancestor-privaten** App-Helfer `header`/`caption` und `ui.row` –
KEINE privaten Methoden anderer Module. Reine sysfs-Lesung, keine Panics.

## app.rs

**1) Modul einbinden** – direkt nach den bestehenden `mod monitor;`-Zeilen (Z. ~17):
```rust
#[path = "sensors.rs"]
mod sensors;
```

**2) Screen-Variante** – im `pub enum Screen { … }` eine Zeile ergänzen (z. B. nach `Sound,`):
```rust
    Sensors,
```

**3) Render-Dispatch** – im großen `match` (dort wo `Screen::Monitor => self.s_monitor(…)` steht):
```rust
            Screen::Sensors => self.s_sensors(ui, area, &mut sc),
```

**4) Laufende Aktualisierung** – in `needs_tick()` die Monitor-Zeile erweitern, damit die
Werte live aktualisieren:
```rust
            || matches!(self.stack.last().map(|s| &s.0), Some(Screen::Monitor | Screen::Sensors))
```
(oder separat `|| self.sensors_open()`)

**5) Eintrag in den Einstellungen** – in `s_settings` das Array vergrößern:
`let items: [(Ic, &'static str, String, Screen); 11]` → `; 12`, und eine Zeile ergänzen
(z. B. direkt nach dem „Hardware & drivers"-Eintrag):
```rust
            (Ic::Chip, "Sensors", String::new(), Screen::Hardware), // -> ersetzen durch Screen::Sensors
```
genauer:
```rust
            (Ic::Chip, "Sensors", String::new(), Screen::Sensors),
```

**6) (optional) String-Navigation** – falls `go("sensors")` genutzt werden soll, im
String→Screen-`match` ergänzen:
```rust
            "sensors" => Screen::Sensors,
```

## main.rs
Keine Änderung nötig (sensors ist Untermodul von `app`, nicht Top-Level).

## i18n.rs
Neue Übersetzungsschlüssel hinzufügen (EN, DE, AR). Vorher prüfen, ob „Light"/„on"/„off"
schon existieren – dann NICHT doppeln:
```rust
    ("Sensors", "Sensoren", "المستشعرات"),
    ("sensors detected", "Sensoren erkannt", "مستشعرات مكتشفة"),
    ("No sensors found", "Keine Sensoren gefunden", "لم يُعثر على مستشعرات"),
    ("Acceleration", "Beschleunigung", "التسارع"),
    ("Gyroscope", "Gyroskop", "الجيروسكوب"),
    ("Magnetic field", "Magnetfeld", "المجال المغناطيسي"),
    ("Light", "Licht", "الضوء"),
    ("Proximity", "Näherung", "القرب"),
    ("Pressure", "Druck", "الضغط"),
    ("Steps", "Schritte", "الخطوات"),
    ("Tilt", "Neigung", "الميل"),
    ("Grip", "Griff", "القبضة"),
    ("Motion", "Bewegung", "الحركة"),
    ("Sensor", "Sensor", "مستشعر"),
    ("on", "an", "مشغّل"),
    ("off", "aus", "متوقّف"),
```

## Verifiziert
- `sensors.rs` kompiliert mit obiger Integration statisch (aarch64, build-std, +crt-static),
  qemu-aarch64-static startet ohne Absturz.
- Gerätetest später: `senstest` (tools/senstest.c, Binary unter /mnt/user-data/outputs/senstest)
  über die Tür pushen und laufen lassen → zeigt die echten sysfs-Attributnamen/-Formate.
  Falls Samsungs `raw_data`-Dateien anders heißen oder die Werte erst nach `enable=1` kommen,
  die Kandidatenlisten in `read_sensor_values()` in sensors.rs entsprechend anpassen.
