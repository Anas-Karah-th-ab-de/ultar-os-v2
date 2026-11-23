# ultar-init on the service runtime – integration notes (wave 4, task I1)

Patch: `CHANGES.patch` (`patch -p1` against repo commit a5eb384). `ultar-init` keeps its PID-1 duties
(mounts, modules, firmware helper, emergency exit, reaping, never ending on its own) and now runs the
services itself the way the service runtime expects: confined, in order, with readiness, logged.

## What changed

| Where | Change |
|---|---|
| `system/core/ultar-init` | rewritten service manager (library + `ultar-init` + `uinit`); boot modules (`boot`, `diag`, `hw`, `rescue`, `fwhelper`) unchanged except two clippy fixes and the old `/sys/fs/cgroup/ultar` subtree |
| `system/hal/ultar-sys` | `switch_identity` (groups, gid, uid; read back), `new_session`, `set_controlling_terminal`, `set_inherited`, `set_signal_mask`; module `machine` (console, group root, uptime, kernel arguments) |
| `system/core/ultar-service` | `log::output` (the output channel: frame format, checks, fuzz test); `socket::bind_listener_for` (hands a socket to a group before its mode is set) |
| `system/services/ultar-logd` | `--listen-output PATH` (lines of services from init, stored as source kind `output`, free text like kernel lines: cleaned, limited per service, shown to `log.read_all` only, withheld by `ulog export` unless asked); `--group GID` per listener |
| `system/core/ultar-bus` | contract `init.status` / `init.control` (`messages/init.rs`); `SourceKind::Output` in the log contract |
| `rootfs/etc/ultar/services/{logd,bus,settingsd,devd}.toml` | own users, no `--allow-root`, sockets per group (`0660`), `notify`, `dirs`; bus listener `init.sock` for init |
| `rootfs/etc/ultar/services/*.toml` (the others) | identity and the narrowest rights each one uses, every right and every root justified in the file (table "Identities and rights") |
| `rootfs/etc/ultar/bus-policy.toml` | `system.init` registers `init`; `system.admin` may call `init.*` |
| `rootfs/usr/lib/ultar/selftest.sh` | group path `system/heartbeat` |
| `system/core/ultar-busd`, `-settingsd`, `-devd`, `ultar-logd` tests | the shipped-definition tests now require: own user, no root, readiness, sockets per group; sum of the bus bounds with the new listener |
| `tools/check-baseline.toml` | entries for the replaced `main.rs`, `supervisor.rs`, `config.rs` removed (nothing left), `boot.rs` lowered |
| `tools/kern-test/init-e2e.sh` (new) | the end-to-end run on the Ultar kernel (below) |

## Service files – new keys

`user`, `group` (default: `user`), `groups` (≤ 16), `rights` (`all` – refused at start unless named in `init.toml`'s `all_rights_allowed` – or `mount net_admin load_driver kill_any
set_time raw_device reboot manage_groups set_id file_admin`; **default: none**), `slice` (`system` | `session` |
`apps`, default `system`), `dirs` (created, owned by the service, `0700`; what lies below and belongs to someone
else – state of releases that ran the service as root – is handed over, links not followed, ≤ 10 000 entries),
`public_dirs` (the same with `0755`, for socket directories), `notify` (started only when `READY` arrived),
`start_timeout_ms` (default 10 s), `stop_timeout_ms` (default `shutdown_grace_ms`). Unknown keys, unknown rights,
relative paths, bad names, out-of-bound values: the file is refused as a whole (`refused file=… reason=…`).

Ids of the system services: **801 busd, 802 logd, 803 settingsd, 804 devd** (gid = uid). The sockets of a
service are open to its group only (`0660`); busd is a member of 802–804, logd of 801/803/804, so each can hand
its sockets to those groups (owner → own group needs no right on Linux and on the Ultar kernel). The
administrator's sockets and init's sockets stay `0600` (root with `file_admin`).

`init.toml` (all optional): `service_dirs`, `group_root` (default `/sys/fs/cgroup` as PID 1, none otherwise),
`output_socket` (`/run/ultar/log/init.sock`), `bus_socket` (`/run/ultar/bus/init.sock`), `bus_policy`,
`control_socket` (`/run/ultar/ctl.sock`), `net_allowed`, `launcher`, `shutdown_grace_ms`, `all_rights_allowed`
(service names that may ask for `all`; default empty: such a file is refused when it would start, journal
`refused … reason=rights_all_refused`, not restarted – fail closed).

## Identities and rights of the shipped services

Checked by `tests/shipped_services.rs`: no file asks for `all`; a service without its own user must be on the
test's allow-list and carry a `# root:` reason; every kept right has a `# right NAME:` reason; every right at
once only for `entwickler`, `shell`, `ui`. Where each right is enforced by the Ultar kernel: `mount` (mount,
chroot, pivot_root, unshare/clone of namespaces), `load_driver` (modules, `/sys` control files), `net_admin`
(`/proc/ultar/firewall`, `/proc/ultar/verbund`), `file_admin` (permission bypass, chown/chmod of others' files).

| Service | Runs as | Rights | Why (short; the file has the full reason) |
|---|---|---|---|
| bus, logd, settingsd, devd | 801–804 | none | (wave 4) |
| heartbeat | 65534 (nobody) | none | prints and sleeps |
| llm | 810 | none | socket moved to its own `/run/ultar/llm/llm.sock` (`public_dirs`, `--socket`); clients try it first, the old root socket stays a fallback; images in `/run/ultar/llm/img` |
| verbund | root | none | reads/writes the surface's root-only files in `/run/ultar` (`shell.fern`, `bildschirm.raw`) |
| tontest | root | none | root-only control socket (power off) |
| compat-bind, llm-compat, selftest, webtest | root | mount | bind mount / the compat runner (unshare, mounts, chroot; it does not drop to a user yet) |
| drivers | root | load_driver | finit_module, `/sys/module/firmware_class/parameters/path` |
| netzregeln | root | net_admin | firewall rules |
| ril | root | net_admin | modem IPC devices, mobile data address and routes |
| net, wifitest | root | net_admin, mount | addresses, routes, DHCP port 68; wpa_supplicant/hostapd via the compat runner |
| modem | root | mount, load_driver | EFS mount, driver files in `/sys`, `/dev/umts_boot0` |
| fingerabdruck, gnss | root | mount, file_admin | vendor HALs in a chroot with bind mounts, chmod of vendor device nodes |
| shell, entwickler | root | all ten, listed | administrator shells (console rescue, developer access) |
| ui | root | all ten, listed | the surface still administers the system itself (modprobe, iw, cells, dpkg, clock, reboot, kill, fb0/input) |

## How a service starts

1. Order: depth from the `after` lists (`order.rs`); a cycle is refused (`dependency_cycle`), the rest runs. A
   dependant starts when its dependencies are `running` (with `notify`: after `READY`) or done successfully.
2. Init prepares the service's directories and its memory group `<root>/<slice>/<name>` with the limits of
   `[cgroup]` (only `memory.* pids.* cpu.* io.*` keys, plain values). Detected, not assumed: no tree (no
   `cgroup.procs` at the root) → services run without groups (`platform groups=no`); a tree without a file for a
   `memory.*`/`pids.*` limit → the start is refused (fail closed); a controller the kernel has none of (`cpu.*`,
   `io.*` on the Ultar kernel) is skipped and named in the journal.
3. Init executes **itself** as launcher (`ultar-init --launch …`, no `unsafe`, no `pre_exec`): the launcher moves
   into the group, starts a session, takes the console (console services), switches groups/gid/uid (read back),
   sets no-new-privileges, drops every kernel right the service does not keep (read back; only where the kernel
   has rights), unblocks the signals and executes the program. Every step that fails ends the launcher with its own
   code (120–127, journal `launch=…`) before the service ran.
4. Standard output and error go through pipes to init, which forwards each line (≤ 512 bytes) to the log
   service's output socket; while the log service is not there the lines wait in a ring of 512 lines / 128 KiB
   (the oldest go first, counted). Console services keep `/dev/console`.
5. `notify = true`: the write end of a pipe is inherited (`ULTAR_READY_FD`); `READY` within `start_timeout_ms`
   makes the service `running`. A deadline that passes or a pipe closed before `READY` ends the process
   (`ready_timeout` / `not_ready`) and counts as a failure.
6. Ends: `restart` policy; failures back off 200 ms · 2ⁿ up to 30 s; 10 short failed runs in a row (run < 10 s)
   give the service up (`failed`, `reason=crash_loop`) until it is started on request. After every end the rest
   of its process group and memory group is ended.

Without root (development, tests) init cannot switch ids: a service naming another user is refused
(`identity_needs_root`), the others run as init's user.

## Shutdown

`SIGTERM`/`SIGINT` (restart as PID 1, exit otherwise), `SIGUSR2`/`SIGPWR` (power off), `poweroff`/`reboot` on the
control socket: no more starts; services stop in waves, deepest dependants first, the log service last; each gets
`SIGTERM` to its process group, `SIGKILL` (and `cgroup.kill`) after its stop deadline; before the last wave the
waiting output gets up to 1 s to reach the log service. Then (PID 1) everything else is killed, file systems
are unmounted, power off or restart.

## Status and control

* Bus service `init` (`ultar_bus::messages::init`): `init.status` → name, state, pid, uid, restarts, last end;
  `init.control` (start | stop | restart) → state after the request; errors `Service(1)` unknown, `(2)` already,
  `(3)` shutting down. The broker's policy gives the calls to `system.admin`; init additionally requires
  `init.admin` from the same policy file (read at start and on `SIGHUP`), so a policy that lets an app call
  `init.status` does not make it an administrator.
* `uinit [--bus PATH] [--token-file FILE | --token-fd N] [--lang ar|en|de] status | start|stop|restart SERVICE`
  (default bus: the administrator's socket), messages in Arabic, English, German.
* The text control socket of earlier releases (`ultar-ctl`, admin web page, scripts) answers as before
  (`status` table with the same five columns, `start|stop|restart|reload|uptime|poweroff|reboot|rescue`).

## Tests

* Unit: service files (all keys, defaults, refusals, the shipped files), order and cycles, groups (limits,
  unsupported controller skipped vs. refused, key/value checks), launcher command line (round trip, refusals),
  output (line cutting, bounded ring, delivery once the log service listens), units (back-off, crash loop,
  readiness parsing, a flooding readiness pipe is cut off), supervisor with real processes (order after
  readiness, readiness deadline, crash loop, requests, stop deadline → `SIGKILL`, output, cycles), start
  (identity rules with and without root, directories, hand-over of a root-owned tree), control (admin rule).
* Process (`tests/process.rs`, `tests/bus_control.rs`, real `ultar-init` not as PID 1): order and reverse stop
  order, output to a stand-in log service, memory group with limits and the launcher's pid in `cgroup.procs`,
  as root: user/groups/no-new-privileges/owned directory; as non-root: another user refused; restarts and
  requests; readiness deadline; bus: administrator steers, an identity without `init.admin` is refused, `uinit`
  exit codes. All run as root and as a user without root with umask 002.
* logd: `tests/output.rs` (stored cleaned, administrators only, limited per service, listener refuses records).
* End to end (`examples/inittest.rs`): init starts the **shipped** logd, bus, settingsd and devd definitions
  (paths moved below a work directory), each as its own user; checks the users (bus status, peer credentials of
  their sockets, owners and modes of their directories), that logd stored init-forwarded output, and a clean
  shutdown of init. Linux (root, debug and static release builds): `INIT: ALLES OK`, all four services exit 0.
  **Ultar kernel** in QEMU (`tools/kern-test/init-e2e.sh`, kernel image of a5eb384 with the identities patch,
  `--groups`: the kernel's group tree, rights dropped): `INIT: ALLES OK` – the four services start in order, each
  reports `READY`, runs as 801–804 in `system/<name>` with its limits; shutdown in reverse order. Journal of the run:

  ```text
  platform groups=yes rights=yes ids=root
  start logd pid=4 / ready logd / start bus pid=12 / ready bus / start devd / start settingsd / … / settled
  INIT: services running as their users: [("bus", 801), ("devd", 804), ("logd", 802), ("settingsd", 803)]
  shutdown begin / exit settingsd code=0 / exit devd code=72 / exit bus code=72 / exit logd code=0 / shutdown done
  INIT: ALLES OK
  ```

  `tools/kern-test/ci.sh` runs it as its last step (two lines added; `selftest.rs` untouched).

## Found on the Ultar kernel (for the kernel agent)

* **`posix_spawn` parent held until the child ends**: a parent that starts a program the way glibc's
  `posix_spawn` does (`clone` with `CLONE_VM|CLONE_VFORK`, which `std::process::Command` uses whenever it
  can) is released only when the child **exits**, not when it calls `execve`. A short program looks fine; a
  service that keeps running holds its parent forever. Init (and `inittest`) set the group id explicitly,
  which makes `std` fork instead; any other Rust program that starts long-running children on this kernel
  hangs the same way.
* On `SIGTERM` busd and devd end with code 72 (their runtime's own stop deadline, 2.5 s) on the Ultar kernel,
  0 on Linux: a thread blocked in `read`/`accept` on a Unix socket seems not to wake when another thread calls
  `shutdown` on it. Not investigated further.

## Open

* 17 services still run as root (table above). Not split yet, by size: `ui` (every administration page has to
  become a bus call to the owning service), `net`/`ril` (address/route helper behind the bus, DHCP and Wi-Fi as a
  user), the compat runner (drop to a user after chroot; then compat-bind/llm-compat/webtest/selftest), `verbund`
  (needs the surface to create its exchange files for a group), the tests on the root-only control socket
  (needs `init.control` over the bus for `system.admin`), the vendor HALs (the Android daemons expect root).
* On Linux the rights list is not enforced (no capability drop); a root service keeps root's capabilities there.
* Exit codes 120–127 of a service itself are read as launcher failures in the journal word (only the word).
* `cpu.weight` has no controller on the Ultar kernel (skipped, reported); on Linux hybrid cgroup v1 machines
  the tree is "not there" (only cgroup2 at the group root is used).
* Output lines are limited per service in the log service, but all services share init's queue share there;
  a service printing without end can delay others' lines (they wait in init's ring, which drops the oldest).
* The Ultar kernel's `/proc/<pid>/status` reports uid 0 for everyone; the end-to-end run checks identities
  through peer credentials, directory owners and the bus instead.
* The kernel self-test (`selftest.rs`) is unchanged; the QEMU run is the separate script above.
* Not tested on the Ultar kernel: init as the real PID 1 of a boot (the kernel shell starts programs, and a
  program started from it reports pid 1 in its own namespace – `ultar-init` then also runs the PID-1 boot steps;
  the end-to-end run starts init from `inittest`, as pid ≠ 1), console services, `uinit` over the bus.
