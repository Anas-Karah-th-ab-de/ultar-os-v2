# A30 – Sensoren

Am Samsung A30 (Exynos 7904, Samsung-Sensorhub) live gefunden:

- `/sys/class/sensors/` mit: `accelerometer_sensor`, `gyro_sensor`, `light_sensor`,
  `proximity_sensor`, `magnetic_sensor`, `grip_sensor`, `step_cnt_sensor`,
  `step_det_sensor`, `tilt_sensor`, `SignificantMotionDetector`, `ssc_core` (Steuerknoten).
- `/sys/bus/iio/devices/iio:device0`.

## Umsetzung in Ultar
`crates/ultar-shell/src/sensors.rs` (Untermodul von `app`):
- `read_all() -> Vec<SensorReading>` – erfasst zuerst `/sys/class/sensors/*`
  (liest `name`, `enable` und je nach Typ `raw_data`/`lux`/`state`/`step_cnt` …),
  dann generisch IIO-Kanäle (`in_accel_*_raw`, `in_anglvel_*_raw`, `in_magn_*_raw`,
  `in_illuminance_input`, `in_proximity_raw`, `in_pressure_input`). Robust gegen
  fehlende Dateien, keine Panics. Dedupliziert gleiche Namen.
- `s_sensors(ui, area, sc)` – Einstellungen → „Sensoren": zeigt je Sensor eine Zeile
  mit Typ-Name, Rohname und Live-Werten (Beschleunigung/Gyro/Magnet als x·y·z,
  Licht in lux, Näherung nah/fern, Schritte als Zähler, an/aus-Status).
- `sensors_open()` – für laufende Aktualisierung in `needs_tick`.

Integration: siehe `crates/ultar-shell/src/SENSORS_INTEGRATION.md`.

## Gerätetest (über die Tür)
`tools/senstest.c` (statisches C-Tool, Binary unter `/mnt/user-data/outputs/senstest`):
1. `senstest` nach `/data/senstest` pushen, `chmod 755`.
2. `/data/senstest` ausführen → listet je Sensor die vorhandenen Dateien und die Werte der
   üblichen Attribute. Damit sieht man die **echten** Dateinamen/Formate des A30.
3. Abgleich mit `read_sensor_values()` in sensors.rs: falls Samsung andere Attribute nutzt
   (z. B. `raw_data` liefert nichts, Werte erst nach `echo 1 > enable`), die Kandidatenlisten
   bzw. die enable-Logik anpassen.

## Hinweis
Samsung-Sensoren liefern „raw_data" oft erst nach `enable=1`; einige Werte sind roh (ADC)
und bräuchten den Scale-Faktor (`in_*_scale`) für physikalische Einheiten. Die Rohanzeige
reicht für den Funktionstest „Sensor liefert sich ändernde Werte"; die Skalierung kann
nach dem Gerätetest ergänzt werden (dann sind die Einheiten-Strings in sensors.rs anzupassen).
