# Eigener Kernel für das Samsung Galaxy A30 (SM-A305F, Exynos 7904)

Samsungs Bootloader lädt beim normalen Start kein Startabbild (Ramdisk) aus der Boot-Partition. Deshalb wird für
dieses Gerät ein eigener Kernel gebaut, in den das Ultar-Startabbild fest eingebaut ist.

- Quelle: https://github.com/topser9/kernel_samsung_universal7885 (Samsung-Quelltext 4.4.177)
- Werkzeuge: https://github.com/LineageOS/android_prebuilts_gcc_linux-x86_aarch64_aarch64-linux-android-4.9
- Änderung am Quelltext: `init/initramfs.c` – `skip_initramfs` vom Bootloader wird ignoriert (`do_skip_initramfs = 0`).
- Einstellungen: `exynos7885-a30_defconfig`, dann
  - aus: `UH UH_RKP RKP_KDP RKP_NS_PROT RKP_DMAP_PROT TIMA* SECURITY_DEFEX FIVE* PROCA LOCALVERSION_AUTO`
  - an: `DEVTMPFS_MOUNT`, `INITRAMFS_SOURCE=<entpacktes Ultar-cpio>`, `LOCALVERSION=-ultar`
- Bau: `make O=out ARCH=arm64 CROSS_COMPILE=…/aarch64-linux-android- ANDROID_MAJOR_VERSION=r PLATFORM_VERSION=11 HOSTCFLAGS=-fcommon Image`
  (ein leeres `ccache`-Skript im PATH, `firmware/` vorher in den Ausgabeordner kopieren)
- Boot-Abbild: Kopf Version 1 wie im Original (Basis 0x10008000, Seite 2048, Name SRPRL14A006KU), nur Kernel,
  Endmarke `SEANDROIDENFORCE`; dazu ein leeres vbmeta (Flags 2).

Stand: gebaut und vom Handy angenommen; ob er startet, ist noch nicht bestätigt.
