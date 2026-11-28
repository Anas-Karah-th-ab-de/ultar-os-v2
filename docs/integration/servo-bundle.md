# Laufzeit-Bündel der Servo-Engine (`/usr/lib/ultar/web`)

Das Bündel ist alles, was `ultar-web-engine` (siehe `servo-engine.md`) zum Laufen braucht – gebaut aus
Original-Quellen mit `tools/ultar-paketbau`, nichts vom Wirtsrechner. Es liegt an einem festen Platz im
Systemabbild, die Engine findet ihre Bibliotheken selbst, es wird nichts exportiert (kein `LD_LIBRARY_PATH`).

## Aufbau

```text
/usr/lib/ultar/web/
  libexec/ultar-web-engine     Interpreter = Lader des Bündels, RUNPATH $ORIGIN/../lib (patchelf)
  lib/                         glibc + Lader, libstdc++, libgcc_s, zlib, bzip2, libpng, brotli, expat,
                               freetype, fontconfig, libdrm, Mesa (libEGL, libgbm, libgallium: llvmpipe, iris),
                               libwayland-client (Platzhalter, s. u.)
  share/fonts/noto, extra      Noto Sans, Naskh, Kufi Arabic (Repo), weitere Schriften mit --fonts
  share/ssl/cacert.pem         Zertifikate (Mozilla-Liste des Bauers, `--cacert`)
  etc/fonts/fonts.conf         fontconfig kennt nur die Schriften des Bündels
  share/licenses               OFL der Schriften
```

## Pakete (`tools/ultar-paketbau/pakete/servo-laufzeit`)

Ein Rezept mit Prefix `/usr/lib/ultar/web` (der Bauer erlaubt neben `/pkg/<id>` jetzt `/usr/lib/ultar/<name>`).
Werkzeuge: wie `grafik-laufzeit`, dazu gperf, gmp, mpfr, mpc. Laufzeit-Teile: glibc, **gcc-runtime 15.2.0**
(nur `libstdc++.so.6` und `libgcc_s.so.1` aus den GCC-Quellen; Lizenz GPLv3 **mit GCC Runtime Library
Exception**, die das Weitergeben der Bibliotheken zusammen mit beliebiger Software erlaubt – Lizenztext liegt
in den GCC-Quellen als `COPYING.RUNTIME`; nach dem Wissen des Autors, im Quelltext nicht erneut nachgelesen, kein Rechtsrat), zlib, bzip2, libpng, brotli, expat, freetype, fontconfig,
libdrm, Mesa 26.3.1 mit EGL (surfaceless + GBM), llvmpipe über statisch gelinktes LLVM 23.1.6 und iris.
Prüfungen im Rezept: `egl-probe` (EGL-Display ohne Fenster, Rot gezeichnet und zurückgelesen: „EGL: llvmpipe
ok“, Renderer „llvmpipe (LLVM 23.1.6, 256 bits)“) und `fc-list --version` (fontconfig 2.16.0).

## Bau des Bündels

```text
scripts/bundle-servo-engine.sh --runtime <paketbau work>/out/usr/lib/ultar/web \
    --engine $TARGET/release/ultar-web-engine [--out DIR] [--fonts DIR] [--cacert FILE] [--tests FILE...]
```

Das Skript sammelt, setzt Lader und RUNPATH der Engine, legt Schriften, Schrift-Konfiguration und
Zertifikate dazu und **prüft jede ELF-Datei** (readelf/objdump): Lader und Laufpfade nur im Bündel, jede
NEEDED-Bibliothek vorhanden, keine zu neuen GLIBC-/GLIBCXX-Versionen, und der Lader des Bündels listet die
Engine ohne Umgebung vollständig auf. Letzter Lauf: `BUNDLE: OK … (276M), 107710324 bytes packed, 304
objects checked`, Probleme 0. Ergebnis: `usr/…`, `CHECK.txt` und ein **unsigniertes** Paar
`servo-bundle-<v>.manifest.toml` + `.payload.tar.gz` (Art `runtime`; Signatur nur auf dem Release-Server;
ob der Store die Art `runtime` annimmt, ist nicht geprüft).

## Was der Host selbst setzt (`external/servo-host/src/runtime.rs`)

Liegt das Programm in `<Wurzel>/libexec/` mit `lib/` und `etc/fonts/fonts.conf`, setzt der Host vor Servo:
`FONTCONFIG_FILE`/`FONTCONFIG_PATH` (immer), `MESA_SHADER_CACHE_DISABLE`, `MESA_GLSL_CACHE_DISABLE`
(immer), `EGL_PLATFORM=surfaceless`, `LIBGL_ALWAYS_SOFTWARE=1`, `LP_NATIVE_VECTOR_WIDTH=128`,
`SSL_CERT_FILE` (Vorgaben, ein von außen gesetzter Wert gewinnt) und, falls `lib/gstreamer-1.0` da ist, die
`GST_*`-Pfade. Außerhalb eines Bündels ändert sich nichts. 8 Einheitentests.

## Platzhalter für libwayland-client

surfman (Servos Grafik-Schicht) versucht zuerst Wayland; die Rust-Bibliothek `wayland-sys` bricht aber mit
einem Panic ab, wenn `libwayland-client.so` fehlt, statt auf die Anzeige ohne Fenster zurückzufallen. Das
Ultar-System hat keinen Wayland-Server. Das Bündel bringt deshalb eine winzige Ersatz-Bibliothek
(`pakete/servo-laufzeit/pruefung/wayland-client-stub.c`, wird beim Bündeln mit gcc gebaut), die jede von
`wayland-sys` gesuchte Funktion anbietet und „keine Anzeige“ meldet; surfman fällt dann über X11 (Xlib
fehlt ebenfalls, Fehler) auf die EGL-Anzeige ohne Fenster zurück. Es ist kein Wayland-Ersatz.

## Lauf auf dem Ultar-Kern

`tools/kern-test/servo-run.sh --bundle DIR --kernel ultar-kern.efi` baut ein Wurzel-Abbild (ext4 mit dem
Bündel, `/etc/passwd`, `group`, `nsswitch.conf`, `resolv.conf`, `/tmp`), startet QEMU/KVM (q35, OVMF, virtio-blk,
user-Netz, Gast-CPU Nehalem, seriell in eine Datei) und lässt den Selbsttest des Kerns das Programm
`tests/shot` ausführen: `shot` startet `ultar-tor-port` (Tor-Anschluss) und die Engine, lädt die Seite und
schickt das PNG Base64-kodiert über die serielle Leitung. Das Urteil `SERVO: ALLES OK` kommt nur, wenn der
Host-Rechner aus diesem PNG mindestens 300 Pixel findet, die vom Hintergrund abweichen.

Beleg (Lauf vom 28.11.2026): Engine bereit nach 0,48 s, Seite `https://example.org/` über Tor in 31 s geladen,
`SHOT-PIXELS: 34927 of 1024000`, `SERVO: ALLES OK (1280x800 PNG … 1161 colours)`. Das Bild zeigt den Text der
Seite in Latein, Arabisch (mit Verbindungen), Kyrillisch; Thai als Kästchen. Auf dem Weg dorthin fanden
sich und sind behoben (nicht im Kern): fehlendes `/etc/passwd`/`group` (Tor-Port), der Wayland-Panic, fehlender
Zertifikatsspeicher (Servos Verifizierer bricht ohne ihn ab).

### Kern-Lücken (jede mit Aufruf)

- `getrusage` (Nr. 98): „[syscall] noch nicht umgesetzt: 98“, einmal am Programmende. Ohne Folge.
- Sonst im Lauf keine Meldung des Kerns über fehlende Aufrufe. `socket.rs` und `syscall.rs` wurden nicht
  angefasst, am Kern nichts geändert.
- Annahme, kein Fund: Der Kern rettet keine AVX-Register, darum Nehalem-CPU und 128-Bit-llvmpipe.

## Ton

`external/servo-host/src/pcm.rs`: Schreib-Logik für `/dev/ultar/pcm` (48 kHz, 16 Bit, 2 Kanäle, nicht
blockierend: ein volles Gerät lässt den Rest fallen, halbe Frames werden zu Ende geschrieben, ein dauerhaft
fehlendes Gerät wird als „weg“ gemeldet). `src/media/gst.rs`: GStreamer-Senke `ultarpcmsink` (BaseSink,
S16LE/48000/2), Rang 356, registriert in `media::install()` beim Start, nur mit Merkmal `media`.
Einheitentests: 10 (Logik, mit Schreib-Attrappe) + 3 (Senke: Registrierung, Pufferfluss zum Knoten,
Verwerfen ohne Gerät), alle grün. **Nicht belegt:** Ton auf dem Kern (GStreamer-Pakete und Plug-ins sind
noch nicht gebaut, das Bündel hat kein `lib/gstreamer-1.0`).

## Offen und nicht belegt

- GStreamer (base, good, matroska, isomp4, vpx, dav1d/libav, opus) als Pakete: nicht gebaut.
- Noto Serif und CJK/Thai-Schrift: nicht im Bündel (Lizenz OFL erlaubt es, Quelle noch nicht gepinnt).
- iris: gebaut, nie benutzt (die Messrechner-GPU gehört einem anderen Team); nur llvmpipe belegt.
- Bündel unsigniert; Annahme der Art `runtime` im Store ungeprüft; Spawn der Engine durch den Browser auf dem
  Kern ungeprüft (belegt ist `shot`, das die Engine als Kind startet).
- `ultar-tor-port` ist ein vorhandener Bau (aus dem Ziel-Ordner von `main`), nicht von diesem Paket.
- Quell-Prüfsummen einiger Tarballs (bzip2, gmp, mpfr, mpc, gcc) sind „wie geladen“, nicht gegen
  Entwickler-Signaturen geprüft; `Cargo.lock` von `servo-host` ist nicht eingecheckt.
- Schon vorher rot, nicht von diesem Paket: Test `frames::tests::only_changed_rows_are_written_and_reported`,
  Clippy-Fehler in `src/input.rs:150` und `src/persona.rs:22`, `cargo fmt` in `ultar-paketbau`
  (`closure.rs`, `work.rs`).
