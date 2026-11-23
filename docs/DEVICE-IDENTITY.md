# Device identity (finding S15)

Status: implemented in `server/ultraos-server` (schema 2) and `system/services/ultar-cloud`.
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
| A3 | Thief of the account password (no device) | receive calls of an existing number; take a number over without delay and without the owner's devices being told |
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
| recover (lost device) | `ultar-device-recover-v1` | new key | id of the lost device (UTF-8) |

No label is a prefix of another once the `0x00` is appended; every variable field carries its
length, fixed fields have fixed sizes. A signature made for one purpose, account, server, key or
challenge does not verify for any other (A2).

**Server name**: the authority of the server's public address, lower case, default port dropped
(`ultar.code-craft.tech`, in tests `127.0.0.1:41234`). The server has it configured
(`ULTRAOS_SERVER_NAME`); the device derives it from its own configured server address and refuses
to sign when the name in a challenge differs (A6: a signature obtained by server M names M and is
useless at S).

**Challenge**: 32 random bytes from the OS generator, valid 120 s, single use, bound at issue
time to (purpose, account, claimed public key). It is addressed by a separate random 16-byte
`challenge_id`; the challenge bytes themselves are never used as a lookup key and are compared
only inside the signature check. The first presentation consumes it, whatever the outcome.
Pending challenges are kept in memory only: at most 8 per account or key, 65 536 in total;
beyond that the request is refused (`challenge_capacity`) – nothing is evicted early (A7).
A restart forgets all pending challenges (fail closed).

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
S → D  {"device_id":"…","number":"UOS-…","account_id":7,"token":"<DT>","expires_in":86400}
```
All checks and writes of one registration run in one database transaction; two concurrent
registrations of one key end with one device row and one number.

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
S → D  {"type":"ready","number":"UOS-…"}            or {"type":"error","error":"<code>"}
```
No token and no password is involved: the socket is opened with the device key, and it serves
exactly the number of that key. An account password alone cannot receive calls (A3). The first
frame of the first release (`{"token","number"}`) is answered with `device_key_required`.
Removing or replacing a device closes its socket at once. Security notices (§5.8) are pushed to
connected sockets of the account as `{"type":"notice","event":"…","device":"<prefix>"}`.

### 5.5 Devices of an account
| Call | Auth | Effect |
|---|---|---|
| `GET /v1/devices` | AT or DT | name, number, state, day added, day last seen, limit |
| `POST /v1/device/rename` | DT | new name for a device of the account |
| `POST /v1/device/remove` | DT | device → removed; its number is retired; tokens and socket die |
| `GET /v1/device/events` | DT | security notices of the account |

Changes need a device token, i.e. a registered device in hand: a password thief cannot remove
devices or retire numbers. Limit: `ULTRAOS_MAX_DEVICES` active devices per account (default 10)
→ `device_limit`; over its lifetime an account may register 10 × that many keys →
`device_quota_exhausted` (bounds rows and burnt numbers, A7).

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

### 5.7 Replace a lost device (password + cool-down + notification)
```
N → S  POST /v1/device/recover/start   [AT]
       {"password":"…","old_device_id":"…","challenge_id":"…","signature":"<T(recover)>"}
S      password verified (Argon2id), PoP of the new key, old device active or legacy on this account
       → pending recovery: eligible after ULTRAOS_DEVICE_COOLDOWN_SECS (default 72 h),
         expires 7 days after that; notice "recovery_started" to all devices of the account
S → N  {"eligible_in":259200}
any device of the account (also the "lost" one):
       POST /v1/device/recover/cancel [DT]  → cancelled, notice
N → S  POST /v1/device/recover/finish  [AT]   (after the cool-down, fresh challenge)
       {"old_device_id":"…","challenge_id":"…","signature":"<T(recover)>","name":"…"}
S      same new key as at start, cool-down over, not expired, not cancelled → as §5.6
```
One recovery per account at a time; starting again never restarts the clock. A password change
cancels a pending recovery. Before the cool-down: `cooldown_active` with `Retry-After`.

### 5.8 Notices
Every change (device added / removed / replaced / upgraded, recovery started / cancelled /
finished, password changed) is appended to a per-account list (newest 64 kept) and pushed to
connected signalling sockets. Devices are named by the first 8 characters of their id.

### 5.9 Password change
`POST /v1/account/password [AT] {"old_password","new_password"}` → account epoch + 1 (all
account and device tokens of the account are dead), pending recovery cancelled, new account
token returned. Devices stay registered and fetch new device tokens with their keys.

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
```
A removed or replaced key can never be registered again (`device_retired`); the device has to
create a new key and gets a new number.

## 7. What each side stores

**Server** (SQLite, schema 2):

| Table | Content |
|---|---|
| `devices` | account, device id, public key (32 bytes; NULL = legacy), state, name, token epoch, creation time, last-seen **day** |
| `numbers` | number, account, device row, state (active / retired), creation time |
| `retired_numbers` | numbers whose row was deleted (filled by a trigger, e.g. when an account is deleted) – consulted before a number is issued |
| `device_recoveries` | account, old device row, new public key, eligible / expiry time |
| `device_events` | notices (kind, 8-character id prefix, time) |
| `accounts.token_epoch`, `accounts.device_quota_used` | revocation epoch, lifetime registration counter |
| `meta` | `legacy_cutoff` – time of the migration |
| memory only | pending challenges; number → live socket |

No hardware serial number, no model, no address is stored. Last seen is rounded down to the day
(UTC) and written at most once per day and device. Logs name a device by the first 6 characters
of its id only; public keys, challenges, signatures and tokens are never logged (A8).

**Device** (`/data/ultar`; key and state files have mode 0600 and are written atomically; the
key file is checked on every use: regular file, owned by the calling user, no access for others,
intact check line – otherwise nothing is signed and the file is left as it is):

| File | Content |
|---|---|
| `device.key` | `ultar-device-key-v1`, seed, public key, SHA-256 check line (file key store) |
| `device.state` | server name, account id, device id, number |
| `number` | the number (read by the shell, as before) |

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
| too many pending challenges | `challenge_capacity` 429 + `Retry-After` | wait |
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

## 9. Migration (schema 1 → 2)

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
* Not eligible (`legacy_not_eligible`): the device registers as new and gets a new number; the
  owner can still move the old number with §5.7 (password + cool-down + notice).
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
notice `device_upgraded`; with a device at hand the intruder can be removed.

Tested against a database written by the server code of schema 1 (fixture
`tests/fixtures/v1-devices.sql`, produced by running that server; see the test header).
