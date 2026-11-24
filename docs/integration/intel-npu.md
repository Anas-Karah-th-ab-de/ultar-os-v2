# Intel NPU driver – "AI Boost" (Ultar kernel)

Area: drivers (kernel part). Target: **T2** Beelink GTi15 Ultra, NPU 3720 `8086:7d1d` (Meteor Lake / Arrow Lake-H;
`8086:ad1d` Arrow Lake-S is the same generation). Goal: inference on the NPU for local models, through the Ultar
kernel, without Linux.

## 0. State and open points

| Stage | What | State |
|---|---|---|
| N0 | Identification, buttress read-out (clock fuses, state), work point request, power-up to a running IP block, hardware version, power-down, firmware container checks | **done, run on the Beelink** (NPU through VFIO, 24.11.2026): fuses 333–1600 MHz, nominal 700 MHz; power-up in 241–333 µs, hardware version `0x3630`, clock locked and ready, power-down; `NPUTEST: ALLES OK`, read-only boot `READ-ONLY OK`, `intel_vpu` rebound with its firmware |
| N1 | The NPU's MMU (stream table, context descriptor, page tables), firmware image and boot parameters in NPU memory, start of the NPU's processor, IPC channel, first message (firmware version) | open – next |
| N2 | Job interface (JSM): contexts, command queues, doorbells, heartbeat, recovery after a hang | open |
| N3 | User interface: the Linux `ivpu` uAPI on `/dev/accel/accel0` (parameters, buffer create/info, submit, wait) | open |
| N4 | Intel's user-space NPU driver (Level Zero, MIT) and OpenVINO unchanged; NPU backend in `ultar-llm` for small models | open |

1. **Hardware run** (`tools/kern-test/npu-passthrough.sh <ultar-kern.efi>` on the Beelink: NPU → vfio-pci → Ultar
   kernel under KVM, always given back to `intel_vpu`; logs `/root/ultar-test/npu-run1.log`, `gt-run1.log`): two
   runs with opt-in, one without (`OPTIN=0` → `READ-ONLY OK`). The first run showed that the IP block reports
   `0x36`/`0x30` in the generation/SoC fields of its hardware version (the two-tile SKU `0x3630`), not the IP
   generation 37 – the self test compares with that now. Under VFIO only (no bare-metal start).
2. **No inference yet.** After N0 the NPU can be powered and its registers read; its processor does not run.
3. **No public specification.** Register offsets and the order of steps come from reading the Linux `ivpu` driver
   (GPL) as a description; the code is written for the Ultar kernel and copies none of it.
4. **Firmware.** `vpu_37xx_v1.bin` (linux-firmware `intel/vpu/`, redistributable) – only its header is parsed so
   far; a signed `.utr` package `intel-npu-fw` with the licence text comes with N1.
5. **No driver package yet.** The driver is built into the kernel and binds only with the opt-in; a manifest under
   `drivers/manifests/` follows with N1, so the package plan still lists the NPU as driverless.
6. **Interrupts.** The kernel has no MSI routing; IPC and job completion will be polled until it has.
7. **Opt-in.** Without `ultar.intel_npu=on` (boot option) or `\ultar\intel-npu` (`on`) the probe only reads the
   buttress and reports `not driven, read only (…)`. With it the probe enables the device and reads; the power
   cycle runs only in the self test.

What the NPU brings for local models (to be measured in N4, no numbers promised here): low power draw for small
and medium models with fixed shapes through OpenVINO; large models belong on the GPU or the processor.

## 1. Layout

| Part | Where | What |
|---|---|---|
| Pure logic | `drivers/kernel/intel-npu/` (crate `ultar-intel-npu`, `no_std` + `alloc`, no `unsafe`) | `hw` (two register blocks, bounded polls), `regs`, `ident` (ids, generation, address ranges), `optin`, `pll` (fuses, work point), `power` (D0i3, up, IP reset, down, hardware version), `fw` (header checks), `state` (read-only facts). Tested against `sim.rs`. |
| Kernel module | `kernel/ultar-kern/src/drivers/intel_npu.rs` | PCI driver `intel-npu`, MMIO over BAR0 (IP) and BAR4 (buttress) with bounds checks, opt-in, `intel-npu=` report line, NPUTEST. Two `unsafe` (volatile load/store) with `// SAFETY:`. |
| Test on the machine | `tools/kern-test/npu-passthrough.sh` | NPU → vfio-pci → Ultar kernel under KVM, verdict, always given back. |

## 2. Sequences (as implemented, NPU 3720)

* **Work point** (clock): wait until the request channel is free → min/max ratio, target ratio + preference `0x80`,
  configuration (`0x0202`: two tiles, divider 4/3; `0` for off) → send → wait for the channel, the PLL lock and the
  ready bit (1.5 s each). Ratio × 50 MHz × 2/3 = frequency.
* **Power-up:** work point 0 (clears a stale request; failure only noted) → leave D0i3 (failure only noted) → work
  point on → IP block answers (always-on reset bit reads 0, 100 µs) → host subsystem resets cleared, interconnect
  quiescent (request/accept/deny all 0) → idle generator off → power island of the processor: trickle, ≥ 500 ns,
  main switch, status within 5 ms → clocks on, isolation off, resets released, "active" set → host interconnect and
  top interconnect requested and accepted, not denied.
* **Power-down:** IP reset through the buttress, then work point 0; both steps run, the first error is reported.
* **Polls** never accept a register that reads all ones (an unpowered block reads so); at the time limit that
  counts as "device gone".

## 3. Sources

Linux `drivers/accel/ivpu` (GPL – description only): `ivpu_hw.c`, `ivpu_hw_btrs.c`, `ivpu_hw_btrs_reg.h`,
`ivpu_hw_ip.c`, `ivpu_hw_ip_reg.h`, `ivpu_fw.c`, `vpu_boot_api.h` (read at commit `8d7fe98`). For N1–N3:
`ivpu_mmu.c`, `ivpu_mmu_context.c`, `ivpu_ipc.c`, `ivpu_jsm_msg.c`, `ivpu_job.c`, `include/uapi/drm/ivpu_accel.h`.
