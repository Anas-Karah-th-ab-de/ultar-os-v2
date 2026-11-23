# Security review round 3 – device identity (finding S15)

Independent reviewer, 2026-11-23. Subject: `docs/DEVICE-IDENTITY.md`, `server/ultraos-server`, `system/services/ultar-cloud`,
`server/cloud-e2e`, call sign-in (`ultar-call/src/real.rs`, `ultar-shell/src/app_phone.rs`).
Verdict: squatting (S15) is closed, no critical finding, no own cryptography. Five major findings block production.

| # | Severity | Finding | Failing regression test |
|---|---|---|---|
| R3-1 | major | Recovery notice never reaches the owner: password + 72 h = silent number take-over | `ultar-call/tests/r3_notices.rs`; `r3_findings.rs::r3_1_*` |
| R3-2 | major | Password thief registers his own key and removes the owner's devices, cancels the owner's recovery, reads notices | `r3_findings.rs::r3_2_*` |
| R3-3 | major | Call sign-in signs for the stored server name, not the connected server | `cloud-e2e/tests/r3_signal_relay.rs` |
| R3-4 | major | Older socket of a removed or replaced device stays open and usable | `r3_findings.rs::r3_4_*` (2) |
| R3-5 | major | Password checks outside the login rate class, no per-account limit | `r3_findings.rs::r3_5_*` |
| R3-6 | minor | Used challenges keep their slot; a known public key can be locked out | `r3_findings.rs::r3_6_*` |
| R3-7 | minor | `write_atomic` writes token and state through a fixed `.part` name and follows links | `ultar-cloud/tests/r3_state_files.rs` |
| R3-8 | minor | No time limit for an unfinished request in the server process | `ultraos-server/tests/r3_slow_client.rs` |

## Findings and proposed fixes

**R3-1.** The device drops `notice` frames (`real.rs`), nothing calls `Client::device_events` or `recover_cancel`; the
64-entry event list (`device_events.rs:8`, trim at `:80`) can be flushed by registering and removing throw-away keys;
legacy rows have no channel at all (`device_moves.rs:221`).
Fix: (1) `CallEvent::Notice { event, device }`, the shell shows a lasting alert with "this was not me" → `recover_cancel`
and reads `device_events(after)` at start and periodically. (2) Server: a pending recovery is a state – return
`"recovery": {"old_device","eligible_in"}` in `GET /v1/devices`, in the `ready` frame and in the register answer; never
trim `recovery_started` while pending. (3) Legacy rows: no recovery while pre-migration tokens can still upgrade
(30 days), or a clearly longer cool-down; document it.

**R3-2.** `device_api.rs:162-202` gives any password holder a device token for his own new key; remove (`:352`),
rename (`:338`), events (`:384`) and `recover_cancel` accept any active device, however new.
Fix: a key added to an account that already has an active device gets management rights only after an existing
device approved it (signature over the new key, own label) or after the recovery cool-down since its registration.
Until then: own number and remove itself only. First device of an account is exempt.

**R3-3.** `app_phone.rs` connects to `base_url()` but builds the proof from the stored identity; `signal_auth` compares
with the stored name. A relay server can pass the real server's challenge through.
Fix: one function in `ultar-cloud`, e.g. `DeviceHome::signal_identity(base_url)` → `None` unless
`identity::server_name(base_url) == Some(identity.server)`; used by the shell.

**R3-4.** `signal.rs:230` `hub.insert` replaces an entry without stopping the socket behind it; `kick` (`:53-59`) closes only
the map entry; `forward` (`:173-187`) relays for any socket that once authenticated. By inspection: `confirm` reads the
device state under the database lock but inserts later – a removal in between finds nothing to kick.
Fix: one socket per number (stop the previous peer on insert), insert while `confirm` holds the lock or re-read after
insert, `forward` relays only for the hub's current entry.

**R3-5.** `guard.rs:77-87`: only register/login are `Auth` (6/min). `/v1/account/password` and
`/v1/device/recover/start` check the password in class `General` (120/min).
Fix: both into `Auth` plus a failed-password budget per account for all three checks.

**R3-6.** `challenge.rs:257` marks used, the count only falls in `purge` (`:161`) after 120 s; `:202` refuses the ninth;
`retry_after_secs` uses the oldest entry of the whole store. Fix: release the slot on take; per-owner retry time.

**R3-7.** `ultar-cloud/src/http.rs:182-186, :253`. Fix: key-store pattern (random suffix, `create_new`), state directory 0700.

**R3-8.** `ultraos-server/src/lib.rs:249` `axum::serve` without header/body timeout or connection cap.
Fix: header read timeout, request timeout layer, connection cap.

**Observations without a test:** `ucloud register|login <user> <password>` takes the password on the command line;
recovery start and finish share one label; key material is not wiped and a crash between `hard_link` and `remove_file`
leaves `device.key.new.<hex>`; `real.rs` accepts answer/bye/unreachable from any sender; with default features the
shell uses the mock call backend.

## Verified as sound (36 attack tests, all green)

Replay and transplant of challenges and signatures (devices, accounts, servers), cross-protocol label reuse (7 labels
× 5 endpoints), malformed Ed25519 (small-order keys, non-canonical S), races (register, replace, remove, recover,
legacy upgrade – always exactly one winner), recovery abuse (early finish, restart, foreign device), tokens after
removal and password change, legacy abuse after migration, flooding and hostile input, enumeration and oracles, log
leakage (30+ secrets, none in the log), key file handling. Ed25519 via `ed25519-dalek` `verify_strict`, Argon2id,
`OsRng`, constant-time comparisons.

## Not covered

`ultar-shell` not built or run; global challenge bound not driven over HTTP; hub race of R3-4 not reproduced;
no TPM store exists; `verbund.rs` not reviewed (accepts device tokens too); proxy limits not tested.
