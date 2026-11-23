# ultraos-server – API v1

Base URL: `https://ultar.code-craft.tech` (TLS is terminated by the reverse proxy; the server
itself speaks plain HTTP on `ULTRAOS_BIND`).

## 1. Conventions

| Topic | Rule |
|---|---|
| Auth | `Authorization: Bearer <token>`. **Account token (AT):** from register/login, valid 30 days, ended by a password change. **Device token (DT):** from device registration / `device/session`, valid 24 hours, bound to one device key, ended at once when that device is removed or replaced or the password changes. "Bearer" below accepts either; "AT" / "DT" mean exactly that kind. "none" = no token needed; a token that is sent anyway is ignored on such endpoints. |
| Device values | Public keys (Ed25519, 32 bytes), challenges (32 bytes), challenge ids (16 bytes) and signatures (64 bytes) are base64url without padding. Device ids are 32 characters `a-z2-7`; registrations from before device keys appear as `legacy:<old id>`. What is signed: `docs/DEVICE-IDENTITY.md` §4. |
| Errors | Every error is `{"error":"<code>"}` with the status below. Codes are stable keys, never display text. Error responses carry `Cache-Control: no-store`. |
| Bodies | JSON, `Content-Type: application/json`, at most 16 KiB. Request target (path + query) at most 2048 bytes. |
| Identifiers | App ids, versions, channels, architectures, editions, categories: 1–128 characters of `a-z A-Z 0-9 . _ - +`, first character a letter or digit, no `..`. Channel/arch/edition/category at most 32 characters. Anything else is `invalid_request` (query) or `not_found` (path). |
| Files | Addressed only by app id (+ version) or numeric release id. Clients never send file names or paths. |
| Validators | JSON answers of the store and update endpoints and all file downloads carry a strong `ETag`; `If-None-Match` gives `304`. |
| Ranges | File downloads accept one `Range: bytes=a-b`, `a-` or `-n` (with optional `If-Range: <etag>`) → `206` + `Content-Range`. Several ranges or other units are ignored (full `200`). A range outside the file → `416 range_not_satisfiable` + `Content-Range: bytes */<size>`. |
| Trust | Packages (`.uap`) and system images are signed by the publisher and verified **on the device**. Hashes and signatures in answers are transport information; the server is not a trust anchor. |

### Error codes

| Code | Status | Meaning |
|---|---|---|
| `auth_required` | 401 | Token missing, malformed, forged or expired (`WWW-Authenticate: Bearer`). |
| `not_entitled` | 403 | Token valid, but the account has no entitlement for this premium app / gated channel. |
| `not_found` | 404 | Unknown app, version, channel, release or route; also malformed path identifiers. |
| `rate_limited` | 429 | Per-client limit exceeded, or the failed-password budget of this client for this account is used up (`ULTRAOS_PASSWORD_FAILURES`, default 20 in 15 minutes; login, password change and recovery start share it), or the account has 200 failures in 15 minutes from all clients (not for a managing device's token); `Retry-After: <seconds>`. |
| `invalid_request` | 400 | Malformed parameter or body. |
| `payload_too_large` | 413 | Body over 16 KiB or request target over 2048 bytes. |
| `range_not_satisfiable` | 416 | Requested byte range lies outside the file. |
| `invalid_credentials` | 401 | Login: unknown user or wrong password. |
| `username_taken` | 409 | Register: name in use. |
| `device_taken` | 409 | The device key is registered to another account (or is in use and cannot take a second number). |
| `device_unknown` | 404 | No such device in this account; `session` challenge for a key that is not registered. |
| `device_retired` | 410 | The key was removed or replaced earlier; it is never registered again. |
| `device_limit` | 409 | The account has `ULTRAOS_MAX_DEVICES` devices in use (default 10). |
| `device_quota_exhausted` | 409 | The account registered 10 × the device limit keys over its lifetime. |
| `device_key_required` | 426 | Registration in the form of the first release for an id that is not a legacy registration of the caller. |
| `legacy_not_eligible` | 403 | Legacy registration unknown, owned by another account, or the account token was issued after the migration. |
| `challenge_unknown` | 400 | No challenge with this id (never issued, or forgotten after a restart). |
| `challenge_used` | 400 | The challenge was presented before (replay). |
| `challenge_expired` | 400 | Older than 120 s. |
| `challenge_mismatch` | 400 | Issued to another account or for another purpose. The challenge is used up. |
| `challenge_capacity` | 429 | More than 8 pending (not yet presented) challenges of the account; more than 16 accepted session challenges of one key within 120 s; or the store is full of pending challenges; `Retry-After`. Requesting a `session` challenge is never refused (it is stateless). |
| `device_not_approved` | 403 | Management call from a device that was added to an account with other devices and is neither approved nor past the cool-down; `Retry-After` = seconds until it manages without approval. |
| `recovery_locked` | 409 | Recovery of a registration from before device keys within 30 days of the migration; `Retry-After`. |
| `request_timeout` | 408 | The request was not complete within 30 s. The connection is closed. |
| `signature_invalid` | 401 | A signature does not verify for the claimed key and this exact request. |
| `cooldown_active` | 409 | Recovery of a lost device: the cool-down is not over; `Retry-After`. |
| `recovery_pending` | 409 | Another recovery is pending for the account. |
| `recovery_unknown` | 404 | No matching recovery (none, cancelled, expired, other key). |
| `invalid_wireguard_key` | 400 | VPN: not a 32-byte base64 key. |
| `turn_not_configured` | 503 | No TURN secret on this server. |
| `vpn_pool_exhausted` | 503 | Internal address pool 10.80.0.0/16 used up. |
| `internal` | 500 | Server-side failure (details only in the server log). |

### Rate limits (per client address; IPv6 per /64)

| Class | Endpoints | Default burst | Default refill |
|---|---|---|---|
| auth | `/v1/account/register`, `/v1/account/login`, `/v1/account/password`, `/v1/device/recover/start` (every call that checks a password) | 10 | 6 / minute |
| download | `…/download`, `/v1/update/file/…` | 30 | 20 / minute |
| general | everything else | 120 | 120 / minute |
| – | `/v1/health` | not limited | |

Configurable with `ULTRAOS_LIMIT_GENERAL|DOWNLOAD|AUTH=burst,per_minute`.

Password checks (login, password change, recovery start): per account and client address
`ULTRAOS_PASSWORD_FAILURES` failures in 15 minutes (default 20), then `rate_limited` for that
client; per account, all clients together, the first 10 failures are free, then every check
waits 1–8 s, and at 200 failures checks are refused – except with a managing device's token.
Per process: request head within 10 s, whole request within 30 s (`request_timeout`),
`ULTRAOS_MAX_CONNECTIONS` open connections (default 4096) and `ULTRAOS_MAX_CONNECTIONS_PER_PEER`
per client (default 64, trusted proxies exempt); when full, the oldest idle connection is closed
to make room.

---

## 2. Public endpoints (no account)

### GET `/v1/health`
Auth: none. → `200`, body `ok` (text).

### GET `/v1/store/catalog`
Auth: none. Paged, searchable list of apps, ordered by name.

| Query | Default | Meaning |
|---|---|---|
| `q` | – | Free text, ≤ 100 characters. Matches name and description (case-insensitive; Arabic without diacritics/tatweel, alef/yaa/taa-marbuta variants folded) or an exact file type. A leading dot searches file types only (`.xlsx`). |
| `category` | – | Category id. |
| `filetype` | – | File type, with or without dot, 1–16 letters/digits. |
| `page` | 1 | 1…100000. |
| `per_page` | 20 | 1…100. |

`200`:
```json
{"total":3,"page":1,"per_page":20,"items":[
  {"id":"com.kmcc.sheets","name":"KM Sheets","description":"…","category":"office",
   "publisher":{"name":"KM Code Craft","verified":true},
   "premium":false,"version":"1.1.0","size":200000}]}
```
There are no price fields; `premium` is the only commercial information.
Errors: `invalid_request`, `rate_limited`.

### GET `/v1/store/categories`
Auth: none. `200`: `{"categories":[{"id":"office","count":2}]}` (only categories with apps).

### GET `/v1/store/apps/{id}`
Auth: none (also for premium apps). `200`:
```json
{"id":"com.kmcc.sheets","name":"KM Sheets","description":"…","category":"office",
 "publisher":{"name":"KM Code Craft","verified":true},"premium":false,
 "version":"1.1.0","size":200000,"installed_size":600000,
 "sha256":"<64 hex>","sig":"",
 "permissions":["files","network"],"file_types":["csv","xlsx"],
 "download":"/v1/store/apps/com.kmcc.sheets/download",
 "versions":[{"version":"1.1.0","size":200000,"sha256":"…","changelog":"faster","created":1795390000},
             {"version":"1.0.0","size":17,"sha256":"…","changelog":"first release","created":1795380000}]}
```
`versions` is newest first (at most 200). `installed_size` 0 = unknown. `sig` is the detached
field of the first server release; the binding signature is inside the package.
Errors: `not_found`.

### GET `/v1/store/apps/{id}/download`
Query: `version` (optional; default = current version; allows rolling back).

| App | Auth | Result |
|---|---|---|
| free | none | `200`/`206` package bytes, `ETag: "<sha256>"`, `Accept-Ranges: bytes`, `Cache-Control: no-cache` |
| premium | Bearer + entitlement `app:<id>` | same, `Cache-Control: private, no-store` |
| entry from the first server release whose file lives elsewhere (free only) | none | `307` with `Location` |

Errors: `auth_required` (premium, no valid token), `not_entitled` (premium, no entitlement),
`not_found` (unknown app/version), `range_not_satisfiable`, `rate_limited`.

### GET `/v1/store/apps`
Auth: none. Unpaged list in the shape of the first server release (a bare JSON array, at most
500 entries): `[{"id","name","version","size","sha256","sig","url","premium"}]`. `url` is the
download location, relative to the server or absolute. New clients use `/v1/store/catalog`.

### GET `/v1/update/latest`
| Query | Default | Meaning |
|---|---|---|
| `channel` | header `x-ultar-channel` (older devices), else `stable` | Update channel. |
| `arch` | `any` | Device architecture; matches releases for it or for `any`. |
| `edition` | `any` | Device edition; matches releases for it or for `any`. |

Auth: none for open channels (default `stable`, `beta`); Bearer + entitlement
`channel:<name>` for gated channels (default `preview`, `lts`).

`200` (newest matching release by publication time):
```json
{"id":7,"channel":"stable","version":"0.7.0","arch":"aarch64","edition":"phone",
 "size":123456789,"sha256":"<64 hex>","roothash":"<hex>","sig":"<base64>",
 "url":"/v1/update/file/7"}
```
`204` (empty): no release for this device. `size` 0 / `sha256` "" and an absolute `url` occur
for releases recorded by the first server release.
Errors: `auth_required`, `not_entitled` (gated channel), `not_found` (channel does not exist),
`invalid_request`.

### GET `/v1/update/file/{id}`
`id`: decimal release id from `update/latest`. Auth as for the release's channel. Result as for
the package download (`200`/`206`/`304`, or `307` for an open-channel release whose file lives
elsewhere). Errors: `auth_required`, `not_entitled`, `not_found`, `range_not_satisfiable`.

---

## 3. Account endpoints

### POST `/v1/account/register`
Auth: none. Body `{"username":"…","password":"…"}` (username 3–64 characters, password 8–256
bytes). `200`: `{"account_id":1,"token":"…"}`.
Errors: `invalid_request`, `username_taken`, `payload_too_large`, `rate_limited`.

### POST `/v1/account/login`
Auth: none. Body as above. `200`: `{"account_id":1,"token":"…"}`.
Errors: `invalid_credentials`, `invalid_request`, `rate_limited`.

### GET `/v1/account/entitlements`
Auth: Bearer. `200`: `{"apps":["com.kmcc.pro"],"channels":["preview"]}`. Error: `auth_required`.

### POST `/v1/account/password`
Auth: Bearer. Body `{"old_password":"…","new_password":"…"}` (new password 8–256 bytes).
`200`: `{"account_id":1,"token":"<new AT>"}`. Every account token and device token issued before
stops working; a pending recovery is dropped; devices stay registered (notice `password_changed`).
Errors: `auth_required`, `invalid_credentials`, `invalid_request`, `rate_limited` (client limit of
class auth, or the account's failed-password budget).

---

## 3a. Devices (device identity, `docs/DEVICE-IDENTITY.md`)

A device is an Ed25519 key. Device id = base32(SHA-256("ultar-device-v1" ‖ public key)[0..20]).
Every step that binds or uses a key needs a signature over a fresh challenge.

### POST `/v1/device/challenge`
Auth: Bearer; none for purpose `session`. Body
`{"purpose":"register|session|upgrade|replace|recover|approve","public_key":"…"}`. `200`
(`challenge_id`: 22 characters for account challenges, 108 for the stateless `session`
challenges):
`{"challenge_id":"…","challenge":"…","server":"<server name>","account_id":7,"expires_in":120}`.
Single use, bound to purpose, account and key. `server` is `ULTRAOS_SERVER_NAME`; a device signs
only when it equals the name of the server it is configured for.
Errors: `auth_required`, `invalid_request` (purpose, malformed or weak key), `device_unknown` /
`device_retired` (`session`), `challenge_capacity`.

### POST `/v1/device/register`
Auth: Bearer. Body `{"challenge_id":"…","signature":"…","name":"…"}` – signature over the
`ultar-device-register-v1` transcript; `name` optional, ≤ 64 characters, no control characters.
`200`: `{"device_id":"…","number":"UOS-XXXX-XXXX","account_id":7,"token":"<DT>","expires_in":86400,
"manage":false,"manage_in":259200,"recovery":null,"unapproved":[]}`.
Registering an active key of the same account again returns the same number.

`manage`: the device may manage the account's other devices (rename, remove, approve, notices,
cancel a recovery). The first device of an account manages at once; a device added later only
after `POST /v1/device/approve` by a managing device or `manage_in` seconds after its key was
bound (cool-down, default 72 h). `recovery`: the pending recovery of the account,
`{"old_device":"<device id>","eligible_in":<seconds>}`, or `null`. Every answer of this shape
(register, session, upgrade, replace, recover/finish) carries these fields, and `unapproved`:
the account's other devices that were added and may not manage yet,
`[{"device":"<device id>","manage_in":<seconds>}]` – shown to the owner like a recovery.
Errors: `auth_required`, `invalid_request`, `challenge_*`, `signature_invalid`, `device_taken`,
`device_retired`, `device_limit`, `device_quota_exhausted`.

*First-release form* `{"device_id":"…","pubkey":"…"}`: `200 {"number":"…"}` only if the id is a
legacy registration of the caller's account (read only); otherwise `device_key_required`. It
creates nothing.

### POST `/v1/device/session`
Auth: none (the signature is the authentication). Body `{"challenge_id":"…","signature":"…"}` –
challenge of purpose `session`, transcript `ultar-device-session-v1`. `200` as for register.
Errors: `invalid_request`, `challenge_*`, `signature_invalid`, `device_unknown`.

### POST `/v1/device/upgrade`
Auth: AT, issued before the migration to device keys. Body
`{"legacy_device_id":"<id of the first release>","challenge_id":"…","signature":"…","name":"…"}` –
transcript `ultar-device-upgrade-v1` with the legacy id as extra data. The legacy registration
gets the key in place and keeps its number. `200` as for register.
Errors: `auth_required`, `legacy_not_eligible`, `challenge_*`, `signature_invalid`,
`device_taken`, `device_retired`, `invalid_request`.

### GET `/v1/devices`
Auth: Bearer. `200`:
`{"devices":[{"device_id":"…","name":"…","number":"UOS-…","state":"active|legacy","added":<day>,"last_seen":<day>,"current":false,"manage":true,"manage_in":0,"public_key":"…"}],"limit":10,"recovery":null,"unapproved":[]}`.
`added` / `last_seen` are Unix seconds rounded down to the day (`last_seen` 0 = unknown);
`current` marks the device of the presented device token; `manage` / `manage_in` as above
(`manage_in` and `public_key` are `null` for legacy rows); `recovery` as above.

### POST `/v1/device/rename`
Auth: DT of a managing device. Body `{"device_id":"…","name":"…"}`. `200`: `{"ok":true}`.
Errors: `auth_required`, `device_not_approved`, `device_unknown`, `invalid_request`.

### POST `/v1/device/remove`
Auth: DT. A device may always remove itself; removing another device of the account needs the
token of a managing device. Body `{"device_id":"…"}`. `200`: `{"removed":true}`. The key and its
number are retired for good, the device's tokens stop working, its signalling socket is closed.
Errors: `auth_required`, `device_not_approved`, `device_unknown`.

### POST `/v1/device/approve`
Auth: DT of a managing device. Body `{"device_id":"<added device>","challenge_id":"…","signature":"…"}`
– challenge of purpose `approve` issued for the **approver's** key; signature by the approver
over `ultar-device-approve-v1` with the public key of the approved device as extra data (the
key is in `GET /v1/devices`; its hash is the device id the user confirms). `200`:
`{"approved":true}`; the device is a managing device from now on (notice `device_approved`).
Errors: `auth_required`, `device_not_approved` (the caller itself is not managing),
`device_unknown`, `challenge_*` (`challenge_mismatch` also when the challenge was issued for
another key than the caller's), `signature_invalid`, `invalid_request`.

### POST `/v1/device/replace`
Auth: Bearer. Body
`{"challenge_id":"…","old_device_id":"…","old_signature":"…","new_signature":"…","name":"…"}` –
challenge of purpose `replace` issued for the **new** key; `old_signature` by the old key over
`ultar-device-replace-old-v1` (extra = new public key), `new_signature` by the new key over
`ultar-device-replace-new-v1` (extra = old public key). The number moves to the new key; the old
device is retired. `200` as for register.
Errors: `auth_required`, `device_unknown`, `challenge_*`, `signature_invalid`, `device_taken`,
`device_retired`, `device_quota_exhausted`, `invalid_request`.

### POST `/v1/device/recover/start` · `/cancel` · `/finish`
Replacement of a lost device. What protects the number is the owner's objection: a pending
recovery is reported to every device of the account for as long as it lasts (`recovery` in
`GET /v1/devices`, in register/session answers and in the `ready` frame of `/v1/signal`).

| Call | Auth | Body | Result |
|---|---|---|---|
| `start` | Bearer | `{"password":"…","old_device_id":"…","challenge_id":"…","signature":"…"}` (purpose `recover`, transcript `ultar-device-recover-v1`, extra = old device id) | `{"eligible_in":259200}`; notice `recovery_started`. Repeating the same request returns the remaining time. |
| `cancel` | DT of a managing device | `{}` | `{"cancelled":true}` |
| `finish` | Bearer | `{"old_device_id":"…","challenge_id":"…","signature":"…","name":"…"}` (fresh challenge of purpose `recover`, same new key, transcript `ultar-device-recover-finish-v1`, extra = old device id) | as for register; the number moves; the new device is a managing device |

Cool-down `ULTRAOS_DEVICE_COOLDOWN_SECS` (default 72 h); a recovery expires 7 days after the
cool-down; a password change cancels it. A registration from before device keys (`legacy:…`)
cannot be recovered within 30 days of the migration (`recovery_locked`): during that time its
own device upgrades it in place.
Errors: `auth_required`, `invalid_credentials`, `rate_limited` (also: failed-password budget),
`device_unknown`, `device_taken`, `device_retired`, `device_not_approved` (cancel),
`recovery_locked`, `recovery_pending`, `recovery_unknown`, `cooldown_active`, `challenge_*`,
`signature_invalid`, `invalid_request`.

### GET `/v1/device/events`
Auth: DT of a managing device. Query `after` (optional event id). `200`:
`{"events":[{"id":12,"event":"recovery_started","device":"<8 characters>","at":1795390000}]}` –
the newest 64 notices of the account – plus, on top, the `recovery_started` of a recovery that is
still pending, which is never dropped: `device_added`, `device_removed`, `device_replaced`,
`device_upgraded`, `device_approved`, `recovery_started`, `recovery_cancelled`,
`recovery_finished`, `password_changed`.
Errors: `auth_required`, `device_not_approved`, `invalid_request`.

---

## 3b. Calls and network

### GET `/v1/turn-cred`
Auth: Bearer. `200`: `{"urls":["stun:…","turn:…?transport=udp","turn:…?transport=tcp"],
"username":"<expiry>:<account>","credential":"<base64 HMAC-SHA1>","ttl":3600}`.
Errors: `auth_required`, `turn_not_configured`.

### POST `/v1/vpn/peer`
Auth: Bearer. Body `{"pubkey":"<WireGuard public key, base64>"}`. `200`:
`{"server_pubkey":"…","endpoint":"host:51820","address":"10.80.x.y/32","dns":"10.80.0.1",
"allowed_ips":"0.0.0.0/0, ::/0"}`; the same key always gets the same address.
Errors: `auth_required`, `invalid_wireguard_key`, `vpn_pool_exhausted`.

### GET `/v1/signal` (WebSocket)
Auth: the device key, in three text frames within 10 s each:

1. device → `{"type":"hello","device_id":"…"}`
2. server → `{"type":"challenge","challenge":"…","server":"<server name>","account_id":7}`
3. device → `{"type":"auth","signature":"…"}` (transcript `ultar-device-signal-v1`)

Server answers `{"type":"ready","number":"UOS-…","manage":true,"recovery":null}` (the number of
that key; `manage` and `recovery` as in the registration answer) or
`{"type":"error","error":"auth"}`. No token or password opens this socket; the frame of the
first release (`{"token":"…","number":"…"}`) is answered with
`{"type":"error","error":"device_key_required"}`.
Then: `{"to":"UOS-…","type":"offer|answer|ice|bye","data":…}` is forwarded to that number with
`"from"` set by the server; an offline or overloaded target yields
`{"type":"unreachable","to":"…"}`. Messages ≤ 64 KiB, queue 256 messages per socket.

One socket per number: when the device signs in again, its previous socket is closed (close
frame, code 1008, reason `signed_in_elsewhere`). When the device is removed or replaced the
server sends `{"type":"error","error":"device_removed"}` and closes. A socket that is no longer
the current one of its number relays nothing. To managing devices the server sends
`{"type":"notice","event":"…","device":"<8 characters>"}` (security notices of the account).

---

## 4. Ultar-Verbund (unchanged protocol, German field names)

| Method | Path | Auth | Purpose |
|---|---|---|---|
| POST | `/v1/verbund/geraet` | Bearer | Body `{"pk":"<64 hex>","name":"…","ausgabe":"…"}` → `{"ok":true}`. `400` invalid key, `409` key belongs to another account (body `{"error":"…"}` with the text of the first release). |
| POST | `/v1/verbund/geraet/entfernen` | Bearer | Body `{"pk":"…"}` → `{"entfernt":0|1}`. |
| GET | `/v1/verbund/geraete` | Bearer | `{"geraete":[{"pk","name","ausgabe","zuletzt","online"}]}`. |
| GET (WS) | `/v1/verbund/relais` | first frame `{"token","pk","name","ausgabe","rolle","kerne","frei"}` | Relay of binary frames `[type:1][pk:32][session:8][payload]` between devices of one account; content is Noise-encrypted end to end. Frames ≤ 70 KiB, 512 frames buffered per connection, 64 connections per account. |
| GET | `/v1/verbund/statistik` | none | `{"verbindungen":n,"rahmen":n,"verworfen":n}`. |

Missing/invalid token on the three HTTP endpoints: `401` with an empty body (as in the first
release).

---

## 5. Administration (no HTTP interface)

Entitlements, channels, publishers and publications are managed with the `ultraos-server admin`
command on the server host (see DEPLOY.md §9). There is deliberately no administrative endpoint.

## 6. Changes against the first server release

| Before | Now |
|---|---|
| `/v1/store/apps`, `/v1/update/latest` needed a token | public; token optional |
| Channel only via header `x-ultar-channel` | query `channel`, `arch`, `edition`; the header still works |
| Errors partly without body or with German text | always `{"error":"<code>"}` (Verbund endpoints unchanged) |
| Invalid JSON body: `415`/`422` from the framework | `400 invalid_request` |
| A device id could be re-registered by another account (returned the first account's number) | `409 device_taken` |
| No limits | rate limits, body limit 16 KiB (was 2 MiB), URI limit |
| Device id = any string the device sent; first sender owned it | device id derived from the device's Ed25519 key; registration, sessions and the signalling socket need a signature over a fresh challenge (§3a) |
| `/v1/signal` opened with account token + number | opened with the device key only |
| Tokens could not be revoked; no password change | account and device epochs: password change, device removal and replacement end tokens at once |
| No limit of devices per account | `ULTRAOS_MAX_DEVICES` (default 10) and a lifetime quota |
| (review round 3) a device registered with the password could manage the account's devices at once | management rights only for the first device, an approved device, or after the cool-down (`manage`, `POST /v1/device/approve`, `device_not_approved`) |
| (review round 3) recovery start and finish shared one label | finish signs `ultar-device-recover-finish-v1` |
| (review round 3) a recovery was only a notice | `recovery` state in device list, session answers and `ready` |
| (review round 3) no time limit for an unfinished request, no connection cap | 10 s / 30 s, `ULTRAOS_MAX_CONNECTIONS` |
| (review round 4) one stranger's wrong passwords locked the owner out | budget per account **and client**; per account only waits, refused at 200; managing device exempt |
| (review round 4) session challenges were stored and could be pushed out | stateless session challenges (HMAC-authenticated id), consumed only by a verified signature |
| (review round 4) one client could hold every connection | per-client cap; idle connections give way |
