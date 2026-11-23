# ultar-cloud – store, update and account messages. Reference: ar.ftl.

# Store
cloud.store.sign_in_required = Sign in to get this app.
cloud.store.not_included = This app is not included in your access.
cloud.store.not_found = This app is not in the store.
cloud.store.state.free = Free
cloud.store.state.premium_sign_in = Premium – sign-in required
cloud.store.state.premium_not_included = Premium – not included in your access
cloud.store.state.unlocked = Unlocked for you
cloud.store.publisher.verified = Verified publisher
cloud.store.empty = No matching apps.
cloud.store.installed = Installed {name} version {version}.

# System updates
cloud.update.sign_in_required = Sign in to use this update channel.
cloud.update.not_included = This update channel is not included in your access.
cloud.update.not_found = This update channel does not exist.
cloud.update.available = An update is available: version {version} (channel {channel}).
cloud.update.none = The system is up to date.

# General errors
cloud.error.auth_required = You need to sign in for this.
cloud.error.not_entitled = Your account does not have access to this.
cloud.error.not_found = The requested item does not exist.
cloud.error.rate_limited = {seconds ->
    [one] Too many requests. Try again in {seconds} second.
   *[other] Too many requests. Try again in {seconds} seconds.
}
cloud.error.invalid_request = The server rejected the request as invalid.
cloud.error.invalid_credentials = Wrong username or password.
cloud.error.username_taken = This username is already taken.
cloud.error.server = The server reported an error. Try again later.
cloud.error.offline = The server could not be reached. Check your internet connection.
cloud.error.invalid_response = The server's answer could not be understood.
cloud.error.io = Writing to storage failed.
cloud.error.integrity = The downloaded file is damaged or incomplete. Try again.
cloud.error.package_rejected = The package was rejected: its signature is missing or not trusted. Nothing was installed.
cloud.error.not_signed_in = You are not signed in on this device.
cloud.error.vpn = The VPN keys could not be prepared.

# Device identity
cloud.device.challenge_rejected = The server did not accept the security check. Try again.
cloud.device.signature_rejected = The server did not accept this device's proof of identity.
cloud.device.taken = This device is already registered to another account.
cloud.device.retired = This device was removed from the account. Reset its device identity to register it again; it will get a new number.
cloud.device.limit = Your account has reached its number of devices. Remove a device first.
cloud.device.unknown = The server does not know this device.
cloud.device.update_required = Update the system on this device before you register it.
cloud.device.legacy_not_eligible = The earlier registration could not be taken over by this device.
cloud.device.cooldown_active = {seconds ->
    [one] The number can be moved in {seconds} second.
   *[other] The number can be moved in {seconds} seconds.
}
cloud.device.recovery_pending = A device replacement is already waiting for this account.
cloud.device.recovery_unknown = There is no device replacement waiting.
cloud.device.server_mismatch = The security check named another server or account. Nothing was signed.
cloud.device.key_unusable = The device key cannot be used. It was not changed.
cloud.device.not_registered = This device is not registered yet.
cloud.device.not_approved = {seconds ->
    [one] This device was added recently. It can manage your other devices in {seconds} second, or at once when one of them approves it.
   *[other] This device was added recently. It can manage your other devices in {seconds} seconds, or at once when one of them approves it.
}
cloud.device.recovery_locked = {seconds ->
    [one] This earlier registration cannot be replaced yet. Update that device, or try again in {seconds} second.
   *[other] This earlier registration cannot be replaced yet. Update that device, or try again in {seconds} seconds.
}
cloud.device.approved = The device was approved.
cloud.device.state.waiting = added recently – cannot manage other devices yet

# Security alerts
cloud.alert.title = Security alert
cloud.alert.recovery_pending = {hours ->
    [0] Someone with your password asked to move the number of device {device} to another device. It can be moved now.
    [one] Someone with your password asked to move the number of device {device} to another device. It can be moved in {hours} hour.
   *[other] Someone with your password asked to move the number of device {device} to another device. It can be moved in {hours} hours.
}
cloud.alert.not_me = This was not me – stop it
cloud.alert.recovery_cancelled = The move of the number was stopped. Change your password now.
cloud.alert.cannot_cancel = This device was added recently and cannot stop it. Use one of your other devices, or change your password.
cloud.alert.none = Nothing to report.
cloud.alert.event.device_added = A device was added to your account: {device}
cloud.alert.event.device_removed = A device was removed from your account: {device}
cloud.alert.event.device_replaced = A number was moved to a new device: {device}
cloud.alert.event.device_upgraded = An earlier registration was taken over by device {device}
cloud.alert.event.device_approved = A device was approved to manage your devices: {device}
cloud.alert.event.recovery_started = A request to move the number of device {device} was made with your password.
cloud.alert.event.recovery_cancelled = A request to move a number was stopped.
cloud.alert.event.recovery_finished = A number was moved to device {device} after the waiting time.
cloud.alert.event.password_changed = The password of your account was changed.
cloud.device.state.active = registered
cloud.device.state.legacy = registered before device keys – update this device
cloud.device.this_device = this device
cloud.device.none = No devices are registered.
cloud.device.removed = The device was removed.
cloud.device.renamed = The device was renamed.
cloud.device.reset = The device identity was deleted. Register again to get a new number.

# Command line
cloud.cli.signed_in = Signed in.
cloud.cli.number = Ultar number: {number}
cloud.cli.vpn_ready = The VPN is set up.
cloud.cli.server = Server: {server}
cloud.cli.account.signed_in = Account: signed in
cloud.cli.account.signed_out = Account: signed out (store and updates work without an account)
cloud.cli.unknown_command = Unknown command: {command}
cloud.cli.usage = Usage: ucloud register|login <user> [--password-file <file>] (password on standard input) · number · devices · device-rename <id> <name> · device-remove <id> · device-approve <id> · device-reset · alerts · not-me · vpn · store [search] · app <id> · install <id> · update [channel] · whoami
