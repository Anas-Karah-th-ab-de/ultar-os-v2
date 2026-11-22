# micprobe – Mikrofon des A30 prüfen

    aarch64-linux-gnu-gcc -static -O2 -o micprobe micprobe.c -lm
    micprobe dump [text]                  alle ALSA-Regler (Typ, Wert, Auswahl-Einträge)
    micprobe set "<genauer Name>" <wert>  Zahl oder Auswahl-Eintrag setzen
    micprobe rec 9 3000 /data/x.raw 2 48000 960 8

Aufnahmeweg (aus Samsungs `/vendor/etc/mixer_paths.xml`, `media-mic`):
`ABOX SPUM ASRC0..3 = On`, `ABOX NSRC0 = UAIF0`, `ADCL Mixer MIC1L Switch = 1`,
`ADCR Mixer MIC1R Switch = 1`, `MIC1 MIC1 On = 1`; Gerät `pcmC0D9c` (WDMA1).
Periode × Perioden muss den Puffer genau ergeben (960 × 8) – siehe docs/HELFER.md.
