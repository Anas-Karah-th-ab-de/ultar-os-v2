# Statische Werkzeuge für Sicherheit, VPN & Cloud (ins Image legen)

Diese geprüften Standard-Binaries müssen beim Image-Bau nach `/usr/lib/ultar/` (statisch für
aarch64). Die Rust-Crates rufen sie nur auf — keine eigene Kryptografie.

| Datei | Herkunft | Zweck |
|---|---|---|
| `cryptsetup` | cryptsetup (statisch) | /data-Verschlüsselung (LUKS2, AES-XTS, Argon2id) |
| `veritysetup` | cryptsetup-Paket | verifizierter Start (dm-verity) |
| `mke2fs` | e2fsprogs | ext4 im verschlüsselten Mapper anlegen |
| `wg` | wireguard-tools | WireGuard-Schlüssel/Config |
| `boringtun` | cloudflare/boringtun | WG-Userspace, falls Kernelmodul fehlt (über /dev/net/tun) |
| `curl` | curl (statisch, mit TLS) | HTTPS-Aufrufe der Ultar-Cloud |

Zusätzlich im System vorhanden sein müssen: `ip` (iproute2), `nft` (nftables) für Routen/Killswitch
— stehen i. d. R. über den Compat-/Busybox-Pfad bereit.

## Startkette (verified boot)
- `ultar-stage0` als `/init` ins Image (statisch).
- echter Init als `/sbin/ultar-init`.
- `/ultar/verity.conf` (data_dev, hash_dev, roothash) + `/ultar/verity.sig` + eingebrannter
  Pubkey `/ultar/pub.ed25519` — vom Bau-Skript `mkverity.sh` (WSL) erzeugt.

Fehlen stage0/verity.conf, bootet das Gerät unverändert weiter (ungesichert) — nur echte
Manipulation (Hash passt nicht) hält den Start an. So bleibt die Projektregel „Gerät bootet immer"
erhalten, bis Verity bereitgestellt ist.
