# ultar-update – English

update.state.idle = The system is up to date
update.state.checking = Checking for updates…
update.state.available = A system update is available
update.state.downloading = Downloading the update…
update.state.paused = The download is paused and can be continued
update.state.staged = The update is ready to install
update.state.applying = Installing the update…
update.state.reboot_required = Restart the device to finish the update
update.state.verifying = Checking the new version after the restart…

update.error.io = Could not read from or write to the storage
update.error.malformed = The update file is damaged or is not an Ultar system update
update.error.no_trusted_keys = This device has no trusted keys to verify updates
update.error.unknown_key = The update is signed with a key this device does not trust
update.error.bad_signature = The signature of the update is wrong; nothing was installed
update.error.expired = This update has expired; check for a newer one
update.error.wrong_arch = This update is built for another processor type
update.error.wrong_edition = This update is built for another edition of the system
update.error.wrong_device = This update is not meant for this device
update.error.wrong_channel = This update belongs to another channel than the one selected
update.error.payload_set = The content of the update does not fit the layout of this device
update.error.rollback_refused = The update was refused because it is older than the device's security level
update.error.needs_intermediate = An intermediate update must be installed first
update.error.up_to_date = This version is already installed
update.error.downgrade_refused = This version is older than the running one
update.error.hash_mismatch = The downloaded data does not match the signed update; it was removed and will be downloaded again
update.error.read_back = The storage did not keep the data as written; the current system is unchanged
update.error.size_exceeded = The update file is larger than its signed size; it was refused
update.error.truncated = The download stopped before it was complete and can be continued
update.error.rotation_refused = The new update keys could not be accepted
update.error.state_corrupt = The protection data stored on the device is damaged; updates are stopped to stay safe
update.error.no_space = Not enough free space to download the update
update.error.not_staged = No downloaded update is ready to install
update.error.busy = An update operation is already running
update.error.cancelled = The operation was cancelled
update.error.interrupted = The operation was interrupted
update.error.no_bootable_slot = There is no system version that can be started
update.error.boot_control = The boot data could not be read
update.error.previous_unavailable = There is no previous version that can be started
update.error.no_update = There is no update for this device
update.error.metered = The current connection is metered; allow downloading over it or use another network
update.error.permission_denied = This action needs administrator rights
update.error.auth_required = This channel needs you to sign in
update.error.not_entitled = Your account has no access to this channel
update.error.unknown_channel = The update channel does not exist on the server
update.error.rate_limited = The server is busy; try again later
update.error.network = Could not reach the update server
update.error.unhealthy = The new version did not work properly; restart to return to the previous version
update.error.invalid_argument = Invalid value
update.error.boot_failed = The new version did not start; the device returned to the previous version by itself
update.error.tls_unavailable = This device's processor cannot make the encrypted connection to the update server; use an update from a USB stick

update.history.staged = Update downloaded and verified
update.history.applied = Update installed, waiting for restart
update.history.installed = System update completed
update.history.rolled_back = The system returned to the previous version
update.history.failed = The update operation failed
update.history.previous_selected = Previous version chosen for the next start
update.history.cancelled = Update cancelled
update.history.keys_rotated = Update verification keys replaced

update.notes.generic = Improvements and fixes for the system

update.cli.usage = Usage: uupdate [--socket PATH] [--lang ar|en|de] [--no-wait] COMMAND
        Commands: status · check · download [--metered] · apply · cancel · history [N]
        policy · policy set KEY=VALUE… · offline PATH [--allow-downgrade] · previous
        verify FILE.uup --keys DIR [--state DIR]
update.cli.current = Running version: {version} (slot {slot})
update.cli.channel = Channel: {channel}
update.cli.pending = Pending version: {version}
update.cli.previous = Previous version: {version} (can be started)
update.cli.previous_none = No previous version can be started
update.cli.floor = Security level: {index}
update.cli.critical = Important security update
update.cli.progress = {percent}%
update.cli.accepted = Request accepted
update.cli.no_bus = Could not reach the update service
update.cli.timeout = The update service did not answer in time
update.cli.verify_ok = The file is intact and signed: version {version}
update.cli.history_empty = The history is empty
update.cli.history_line = {time} · {what} · {version}
update.cli.policy.automatic = Automatic updates: {value}
update.cli.policy.window = Time window (UTC): {start}–{end}
update.cli.policy.window_any = Time window: any time
update.cli.policy.metered = Download over metered connections: {value}
update.cli.policy.rate = Speed limit: {rate} KiB/s
update.cli.policy.rate_unlimited = Speed limit: none
update.cli.yes = yes
update.cli.no = no

update.daemon.usage = Usage: ultar-updated --config FILE [--socket PATH] [--log-socket PATH] [--lang ar|en|de]
update.daemon.config = The configuration file is invalid (line {line}): {detail}
update.daemon.bus = Could not connect to the system bus
update.daemon.ready = Update service ready
update.daemon.hardening_failed = The update service could not restrict itself at start: {detail}

update.sign.usage = Usage: uupdate-sign COMMAND [options]
        keygen --secret FILE --public FILE
        manifest --out FILE --version X.Y.Z --rollback-index N --channel NAME --edition NAME --arch NAME
                 --devices LIST --min-version X.Y.Z --notes-key KEY --created SECONDS --expires SECONDS [--critical]
                 --payload ROLE=FILE[:VERITY-ROOT]…
        sign --manifest FILE --key SECRET-FILE --out FILE
        rotate --serial N --created SECONDS --key SECRET-FILE --out FILE --public FILE…
        bundle --signed FILE --out FILE.uup [--rotation FILE]… --payload ROLE=FILE…
        verify --bundle FILE.uup --keys DIR
update.sign.wrote = Wrote {path}
update.sign.verified = Signature valid: version {version}, rollback index {index}
update.sign.key_id = Key id: {id}
update.sign.failed = Failed: {detail}
