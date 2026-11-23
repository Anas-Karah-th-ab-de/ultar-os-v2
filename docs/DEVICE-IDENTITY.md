# Device identity (finding S15)

Status: implemented in `server/ultraos-server` (schema 3) and `system/services/ultar-cloud`;
revised after security review round 3 (`docs/review/identity-runde-3.md`). §10 says in one place
what the account password alone can and cannot do.
Wire reference: `server/ultraos-server/API.md` §3. This document is the design: threat model,
protocol, state machines, storage, failure cases, migration.

Only standard building blocks are used: Ed25519 (`ed25519-dalek`, strict verification), SHA-256
(`sha2`), HMAC-SHA256 (`hmac`), Argon2id (`argon2`), the operating system's random generator.
Nothing here is a new primitive; base32 and the transcript framing are encodings.

## 1. What was wrong

* The device id was a 64-bit FNV hash of "serial number + contents of a key file". The server
  accepted any string: **whoever sent an id first owned it** (`device_taken` for the real device).
* Without an Android serial number every device computed the id of `"unknown"` + empty key file,
  so all such devices shared one id (`f2a5c3…`-style constant) and one account took it for good.
* The stored "public key" was never used. The signalling socket accepted an account token plus a
  number of that account: a stolen password was enough to receive somebody's calls.
* No limit of devices per account; tokens could not be revoked (stateless, 30 days), there was no
  password change and no way to remove a device.

## 2. Threat model

Attackers considered:

| # | Attacker | Must not be able to |
|---|---|---|
| A1 | Any account holder | claim or block a device id / number of another device (squatting), register a key he does not hold |
| A2 | Network observer of old traffic, replaying messages | reuse a signature: at another time, for another account, for another purpose, towards another server |
| A3 | Thief of the account password (no device) | receive calls of an existing number; remove, rename or read about the owner's devices; take a number over without delay and without the owner's devices being told (see §10 for what remains) |
| A4 | Thief of a token (account or device) | keep access after the device is removed or the password is changed |
| A5 | Holder of a removed / replaced device | keep using its number or tokens |
| A6 | Malicious or impersonated server | obtain a signature usable on the real server; learn the private key |
| A7 | Flooder | exhaust memory (challenges), numbers or rows through one account |
| A8 | Reader of server logs / database dump | learn hardware serial numbers or exact activity times |

Out of scope: an attacker who holds the server's token key (`ULTRAOS_SECRET`) or controls the
server process (he can route calls anyway; media stay end to end encrypted); malware running as the
device's cloud user while the file key store is in use (it can sign; a TPM/secure element store
removes key extraction, not live use); theft of the unlocked device together with the account
token. Transport security is TLS in front of the server.

## 3. Identity

* **Device key**: Ed25519 key pair generated on the device at first use. The private key lives in
  a `KeyStore` and never leaves it; callers only get `public_key()` and `sign(message)`.
* **Device id** = lower-case RFC 4648 base32, no padding, of the first 160 bits of
  `SHA-256("ultar-device-v1" ‖ public_key)` → exactly 32 characters of `a-z2-7`. No serial number,
  no hardware property enters it. Two devices have different ids because they have different keys.
* **Ultar number** `UOS-XXXX-XXXX`: assigned by the server to a device key at registration. It
  stays with that key; it moves to another key only through §5.6 / §5.7 and is never given to a
  different key otherwise, not even after removal or account deletion (§7).

## 4. Signed transcripts

Every signature is over a domain-separated, length-framed byte string:

```
T = label ‖ 0x00
  ‖ be16(len(server)) ‖ server          server name, see below
  ‖ be64(account_id)
  ‖ challenge                           32 bytes
  ‖ subject_key                         32 bytes, the signer's own public key
  ‖ be16(len(extra)) ‖ extra
```

| Purpose | `label` | signer / `subject_key` | `extra` |
|---|---|---|---|
| register | `ultar-device-register-v1` | new key | empty |
| session (device token) | `ultar-device-session-v1` | device key | empty |
| signalling socket | `ultar-device-signal-v1` | device key | empty |
| legacy upgrade | `ultar-device-upgrade-v1` | new key | legacy device id (UTF-8) |
| replace, old side | `ultar-device-replace-old-v1` | old key | new public key (32 bytes) |
| replace, new side | `ultar-device-replace-new-v1` | new key | old public key (32 bytes) |
| recover, start | `ultar-device-recover-v1` | new key | id of the lost device (UTF-8) |
| recover, finish | `ultar-device-recover-finish-v1` | new key | id of the lost device (UTF-8) |
| approve an added device | `ultar-device-approve-v1` | approving (managing) device | public key of the approved device (32 bytes) |

No label is a prefix of another once the `0x00` is appended; every variable field carries its
length, fixed fields have fixed sizes. A signature made for one purpose, account, server, key or
challenge does not verify for any other (A2).

**Server name**: the authority of the server's public address, lower case, default port dropped
(`ultar.code-craft.tech`, in tests `127.0.0.1:41234`). The server has it configured
(`ULTRAOS_SERVER_NAME`); the device derives it from the address it is **connected to** and refuses
to sign when the name in a challenge differs (A6: a signature obtained by server M names M and is
useless at S). This holds for the HTTP calls (`Client::device_challenge`) and for the call
sign-in: `DeviceHome::signal_proof(address)` hands out a proof only when `address` is the server
of the device's registration, so a device that is pointed at another server after it registered
does not even answer that server's challenge.

**Challenge**: 32 bytes, valid 120 s, single use, bound at issue time to (purpose, account,
claimed public key) and addressed by a `challenge_id`; the challenge bytes are never a lookup key
and are compared only inside the signature check. Two kinds (revised in review round 4):

* **Account challenges** (register, upgrade, replace, recover, approve – requested with a
  token): 32 random bytes, a random 16-byte id, stored while pending; at most 8 pending per
  account (`challenge_capacity` with `Retry-After` = until the account's own oldest one
  expires). The first presentation removes the pending entry, whatever the outcome. A small
  marker of the presentation is kept only to answer a replay with `challenge_used`; an account
  keeps at most 8 markers (its oldest goes first) and a full store drops markers before it
  refuses anything. A replay is refused either way (`challenge_used` or `challenge_unknown`).
* **Session challenges** (requested without a token, by naming a public key): **stateless**.
  The id (81 bytes) is `0x53 ‖ account ‖ issue time ‖ public key ‖ 16-byte nonce ‖ tag`, the tag
  being 16 bytes of HMAC-SHA256 under a per-process random key; the challenge is HMAC-SHA256
  under a second per-process key over the same bytes. Issuing stores nothing, so requests of a
  stranger who knows the key change nothing for the device. A session challenge is consumed only
  by a presentation whose **signature verifies** (its nonce is then remembered for the rest of
  its 120 s; at most 16 accepted per key and 120 s); a wrong signature leaves it usable.

At most 65 536 entries are stored in total (pending account challenges, markers, accepted
session nonces); markers give way first (A7). The per-process keys are new after a restart: every
earlier id is unknown (fail closed).

Verification uses `verify_strict` (rejects small-order keys and non-canonical signatures); weak
public keys are refused when the challenge is requested.

## 5. Protocols

All bodies are JSON; keys, challenges and signatures are base64url without padding.
`AT` = account token (from register/login), `DT` = device token.

### 5.1 Challenge
```
D → S  POST /v1/device/challenge   [AT, except purpose "session"]
       {"purpose":"register|session|upgrade|replace|recover","public_key":"<32 bytes>"}
S → D  {"challenge_id":"…","challenge":"…","server":"<name>","account_id":7,"expires_in":120}
```
For `session` the account is the one the key is registered to (`device_unknown` /
`device_retired` otherwise).

### 5.2 Registration with proof of possession
```
D → S  POST /v1/device/register   [AT]
       {"challenge_id":"…","signature":"<sig over T(register)>","name":"Phone"}
S      challenge exists, unused, unexpired, purpose register, same account   → else typed error
       signature verifies under the public key bound to the challenge       → else signature_invalid
       key unknown → device limit, lifetime quota → insert device, assign number
       key already active on this account → same number (idempotent)
       key on another account → device_taken; key removed/replaced → device_retired
S → D  {"device_id":"…","number":"UOS-…","account_id":7,"token":"<DT>","expires_in":86400,
        "manage":false,"manage_in":259200,"recovery":null}
```
All checks and writes of one registration run in one database transaction; two concurrent
registrations of one key end with one device row and one number.

`manage` / `manage_in`: whether the device may manage the account's other devices (§5.5).
`recovery`: the pending recovery of the account, `{"old_device":"…","eligible_in":…}` or `null`
(§5.7). Both are part of every answer of this shape (register, session, upgrade, replace,
recover/finish).

### 5.3 Device token (device-bound session)
```
D → S  POST /v1/device/challenge {"purpose":"session","public_key":"…"}      (no token)
D → S  POST /v1/device/session   {"challenge_id":"…","signature":"<sig over T(session)>"}
S → D  {"token":"<DT>","expires_in":86400,"device_id":"…","number":"UOS-…","account_id":7}
```
`DT = b64("d1:<account>:<device_id>:<expiry>:<account_epoch>:<device_epoch>") "." b64(HMAC)`,
keyed with `HMAC-SHA256(ULTRAOS_SECRET, "ultar-device-token-v1")` (a key separate from account
tokens). A device token is issued **only** against a fresh signed challenge and names the key id.
On every use the server reads the device row: it must be active, belong to the account, and both
epochs must match. A token therefore dies the moment the device is removed or replaced (device
epoch) or the password changes (account epoch). A token copied to another device still names the
original key id; it cannot be turned into a token for another key, and it stops when the
original device is removed.

Account tokens carry the account epoch as well (`<account>:<expiry>:<epoch>`); tokens of the
first server release have no epoch field and count as epoch 0, so they stay valid until the
first password change.

### 5.4 Signalling socket (calls by number)
```
D → S  {"type":"hello","device_id":"…"}
S → D  {"type":"challenge","challenge":"…","server":"<name>","account_id":7}
D → S  {"type":"auth","signature":"<sig over T(signal)>"}
S → D  {"type":"ready","number":"UOS-…","manage":true,"recovery":null}
                                                    or {"type":"error","error":"<code>"}
```
No token and no password is involved: the socket is opened with the device key, and it serves
exactly the number of that key. An account password alone cannot receive calls (A3). The first
frame of the first release (`{"token","number"}`) is answered with `device_key_required`.

There is **one socket per number**. The last step of the sign-in – reading the device's state
again, looking up its number, entering the socket in the table – happens under the database
lock, so a removal either came before (the sign-in fails) or comes after (it finds the socket).
A new sign-in of the same device closes its previous socket (close frame, reason
`signed_in_elsewhere`). Removing or replacing a device closes its socket at once
(`{"type":"error","error":"device_removed"}`, then close). A socket relays a frame only while it
is the table's current socket for its number; a socket that lost its place is closed with its
next frame at the latest. `ready` carries the pending recovery (§5.7). Security notices (§5.8)
are pushed to the connected **managing** devices of the account – a socket gets them from the
moment its device is approved or its wait is over, without signing in again – as
`{"type":"notice","event":"…","device":"<prefix>"}`.

### 5.5 Devices of an account
| Call | Auth | Effect |
|---|---|---|
| `GET /v1/devices` | AT or DT | per device: name, number, state, day added, day last seen, `manage`, `manage_in`, public key; the limit; the pending recovery |
| `POST /v1/device/remove` (itself) | DT of that device | device → removed; its number is retired; tokens and socket die |
| `POST /v1/device/remove` (another) | DT of a **managing** device | the same, for a device of the account |
| `POST /v1/device/rename` | DT of a managing device | new name for a device of the account |
| `GET /v1/device/events` | DT of a managing device | security notices of the account |
| `POST /v1/device/approve` | DT of a managing device + its signature | an added device becomes a managing device at once |
| `POST /v1/device/recover/cancel` | DT of a managing device | the pending recovery is cancelled |

**Managing devices.** A device token alone is not "a device of the owner in hand": the account
password gives an account token, the account token registers a new key, and that registration
answers with a device token. So management rights are separate from registration:

* The first device of an account (no other device in use, legacy rows included) manages at once.
* A key added to an account that already has devices gets its own number, its own calls, may
  list the devices and remove **itself** – nothing else. It becomes a managing device
  * when a managing device approves it: `POST /v1/device/approve` with that device's token and
    its signature over the new device's public key (`ultar-device-approve-v1`, challenge of
    purpose `approve` issued for the approver's key), or
  * `ULTRAOS_DEVICE_COOLDOWN_SECS` (default 72 h) after its key was bound – the same time a
    recovery takes; a legitimate owner without any other device is not locked out for longer
    than a recovery would take, and during that time the owner's devices show the added device.
* A replacement (§5.6) inherits the standing of the old device, clock included. A finished
  recovery (§5.7) and the upgrade of an account's only legacy row give a managing device; the
  upgrade of a legacy row next to other devices gives an added device.
* Until then every management call answers `device_not_approved` (403) with `Retry-After`.

Limit: `ULTRAOS_MAX_DEVICES` devices in use per account (default 10) → `device_limit`; over its
lifetime an account may register 10 × that many keys → `device_quota_exhausted` (bounds rows and
burnt numbers, A7).

### 5.6 Replace a device (old device at hand)
```
N → S  POST /v1/device/challenge {"purpose":"replace","public_key":"<new>"}   [AT]
N → O  challenge, server, account id, new public key      (QR code / cable / Verbund; any channel)
O      shows "move number UOS-… to a new device?" and signs T(replace-old, extra = new key)
O → N  old public key, signature
N      signs T(replace-new, extra = old key)
N → S  POST /v1/device/replace   [AT]
       {"challenge_id":"…","old_device_id":"…","old_signature":"…","new_signature":"…","name":"…"}
S      both signatures over the same challenge; old device active on this account; new key unused
       → one transaction: old → replaced (epoch+1), new device inserted, number re-pointed
S → N  as registration (same number)
```
The old key authorises exactly this new key, this account, this server, once.

### 5.7 Replace a lost device (password + cool-down + the owner not objecting)
```
N → S  POST /v1/device/recover/start   [AT]
       {"password":"…","old_device_id":"…","challenge_id":"…","signature":"<T(recover start)>"}
S      password verified (Argon2id, failed-password budget §5.10), PoP of the new key,
       old device active – or legacy and no longer upgradable, see below – on this account
       → pending recovery: eligible after ULTRAOS_DEVICE_COOLDOWN_SECS (default 72 h),
         expires 7 days after that
S → N  {"eligible_in":259200}
a managing device of the account (the "lost" one included):
       POST /v1/device/recover/cancel [DT]  → cancelled, notice
N → S  POST /v1/device/recover/finish  [AT]   (after the cool-down, fresh challenge)
       {"old_device_id":"…","challenge_id":"…","signature":"<T(recover finish)>","name":"…"}
S      same new key as at start, cool-down over, not expired, not cancelled → as §5.6;
       the new device is a managing device
```
One recovery per account at a time; starting again never restarts the clock. A password change
cancels a pending recovery. Before the cool-down: `cooldown_active` with `Retry-After`. Start
and finish have different labels: a signature captured at the start cannot finish.

**How the owner learns of it.** The only thing between the password and the number is the
owner's objection, so a pending recovery is a *state*, not a message that can be missed:

* it is part of `GET /v1/devices`, of every register/session answer and of the `ready` frame of
  the signalling socket (`"recovery":{"old_device","eligible_in"}`), for as long as it lasts;
* the notice `recovery_started` is pushed to connected managing devices and is **never dropped
  from the notice list while the recovery is pending**, however many notices follow (the list
  otherwise keeps the newest 64);
* in the same way a device that was **added** and may not manage yet is a state:
  `"unapproved":[{"device","manage_in"}]` in `GET /v1/devices` and in every register/session
  answer of the account's other devices, until it is approved, removed or its wait is over; the
  phone app shows it like a recovery, with "This was not me – remove it";
* the device shows it: the call service passes notices on (`CallEvent::Notice`),
  `ultar_cloud::SecurityWatch::check` reads state and new notices at start, every 10 minutes and
  on a notice; the phone app shows a lasting banner with "This was not me" (→
  `recover/cancel`), and a new pending recovery is posted as an urgent system notification.

**Legacy rows.** A registration from before device keys has no key: no socket, no device token,
no way to be told or to cancel. Its own device can take it over with the account token it
already has (§9) for as long as such tokens live – 30 days after the migration. During that
time a recovery of a legacy row is refused (`recovery_locked` with `Retry-After`). Afterwards it
moves with password and cool-down like any other device; the pending state is still visible in
`GET /v1/devices` to any signed-in client of the account.

### 5.8 Notices
Every change (device added / removed / replaced / upgraded / approved, recovery started /
cancelled / finished, password changed) is appended to a per-account list and pushed to the
connected signalling sockets of managing devices. The newest 64 are kept; the `recovery_started`
of a pending recovery is kept on top of that. Devices are named by the first 8 characters of
their id. Reading the list needs a managing device.

### 5.9 Password change
`POST /v1/account/password [AT] {"old_password","new_password"}` → account epoch + 1 (all
account and device tokens of the account are dead), pending recovery cancelled, new account
token returned. Devices stay registered and fetch new device tokens with their keys.

### 5.10 Password checks
The password is checked in three places: login, password change, recovery start. All three are
in the strict rate class (`auth`, default 10 burst / 6 per minute per client address). Each
check is booked before the password is looked at (parallel guesses cannot exceed a budget); a
right password gives its place back. Two budgets (revised in review round 4, R4-1):

* **per account and client** (IPv4 address or IPv6 /64): `ULTRAOS_PASSWORD_FAILURES` (default
  20) failed checks within 15 minutes, then this client's checks of this account are refused
  (`rate_limited`, `Retry-After`) until the window is over. A stranger locks out only himself.
* **per account**, all clients together: the first 10 failures cost nothing; after that every
  check of the account **waits** 1, 2, 4, 8 s (8 s at most) before it runs. Only at 200 failures
  within 15 minutes – many addresses guessing together – are the account's checks refused. A
  check made with the token of a **managing device** of the account is exempt from this part
  (no wait, never refused because of others): the owner with a device in hand can always change
  the password and stop a recovery.

The tables are bounded (100 000 entries each); a full table drops its oldest entry instead of
refusing anyone. The budgets stop password use only: device keys, device tokens and calls are
not affected.

## 6. State machines

```
device:   (none) ──register──▶ ACTIVE ──remove──▶ REMOVED        (terminal, key retired)
                                  │
                                  ├─replace / recover (as old)──▶ REPLACED   (terminal, key retired)
          LEGACY ──upgrade, or recover (as old)──▶ ACTIVE under the new key (row rewritten in place)
          LEGACY ──remove──▶ REMOVED

number:   ACTIVE(key K) ──replace/recover/upgrade──▶ ACTIVE(key K')   same account, explicit flow only
          ACTIVE ──device removed──▶ RETIRED            (terminal, never issued again)

challenge: PENDING ──presented (any result)──▶ USED ──120 s after issue──▶ forgotten
           PENDING ──120 s──▶ EXPIRED

recovery: (none) ──start──▶ PENDING ──cool-down──▶ ELIGIBLE ──finish──▶ (none, number moved)
          PENDING/ELIGIBLE ──cancel | password change | 7 days after eligibility──▶ (none)

standing: FIRST device ─────────────────────────────────────────▶ MANAGING
          ADDED device ──approved by a managing device | cool-down since its key was bound──▶ MANAGING
          REPLACEMENT ── stands where the old device stood (clock included)
          RECOVERED ────────────────────────────────────────────▶ MANAGING

socket:   (none) ──sign-in──▶ CURRENT ──new sign-in of the device──▶ closed (signed_in_elsewhere)
                                 └────device removed / replaced────▶ closed (device_removed)
```
A removed or replaced key can never be registered again (`device_retired`); the device has to
create a new key and gets a new number.

## 7. What each side stores

**Server** (SQLite, schema 3):

| Table | Content |
|---|---|
| `devices` | account, device id, public key (32 bytes; NULL = legacy), state, name, token epoch, creation time, last-seen **day**, `approved` (manages at once), `key_since` (time the key was bound) |
| `numbers` | number, account, device row, state (active / retired), creation time |
| `retired_numbers` | numbers whose row was deleted (filled by a trigger, e.g. when an account is deleted) – consulted before a number is issued |
| `device_recoveries` | account, old device row, new public key, eligible / expiry time |
| `device_events` | notices (kind, 8-character id prefix, time) |
| `accounts.token_epoch`, `accounts.device_quota_used` | revocation epoch, lifetime registration counter |
| `meta` | `legacy_cutoff` – time of the migration |
| memory only | challenges; number → its one live socket; failed-password budgets (account id, count, window start) |

No hardware serial number, no model, no address is stored. Last seen is rounded down to the day
(UTC) and written at most once per day and device. Logs name a device by the first 6 characters
of its id only; public keys, challenges, signatures and tokens are never logged (A8).

**Device** (`/data/ultar`). Key, state and token files have mode 0600. Every write goes through
a temporary file with an unpredictable name (`<file>.new.<16 hex>`) that is created exclusively
– never through an existing name or link –, flushed and renamed (the key: hard-linked, so an
existing key is never replaced); temporary files older than a minute that an interrupted write
left behind are removed at the next write or key use. A missing directory is created with mode
0700; a directory that group or others may write to is refused for private files. Downloads are
written to `<file>.part` after unlinking whatever has that name. The key file is checked on
every use: regular file, owned by the calling user, no access for others, intact check line –
otherwise nothing is signed and the file is left as it is. Copies of the private key in memory
(seed, file text) are wiped after use, best effort:

| File | Content |
|---|---|
| `device.key` | `ultar-device-key-v1`, seed, public key, SHA-256 check line (file key store) |
| `device.state` | server name, account id, device id, number |
| `number` | the number (read by the shell, as before) |
| `events.seen` | id of the newest security notice the user has seen |
| `cloud.token` | account token |

`KeyStore` is a trait (`public_key`, `sign`); `FileKeyStore` is the implementation for hardware
without a secure element. A TPM / secure-element store implements the same two calls and needs no
change anywhere else – no code outside the store ever sees private key bytes.

## 8. Failure cases

| Situation | Server answer (`error`) | Device behaviour |
|---|---|---|
| challenge id never issued / forgotten after restart | `challenge_unknown` 400 | request a new challenge once, then report |
| challenge presented a second time (replay) | `challenge_used` 400 | – |
| older than 120 s | `challenge_expired` 400 | new challenge |
| issued to another account, or for another purpose | `challenge_mismatch` 400 (and consumed) | – |
| signature does not verify (wrong key, wrong server name, wrong account, altered field) | `signature_invalid` 401 | – |
| public key malformed or weak | `invalid_request` 400 | – |
| more than 8 pending challenges of an account, or 65 536 stored in total | `challenge_capacity` 429 + `Retry-After` | wait |
| management call (rename, remove another device, approve, events, recover/cancel) from an added device | `device_not_approved` 403 + `Retry-After` | have a managing device approve it, or wait |
| recovery of a legacy registration within 30 days of the migration | `recovery_locked` 409 + `Retry-After` | update that device (it upgrades in place) |
| failed-password budget of the account used up | `rate_limited` 429 + `Retry-After` | wait; devices keep working |
| request not complete within 30 s (head: 10 s) | `request_timeout` 408 / connection closed | – |
| device pointed at another server than the one it registered with | (device side) `ServerIdentityMismatch`: no call sign-in | sign in / register at that server |
| state directory writable by group or others | (device side) I/O error, nothing written | fix the directory's mode |
| key registered to another account | `device_taken` 409 | cannot happen without the private key; shown as error |
| key removed / replaced earlier | `device_retired` 410 | create a new key (`reset`), register again |
| 10 active devices | `device_limit` 409 | remove one first |
| lifetime quota used up | `device_quota_exhausted` 409 | operator |
| first-release register body for an unknown id | `device_key_required` 426 | update the system |
| legacy id owned by another account / token too new / already upgraded | `legacy_not_eligible` 403 | register as a new device (new number) |
| device token of a removed device or after a password change | `auth_required` 401 | new session challenge / sign in |
| recovery before the cool-down | `cooldown_active` 409 + `Retry-After` | wait |
| another recovery pending | `recovery_pending` 409 | cancel from a device, or wait |
| no recovery pending / other key / expired | `recovery_unknown` 404 | start again |
| wrong password in recover / password change | `invalid_credentials` 401 | – |
| server name in challenge ≠ configured server | (device side) `ServerIdentityMismatch` | nothing is signed |
| key file has wrong owner or mode, is a link, or fails its check line | (device side) `KeyStore` error | nothing is signed, nothing is overwritten |

Every failure leaves the stored state as it was; no failure path grants anything.

## 9. Migration (schema 1 → 2 → 3)

One transaction, together with the version bump:

1. `devices` is rebuilt. Every existing row becomes **LEGACY**: `public_key` NULL, the old free
   text "pubkey" is dropped, the id is stored as `legacy:<old id>` (a namespace no key id can
   reach). `numbers` is rebuilt to point at the device row; all numbers are kept.
2. `accounts` gains `token_epoch` and `device_quota_used`; `meta.legacy_cutoff` = time of the migration.
3. New tables of §7 and the trigger that retires deleted numbers.

Afterwards:

* A legacy device keeps its row and number but **cannot open the signalling socket** (it has no
  key – fail closed) and nothing new can be registered without a key.
* The first-release call `POST /v1/device/register {"device_id","pubkey"}` still answers with the
  existing number when the id is a legacy row of the caller's account (read only); for anything
  else it returns `device_key_required`. No id can be squatted any more.
* **Upgrade in place** (`POST /v1/device/upgrade`, AT): the updated device generates its key and
  presents the legacy id, a proof of possession of the new key (transcript names the legacy id)
  and its account token. Accepted only if the legacy row belongs to the token's account **and the
  token was issued before `legacy_cutoff`** (strictly earlier, to the second) – the closest verifiable form of "the token that
  registered it": a token obtained with a stolen password after the migration is not enough (A3).
  The row is rewritten to the key (state ACTIVE, new id), the number stays.
* Not eligible (`legacy_not_eligible`): the device registers as new and gets a new number. The
  old number can be moved with §5.7 (password + cool-down) once the 30 days are over in which
  old tokens can still upgrade the row; before that a recovery of a legacy row is refused
  (`recovery_locked`), because nobody could be told about it.
* **The squatting case**: Mallory registered the constant id of serial-less devices (or a
  victim's id) before the migration. That row is a legacy row of *Mallory's* account. The
  victim's device now has a key-derived id, registers normally and is no longer blocked; its
  upgrade attempt for the squatted id is refused (`legacy_not_eligible`) and changes nothing.
  Mallory cannot register the victim's key id (no private key → `signature_invalid`), and can
  create no further legacy rows.
* Several devices of one account that shared one legacy id: the first to upgrade keeps the
  number, the others register as new devices.

Residual risk, accepted and documented: a token stolen *before* the migration from the owning
account can perform the one upgrade of a legacy row. The owner's devices of that account see the
notice `device_upgraded`; with a device at hand the intruder can be removed. A legacy row that was upgraded next to other devices is an added
device (§5.5): it manages nothing for 72 h unless a managing device approves it.

Tested against a database written by the server code of schema 1 (fixture
`tests/fixtures/v1-devices.sql`, produced by running that server; see the test header).

**Schema 3** (review round 3) adds `devices.approved` (default 1) and `devices.key_since`
(default 0): devices that exist when it runs keep their management rights. Legacy rows that are
upgraded afterwards follow §5.5.

## 10. What the account password alone can do

Somebody who knows the password and has **no device of the owner** can:

| He can | What limits it |
|---|---|
| sign in, use the store and VPN of the account, read the device list (ids, names, numbers) | – (that is what the password is for) |
| register a device of his own: it gets **its own, new** number | the owner's managing devices are told (`device_added`); the added device manages nothing for 72 h and can be removed by the owner at any time; device limit and lifetime quota |
| start a recovery of one of the owner's devices | 72 h cool-down; reported as a state to every device of the owner for as long as it lasts (§5.7); any managing device stops it; a password change stops it; not for legacy rows within 30 days of the migration |
| after 72 h without the owner objecting: finish the recovery – the number moves to his device, which then manages the account | this is the designed way back for an owner who lost every device; it is the residual risk of password-only recovery |
| after 72 h: manage the account's devices with his added device | the same window, the same visibility (`manage_in` in the device list) |
| change the password (he knows the old one) | ends all tokens; the owner's devices stay registered and keep their numbers and management rights |
| guess further passwords | 20 failed checks per 15 minutes per account and client address; beyond 10 per account every check waits up to 8 s; 200 per account and 15 minutes in all; strict rate class |

He cannot, at any time: open the signalling socket of an existing number or receive its calls;
obtain a device token for a key he does not hold; remove, rename or approve devices, read the
notices or cancel a recovery before his device is approved or 72 h old; shorten a cool-down;
take over a legacy registration by upgrade (needs a token from before the migration).

The owner's side of this: a device that is online shows the alert within seconds, a device that
comes online shows it at its next sign-in (state in `ready` and in the session answer). An owner
whose devices are all offline for 72 h, or who ignores the alert, loses the number to whoever has
the password – exactly as if he had lost his devices and recovered them himself.

## 11. Limits of the server process

| Limit | Value | Where |
|---|---|---|
| request head must arrive within | 10 s, else the connection is closed | `server::HEADER_READ_TIMEOUT` |
| request (body, handler) must finish within | 30 s, else `408 request_timeout` and the connection is closed | `guard::REQUEST_TIMEOUT` |
| open connections | `ULTRAOS_MAX_CONNECTIONS` (default 4096) in all, `ULTRAOS_MAX_CONNECTIONS_PER_PEER` (default 64) per client address or /64 (trusted proxies exempt); when full, the oldest **idle** connection (no request in flight, no response being sent) is closed for the newcomer; when all are busy, the newcomer is closed | `conntable`, `server::serve` |
| body / request target | 16 KiB / 2048 bytes | `guard` |
| signalling: sockets per number | 1 | `signal::Hub` |
| challenges | 8 pending and 8 markers per account; session challenges stateless, 16 accepted per key; 65 536 stored; 120 s | `challenge`, `challenge_session` |
| failed password checks | 20 per account and client, 10 free then waits, 200 per account; 15 min | `pwguard` |
| devices | 10 in use per account, 100 keys over its lifetime | `devices` |
| notices | newest 64 per account (+ the one of a pending recovery) | `device_events` |

WebSockets and downloads are not cut by the request timeout: their answer starts in time, the
stream that follows is theirs. The reverse proxy in front has limits of its own; these hold
without it.
