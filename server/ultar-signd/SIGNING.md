# Signing of Ultar OS releases and store packages (`ultar-signd`)

Owner's decision: the signing keys live on the release server (`ultar.code-craft.tech`).
This document describes how they are protected there, how they are used, how a key is
replaced, and how devices learn about it.

## 1. Keys

| Key | File (server) | Signs | Device trusts it in |
|---|---|---|---|
| `os` | `/var/lib/ultar-sign/keys/os.key` | release files (`.img`, `.zip`) – SSHSIG, namespace `ultar-release` | people downloading: `/downloads/ultar-allowed-signers` (`ssh-keygen -Y verify`) |
| `system` | `/var/lib/ultar-sign/keys/system.key` | system apps (`.uap`, profile `app.system`) | `/etc/ultar/trusted-keys/system/km-code-craft.ed25519` |
| `store` | `/var/lib/ultar-sign/keys/store.key` | store apps (`.uap`, profile `app.standard`, ids `km-code-craft.*`) and the store catalogue | `/etc/ultar/trusted-keys/km-code-craft.ed25519` |

A stolen store key cannot sign a system app or an update; a stolen system-app key cannot sign
an update. Every signature is domain-separated (`ultar-pkg-v1`, `ultar-catalog-v1`, `SSHSIG`
with namespace `ultar-release`), so a signature of one kind never passes as another.

## 2. Protection on the server

* Own system user `ultar-sign` (no login shell, no home). The service refuses to start as root.
* Key files `0400`, owner `ultar-sign`, in `/var/lib/ultar-sign/keys` (`0700`). A key file or
  directory with any group/other bit, a symlink or a foreign owner stops the start.
* systemd sandbox (`ultar-signd.service`): no network at all (`PrivateNetwork`,
  `RestrictAddressFamilies=AF_UNIX`), read-only system, no capabilities, `NoNewPrivileges`,
  system-call filter, 64 MiB memory, no core dumps. The web container (`ultar_server`), nginx and
  coturn have no access to `/var/lib/ultar-sign`.
* Only interface: the Unix socket `/run/ultar-sign/sign.sock` (`0660`, group `ultar-publish`,
  directory `0750`). Only root and members of `ultar-publish` can connect; nothing else on the
  server is in that group.
* It signs **digests only**, in fixed layouts it builds itself – never caller-chosen bytes.
* Rate limit per key: `os` 10 at once / 20 per hour, `system` 60 / 120, `store` 200 / 600.
* Audit log `/var/lib/ultar-sign/audit.log` (`0600`, rotated at 16 MiB, one old file kept): time,
  caller uid, operation, key id, digest, result – written and synced **before** the signature is
  returned; if it cannot be written, nothing is signed.

Remaining risk (honest): root on this server can read the keys and use the socket. Whoever gets
root here can sign. The mitigations above limit what *other* compromised services can do and
make every use visible; they do not replace an offline key.

## 3. Using it (publish scripts)

```text
ultar-signd sign-uap     /run/ultar-sign/sign.sock <system|store> <manifest> <payload.tar.gz> <out.uap>
ultar-signd sign-catalog /run/ultar-sign/sign.sock <ultar-catalog.toml>        # writes .sig
ultar-signd sign-release /run/ultar-sign/sign.sock <file>                      # writes <file>.sig (SSHSIG)
ultar-signd pubkey       /run/ultar-sign/sign.sock <os|system|store>           # hex public key
ultar-signd ssh-signers  /run/ultar-sign/sign.sock                             # allowed_signers line
```
`tools/release/vps/publish-store.sh` and (with the OS image) `publish-release.sh` call these.

## 4. Rotating a key

1. `systemctl stop ultar-signd`; move the old key to `/var/lib/ultar-sign/retired/<role>-<date>.key`
   (`0400`, keep it offline afterwards, then delete it from the server).
2. `runuser -u ultar-sign -- /usr/local/lib/ultar-sign/ultar-signd keygen /var/lib/ultar-sign/keys <role>`,
   `systemctl start ultar-signd`, note the new public key (`pubkey`).
3. Devices:
   * **store / system:** put the new public key into `rootfs/etc/ultar/trusted-keys/…` (same file
     name, so app ids stay bound to the publisher), add `revoke <old hex key>` to
     `rootfs/etc/ultar/trusted-keys/revoked.list`, and ship it with the next system update. Until a
     device has that update it only trusts the old key – so publish the update signed with the
     **update key** first, then re-sign the store with the new key. Note: the launcher binds an
     installed app to its signer key; apps already installed under the old key need the launcher's
     key-change path (open point in docs/integration/ultar-launcher.md).
   * **os:** publish the new `ultar-allowed-signers` (both keys for a transition period, with
     `valid-before`/`valid-after` options), and announce it on the website.
4. Re-sign and republish (`publish-store.sh`), check the audit log.

## 5. Revocation (key stolen)

Stop the service, rotate as above **immediately**, and ship `revoke <hex key>` in
`/etc/ultar/trusted-keys/revoked.list`: the store refuses catalogues signed with a revoked key even
if a key file still lists it (`apps/ultar-store/src/signature.rs`). For the launcher (`ultar-pkg`)
removing the key file is what counts – it does not read `revoked.list` yet (open point).
Update keys rotate through the updater's own signed key-set document (`UUK1`, docs/SYSTEM-UPDATE.md
§2.3); a key revoked there cannot come back.

## 6. Not done

* Signing system **update manifests** (`UUM1`, domain `ultar-update-v1`) is not part of this
  service yet; the update key stays with `uupdate-sign` until it is added (one more operation).
* The launcher/`ultar-pkg` do not read `revoked.list`.
* No hardware security module; no second person needed to sign.
