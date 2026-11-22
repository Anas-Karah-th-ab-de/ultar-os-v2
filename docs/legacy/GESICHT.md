# Gesichtsentsperrung (Stand 20.11.2026)

Bequemlichkeits-Entsperrung wie früher bei Samsung (nur Frontkamera, kein 3D/Infrarot): **weniger sicher als
PIN oder Fingerabdruck** – ein Foto oder eine ähnliche Person kann täuschen. Darum nur zusätzlich zu einer PIN;
die PIN geht immer. Alles läuft offline auf dem Gerät.

## Aufbau
- **`crates/ultar-face`** (Rust, `tract-onnx`, statisch):
  - YuNet (OpenCV-Zoo, MIT) findet Gesichter + 5 Punkte (Eingabe 640×640, Bild eingepasst, BGR 0..255).
  - Ausrichten per Ähnlichkeitstransformation (Umeyama) auf die ArcFace-Vorlage 112×112.
  - SFace (OpenCV-Zoo, Apache-2.0) → 128 Merkmale, normiert; gleiche Person ab Kosinus **0.42**
    (OpenCV empfiehlt 0.363).
  - Frontkamera über `/usr/bin/ultar-cam stream front` (768×576), Frontsensor um 270° gedreht – es werden bei
    Bedarf alle vier Lagen probiert.
  - Befehle: `anlernen [sek]` (6 passende Bilder → `/data/ultar/face/gesicht.bin`, Rechte 0600),
    `pruefen [sek]`, `loeschen`, `status`, Test: `bild a.ppm [b.ppm]`, `foto x.ppm`.
- **Modelle** `/usr/lib/ultar/face/` (im Repo `bins/a30/face/`): YuNet 0,23 MB, SFace als float16 19 MB
  (`tools/face/modell-f16.py`, Ergebnis gleichwertig).
- **ultar-shell** (`face.rs`, `app_sys.rs`): Einstellungen → Bildschirmsperre → „Gesichtserkennung":
  Hinweis zur Sicherheit, „Gesicht einrichten" mit Fortschritt, „Mit Gesicht entsperren" (nur mit PIN),
  „Gesichtsdaten löschen". Sperrbildschirm: einmal je Aufwachen automatisch 8 s prüfen, danach
  „Gesicht erneut versuchen" antippen.

## Messwerte A30 (Exynos 7904)
- Erkennen 240 ms, Merkmale 310 ms je Bild; Laden der Modelle ~1 s.
- Prüfung mit Testbildern: gleiche Person (gespiegelt, verkleinert, heller) 0.888 – andere Person 0.153
  (OpenCV selbst: 0.81 / 0.13).
- Frontkamera liefert 768×576.

## Offen
- Praxistest mit einem echten Gesicht am Gerät (Einrichten, dann Bildschirm aus/an).
- Lebenderkennung (Blinzeln/Kopfbewegung) gegen Fotos.
- Image-Bau: `tools/pc/build_image6.sh` kopiert `ultar-face` und die Modelle mit.
