# GNSS-Integration (für den Koordinator)

Neue Datei: `platform/ultar-shell/src/gnss.rs` (fertig, host-check grün). Kein eigenes Crate nötig –
reines Shell-Modul. Einzufügen in die zentral verwalteten Dateien:

## main.rs
Bei den Modul-Deklarationen (z. B. nach `mod gfx;`):
```rust
mod gnss;
```

## app.rs
1. Screen-Enum um eine Variante erweitern:
```rust
    Gnss,
```
2. App-Struct um den GNSS-Zustand erweitern (hält Fix/Satelliten zwischen Frames):
```rust
    gnss: crate::gnss::GnssState,
```
   und in `App::new(...)` initialisieren:
```rust
    gnss: crate::gnss::GnssState::default(),
```
3. Beim Betreten/Tick von `Screen::Gnss` einlesen (nichtblockierend). In der `enter`-Logik
   (wie `Screen::Hardware => …`) bzw. im Tick:
```rust
    Screen::Gnss => { let _ = crate::gnss::read_fix(&mut self.gnss); }
```
4. Routing-String (wie "storage" => Screen::Storage):
```rust
    "gnss" => Screen::Gnss,
```
5. Navigationszeile in den Einstellungen (bei den anderen `ui.row(..., Screen::...)`-Einträgen,
   z. B. neben Netzwerk):
```rust
    (Ic::Globe, "Location (GPS)", String::new(), Screen::Gnss),
```
6. Render-Zweig (bei `Screen::X => self.s_x(...)`):
```rust
    Screen::Gnss => self.s_gnss(ui, area, &mut sc),
```
7. Screen-Funktion hinzufügen (ruft das Modul):
```rust
    fn s_gnss(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, &self.t("Location (GPS)"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let _ = crate::gnss::read_fix(&mut self.gnss);
        let lang = self.set.lang.clone();
        let st = self.gnss.clone();
        crate::gnss::render_gnss(ui, &mut y, &st, |k| crate::i18n::tr(&lang, k).to_string());
        ui.scroll_end(area, y + *sc - area.y, sc);
    }
```
   (Signaturen von header/scroll_begin/scroll_end an die vorhandenen s_*-Funktionen anpassen;
   `self.t(..)` liefert `&str`, hier wird `tr(lang,k)` direkt genutzt, weil render_gnss eine
   `Fn(&'static str)->String` erwartet.)

## i18n.rs (T-Tabelle, de/ar ergänzen)
```rust
    ("Location (GPS)", "Standort (GPS)", "الموقع (GPS)"),
    ("Position", "Position", "الموقع"),
    ("Fix", "Fix", "التثبيت"),
    ("No fix", "Kein Fix", "لا تثبيت"),
    ("GPS fix", "GPS-Fix", "تثبيت GPS"),
    ("DGPS fix", "DGPS-Fix", "تثبيت DGPS"),
    ("Latitude", "Breite", "خط العرض"),
    ("Longitude", "Länge", "خط الطول"),
    ("Altitude", "Höhe", "الارتفاع"),
    ("Accuracy (HDOP)", "Genauigkeit (HDOP)", "الدقة (HDOP)"),
    ("Time (UTC)", "Zeit (UTC)", "الوقت (UTC)"),
    ("Satellites", "Satelliten", "الأقمار"),
    ("Waiting for satellites…", "Warte auf Satelliten…", "بانتظار الأقمار…"),
    ("No satellites in view", "Keine Satelliten in Sicht", "لا أقمار مرئية"),
```

Keine Cargo.toml-Änderung nötig (gnss.rs nutzt nur std + ein extern `ioctl`).
`render_gnss` nutzt nur öffentliche ui-Methoden (section/row/label/cv.rrect) und gfx::Rect/rgb.
