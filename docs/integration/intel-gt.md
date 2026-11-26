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
| G1c | Command buffers (CTB): H2G/G2H rings, registration (`SELF_CFG` keys, `CONTROL_CTB`), send, receive, polled request | **done, run on the Beelink** (25.11.2026): buffers enabled, the opt-in request Linux sends first (`0x550e`) answered in 108 µs; `GTTEST: ALLES OK` |
| G1d | Full ADS (enabled engines, mapping, per-engine register lists, golden context slots, empty capture list) and second load with features and workarounds | **done, run on the Beelink** (25.11.2026): `firmware 0xf0 ready` with render, copy and compute engine announced; command buffers answer after it. Golden context images are still zero (filled in G3) |
| G2 | Memory: page tables (PPGTT) as pure logic, table memory and buffers in the kernel | **done, run on the Beelink** (25.11.2026) as part of G3: batches run in a context's own address space. TLB invalidation through the GuC runs on the Beelink as well (run 17) |
| G3 | Contexts (LRC), registration with the GuC, scheduling policy, enable/submit, events, disable/deregister; proof: an engine writes a value | **done, run on the Beelink** (25.11.2026, runs 13 and 14): `bcs0` runs a store command from the ring (GGTT only); `bcs0`, `rcs0` and `ccs0` each run a batch from their own page tables that stores a value through them; every context is disabled and deregistered again; `GTTEST: ALLES OK`. Runs 15–17 (25.11.2026): a second job on an enabled context (`SCHED_CONTEXT`), the saved images copied into the ADS as golden contexts, and for each class a context that starts from its golden image; job buffers are given back after each context, with TLB invalidation. Still open: workarounds and default 3D state in the golden images (2c), engine reset handling |
| G4 | User interface: the Linux `xe` DRM uAPI on `/dev/dri/renderD128` | **Done as far as listed, run on the Beelink** (25.11.2026, runs 18–25; `DRITEST: ALLES OK`, 82 checks by a program): `DRM_IOCTL_VERSION` (driver "xe" 1.1), `GET_CAP`, device queries; **buffers** (`GEM_CREATE`, `GEM_CLOSE`, `GEM_MMAP_OFFSET` + `mmap` of the node – kernel memory objects shared with the program); **address spaces** (`VM_CREATE`/`DESTROY`, `VM_BIND` map/unmap/unmap-all, one or many operations); **queues** (`EXEC_QUEUE_CREATE`/`DESTROY`/`GET_PROPERTY`) on render, copy and compute engine; **`EXEC`** of one batch with **user fences** and **sync objects** (`DRM_IOCTL_SYNCOBJ_*`: create, destroy, wait, timeline wait, reset, signal, timeline signal, query, transfer); `WAIT_USER_FENCE`. Proven by `user/dritest.c`: each engine stores into a buffer the program mapped (read back without any flush), 600 jobs through one ring, rebinding, refusals, a process that exits without cleanup, and a **batch that never ends** – after 5 s its queue is stopped, the GuC resets the engine, the waiter is let go, and all three engines work afterwards. The GT stays in operation after the self-test (`gt_run.rs`, 94 ms). Logic `uapi.rs`, `uapi/{drm,file,sync}.rs`; kernel `svcdev/dri.rs`, `intel_display/gt_exec.rs`. **Not there** (each refused, not faked): sync objects and buffers passed between programs as file descriptors (PRIME, sync files), user-pointer bindings, several batches or engines per queue, fault-mode address spaces, the scratch page (accepted, not implemented: a touch outside the bindings faults), observation/performance requests. A job that waits for a sync object makes its `EXEC` wait instead of being queued. Jobs are found finished by looking (interrupts since G5, see there); waits for the GuC keep the kernel busy (up to 3 s when a queue is stopped). A normal boot does not start the GT yet (only the self-test; never run on bare metal). Added for Mesa (G5): the GT topology query, bindings to nothing (null), bind queues, splitting a binding that is partly replaced |
| G5 | Mesa (ANV/Vulkan) and llama.cpp unchanged, built from source by `tools/ultar-paketbau` (packages `grafik-laufzeit`, `llama`; no Ubuntu packages); compute shader; a language model; measure | **Done, run on the Beelink** (25.11.2026, VFIO, 8 cores and 57 GB for the VM): Qwen3.6-35B-A3B Q4_K_XL (21 GB, from a read-only disk) entirely on the GPU with `llama-bench`. Ultar / Linux on the same machine with the same package: prompt 64 tokens 114.6 / 113.6 t/s, 512 tokens 257.8 / 279.8 t/s, generation 32 tokens 18.66 / 18.74 t/s, 128 tokens 18.65 / 18.88 t/s (two runs each). What it took beyond G4: `/sys/dev/char` and the node's device number, GT topology query, null and partly replaced bindings, bind queues, sparse memory files; large disk files read on demand and shared with mappings; jobs that flush before they store their number; **the MOCS table** (left at reset the GT did not cache in L3: 22 t/s for the prompt); the cache attribute the program names (write-combining buffers mapped past the CPU caches, IA32_PAT entry 1); 2 MiB GPU pages; GPU waits that sleep (idle cores tick every 250 µs instead of a core polling, which cost the GT its power budget); no GuC power management (SLPC, see below). **Since then** (25.11.2026): the GuC's power management **SLPC runs** (`slpc.rs`, `gt_slpc.rs`: shared data, reset after a GuC TLB invalidation – without it the GuC refuses the address with 0x80 –, limit RP0, GuC RC6 control, compute strategy; the lowest frequency is raised to RP0 while programs have queues, as Linux' waitboost) and **GPU interrupts** (`irq.rs`, `gt_irq.rs`: MSI vector 0x41, `MI_USER_INTERRUPT` at each job end, xe's DG1 acknowledge path; the handler wakes waiting threads and IPIs sleeping cores; 27 453 engine events for 26 840 jobs, none lost); the 250 µs timer stays only as fallback. Measured: pp64 116, pp512 260, tg32 18.5–18.8 t/s – unchanged: the GT is held back by the **package power limit** in 50–75 % of the looks (`PERF_LIMIT_REASONS` bits 8/10/11, logged per queue), i.e. the VM host's own load takes the budget Linux leaves the GT. Cooperative matrices (XMX via Mesa, 1-line llama.cpp patch) tried on Linux: pp512 75 instead of 282, pp64 17 instead of 108, same greedy output and perplexity – **not enabled**. **Speculative decoding** (25.11.2026, `llama-speculative-simple`, greedy, 192 tokens, Ultar/VFIO): without drafts 17.8 t/s; MTP head of the model (`draft-mtp`) n-max 2 26.6 t/s (81 % accepted), n-max 3 26.2 (77 %); DFlash drafter (z-lab, converted to GGUF Q8_0) n-max 7 24.3 (38 %), n-max 15 10.9 (23 %). Linux on the same machine: i915 MTP n-max 2 21 t/s, xe 27.8 t/s. On Ultar MTP needs `-fa off`: with flash attention a compute job of the MTP context hangs (stops at a PIPE_CONTROL, deterministic, not on Linux xe) – open. Scratch address spaces (Mesa asks for them) are now real (1 GiB null entries). Later the same day: the llama package carries one patch (`vulkan-vec-16.patch`: matrix-vector path up to 16 tokens) – pp10/12/16 79/85/92 instead of 32/38/46 t/s, test-backend-ops MUL_MAT/MUL_MAT_ID 3833/3833; DFlash still trails MTP (n-max 7 25.0, 11 17.7, 15 12.4 t/s; the drafter and the state checkpoints of the hybrid model cost more than the verify), MTP n-max 2 26.8, n-max 4 23.3. **FA hang narrowed further (26.11.2026):** it is not flash attention but the draft's sampling on the GPU (`--spec-draft-backend-sampling`, on by default): with `--no-spec-draft-backend-sampling` MTP runs with `-fa on` on Ultar, 27.5 t/s. The dispatch that never ends (dumped by the kernel since: commands around ACTHD, binding table, surface states) has 512 invocations per group, no shared memory, no barrier, groups 1×512×2, and two buffers of one and two vocabulary rows (993 280 / 1 986 560 bytes) – the draft sampler (TOP_K k=10 over 248 320 logits) – both mapped, no GPU fault recorded; all 654 ARGMAX/TOP_K/ARGSORT/CUMSUM cases of test-backend-ops pass on Ultar. Ruled out: Xe_LPG workarounds (now programmed like xe: 14018575942, 22016670082, 14017856879, 14020495402), TLB invalidation after binds into scratch spaces (now done like xe), sparse FA, stream-K, copy queue, fusion of ops. Root cause still open; until then the sampling of the draft stays on the CPU. Earlier: The FA hang is not a shader of its own: the MTP context runs flash attention with the same shapes as the main model (perf logger), all 174 FA cases with head size 256 pass on Ultar; sparse FA off, stream-K off and no copy queue do not help. Open: the 8 % on pp512 (to be measured bare metal); llama.cpp built for SSE4.2 (the kernel does not save AVX registers yet); rendering untested |

1. **Hardware run** (`tools/kern-test/igpu-run.sh` with `STOP_UNITS="udl-llm-fast.service"`, log on the Beelink:
   `/root/ultar-test/gt-run1.log`, checkout `/root/ultar-igpu-npu`): `INTELTEST: ALLES OK` and `GTTEST: ALLES OK`;
   the whole run takes about 20 s, the host's i915 loads its own GuC again afterwards (function level reset).
   Five runs on 24.11.2026 (logs `gt-run1.log` … `gt-run5.log`, loop script `/root/ultar-test/gt-loop.sh <log>`);
   runs 7–12 on 25.11.2026 with the command buffers and the full ADS (run 9 failed once, see 2b), runs 13–17 with jobs on the engines, runs 18–25 with the render node and programs; under VFIO only (no bare-metal start). The WOPCM write path (registers not yet locked) has not run
   on hardware: i915 had locked them, the driver kept that layout.
2. **Engines run test jobs, nothing more yet.** With the full ADS the GuC schedules contexts on the copy, render
   and compute engine (G3). The jobs are single store commands; there is no interface for programs (G4) and no
   3D or compute state. GTTEST puts the GuC back into reset.
2a. **CPU caches and memory the GT writes** (found on hardware): the table the GuC copied read as zeroes in its
   first five cache lines until the kernel dropped those lines (`clflush`) before reading. The GGTT's "coherent"
   cache attribute (index 3, what Linux uses) does not change that – tried on 25.11.2026. Buffers the GT writes are
   read after dropping the lines; buffers the CPU writes are flushed before the GT reads them.
2b. **GGTT read-back fails now and then.** In 2 of 17 runs one GGTT entry did not read back as written while the
   GuC was running (runs 9 and 16); both times every entry was right on a second look, so the read was wrong, not
   the table. The kernel now maps a second time (write, read back, flush) before it gives up, and logs a note.
   Why the read fails is not understood.
2c. **Golden contexts.** The image each engine saved after its first batch is copied into the ADS (words set in
   register and engine state: copy 119, render 740, compute 157) and new contexts start from it. Linux primes that
   first context with its workaround register list and the render engine's default state commands before it
   records the image; that priming is not written yet, so the images are the hardware's plain state. It has to be
   added before Mesa renders (G5). What the GuC does with the images at an engine reset has not been exercised.
2e. **Engine reset needs more than the status page** (found on hardware, run 23): the register list in the ADS
   only named status page and interrupt mask. After the GuC reset the compute engine for a hung batch, the engine
   ran nothing any more and the GuC never confirmed the disable. With interrupt handling (`RING_HWSTAM`), context
   mode (`GFX_MODE`, masked) and – with the render engine – the compute switch (`RCU_MODE`, masked) in the list,
   the engine works again after the reset (runs 24, 25). Linux additionally lists its workaround registers.
2f. **Who holds the GT awake** (found on hardware, run 20): GuC and clock registers read 0 while the GT forcewake
   domain sleeps. Two `Forcewake` objects do not share their count – the GT in operation is the one holder.
2d. **Stale translations** (found on hardware, run 16): after a job's buffers were given back and their GGTT
   addresses reused, the next context ran with the old context's ring state. Dropping the cached translations
   through the GuC (`TLB_INVALIDATION` `0x7000`, scope GuC and scope full, each confirmed by event `0x7001`) after
   GGTT changes fixes it (run 17).
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
| Pure logic | `drivers/kernel/intel-gt/` (crate `ultar-intel-gt`, `no_std` + `alloc`, no `unsafe`, 59 host tests) | `ppgtt` (page tables), `ctb` (command buffers), `ads` (minimal and full block), (minimal ADS block), `params` (boot parameter words per stage), `hxg` (register message channel), `hwconfig` (table parser), `ident` (GMD_ID → IP), `forcewake` (domains, references, bounded acknowledge wait), `engines` (descriptor + fuses), `fw` (CSS header, versions, RSA key position), `wopcm` (layout, locked registers, write-and-verify), `guc` (status decoding, reset, parameter words, RSA, DMA transfer, wait), `state` (read-out). Generic over `ultar_intel_display::Hw`, tested against `sim.rs`. |
| Kernel module | `kernel/ultar-kern/src/drivers/intel_display/gt.rs`, `gt_mem.rs` (GGTT buffers, word view for the command buffers) | probe read-out (`[intel-gt] …`, `intel-gt=` line of the display report), GGTT buffers for the GuC (`GgttBuf`: kernel pages → the display driver's GGTT window, flushed), GTTEST. Two `unsafe` (copy into and out of owned pages) with `// SAFETY:`. |
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
* **Full ADS:** `[fixed part | pad | register lists 4K | golden contexts (render 56K, copy 8K, compute 56K) | capture
  list 4K | private data]`. Enabled masks and mapping for GuC classes 0 (render), 3 (copy), 4 (compute), instance 0;
  per engine the registers `base+0x80` (status page) and `base+0xa8` (interrupt mask); per class the golden context
  address and the engine state size (image − status page − 96 register words); all capture lists point at one
  empty list. Boot parameters of this load add SLPC, lite restore and the two workaround flags.
* **Page tables:** four levels of 512 entries, entry = page address | present | writable | cache attribute 3
  (bits 3 and 4).
* **Context image:** `[status page 4K | register state 4K | engine state (13 pages for render and compute)]`.
  The register state is the list of register loads the hardware replays (`lrc.rs`: copy/compute list and the
  longer render list), ended by `MI_BATCH_BUFFER_END`; context control = "no restore" for a context that never ran;
  ring start/head/tail/control and, with page tables, their root in PDP0. Descriptor for the GuC = GGTT address of
  the image | valid | 64-bit addressing | (page tables: bit 8).
* **Engine before its first context:** `RING_HWSTAM` = all ones, `GFX_MODE` legacy mode off (masked), status page
  address into `RING_HWS_PGA`; compute additionally `RCU_MODE` enable. Render forcewake held.
* **A job:** requests without response over the command buffers (HXG type 2): `REGISTER_CONTEXT` (`0x4502`, 12
  words: flags, id, class, engine mask, no work queue, descriptor), `UPDATE_CONTEXT_POLICIES` (`0x100b`: priority,
  1 ms slice, 640 ms preemption timeout, frequency request), `SCHED_CONTEXT_MODE_SET` (`0x1001`) enable – this also
  runs what waits in the ring; the GuC confirms with event `0x1002`. On the Beelink the value is in memory by the
  time that event has been read (about 0.1 ms). Retire: mode set disable (event `0x1002`), `DEREGISTER_CONTEXT`
  (`0x4503`, event `0x4600`). Further jobs of an enabled context: commands behind the ring's tail, new tail into
  the context image, `SCHED_CONTEXT` (`0x1000`); on the Beelink the second value is there after about 50 µs.
* **Golden image and new contexts:** `ads::golden_slot` says where a class's image lies in the ADS; a new context
  takes register and engine state from it and sets its own ring, page table root and run time (`lrc::image_from`).
* **Ring and batch:** GGTT job = `MI_ARB_ON_OFF` on, `MI_STORE_DATA_IMM` (GGTT), arbitration check; page table job
  = `MI_BATCH_BUFFER_START` (bit 8: context address space) to a batch holding `MI_STORE_DATA_IMM` and
  `MI_BATCH_BUFFER_END`. The ring tail is a multiple of 8 bytes.
* **Command buffers:** one GGTT buffer `[H2G descriptor | G2H descriptor (2K) | H2G ring 16K (4K) | G2H ring 128K
  (20K)]`. Registration over the register channel: six `SELF_CFG` (`0x0508`) keys `0x0902`–`0x0907` (each must be
  answered with "1 key taken"), then `CONTROL_CTB` (`0x4509`) = 1. Message = header (fence 31:16, length 7:0) +
  HXG words. H2G: check status and head, a message never wraps (rest of the ring is zero-filled, tail to 0), write,
  move the descriptor's tail, notify. G2H: read tail, take one message, move head; response/failure/retry/busy carry
  the request's fence, events carry an action. The kernel's word view writes through to memory and reads from it.
* **GGTT range of the GuC:** addresses below the WOPCM size and from `0xFEE00000` are not reachable for it; every
  buffer is checked before any register is written.

## 4. Sources

Linux `xe` driver (MIT): `regs/xe_gt_regs.h`, `regs/xe_guc_regs.h`, `xe_force_wake.c`, `xe_pci.c`, `xe_hw_engine.c`,
`abi/uc_fw_abi.h`, `xe_uc_fw.c`, `xe_wopcm.c`, `xe_guc.c`, `abi/guc_errors_abi.h` (read at commit `8d7fe98`).
`xe_guc_ads.c`, `xe_guc_fwif.h`, `xe_guc_hwconfig.c`, `abi/guc_messages_abi.h`.
`xe_guc_ct.c`, `abi/guc_communication_ctb_abi.h`, `abi/guc_actions_abi.h`, `abi/guc_klvs_abi.h`.
`xe_lrc.c`, `regs/xe_lrc_layout.h`, `regs/xe_engine_regs.h`, `instructions/xe_mi_commands.h`, `xe_ring_ops.c`,
`xe_guc_submit.c`, `abi/guc_scheduler_abi.h` (G3). No GPL code. Also read: `xe_ggtt.c`, `xe_pt.c`;
for G4: `include/uapi/drm/xe_drm.h`; register and command layouts: Mesa `src/intel/genxml/` (MIT).
