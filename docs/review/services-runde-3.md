# Review round 3 – service runtime, broker daemon, log service (commit 157060b)

Reviewer role: security/services, independent. Repo read-only; work on copies; x86_64 Linux, tests run as root,
debug build. Nothing was run on the Ultar kernel or aarch64 – statements about the Ultar kernel rest on reading
`kernel/ultar-kern/src` and are marked "read".

**Verdict: not ready for production.** One critical defect (any process that can reach a bus socket ends the broker,
before authentication), six major, four minor. Block B is functionally closed (signals, log service, retained topics,
limits exist), but the limits of the broker contradict each other and the identity
fallback for the Ultar kernel does not exist in practice.

Regression tests (fail on 157060b, `cargo fmt` clean, clippy `-D warnings` clean, functions <= 60 lines), under
`/home/claude/agents/r3/service/regression/` with their target paths:

| File (target path in the repo) | Tests |
|---|---|
| `system/core/ultar-busd/tests/review3_busd.rs` | R3-B0, R3-B1, R3-B3 fail; 1 guard passes |
| `system/services/ultar-logd/tests/review3_logd.rs` | R3-L1, R3-L3, R3-L4 fail; 1 guard passes |
| `system/core/ultar-settingsd/tests/review3_settingsd.rs` | R3-S1, R3-S2 fail |

## Critical

### R3-B0 – silent connections end `ultar-busd` (pre-authentication, exit 70)
* Where: `system/core/ultar-busd/src/main.rs:54-59` (RLIMIT_AS 1 GiB), `system/core/ultar-bus/src/broker/mod.rs:398-427`
  (one thread per accepted connection before the handshake), `broker/conn.rs:175-181` (second thread),
  `system/core/ultar-service/src/runtime.rs` panic hook (`process::exit(70)`).
* What: every connection costs threads. Under the broker's own address-space limit the C library's per-thread arenas
  fill the address space after ~20 threads (measured: VmSize 207 MB with 4 threads, 999 MB with 24); from there
  each connection adds ~276 kB and after 190-300 connections a thread start fails. If the failing allocation is the
  new thread's signal stack, `std` panics (`stack_overflow.rs:236`), the hook ends the process with code 70. No
  handshake, identity or policy rule is needed; 256 connections are inside the listener's own bound.
* Effect: all sessions and retained topics gone, sockets left behind, `ultar-init` restarts with back-off up to
  30 s (`supervisor.rs:391`) – repeatable, so the bus can be kept down. On the Ultar kernel the socket path is free
  after the crash and any process can bind it (see R3-K1).
* Reproduction: test `silent_connections_inside_the_listener_bound_must_not_end_the_broker` (6/6 failing runs;
  passes with `MALLOC_ARENA_MAX=1`, which confirms the cause).
* Fix: do not spawn per connection before the handshake is paid for (one acceptor doing the handshake with a
  deadline, or a small fixed pool); make limits consistent: connections x threads x stack + arenas < RLIMIT_AS,
  or drop RLIMIT_AS in favour of the cgroup `memory.max`; cap arenas (static musl build has none); treat a failed
  thread start as "refuse this connection", never as panic.

## Major

### R3-B1 – per-listener bounds exceed the broker's descriptor limit: other sockets locked out
* Where: `broker/mod.rs:66` (256 per listener), `broker/conn.rs:137,175` (3 descriptors per connection),
  `ultar-busd/src/main.rs:56` (NOFILE 1024), `rootfs/etc/ultar/services/bus.toml` (4 listeners).
* What: 4 x 256 x 3 = 3072 descriptors against 1024. With the arena problem masked, two identities with 337
  handshaken connections in total (each inside its bound) use up all descriptors; the admin socket then accepts
  nobody. "One socket cannot starve the others" (`mod.rs:386`) holds for slots, not for descriptors or threads
  (`pids.max = 1100` in the service file: 2 threads per connection).
* Reproduction: test `two_identities_inside_their_bounds_must_not_lock_out_the_admin_socket` (today it already
  fails at ~125 connections because of R3-B0; with `MALLOC_ARENA_MAX=1` at 337).
* Fix: one descriptor per connection (no `try_clone`), per-identity bounds that sum below the process limits,
  a reserve for the admin listener; test the sum in `service_files_tests.rs`.

### R3-K1 – on the Ultar kernel neither identity mechanism works; the shipped setup is open, and says nothing (read)
* `kernel/ultar-kern/src/syscall.rs:2157-2178`: `connect` on a Unix socket checks no permission at all;
  `syscall.rs:375`: `chmod`/`chown` (90-94, 260, 268) return 0 without doing anything; `sys_bind` (2362-2376) creates
  no file. So `--mode 0600` is reported as applied and means nothing; whoever sees the path is the identity – including
  `system.sock = system.admin` (settings.admin, devices.admin incl. shutdown, log.read_all with kernel text).
* The documented fallback, the token file, cannot be loaded there: the kernel reports every regular file as mode
  `0o100755` (`syscall.rs:1145,1163`), and `read_secret_file` refuses any group/other bit
  (`ultar-service/src/secret.rs:213`) -> `TooPermissive`. Same for `--token-file` of settingsd/devd/logd/ulog and for
  `--chain-key-file` (no keyed chain on the target kernel). It fails closed, but the fallback does not exist.
* A bearer token is also sent to whoever listens on the path (`Hello.token`); after a broker crash (R3-B0) any
  process can bind the path on the Ultar kernel and collect tokens.
* INTEGRATION.md §3 names "every root process can reach every socket"; the daemons log only
  `service.hardening.unsupported step=refuse_root`. Nothing at start says that bus identities are not enforced.
* Sound part: `SO_PEERCRED` detection is fail closed (4 zero bytes -> `ENOPROTOOPT`, `probe()` stops the start);
  limits are reported `Unsupported` correctly (kernel answers fixed values, read-back differs for all four); the
  rights drop is real and read back.
* Fix: kernel – credentials per connection and permission check in `connect` (or sockets only reachable through
  the cell's mount); runtime – on `Platform::Ultar` log a warning-level record `bus.identity.unenforced` per fixed
  listener and refuse `system.admin` on a path visible to cells; secrets on Ultar through `--token-fd`/inherited
  descriptor for every secret (busd token table and chain key have no fd option today).

### R3-L1 – one source inside its rate limit evicts everybody's records in about 11 minutes
* Where: `ultar-logd/src/store/mod.rs:297` (one pool, oldest segment goes), `core.rs` (rate is the only per-source
  bound), ring `core.rs` (2048 records: 41 s at 50/s).
* What: 50 records/s of ~540 bytes fill 16 MiB in ~10.3 min. A compromised or broken service erases the
  broker's `bus.auth_failed`/`bus.refused` records and its own traces without exceeding any limit; "30 days" is
  not a property any source has.
* Reproduction: test `one_source_inside_its_rate_limit_must_not_evict_another_sources_records`.
* Fix: per-source byte budget (share of `total_bytes`, evict the largest consumer's oldest records first, or one
  segment series per source), a protected class for level >= warn of other sources, a daily byte quota per source.

### R3-L3 – a cut tail is not detected when the head file is deleted with it (keyed chain too)
* Where: `ultar-logd/src/verify.rs:197-206` (`check_head`: missing head = no problem); the next start writes a
  new head.
* Reproduction: test `a_removed_tail_is_detected_also_when_the_head_file_is_removed_with_it` – 300 records,
  last segment and `head` removed -> `ulog verify`: intact, 252 records.
* Fix: a log with segments and no valid head is a finding (`head_missing`), at `verify` and at start (log
  `log.tail_missing`); with a key, keep the head's counter somewhere the log directory's writer cannot reach, or
  export it periodically.

### R3-S2 – `adopt_fd` hands out a second owner of a live descriptor (`--token-fd`)
* Where: `system/hal/ultar-sys/src/raw.rs:231-250` (safe function, `OwnedFd::from_raw_fd` on any open number),
  callers `ultar-settingsd/src/daemon.rs:331`, `ultar-devd/src/runner.rs:257` – on the work thread, after the
  runtime opened its own descriptors. The SAFETY comment assumes "adopts during start-up, before it opens
  anything"; that is not what happens.
* Reproduction: `ultar-settingsd … --token-fd 3` without passing a descriptor: 3 is the runtime's `signalfd`; the
  daemon reads its token from it, never becomes ready, swallows SIGTERM and has to be killed. Test
  `a_token_descriptor_that_was_not_inherited_is_refused_at_start`.
* Fix: declare inherited descriptors in `ServiceSpec`; `Runtime::start` adopts them before `MainSource::open` and
  returns the owners; check the type (`fstat`: pipe or regular file); make the raw adopt non-public.

### R3-S1 – stop signals are ignored while a daemon waits for the broker at start
* Where: `ultar-settingsd/src/daemon.rs:376-378` (10 s), `ultar-devd/src/runner.rs:28` (30 s),
  `ultar-service/src/session.rs:168-178` (`connect` does not look at the lifecycle).
* Measured: SIGTERM 0.4 s after start, no broker: settingsd and devd end after 2.5 s with code 72 (deadline),
  logd ends at once with 0. Every stop during boot/shutdown/broker back-off is a "failed" stop and costs 2.5 s,
  0.5 s below `ultar-init`'s kill.
* Reproduction: test `sigterm_while_waiting_for_the_broker_stops_at_once_and_cleanly`.
* Fix: `BusSession::connect` takes the lifecycle and returns on stop; exit code 0.

## Minor

* **R3-B3** token file limited to 4096 bytes in total (`ultar-busd/src/tokens.rs:105-107` reads it as one
  `Secret`, `secret.rs:32`): ~60 clients, then the broker does not start. Test
  `a_token_file_for_a_hundred_clients_is_accepted`. Fix: own bounded reader (e.g. 256 KiB, 1024 entries).
* **R3-L4** direction overrides (U+202A-202E, U+2066-2069) pass `clean_text` (`ultar-logd/src/record.rs:104-110`,
  `format.rs:86`) into `ulog` output of kernel text; control characters and escapes are removed correctly. Test
  `direction_overrides_in_kernel_text_do_not_reach_the_terminal`.
* **R3-W1** the wire decoder cannot enforce "static text only": keys and words accept any 48 characters of
  `a-z0-9._-` and unlimited digit runs (`ultar-service/src/log/record.rs:137-145`; the 6-digit rule holds for
  `ShortId` only). A non-conforming client can store e.g. phone numbers as words. INTEGRATION.md §4 says the
  decoder enforces the same rules. Fix: digit-run rule for words/keys on the wire, or an allow-list of keys per
  source. No test (design statement).
* **R3-P1** policy reloads reach the broker only: logd, settingsd and devd keep the policy read at start
  (`log.read_all`, `settings.admin`, `devices.admin`); revoking an administrator needs a restart of three
  services (INTEGRATION.md names it for logd only). Second stop signal = exit 73 with socket files left and no
  flush: `kill -TERM` followed by `-INT` is enough (documented behaviour; stale sockets are replaced at start).
* Not confirmed, worth a look: RLIMIT_AS leaves logd ~34 MB head-room (VmSize 477 of 512 MiB at 18 threads, same
  arena effect as R3-B0; a failed allocation aborts without the panic hook). `fsync` (calls 74/75) returns 0
  without work on the Ultar kernel (`syscall.rs:375`, read) – the durability statements of logd are unproven
  there. Queue bound 8 MiB per connection against `memory.max = 64M` holds only because frames are shared and
  the shipped policy has 6 publishable topics.

## Verified as sound

* `ultar-sys`: system call numbers and argument order for both architectures against the kernel's dispatcher
  (x86_64 only could be run); sizes (8-byte signal set, 12-byte `ucred`, 128-byte siginfo); `check()` range;
  `SO_PEERCRED` fail-closed logic; EINTR repeated in `wait_readable`; signals blocked before the first thread in
  all four daemons; main thread reads the `signalfd` (matches `signal.rs::send_locked`).
* Signals: 300 x SIGHUP then SIGTERM -> busd exit 0, sockets removed; SIGTERM with 8 half-open peers -> exit 0,
  sockets removed (guard test); logd SIGTERM under flood -> exit 0 in 15 ms, `ulog verify` intact.
* Hardening on the Ultar kernel (read): the four limits come out as `Unsupported`, not as falsely applied;
  detection of the kernel cannot be faked on Linux.
* Shipped files: no `--allow-all`, no `--token`; `--policy` + `--allow-all` is an error in all four daemons;
  policy is default deny, no wildcard identity; only `system.logd` may register `log`.
* Identity: the source name of a log record and the bus identity never come from the peer's data; server
  assigns sequence and time; a token listener without token file / credential listener without map does not start.
* logd: a flooding source (4 connections, full speed) does not starve another source – 100/100 records stored
  (guard test); 900 connection attempts in 6 rounds and 300 random-byte connections: bounded threads, no crash;
  stored records are validated again when read from disk; control characters cannot reach `ulog` output.
* Broker fan-out memory: 24 non-reading subscribers x 7 MiB queued -> 11.5 MB resident (frames shared).
* Existing suites of `ultar-busd` pass here (16 unit, 13 daemon, 2 resilience). The suites of `ultar-sys`,
  `ultar-service`, `ultar-logd`, `ultar-settingsd`, `ultar-devd` were NOT re-run by me (the run stopped at my
  first failing test file and was not repeated).

## Not covered

Ultar kernel and aarch64 runs; devd internals beyond start/stop; rotation races and disk-full (the authors' suites
only, not attacked again); timing of token comparison; `ultar-init` readiness pipe (not consumed yet).
