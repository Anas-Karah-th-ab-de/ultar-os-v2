# A30 Audio – Codec-/Amp-Seite (Subpaket Audio-2)

Aus der echten Kernelquelle (/home/claude/a30k) abgeleitet. Ergänzt Subpaket Audio-1 (ABOX/DPCM/Takt).

## Topologie (verifiziert)
Die Karte „Exynos7885-COD3035" hat **zwei** Audio-Bausteine am ABOX:
- **SMA1301** (Silicon Mitus, `sound/soc/codecs/sma1301.c`) = **Lautsprecher**-Class-D-Verstärker,
  bekommt sein I2S direkt von **ABOX UAIF3** (mixer_paths: route-spkamp-playback → UAIF SPK3).
  Wird **nicht** über den COD3035 angesteuert.
- **COD3035** (`sound/soc/codecs/cod3035x.c`) = **Kopfhörer/Ohrhörer + Mikrofon**
  (DAC→HPDRV/EPDRV, ADC←MIC_PGA), an einem eigenen UAIF.

## KERN-ERKENNTNIS (warum bisher kein Ton)
SMA1301 hat das Control **`Power Up(1:Up_0:Down)`** (`sma1301.c:1786`, `SOC_SINGLE_EXT`), dessen
Put-Handler **`sma1301_startup()`** aufruft → der Amp wird erst dadurch mit seiner vollen
Register-Init (Boost, Class-G, Modulator, POWER_ON) hochgefahren. **Dieses Control wurde in den
bisherigen Mixer-Versuchen nie gesetzt.** Zusammen mit dem fehlenden BE-Takt (Audio-1) erklärt das
das EIO/Stummbleiben vollständig.

## Lautsprecher-Sequenz (SMA1301), geordnet
1. `Force AMP Power Down` = 0   (sonst blockt `force_amp_power_down` das Startup, sma1301.c:420)
2. `Speaker Volume` = ~0x38     (Register 0x0A, Bereich 0..167)
3. `Speaker Mute Switch(1:muted_0:un)` = 0
4. **`Power Up(1:Up_0:Down)` = 1**  → `sma1301_startup()` (Boost/Class-G/Modulator + POWER_ON)
(Optional, falls Pfad zusätzlich über COD3035-HP läuft – mixer_paths dev-speaker: `HP HP On`=1, `DAC Soft Mute`=0.)
Voraussetzung: ABOX liefert bereits den I2S-Takt auf UAIF3 (Audio-1), sonst hat der digitale Amp keinen Bitclock.

## Mikrofon-Sequenz (COD3035), geordnet
Pfad (DAPM-Routen cod3035x.c): `MIC1 → MIC1_PGA → ADCL/ADCR Mixer → ADC`. MIC_PGA braucht VMID,
MIC1 schaltet Register 0x6C (`COD3035X_6C_MIC_ON`). MICBIAS/VMID/PGA werden von den DAPM-Event-
Callbacks (`cod3035x_mic_bias_ev`, `adc_ev`) automatisch gesetzt, sobald der Capture-Stream aktiv ist.
1. `MIC1 On` (bzw. `MIC1 MIC1 On`) = 1
2. `ADCL Mixer MIC1L Switch` = 1
3. `ADCR Mixer MIC1R Switch` = 1

## Warum reine Mixer-Writes reichen (mit aktivem Stream)
`HP On`/`MIC1 On`/`ADC* Mixer *Switch` sind **DAPM-Switch-kcontrols**. Ein Schreibzugriff darauf
stößt die ASoC-DAPM-Sequenz an → die Event-Callbacks (VMID/Bias/DAC/ADC/Charge-Pump) feuern. Das
funktioniert aber nur, wenn ein **aktiver Stream** auf dem Codec-DAI liegt (→ Audio-1 liefert den
BE-Takt via DPCM). SMA1301 dagegen wird direkt über sein `Power Up`-Control gestartet (kein DAPM nötig),
braucht aber den UAIF3-Bitclock.

## Geräte-Testanleitung (für den Koordinator, über die Tür)
Voraussetzung: Audio-1 hat Route+Takt gesetzt und einen Stream geöffnet (oder parallel testen).
1. `upush codectest /data/codectest`
2. Lautsprecher an: `ucmd "/data/codectest spk"` → sollte alle Controls setzen (inkl. `Power Up`=1).
3. Ton über Audio-1 abspielen: `ucmd "/data/ultar-audio --tone"` → **jetzt sollte der Lautsprecher klingen** (kein EIO mehr).
   Gegencheck dmesg: `ucmd "dmesg | grep -iE 'sma1301|abox|uaif' | tail"` → `sma1301 ... power on` / kein EIO.
4. Lautstärke: `ucmd "/data/codectest spkvol 80"`.
5. Mikro: `ucmd "/data/codectest mic"`, dann Aufnahme über Audio-1 (WDMA/Capture).
6. Reihenfolge-Fehlersuche: falls weiter still → zuerst prüfen, ob `Power Up` wirklich gesetzt wurde
   (`/data/codectest set "Power Up(1:Up_0:Down)" 1`) und ob der UAIF3-Takt läuft (Audio-1-dmesg).

## Dateien
- `crates/ultar-audio/src/codec_cod3035.rs` – Rust: `trait Codec` + `Cod3035Codec` (set per Name), `enable_speaker/enable_mic/set_speaker_volume`.
- `tools/codectest.c` → `/mnt/user-data/outputs/codectest` (statisch).

## Stand 2026-11-21: Hörmuschel, Kopfhörer, Mikrofon und Gesprächston (ultar-audio neu)

`ultar-audio` nutzt jetzt Samsungs eigene Tonwege aus `/vendor/etc/mixer_paths.xml` (+ `mixer_gains.xml`) –
gleiche Logik wie Androids audio_route (Grundeinstellung = Regler auf oberster Ebene, Weg = Regler + Verweise,
dazu `gain-<weg>`). Nur der Lautsprecher behält unseren bestätigten Weg (RDMA0 → SPUS OUT0 → SIFS1 → UAIF3 → SMA1301).

| Teil | Weg | Stand am Gerät |
|---|---|---|
| Mikrofon unten (MIC1) | `media-mic`: NSRC0 = UAIF0, MIC1 → ADC → WDMA1 = `pcmC0D9c` | ✅ Lautsprecher-Testton klar aufgenommen (RMS ≈ 17000–27000) |
| Mikrofon oben (MIC2) | `media-2nd-mic` | ✅ Testton aufgenommen (RMS ≈ 1900) |
| Kopfhörer-Mikro (MIC3) | `media-headset-mic` | Weg gesetzt, ohne Kopfhörer nicht prüfbar |
| Hörmuschel | `media-handset`: EP On, RDMA0 → SIFS0 → UAIF0 → COD3035 EPDRV | ⚠️ DAPM-Kette läuft fehlerfrei (EP/EPDRV an, kein EIO); mit den Mikros nicht hörbar nachweisbar → Hörprobe offen |
| Kopfhörer | `media-headset`: HP On + Lautstärke; Erkennung `/sys/class/audio/earjack/state` (0 = nichts) | Weg gesetzt; `--route` nimmt automatisch Kopfhörer, wenn eingesteckt |
| Gesprächston | `incall_nb-<gerät>` + `incall_nb-<gerät>-mic`, `ABOX Audio Mode = IN_CALL`, Basisband-Strecken `pcmC0D4p` (RDMA4) und `pcmC0D10c` (WDMA2) offen + gestartet | ✅ Strecken starten (RDMA4 → UAIF0, UAIF0 → WDMA2), Umschalten Hörer/Lautsprecher geht; mit echtem Anruf noch zu prüfen (SIM fehlte) |

Modem-Seite (aus `libsec-ril-dsds.so`, `IpcProtocol41Sound`): Gruppe 0x09
- `0x0909` SND_CLOCK_CTRL, EXEC `[1]` beim Gesprächsbeginn, `[0]` danach
- `0x0903` SND_AUDIO_PATH_CTRL, SET `[weg, 0]` – Hörer 1, Kopfhörer 2, Lautsprecher 6 (Werte wie libsamsung-ipc)
- `0x0901` SND_SPKR_VOLUME_CTRL, SET `[art, stufe]` – Hörer 0x01, Lautsprecher 0x11, Kopfhörer 0x31
- `0x0902` SND_MIC_MUTE_CTRL, SET `[0|1]`
`ultar-ril` schickt das selbst, sobald ein Gespräch wählt/klingelt-beim-Gegenüber/aktiv ist, und startet
`ultar-audio --call`. Steuerdatei `/run/ultar/callaudio` (`auto|handset|speaker|headset|off`): die Telefon-App
schreibt `speaker`/`auto` (Lautsprecher-Knopf), der RIL `auto` beim Start und `off` am Ende. Stummschalten:
Funkbefehl `mute 0 1|0`.

Wichtige Funde:
- **DSP kann nach Stunden hängen**: nach ~8 h Laufzeit beantwortete Calliope keine IPC mehr
  („Transaction timeout(n)", Wiedergabe und Aufnahme → EIO). Neustart behebt es. Ursache noch offen.
- Eine Medien-Wiedergabe darf die Aufnahmeregler nicht zurücksetzen (sonst EIO der laufenden Aufnahme) →
  `reset_playback()` lässt NSRC/SPUM/MIC/ADC unangetastet.

Werkzeug:

    ultar-audio --rec 3000 /data/ultar/m.wav mic|2nd|headset   Pegel je Viertelsekunde
    ultar-audio --out handset|headset|speaker 0.8 1000        Weg + Testton
    ultar-audio --beep 1000 1500                              Ton ohne Wegänderung
    ultar-audio --call handset   (Ende: echo off > /run/ultar/callaudio)
    ultar-audio --list [text] | --set <name> <wert> | --path <weg>… | --paths [text] | --jack
