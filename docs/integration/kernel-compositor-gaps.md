# Ultar kernel – compositor gaps K1–K13, device node modes, input hot-plug

The kernel side of `docs/COMPOSITOR.md` §3 ("Kernel gaps"), plus owner/group/mode of device nodes for
`ultar-devd-seat` and USB keyboard hot-plug. Test: `user/fenstertest.c` (`FENSTERTEST: ALLES OK`, in `ci.sh`
and the hardware self test) and the USB part of `geraetetest --hotplug` (`geraete-hotplug.py`).

## 1. Status

| # | Gap | Now |
|---|---|---|
| K1 | `memfd_create` missing | `memfd_create` (319), flags `MFD_CLOEXEC`, `MFD_ALLOW_SEALING`; `read`/`write`/`pread`/`pwrite`/`lseek`/`fstat`/`ftruncate`/`fallocate` (mode 0, `KEEP_SIZE`) |
| K2 | no seals | `F_ADD_SEALS`/`F_GET_SEALS` (1033/1034): `SEAL`, `SHRINK`, `GROW`, `WRITE` (`EBUSY` while a writable shared mapping exists), `FUTURE_WRITE`; without `MFD_ALLOW_SEALING` the object starts with `SEAL` |
| K3 | `mmap` copied | `MAP_SHARED` maps the object's own pages (also after `fork`, also in another process that got the descriptor); `MAP_PRIVATE` copies as before; a writable shared mapping of a write-sealed object: `EPERM` |
| K4 | no `SCM_RIGHTS` | descriptor passing on connected Unix stream sockets (≤ 253 per message, `MSG_CMSG_CLOEXEC`, `MSG_CTRUNC`) |
| K5 | `MSG_DONTWAIT` ignored | honoured by `recvmsg`/`recvfrom`/`sendmsg`/`sendto` on Unix sockets; `MSG_NOSIGNAL` too |
| K6 | `fb0`: no mmap, no vblank | `mmap(MAP_SHARED)` of the framebuffer; `FBIO_WAITFORVSYNC` sleeps to the next blank of a 60 Hz grid; `FBIOGET_VBLANK` counts them |
| K7 | one display | **open**: the firmware framebuffer is the only display; more need a display driver (virtio-gpu, Intel/AMD) |
| K8 | fixed input nodes, no capabilities | `EVIOCGBIT`, `EVIOCGID`, `EVIOCGVERSION`, `EVIOCGABS`, `EVIOCGPHYS`/`UNIQ`/`PROP`/`KEY`/`LED`/`SND`/`SW`, `EVIOCGRAB`; every USB boot keyboard, mouse and absolute tablet gets its own node (keys with real press and release; mice `EV_REL`; tablets `EV_ABS` in their descriptor's range); **open**: touch devices (no driver) |
| K9 | per-app uids | launcher side (unchanged) |
| K11 | `POLLOUT` not woken | fixed (lost wake-up, §3) |
| K12 | sub-second timeouts ignored | `ppoll`/`pselect` nanoseconds and `select` microseconds round **up** to milliseconds |
| K13 | `mremap` missing | `mremap` (25) with `MREMAP_MAYMOVE` and `MREMAP_FIXED` |

## 2. Memory objects and shared mappings (K1–K3, K13)

* `memfd.rs`: `MemObj` = list of pages + size + seals + mapping counters. Pages exist for the whole size (no
  holes; `st_blocks` covers the size – the HAL's "sparse" check passes) and are charged to the memory group of
  the process that grows the object. Largest object 4 GiB.
* `paging/shared.rs`: a shared mapping puts the object's pages into the page table without the ownership bit, so
  unmapping or the end of the address space never frees them; `AddrSpace.shared` keeps the object (or the
  framebuffer) per mapping and counts it in the object. `fork` maps the same pages in the child.
* Differences from Linux: shrinking an object while it is mapped is refused (`EBUSY`; Linux allows it and
  later accesses get `SIGBUS`) – no page another process can reach is ever freed. A mapping longer than the
  object maps what exists; access beyond is `SIGSEGV`, not `SIGBUS`. `MAP_SHARED|MAP_ANONYMOUS` is still private
  after `fork`. `mprotect` stays without effect (as before). The `memfd_create` name is not kept.
* `mremap`: shrinking stays in place; growing moves the page table entries (no copy) to a new place whose tail
  is reserved like the old range – without `MREMAP_MAYMOVE` growth is refused (`ENOMEM`; Linux may grow in
  place). A shared mapping is mapped anew from the same object and offset (e.g. after `ftruncate` grew the
  pool). glibc `realloc` of large blocks uses it.

## 3. Unix sockets (K4, K5, K11, K12)

* `SCM_RIGHTS` (`scm.rs`, `pipe.rs`): the descriptors of one `sendmsg` travel with its first byte (a queue per
  direction keyed by stream offset). `recvmsg` that reads that byte installs them (lowest free numbers); it never
  reads past the next byte that carries descriptors, so two messages' descriptors never arrive together. What
  does not fit the control buffer is closed (`MSG_CTRUNC`). A plain `read` discards descriptors. Descriptors in
  flight keep their objects alive. A connected Unix socket travels too (Wine hands its server one end of a
  socket pair per new process) unless it would land in its own receiving direction or a Unix socket already
  travels there; listening sockets never travel (`EOPNOTSUPP` for all three). Without Linux' garbage collector
  this rule keeps rings of sockets that only hold each other from forming (docs/integration/kern-wine.md). Not supported: `SCM_CREDENTIALS` (use `SO_PEERCRED`), `MSG_PEEK` on Unix sockets, datagram
  sockets.
* K11 cause: `poll`/`epoll_wait`/`select` and a full pipe write read the wake-up counter *after* checking
  readiness, so a reader that made room in between was missed until the next unrelated event. The counter is
  now read before the check (pipe writes: before the push).
* K12: the poll timer has millisecond resolution; a timeout below a millisecond waits one, `16 000 000 ns` waits
  16 ms (it was 0).

## 4. Device nodes: owner, group, mode (for `ultar-devd-seat`)

* Device nodes below `/dev` (not the directories) have attributes like Linux' devtmpfs: default root `0600` for
  `fb0` and `input/event*`, root `0666` for `null`, `zero`, `random`, `urandom`, `tty`, `console`; `stat` reports
  `S_IFCHR` with the Linux numbers (`fb0` 29:0, `input/eventN` 13:64+N, `null` 1:3 …).
* `chmod`/`chown`/`lchown`/`fchownat` on a node need `FILE_ADMIN` (being the owner is not enough); `/dev` and
  `/dev/input` themselves stay unchangeable (`EPERM`).
* Opening a node: `FILE_ADMIN`, or `RAW_DEVICE` for raw devices (`fb0`, input nodes), or the mode bits – where
  owner root grants nothing by itself: a process of uid 0 without those rights needs group or other bits like
  anyone else. So `root:seat 0660` opens for members of `seat` and nobody else without a right.
* Attributes are kept in memory (`perm::ATTRS`) until restart; a USB keyboard's node starts with the defaults
  again each time it appears. `ultar-devd-seat` sets them at every start and on device events: on this kernel
  its verdict is `kept` (`devd-qemu.sh`), so the compositor's group check applies and its `kernel-gated`
  relaxation is unused.
* `/dev/dri/card*` and `/dev/uinput` do not exist on this kernel.

## 5. Input nodes (K8) and hot-plug

* `input.rs`: `event0` (PS/2 mouse, absolute) and `event1` (PS/2 keyboard) as before; each USB boot keyboard,
  mouse and tablet gets the lowest free number from `event2` on while attached (xHCI attach/detach, also at
  boot). Mice and tablets (`drivers/gen_xhci/pointer.rs`): every HID interface whose report descriptor
  (`drivers/gen_xhci/hid.rs`, a small parser of HID 1.11 short items) has X and Y is read in report protocol;
  buttons 1–3 → `BTN_LEFT/RIGHT/MIDDLE`, X/Y → `REL_X/REL_Y` (relative) or `ABS_X/ABS_Y` with the logical range
  as `EVIOCGABS`, the wheel → `REL_WHEEL`. Its HID boot
  reports become press **and** release events (modifier bits as their own keys; HID usages mapped like Linux'
  `usbkbd`). The keys go to the node while any program has an input node open; otherwise to the console as
  before. After the keyboard left, an open descriptor reads `ENODEV`.
* Device events: `add`/`remove` with `SUBSYSTEM=input`, `DEVNAME=input/eventN`, `MAJOR`/`MINOR`, `NAME`.
  `/sys/class/input/eventN/{dev,uevent,device/name,device/uevent}`; writing `add`/`change`/`remove` into
  `uevent` (right `FILE_ADMIN`) announces the node again – what `ultar-devd-seat` does after a change. The
  entries are plain directories (no `/devices/virtual/` path: every node is hardware).
* Capabilities: PS/2 mouse and USB tablets `EV_SYN|EV_KEY|EV_REL|EV_ABS` (buttons, wheel, `ABS_X`/`ABS_Y`), USB
  mice `EV_SYN|EV_KEY|EV_REL` (`REL_X`/`REL_Y`/`REL_WHEEL`); keyboards `EV_SYN|EV_KEY`, key codes 1–127; ids
  `BUS_I8042` (PS/2) / `BUS_USB` with vendor and product.
  No autorepeat (`EV_REP` not reported; the compositor repeats).

## 6. Framebuffer (K6)

`mmap(MAP_SHARED)` of `/dev/fb0` maps the firmware framebuffer (`MAP_PRIVATE`: `ENODEV`; opened read-only:
read-only mappings only). The firmware gives no blank interrupt and no second buffer, so `FBIO_WAITFORVSYNC`
waits for the next point of a 60 Hz grid (an interruptible sleep) and `FBIOPAN_DISPLAY` stays a no-op: tearing
is reduced by pacing, not prevented.

## 7. Tests

* `fenstertest` (100 checks): memfd I/O, `fallocate`, seals (incl. `EBUSY`/`EPERM` cases), shared mapping across
  `fork` and two mappings, `MAP_PRIVATE`; `mremap` grow/shrink/refusal, `realloc`, pool growth; `SCM_RIGHTS`
  (new descriptor, close-on-exec, message boundaries, discard by `read`, `MSG_CTRUNC`, `EBADF`, sockets refused, mapping in
  another process); `MSG_DONTWAIT`, `MSG_NOSIGNAL`; `POLLOUT` wake-up within 2 s; 16 ms `ppoll`/`select`;
  `fb0` mmap, vblank pacing and counter; node defaults and numbers, giving nodes to group 812, opening as a
  seat member, a stranger, uid 0 without rights, uid 0 with only `RAW_DEVICE`; evdev ioctls; sysfs entries and
  a `change` event.
* `geraetetest --hotplug` (via `geraete-hotplug.py`): a USB keyboard added through the QEMU monitor gets
  `event2` with `add` event, `13:66`, `BUS_USB 0627:0001`, `KEY_A`; a key sent by the monitor arrives as press
  and separate release; after removal: `remove` event, node gone, open descriptor `ENODEV`. Then a USB mouse and
  a USB tablet: own nodes (`EV_REL`; `EV_ABS` with `EVIOCGABS` 0..32767), motion and a left click from the
  monitor (`mouse_set` per device) on the right node, both removed again.
* `compositor-qemu.py`: two apps on the Ultar kernel send their frames through shared memory (memfd +
  `SCM_RIGHTS`); `kerncheck` counts them from the compositor's window list:
  `COMPOSITOR-CHECK: OK windows=2 transport=shared-memory offered=shared-memory`.
* `devd-qemu.sh`: `ultar-devd-seat --once` sets `fb0`, `input/event0`, `input/event1` to `root:812 0660`,
  verdict `kept`.
