# Ultar kernel – device interfaces for ultar-devd

Patch: `CHANGES.patch` (`patch -p1` against repo commit a5eb384). Task: the six kernel interfaces listed in
`ultar-devd-daemon.md` §2 ("Kernel interfaces that are missing"), Linux-compatible where Linux has one, and ultar-devd
wired to them instead of parsing the kernel log.

## What changed

| File | Change |
|---|---|
| `kernel/ultar-kern/src/devices/` (new: `mod`, `sysfs`, `events`, `usb`, `platform`) | device model for user space: control (bind, unbind, enable), sysfs tree, uevents, USB registry, platform devices |
| `src/drivers/gen_xhci/hotplug.rs` (new), `src/drivers/gen_xhci.rs` | USB hot-plug at root ports; every attached device registered (`ports` field, one call in `attach`) |
| `src/driver.rs` | `Driver::can_remove`/`remove` (teardown, default: none), `NetDev::link`; `Bound` gets `tried`, `disabled`, `slots` |
| `src/drivers/gen_intel_eth.rs`, `virtio_net.rs`, `virtio.rs` | teardown paths (e1000: RX/TX off, interrupts masked, device reset; DMA buffers freed after the reset; virtio-net: device reset), e1000 `link` |
| `src/pipe.rs` | packet mode (one message per read) for event sockets |
| `src/syscall.rs`, `src/fs.rs`, `src/groups/files.rs`, `src/isolation/path.rs`, `src/socket.rs`, `src/main.rs` | small hooks: route `/sys/bus`, `/sys/kernel`; control writes; `socket(AF_NETLINK, …)`; `bind` on an event socket; `driver` links; hot-plug poll |
| `src/selftest.rs`, `user/geraetetest.c` (new), `user/Makefile`, `scripts/build-kern.sh` | self test `GERAETETEST: ALLES OK` (50 checks with an e1000) and `--hotplug` mode (`GERAETETEST-USB: ALLES OK`) |
| `tools/kern-test/geraete-hotplug.py`, `devd-qemu.sh` (new), `ci.sh`, `target-stage1.sh` | USB hot-plug through the QEMU monitor; ultar-devd end to end; gate and stage-1 verdicts |
| `system/hal/ultar-hal-system` | reads only the sysfs tree; kernel-log parser, class-name table and `InventoryNote` removed; bind/unbind/health/candidates/ABI/seqnum |
| `system/core/ultar-devd` | loader binds through sysfs, health uses the kernel's result, rescan on kernel events; tests and locales follow |
| `tools/treiber-werkstatt/werkstatt.py` | the generator's brief asks for the teardown path |

No new `unsafe` without `SAFETY` comment, no new string literal with umlauts in kernel Rust, no `unwrap`.

## 1. Device list

| Path | Content |
|---|---|
| `/sys/bus/pci/devices/<0000:bb:dd.f>/` | `vendor`, `device`, `class` (`0x020000`), `subsystem_vendor`, `subsystem_device`, `revision`, `uevent` (`DRIVER`, `PCI_CLASS`, `PCI_ID`, `PCI_SUBSYS_ID`, `PCI_SLOT_NAME`, `MODALIAS` as Linux), `enable`, `driver` (link `../../drivers/<name>` when bound); Ultar: `ultar_state` (`bound`, `unbound`, `failed`, `disabled`), `ultar_probe`, `ultar_health`, `ultar_drivers` (every built-in driver whose `matches` takes the device), `ultar_unbind` |
| `/sys/bus/usb/devices/<bus>-<port>[.<port>…]/` | `idVendor`, `idProduct`, `bcdDevice`, `bDeviceClass`/`SubClass`/`Protocol`, `busnum`, `devnum`, `uevent` (`DEVTYPE=usb_device`, `PRODUCT`, `TYPE`, `BUSNUM`, `DEVNUM`) |
| `/sys/bus/usb/devices/<dev>:1.<n>/` | `bInterfaceNumber`/`Class`/`SubClass`/`Protocol`, `uevent`, `driver` (`hub`, `usbhid`, `usb-storage`) |
| `/sys/bus/platform/devices/` | `efi-framebuffer.0` (if the firmware framebuffer is used), `i8042` (if the controller answers), `serial8250` (COM1); `uevent`, `modalias`, `driver`, `ultar_state` |
| `/sys/kernel/ultar/devices` | one line per device: `bus name vendor:device class subsystem driver state probe` (`-` for none) |

Bus number = xHCI controller index + 1; device number = the controller's slot. Names and file formats are those of
Linux, so the HAL's Linux reader serves both kernels. Deviations: the device directories are real directories under
`/sys/bus/*/devices` (Linux: links into `/sys/devices`); `DEVPATH` of events is `/bus/<bus>/devices/<name>`.
**ACPI devices are not listed** (the kernel has no ACPI namespace interpreter); platform devices are the three above.

## 2. Driver register

`/sys/bus/pci/drivers/<name>/`: `bind`, `unbind`, `uevent` (`DRIVER=`), one link per bound device (Linux);
Ultar: `ultar_origin` (`hand`, `werkstatt`), `ultar_enabled` (`1`/`0`), `ultar_unbind` (`supported` or
`unsupported: <reason>`). `/sys/bus/usb/drivers/{hub,usb-storage,usbhid}` and `/sys/bus/platform/drivers/{efi-framebuffer,i8042,serial8250}`
are listed with `ultar_origin = builtin` and `ultar_unbind = unsupported: part of the xhci driver` / `… kernel core`
(no `bind`/`unbind` files). `/sys/kernel/ultar/drivers`: `name bus origin enabled unbind devices` per line.

## 3. Bind, unbind, enable, disable

All writes need the right **`LOAD_DRIVER`** (bit 2, already used for loading drivers; `ultar-service`'s
`Right::LoadDriver`). Without it `open(…, O_WRONLY)` of a control file gives `EACCES`; every other file of the tree
is read-only for everybody (`EACCES`). Reading needs no right. Cells see `/sys` only if it is mounted into them.

| Write | Effect | Errors |
|---|---|---|
| `<dev>` → `/sys/bus/pci/drivers/<d>/bind` | probe `<d>` on the device (the device's earlier network slots are reused, so `NET` indices stay stable) | `ENODEV` unknown device/driver or `<d>` does not match it (Linux); `EBUSY` already bound; `EPERM` device or driver turned off; `EIO` probe failed (`ultar_probe` = `failed: <d> <text>`) |
| `<dev>` → `/sys/bus/pci/drivers/<d>/unbind` | teardown (below) | `ENODEV` not bound to `<d>`; `EOPNOTSUPP` the driver has no teardown path (device untouched); `EBUSY` it serves a block device (the file systems hold it by index); `EIO` teardown reported an error (device stays off, `ultar_probe` = `failed: <d> teardown: …`) |
| `<dev>` → `/sys/bus/pci/drivers_probe` | bind the first turned-on driver that matches (order of `drivers::ALL`) | `ENODEV` none matches |
| `0`/`1` → `/sys/bus/pci/devices/<dev>/enable` | `0`: unbind (as above), turn decoding and bus mastering off, nothing may bind; `1`: allow binding again (no automatic bind) | as unbind; `EINVAL` other values |
| `0`/`1` → `/sys/bus/pci/drivers/<d>/ultar_enabled` | `0`: unbind it from every device, then refuse binds; nothing changes if one device cannot be unbound (`EOPNOTSUPP`); `1`: turn on (no automatic bind) | `EINVAL` |

**Teardown** (unbind): under one lock for all control operations (`devices::CONTROL`), the device's network entries
are replaced by placeholders under the `NET` lock (no CPU is inside the driver afterwards), then `Driver::remove`
quiesces the hardware, then the old driver object is dropped (e1000: rings and buffers go back to the allocator), then
bus mastering, memory and I/O decoding are turned off in the PCI command register. IRQ: the drivers use no interrupts
(polling), so there is none to release. Drivers with a teardown path: **gen-intel-eth** (RX/TX off, interrupts masked,
`CTRL.RST`, wait ≤ 500 ms) and **virtio-net** (device status 0, confirmed). Every other driver answers
`unsupported: no teardown path in this driver` and is never unbound; block drivers would additionally be refused
(`EBUSY`). Boot binding is unchanged (first match in `drivers::ALL`); stage-A activation is now devd's job.

## 4. Driver ABI number

`/sys/kernel/ultar/driver_abi` = `1` (`devices::DRIVER_ABI`: the interface between the kernel and its built-in drivers,
`driver.rs`). The HAL turns it into `ultar-kern-abi-1`; `/proc/version` is no longer used for that.

## 5. Device events

`socket(AF_NETLINK, SOCK_RAW|SOCK_DGRAM, NETLINK_KOBJECT_UEVENT)` (+ `bind` with any `sockaddr_nl`, which is
accepted and ignored) gives a descriptor that delivers every event from then on; `read`/`recv`/`recvmsg`, `poll`,
`epoll` work. One read = one message, Linux uevent format: `ACTION@DEVPATH\0ACTION=…\0DEVPATH=…\0SUBSYSTEM=…\0SEQNUM=n\0`
+ `KEY=VALUE\0` pairs (PCI: `PCI_SLOT_NAME`, `PCI_ID`, `PCI_CLASS`, `ULTAR_STATE`, `DRIVER`; USB: `DEVTYPE`,
`PRODUCT`, `TYPE`, `BUSNUM`, `DEVNUM`). A buffer smaller than the message cuts it (rest dropped, like a datagram).
Actions: `add`/`remove` (USB hot-plug), `bind`/`unbind`, `change` (enable, failed probe).
**Resync:** `SEQNUM` counts every event of the machine from 1 without gaps; each socket queues 64 KiB (a few hundred
events) – when it is full, messages for that socket are dropped and counted (`lost` in `/sys/kernel/ultar/uevents`); a
reader that sees a gap reads the tree again. `/sys/kernel/uevent_seqnum` is the last number; `/sys/kernel/ultar/uevents`
keeps the last 128 events as text lines. Only processes in the machine's own pid namespace may open the socket
(cells: `EACCES`); at most 64 sockets (`ENOBUFS`). Other netlink protocols: `EPROTONOSUPPORT`.

Hot-plug sources: **USB** – the xHCI driver looks at the connect-change bit of every root port at most every 200 ms
(from the kernel's polling path, `socket::poll`, never while a bind/unbind runs); a new device is reset, addressed,
configured and registered (`add`), a leaving one and everything behind it (hubs) loses its slots (`remove`).
**PCI hot-plug is not reported**: the kernel has no PCIe hot-plug or ACPI hot-plug support; the PCI list is the boot scan.

## 6. Probe and health results

`ultar_probe`: `ok`, `none`, or `failed: <driver> <kernel text>`. `ultar_health` (`key=value` lines): `probe`,
`error`, `link=up|down|unknown` per network interface the device has (e1000 reads `STATUS.LU`; virtio-net cannot tell),
`media=ok|failed|missing` per block device (sector 0 is read when the file is read), `bus_master=on|off`.

## 7. ultar-devd and the HAL

- `ultar_hal_system`: inventory and bindings only from sysfs (Ultar and Linux); `KernelDrivers` reads `ultar_*`,
  `bind_device`/`unbind_device` write the control files; `kernel_abi` reads `driver_abi`; `uevent_seqnum`.
  The kernel-log parser is removed: with the device tree it is not needed (no fallback kept – every image is built from
  this repository).
- `ultar-devd`: the stage-A loader binds the package's kernel driver (unbinding the current one only when
  `ultar_unbind = supported`) and confirms from the new state; `unload` unbinds when safe; health also needs the
  kernel's `probe=ok` and no failed medium; `kernel_switch_supported` follows the control files; the serve loop
  rescans when `/sys/kernel/uevent_seqnum` changed (checked every second – the crate has no `unsafe`/libc for a socket).

## Tests

| Test | What |
|---|---|
| `geraetetest` (in `ci.sh` with a second NIC `e1000`, and in the hardware self test) | ABI, list (table = PCI + USB + platform entries), register (origin, unbind support, bind files), event socket, rights (`EACCES` without `LOAD_DRIVER`, read without it, cell refused, wrong protocol), refusals (`EOPNOTSUPP` for virtio-blk and a bridge), full cycle on the e1000: unbind (state, link gone, `bus_master=off`, event), second unbind `ENODEV`, wrong driver `ENODEV`, driver off (`EPERM`, `drivers_probe` `ENODEV`), device off/on (`change` events), bind (event with `DRIVER`, `link=up`), `EBUSY`, unbind + `drivers_probe`, `SEQNUM` gap-free and equal to `uevent_seqnum`, history. Without an e1000 (e.g. stage 1 on real hardware) the cycle is skipped and said so. |
| `geraete-hotplug.py` (in `ci.sh`) | QEMU with `qemu-xhci`: `device_add usb-kbd` → `add` event with `PRODUCT=627/1/…`, device and `usbhid` interface link in sysfs, table line; `device_del` → `remove` event, entry gone, no `SEQNUM` gap |
| `devd-qemu.sh` (in `ci.sh`) | static `ultar-devd --once` on the kernel: e1000 unbound first, signed test package `qemu-e1000` (activation `gen-intel-eth`) → bound through sysfs, listed `[working]`, kernel reports `bound` |
| `ultar-hal-system`, `ultar-devd` unit/integration tests | sysfs fixtures in the kernel's format; loader writes `bind`/`unbind`, refuses switching away from a driver without teardown, health from `ultar_health`, event counter triggers a new reading |

## Open

- PCI hot-plug, ACPI device enumeration: not implemented (see above).
- Teardown exists for two drivers only; xHCI, AHCI, NVMe, virtio-blk, HDA, graphics, SMBus, bridges are refused honestly.
  virtio-net's queue pages are not returned on unbind (a few pages per cycle); e1000's are.
- USB function drivers cannot be bound or unbound individually; a USB disk that leaves keeps its block entry, which
  then fails every access.
- Hot-plug attach runs in the kernel's polling path (CPU 0) and can take about a second there per new device.
- ultar-devd polls the event counter instead of reading the netlink socket.
