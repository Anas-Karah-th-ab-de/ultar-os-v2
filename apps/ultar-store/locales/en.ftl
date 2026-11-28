# Ultar Store – English texts (every key exists in ar.ftl; Arabic is the reference).

app.store.name = Ultar Store
app.store.desc = Discover, install, update and remove apps and components; the catalogue is local and signed and works without internet.

store.window.title = Ultar Store
store.search.placeholder = Search apps and file types
store.search.aria = Search apps and file types

store.area.discover = Discover
store.area.categories = Categories
store.area.installed = Installed
store.area.updates = Updates
store.area.install-file = Install from a file
store.area.offline-sources = Offline sources
store.area.account = Account
store.aria.more-areas = Other areas: install from a file, offline sources, account

store.discover.title = Discover
store.section.components = Components by Ultar
store.section.components.desc = Add or remove parts of the system at any time. Removing a component leaves nothing behind.
store.section.categories = Categories
store.section.categories.desc = Everything in the store, by subject.

store.badge.installed = Installed
store.badge.not-installed = Not installed
store.badge.update = Update available
store.badge.chosen = Chosen at installation
store.chosen.title = { $count } components chosen at installation are not installed yet
store.chosen.message = They are marked below. Open a component to install it with its rights.
store.open = Open
store.aria.open = Open {name}
store.aria.details = {name}: details
store.publisher.verified = {publisher} · verified
store.publisher.unverified = {publisher} · not verified
store.note.sizes = — = size computed at run time; the specification defines no value.
store.note.discover = — = size comes from the catalogue at run time; the specification defines no value. Install and Remove are on the app page.
store.size.unknown = —

store.app.install = Install
store.app.update = Update
store.app.remove = Remove
store.app.cancel = Cancel
store.app.size = Size
store.app.version = Version
store.app.source = Source
store.app.permissions = Permissions in plain words
store.app.permissions-hint = The store asks for nothing when the permissions are only those shown here.
store.app.requires = Needs
store.app.requires-note = {name} is not installed; install it first from its page.
store.app.installed-version = Installed version: {version}
store.app.description = Description
store.app.changes = Changes
store.app.changes.empty = This catalogue carries no change log.
store.note.app-page = — = size and version come from the catalogue at run time; the specification defines no value.
store.app.free-space = Free space required at run time
store.progress.percent = {percent}%

store.search.no-results = No results for “{query}”
store.search.no-results-hint = Try another name or a file type like {ext}.
store.search.clear = Clear search
store.search.results = Results for “{query}”

store.installed.on-device = Installed on this device
store.installed.size = Size: {size}
store.installed.last-used = Last used: {when}
store.installed.last-used.never = Never opened
store.installed.empty = Nothing is installed from the store yet.
store.installed.empty-hint = Open Discover and install your first component.
store.installed.remove = Remove
store.installed.permissions = Permissions

store.updates.apps = App updates
store.updates.system = System updates
store.updates.system-desc = The system switches between two slots; restarting happens when you choose. System updates are managed by the Updater.
store.updates.none = Everything is up to date.
store.updates.available = {count} updates available
store.updates.update-all = Update all
store.updates.version = From {from} to {to}
store.updates.history = History
store.updates.history.empty = No operations recorded yet.
store.updates.rollback = Roll back to the previous version
store.updates.rollback-only-offline = Roll-back is available when the previous version sits in a local source.

store.file.title = Install from a file
store.file.desc = A package file (.uap) on a USB stick or in a folder: the signature is checked first, and nothing is installed before the check passes.
store.file.pick = Pick a file from the sources
store.file.not-signed = This package is unsigned or has an unknown signer; it cannot be installed.
store.file.check = Check the signature and show the package
store.file.info = {name} · version {version} · {size}
store.file.empty = No package files in the local sources.

store.offline.title = Offline sources
store.offline.desc = A USB stick or folder holding a signed catalogue; the store works from it without internet.
store.offline.usb = USB stick: signed catalogue
store.offline.sources = Offline sources
store.offline.browse = Browse
store.offline.none = No signed source was found.
store.offline.sign-ok = Catalogue signed and trusted
store.offline.sign-bad = Signature check failed: {reason}
store.offline.use = Use this source
store.offline.in-use = In use now

store.account.title = Account
store.account.desc = The Ultar account is optional: store purchases, backup and linking devices over the internet. Without it the device stays complete and the store and updates work from offline sources.
store.account.signed-out = Signed out
store.account.signed-in = Signed in
store.account.services = Account services
store.account.sign-in-note = Signing in and management: Settings → Account.

store.state.loading = Loading the store…
store.offline.banner.title = Not connected to the internet
store.offline.banner.desc = Installed apps and offline sources are shown.
store.retry = Retry
store.error.title = Could not load the store
store.error.reason = Reason: {reason}
store.error.timeout = the server {server} did not respond within {seconds} seconds
store.error.local-read = the local catalogue could not be read
store.log = Save the log
store.log.saved = Report saved to {path}
store.log.failed = Could not save the report
store.installed.header = Installed on this device

store.nospace.title = Not enough space to install “{name}”
store.nospace.sizes = Required: {required} · Available: {available}
store.free.heading = What can be freed
store.free.item.trash = Trash
store.free.item.temp = Temporary files
store.free.item.old-updates = Old updates
store.free.note = — = sizes are computed at run time; the specification defines no value.
store.free.cancel = Cancel
store.free.go = Free the space and continue
store.freed.toast = Freed {size}

store.error.unreadable = The catalogue cannot be read; the source is missing or not allowed.
store.error.damaged = The catalogue is damaged or not understood.
store.error.unsigned = The catalogue is not signed; it is not used.
store.error.bad-signature = The catalogue signature does not match its contents.
store.error.unknown-signer = The catalogue was not signed by a key trusted on this device.
store.error.too-new = The catalogue format is newer than this store; update the system.
store.error.package-mismatch = The downloaded package does not match the fingerprint signed in the catalogue; it was deleted and not installed.
store.error.no-launcher = The launcher service is not available in this session.
store.error.package-refused = The package was refused: invalid signature or unknown publisher.
store.error.unknown-app = This app is not installed.
store.error.start-failed = The app could not be started.
store.error.shutting-down = The device is shutting down; try again later.
store.error.storage = The system state could not be stored.
store.error.invalid = Invalid request.
store.error.io = The file could not be read or written.
store.error.unreachable = The system service could not be reached; it may not be running or the link is down.
store.error.protocol = Ununderstood answer from a system service.

store.remove.dialog = Remove {name}?
store.remove.dialog.desc = Choosing keep leaves the app data in the app's own area; choosing delete removes it for good.
store.remove.keep-data = Remove, keep the data
store.remove.delete-data = Remove and delete the data
store.remove.cancel = Cancel
store.cancel.dialog = Cancel the installation?
store.cancel.dialog.desc = Nothing was written yet; you can retry at any time.
store.cancel.keep = Keep waiting
store.cancel.go = Cancel the install

store.perm.files = Files
store.perm.network = Network
store.perm.printing = Printing
store.perm.camera = Camera
store.perm.microphone = Microphone
store.perm.location = Location
store.perm.contacts = Contacts
store.perm.notifications = Notifications
store.perm.background = Background work
store.perm.devices = Connected devices
store.perm.storage = Storage
store.perm.ui.input-method = Keyboard
store.perm.ui.capture = Screen capture

store.category.office = Office
store.category.development = Development
store.category.media = Media
store.category.communication = Communication
store.category.assistant = Assistant and local AI
store.category.linux = Linux programs
store.category.windows = Windows programs
store.category.android = Android apps
store.category.server = Containers and server
store.category.remote = Remote access
store.category.mesh = Device mesh
store.category.system = System

store.tab.discover = Discover
store.tab.installed = Installed
store.tab.updates = Updates
store.aria.window = Ultar Store
store.aria.back = Back
store.aria.app-icon = {name} icon
store.app.system = System app
store.error.protected = This is a base app of the system and cannot be removed. Updates replace it.
