# Integration ultar-audio in ultar-shell

## Workspace
Root-`Cargo.toml` (members): `"system/services/ultar-audio"` hinzufügen (Koordinator).

## Nutzung in ultar-shell/audio.rs
Das bestehende `audio::play_blocking`/`mixer_setup` scheiterte am EIO. Sobald der ABOX-DSP läuft
(calliope-Firmware geladen, siehe docs/A30-AUDIO-ABOX.md), ersetzt die ultar-audio-Logik den Tonpfad:

1. Beim Ton/Alarm/Notify: statt direktem pcm-Write die Sequenz
   `mixer::Mixer::open().route_speaker(0)` → `codec::StubCodec.enable_speaker()` →
   `pcm::play(0, &tone)`.
2. `audio.rs` kann die drei Module (mixer/pcm/codec) als `ultar_audio`-Crate einbinden
   (`ultar-audio = { path = "../ultar-audio" }` in ultar-shell/Cargo.toml) und die public API nutzen:
   `ultar_audio::mixer`, `ultar_audio::pcm`, `ultar_audio::codec` — dafür in ultar-audio `lib.rs`
   ergänzen (aktuell nur bin). Alternativ die drei Dateien als Submodule von audio.rs kopieren.
3. Lautstärke-Tasten (bereits in ultar-shell) rufen `codec.set_speaker_volume(set.volume)`.
4. Mikrofon (später): `codec.enable_mic()` + WDMA-Capture (pcmC0D8c…), analog pcm.rs für Capture.

## Codec-Schnittstelle (Audio-2 ersetzt den Stub)
`trait Codec { enable_speaker(); enable_mic(); set_speaker_volume(v); }` ist definiert; der StubCodec
setzt die rohen Cod3035-Controls. Audio-2 (echtes Cod3035-Codec-Modul mit DAPM-Widgets/Gain-Tabellen)
implementiert dasselbe Trait – ultar-shell bleibt unverändert.
