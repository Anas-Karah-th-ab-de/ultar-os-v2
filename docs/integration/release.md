# Release and distribution (v0.8.0-beta)

Branch `release/v0.8` · KM Code Craft · 2026-11-24

## Open points first

1. **OS image not published** (lead's order): the VMware zip and the USB installer image wait for
   `fix/vmware-input` (apps do not open in the VMware image, lock screen not shown at start).
   `releases.json` stays `{"releases": []}`; the website says the image follows.
2. **Version:** one source `tools/release/VERSION` = `0.8.0-beta`. The workspace (`0.6.0`), kernel
   (`0.4.0`) and brand/os-release (`0.7.0`) are aligned in the image step (deliverable 1), not yet.
3. **Device trust:** the public keys created on the server must be put into
   `rootfs/etc/ultar/trusted-keys/km-code-craft.ed25519` (store) and
   `rootfs/etc/ultar/trusted-keys/system/km-code-craft.ed25519` (system apps) – done in this branch
   (see §4). Images built before this commit do not trust the server's packages.
4. **Store app ids:** the server catalogue offers what exists as packages: `store`, `settings`,
   `org.ultar.files` (system key) and `km-code-craft.appbuilder` (store key). The image's built-in
   catalogue lists twelve components (`km-office`, `coder` …) that have no packages yet.
5. **ultar-office, ultar-phone, ultar-web** have no `ultar-app.toml` and are not packaged.
6. **App-Builder install for testing** works only where the developer key is trusted
   (`/etc/ultar/trusted-keys/<publisher>.ed25519`) and the app's identity may call
   `launcher.install` (needs a `bus-policy.toml` line + `launcher.admin`); on a stock image the
   launcher refuses and App-Builder says so. It does not compile code (apps start no programs,
   APP-RULES §4): the developer's toolchain fills `payload/`.
7. **Store manifest:** `network = "allowed"` for the online catalogue (was `none`).
8. **Signing service** has no operation for update manifests (`UUM1`) yet – SIGNING.md §6.

## 0. Live state (2026-11-24, verified with curl from the owner's PC)

* Website v0.8 (ar/en/de) at https://ultar.code-craft.tech/ – old site kept as `/opt/ultar/web.old-20261124-142646`
  (`rollback-web.sh`), archive in `/root/ultar-backup-20261124-142646/`.
* Store: https://ultar.code-craft.tech/store/v1/ultar-catalog.toml (+ `.sig`, 4 packages, `keys/`);
  set `20261124-142640`. Checked with the device code path from B2 (`tools/release/live-check.sh`): catalogue
  signature and all four package digests verify over HTTPS.
* `ultar-signd` active (systemd exposure 0.4 "SAFE"); keys created on the server 2026-11-24:
  os `39025262…52f5ae` (key id `66a1912229841a19`), system `352a24a1…98c827` (`54d9f4ea441a5da2`),
  store `d69d4d77…3023aa` (`68b4f13ebe621352`). Release signature checked with the real `ssh-keygen -Y verify`
  (genuine accepted, tampered refused).
* `ultar_server`, `ultar_turn`, wg0, nginx, code-craft.tech, maps-sy unchanged and up. Backups:
  `/root/ultar-backup-20261124-142544/` (nginx conf, snippets, web) before the setup.

## 1. What is where

| Part | Path |
|---|---|
| Signing service + client (Rust, own workspace) | `server/ultar-signd/` – operation and key rotation: `server/ultar-signd/SIGNING.md` |
| Online catalogue source of the Store | `apps/ultar-store/src/online.rs`, `online_https.rs` (feature `online`, on with `gui`) |
| Developer app | `apps/ultar-appbuilder/` |
| App packages for the store | `tools/release/build-apps.sh`, `apps.list`, `catalog/*.toml` |
| Builder gate | `tools/release/ci-release.sh` (+ `shots.sh`, `bundle.sh`, `b2-run.sh`) |
| Release server scripts | `tools/release/vps/` (`setup.sh`, `publish-store.sh`, `store-rollback.sh`, `publish-release.sh`, `deploy-web.sh`, `rollback-web.sh`, `install-signd.sh`, unit, nginx snippet) |
| Website sources | `tools/release/site/` (deployed to `/opt/ultar/web`) |

## 2. Store server

Static files served by nginx, the same layout as a USB stick:

```text
https://ultar.code-craft.tech/store/v1/ultar-catalog.toml       signed catalogue (format ultar-catalog-v1)
https://ultar.code-craft.tech/store/v1/ultar-catalog.toml.sig   "<key-id>\n<base64 Ed25519>"
https://ultar.code-craft.tech/store/v1/<id>-<version>.uap       packages (ultar-pkg-v1), sha256 pinned in the catalogue
https://ultar.code-craft.tech/store/v1/keys/*.ed25519           public keys (for checking; devices use the image's)
https://ultar.code-craft.tech/downloads/ultar-allowed-signers   OS release key for ssh-keygen -Y verify
```
`/store/v1` is a symlink to `/opt/ultar/store/releases/<time>/`; publishing swaps it atomically,
`store-rollback.sh` points it back, the last five sets are kept. nginx: GET/HEAD only, the site's
security headers, `Cache-Control: no-cache`. No new dynamic endpoint; `ultar_server` unchanged.

Device side (`OnlineCatalogSource`): HTTPS only (rustls + Graviola, Mozilla roots, no redirects,
no proxy from the environment, 20 s timeouts); catalogue ≤ 4 MiB verified with the same function
as the file source before it is parsed or cached; cache written atomically and verified again when
loaded; server unreachable → cache → image catalogue. Packages: only entries of the last verified
catalogue, only with `sha256`, at most `size_download` bytes, `.part` + digest check + rename; the
launcher verifies the package signature again. Environment: `ULTAR_STORE_URL`,
`ULTAR_STORE_OFFLINE=1`, `ULTAR_STORE_CATALOG` (stick/folder wins), cache in
`ULTAR_STORE_DATA/catalog-cache`.

## 3. Pipeline

```text
B2:  bash tools/release/ci-release.sh      # signd tests+binary, store/appbuilder gate x3, build-apps.sh
     bash tools/release/bundle.sh          # out/release-ci/bundle.tar.gz
PC:  scp B2:…/bundle.tar.gz → VPS:/root/ultar-publish/
VPS: bundle/vps/setup.sh bundle/ultar-signd             # once (idempotent): service, nginx, allowed-signers
     bundle/vps/publish-store.sh bundle/store-staging   # sign packages + catalogue, switch /store/v1
     bundle/vps/deploy-web.sh bundle/site               # website (rollback-web.sh)
     bundle/vps/publish-release.sh <ver> beta notes.json <files>   # OS image, after the fix
```

## 4. Tests

| Crate | Tests |
|---|---|
| ultar-signd | 23 unit + 1 socket end-to-end (fixed-seed fuzz of the request parser, permissions, rate limit, audit fail-closed, uap/catalogue/release signatures verified independently). SSHSIG verified with the real `ssh-keygen` on the release server (the unit test only checks the format where `ssh-keygen` is missing). |
| ultar-store | 38 lib (7 new: online load/verify/cache, offline fallback, tampered/unsigned server, pinned digest and size, unpinned refused, https only, fuzzed answers; revocation) + 65 integration – 3× on B2, clippy `--features gui` clean |
| ultar-appbuilder | 4 unit + 3 flow (check → build → verify with `ultar-pkg` → install double; every issue listed; nothing built on errors) + 2 texts/screens |
