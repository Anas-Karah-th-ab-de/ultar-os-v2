# Ultar kernel – memory that cannot stop the machine, and resource groups (design)

Stage 2 of PLAN.md §3.7 ("Gruppen": how much a process may *consume*) and the fix for finding K5
("running out of memory is a kernel panic – a cell without rights halts the whole machine").
Integration notes, changed system calls and test results: `kernel-memory-groups.md`.

Owner's requirement, binding for every rule below: **limits are dynamic where nothing is configured, so that
everything keeps running well – no fixed small caps that break normal programs – but the machine never stops.**

## 1. What was wrong

* The kernel heap was one fixed block (a quarter of the largest memory region, at most 256 MiB); user pages came
  from the rest. A failed allocation in either pool was `expect("kein Speicher")` or Rust's allocation error –
  a kernel panic.
* Everything a process can make the kernel hold lived in that heap without any bound or owner: file contents
  (every open file is a buffer in the kernel), tmpfs trees, pipe buffers, process tables.
* So one process without any right could stop the machine in four ways (reproduced with
  `tools/kern-test/attacks/ausbruch.c --dos mem|tmpfs|pipe|fork`).

## 2. One pool of memory

`mem.rs`, `buddy.rs` – **page allocator.** All usable memory of the firmware map is managed by a buddy allocator (blocks of
2^0 … 2^16 pages, merged on free), two zones (below/above 4 GiB: drivers get DMA memory from below first,
processes from above first). A per-page record (4 bytes) holds the state and the owner. In front of the buddy
lists sits a cache of up to 512 single pages, refilled 32 at a time, so the common case – one page for a process –
costs a few instructions.

`slab.rs`, `heap.rs` – **kernel heap on top of the page allocator**, no fixed size any more:

| Request | Served from | Given back |
|---|---|---|
| ≤ 2 KiB, alignment ≤ 16 | slabs: 13 size classes, constant time | a slab without objects returns its pages |
| < 64 KiB | chunks (2 MiB, 4 MiB above 4 GiB of memory …; smaller if memory is fragmented), each a `linked_list_allocator` heap | an empty chunk returns its pages (one is kept) |
| ≥ 64 KiB | a run of pages straight from the page allocator | at once |

So file data, pipes, process tables and user pages all draw from the same memory and return to it. Nothing is
"heap memory" or "user memory" in advance – that is what lets the limits be dynamic.

**Reserves** (so that the kernel can always handle a shortage):

* *kernel reserve* = `T/64`, 4 … 64 MiB (`T` = managed memory). Requests *with policy* (§4) are refused before
  free memory falls below it; only the kernel's own small objects go lower.
* *emergency heap* = `T/256`, 1 … 8 MiB, set aside at start and never used otherwise. It serves a request only
  when the page allocator has nothing left; using it marks memory as critical (§6).

## 3. Accounting: every byte has an owner

Every allocation is charged to a **resource group** (§5) – in the allocators themselves, not at selected call
sites, so there is no kind of kernel object that escapes it:

* The *context* of a processor (`heap::context`) is the group of the process it works for: set when a process is
  scheduled, on every system call and fault; the kernel itself (console, scheduler) is the root group.
* A small object carries a 16-byte header with its owner and charged size; a page or page run has the owner in
  its page record. Freeing un-charges *that* owner, whoever frees and whenever.
* A child is charged to all ancestors (atomic counters in a fixed table: charging needs no lock and no memory).

What this covers: anonymous pages and page tables, file buffers of open descriptors, tmpfs data and names, pipe
and Unix-socket buffers, TCP/UDP buffers of a socket (charged to the group that created the socket), kernel
stacks, process and descriptor tables, mount tables, epoll lists, strings.

Scopes: `heap::scope(group, …)` charges to another group – used by `fork`/`clone` (everything the child needs is
the child's, which matters when the child is a new cell) and for programs the kernel starts. `heap::kernel(…)`
charges to the kernel: used where a process touches a table shared by all (process table, pid tables, the RAM
layer's and tmpfs' index trees, socket table, network queues, terminal buffers, the FAT sector cache, console
output, the initial namespaces). Rule of thumb: data belongs to who wrote it, shared index structures to the
kernel.

Consequence that is tested: when the processes of a group are gone, its `memory.current` is exactly 0, and the
kernel's own total after thousands of processes, files, pipes and cells is where it was (± a few kB of grown
tables).

## 4. Refusing instead of panicking

Rust's collections cannot report a failed allocation. Therefore two kinds of requests:

* **With policy** – may be refused: limits of the group (§5), dynamic rules (§6), kernel reserve.
  * every page for a process (`mem::alloc_page`: user pages, page tables) – `paging.rs` returns the refusal;
  * every kernel buffer whose size a caller chooses: inside `heap::fallible(…)` (helpers `mem::try_vec`,
    `try_with_capacity`, `try_reserve`, `try_copy`), from one page up. Converted sites: user buffers of
    `read`/`write`/`send*`/`recv*`, file buffers (open, write, truncate, FAT/ext4 reads, copy-on-write), pipe
    and socket buffers, descriptor tables, directory listings, `getdents`, the mapping list of `mmap`, kernel
    stacks and descriptor tables of `fork`, argument strings of `execve`, `getrandom`.
* **Forced** – everything else (the small bookkeeping objects of a system call, and whatever large site is not
  converted): charged without asking and served as long as memory physically exists – from the kernel reserve
  and finally the emergency heap. The limits catch up at the next safe point (§7).

What a refusal becomes:

| Where | Result |
|---|---|
| inside a system call | `ENOMEM` (also when the code's own error would have been `EFAULT`/`ENOENT`: a call that fails after a refused request reports `ENOMEM`), `EAGAIN` for `pids.max`, `ENOSPC` for a tmpfs quota |
| page fault, the group's own doing (explicit limit, protected minimum, fair share) | the faulting process ends like `SIGKILL` (status 9) |
| page fault, the machine is short | the kernel ends the *victim* (§6) and the faulting process tries again; if there is nobody to end, the faulting process ends |

A request below one page is always forced: the many tiny buffers of a system call must keep working for a group
at its limit, otherwise it could not even exit cleanly.

## 5. Groups

A tree with four fixed branches: `system` (core services), `session` (the user's surface and programs), `apps`
(one group per app), `cells` (containers, foreign systems). Every other group is a child of a branch or of
another group (at most 6 levels, 1024 groups).

* Programs the kernel starts: `ultar-init` and background services → `system`; console programs, the desktop
  shell, the self test → `session`. A child stays in its parent's group.
* **A process started in new namespaces** (`clone` with `CLONE_NEWNS`/`CLONE_NEWPID` – a cell) **gets its own
  group** `cell-<pid>`: below `cells` if the starter is in `system`/`session`, otherwise below the starter's own
  group – so starting a cell is never a way out of a limit. The group disappears with its last process.
* Per group: `memory.current/peak/max`, `pids.current/peak/max` (threads; released when reaped), event counters.
* Interface: a `cgroup2`-style file tree at `/sys/fs/cgroup` (`groups/files.rs`; table of files in
  `kernel-memory-groups.md`), `/proc/<pid>/cgroup`, `/proc/ultar/gruppen`, console command `gruppen`.
* Right **`MANAGE_GROUPS`** (bit 7, Linux name `CAP_SYS_RESOURCE` for `prctl(PR_CAPBSET_*)`): needed for every
  change – create/remove a group, set a limit, move a process, raise the largest pipe size. Droppable like the
  others; programs started by the kernel hold it, a cell is expected to drop every right (`~0`).
* Moving a process counts its threads in the new group at once; memory already charged stays where it is until
  freed (as under Linux). No process can be placed directly in the root, `apps` or `cells`.
* A removed group that still has memory charged (objects that outlived its processes, e.g. a file a
  non-confined process left in the host's RAM layer) keeps its slot until that memory is freed; the ancestors
  keep counting it. `/proc/ultar/gruppen` shows such groups (`reste`).

**Explicit limits** are exact for everything with policy: `memory.current` never exceeds `memory.max` through
pages or buffers. Small forced objects are charged too; a process whose system call leaves its group above
`memory.max` is ended at the end of that call. `memory.max` below the current use is refused (`EBUSY`) – the
kernel cannot take memory back. `pids.max`: the fork that would exceed it gets `EAGAIN`.

**CPU** is not part of this stage: the scheduler is a plain round-robin over all threads and has no notion of
weight; adding one is a scheduler change of its own (plan: "the scheduler knows the branches").

## 6. Dynamic rules (nothing configured)

All derived from `T` at start (`groups/policy.rs`); with plenty of memory the whole check is one comparison.

| Name | Value | 512 MiB VM (T = 243.5 MiB) | 2 GiB VM (T = 1772 MiB) |
|---|---|---|---|
| reserve | `T/64`, 4 … 64 MiB | 4 MiB | 27.7 MiB |
| critical | reserve × 1.5 | 6 MiB | 41.5 MiB |
| low | reserve + `T/16` (16 … 256 MiB) | 20 MiB | 138 MiB |
| protected(`system`) | `T/16`, 16 … 512 MiB | 16 MiB | 111 MiB |
| protected(`session`) | `T/8`, 32 MiB … 1 GiB | 32 MiB | 222 MiB |
| threads, whole machine `G` | `T/64 KiB`, 256 … 65536 | 3896 | 28353 |
| threads per branch / per group | `G/2` / `G/8` (`system`: `G`) | 1948 / 487 | 14176 / 3544 |
| tmpfs / RAM-layer quota | `T/4`, at least 8 MiB (`size=` overrides) | 60.9 MiB | 443 MiB |
| pipe | 64 KiB, `F_SETPIPE_SZ` up to `/proc/sys/fs/pipe-max-size` (1 MiB) | | |

(On this kernel the first 256 MiB of physical memory are the window for fixed-address programs and are not
managed, hence `T` < RAM.)

A request with policy of `n` bytes by a group in branch `B` is refused, in this order:

1. `free − n < reserve`;
2. `B ≠ system` and `free − n < reserve + unused protected minimum` of `system` (and of `session` if `B` is
   `apps`/`cells`). Unused = protected − charged. *The other branches cannot consume the minimum.*
3. `B ≠ system`, `free − n < low`, and the requester's *unit* is the **offender**. *Largest relative to its fair
   share is refused first; the others go on.*

Units: `session` as a whole, and every child group of `apps` and `cells` (with what lies below it). With `M` =
number of such children that hold memory or processes, plus one, and `S = T − reserve − protected(system) −
protected(session)`: fair share of a child = `S/M`, of the session = `protected(session) + S/M`. Offender = the
unit with the largest `charged/share`; equal: the higher slot number.

**Critical** (`free < critical`, or the emergency heap was used): the kernel ends a victim –

1. the offender among the children of `cells` and `apps` that still have a live process: *all* processes of
   that group and the groups below it;
2. only if there is none: the single largest process of `session` (most resident pages; equal: highest pid);
3. never a process of `system`.

One victim at a time: the next is chosen when memory is still critical after the previous victim's memory was
released, at the earliest 250 ms later. Checked at every scheduling decision and after every system call
(`oom::watch`), and directly by a page fault that the reserve refused. Each decision is logged
(`[mem] oom-kill …` / `[mem] oom-end …`) and counted in `memory.events` (`oom_kill`).

Why these rules do not disturb normal work: rule 1–3 need free memory to be below
`max(reserve + both protected minimums, low)` – about 20 % of memory on the VMs above. A process alone on the
machine can use everything down to that line.

## 7. Bounds on single objects

* **Pipe / Unix socket**: bounded buffer (64 KiB / 256 KiB per direction). A full buffer blocks the writer or
  gives `EAGAIN`; writes up to 4096 bytes are all-or-nothing; a blocking `write` delivers everything, continuing
  across waits (`Res::Partial`). `poll` reports writable only with ≥ 4096 bytes of room.
* **tmpfs and the host's RAM layer**: quota per tree (`isolation/tmpfs.rs`) counting data + path + 128 bytes per
  entry, reserved when a descriptor grows a file (so the writer gets `ENOSPC` at `write`). File buffers are
  shared between the stored file and descriptors until written (copy on write) – opening a RAM file no longer
  copies it.
* **Descriptors**: 65536 per process (`EMFILE`). **Mappings**: 65530 reserved ranges per address space
  (`ENOMEM`). **epoll**: 65536 entries. **Arguments of `execve`**: 192 KiB per vector (`E2BIG`).
* **Sockets**: 4096 in all, 512 per group below `apps`/`cells` (`ENFILE`); UDP datagrams waiting on a socket
  ≤ 256 KiB in total; the send queue ≤ 4 MiB; received data that the owner's group gets no memory for is
  dropped (TCP: not acknowledged).
* **ELF loader**: segment addresses and sizes are checked before pages are mapped. (Also closes a hole next to
  K5: a relocatable program with a wrapped segment address could place user pages over the kernel's identity
  map in its own address space.)

## 8. Why the machine does not stop – and what is left

1. Every path that grows with a size the caller chooses (pages, buffers ≥ one page listed in §4) is refused by
   policy *before* memory is short and reports an error.
2. What is not converted is forced: bounded per call, charged, and served from the kernel reserve, which the
   policy keeps free. If a group is pushed over its limit that way, its process is ended at the end of the call.
3. Before free memory reaches the reserve it passes *critical*: the kernel ends the group that holds the most
   (never `system`), which frees memory synchronously (or at the victim's next kernel entry on another core).
4. The emergency heap covers the moment between.

An allocation can still fail for good only if the emergency heap is exhausted as well, or if a *forced* request
needs a contiguous block that fragmented memory cannot supply (the emergency heap serves it if it fits).
Known forced sites that can be large: the copy of the epoll list in `epoll_wait` (≤ 1 MiB), reads of the frame
buffer (≤ 16 MiB, needs `RAW_DEVICE`), listings in `/proc` and the scheduler's wake-up table (proportional to
the number of threads, ≤ 65536), the process table itself, ext4/FAT metadata of the host's disks. None of them
can be driven by a cell beyond the bounds named. They are listed under "Not done".

## 9. Cost

Instruction counts in QEMU (`-icount`, one CPU, 1 ns per instruction), original kernel → this kernel:

| Operation | before | after |
|---|---|---|
| `getpid` × 200 000 | 60 ms | 68 ms |
| `fork` + `wait` × 300 | 129 ms | 146 ms |
| touch 128 MiB of fresh memory | 28 ms | 32 ms |
| unmap 128 MiB | 3 ms | 10 ms |
| create 2000 files in one RAM directory | 17 861 ms | 123 ms |
| `stat` + read / unlink 2000 files | 34 / 18 ms | 38 / 21 ms |
| pipe: 4 KiB write + read × 20 000 | 1676 ms | 92 ms |
| `mmap` + touch + `munmap` × 20 000 | 60 ms | 60 ms |
| `fork` + `execve` × 50 | 234 ms | 240 ms |

The price of accounting and of a page allocator that merges is 5–15 % on the bare paths (unmap: freed pages are
merged now); files and pipes are faster by orders of magnitude because small objects are constant-time (slabs),
`stat` of a directory no longer builds its listing, and pipe data is copied in bulk.
