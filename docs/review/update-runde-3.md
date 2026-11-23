# Security and robustness review – `system/core/ultar-update` (reviewer r3)

Subject: library, `ultar-updated`, `uupdate`, `uupdate-sign`; design `docs/SYSTEM-UPDATE.md`.
Method: read of every source module (not the `*_tests.rs` files, `msg.rs` and the locale
tables), then attacks as tests against an unmodified copy (own target dir, `-j1`, deleted
afterwards). Baseline of the copy: 186 tests pass, `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings` clean.

**Result: no critical finding. 3 major, 5 minor, all confirmed by a failing test.**
The cryptographic core (signature, canonical form, rotation, rollback floor, A/B write order,
power-loss recovery) held against everything I tried. What breaks is the policy layer on top
of it: the automatic path and the rights of unprivileged bus callers.

Line numbers refer to `/home/claude/ultar-os-v2/system/core/ultar-update/src/`.

## Regression tests

In `regression/system/core/ultar-update/tests/` (target path = same path in the repo; they use
the crate's own `tests/common`). All are fmt and clippy clean, no function over 60 lines.

| File | Finding | State today |
|---|---|---|
| `r3_auto_retry_bad_release.rs` (3 tests) | F1 | fails |
| `r3_stale_staged_bundle.rs` | F2 | fails |
| `r3_unprivileged_download.rs` (2 tests) | F3 | fails |
| `r3_channel_switch.rs` | F4 | fails |
| `r3_time_floor_poison.rs` | F5 | fails |
| `r3_trust_chain_limit.rs` | F6 | fails |
| `r3_https_downgrade.rs` | F7 | fails |
| `r3_state_dir_private.rs` | F8 | fails |
| `r3_crash_walk.rs` | none – evidence for "sound" | passes (12 s) |

Run: copy the files into the crate's `tests/`, then `cargo test -j1 --test <name>`.

## Major

### F1 – Automatic mode installs a rejected release again, for ever
`service.rs:366-385` (`automatic`), `commit.rs:60-92` (`trial_elsewhere`,
`give_up_running_slot`).

After a fallback (failed health check, or N failed starts) the journal goes to idle and
nothing records which release failed. The staged bundle is even kept (staging is purged only
in `commit`). The next automatic run sees "staged" – or asks the server and gets the same
"latest" – and installs and arms the same release. The device then alternates: bad start(s),
fallback, reinstall, bad start(s) … Each cycle rewrites a full slot. With "boot failed" it
costs three failed starts per cycle.

The same hole undoes the administrator's "start previous version": after the restart into
the old system the next automatic run reinstalls the newer one.

Reproduction: 3 tests in `r3_auto_retry_bad_release.rs`; each ends with
`phase: RebootRequired, version: 0.7.1` for the release that was just rejected.

Fix: store the digest of the signed manifest when a release is given up (`RolledBack`,
`HealthFailed`) or left by `boot_previous`; purge staging at that point; the automatic path
skips a release whose digest is stored (manual `apply` by an administrator stays possible,
a release with another digest clears the record).

### F2 – A staged bundle that can no longer be installed freezes automatic updates
`service.rs:377` (`EnginePhase::Staged => return self.apply(engine)`), `apply.rs:119-129`.

`open_verified_stage` returns the refusal (expired, unknown key after a rotation, rollback
floor above it) without dropping the bundle. `automatic` returns that error on every run and
never reaches `check`/`download`. The device stops updating until an administrator calls
`cancel`. Ways into this state: a download without apply (any caller may download, see F3),
power loss or daemon restart between download and apply, device switched off past the
expiry, a later `check` that raises the time floor or brings a key rotation.

Reproduction: `r3_stale_staged_bundle.rs` – three ticks, each `Err(Expired)`, phase stays
`Staged 0.7.1` while 0.7.2 is on the server.

Fix: when verification of the staged bundle fails in `open_verified_stage` with anything
but an I/O error, purge staging; in `automatic`, on such a refusal continue with
`download`. Add `UpToDate`/`DowngradeRefused` to the cases.

### F3 – `download` is open to every caller, overrides policy and replaces admin staging
`service.rs:254` (no right needed), `service.rs:348` (caller's own flag decides),
`stage.rs:286` (`Some(_) => self.purge()`), `contract.rs` (`Apply {}` names no release).

1. Any bus caller passes `allow_metered: true` and downloads a full bundle on a metered
   network although the policy says `allow_metered = false`.
2. Any bus caller's `download` discards a bundle an administrator staged from a stick and
   replaces it with what the untrusted server offers. `apply` then installs that. In the
   test the administrator stages 0.7.2, the server replays the older 0.7.1 (same rollback
   index), a normal app calls `download`, the administrator's `apply` installs 0.7.1. Only
   signed, admissible releases can be swapped in – but it is a replay the administrator did
   not choose, and for an offline device the stick may be gone.
3. Related: one operation at a time and no priority – an unprivileged caller that repeats
   `check` keeps `apply`/`offline` at `busy` and makes `tick` skip silently
   (`service.rs:482-487`).

Reproduction: `r3_unprivileged_download.rs`.

Fix: the metered override needs an administrator (or policy `allow_metered`); replacing a
complete staged bundle needs an administrator; `Apply` carries the version or digest the
caller saw and is refused when staging holds something else; rate-limit `check`/`download`
per caller, or let admin calls cancel a job of a non-admin.

## Minor

### F4 – Channel is not checked at install time; channel change keeps the staged bundle
`apply.rs:122` (`inspect(&header, None, …)`), `service.rs:433-449`. After `beta → stable`
the next automatic run installs the staged beta release. Test `r3_channel_switch.rs`.
Fix: record the channel (or "offline") in the staging marker and compare with the policy at
apply; `set_policy` with another channel purges staging.

### F5 – Time floor can be poisoned by one signed manifest, also on devices it is not for
`engine.rs:311` (witness before `admit`), `signcli.rs:105-106` (any number accepted).
A manifest with `created` in milliseconds, for *another* board, is refused with
`WrongDevice` – and afterwards every correct release is `Expired` on that device, for ever.
Needs a publisher mistake, but the effect is fleet-wide and cannot be undone on the device.
Test `r3_time_floor_poison.rs`. Fix: witness only after `admit` succeeded; `uupdate-sign
manifest` refuses `created` more than a day ahead of its clock and an expiry span over a
fixed maximum.

### F6 – Rotation chain is never shortened; the 65th rotation is refused for good
`keys.rs:198` (skipped links are kept), `keys.rs:232`. Comment at `keys.rs:18` claims a new
image resets the need; it does not. Test `r3_trust_chain_limit.rs`. Fix: in
`TrustStore::open` drop links with serial ≤ burned-in serial and rewrite the file.

### F7 – https source follows locations to plain http on any host
`net.rs:57-74`. An `https://` configuration accepts `http://…` from `update/latest` or a
redirect, including loopback/LAN targets (blind GET from the root daemon). Signature keeps
integrity; privacy and bandwidth are lost. Test `r3_https_downgrade.rs`. Fix: with an https
base accept only https locations; optionally an allow-list of hosts.

### F8 – State directory is not created private and is trusted unchecked
`engine.rs:134-135`, `stage.rs:278`, `fsutil.rs:31`. Mode follows the umask (0755 here).
Rollback floor, slot manifests, key chain and the staging marker with the downgrade
permission are trusted because "root only". Test `r3_state_dir_private.rs`. Fix: create
with 0700, refuse an existing directory that group/others can write or that another user
owns.

## Observations (analysis only, no test; not counted as confirmed defects)

* `service.rs:389-393`: `cancel` reads `running`, then sets the flag. If the job ends in
  between (`service.rs:297` resets first), the flag stays set and the *next* job is
  cancelled at its first checkpoint, which also runs `cancel_pending` (drops staging and a
  pending restart). Window is tiny. Fix: reset the flag in `accept`.
* `daemon.rs:219-226`: the timer sleeps a full interval (default 6 h) after every daemon
  start and nothing stores the last check. A device that never stays up 6 h never checks.
* `net.rs:47`: read time-out per read only. A server that drips one byte a minute holds the
  single operation slot until an administrator cancels. No minimum rate or deadline.
* `filectl.rs:147`: at sequence `u32::MAX` every table change fails (`BootControl`),
  including the boot code's own write. Confirmed by a probe; unreachable in normal life.
* No lock on the state directory: a second `ultar-updated` runs `Engine::open` recovery and
  `resume_after_boot` before it fails at `register`. Outcomes I traced are fail-safe.
* `cli.rs:256-258`: `uupdate verify` checks against the burned-in keys only; a bundle
  signed by a key that was rotated out is reported as valid.
* `config.rs:228`: with no `health_service` configured every start counts as healthy; the
  floor rises and the old slot is retired as soon as the daemon runs.
* After `HealthFailed`, a daemon restart without a device restart ends in
  `Err(BootControl)` when health now passes (`mark_successful` on the given-up slot); the
  published error key is misleading. State stays safe.
* Read-back (`apply.rs`) reads through the page cache after `fsync`; a real partition
  backend should read from the medium.
* No bus policy for the service ships in the repo. Who may publish `update.progress`
  (spoofed progress) and who may call `update.*` is decided there, not in this crate.
* A new device without a clock (floor 0) accepts a long-expired release that is newer than
  its image. This follows from the documented design.

## Verified as sound

Basis is named per item: *test* = my own probe or test, *suite* = the crate's existing tests
(all pass), *read* = code reading only.

* **Manifest** (suite, read): only the canonical byte string parses (signs, leading zeros, CR, missing or
  extra newline, upper-case hex, extra fields, duplicate/reordered lines, overflow in
  versions, sizes and times). Sizes are bounded before use; no arithmetic can overflow.
* **Envelope** (suite, read): domains are not prefixes of each other; a rotation cannot pass as a
  manifest or the reverse; key id only selects among trusted keys; `verify_strict`.
  No own cryptography: Ed25519 from `ed25519-dalek`, SHA-256 from `sha2`; the hand-written
  CRC-32 only guards the slot table against torn writes.
* **Rotation** (suite, read): must be signed by the set in force, serial strictly larger; a retired key
  cannot sign a release, rotate itself back, or fork at an equal serial; removing or
  cutting the chain file fails closed. A rotation by a stolen, not yet retired key is
  accepted – that is the documented limit of the scheme.
* **Rollback** (suite, test): replay of an old release, mixed manifest and payload, swapped staged header
  or payload, floor with one or two damaged copies – all refused. In the crash walk the
  floor never fell, never rose above a release that had run healthy, and no slot below the
  floor ever started.
* **Verification before a bootable slot** (suite, read): the target is unbootable before its first byte;
  the staged file is read through one descriptor; hash while writing, read-back, then
  manifest, then bootable. A modified staged bundle is caught and the slot stays dead.
* **Power loss** (test): the existing sweep covers the named checkpoints only. My walk
  (`r3_crash_walk.rs`, 30 seeds; an earlier form of it ran 460 seeds) adds a power loss before and after every
  boot-control call, I/O errors from boot control with clean-up running, failing health
  checks and daemon restarts without reboot: always a complete startable system. Stale
  `bundle.pos` with a garbage tail: caught by the hash, second attempt installs (test). Torn
  slot table copies (suite). Chain written without serial, half-done `forget_slot`, marker
  without rename: recover or fail closed (read).
* **Download** (suite, read): the server's size is ignored; lying `Content-Range`, a 200 with another
  file, extra bytes, a cut stream, oversized `latest` answer are handled; one redirect; the
  token goes to the configured origin only (prefix tricks with port, userinfo, `//`, `\`:
  read) and never appears in errors or output.
* **Bundle** (suite, read): no names or paths; every length bounded before allocation.
* **Bus** (suite, test): `apply`, `cancel`, `set_policy`, `offline`, `boot_previous` refuse non-admins;
  `network` only from its identity; offline paths are resolved before the root check and
  links in a folder are skipped. 3 600 malformed and mistyped calls from a user and an
  administrator identity over a real broker: no crash, no slot touched, service answers.
* **Logs** (read): two `eprintln!` in the daemon, message keys only; no secret on a command line.
