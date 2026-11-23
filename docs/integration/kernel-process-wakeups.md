# Ultar kernel – vfork/posix_spawn, Unix-socket wake-ups, signalfd with threads (FIX1 after D1)

Patch: `FIX1.patch` (base: HEAD dee3bc3 plus the uncommitted ultar-init working tree of 2026-11-23).
Found by the ultar-init integration; causes verified with `weckertest` on the unfixed kernel (13 of 21 checks failed).

## Causes and fixes

| Defect | Cause | Fix |
|---|---|---|
| A `posix_spawn` parent (glibc: `clone(CLONE_VM\|CLONE_VFORK)`, Rust `Command`) waited until the child **ended** | the parent of a vfork waited for the child's end only; `CLONE_VM` was ignored (the child got a copy of the memory), so glibc's report of a failed `execve` (written into the shared memory) was lost too | a `CLONE_VM\|CLONE_VFORK` child shares the parent's address space (`Group.space` is an `Arc`) until `execve` or its end; `execve` releases the waiting parent (`proc::vfork_exec`), the end still does. Without `CLONE_VM` the child gets a copy as before |
| A thread blocked in `read`/`write`/`accept` on an AF_UNIX socket did not wake on `shutdown` from another thread | `shutdown` on Unix sockets returned 0 and did nothing | per direction a `shut` flag on the pipe: `SHUT_RD` → our reads end of file once the buffer is empty, the peer's writes `EPIPE`; `SHUT_WR` → our writes `EPIPE`, the peer reads the rest then 0; `SHUT_RDWR` both; every blocked thread is woken. On a listening socket: it stops listening, a blocked `accept` ends with `EINVAL`, queued connections are closed. `poll`/`epoll`: `POLLIN` at end of file, `POLLRDHUP` when the peer stopped sending, `POLLHUP` when both directions are closed |
| (checked) the peer's `close` | already woke readers (0) and writers (`EPIPE`) | test added |
| signalfd in a threaded process | a signal for the process that every thread blocks is parked at one thread; `read`/`poll` of a signalfd looked only at the calling thread | `read`, `poll`, `select`, `epoll` on a signalfd see the signals pending for any thread of the process (own thread first) |

Effect: ultar-init starts services with `posix_spawn` again (its `gid` workaround and the one in `inittest` are
removed), and busd, logd, settingsd and devd end with code 0 on `SIGTERM` (before: 72, their stop deadline).
`init-e2e.sh` now also requires `exit <service> … code=0` for all four.

## Device tree on bigger machines (Beelink stage 1)

The failing check "Register: /sys/kernel/ultar/drivers nennt jeden Treiber" assumed QEMU's machine (a virtio-blk
device bound to `virtio-blk`); on the Beelink no virtio device exists, so its line ends in `-`. No truncation: every
generated file of the device tree is built whole for each `open` and read at any offset (checked). The check now
compares every driver directory with its line (exactly the linked devices, or `-`) on any machine; a new check
reads every table of `/sys/kernel/ultar` and `uevent_seqnum` in 7-byte pieces and compares with one large read;
the test's own readers no longer stop at a fixed buffer. `ci.sh` has an extra boot with 57 extra PCI functions
(seven multifunction slots, one behind a PCIe root port) and four USB devices and requires the device table to be
larger than a page (4.6 KiB). That boot runs QEMU's vCPUs on one host thread: with one thread per vCPU, QEMU 8.2
itself crashed (SIGSEGV, exit 139) in about one of three boots at SMP=4 during the guest's PCI scan.

## Console lines and the hot-plug harness (GERAETETEST-USB at SMP=4)

- Kernel lines were written in pieces, each under the console lock: `kprintln!` wrote the formatted parts and the
  line end separately, so lines of several cores interleaved (seen on the Beelink as mixed `[mem]` lines), and an
  interrupt on the same core could print into a line (the lock is re-entrant per core). Now a formatted message
  and its line end are one write under the lock, and interrupts are off while the lock is held. `weckertest`
  checks it: 4 processes on 4 cores make 160 `[exec]` lines at once; all must be whole in the kernel log (the old
  code gave 38 torn lines of 160).
- A byte for the serial line was dropped after ~100 000 status reads when the emulator held the transmitter; the
  wait is 20× longer now.
- `geraete-hotplug.py` stopped QEMU 5 s after the start of the verdict line, whether the line had arrived or not.
  It now waits for the complete line and the next prompt (30 s each) and writes `[harness] QEMU ended by itself
  with code …` when the emulator ended on its own. 20 runs at SMP=4: see the report.

## Device events for ultar-devd

`ultar_sys::uevent::{UeventSocket, Uevent}`: safe netlink socket (`rustix::net`, feature `net` added; no new crate)
that joins the kernel's event group, waits with a timeout and parses messages (`libudev` re-broadcasts skipped).
ultar-devd reads it on its own thread and rescans when an event came; the counter file is read only when the
kernel refuses the socket. The bus endpoint and the socket cannot be waited on together, so the main loop still
wakes once a second – it now checks a flag instead of reading a file.

## Tests

`user/weckertest.c` (21 checks, `WECKERTEST: ALLES OK`; in `ci.sh`, the hardware self test and the stage-1 verdict):
posix_spawn returns while the child sleeps, failed execve → `ENOENT`, vfork shares memory, vfork+execve releases the
parent; shutdown RD/RDWR/WR and peer close wake readers (0) and writers (`EPIPE`); poll/epoll readiness; accept ends
at shutdown (`EINVAL`); signalfd read by a second thread, poll in the main thread. `ultar-sys`: message parsing and
socket open. `ultar-init` process test: waits for the launcher's move into its group (posix_spawn returns earlier).

## Open

- A vfork child shares the parent's memory map bookkeeping only through the address space; `brk`/`mmap` calls of the
  child before `execve` (not done by posix_spawn) are not reflected in the parent's counters.
- devd still wakes once a second between requests (see above).

## FIX2: a killed process that was about to block stayed forever (GRENZTEST 2048 MiB at SMP=4)

**Cause (not vfork):** `end_group` only *marks* a process `killed` when it runs on another core at that moment
(it ends "at the next entry"). The scheduler ends marked processes only when they are `Ready`, and the wake rules
look only at what a process waits for. A process that was marked while on its way into `pause()` (or any wait that
nothing else ends) therefore stayed blocked forever with all its memory: after the process avalanche one child of
the ended cell (≈ 3.5 MB) was left charged to `cells` – the namespace was already gone, nobody killed it again.
`weckertest` reproduces it: SIGKILL right after a child said it is going into `pause()`, 300 rounds – 1 child hung
on the old kernel, 0 now. The race is older than FIX1; heavier load makes it likelier.

**Fix:** the wake rules let every blocked thread with `killed` set go on at once; the scheduler then ends it.
Also: entries of the vfork wait list are dropped when the waiting parent ends (they stayed when a parent was
killed while waiting).

**Tests:** `weckertest` "SIGKILL: ein Kind, das gerade in pause() geht, endet immer (300 Runden)"; `grenztest`
"vfork in einer Zelle": in a cell with `memory.max` 32 MiB, a vfork child killed by SIGKILL before execve, a failed
execve (127), and a vfork child that touches the parent's reserved memory until the limit ends it; the parent
lives on, and afterwards `cells` is back at exactly its old value with no removed group holding anything.
