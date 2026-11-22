# Laufzeit für Ultar Web (Buildroot)

Baut WPE WebKit 2.54.3 (nur WPEPlatform), GStreamer, Mesa (nur als Bauvoraussetzung), glibc u. a. für aarch64
(Cortex-A73/A53, Kernel-Header 4.4) – ohne Ubuntu. Getestet mit Buildroot 2026.08.2.

```bash
curl -L https://buildroot.org/downloads/buildroot-2026.08.2.tar.xz | tar xJ
cd buildroot-2026.08.2
make BR2_EXTERNAL=<repo>/tools/ultar-web-br/br-ext O=<out> ultar_aarch64_defconfig
make BR2_EXTERNAL=<repo>/tools/ultar-web-br/br-ext O=<out>        # 16 Threads: ca. 1,5–2 h
<repo>/scripts/build-ultar-web.sh <out> aarch64                     # Rust-Kern + Bündel + Filter
```
Patches: `0001` (Bau ohne libdrm), `0002` (CPU-Darstellung per `WEBKIT_DISABLE_HARDWARE_ACCELERATION`).
GnuTLS liest die Zertifikate aus `/usr/lib/ultar/web/etc/ssl/certs/ca-certificates.crt` (external.mk).
