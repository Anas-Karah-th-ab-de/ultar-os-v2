# Review round 1 – Intel display driver (CHANGES.patch, part A)

Role: Prüfer Sicherheit + Kern (independent). Base `ce287d2`, patch `/home/claude/agents/w5/intel/CHANGES.patch`
applied with `patch -p1` to a fresh clone (`/home/claude/agents/w5/review/intel`, commit "apply intel").
Scope: `drivers/kernel/intel-display/**`, `kernel/ultar-kern/src/drivers/intel_display/**`, kernel wiring
(`drivers/mod.rs`, `grafik.rs`, `selftest.rs`, `Cargo.toml`), `docs/integration/intel-display.md`,
`tools/kern-test/igpu-*.sh`, manifests. `KMS-GLUE.patch` is **not** part of this patch and was not reviewed.

## 0. What I ran (all on this machine, 2 CPUs)

| Check | Result |
|---|---|
| `cargo test --manifest-path drivers/kernel/intel-display/Cargo.toml` ×3 | 104 passed, 0 failed (×3) |
| `cargo clippy --all-targets -- -D warnings` (crate) | clean |
| `cargo fmt --check` (crate / workspace) | clean / clean |
| kernel `IMG=1 scripts/build-kern.sh` | builds; no warning from `intel_display` (38 pre-existing kernel warnings) |
| kernel `cargo fmt --check` | 38 diffs in the new `intel_display/*.rs` – kernel tree is not fmt-gated (935 pre-existing diffs) |
| `ultar-check all` | ok, 5550 baselined, **148 warnings** (base 147): new `drivers/kernel/intel-display/src/init.rs: file is getting long: 432 lines` |
| QEMU boot of the patched kernel (`qtest.py`) | driver does not bind, `gen-bochs-vga` active, no panic |
| Traceability | driver crate + kernel module of the patch are **byte-identical** to `ccf369d` (branch `wip/intel-display`, the hardware-tested commit) |
| Reviewer tests (`TESTS.patch`: `src/review_tests.rs`) | 7 tests: 4 pass (fuzzers, fallback proof), **3 fail = findings I-7, I-8** |

Hardware evidence (3/3 VFIO runs ALLES OK from cold) is taken as reported: no raw serial log of runs 6–8 is in the
deliverables (`/home/claude/agents/w5/intel/out/` holds the run-1 binary from 01:26). See I-15.

## 1. Findings

Severity: **blocking** (must be fixed before merge), **major** (fix before the feature is relied on), **minor**.

### I-1 major – Link-training fallback is dead code; DP-alt with 2 lanes cannot work
`drivers/kernel/intel-display/src/modeset.rs:245-264`, `src/dp/link.rs:72-104`.
`link::choose` already returns the *slowest rate and fewest lanes* that carry the mode. Every lower rate with the same
lane count, and 2 lanes at the same rate, were therefore rejected by `choose` itself, so `link::fallback(cfg)` and the
`or_else` two-lane branch can **never** return a config: the loop always ends after the first failed training.
Proof: `review_tests::link_fallback_is_unreachable_after_choose` sweeps 25–1300 MHz × 18/24/30 bpp × all rate/lane caps:
0 reachable fallbacks (test passes = finding holds).
Consequences: (a) the doc's "Fallback on training failure: lower rate, then 2 lanes" (§3) and "DP-alt pin assignment
… the fallback to 2 lanes covers it" (§0.3, §3) are false; (b) a USB-C DP-alt sink with pin assignment D (2 DP lanes,
the common hub/dock case; the Beelink's TC2 is such a port) advertising 4 lanes in DPCD gets RBR×4 for 1080p60, training
fails, and no 2-lane config (HBR2×2 would carry it) is ever tried. (c) Unit test
`modeset/tests.rs:201 failed_training_falls_back_and_cleans_up` claims a fallback but 1080p120 at HBR×4 has none: it
only proves cleanup after one attempt (vacuous name).
Fix direction: on failure re-run the choice with reduced limits (Linux: lower `max_rate`, then `max_lanes/2`, allow a
*higher* rate with fewer lanes), and read the TC pin assignment (FIA) before choosing.

### I-2 major – Resolution switch frees the console's buffer while the console still draws into it; no rollback
`kernel/ultar-kern/src/drivers/intel_display/anzeige.rs:430-436`, `device.rs:333-337, 366-380`, `grafik.rs` `modus()`.
`IntelAnzeige::setzen` → `modeset_dp` → `keep(new)` releases every kept buffer not on a plane, i.e. the buffer the
console currently draws into (second and later switches): `ggtt::unmap` (PTEs := 0) + `mem::free_run`. The console's
`SCREEN.fb` still points to that buffer's **aperture** address until `grafik::modus` calls `console::screen_init` after
`setzen` returned (and after the `ANZEIGEN` lock is dropped). Any `kprintln!` on another CPU in that window writes
through the aperture into GGTT entries that are no longer present (behaviour of aperture writes to non-present
entries is not defined by this driver – i915 never leaves them at 0, see I-4) – a use-after-unmap of DMA-visible
memory. Fix: switch the console first (or keep the old buffer until `screen_init` ran), then release.
Error path: `modeset_dp_plane` disables the running pipe (`device.rs:333-337`) *before* the new mode is validated
against the link/CDCLK; any later error leaves the screen dark, `pipes[p]` reset, the console pointing at a buffer no
longer scanned out, and no attempt to restore the previous mode. Reachable from the kernel shell (`anzeige WxH`).
This path (resolution switch) has never run on hardware (doc §0.2) and has no test.

### I-3 blocking (merge gate) – Driver binds by default on 19 device ids at every boot; stage A never ran there
`kernel/ultar-kern/src/drivers/mod.rs:+30`, `intel_display/mod.rs:169-186`, `device.rs:130-172, 353-363`,
`drivers/manifests/intel-display.toml` (19 PCI ids), `src/platform.rs:51-73`.
On every normal boot of T2 (bare metal, GOP pipe running) and of every listed MTL/ARL/CML machine (incl. T1) the
driver takes over: `disable_dc_states`, IMR unmask + IIR clear on all pipes, hot-plug enable, AUX power request and –
in `collect_modes → probe` – a Type-C PHY ownership handshake. The doc itself lists "takeover of a pipe the UEFI GOP
left running (bare metal only)" and T1 as **never run**; the VFIO runs exercise stage B in a VM without GOP. Eight
MTL/ARL and ten CML ids were never seen at all. A failed takeover leaves side effects (DC states off) and the GOP
fallback; a "successful" wrong takeover can cost the only display. Required before merge: opt-in (boot flag / not in
`ALL` by default) **or** one recorded bare-metal boot on T2 and T1 with the takeover log.

### I-4 minor – GGTT entries are cleared to 0, never pointed at a scratch page
`drivers/kernel/intel-display/src/ggtt.rs:80-86` (`unmap`), window setup in `device.rs:150-154`.
i915 fills every unused/cleared GGTT entry with a scratch page (`clear_range` → scratch PTE), because the display engine
prefetches past the end of a surface and older parts ignore the valid bit. Here released entries become 0 and window
entries never mapped keep whatever i915/firmware left (stale physical addresses of memory the kernel now owns). Scan-out
is read DMA, so the effect is an information leak to the screen / undefined prefetch, plus I-2's aperture writes.
Suggest a scratch page (one zeroed kernel page) for unmap and for the whole window at takeover.

### I-5 minor – GGTT window arithmetic with large apertures / high firmware surfaces
`device.rs:146,152`: `size.min(entries as u64 * 4096) as u32` truncates a 4 GiB aperture (resizable BAR2) to **0** →
`Window::new` error → takeover fails. `fw_end.next_multiple_of(1 << 20)` overflows (panic in a debug kernel, wraps in
release) when a firmware surface ends within 1 MiB of 4 GiB; `fw_end` uses `saturating_add` of hardware values.

### I-6 minor – OpRegion RVDA/RVDS trusted as an address
`intel_display/opregion.rs:16-24, 47-52`: `asls + rvda` (u64 from firmware) and `addr + len` are unchecked; `read_bytes`
identity-maps and reads whatever physical address results (non-canonical → #GP in the kernel; wrap-around → `end`
below `start`, nothing mapped, page fault). No check that the raw VBT lies in reserved/ACPI memory or near the
OpRegion. Firmware is semi-trusted, but the task asks for no panic paths from hardware data: bound `rvda` (e.g.
OpRegion-relative ≤ 16 MiB, below the top of physical memory) and use checked arithmetic.

### I-7 minor – Overflow on public validation paths (panic in debug builds)
`src/plane.rs:81-84`: `self.pitch < self.width * bpp` is evaluated **before** the `width > 5120` check → u32 overflow for
a huge width (KMS framebuffer values reach here through the glue). `src/mode.rs:50-52` `pair()`: `lo - 1` underflows
for a zero timing; `Timing::regs()` is `pub` and does not say "validate first". Release kernels wrap instead of
panicking, so not exploitable today; the dev kernel profile has overflow checks on and `panic = "abort"`.
Repro: `review_tests::plane_validate_does_not_overflow_on_huge_width`, `review_tests::timing_regs_on_unvalidated_timing` (both fail).

### I-8 minor – CTA-861 block with `d = 0` is parsed as if DTDs start at byte 4
`src/edid.rs:134-142`: `start = blk[2].clamp(4, 127)`; per CTA-861 `d = 0` means "no DTDs and no data blocks", but the
parser decodes bytes 4.. as detailed timings → bogus modes in `Device::modes` / the resolution list.
Repro: `review_tests::cta_block_with_d_zero_has_no_detailed_timings` (fails).

### I-9 minor – One odd firmware pipe aborts the whole takeover; takeover is not side-effect free on failure
`src/readout.rs:71-72, 117-125`: a running transcoder whose port bits do not decode returns `Err` and `read_all` (`?`)
fails the probe – the other (good) pipes are lost too. `device.rs:134` disables DC states *before* `read_all`; on error
nothing is undone. Same pattern in `modeset::enable_dp` (`modeset.rs:205-236`): errors from `core_init`, wells,
`dbuf_on`, PM demand, `tc_connect` return without releasing what was enabled (TC ownership stays taken).

### I-10 minor – Long operations under spin locks
`anzeige.rs:414` holds `DEVICE` (spin) for probe + full DP mode set (AUX retries up to 32×DEFER, 5×10 ms per transfer,
link training up to ~16 ms per step, `TRANSCONF` waits 100 ms) while `grafik::modus` also holds `ANZEIGEN`.
`sleep_us` is a busy wait, so no deadlock, but every CPU that reads `/proc/ultar/anzeige` / `geraet` spins for the whole
mode set (hundreds of ms to seconds).

### I-11 minor – Leaks on partial failure
`ScanBuf` has no `Drop`; `selftest.rs:149-153` leaks buffer `a` (pages + GGTT range) when only the second allocation
fails; `Device` replaced in `DEVICE` (second probe) drops `kept` without `release`.

### I-12 minor – Safety comments rely on something the kernel does not guarantee
`mmio.rs:1-3, 28-31` ("identity-mapped uncached by the PCI scan"): `pci.rs:127` calls `map_identity(.., true)`, but
`paging::map_identity` skips 2 MiB entries that are already present, and `paging::init` mapped `0..max(top, 4 GiB)` WB.
BARs below 4 GiB therefore stay **WB** and are uncached only through the firmware's MTRRs. Pre-existing kernel
behaviour, but the driver's SAFETY argument should say so (or the kernel should remap device ranges). Doc §1 says
`mmio.rs` is "the only `unsafe` besides cache flushes" – `opregion.rs:21` and `buffer.rs:116` are `unsafe` too.
`Hw::write` lets any computed register offset reach the GGTT half of BAR0 (8–16 MiB); all offsets I traced are bounded
(ports from `port_from_bits`/VBT map, pipes < `plat.pipes`), but a guard (`reg < 8 MiB` outside `ggtt.rs`) would make
"no GGTT write except through `ggtt::map/unmap`" a checked property.

### I-13 minor – Test quality
* `Sim` returns 0 for every unset register, so each `wait_clear` on a status bit the sim does not model succeeds at
  once (e.g. the waits in `disable_port`, `modeset.rs:271-276`); the disable/cleanup tests cannot see a missing wait.
* `failed_training_falls_back_and_cleans_up` – see I-1 (no fallback exists for that mode).
* INTELTEST "hot-plug status shows the active output" reads the live bit only (no plug event, no IIR); "flip back to
  the surface shown before" checks `is_ok()` of the CRC collection, not that the CRC equals the firmware picture.
* Kernel-side logic (`keep`/release, `aperture_addr`, `read_edid` extension rewrite, `takeover` window choice) has no
  host test; it only runs inside INTELTEST on hardware (and `setzen` not even there).
* Positive: parsers survive 20 000 random/mutated EDIDs, 20 000 VBTs, 50 000 DPCD/link-status blocks without panic
  (`review_tests`), AUX reply length is bounded (`aux.rs:132-139, 157-161`), every poll loop has a timeout
  (`hw.rs:263-275`, `gmbus.rs:46-58`, `device.rs:211-216`, `selftest.rs:48-57`, bounded `for` loops in AUX/training/PHY).

### I-14 minor – Gates
`ultar-check`: one new warning (`init.rs` 432 lines > 400). New kernel files not rustfmt-formatted (kernel not gated).

### I-15 minor – Evidence and doc consistency
* §0.1 says runs 6–8 started "from a cold display core (after FLR: wells 0, CDCLK off)", §5 says the script clears
  `reset_method` so that **no** FLR happens at VFIO open and "the display state i915 leaves stays". Both cannot describe
  the same run; please attach the serial logs (the `wells`/`CDCLK` lines of INTELTEST) of runs 6–8.
* §0.1 "3/3 green from cold" counts run 6 + 7 + 8 of `ccf369d`; that commit is identical to the patch (verified), good.
* `igpu-passthrough.sh` is sound (refuses shared IOMMU group / DRM holders, trap restores driver, `reset_method`, PM,
  FLR before rebind only when the driver changed).

## 2. Verdict

Memory safety of the MMIO/DMA core is careful (bounds + alignment in `Mmio`, PTE address mask, read-back of every PTE,
window allocator below 4 GiB, pages from the kernel allocator only, AUX/EDID/VBT bounds, timeouts everywhere).
**Blocking: I-3** (default binding of an untested takeover on all listed ids). **Major: I-1, I-2.** The rest is minor.
