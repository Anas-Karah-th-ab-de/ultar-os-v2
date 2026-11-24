# Intel GT driver – render and compute side of the integrated GPU (Ultar kernel)

Area: drivers (kernel part). Target: **T2** Beelink GTi15 Ultra, Arc 140T `8086:7d51` – graphics 12.74 "Xe_LPG+",
media 13.00 "Xe_LPM+" (Meteor Lake / Arrow Lake-H). Same PCI device and BAR0 as the display engine
(`docs/integration/intel-display.md`), so the GT lives in the `intel-display` kernel driver and under its opt-in.

Goal: 3D acceleration for the compositor and **local models on the integrated GPU** – through the Ultar kernel,
without Linux.

## 0. State and open points

| Stage | What | State |
|---|---|---|
| G0 | Identification (GMD_ID), forcewake, engines, GuC status, WOPCM read-out | **done, run on the Beelink** (iGPU through VFIO, 24.11.2026): graphics 12.74 Xe_LPG+ rev 4 `[rcs0 bcs0 ccs0]`, media 13.00 Xe_LPM+ rev 8 `[vcs0 vcs2 vecs0 gsccs0]`, WOPCM locked 2048K+832K |
| G1a | GuC firmware container, WOPCM layout, reset, boot parameters, RSA + DMA transfer, load status | **done, run on the Beelink**: GuC 70.53.0 transferred, `signature good`, the firmware starts and stops at `0x71 init data invalid` after 32 ms (expected without ADS); `GTTEST: ALLES OK`, i915 rebound cleanly |
| G1b | Minimal ADS block, boot parameters for the first load, GuC **ready**, register message channel (HXG over MMIO), hardware config table | **done, run on the Beelink** (24.11.2026): `firmware 0xf0 ready` 67 ms after the reset, table of 752 bytes with 56 entries fetched and parsed; `GTTEST: ALLES OK` |
| G1c | Command buffers (CTB: H2G/G2H descriptors, `HOST2GUC_SELF_CFG`, `CONTROL_CTB`), full ADS (engine mapping, register save/restore lists, golden contexts), second load with features and workarounds | open – next |
| G2 | Memory: GGTT manager for the GT, PPGTT (4 levels), buffer objects, TLB invalidation | open |
| G3 | Contexts (LRC), exec queues registered with the GuC, submission, G2H handling; proof: a batch on `ccs0`/`rcs0` writes a value | open |
| G4 | User interface: the Linux `xe` DRM uAPI on `/dev/dri/renderD128` (device query, GEM create/mmap, VM create/bind, exec queue, exec, syncobj) | open |
| G5 | Mesa (ANV/Vulkan, iris) from the Ubuntu packages unchanged; `vulkaninfo`, off-screen cube, llama.cpp Vulkan in `ultar-llm` | open |

1. **Hardware run** (`tools/kern-test/igpu-run.sh` with `STOP_UNITS="udl-llm-fast.service"`, log on the Beelink:
   `/root/ultar-test/gt-run1.log`, checkout `/root/ultar-igpu-npu`): `INTELTEST: ALLES OK` and `GTTEST: ALLES OK`;
   the whole run takes about 20 s, the host's i915 loads its own GuC again afterwards (function level reset).
   Five runs on 24.11.2026 (logs `gt-run1.log` … `gt-run5.log`, loop script `/root/ultar-test/gt-loop.sh <log>`);
   under VFIO only (no bare-metal start). The WOPCM write path (registers not yet locked) has not run
   on hardware: i915 had locked them, the driver kept that layout.
2. **GuC runs, engines do not yet.** With the minimal ADS the firmware is ready and answers requests, but no
   engine is mapped and nothing can be submitted. GTTEST puts the GuC back into reset. Nothing renders or computes
   before G3.
2a. **CPU caches and memory the GT writes** (found on hardware): the table the GuC copied read as zeroes in its
   first five cache lines until the kernel dropped those lines (`clflush`) before reading. Every buffer the GT
   writes and the CPU reads needs that, or an uncached/write-combining mapping (kernel issue I-12 of
   intel-display.md); buffers the CPU writes are flushed before the GT reads them.
3. **Interrupts.** The kernel has no MSI routing for PCI devices; G2H messages will be polled until it has
   (same limit as the display driver, intel-display.md §0.4).
4. **Media GT.** Identified and read (engines, GuC status); its GuC, HuC and the GSC are not loaded. Video decode
   is not part of the path to local models.
5. **Firmware file.** The kernel reads `mtl_guc_70.bin` from `\firmware` of the boot medium or
   `/lib/firmware/i915/`. Intel's licence allows redistribution; shipping it as a signed `.utr` package `intel-guc`
   (with the licence text) comes with G1b.
6. **Opt-in.** As the display: without `ultar.intel_display=on` (or the opt-in file) nothing of the GT is touched.
   With it, the probe only wakes the GT domain to read; the GuC is reset and loaded only by the self test.

## 1. Why this route (decision)

On Meteor Lake and later the GuC schedules the engines; there is no supported way around it. The shortest way to a
*useful* GPU is therefore: GuC up → memory and contexts → **the same user interface Linux' `xe` driver offers**.
Then Mesa's Vulkan and OpenGL drivers – hundreds of thousands of lines including the shader compilers – run
unchanged from the Ubuntu packages, and llama.cpp's Vulkan backend works in `ultar-llm` without a port. An own
user-space graphics stack would take years and gain nothing for the user.

What the integrated GPU brings for local models (to be measured in G5, no numbers promised here): it shares the
machine's RAM, so large models fit; prompt processing gains most; generation speed stays bound by memory bandwidth.

## 2. Layout

| Part | Where | What |
|---|---|---|
| Pure logic | `drivers/kernel/intel-gt/` (crate `ultar-intel-gt`, `no_std` + `alloc`, no `unsafe`, 42 host tests) | `ads` (minimal ADS block), `params` (boot parameter words per stage), `hxg` (register message channel), `hwconfig` (table parser), `ident` (GMD_ID → IP), `forcewake` (domains, references, bounded acknowledge wait), `engines` (descriptor + fuses), `fw` (CSS header, versions, RSA key position), `wopcm` (layout, locked registers, write-and-verify), `guc` (status decoding, reset, parameter words, RSA, DMA transfer, wait), `state` (read-out). Generic over `ultar_intel_display::Hw`, tested against `sim.rs`. |
| Kernel module | `kernel/ultar-kern/src/drivers/intel_display/gt.rs` | probe read-out (`[intel-gt] …`, `intel-gt=` line of the display report), GGTT buffers for the GuC (`GgttBuf`: kernel pages → the display driver's GGTT window, flushed), GTTEST. Two `unsafe` (copy into and out of owned pages) with `// SAFETY:`. |
| Test on the machine | `tools/kern-test/igpu-passthrough.sh` | copies the GuC firmware (`GUC=<file>|0`), waits for the GTTEST verdict, prints both verdicts. |

## 3. Sequences (as implemented)

* **Forcewake:** write `mask<<16 | bit` to the domain's request register, wait up to 50 ms for the acknowledge
  bit; all ones = device gone; a failed wake withdraws the request. The media GT's registers below `0x40000` are
  the same offsets + `0x380000`.
* **WOPCM:** `[16K reserved | HuC | GuC part (16K-aligned start) | 36K contexts]`, 4 MiB on Meteor Lake. Registers
  `GUC_WOPCM_SIZE`/`DMA_GUC_WOPCM_OFFSET` lock on the first write until the next reset: a locked layout is kept if
  the image fits, otherwise the upload is refused (no write).
* **Upload:** `GDRST = GRDOM_GUC` → wait → core in reset → parameters to `SOFT_SCRATCH(1..14)` → shim control,
  doorbells, ARAT interrupt → RSA: key > 256 bytes (Meteor Lake: 384) by its GGTT address in `UOS_RSA_SCRATCH(0)` →
  DMA from the GGTT to WOPCM offset `0x2000`, header + code in one transfer → poll `GUC_STATUS` (ready `0xF0`,
  failure codes of boot ROM and firmware, 3 s).
* **Minimal ADS:** `[guc_ads 4572 | policies 96 | system info 640 | engine usage 16384 | um init 128 | pad | private
  data]` – policies valid (promote time 500 ms, 15 work items), engine mapping table all "invalid" (32), doorbells
  per SQIDI from `DIST_DBS_POPULATED`, the three addresses (policies, system info, private data); everything else 0.
  Boot parameters of this load: log, ADS, device id, logging off; features and workarounds 0.
* **Register messages:** request words to `VF_SW_FLAG(0..3)` (`0x190240`), posting read, write 0 to
  `GUC_HOST_INTERRUPT` (`0x1901f0`), wait for origin = GuC in word 0 (50 ms), ride out "busy" (2 s), resend on
  "retry" (bounded), failure → error + hint. `GET_HWCONFIG` (`0x4100`) with size 0 returns the table size, with a
  GGTT address copies it.
* **GGTT range of the GuC:** addresses below the WOPCM size and from `0xFEE00000` are not reachable for it; every
  buffer is checked before any register is written.

## 4. Sources

Linux `xe` driver (MIT): `regs/xe_gt_regs.h`, `regs/xe_guc_regs.h`, `xe_force_wake.c`, `xe_pci.c`, `xe_hw_engine.c`,
`abi/uc_fw_abi.h`, `xe_uc_fw.c`, `xe_wopcm.c`, `xe_guc.c`, `abi/guc_errors_abi.h` (read at commit `8d7fe98`).
`xe_guc_ads.c`, `xe_guc_fwif.h`, `xe_guc_hwconfig.c`, `abi/guc_messages_abi.h`.
No GPL code. For G1c–G3: `xe_guc_ct.c`, `abi/guc_communication_ctb_abi.h`, `xe_ggtt.c`, `xe_pt.c`, `xe_lrc.c`, `xe_guc_submit.c`;
for G4: `include/uapi/drm/xe_drm.h`; register and command layouts: Mesa `src/intel/genxml/` (MIT).
