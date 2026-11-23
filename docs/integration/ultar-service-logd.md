# Service runtime + log service – integration notes (wave 3, agent "service")

Closes review block **B – services** (`docs/review/OFFENE-BEFUNDE.md`): no daemon handled stop signals, no log
service, the broker could not identify its peer, retained topics were lost after a broker restart, daemons had no
limits. Also closes the minor findings 7 (settingsd `--policy` + `--allow-all`), 18 (`devices.shutdown` for every
caller) and "do not blindly replace a file at the socket path".

Patch: `CHANGES.patch` (`patch -p1` in the repo root). It **removes** `system/core/ultar-bus/src/bin/ultar-busd.rs`
(the binary moved, see below).

## 1. What exists

| Crate | Path | Content |
|---|---|---|
| `ultar-service` (new, lib; test helper bin `ultar-service-probe` under `tests/helpers/`) | `system/core/ultar-service` | runtime of every daemon |
| `ultar-sys` (new, lib) | `system/hal/ultar-sys` | safe wrappers for the system calls `std` lacks |
| `ultar-logd` (new, lib + bins `ultar-logd`, `ulog`) | `system/services/ultar-logd` | log service |
| `ultar-busd` (new, bin `ultar-busd`) | `system/core/ultar-busd` | the broker daemon, moved out of `ultar-bus` |
| `ultar-bus` (changed) | `system/core/ultar-bus` | observer, socket access, stale-path rule, generation API, log contract |
| `ultar-settingsd`, `ultar-devd` (changed) | `system/core/…` | wired to the runtime |
| service files + policy (new) | `rootfs/etc/ultar/services/{logd,bus,settingsd,devd}.toml`, `rootfs/etc/ultar/bus-policy.toml` | |

**Why `ultar-busd` is its own crate:** the daemon needs `ultar-service`, and `ultar-service` needs `ultar-bus`
(peer identity, bus session). A binary inside the `ultar-bus` package would make the package depend on itself.

### To do by the coordinator

1. Root `Cargo.toml`, `members`: add `"system/hal/ultar-sys"`, `"system/core/ultar-service"`,
   `"system/core/ultar-busd"`, `"system/services/ultar-logd"`. (Not in the patch: several agents add members.) The three new `Cargo.toml` have
   no `[workspace]` table, like the other integrated crates.
2. Image build: install `ultar-busd`, `ultar-logd`, `ulog` (and `ultar-settingsd`, `usettings`, `ultar-devd`,
   `udevctl`) to `/usr/bin`; create `/data/ultar/{log,settings,devd}`. `/run/ultar/bus` is created by `ultar-busd`.
3. `tools/budgets.toml`: `ultar-busd` is now 0.49 MB (budget 0.7 MB still holds); add `ultar-logd` (0.63 MB) and
   `ulog` (0.47 MB) if wanted.
4. `docs/integration/ultar-bus.md`, `ultar-settingsd.md`, `ultar-devd-daemon.md` still show the old command lines
   (I did not edit other agents' notes); `docs/review/OFFENE-BEFUNDE.md` row B can move to "fixed" with the limits
   listed in §7.

## 2. `ultar-service` – public API

```rust
// main() of a daemon
let runtime = Runtime::start(ServiceSpec { name, hardening, stop_deadline, log })?;   // main thread, first thing
exit::code(runtime.run(|runtime| {          // work on a 2nd thread; main thread reads the signals
    runtime.ready();                        // state Ready, READY to the supervisor, log record
    runtime.lifecycle().wait_stop();        // or wait_change(reloads) -> Wake::{Stop(cause), Reload(n)}
    exit::OK
}))
```

| Module | API |
|---|---|
| `signal` | `MainSource::open()`, `watch(source, &lifecycle, &config)`, `SignalSource` (trait, scripted in tests) |
| `lifecycle` | `Lifecycle::{state, set_ready, request_stop(StopCause), mark_stopped, on_stop, on_reload, reload, wait_for, wait_for_timeout, wait_stop, wait_change}`, `State::{Starting, Ready, Stopping, Stopped}` |
| `ready` | service: `ReadyNotifier::{from_env, notify}`; supervisor: `ready_pipe()`, `ReadyReader::{next_event, until_ready}`, `wait_ready(reader, timeout)`, `READY_FD_ENV` |
| `harden` / `rights` | `Hardening { close_descriptors, keep_descriptors, umask, working_directory, allow_root, no_new_privileges, limits, rights }` (default: all on), `Limits`, `RightSet`, `apply() -> HardeningReport` |
| `peercred` | `KernelCredentials` (`impl ultar_bus::CredentialSource`), `UidMap::{parse, load, into_peer_identity, identify}`, `peer_credentials(&UnixStream)`, `probe()` |
| `session` | `BusSession::{connect, register, publish_retained, remember, remember_raw, client, republished, close}` |
| `secret` | `Secret` (no `Display`, no `Clone`, `Debug` = `Secret(<redacted>)`, wiped on drop), `read_secret_file`, `read_secret_fd`, `constant_time_eq` |
| `socket` | `bind_listener(path, mode)` (replaces only a dead **socket**), `ensure_parent` |
| `log` | `Logger::{start, emit, log, flush, stats}`, `Record::new(level, key).count/.signed/.flag/.word/.id/.id_or_invalid`, `ShortId`, `wire::{encode_frame, decode_body, FrameReader}`, `TokenBucket` |
| `exit` | `OK 0, FAILURE 1, USAGE 2, PANIC 70, HARDENING 71, DEADLINE 72, FORCED 73` |

### Signals – mechanism and why

No C library, no signal handler: `SIGTERM`/`SIGINT`/`SIGHUP` are blocked with `rt_sigprocmask` before any thread
exists and read as data from a `signalfd` (via `ppoll` together with a wake pipe). First stop signal → stop hooks
+ deadline watchdog (default 2.5 s, below the 3 s of `ultar-init`; exit 72); second stop signal → exit 73 at once;
`SIGHUP` → reload hooks.

**`unsafe`:** none in `ultar-service`, `ultar-busd`, `ultar-logd` (all `#![forbid(unsafe_code)]`). The system
calls live in the HAL crate **`system/hal/ultar-sys`** (FIX1; registered in `tools/check-layers.toml` as an allowed
core → HAL dependency). It uses `rustix` 1.1 (already in the lock, `linux_raw` backend, no C library) wherever that
has a safe function: `getrlimit/setrlimit`, `umask`, `get*id`, `poll`, `set_no_new_privs`. Its one file with
`unsafe`, `src/raw.rs` (every block with `// SAFETY:`), holds what `rustix` cannot do: `rt_sigprocmask` +
`signalfd4` (`rustix` has **no `signalfd`**, `sigprocmask` only in its unsupported `runtime` feature),
`getsockopt(SO_PEERCRED)` (`rustix::net::sockopt::socket_peercred` **panics** on the 4-byte answer of the Ultar
kernel and would build a non-zero pid type from zero – the fail-closed check needs the raw call), the two Ultar
`prctl` right options, and the three functions on inherited descriptor *numbers* (`is_open`, `adopt_fd`,
`close_inherited`; number → descriptor is `unsafe` in `std` and `rustix` alike).

**Kernel finding that shaped the design:** `kernel/ultar-kern/src/signal.rs::send_locked` marks a signal that all
threads block pending on the **first thread of the group only**, and `read`/`poll` on a `signalfd` look at the
calling thread's pending set (`syscall.rs`, `Src::SigFd`). A dedicated signal thread would never see a signal on the
Ultar kernel. The runtime therefore reads on the **main thread** and runs the service's work on a second thread
(a test checks that the main thread sits in `ppoll`). Works unchanged on Linux.

### Readiness contract (for `ultar-init`; `ultar-init` was not modified)

Supervisor creates a pipe (`ready_pipe()`), hands the write end to the child as descriptor `N ≥ 3`
(`dup2` in the existing `pre_exec`) and sets `ULTAR_READY_FD=N`. The service writes ASCII lines `READY <pid>`,
`STOPPING`, `STOPPED <code>` (≤ 64 bytes, unknown lines ignored up to 16, then error). End of file before `READY`
= the service died during start. Supervisor side: `wait_ready(reader, timeout) -> (pid, ReadyReader)`.
Without the variable a service simply does not notify. `ultar-settingsd` and `ultar-logd` additionally print one
"ready" line on standard output (as settingsd did before).

### Hardening at start (default ON, each step can only be turned off in the service's own code)

take the declared inherited descriptors (`ServiceSpec::inherited`, type-checked pipe/file, one owner) and close
all others – before the signal descriptor exists (R3-S2) → umask 077 → cwd `/` → refuse root unless declared
(`--allow-root`) → `PR_SET_NO_NEW_PRIVS` → `RLIMIT_CORE 0`, `RLIMIT_NOFILE`, `RLIMIT_NPROC` (no `RLIMIT_AS` any
more, R3-B0: memory is bounded by the group's `memory.max`)
(set and **read back**) → drop every Ultar kernel right the service does not declare (`prctl 0x554c5201`, read back
with `0x554c5200`; the mask is `!keep`, so rights a later kernel adds are dropped too) → panic hook (logs
`service.panic` with the source line, never the message; exit 70).

| Daemon | AS | NOFILE | NPROC | rights |
|---|---|---|---|---|
| busd | – | 1024 | 1024 | none |
| logd | – | 256 | 256 | none |
| settingsd, devd | – | 256 | 128 | none |

Paths given to a daemon must be absolute (cwd is `/`).

## 3. What the Ultar kernel does not provide yet (found by reading `kernel/ultar-kern`, state 23.11.2026)

| Mechanism | Kernel today | Effect | Needed kernel change |
|---|---|---|---|
| `SO_PEERCRED` | `sys_getsockopt` knows `SO_ERROR`/`SO_TYPE` only, answers any other option with 4 zero bytes; no user ids (`getuid` = 0) | `peercred::peer_credentials` detects it (length ≠ 12 or pid 0) and fails → a credential listener refuses every peer; `ultar-busd`/`ultar-logd` refuse to start with one (`probe()`), message tells to use fixed sockets | record `{pid, uid/cell id, gid}` of the connecting group in `UnixConn` at `connect`/`socketpair`, return 12-byte `struct ucred`; needs an identity per cell first |
| resource limits | `sys_prlimit(res, old)` ignores the new value | limits reported `Unsupported` in the `HardeningReport` and logged (`service.hardening.unsupported`), service still starts | real `prlimit64` set (belongs to K5 / groups with limits) |
| refuse root | every process is uid 0 | step `Unsupported`; the rights drop is the privilege model there | – |
| blocked signal pending per process | pending on first thread only | handled by design (main thread reads) | optional: group-wide pending set |
| `ppoll` timeout | only whole seconds are honoured (`tv_nsec` ignored) | not used (the runtime polls without timeout) | – |

**Fallback identity, as shipped in the service files:** one socket per identity with mode 0600
(`--listen PATH=IDENTITY`), plus `--listen-token` with a token file for cases where sockets cannot be separated.
Until cells mount exactly their own socket, every root process can reach every socket – the separation is real only
with cells or user ids.

**Not run on the Ultar kernel and not built for aarch64** (system call numbers for aarch64 are from the generic
table; the `asm!` block is untested there). Everything was run on x86_64 Linux 6.x only.

## 4. `ultar-logd`

* **Ingestion:** Unix socket, frames `u16 len | body` (`ultar_service::log::wire`, strict decoder, fuzzed). The
  source name is never in a record: it is the identity of the connection (kernel credentials → `--uid-map`, the same
  file format as the bus: `1001 = "system.settingsd"`), or fixed per socket. One reader thread per connection,
  ≤ 128 connections, ≤ 4 per source.
* **Types instead of convention:** a record is `key` + ≤ 8 fields; keys/field names/words are `&'static str`,
  values are `u64 | i64 | bool | &'static str | ShortId`. `ShortId` = ≤ 32 chars `a-z0-9._:-`, starts with a letter,
  no run of > 6 digits; since R3-W1 keys and words on the wire obey the same digit-run rule. Four `compile_fail` doc tests show that a `String` cannot get in; the decoder enforces the
  same rules on the receiving side. Maximum body 1024 bytes.
* **Rate limit per source:** token bucket (default 200 + 50/s), applied at the socket (a flood never fills the
  shared queue) and again in the core; refused records are counted and announced by one `log.dropped` record
  (`server=N`) when records pass again. The client has its own bucket and marker (`rate`, `buffer`).
* **Bounds:** ring ≤ `ring_records` and ≤ `ring_bytes`; one segment ≤ `segment_bytes`; all segments ≤ `total_bytes`
  (room for a whole segment is made *before* a new one starts); age limit. Head file: 80 bytes.
* **Hash chain:** `link(n) = H(link(n-1) || len || payload)`, segment `seed = H(prev_link || header)`, SHA-256 or
  HMAC-SHA-256 with `--chain-key-file`. `ulog verify DIR [--key-file F]` detects a changed, removed, inserted,
  duplicated or swapped record, a removed middle/last segment, a cut tail (through the authenticated head file),
  a changed header.
* **Disk full / broken:** a failed write is cut back, the store becomes *degraded* (records stay in the ring, are
  counted, `log.persist_failed`), retries every 64 records in a **new segment** whose header declares the gap;
  `verify` reports declared gaps separately from damage. Clients are never blocked: the logging API only appends to
  a bounded queue.
* **Reading:** bus service `log` (`ultar_bus::messages::log`): `query {service, min_level, since_ms, until_ms,
  after_seq, limit ≤ 256, wait_ms ≤ 30 s}` (long poll = follow, ≤ 16 waiting readers), `stats`. An identity sees
  records with its own identity as source; the call rule `log.read_all` shows all sources and the kernel's text.
  `ulog query|follow|stats|verify|export`.
* **Kernel log:** separate source `kernel` (`SourceKind::Kernel`), read from `--kernel-log` at start, on `SIGHUP`
  and before an administrator's query (at most once per second; the kernel has no event interface). Its lines are
  free text: only for `log.read_all`, and withheld from exports unless `--include-kernel-text` (the export's
  redaction report counts them).
* **Service without logd:** `Logger` keeps the newest 256 records (bounded by count; ≤ 1026 bytes each), drops the
  oldest, reconnects with back-off 0.1 s … 5 s only while records wait.

## 5. Changed daemons

| Daemon | Changes |
|---|---|
| `ultar-busd` | runtime; `--listen PATH=ID`, `--listen-cred PATH` + `--uid-map`, `--listen-token PATH` + `--token-file`; `--mode/--owner/--group` per listener; `--log-socket`; `--allow-root`; `--policy` with `--allow-all` = usage error; SIGTERM → clients told, sockets removed, exit 0; SIGHUP → policy re-read (faulty file keeps the old one); logs `bus.refused`, `bus.limit_hit`, `bus.slow_consumer_dropped`, `bus.auth_failed`, `bus.protocol_error`; never removes a non-socket |
| `ultar-settingsd` | runtime; SIGTERM/SIGINT flush pending writes, exit 0; `--token` **removed** → `--token-file` / `--token-fd`; `--log-socket`; `--allow-root`; `--policy` + `--allow-all` = error (was: allow-all won silently); `settings.store_report` re-published after broker restart; `usettings --token` → `--token-file` |
| `ultar-devd` | runtime; SIGTERM/SIGINT clean stop, exit 0; **`--policy FILE` or `--allow-all` now required** (not with `--once`); `devices.admin` needed for `switch`, `rollback`, `clear_quarantine`, `shutdown` (refused before decoding, logged as `devd.admin_refused`); `--token-file`/`--token-fd`, `--log-socket`, `--allow-root`; `devices.capabilities` re-published after broker restart |

`ultar-bus` library additions: `BrokerBuilder::{observer, listen_with_access}`, `BrokerObserver`, `BrokerEvent`,
`SocketAccess`, `BusError::NotASocket`, `Client::{generation, wait_reconnected}`, `messages::log`,
`device_service::ADMIN_METHOD`; `Debug` of `ClientOptions` and `TokenIdentity` no longer prints tokens.

Operator lines that stay on standard error: usage/start errors of all daemons, the `--allow-all` warning, the
"listening"/"stopped" lines of busd and the two status lines of devd (an existing test pins them). Everything that
happens while running goes through the logging API only.

### Existing tests I had to change (all others untouched)

* `ultar-settingsd/src/daemon.rs` tests (moved to `daemon_tests.rs` for the 500-line rule): `--token tk` →
  `--token-file`; the assertion "`--allow-all` wins over `--policy`" became "is `PolicyConflict`" (that *is*
  finding 7). `src/cli.rs` test `options`: `--token` → `--token-file`.
* `ultar-settingsd/tests/e2e.rs`, `ultar-devd/tests/e2e_tools.rs`: the helpers that spawn the daemon add
  `--allow-root` when the tests run as root, and devd's adds `--allow-all`. No assertion changed.
* `ultar-bus`: the 5 argument tests of the old binary moved to `ultar-busd/src/args.rs` (and grew).

## 6. Gate (x86_64 Linux, rustc 1.97, 2 shared cores, `-j1`)

`cargo fmt --check` clean · `cargo clippy --all-targets -- -D warnings` clean (6 crates) · `cargo doc -D warnings`
clean · `ultar-check all --no-baseline` clean for these paths (after FIX1) · `cargo test` **three times in a row: 644 passed, 0 failed, 3 ignored each time** (after FIX1, with `ultar-sys`: 646, run once) (ignored = the three
existing benchmarks). The tests ran as root; the root-specific branches are noted in the tests.

| Crate | Tests | of which new |
|---|---|---|
| `ultar-service` | 109 (74 unit, 16 process, 6 bus, 4 fuzz, 9 doc incl. 6 `compile_fail`) | 109 |
| `ultar-busd` | 31 (16 unit incl. service files + policy, 13 + 2 with the real binary) | 26 |
| `ultar-logd` | 79 (39 unit, 6 bounds, 12 tamper, 5 disk-full, 4 query/fuzz, 12 real binaries, 1 service file) | 79 |
| `ultar-bus` | 142 (+1 ignored) – all existing suites pass | 7 unit |
| `ultar-settingsd` | 174 (+1 ignored) – all existing suites pass | 9 process tests (`tests/runtime.rs`) |
| `ultar-devd` | 109 (+1 ignored) – all existing suites pass | 7 process tests (`tests/runtime.rs`), access + argument unit tests |

Requested tests and where they are: signals end-to-end with real binaries (`*/tests/runtime*.rs`,
`ultar-busd/tests/daemon.rs`, `ultar-logd/tests/daemon_tools.rs`: SIGTERM → exit 0, socket removed, data flushed;
SIGKILL → restart → recovery) · limits read back (`/proc/<pid>/limits` of busd, logd, settingsd, devd; probe) ·
peer identity mapped / unmapped refused (`ultar-busd/tests/daemon.rs`, `ultar-service/tests/bus_identity_session.rs`)
· rate limit (`bounds.rs`, `log/tests.rs`) · 1e6-record flood inside every bound (`bounds.rs`, 32–52 s in a debug
build) · every single-record change and removal detected, keyed and unkeyed (`tamper.rs`) · disk full (`disk_full.rs`)
· personal data cannot be put into a record (`compile_fail` doc tests in `log/record.rs`) · wire fuzz, fixed seeds
(`log_wire_fuzz.rs`, 250 000 inputs; segment reader 25 000 inputs) · broker restart → retained topics back
(busd killed with SIGKILL; settingsd and devd binaries with an in-process broker).

Measured (release, `opt-level = "s"`, LTO, stripped): `ultar-busd` 487 kB, `ultar-logd` 628 kB, `ulog` 466 kB,
`ultar-service-probe` 363 kB; static-pie (`+crt-static`, glibc) builds: 1.45 / 1.60 / 1.43 MB.

The fuzz of the segment reader found one real gap during development: the segment header's creation time was not
covered by the chain. Fixed (segment seed), test added.

## 7. Not done / not tested – honest list

* **Ultar kernel and aarch64: not run** (see §3). The design decisions rest on reading the kernel source.
* **`ultar-init` is unchanged**: it does not consume the readiness pipe (dependants still start when the process
  exists, the daemons retry the bus for 10–30 s), cannot switch users (so the service files carry `--allow-root`
  and all sockets are 0600), and appends the daemons' standard error to `/run/ultar/log/<name>.log` without bound
  (little is written there now).
* **Chain key:** no key file is shipped or generated; without `--chain-key-file` the chain detects damage and naive
  edits, not an attacker who recomputes all links. Removing the *oldest* segments looks like retention (the report
  shows the first position).
* **Bus tokens** end up as `String` inside `ultar_bus::ClientOptions` (not wiped); `Secret` wiping is best effort
  (`black_box`), not `mlock`ed.
* `RLIMIT_NPROC` does nothing for root; the cgroup `pids.max` in the service files is the effective bound there.
* `ultar-logd`: the policy is read at start only (no reload); bus queries return intact records only; a torn tail
  larger than one record is treated as damage (kept, new segment); the rate bucket at the socket restarts when the
  last connection of a source closes (the core's bucket does not).
* `ultar-devd` declares no kernel right; when the kernel gets the bind/unbind interface (finding D1) it must
  declare `LoadDriver`. `--once` runs without hardening (tool mode).
* Review minors **not** addressed (outside the task): per-identity bus limits for retained topics and service
  names, settingsd second-instance lock and confirmation details, devd index serial number / key revocation.
* Operator messages of `ultar-busd` come from the bus crate's table; `ulog verify` prints problem details as
  identifiers (`problem=broken_chain …`), not as translated sentences.
* Test hygiene: the process tests of the new crates leave their scratch directories in the system temp directory
  (`usvc-*`, `ubusd-*`, `ulogd-*`); they are unique per run and never reused. `ultar-service-probe` is a test
  helper (source under `tests/helpers/`); its `--panic` option is the one deliberate `panic!`.

## 8. Review round 3 (`docs/review/services-runde-3.md`) – FIX2

| Finding | Fix |
|---|---|
| R3-B0 | Broker: one **gate thread per listener** accepts and runs every handshake non-blocking (`poll`, `ultar-sys`); threads only after handshake + admission; at most `max_handshakes` (16) per listener in the handshake, more wait in the kernel's listen queue; a thread that cannot start refuses that connection (`LimitExceeded`). No `RLIMIT_AS` in any daemon. |
| R3-B1 | One descriptor per connection (`Arc<UnixStream>` shared by reader, writer, control). Per-listener bound `--max-connections N` (default 64; shipped: 8/8/8, admin 32). `BrokerBuilder::needs()` + `busd` start check (descriptors and threads incl. reserve ≤ 1024) – refused with `LimitsTooSmall`; `service_files_tests.rs` checks the sum against `pids.max` (now 256). |
| R3-K1 (runtime) | `busd` reads every fixed-identity socket back after bind (socket node, mode, owner, group); a mismatch (Ultar kernel today: no node) logs `bus.identity.unenforced` (warn, `listener=N`) – detected, not assumed. Inherited-descriptor option for every secret: `busd --token-fd`, `ultar-logd --chain-key-fd/--bus-token-fd`, `ulog --key-fd/--token-fd`, `usettings --token-fd` (settingsd/devd had `--token-fd`). Kernel side open (kernel agent). |
| R3-L1 | Size eviction carries records of other sources forward (not the largest consumer; within total/8 per source; warn+ first; ≤ half a segment per eviction; field `carried` = original position, original time kept). |
| R3-L3 | Head written before the first segment; segments without head = `head_missing` at `ulog verify` and `log.head_missing` at start; invalid head = `log.head_invalid`. |
| R3-S1 | `SessionOptions::stop`: connect/register end at once on stop; settingsd, devd, logd exit 0. |
| R3-S2 | `adopt_fd`/`close_inherited` no longer public; `ultar_sys::take_inherited` (one-shot) in `Runtime::start`; `Runtime::read_secret(SecretSource, max)`; undeclared/non-pipe/non-file descriptor = start error `service.start.descriptor_refused` (exit 71). |
| R3 guard `a_flooding_source_does_not_starve_another_source` (failed under load, FIX3) | Fair admission to the engine queue (`ultar-logd/src/share.rs`): each source may have at most `IngestLimits::share` (32) records waiting; at its share a source's reader waits on its own connection. Shares of all sources fit the queue (128 × 32 = 4096, compile-time check). Deterministic test `tests/fair_queue.rs` stalls the engine. |
| R3-B3 | Token table ≤ 256 KiB, ≤ 1024 entries. |
| R3-L4 | `clean_text` replaces bidi controls (U+061C, 200B–200F, 202A–202E, 2060–2069, FEFF). |
| R3-W1 | Wire keys/words: digit run ≤ 6. The review's `review3_logd.rs` constant `LONG` had a run of 10 and was adapted (run of 6). |
| R3-P1 | `SIGHUP` reads the policy again in logd (waiting readers re-scoped), settingsd and devd (`Reloadable` access); faulty file keeps the old one, logged. |

Open: kernel side of R3-K1 (connect permission check, socket nodes, real file modes); refusing `system.admin`
on a path visible to cells; `ultar-bus` now depends on `ultar-sys` (Cargo.lock entry of `ultar-bus` changes).
