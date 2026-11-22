# Server agent – integration notes (store and updates without an account)

Date: 2026-11-23. Implements DESKTOP-SPEC §12.5: **store and system updates work without an Ultar
account; premium apps and gated update channels need a token and an entitlement.**

Nothing in `/home/claude/ultar-os-v2` or `/home/claude/ultar-os` was changed. Everything is in
`/home/claude/agents/server/`.

## 1. What is here

| Directory | Origin | State |
|---|---|---|
| `ultraos-server/` | copy of `server/ultraos-server` | reworked: library + binary, v0.7.0 |
| `ultar-cloud/` | copy of `system/services/ultar-cloud` | rewritten (typed errors, signed-out store/updates, locales) |
| `ultar-pkg/` | copy of `system/core/ultar-pkg` | **two additions only** (`install_at`, `valid_id`) + tests; rest unchanged |
| `e2e/` | new, test-only crate | device client against the real server |
| `deps/ultar-vpn`, `deps/ultar-sprache` | copies | only `Cargo.toml` made standalone |
| `deps/ultar-i18n` | copy | unchanged |

Path dependencies to fix on integration: `ultar-cloud` → `../deps/ultar-vpn`, `../ultar-pkg`,
`../deps/ultar-i18n`; `ultar-pkg` → `../deps/ultar-sprache`; `e2e` → all three plus
`../ultraos-server`. Every crate has an empty `[workspace]` table to remove.

## 2. Server (`ultraos-server`)

Documents: `API.md` (every endpoint), `DEPLOY.md` (operation, migration, admin commands).

### Modules
| Module | Purpose |
|---|---|
| `lib.rs` | `Config`, `AppState::new(config, clock)`, `router(state)`, `serve(listener, state, shutdown)` |
| `store_api`, `catalog` | public catalogue, categories, app details, package download |
| `updates` | channels, `update/latest`, image download |
| `entitlements` | token check, `require_entitlement` (→ `auth_required` / `not_entitled`), grant/revoke |
| `admin`, `cli`, `msg` | publish app/release, channel, publisher, premium switch, grant/revoke; `ultraos-server admin …`; ar/en/de texts |
| `files`, `conditional` | content-addressed file store (atomic import), delivery with `ETag`/`Range` |
| `guard`, `ratelimit`, `clock` | request guard (URI limit, per-client token bucket, access log), injectable clock |
| `db`, `migrate` | SQLite, in-place migration schema 0 → 1 |
| `accounts`, `signal`, `verbund`, `auth` | endpoints of the first release (kept) |
| `error` | `ApiError` → `{"error":"<code>"}` |

### Decisions made while implementing (please confirm or overrule)
1. **Channels:** `stable` and `beta` open, `preview` and `lts` gated; channels that already had
   releases in an existing database stay open. Unknown channel → `not_found`.
2. **No administrative HTTP interface.** Entitlements and publishing only through the local
   command (smaller attack surface). A payment system would call `admin::grant`/`revoke`.
3. **Premium only for files in the server's file store.** Old rows whose file lives at an
   external URL are redirected (307) and cannot be made premium (`admin premium … on` refuses).
4. **"Newest release" = latest publication time**, not a version comparison.
5. **Error bodies are codes now** (`username_taken`, `invalid_credentials` …) instead of German
   text; statuses unchanged. Verbund endpoints keep their old bodies.
6. **Fixes to existing behaviour:** a device id registered to another account is refused
   (`device_taken`; before, the other account's number was returned); WireGuard pool exhaustion is
   an error instead of address reuse; a malformed `ULTRAOS_SECRET` stops the start instead of
   silently using a random key; signalling got a 10 s login timeout, a 64 KiB message limit and a
   bounded queue (256) instead of an unbounded one; a newer socket of the same number is no
   longer removed from the hub when the older one closes; password hashing runs off the async
   workers; logs go to stderr and never fail a request (found by test: with a closed stderr the
   logger's fallback panicked inside request handling).
7. **New endpoint beyond the brief:** `GET /v1/account/entitlements`, so the store can show
   "unlocked" without trial downloads.
8. Body limit 16 KiB (was the framework default of 2 MiB).

### Migration
`PRAGMA user_version` 0 → 1 in one transaction. Tested against `tests/fixtures/old-v0.sqlite`,
which was produced by **building and running the original server code** (v0.1.0 from the repo):
account, device, number, WireGuard peer and Verbund device created through its HTTP API; two
store apps and four releases inserted with SQL. Also tested from the old schema statements
(unit tests), with a half-applied column, twice in a row, and against a newer file (refused,
untouched).

## 3. Device side (`ultar-cloud`)

```rust
let cloud = ultar_cloud::client();                 // stored server address; token only if present
let page  = cloud.catalog(&CatalogQuery { text: ".xlsx".into(), ..Default::default() })?;
let app   = cloud.app("com.kmcc.sheets")?;         // permissions, sizes, versions, premium
let mine  = cloud.entitlements().ok();             // None when signed out
let state = AppAccess::of(&app.summary, mine.as_ref());   // Free · PremiumSignInRequired · PremiumNotIncluded · Unlocked
cloud.install_app("com.kmcc.sheets", work_dir, Path::new(ultar_pkg::TRUSTED_DIR))?;
let rel   = cloud.latest_update("stable", "aarch64", "phone")?;   // Option<Release>
// errors:
let text = messages::describe(&translator, &err, Subject::App);   // "Sign in to get this app."
```
- `CloudError` variants: `AuthRequired`, `NotEntitled`, `NotFound`, `RateLimited{retry_after_secs}`,
  `InvalidRequest`, `InvalidCredentials`, `UsernameTaken`, `Server{status}`, `Transport`,
  `InvalidResponse`, `Io`, `Integrity`, `Package`, `NotSignedIn`, `Vpn`.
  `message_key(Subject::{App, UpdateChannel, General})` → keys in `locales/{ar,en,de}.ftl`
  (38 keys each, ultar-i18n format, checked with `ultar_i18n::check_dir`); register them with
  `messages::register(&mut translator)`.
- Installation order: details → download to `<file>.part` (size + SHA-256) → `ultar_pkg::verify`
  against the trusted keys → signed manifest id must equal the requested id →
  `ultar_pkg::install_at`. Any failure: nothing installed, download removed.
- The token is sent only to the configured server; redirects are followed by hand (one hop)
  without the token when they leave it.
- Token file and WireGuard peer file are written atomically with mode 0600.
- **API change:** the free functions (`register`, `login`, `store_list`, `store_install`,
  `update_check`, `register_number`, `vpn_provision`) now return `CloudError` instead of `String`;
  `store_list`/`store_install`/`update_check` no longer need a token. `base_url()`, `token()`,
  `my_number()` are unchanged (these are what `ultar-shell` uses). `ucloud` got `app <id>` and
  `install <id>` and prints through message keys.

`ultar-pkg` additions: `install_at(uap, trusted_dir, root)` (`install` = root `/`) and
`valid_id` (a signed manifest may not choose a directory outside the app area).

## 4. Tests (run three times, identical results; `cargo 1.97.0`, `-j1`)

`cargo fmt --check` clean and `cargo clippy --all-targets -- -D warnings` clean for all four crates.

| Crate | Target | Tests |
|---|---|---|
| ultraos-server | unit (`src`) | 78 |
| | `tests/public_store.rs` – anonymous catalogue, search, details, free download, Range, ETag | 9 |
| | `tests/entitlements.rs` – premium and gated channel: none → `auth_required`, token → `not_entitled`, granted → ok, revoked → `not_entitled`; forged/expired tokens | 9 |
| | `tests/abuse.rs` – rate limit hit + recovery (manual clock), per client/class, spoofed header, 20 traversal targets, size limits, damaged file | 8 |
| | `tests/migration.rs` – database of the old server | 5 |
| | `tests/accounts.rs` – register → token → number → VPN peer, login, TURN, signalling and Verbund relay over WebSocket | 7 |
| | `tests/binary.rs` – real process: admin commands, exit codes, limits from the environment | 4 |
| | **total** | **120 passed, 0 failed** |
| ultar-cloud | unit | **19 passed** |
| ultar-pkg | `tests/install.rs` | **5 passed** |
| e2e | device client ↔ real server (7) + token scope (1) | **8 passed** |

Integration tests start the real server in-process on an ephemeral port and talk over TCP; the
binary tests spawn the built executable. No sleeps; rate-limit time comes from `ManualClock`.

**Do the tests have teeth?** Six protections were removed one at a time and the suites re-run:
premium gate, entitlement lookup, rate limiter, channel gate, token-only-to-own-server,
manifest-id check – each made 1–4 tests fail. One mutation was **not** caught by the integration
tests: switching off identifier validation (`valid_ident`). Traversal is still blocked then,
because files are found only through the database and are named by SHA-256; the validation is a
second layer covered by unit tests only.

### Measured (release build, loopback, shared 2-core machine, `curl` per request incl. process start)
| Request | Result |
|---|---|
| Catalogue page, 5 000 apps | median 4.2 ms, p95 5.5 ms |
| Search (`q=`), 5 000 apps | median 6.1 ms, p95 7.3 ms (substring scan, grows linearly) |
| App details | median 0.4 ms |
| Download 200 MiB | 0.16 s (≈ 1.3 GB/s), memory peak of the process 10.4 MB |
| Range, last 1 MiB of 200 MiB | 1.2 ms |
| Binary size | 6.3 MB |

## 5. Threat review of the public endpoints

This is a developer's review, **not an audit**. No external party has tested this code.

### What an anonymous attacker can do
- **Read everything public:** the whole catalogue including which apps are premium, descriptions,
  permissions, version history and file hashes; every free package in every published version
  (the `version` parameter also serves old, possibly vulnerable versions); every image of the
  open channels.
- **Learn what exists behind the gates:** a gated channel or gated release answers `401`, a
  missing one `404`. Names of gated channels and ids of gated releases are therefore
  discoverable (not their content or metadata).
- **Use bandwidth:** limits count requests, not bytes. One address gets 30 downloads at once and
  20 per minute; many addresses multiply that. Slow readers hold a connection open; the server
  has no per-client connection cap and no write timeout of its own.
- **Degrade the rate limiter for newcomers:** more than 50 000 active client keys within a
  window push new clients into one shared bucket per class. Known clients keep their own.
- **Hit legitimate users behind one address:** carrier-grade NAT puts many devices behind one
  IPv4 address; they share one budget and may see `rate_limited` earlier. Limits are settable.
- **Create accounts and guess passwords:** registration needs no proof (10 at once, then 6 per
  minute per address; each costs an Argon2 computation). There is no lockout per account, only
  the per-address limit. Whether a username exists can be found out (register answers
  `username_taken`; login answers faster for unknown names).
- **Read relay counters** at `/v1/verbund/statistik`.

### What an anonymous attacker cannot do (each point has a test)
- Get a premium package, a gated image or gated release metadata – not with `Range`, validators,
  the `version` parameter, the old listing, file digests as ids, or forged, expired or
  foreign-key tokens.
- Read any file outside the file store or the database file (20 traversal variants).
- Change SQL (parameters everywhere; `LIKE` wildcards escaped).
- Grant itself an entitlement (no such interface over HTTP).
- Pick its own rate-limit bucket with `X-Forwarded-For` (believed only from configured proxies,
  right-most entry).
- Cause personal data to be stored: public endpoints write nothing; client addresses live only
  in memory and are dropped when the bucket is full again (≤ window + 10 s); logs hold no
  address, username or search text.

### What someone who controls the server (or the connection) can and cannot do
- **Cannot** make a device install an unsigned or re-signed package, or a genuine package of a
  different app under another id (tested end to end). Images: the client hands out root hash and
  signature; verification is the updater's job (not part of this work).
- **Can** withhold updates, and can offer an **older, genuinely signed version** – the device does
  not yet refuse downgrades. Can lie in catalogue metadata; in particular the "verified
  publisher" badge is the server's statement, not derived from the signing key.
- With the token key (`ULTRAOS_SECRET`) every session can be forged. Tokens are stateless for
  30 days and cannot be revoked one by one. Entitlement revocation, in contrast, is immediate.

### To do before production
1. TLS termination with the adjusted Caddy configuration, verified on a staging host (header and
   idle timeouts, body limit, `Range`/`ETag` and WebSockets through the proxy). If a CDN sits in
   front: forward the real client address and close the origin to everything else (DEPLOY.md §4).
2. Backups of database and file store, off-site, with a tested restore.
3. Monitoring and alerting: health, 5xx and 429 rates, disk space, certificate expiry.
4. An external penetration test (authentication, signalling, relay, public endpoints).
5. Bandwidth and connection limits per client at the proxy or firewall.
6. Per-account login throttling or lockout; a way to end sessions (password change, logout).
7. Downgrade protection on the device (refuse versions older than the installed one).
8. Load test with real image sizes and many concurrent downloads.

## 6. Not done / not tested (honest list)
- **No payment logic and no price fields** (as specified). No endpoint for account deletion or
  password change (§12.1) – outside this task.
- **Deployment files not executed here:** `deploy/ultraos.service`, both Caddyfiles,
  `docker-compose.yml`, `Dockerfile`, `install.sh` and the new `deploy/ultraos-admin` wrapper
  were edited/written but could not be run (no systemd service, no Caddy; Docker not tried).
- **Device side:** no resume of interrupted downloads (the server supports `Range`; the client
  starts over); no updater that verifies and applies an image (download + hash only);
  `update_check` sends the compile-time architecture and edition `any`; offline/USB store source
  and signed catalogue (§7.3) not built; no store UI. The free functions in `device.rs`
  (`register`, `store_install` …) are thin wrappers over tested `Client` methods but were not
  executed themselves, because they read and write fixed paths under `/data/ultar` and `/run/ultar`.
  `ucloud` has no tests.
- **Existing code left as it was:** `verbund.rs` keeps German identifiers, JSON fields and error
  texts (protocol in the field); its relay loop was split into functions and has one forwarding /
  isolation test and one refusal test, no load or chaos test. `ultar-pkg` (apart from the two
  additions), `ultar-vpn` and `ultar-sprache` keep German identifiers and `String` errors.
  `ultar-cloud::device_id` still reads `/proc/cmdline` and a `/sys/…` path directly (rule: HAL only).
- **Not pure Rust / static:** the server uses bundled SQLite (C) via `rusqlite`; `ultar-cloud`
  uses `ring` through rustls. Both were dependencies before.
- **Scaling limits:** one SQLite connection behind a mutex, used from async handlers; search is
  a substring scan; rate-limit state is per process (no sharing between several instances).
- **Timeouts:** no server-side read/write timeouts for HTTP (left to the proxy).
- **Token comparison:** HMAC verification uses the constant-time `verify_slice`; timing was not
  measured.
- **Texts:** Arabic and German wording written by the agent, not reviewed by a native speaker.
- **Not audited.** See §5.
