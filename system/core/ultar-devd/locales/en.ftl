# ultar-devd / udevctl – English.
# Positional arguments of bus texts are named {a}, {b} in order.

devd.phase.detected = detected
devd.phase.probing = probing
devd.phase.active = working
devd.phase.no-driver = no driver
devd.phase.failed = not working
devd.phase.removed = removed

devd.reason.no-package = The depot has no driver that fits this device.
devd.reason.kernel-only = The kernel runs this device with its driver "{a}", but the depot has no driver package for it.
devd.reason.not-loadable = The driver package needs a loadable kernel module, which this kernel does not support yet.
devd.reason.not-bound = The kernel did not bind its driver "{a}" to this device at boot, and it cannot be activated while running.
devd.reason.package-missing = The driver package file is not on this system.
devd.reason.rejected-detail = The driver package was rejected: {a}
devd.reason.probe-failed = The kernel driver failed to set up the device: {a}
devd.reason.load.package-rejected = The driver package was rejected because its signature or content is not intact.
devd.reason.load.incompatible-kernel = The driver does not fit the running kernel.
devd.reason.load.init-failed = The driver failed to set up the device.
devd.reason.load.timeout = Loading the driver timed out.
devd.reason.load.unsupported = This driver cannot be used with the running kernel.
devd.reason.health.no-response = The device does not respond with this driver.
devd.reason.health.self-test-failed = The device failed its self test with this driver.
devd.reason.health.timeout = Checking the device timed out.
devd.reason.health.unstable = The device works only intermittently with this driver.
devd.reason.unknown = No driver works for this device.
devd.detail.wrong-package = the file holds another driver or version than the index lists
devd.detail.bad-activation-record = the activation record inside the package is not valid

devd.error.unknown-device = There is no device at address {a}.
devd.error.not-a-candidate = Driver {b} does not fit device {a} or is not in the depot.
devd.error.quarantined = Driver {b} is quarantined for device {a}. Clear the quarantine first.
devd.error.already-active = Driver {a} is already in use.
devd.error.nothing-to-roll-back = Device {a} has no earlier driver to return to.
devd.error.wrong-phase = Not possible while device {a} is in the state: {b}.
devd.error.unsupported = Device {a} cannot be switched to driver {b}: the running kernel offers no way to activate or deactivate its built-in drivers while running.
devd.error.transition = Internal error in the device service.
devd.error.bad-request = The request is not valid.

devd.notice.keys-missing = There are no trusted keys, so no driver can be verified.
devd.notice.keys-unreadable = The key file {a} could not be read and was ignored.
devd.notice.depot-unreadable = The driver depot cannot be used: {a}
devd.notice.hardware-unreadable = The device list could not be read: {a}
devd.notice.state-recovered = The state file was damaged ({a}); the service started with an empty state and kept the damaged file.
devd.notice.state-unavailable = The choices could not be saved ({a}); they stay in effect until shutdown.
devd.notice.publish-failed = The device state could not be published on the system bus; other services may not know the latest changes.

devd.store.problem.header = file header missing or invalid
devd.store.problem.length = file is cut short
devd.store.problem.checksum = checksum does not match
devd.store.problem.body = content is not valid
devd.store.problem.version = file comes from a newer version
devd.store.problem.too-large = file is larger than allowed
devd.store.unavailable = storage location not available
devd.store.corrupt = stored content not understood
devd.store.write-failed = writing failed

hal.hardware.no-source = neither the kernel nor the system provides a device description
hal.hardware.io = the device description could not be read
hal.hardware.too-many-devices = more devices than plausible
hal.hardware.note.log-wrapped = The kernel log has wrapped; some devices may not be shown.
hal.hardware.note.bound-only = The kernel only shows devices that have a built-in driver; other devices are not visible.
hal.hardware.note.class-incomplete = The class of some devices is not known exactly; drivers matched by class may not be found.
hal.hardware.note.pci-count = The number of devices the kernel announced differs from the number described.

devd.capability.no-driver = The hardware is present but no driver works for it.

devd.start.unknown-kernel = The running kernel could not be determined. Use --kernel-abi.
devd.start.hardware = The system description could not be read: {a}

devd.cli.usage = Usage: udevctl [--bus SOCKET] [--lang ar|en|de] list | switch <address> <driver> <version> | rollback <address> | rescan [address] | clear-quarantine [address [driver version]] | shutdown
devd.cli.daemon-usage = Usage: ultar-devd --depot <dir> --keys <dir> --state <dir> --bus <socket> (--policy <file> | --allow-all) [--token-file <file> | --token-fd <n>] [--log-socket <socket>] [--allow-root] [--root <dir>] [--kernel-abi <name>] [--once] [--lang ar|en|de]
devd.cli.bus-failed = Could not reach the device service: {a}
devd.cli.no-devices = No devices.
devd.cli.device = {address}  {id}  [{phase}]
devd.cli.label = label: {a}
devd.cli.driver = driver: {a} {b}
devd.cli.alternative = alternative: {a} {b}
devd.cli.alternative-quarantined = alternative in quarantine: {a} {b} ({c})
devd.cli.previous = previous: {a}
devd.cli.reason = reason: {a}
devd.cli.notice = notice: {a}
devd.cli.kernel-switch-unsupported = Note: the running kernel cannot switch its built-in drivers while running.
devd.cli.done = Done.
devd.cli.restored = The requested driver failed; the previous driver was restored.
devd.cli.refused = Refused: {a}
devd.cli.released = {count ->
    [zero] No quarantine entry to release.
    [one] Released one quarantine entry.
   *[other] Released {count} quarantine entries.
}
devd.cli.added = appeared: {a}
devd.cli.removed = disappeared: {a}
devd.cli.shutdown = The device service was stopped.

devd.daemon.started = Device service running: {devices} devices, {active} with a working driver.
devd.daemon.stopped = The device service was stopped.
devd.daemon.bus-lost = Lost the connection to the system bus: {a}
devd.daemon.policy-required = A policy file (--policy) or --allow-all is required.
devd.daemon.policy-conflict = --policy and --allow-all cannot be combined.
devd.daemon.policy-rejected = The policy file is not valid: {a}
devd.daemon.token-rejected = The token could not be read: {a}
devd.daemon.root-refused = The device service does not run as root without --allow-root.
devd.daemon.hardening-failed = The self-limits could not be applied: {a}
