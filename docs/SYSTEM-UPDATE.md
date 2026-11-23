# System update (`ultar-update`) – design

Area: system core (area 3, PLAN §3.0). Closes review finding **S16** ("system images from the server are
not verified on the device; there is no updater"). Crate: `system/core/ultar-update` (library, daemon
`ultar-updated`, CLI `uupdate`, signing tool `uupdate-sign`).

The server is **not** a trust anchor (server API.md, "Trust"). Everything the device acts on is proven by an
Ed25519 signature of the publisher and by state stored on the device.

## 1. Threat model

| Attacker / fault | What it can do | Defence |
|---|---|---|
| Malicious or compromised server | answer `update/latest` with anything, serve any bytes | Only a manifest signed by a trusted update key is acted on. All properties (version, sizes, hashes, device, expiry) are inside the signed bytes; `size`, `sha256`, `version` of the server answer are ignored. |
| Network attacker (no TLS, or TLS broken) | modify, truncate, replay, reorder, extend the stream | Same as above; payload SHA-256 while streaming; total size is bounded by the signed sizes before a byte of payload is stored; a byte more than signed is an error. |
| Replay of an old signed release ("latest" from last year) | make the device install a release with known holes | (a) **rollback index** in the manifest must be ≥ the index stored on the device, which only grows; (b) manifest **expiry**; (c) version must be greater than the running version. |
| Release for another device / edition / architecture | brick or downgrade the device | architecture, edition and device list are signed and compared with the device's own identity. |
| Wrong or missing clock | defeat expiry, or lock the device out | "now" = max(system clock, stored time floor, build time of the running image). The floor is the largest `created` time of any manifest **this device accepted** (signature and admission – a release for another board, edition or channel does not move it); it never goes backwards and is not fed from the local clock (a clock set into the future cannot poison it). There is no "not yet valid" rule, so a device whose clock is behind still updates. |
| Power loss at any byte | half-written slot is booted | The running slot is never written. The target slot is marked unbootable *before* the first byte and bootable only after a complete read-back. All state files are written atomically; recovery after restart needs no clean-up code to have run. |
| New system does not start | device stays dead | The new slot is booted "once" with a boot counter; the old slot stays the default until the new system reports health. After N failed starts the loader returns to the old slot by itself. |
| Stolen/retired signing key | sign malicious updates | Key rotation: a key-set document signed by a still trusted key replaces the set; its serial only grows. |
| Hostile bundle file on a USB stick | path traversal, huge allocations | The bundle carries **no file names**; every length is bounded before allocation; parsing is fuzzed. |
| Unprivileged local caller on the bus | trigger apply, change channel | `apply`, `cancel`, `set_policy`, `boot_previous`, offline staging: admin identities only (default: nobody). |

Out of scope: an attacker who already is root on the device or holds a trusted signing key; hardware attacks.
Until the rollback index is also kept in firmware variables/TPM (trait hook exists), an attacker who can rewrite
the state directory offline (disk removed) can lower it – on-disk encryption of the data partition is the
defence there.

## 2. Formats

All numbers decimal ASCII in text, little endian in binary. Identifiers: 1–32 characters of
`a-z A-Z 0-9 . _ - +`, first one a letter or digit (same rule as the server).

### 2.1 Manifest (canonical text, signed)

```
ultar-update-manifest 1
version=0.7.1
rollback_index=3
channel=stable
edition=desktop
arch=x86_64
devices=beelink-gti15,pc-uefi
min_version=0.7.0
notes_key=update.notes.v0_7_1
created=1795400000
expires=1798000000
critical=0
payload=kernel 1048576 <sha256 hex> -
payload=system 734003200 <sha256 hex> <verity root hash hex>
```

* Exactly these lines in exactly this order, `\n` line ends, at most 16 KiB.
* `version`, `min_version`: `major.minor.patch` (each < 2^32, no leading zeros).
* `devices`: sorted, unique, at most 64 entries, or the single entry `*` (every device of that
  architecture and edition).
* `payload`: role `kernel`, `system` or `boot`; each role at most once, in that order; size 1 … 64 GiB without
  leading zeros; SHA-256 of the payload; verity root hash (64 hex) or `-`.
* `critical`: `0` or `1`. `notes_key`: i18n key of the release notes (never text).
* **Canonical:** the device parses the bytes and encodes the result again; if the bytes differ the manifest is
  refused. So there is exactly one byte string per manifest and nothing unparsed is signed.

### 2.2 Signed manifest

`"UUM1"` · u16 length · manifest bytes · key id (8 bytes) · signature (64 bytes).
Signature: Ed25519 over `"ultar-update-v1\n"` + manifest bytes (domain separation: a signature made for an app
package or a driver index can never be taken for an update). Key id: first 8 bytes of SHA-256(public key).

### 2.3 Trust keys and rotation

* Burned into the system image (read-only, verity): `/etc/ultar/update-keys/*.ed25519` – one hex public key
  per line – and optionally `serial` (decimal, default 0). **Separate from the app keys** in
  `/etc/ultar/trusted-keys`: an app publisher key cannot sign a system update.
* Rotation document (canonical text): `ultar-update-keys 1`, `serial=N`, `created=T`, then 1–8 sorted
  `key=<hex>` lines = the **complete new set**. Signed like a manifest with magic `"UUK1"` and domain
  `"ultar-update-keys-v1\n"`, by a key of the set that is in force at that moment.
* The device keeps the accepted rotations in `trust/chain` and the highest serial in a monotonic store.
  On every start the chain is verified again from the burned-in set (links with a serial ≤ the burned-in serial
  are skipped). A chain that does not reach the stored serial is an error (fail closed): removing the file
  cannot bring a revoked key back. At most 64 links are kept; when the chain is full, links not above the
  burned-in serial of the running image are dropped (not earlier, so that a fall-back to an older image still
  finds its way through the chain). Only if none can be dropped is a further rotation refused.
* Rotations travel inside bundles (§2.4), so they need no server support and work offline.

### 2.4 Bundle `.uup` (download file and USB file are the same)

`"UUP1"` · u32 length of the rotation section (0 … 64 KiB; zero or more signed rotations) · rotation section ·
signed manifest · payload bytes in manifest order, nothing after them.
The **header** (everything before the payloads) is at most 128 KiB. Total size = header + signed sizes.

### 2.5 Slot table (boot control data, read by the boot code)

Two copies, 64 bytes each (`slots.0`, `slots.1`):

| Offset | Field |
|---|---|
| 0 | magic `"USLT"` |
| 4 | format = 1 |
| 5 | default slot (0 = A, 1 = B) |
| 6 | trial slot (0, 1, 0xFF = none) |
| 7 | trial tries left |
| 8, 9 | flags of slot A, B: bit 0 bootable, bit 1 successful |
| 10 | slot started last (written by the boot code; 0xFF = unknown) |
| 12 | sequence number u32 |
| 60 | CRC-32 (IEEE) of bytes 0 … 59 |

Reader: the valid copy with the newer sequence number. Sequence numbers wrap around (`0` follows `u32::MAX`;
"newer" = ahead by less than 2^31, `slottable::seq_newer`). Writer: overwrite the *other* copy with sequence + 1 and
flush. A torn write damages only the copy being written; no rename is needed (works on FAT).

Boot decision (`boot::select`, the code the boot path has to run, also used by the tests):
1. Trial slot set, bootable and tries left > 0 → tries − 1, start the trial slot.
2. Trial slot set otherwise → mark it unbootable, clear the trial.
3. Default slot bootable → start it; else the other slot if bootable (it becomes default); else no slot (rescue).
The slot that is started is recorded as "started last".

## 3. State on the device (`/var/lib/ultar/update`)

| File | Content | Written |
|---|---|---|
| `state/rollback.{a,b}` | rollback floor (monotonic store) | after a successful start of a new release |
| `state/time.{a,b}` | time floor | when a manifest was accepted for this device |
| `state/keyserial.{a,b}` | serial of the key set | when a rotation is accepted |
| `trust/chain` | accepted rotations | idem |
| `journal` | apply phase: `idle`, `writing <slot>`, `trial <slot> <previous version>` | atomically |
| `slots/{a,b}.manifest` | signed manifest of what is in each slot | before the slot becomes bootable |
| `slots/{a,b}.factory` | version and rollback index of a slot that came from the installer (no manifest) | at the first start of the updater |
| `staging/bundle.part`, `bundle.pos` | download in progress, flushed length | every 4 MiB |
| `staging/bundle.uup`, `bundle.ok` | complete verified bundle + completion marker (manifest digest, size, downgrade permission, channel it was fetched for or `-` for an offline bundle) | once |
| `rejected` | digest of the signed manifest of the release this device gave up last (§6) | at fallback, failed health check, "start previous version"; removed at the next commit |
| `next_check` | time the next automatic run is due | after every automatic run |
| `lock` | exclusive lock of the running daemon | – |
| `policy`, `history` | policy; bounded history (200 entries) | atomically |

A slot's stored manifest was verified when the slot was written. It is not verified again against
today's keys when the updater only needs to know what its own slots hold – otherwise retiring a key
would make the device forget its running version.

**Monotonic store:** two files `value sha256(value)`; read = the larger valid one; a write is refused unless it
is larger; both files are replaced atomically one after the other. One damaged or missing file is repaired from
the other. Both present but damaged → `state_corrupt`, no update is accepted (fail closed). Both absent → 0
(first start).

## 4. Verification (before anything is written to a slot)

In this order, the first failure wins: envelope shape → key id known (`unknown_key`) → signature
(`bad_signature`) → canonical manifest (`manifest_invalid`) →
expired (`now > expires + 24 h`) → architecture → edition → device → channel (online: must equal the channel
asked for) → payload roles equal the roles of the slot layout → `rollback_index ≥ floor` (`rollback_refused`)
→ `running version ≥ min_version` (`needs_intermediate`) → version greater than the running one
(`up_to_date` / `downgrade_refused`; an administrator may allow a downgrade for an offline bundle, never below
the floor). The result is a `VerifiedManifest`; slot writing accepts nothing else.

Payloads: SHA-256 while streaming into staging, again while streaming from staging into the slot, and a third
time by reading the slot back. The verity root hash is handed to boot control together with "bootable"; the
start chain (stage0) uses it to open the system image.

## 5. Download

Transport (`net.rs`): HTTP/1.1 through `ureq` 3; TLS by rustls with the **Graviola** crypto provider – pure
Rust plus formally verified assembly, compiled by `rustc` alone, so the updater builds statically for x86_64 and
aarch64 without any C compiler. Certificates are always validated (Mozilla roots from `webpki-roots`, or the
roots given to `HttpSource::with`). An `https` source never follows a location or redirect to `http`. Time limits
(none can be switched off): 15 s name lookup and connect, 60 s for the response head, 60 s per body read, plus the
minimum average rate (§9). No proxy is taken from the environment. Graviola needs CPU features (x86_64: AES,
PCLMUL, BMI1, ADX, AVX, AVX2; aarch64: NEON, AES, PMULL, SHA2); on a CPU without them an `https` server is refused
with `update.error.tls_unavailable` – never a fallback to plain text – and offline bundles still work.

1. `GET /v1/update/latest?channel&arch&edition` → release id and location.
2. `Range: bytes=0-131071` of `/v1/update/file/{id}` → header → rotations, signed manifest → full verification.
   Nothing is stored before this succeeds.
3. Free space ≥ bundle size + reserve (64 MiB), else `no_space`.
4. Body with `Range: bytes=<flushed length>-`; `206` continues, `200` restarts from zero. More bytes than
   signed → `size_exceeded`; fewer → `truncated` (kept, resumed later). After every 4 MiB: flush, then record
   the flushed length atomically.
5. Resume after power loss: file is cut to the recorded length, the header is compared with the freshly
   fetched one (other release → staging is discarded), the prefix is hashed again from disk.
6. Complete and all hashes right: flush, rename to `bundle.uup`, write `bundle.ok` (digest of the signed
   manifest + size), flush the directory. A payload hash mismatch discards the staging area.

Policy inputs: automatic yes/no, daily time window (minutes of the day, UTC – the UI converts), channel,
metered allowed yes/no, rate limit (KiB/s). The network state "metered" is pushed by the network service;
unknown counts as metered. A manual download on a metered (or unknown) network needs the policy's
`allow_metered` or the explicit flag of an **administrator**; the flag of any other caller does not count.
A complete staged bundle is replaced by a download only for an administrator (or by the automatic run after
the staged one was refused). Callers that are not administrators can start at most 6 checks/downloads per
10 minutes together.

## 6. Apply (A/B)

```
            check          download           apply                 reboot           health ok
 idle ───▶ available ───▶ staged ───▶ writing ───▶ trial(reboot needed) ───▶ trial(booted) ───▶ idle (new)
                                   │ power loss        │ N failed starts / health failed
                                   ▼                   ▼
                                 staged             idle (old) + history "rolled back"
```

Steps of `apply` (target = the slot that is not running); each is durable before the next starts:

1. Verify marker, bundle size and manifest again (§4).
2. Journal `writing <target>`.
3. Boot control: target **unbootable** (also clears a trial on it). Remove `slots/<target>.manifest`.
4. Per payload: stream staging → slot, hash; flush.
5. Per payload: read the slot back, hash, compare.
6. Write `slots/<target>.manifest`.
7. Boot control: target **bootable** (with verity root hash).
8. Journal `trial <target> <running version>`.
9. Boot control: **start target once**, N tries (default 3).

After the restart (`resume_after_boot`):
* running slot = target and already marked successful → finish the commit (idempotent);
* running slot = target → health checks (required services answer on the bus). OK → **commit**:
  boot control "successful" (target becomes default) → rollback floor raised to the manifest's index →
  the other slot is marked unbootable if its index is below the new floor → staging removed → record of a
  given-up release removed → journal `idle`.
  Not OK → the release is recorded as given up, staging removed, target unbootable, journal `idle`, history,
  "restart needed" (the loader starts the old slot). A daemon restart without a device restart keeps that verdict;
* running slot ≠ target, target still bootable and trial not consumed → the trial is armed again (power loss
  between steps 8 and 9) or simply not restarted yet;
* running slot ≠ target, target unbootable → the loader fell back: the release is recorded as given up, staging
  removed, history "rolled back", journal `idle`.

**Given-up release.** The device remembers the digest of the signed manifest of the release it gave up last
(fallback, failed health check, or left by an administrator's "start previous version"). The automatic run never
installs that release again – not from the server and not from a staged copy; another release is installed
normally, and an administrator can still install the recorded one by hand. Any confirmed release clears the
record.

**Staged bundle that can no longer be installed.** `apply` verifies the staged bundle again; if it is refused
(expired meanwhile, key rotated out, floor raised, channel of the policy changed, damaged) it is removed, and
the automatic run goes on with what the source offers. Changing the channel of the policy removes a bundle that
was fetched for another channel at once; an offline bundle belongs to no channel and stays.

Power loss during 2–7: the target is unbootable, the loader ignores it; on start the journal goes back to
`idle`, the staged bundle is still complete and `apply` can run again (this is an interrupted write, not a
given-up release). The rollback floor is raised **only**
in the commit, so a release that never started successfully cannot lock out the old one.

**Start previous version** (manual): allowed while the other slot is bootable, marked successful and its
rollback index is ≥ the floor. It becomes the default slot; nothing is written.

## 7. Offline update

`uupdate offline <file.uup | folder>`: a folder is searched (not recursively, at most 64 entries, regular
files ending in `.uup`); every header is verified and the newest acceptable release is chosen. The bundle is
copied into staging with the same code as a download (same hashes, same marker) and then applied like any
other. No network, no account.

## 8. Service, CLI, texts

`ultar-updated` runs under the service runtime (`ultar-service`): inherited descriptors closed, umask 077,
working directory `/`, standard limits, every kernel right dropped, no privilege gain; root on purpose. Readiness
is reported once the service is registered on the bus. `SIGTERM`/`SIGINT`: a running operation stops at its next
checkpoint with `interrupted` – no clean-up, nothing staged or fetched is dropped (as after a power loss) – and a
release under test is not judged when the stop comes during the health wait. A body read that is blocked when the
stop comes can outlast the runtime's stop deadline; the runtime then ends the process, which every step survives.
Who is an administrator is decided on the identity the broker gives the caller (`ultar-busd` derives it from the
kernel's peer credentials); the updater itself never looks at sockets.

Bus service `update` (contracts in `ultar_update::contract`, version 1): `status`, `check`, `download`,
`apply`, `cancel`, `history`, `get_policy`, `set_policy`, `offline`, `boot_previous`, `network`; topic
`update.progress` (retained) with phase, percentage and message key. Admin-only: `apply`, `cancel`,
`set_policy`, `offline`, `boot_previous` (identities listed in the daemon's configuration; default nobody).
`network` only from the configured network identity. The bus policy shipped in
`rootfs/etc/ultar/bus-policy.toml` lets only `system.updated` register `update` and publish `update.progress`.
The state directory is created with mode 0700 (files 0600) and refused when it belongs to another user or is
writable by group or others; one daemon per state directory (lock file). The daemon's configuration must name
at least one `health_service` (or the explicit `none`). Every state and error is an i18n key
(`locales/{ar,en,de}.ftl`, Arabic first). Works without an account; a gated channel needs the session token.

Offline bundles are only read from folders named in the daemon's configuration (`offline_root=`); the path is
resolved (links, `..`) before it is compared and the resolved path is what is read.

The updater never restarts the device by itself. After a failed health check it gives the new slot up and
reports `update.error.unhealthy`; the restart is the user's (or the power service's) step.

## 9. Limits (all enforced before memory is allocated or bytes are stored)

| What | Limit |
|---|---|
| Manifest | 16 KiB; 64 devices; 3 payloads; identifiers 32 characters; notes key 96 |
| Payload | 1 byte … 64 GiB each |
| Rotation document | 1 KiB; 8 keys; 64 rotations kept on a device (when full, links not above the burned-in serial of the running image are dropped); 64 KiB rotation section in a bundle |
| Bundle header | 128 KiB (probe request `Range: bytes=0-131071`) |
| `update/latest` answer | 64 KiB; location 2048 characters; one redirect; an `https` server is never left for `http` |
| Transfer | given up when slower than 256 B/s on average after 60 s (kept, resumed later) |
| Unprivileged callers | 6 checks/downloads per 10 minutes together |
| Signing tool | `created` at most 1 day ahead of its clock; validity at most 400 days |
| Download | exactly header + signed sizes; flushed every 4 MiB; free space ≥ missing bytes + reserve (64 MiB) |
| Offline folder | 64 bundle files, not recursive, no links |
| History | 200 entries; policy file 512 bytes; journal 64 bytes; key file 4 KiB |
| Progress events | one per percent (≤ 101 per operation) |
| Trial starts | N = 3 (configurable, ≥ 1); expiry tolerance 24 h (configurable) |
| Automatic check | every 6 h of wall time (configurable, ≥ 15 min; the due time is stored, the timer wakes every 5 min); after a failed run again after 15 min; one operation at a time |

## 10. What this design does not protect against

* A malicious server can waste bandwidth: a correctly signed header followed by wrong payload bytes costs one
  download per automatic check (hash mismatch → discarded). It can also withhold updates (expiry only limits how
  long an *old* "latest" stays acceptable, it cannot force a new one to appear).
* Data that was written but not yet flushed when power is lost: the code flushes before every state change that
  relies on the data (`fsync` of files and directories); that the medium honours the flush is assumed.
* The rollback floor lives in files until the platform offers a firmware variable or TPM counter (trait hook
  `firmware_rollback_index`).
