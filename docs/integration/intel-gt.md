# Intel GT driver – render and compute side of the integrated GPU (Ultar kernel)

Area: drivers (kernel part). Target: **T2** Beelink GTi15 Ultra, Arc 140T `8086:7d51` – graphics 12.74 "Xe_LPG+",
media 13.00 "Xe_LPM+" (Meteor Lake / Arrow Lake-H). Same PCI device and BAR0 as the display engine
(`docs/integration/intel-display.md`), so the GT lives in the `intel-display` kernel driver and under its opt-in.

Goal: 3D acceleration for the compositor and **local models on the integrated GPU** – through the Ultar kernel,
without Linux.

## 0. State and open points

| Stage | What | State |
|---|---|---|
| G0 | Identification (GMD_ID), forcewake, engines, GuC status, WOPCM read-out | written, 29 host tests, kernel builds, QEMU boot unchanged; **not run on the Beelink yet** |
| G1a | GuC firmware container, WOPCM layout, reset, boot parameters, RSA + DMA transfer, load status | written and simulated; GTTEST drives it on hardware; **not run on the Beelink yet** |
| G1b | ADS block, log buffer layout, CTB (H2G/G2H), `HOST2GUC_SELF_CFG`, hardware config table → GuC **ready** | open – next |
| G2 | Memory: GGTT manager for the GT, PPGTT (4 levels), buffer objects, TLB invalidation | open |
| G3 | Contexts (LRC), exec queues registered with the GuC, submission, G2H handling; proof: a batch on `ccs0`/`rcs0` writes a value | open |
| G4 | User interface: the Linux `xe` DRM uAPI on `/dev/dri/renderD128` (device query, GEM create/mmap, VM create/bind, exec queue, exec, syncobj) | open |
| G5 | Mesa (ANV/Vulkan, iris) from the Ubuntu packages unchanged; `vulkaninfo`, off-screen cube, llama.cpp Vulkan in `ultar-llm` | open |

1. **Hardware run pending.** `tools/kern-test/igpu-run.sh` on the Beelink now also runs GTTEST (the script copies the
   host's `mtl_guc_70.bin` to the boot medium). Expected: `GTTEST: ALLES OK` with
   `GuC image transferred and signature accepted` and the note that the firmware stops at its init data.
2. **GuC not running yet.** Without the ADS the firmware stops with "init data invalid" after the boot ROM accepted
   it. GTTEST puts the GuC back into reset. Nothing renders or computes before G3.
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
| Pure logic | `drivers/kernel/intel-gt/` (crate `ultar-intel-gt`, `no_std` + `alloc`, no `unsafe`) | `ident` (GMD_ID → IP), `forcewake` (domains, references, bounded acknowledge wait), `engines` (descriptor + fuses), `fw` (CSS header, versions, RSA key position), `wopcm` (layout, locked registers, write-and-verify), `guc` (status decoding, reset, parameter words, RSA, DMA transfer, wait), `state` (read-out). Generic over `ultar_intel_display::Hw`, tested against `sim.rs`. |
| Kernel module | `kernel/ultar-kern/src/drivers/intel_display/gt.rs` | probe read-out (`[intel-gt] …`, `intel-gt=` line of the display report), GGTT buffers for the GuC (`GgttBuf`: kernel pages → the display driver's GGTT window, flushed), GTTEST. One `unsafe` (copy of the firmware into owned pages) with `// SAFETY:`. |
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
* **GGTT range of the GuC:** addresses below the WOPCM size and from `0xFEE00000` are not reachable for it; every
  buffer is checked before any register is written.

## 4. Sources

Linux `xe` driver (MIT): `regs/xe_gt_regs.h`, `regs/xe_guc_regs.h`, `xe_force_wake.c`, `xe_pci.c`, `xe_hw_engine.c`,
`abi/uc_fw_abi.h`, `xe_uc_fw.c`, `xe_wopcm.c`, `xe_guc.c`, `abi/guc_errors_abi.h` (read at commit `8d7fe98`).
No GPL code. For G1b–G3: `xe_guc_ads.c`, `xe_guc_ct.c`, `xe_ggtt.c`, `xe_pt.c`, `xe_lrc.c`, `xe_guc_submit.c`;
for G4: `include/uapi/drm/xe_drm.h`; register and command layouts: Mesa `src/intel/genxml/` (MIT).
