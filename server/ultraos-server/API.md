# ultraos-server – API v1

Base URL: `https://ultar.code-craft.tech` (TLS is terminated by the reverse proxy; the server
itself speaks plain HTTP on `ULTRAOS_BIND`).

## 1. Conventions

| Topic | Rule |
|---|---|
| Auth | `Authorization: Bearer <token>`; the token comes from register/login and is valid 30 days. "none" = no token needed; a token that is sent anyway is ignored on such endpoints. |
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
| `rate_limited` | 429 | Per-client limit exceeded; `Retry-After: <seconds>`. |
| `invalid_request` | 400 | Malformed parameter or body. |
| `payload_too_large` | 413 | Body over 16 KiB or request target over 2048 bytes. |
| `range_not_satisfiable` | 416 | Requested byte range lies outside the file. |
| `invalid_credentials` | 401 | Login: unknown user or wrong password. |
| `username_taken` | 409 | Register: name in use. |
| `device_taken` | 409 | Device id is registered to another account. |
| `invalid_wireguard_key` | 400 | VPN: not a 32-byte base64 key. |
| `turn_not_configured` | 503 | No TURN secret on this server. |
| `vpn_pool_exhausted` | 503 | Internal address pool 10.80.0.0/16 used up. |
| `internal` | 500 | Server-side failure (details only in the server log). |

### Rate limits (per client address; IPv6 per /64)

| Class | Endpoints | Default burst | Default refill |
|---|---|---|---|
| auth | `/v1/account/register`, `/v1/account/login` | 10 | 6 / minute |
| download | `…/download`, `/v1/update/file/…` | 30 | 20 / minute |
| general | everything else | 120 | 120 / minute |
| – | `/v1/health` | not limited | |

Configurable with `ULTRAOS_LIMIT_GENERAL|DOWNLOAD|AUTH=burst,per_minute`.

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

### POST `/v1/device/register`
Auth: Bearer. Body `{"device_id":"…","pubkey":"…"}` (device id 1–128 bytes, key ≤ 512 bytes).
`200`: `{"number":"UOS-XXXX-XXXX"}`; repeated calls for the same device return the same number.
Errors: `auth_required`, `device_taken`, `invalid_request`.

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
Auth: first text frame `{"token":"…","number":"UOS-…"}` within 10 s; the number must belong to
the token's account. Server answers `{"type":"ready"}` or `{"type":"error","error":"auth"}`.
Then: `{"to":"UOS-…","type":"offer|answer|ice|bye","data":…}` is forwarded to that number with
`"from"` set by the server; an offline or overloaded target yields
`{"type":"unreachable","to":"…"}`. Messages ≤ 64 KiB, queue 256 messages per socket.

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
