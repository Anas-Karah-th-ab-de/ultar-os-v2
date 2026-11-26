# Ultar kernel – AVX state (XSAVE), Unix listener teardown, SO_RCVTIMEO

2026-11-26 · kernel area · tests: `user/avxtest.c` (`AVXTEST: ALLES OK`), `user/weckertest.c` (four new checks),
both in `ci.sh`; the restart at the end of `installer-e2e.sh` depends on the socket fixes.

## Open points (first)

1. **Hardware:** checked under QEMU (TCG, `-cpu max`, 2 cores) only. On the Beelink the first run should be
   `avxtest` (it needs no opt-in). XCR0 is limited to x87|SSE|AVX: **AVX-512** (opmask/ZMM, XCR0 bits 5–7) is not
   switched on – its XSAVE area (2.7 KiB) does not fit `trap::FX_SIZE` (1024) and the target processors (Arc
   140T host: Arrow Lake H; ThinkPad P1 Gen 3) have no AVX-512. AMX is not switched on either.
2. llama.cpp is still built as SSE4.2 (`tools/ultar-paketbau`); with this kernel an AVX2 build can be tried
   (`GGML_AVX2=ON`, measure against the SSE4.2 build on the Beelink). Graviola TLS (store, updater) can be tried
   on the Ultar kernel again (it refused because AVX was not usable).
3. `SO_SNDTIMEO` is still accepted without effect (a blocking write waits until there is room or the peer
   closes). `SO_RCVTIMEO` holds for Unix stream sockets only (not TCP/UDP).
4. The signal frame carries the whole XSAVE area, but without Linux' `FP_XSTATE_MAGIC` markers: programs
   that parse `uc_mcontext.fpregs` beyond the 512-byte FXSAVE part see no extended-state header. glibc does not.

## 1. AVX state

* `arch::xsave_an` (from `haerten`, every core): if CPUID has XSAVE, set CR4.OSXSAVE and XCR0 = x87|SSE|AVX
  (as far as CPUID 0xD reports them). The first core decides; others follow. If the standard-format size for
  those bits exceeds `trap::FX_SIZE`, XSAVE stays off (fxsave as before).
* `trap.rs`: `TrapFrame.fx` is 1024 bytes + 16 bytes padding, the frame is `align(64)` and 1216 bytes
  (19 × 64). Kernel stacks end on 64 bytes (`proc::kstack_top`), so the frame of a trap from ring 3 (fixed
  layout below the stack top) starts 64-aligned. Frames from ring 3 are saved with `xsave64`/`xrstor64`
  (mask all ones, XCR0 limits it), frames from the kernel itself with `fxsave64` (the kernel uses no vector
  instructions; any 16-byte alignment is enough). The CS of the frame decides which, on entry and exit.
* New threads (`fresh_frame`) start with a zero XSAVE header: `xrstor` puts every component in its init state.
* Signals: the frame on the user stack holds `FX_SIZE` bytes; `rt_sigreturn` takes them back and
  `signal::sanitize_fx` clears what would make `xrstor` fault (MXCSR reserved bits, XSTATE_BV bits outside
  XCR0, XCOMP_BV and the header's reserved bytes).
* Effect for programs: CPUID reports OSXSAVE, `xgetbv(0)` = 7; glibc's ifuncs pick the AVX2 string functions,
  `is_x86_feature_detected!("avx2")` is true.

## 2. Listener teardown

`impl Drop for UnixState`: connections still queued at a listening socket that goes away (last descriptor
closed, the process ended, `shutdown`) close their ends, so the connecting side reads end of file and its
writes get `EPIPE`. Before, the queue's pipe ends were dropped without releasing their reader/writer counts and
a client that had connected just before the server ended waited forever. Linux reports `ECONNRESET` there;
both end the wait.

## 3. SO_RCVTIMEO

`setsockopt(SOL_SOCKET, SO_RCVTIMEO, timeval)` on a connected Unix stream socket stores the limit (rounded
up to milliseconds) in the direction it reads (`Pipe::read_timeout_ms`; dup'ed descriptors share it);
`getsockopt` reports it. A blocking `read`, `recvfrom` or `recvmsg` that would wait waits at most that long
(the deadline is kept across the repeated call like poll's, `p.poll_dl`) and then fails with `EAGAIN`; a
receive that delivers ends the kept deadline. The bus client (`ultar-bus`, `establish`) sets a read limit
while it greets the broker; without it a greeting to a broker that was going away blocked the reader thread
forever – that is what kept `ultar-init` from restarting the machine after the installer's Restart.

## Gates run (2026-11-26, build machine: 2 cores, QEMU TCG)

Green: the `ci.sh` device boot (kerntest, busck, zelltest, rechtetest, geraetetest, weckertest, spawntest,
fenstertest, bttest uinput, avxtest), ausschalten, grenztest (512 MiB), ext4-durable, init-e2e, dienste-e2e,
installer-disk, pcm-e2e, AB-TEST (44 checks), vmware-like's three boots, installer-e2e. Not green, **also with the
kernel before this change** (same failure, checked with the old kernel on the same machine): `desktop-e2e` ("no
desktop bar after the first unlock") and `make-vmware` (services refused with `ready_timeout` – 35 services on
4 virtual cores over 2 host cores under TCG). Cost under TCG: a system call takes about 2.5 µs instead of 1.6 µs
(the emulated `xsave` of the AVX part); on hardware `xsave`/`xrstor` of x87+SSE+AVX cost some 100–200 cycles
more per entry than `fxsave`/`fxrstor` – not measured yet.
