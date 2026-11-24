# Intel display driver (Ultar kernel)

Area: drivers (kernel part). Targets: **T2** Beelink GTi15 Ultra, Arc 140T `8086:7d51`, display version 14
(Meteor Lake display, "Xe_LPD+"); **T1** ThinkPad P1 Gen 3, UHD 630 `8086:9bc4`, display version 9 (takeover only).
Patch: `CHANGES.patch` against `ce287d2`; KMS glue: `KMS-GLUE.patch` (apply after the KMS core, see §6).

## 0. Open points

0. **Opt-in (review round 1, I-3):** the driver writes to the hardware only on machines that opted in – boot option
   `ultar.intel_display=on` (taken out of the UEFI load options before the A/B chain parses them; the A/B loader does
   not forward it), the file `\ultar\intel-display` with `on` on the start partition, or the device id in
   `optin::ALLOWLIST` (**empty**). Without opt-in the probe only reads (no PCI command write, no register write: plat
   + readout), reports `not driven, read only (…)`, does not bind, and the console stays on the GOP framebuffer.
   The VFIO test writes the opt-in file into its boot medium. Plan: add `0x7d51` to the allowlist after the
   recorded bare-metal boot of the Beelink with the opt-in; T1 (`0x9bc4`) likewise after its own boot.
   **A hardware re-run is needed for the round-1 changes** (opt-in path, scratch page, switch with rollback,
   readout/takeover rework): `igpu-run.sh` (with opt-in, expects `ALLES OK`) and `OPTIN=0 igpu-run.sh` (expects
   `READ-ONLY OK`).
1. Hardware (Beelink, VFIO): run 6 (ccf369d) **INTELTEST: ALLES OK from a cold display core** – cold because i915
   turns the display core off when it is unbound (INTELTEST printed wells `0x0`, CDCLK off); there is **no** FLR when
   QEMU opens the device (`reset_method` cleared, §5), the only FLR is in the trap before i915 is bound again – core init, DP mode set TC4 1080p60 RBR x4 on the VX3218, vblank 60.018 Hz (CPU) / 59.999 Hz
   (display base), PLANE_SURFLIVE ok, no FIFO underrun, CRC A≠B, A=A, flip back, hot-plug, EDID. Runs 1–2 (warm core)
   mode set + CRC ok. History of fixes: time base not 1 MHz (run 1), edge-wait limit (2), core init from cold (3–4),
   IRQ masks lost in cold PW1 (4), DBUF S1 address/slices + PM demand (5). Host stays clean (FLR + i915 rebind, no
   wedged GT; named units via STOP_UNITS restarted). Stability runs 7–8 on ccf369d: both ALLES OK with identical CRCs (3/3 green from cold).
   Raw logs of runs 6–8 (serial INTELTEST lines, register snapshots, i915 rebind): on the Beelink in
   `/root/ultar-test/igpu.log` (run 6), `igpu-s1.log` (run 7), `igpu-s2.log` (run 8); attached by the coordinator.
2. Never run: takeover of a pipe the **UEFI GOP** left running (bare metal only; unit-tested against the Linux dump),
   T1 (display 9), two monitors at once, HDMI/DP-alt/TBT outputs, resolution switch through `anzeige` (now with
   rollback, simulated: `failed_switch_restores_the_old_mode`, `refused_switch_touches_nothing`), the link-training
   fallback on real hardware (simulated: `failed_first_attempt_comes_up_on_the_fallback_link`).
3. Not implemented: HDMI and eDP outputs, ports A/B (C10 PHY), MST/DSC/FEC/UHBR, CDCLK change while another pipe
   runs (crawl), watermark levels 1–7 (power only), cursor plane, DMC/DC states, suspend/resume, display 9 mode set
   and hot-plug, DP-alt pin assignment read-out (the training fallback reaches 2 lanes), TBT-alt mode.
4. Interrupts: the kernel has no MSI routing; vblank, flip done, CRC and hot-plug are polled (sources unmasked in
   IMR, read from IIR). Hot-plug at run time is polled by the KMS glue every 100 ms – not yet run with the KMS core.
5. KMS: `KMS-GLUE.patch` compile-checked against the KMS agent's tree, not run; `flip` flushes the whole fb from the
   CPU caches (~0.5 ms at 1080p, over the 50 µs budget) until dumb buffers are mapped WC/UC for user space.
6. VBT: read from the OpRegion on bare metal; in the VM there is none (every Type-C port offered as DP).
7. Kernel issue outside the driver: `auto-aus` (UEFI ResetSystem after ExitBootServices) hangs/raises #UD.
8. Kernel issue outside the driver (review I-12): `paging::init` maps `0..max(RAM, 4 GiB)` write-back and
   `map_identity` keeps existing entries, so BAR0 below 4 GiB is uncached only through the firmware's MTRRs
   (PAT WB + MTRR UC = UC). True on every PC firmware seen; a kernel-wide UC remap of device ranges is the fix.
9. A mode set holds the `DEVICE` spin lock for its whole length (up to ~1 s with AUX retries); `text()` does not
   wait for it (`busy`), the KMS glue and `anzeige` do (the kernel has no sleeping mutex for drivers yet).

## 1. Layout

| Part | Where | What |
|---|---|---|
| Pure logic | `drivers/kernel/intel-display/` (crate `ultar-intel-display`, `no_std` + `alloc`, no `unsafe`) | register layout, firmware state readout, timings, GGTT entries + window allocator, vblank counter/time stamps, pipe CRC, hot-plug decoding, VBT child devices, AUX/DPCD/I2C-over-AUX, GMBUS, EDID, link config + M/N, link-training state machine, C20 PHY message bus + PLL tables + PLL enable/disable + drive levels, power wells/DC states/DBUF, CDCLK check, DDB + level-0 watermarks, DP mode set and disable sequence. Every sequence is generic over `Hw` (read/write/delay/now) and tested against a simulated register file (`sim.rs`, `phy/sim_phy.rs`, `dp/sink.rs`). |
| Kernel module | `kernel/ultar-kern/src/drivers/intel_display/` | `unsafe` in three files, each with `// SAFETY:`: `mmio.rs` (BAR0 volatile access; `write` refuses the GGTT half of BAR0, only `write_gtt` – called from `ggtt.rs` alone – reaches the table), `buffer.rs` (contiguous kernel pages → GGTT, clflush, drawing), `opregion.rs` (VBT from ASLS/OpRegion; RVDA/RVDS bounded by `vbt::raw_vbt_range`, checked arithmetic). `device.rs` (takeover, flips, vblank), `connector.rs` (hot-plug, probe, EDID), `stage_b.rs` (mode set with rollback, pipe off, buffer lifetime), `anzeige.rs` (`grafik::Anzeige`: EDID modes + resolution switch), `selftest.rs` (INTELTEST), `mod.rs` (PCI driver `intel-display`, opt-in, read-only probe). |
| Test on the machine | `tools/kern-test/igpu-passthrough.sh`, `igpu-run.sh` | iGPU → vfio-pci → Ultar kernel under KVM, verdict, i915 always rebound (trap). |
| Package | `drivers/manifests/intel-display.toml` + `payload/intel-display/builtin/` | built-in activation record (stage A of plan §5.5); planned `intel-display-xe` manifest removed (it is this one), `intel-display-gen9` stays planned (mode set for display 9). |

Wiring: `drivers::ALL` has `intel_display::DRV` after `bruecke` (matches only vendor 8086, class 03 and a device id
of `platform.rs` – it never binds in QEMU; on an Intel machine it binds only with the opt-in, §0.0).
`selftest::run()` calls `intel_display::selftest::run()`. `grafik::text()` adds an `intel-display=` line.
`main.rs` takes the opt-in word out of the load options and reads the opt-in file (`efi::datei_lesen`, the old
`sprache_lesen` body made generic). `grafik::Anzeige` got `umgeschaltet()` (default empty), called by `grafik::modus`
after the console moved to the new framebuffer. Kernel `Cargo.toml` depends on the crate by path (allowed by the layer table:
kernel → `drivers/kernel`). `ci.sh` runs the crate's unit tests.

## 2. Stage A – takeover (done)

Probe: enable memory/bus master; read every pipe (`TRANSCONF`, `TRANS_DDI_FUNC_CTL` → port, DP/HDMI, bpc, lanes;
timing registers; link M/N; `DDI_CLK_VALFREQ` → port clock → pixel clock; plane 1 → surface, stride, size,
format). Nothing on the screen changes. DC5/DC6 are turned off (no DMC handling); if the takeover fails after that,
`DC_STATE_EN` is written back. A running pipe that does not decode (e.g. no port) is left alone and listed
(`BootFacts::skipped`) instead of failing the others. The firmware plane must be linear, unrotated, uncompressed RGB
(else it is left alone and reported). GGTT: table size from `GGC`; kernel buffers go into a window of the aperture
range above the firmware buffer (or the upper half of the GGTT when the aperture is small; all window arithmetic in
`u64` with checked rounding, a 4 GiB aperture is not truncated); entries copy the firmware entry's attribute bits
(display 14 without firmware entry: PAT index 2 = uncached). **Scratch page:** one zeroed page; every entry of the
window points at it at takeover and again when a buffer is released (as i915: the display prefetches past a
surface, older parts ignore the valid bit) – no entry is 0 or a stale address.

* **Flip**: `PLANE_SURF` write (arms all plane registers at the next blank); done when `PLANE_SURFLIVE` shows it.
* **Vblank**: 64-bit extension of `PIPE_FRMCOUNT`; time = `uptime_ns` minus the age of `PIPE_FRMTMSTMP` against
  `TIMESTAMP_CTR`. The rate of that display time base is **measured against the CPU clock at takeover** (20 ms):
  on the Arc 140T it is not the 1 MHz of older generations (first hardware run: assuming 1 MHz gave 23 Hz for a
  60 Hz pipe). **No interrupts**: the kernel has no MSI routing for PCI devices; the
  pipe sources (vblank, flip done, CRC done, underrun) are unmasked in `IMR` so they latch in `IIR`, and are polled.
  The KMS core polls too (contract §2.3), so nothing is lost; a vblank IRQ would only save wake-ups.
* **Pipe CRC**: `PIPE_CRC_CTL` source DMUX (pipe output), results from `PIPE_CRC_RES` on CRC-done.
* **Hot-plug** (display 14): legacy Type-C connectors via `SDEISR` (+ `SHOTPLUG_CTL_TC`), DP-alt/TBT via the PICA live
  status (+ `XELPDP_PORT_HOTPLUG_CTL`), ports A/B via `SDEISR`. Changes are found by comparing live masks.
* **Type-C ownership**: before AUX or a mode set on a Type-C port: TCSS power request → `TCSS_DDI_STATUS.READY` →
  TCSS power state → `TC_PHY_OWNERSHIP` (Linux' `xelpdp_tc_phy_connect`); given back on disable. TBT-alt mode is not
  driven (no TBT PLL path).
* **EDID**: DP: I2C-over-AUX at 0x50 (AUX power request + 600 µs on display 14); HDMI: GMBUS pin from the VBT.
  Base block + first extension (blocks 2/3 need the E-DDC segment pointer – the image is rewritten to say one
  extension, checksum kept valid). Every block is checksum-checked.
* **Outputs** come from the VBT (OpRegion mailbox 4 or RVDA); without one (VM) every Type-C port is offered as DP.
  Beelink VBT: TC1 DP, TC2 DP (USB-C), TC3 HDMI, TC4 DP++ (DP-3 / HDMI-A-2, the monitor).

**DMC firmware decision:** not loaded and not shipped. DMC only saves power (DC5/DC6/DC3CO, pipe DMC for PSR/LRR);
basic operation does not need it, and the driver turns DC states off. When power saving is implemented, `mtl_dmc.bin`
(Intel's redistributable licence permits shipping) goes into a separate signed `.utr` package `intel-dmc` with the
licence text in its payload, loaded by the driver from the package store.

## 3. Stage B – mode set (DP on Type-C, display 14: implemented, simulated, hardware test pending)

`modeset::enable_dp`: timing check → **display core init** (`init.rs`, each step skipped if already up: DC off,
PCH+PICA reset handshake, PW1 with fuse PG0/PG1 handshakes and PG1 latch release, CDCLK = lowest MTL operating
point ≥ need (min 192 MHz): PM-demand voltage/clock request, DE PLL ratio + lock, squash waveform, `CDCLK_CTL`;
DBUF S1; on a cold PW1 also the arbiter BW-buddy page mask from `MTL_MEM_SS_INFO_GLOBAL`; CDCLK is only changed
with no other pipe running) → power wells PW1/PW2/PW_<pipe>, the DBUF slices of the pipe's DDB range (4 slices of
1024 blocks: pipe A/C [0,2048) = S1+S2, B/D = S3+S4), PM-demand activity (pipes/PHYs/PLLs/DDI clock) → Type-C connect → AUX power → link config (lowest rate, then fewest lanes, 0.5 % reserve) → C20 PLL
(clock mux, lane reset, P2, SRAM context + vendor registers + context toggle, transmitters, `DDI_CLK_VALFREQ`,
PCLK PLL request/ack) → `TRANS_CLK_SEL` → `TRANS_DDI_FUNC_CTL` → sink D0 → `DP_TP_CTL` TPS1 → D2D link → drive
level → port width → `DDI_BUF_CTL` enable (PHY not idle) → link training (DP 1.4 CR + EQ with TPS2/3/4, swing/
pre-emphasis from the sink, C20 drive table) → normal → timings (+ context latency 0) → M/N → MSA → DDB (pipe
half of 2048 blocks) + level-0 watermarks (Intel method 1/2) + line time → plane → `CHICKEN_TRANS` FEC jitter WA →
function enable → `TRANSCONF` enable, wait running. **Fallback on training failure (DP 1.4 §3.5.1.2.2, Linux'
order):** retry at the next lower rate with the same lane limit; at the lowest rate halve the lanes and allow the
sink's highest rate again; each retry takes the cheapest config that fits (`link::next_after_failure`); the port is
turned off between attempts. Errors turn port and pipe off again and give the Type-C PHY back (also when the TCSS
handshake times out). **Switch with rollback** (`modeset::switch_dp`): the new mode is checked first (timing, plane,
port, link, CDCLK) – a refusal touches nothing; then the old mode goes off and the new one is set; if that fails, the
old mode is set again with its own surface and the outcome of that is returned too. `anzeige WxH` keeps the old
console buffer mapped and shown until the new mode runs and the console moved (`Anzeige::umgeschaltet`), only then
it is released; on failure the console stays on the restored mode's buffer. `modeset::disable` reverses it. After a mode set the driver writes the pipe
interrupt masks and hot-plug enables again (they live in PW1 and are lost when the core was cold). Per pipe also
the MBUS DBOX credits.

Unit test `lights_the_beelink_monitor_like_linux`: the simulated mode set for 1080p60 on TC4 produces exactly the
register values Linux left (`TRANS_DDI_FUNC_CTL 0xba030006`, `DATA_M1 0x7e580000`, `LINK_M1 0x75555`, timings,
`TRANS_CLK_SEL 0x90000000`, `PIPE_WM_LINETIME 0x77`, C20 RBR context `0xbe88/0x5800/0x50a8`, serdes `0x40`).

**Not done in stage B:** HDMI (C20 HDMI tables/TMDS, scrambling), DP on ports A/B (C10 PHY), CDCLK change while
another pipe runs (crawl/squash on the fly), MBUS joining, MST, DSC, FEC, UHBR, SSC, watermark levels 1–7 (power only), TBT-alt, DP-alt pin assignment D lane count
(read from FIA registers; the fallback to 2 lanes covers it), cursor plane, display 9 mode set (Gen9 DPLL/DDI).
Two monitors (extend/mirror): the sequence is per pipe/port and DDB is split by pipe, so pipe B on TC1/TC2 is the
same call – **not yet run on hardware**.

## 4. INTELTEST (self test)

Runs from the kernel self test (`ultar-kern/selbsttest.txt` on the boot medium). Lines `INTELTEST: ok|FAIL …`, end:
`INTELTEST: ALLES OK` or `INTELTEST: FEHLER (<list>)`. Checks: an active pipe (firmware, or stage-B mode set on the
first DP output whose sink answers on AUX), vblank rate within ±1 % of the mode (120 frames between two frame-counter
edges timed with the CPU clock; the rate from the display time base is printed too), pipe CRC of pattern A, B, A (A≠B, A=A, all stable over 4
frames after 2 skipped), flip back to the surface shown before the test **with the same CRC as before**, hot-plug
live status shows the active output (live bit; no plug event is generated), EDID valid. Without the opt-in the
driver does not bind and INTELTEST prints `no Intel display device bound`.

## 5. Hardware test (`tools/kern-test/igpu-run.sh` on the Beelink)

Builds the kernel, prints EDID/reset method/tools, runs `igpu-passthrough.sh` (LIMIT=240): refuses if a process holds
the GPU's DRM nodes or the IOMMU group is shared; register snapshot with i915; `reset_method` cleared (no FLR when
QEMU opens the device – the display state i915 leaves stays: wells, CDCLK, DBUF) and runtime PM off; i915 unbound,
snapshot, vfio-pci bound; QEMU q35 + OVMF, `vfio-pci,addr=02.0,rombar=0,x-igd-legacy-mode=off,x-igd-opregion=on`
(retried without the IGD options if QEMU refuses them), virtio-blk boot medium with the self-test trigger and the
opt-in file `\ultar\intel-display` (`OPTIN=0`: without it; verdict `INTELTEST: READ-ONLY OK` when the probe reported
`not driven` and nothing bound);
QEMU is stopped as soon as the verdict line is in the serial log (the kernel's `auto-aus` power-off is not used: on
the Beelink it ended in an invalid-opcode exception inside the UEFI runtime `ResetSystem` call – a kernel issue
outside this driver, reproduced as a hang in QEMU/OVMF). The trap restores `reset_method` and power control,
**does a function level reset**, binds i915 again in every case and checks the kernel log for a wedged GT. Without
the FLR (first run) i915 came back with its display working but the GT "wedged" (GuC/GSC areas stay locked from
the first firmware load): the FLR gives i915 the device as after power-on. In the VM there is no GOP (OVMF has no Intel GOP driver), so the test exercises
stage B first, then the stage-A paths on the pipe it lit. The display core the VM sees is cold because i915's unbind
turns it off (not because of a reset).

## 6. KMS core interface (`KMS-GLUE.patch`)

`intel_display/kms.rs` implements `gpu::kms::driver::KmsDriver` (contract `kernel-kms.md` §2, as of 2026-11-24):
CRTC = pipe, encoder = output (Tmds), connector per signal (DP++ → DP + HDMI-A), `name "i915"`,
`owns_boot_display` when the firmware runs a pipe, no cursor (`cursor_size 0`), pitch align 64.
`check`: full-screen primary without scaling, supported format, timing valid, and either the running firmware mode on
that output (fast set) or a Type-C DP output (mode set). `commit`: fast set = plane only; else `modeset_dp_plane`.
`flip`: GGTT mapping once per fb (`fb_removed` points the entries back at the scratch page), cache flush, `PLANE_SURF`; `poll` reports `FlipDone` when
`PLANE_SURFLIVE` matches and hot-plug every 100 ms. Compile-checked against the KMS agent's tree.
**Contract notes for the KMS core:** (a) `flip` flushes the whole fb from the CPU caches (~0.5 ms at 1080p) – over
the 50 µs budget; the display engine does not snoop. Better: map dumb buffers write-combining/uncached for user
space, then the flush can go. (b) `probe` of a DP output may take ownership of a Type-C PHY (TCSS handshake ≤ 10 ms).

## 7. Tests (numbers)

* `ultar-intel-display`: **123 unit tests** (incl. the reviewer's `review_tests.rs`), all pass (3×); `cargo clippy
  --all-targets -D warnings` clean; fmt clean. Kernel driver files rustfmt-clean. `Sim` answers 0 for unmodelled
  registers (a wait for a bit to clear passes at once); the waits that matter have their own test
  (`disable_reports_a_port_that_does_not_go_idle`). Kernel-side logic moved into the crate where it is pure and tested:
  window choice (`Window::in_aperture/above_firmware`), buffer release (`optin::releasable`), EDID extension rewrite
  (`edid::claim_extensions`), opt-in parsing (`optin`), raw VBT bounds (`vbt::raw_vbt_range`).
* Kernel builds (no new warnings); QEMU boot: driver does not bind (`1234:1111 → gen-bochs-vga`), `anzeige` unchanged.
* `ultar-check all`: ok (no new baseline entries, no warning in driver files). `tools/kern-test/ci.sh` green, and
  `ab-test.sh` (AB-TEST: ALLES OK, 44 checks): its panic-kernel copy now includes `drivers/kernel/intel-display`
  (a kernel path dependency since this driver; without it that build failed silently).
* Hardware: §0.1 (round-1 changes need the re-run of §0.0). Suspend/resume: not handled (the cold-start init is the base for it).
