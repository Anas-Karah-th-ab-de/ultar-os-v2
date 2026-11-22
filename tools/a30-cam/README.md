# A30-Kamera: Testwerkzeuge (fimc-is2, Stock-Kern 4.4.177)

- `camraw.c`  – Rohbild (Bayer) vom Sensor: Leitknoten ssN + Aufnahmeknoten ssNvc0.
- `fmtprobe.c` – findet, wo der Kern Breite/Höhe/Format im S_FMT-Block liest.
- `vprobe.c`  – probiert Format-/Puffer-ioctls an einem Knoten durch.

Bauen (statisch, aarch64). **Wichtig:** der Samsung-Kern hat `VIDEO_MAX_PLANES 17` statt 8 –
mit normalen Kopfdateien haben S_FMT/G_FMT eine andere ioctl-Nummer und liefern ENOTTY.

    mkdir -p kinc/linux
    sed 's/define VIDEO_MAX_PLANES .*/define VIDEO_MAX_PLANES 17/' \
        /usr/aarch64-linux-gnu/include/linux/videodev2.h > kinc/linux/videodev2.h
    # stub/: fimc-is-metadata.h aus dem Kernquelltext (drivers/media/platform/exynos/fimc-is2/include),
    #        fimc-is-config.h = "#define USE_DEBUG_LIBRARY_VER" + "#define CAPTURE_NODE_MAX 12" (Stock-Kern!),
    #        leere fimc-is-framemgr.h, fimc-is-common-enum.h aus dem Kern
    aarch64-linux-gnu-gcc -O2 -static -Ikinc -Istub -o camraw camraw.c

Stand und Befunde: `docs/A30-KAMERA.md`.

## ultar-cam (Helfer für die Oberfläche)

    sh build.sh          # baut tools/a30-cam/ultar-cam statisch; danach nach bins/a30/ kopieren

`stub/fimc-is-metadata.h` und `stub/fimc-is-common-enum.h` stammen aus Samsungs Kernquelltext
(GPL-2.0 oder Apache-2.0, siehe Dateikopf). `stub/fimc-is-config.h` setzt `CAPTURE_NODE_MAX 12` wie der Stock-Kern.
