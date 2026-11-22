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

# Command line
cloud.cli.signed_in = Signed in.
cloud.cli.number = Ultar number: {number}
cloud.cli.vpn_ready = The VPN is set up.
cloud.cli.server = Server: {server}
cloud.cli.account.signed_in = Account: signed in
cloud.cli.account.signed_out = Account: signed out (store and updates work without an account)
cloud.cli.unknown_command = Unknown command: {command}
cloud.cli.usage = Usage: ucloud register|login <user> <password> · number · vpn · store [search] · app <id> · install <id> · update [channel] · whoami
