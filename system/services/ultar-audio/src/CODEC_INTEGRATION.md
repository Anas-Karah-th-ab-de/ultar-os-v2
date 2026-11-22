# Integration: Codec/Amp (Subpaket Audio-2)

Neue Dateien (nichts Gemeinsames angefasst):
- `system/services/ultar-audio/src/codec_cod3035.rs` – `pub trait Codec { enable_speaker; enable_mic; set_speaker_volume }`
  + `Cod3035Codec` (setzt ALSA-Controls per Name über /dev/snd/controlC0). Standalone kompilierbar
  (nur std + extern ioctl), `rustc --test` grün (ioctl-Nummern/Volume-Skalierung).
- `tools/codectest.c` → statisches Binary `/mnt/user-data/outputs/codectest`.
- `docs/A30-AUDIO-CODEC.md` – begründete Sequenz + Geräte-Testanleitung.

## Für Subpaket Audio-1 / ultar-audio
`codec_cod3035.rs` definiert dasselbe `trait Codec`, das die ABOX-Engine erwartet. Zusammenführen:
- In `system/services/ultar-audio/src/lib.rs` (von Audio-1) `mod codec_cod3035;` aufnehmen und EIN gemeinsames
  `trait Codec` verwenden (falls Audio-1 ein eigenes definiert: die beiden identisch halten oder
  dieses hier als kanonisch nehmen). Die Engine ruft vor dem PCM-Start `codec.enable_speaker()` und
  nach `set_speaker_volume()` auf.
- Reihenfolge am Gerät: ABOX-Route+Takt (Audio-1) → `enable_speaker()` (dieses Modul, inkl. `Power Up`) → PCM WRITEI.

## Für ultar-shell (später)
`platform/ultar-shell/src/audio.rs` kann die Mixer-Logik durch `ultar_audio`/dieses Modul ersetzen:
beim Ton/Alarm zuerst `Cod3035Codec::default().enable_speaker()` + `set_speaker_volume(vol)`, dann abspielen.
Kein app.rs/main.rs/i18n.rs nötig.

## Offen / VERIFY am Gerät
- Ob der Lautsprecherpfad NUR SMA1301 (UAIF3) ist oder zusätzlich COD3035-HP – am Gerät testen
  (`codectest spk` setzt beides vorsichtig). `Power Up(1:Up_0:Down)` ist der entscheidende neue Schritt.
- Exakter Default-Wert für `Speaker Volume` (0x38 ist ein sicherer Mittelwert, Bereich 0..167).
