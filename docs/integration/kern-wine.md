# What Wine, ART and other Linux programs need from the Ultar kernel (stage K2)

Programs of other systems run on the Ultar kernel through the Linux ABI (docs/integration/ultar-foreign.md).
Wine (Windows programs, stage P4) and ART (Android apps, stage P5) ran on the Linux kernel but stopped on the
Ultar kernel. This stage adds what they asked for; each part has a check in `kernel/ultar-kern/user/winetest.c`
(`tools/kern-test/wine-kern.sh`, verdict `WINE-KERN: ALLES OK`, step of `tools/kern-test/ci.sh`) or in the
existing kernel tests named below.

## Address space

| Part | Behaviour | Check |
|---|---|---|
| Address wishes | `mmap` takes a free hint; `MAP_FIXED_NOREPLACE` gives that address or `EEXIST` (never another one); `MAP_FIXED` replaces what lies there (pages, reservations, shared mappings); `MAP_32BIT` places below 2 GiB; 64 GiB `MAP_NORESERVE` reservations | winetest placement |
| Partial `munmap`/`mremap` | a page out of the middle, the rest stays; `mremap` grows in place when the space behind is free (also without `MREMAP_MAYMOVE`, as Linux), else `ENOMEM`/moves; `munmap` and `MAP_FIXED` skip page-table levels without own tables (a 64 GiB range is gone in a few steps) | winetest, fenstertest K13 |
| Second low window | `0x7c000000–0x80000000` belongs to processes (Wine's preloader at `0x7d400000`, Windows' shared user data at `0x7ffe0000`) when the firmware map holds only free memory there, the framebuffer, the firmware stack and no device memory (PCI BARs, ECAM) lie there; then the kernel never allocates from it. The boot lines say whether it is open (`[speicher] Fenster …`). A lazy page in a low window replaces the kernel's 2 MiB page of that window (the user access meets a present page without the user bit: handled like an absent one, its TLB entry flushed) | winetest, boot line |
| Fault information | `si_addr`, `si_code` (`SEGV_MAPERR`/`SEGV_ACCERR`) and `REG_CR2` in the signal frame; a fatal exception other than a page fault logs its error code and the instruction's bytes | winetest |
| `/proc/<pid>/maps` | the own group's ranges in the Linux layout: reservations, shared mappings, mapped pages outside them, `[stack]`, `[heap]`; glibc's `pthread_getattr_np` (ART's thread stack) finds the main stack | winetest |

## Processes, signals, threads

| Part | Behaviour | Check |
|---|---|---|
| `waitid` (247) | `P_PID`, `P_ALL`, `P_PGID`; `WEXITED`, `WNOHANG`, `WNOWAIT` (child stays to wait for); `siginfo_t` with `CLD_EXITED`/`CLD_KILLED`/`CLD_DUMPED`; `ECHILD`, `EINVAL`. `WSTOPPED`/`WCONTINUED` are accepted; the kernel has no stopped children | winetest |
| `arch_prctl` | `ARCH_SET_GS`/`ARCH_GET_GS` next to FS; the user GS base is saved and restored on every switch, kept by `fork`, reset by `execve`; `EPERM` outside the user half | winetest |
| `sigaltstack` | `SA_ONSTACK` handlers run on the alternate stack (not when already on it), `SS_ONSTACK` while there, `SS_AUTODISARM`, `ENOMEM` below 2 KiB, `EPERM` while on it, `uc_stack` in the frame, restored by `rt_sigreturn` | winetest |
| Fatal signals | a signal for one thread waits at that thread (glibc's `raise`/`abort` block every signal around `tgkill`: the signal used to go to the group's first thread, which might sleep for ever – ART did not end after `abort()`); `abort()` in a second thread, with `SIGABRT` blocked or with a returning handler, a fault with `SIGSEGV` blocked or ignored, a fault in a second thread, `SIGTERM` blocked in the main thread only: the whole process ends | winetest |
| `/proc/self/…` | `readlink` of `exe`, `cwd`, `root`, `fd/N` (files as the reader's view sees them, `pipe:[N]`, `socket:[N]`, `anon_inode:[…]`), `/proc/self` (the own pid); `/proc/self/fd` lists the descriptors; opening `fd/N` opens a file anew with the bytes its descriptor holds now, any other descriptor is the same object. The same in a cell's own proc mount (mount and pid namespace, `pivot_root`) and after `chroot` + `mount -t proc` in the initial namespace (ART in the Android area): there proc shows the kernel's /proc on that directory for every process, the initial table itself never changes | winetest proc, cell, chroot |
| Unix sockets through `SCM_RIGHTS` | a connected Unix socket travels (Wine hands its server one end of a socket pair for every new process) unless it would land in its own receiving direction or a Unix socket already travels there; listening sockets never travel (`EOPNOTSUPP`). The last socket sent into a ring would already carry the next one, so no ring of sockets that only hold each other can form – no garbage collector needed. The process lock is held across check and send | winetest, fenstertest K4 |
| Small calls | `creat` (85, the Wine server), `getrusage` (98, zeros – no counters yet), `get_mempolicy` (239, one node, default policy) | Wine on the kernel |

## Files

| Part | Behaviour | Check |
|---|---|---|
| `MAP_SHARED` of files | one memory object per file while it is mapped: two mappings, `read`/`pread` and `write`/`pwrite` of that file and a forked child see each other's bytes; `msync` (and `munmap`, the last unmap) writes the file back; `EACCES` for a writable mapping of a read-only descriptor, `EPERM` for executable on `noexec`. apt's cache is no longer rebuilt on every run | winetest (`/tmp`, `/data` when writable) |
| `statfs` on an ext4 | the file system's own block size, blocks, free and available blocks, inodes and free inodes (it showed the machine's memory: a program checking for room before it writes saw the wrong numbers) | ext4test |
| `ENOSPC` on a half-empty disk | the ext4 writer stores a file as a whole and copies it (new blocks, then the old ones are freed) – a file larger than the free space could never be changed again. Without room for the copy it now writes into the file's own blocks (more only for growth, the rest freed; the inode changes last – a stop while it writes leaves a mix of old and new bytes, the file system stays consistent). A failed allocation no longer counts its blocks free twice in the superblock | ext4test (64 MiB file changed with less free space), e2fsck clean |

## Finding what a program misses

* `spur fehler <word>` in the kernel shell logs the failing calls (not `ENOENT`, `EAGAIN`, `EINTR`, nor mmap's
  `EEXIST` probes) of processes whose name holds the word or ends in `.exe`, at most 4000 lines
  (`[spur-fehler pid name] nr(args) = errno path`); `spur aus` ends it. Unknown calls are logged once each
  (`[syscall] noch nicht umgesetzt: N`).
* `tools/kern-test/foreign-e2e.sh` takes `FOREIGN_PRE="spur fehler wine"` (a shell command before foreigntest).

## Delimited: 32-bit Windows programs (WoW64)

Not built in this stage. A 32-bit program needs a code selector `0x23` for compatibility mode, `modify_ldt`
(154) for its `%fs`, and above all most of the address space below 4 GiB for its stack, heap and DLLs. On the
Ultar kernel `0x23` is the user data selector (layout kcode 0x08, kdata 0x10, user32 0x18, udata 0x20, ucode
0x28), and the kernel maps physical memory 1:1 at its physical addresses: below 4 GiB only the two windows
(about 250 MiB at `0x400000` and 64 MiB at `0x7c000000`) are free for processes. Changing the selector layout
and adding a per-process LDT (saved and loaded on every switch, user segment registers saved too) would give
32-bit code a selector but no room to run. The prerequisite is a kernel direct map in the upper half (no
1:1 map in the low half) – a larger change of every physical access in the kernel. Until then 32-bit Windows
programs run on the Linux kernel; 64-bit ones are the target here. The same limit holds for ART's heap below
4 GiB (`MAP_32BIT`: the two windows) and for machines whose firmware puts tables into `0x7c000000–0x80000000`
(the window stays closed; QEMU with 2 GiB or more is such a machine).

## State on the Ultar kernel

See the status line in docs/STATUS.md (Wine: `wine --version`, `wineboot -i`, `cmd /c echo`, a real program;
ART: `android-art`).
