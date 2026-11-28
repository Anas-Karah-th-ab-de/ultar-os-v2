# ultar-files – texts of the Files app
app.files.name = Files
app.files.desc = Browse your folders, files, the trash and USB drives.
files.places = Places
files.place.home = Home
files.place.documents = Documents
files.place.downloads = Downloads
files.place.pictures = Pictures
files.place.music = Music
files.place.videos = Videos
files.place.desktop = Desktop
files.place.trash = Trash
files.place.drive = {name} drive
files.eject = Eject {name} drive
files.back = Back
files.forward = Forward
files.search = Search {place}
files.view_mode = View
files.items = Items
files.desk.name = Desktop
files.count = {count ->
    [one] 1 item
   *[other] {count} items
}
files.new_folder_name = New folder
files.cmd.open = Open
files.cmd.copy = Copy
files.cmd.cut = Cut
files.cmd.paste = Paste
files.cmd.rename = Rename
files.cmd.trash = Move to trash
files.cmd.properties = Properties
files.cmd.restore = Restore
files.cmd.purge = Delete permanently
files.cmd.empty_trash = Empty trash
files.cmd.new_folder = New folder
files.cmd.select_all = Select all
files.cmd.undo = Undo
files.cmd.eject = Eject
files.col.name = Name
files.col.size = Size
files.col.modified = Modified
files.col.kind = Kind
files.kind.folder = Folder
files.kind.text = Document
files.kind.sheet = Spreadsheet
files.kind.image = Picture
files.kind.audio = Audio
files.kind.video = Video
files.kind.archive = Archive
files.kind.web = Web page
files.kind.link = Link
files.kind.other = File
files.empty.folder = This folder is empty
files.empty.search = No results for this search
files.empty.trash = The trash is empty
files.cancel = Cancel
files.close = Close
files.rename.title = Rename “{name}”
files.rename.field = New name
files.empty_trash.title = Empty the trash?
files.empty_trash.message = {count ->
    [one] 1 item will be deleted permanently and cannot be restored.
   *[other] {count} items will be deleted permanently and cannot be restored.
}
files.purge.title = {count ->
    [one] Delete the item permanently?
   *[other] Delete {count} items permanently?
}
files.purge.message = Deleting permanently cannot be undone.
files.properties.title = Properties of “{name}”
files.prop.kind = Kind
files.prop.size = Size
files.prop.modified = Modified
files.prop.place = Location
files.prop.mode = Permissions
files.prop.origin = Original location
files.conflict.title = “{name}” already exists
files.conflict.message = Choose what happens to items with the same name.
files.conflict.skip = Skip
files.conflict.keep_both = Keep both
files.conflict.replace = Replace
files.busy.title = The drive is in use
files.busy.message = An app still uses files on this drive. Close them, then try again.
files.busy.retry = Try again
files.job.progress = Progress
files.job.cancel = Stop
files.notice.copied = {count ->
    [one] “{name}” copied
   *[other] {count} items copied
}
files.notice.moved = {count ->
    [one] “{name}” moved
   *[other] {count} items moved
}
files.notice.trashed = {count ->
    [one] “{name}” moved to the trash
   *[other] {count} items moved to the trash
}
files.notice.restored = {count ->
    [one] “{name}” restored
   *[other] {count} items restored
}
files.notice.purged = Items deleted permanently
files.notice.emptied = Trash emptied
files.notice.undone = Undone
files.notice.cancelled = Stopped
files.notice.ejected = The {name} drive can be removed safely now
files.notice.no_app = No app can open “{name}”
files.notice.open_failed = Couldn’t open “{name}”: the app did not start
files.error.not_found = The item does not exist
files.error.exists = An item with this name exists
files.error.permission = You have no permission for this place
files.error.link = This is a link and is not opened here
files.error.not_folder = This is not a folder
files.error.no_space = Not enough space
files.error.not_empty = The folder is not empty
files.error.bad_name = Invalid name
files.error.no_area = This place is not available to the app
files.error.into_itself = A folder cannot go inside itself
files.error.too_deep = The folder is too deep or too large
files.error.bad_record = A trash record is damaged
files.error.busy = The item is in use
files.error.cancelled = Stopped
files.error.io = Reading or writing failed
files.error.service = The service is not available now
files.error.refused = The request was refused
files.error.folder_hint = Choose another place in the sidebar.
files.hint.permission = The system does not let this app read or write here. Ask the device administrator for permission or choose another place.
files.hint.no_space = Delete files you do not need or empty the trash, then try again.
files.hint.link = Links are not followed, to keep your files safe.
files.hint.no_area = The app was not given access to this place.
files.hint.name = Choose another name without a slash and of reasonable length.
files.hint.retry = Try again in a moment.
files.thumb.unsupported = No preview for this kind
files.thumb.too_large = The picture is too large to preview
files.thumb.damaged = The picture is damaged

## Apps from other systems (docs/FREMD-APPS.md): the install sheet
files.cmd.install = Install
files.foreign.title = Install “{name}”?
files.foreign.title.done = “{name}” is installed
files.foreign.title.problem = “{name}” cannot be installed
files.foreign.install = Install
files.foreign.open = Open
files.foreign.reading = Reading the file…
files.foreign.starting = Starting the installation…
files.foreign.running_hint = Confirm in the system dialog with an administrator's password. You can close this window: the installation goes on.
files.foreign.done = “{name}” is in the app grid now. It runs in its own isolated cell.
files.foreign.details = Details of the file
files.foreign.unchecked = Not checked by KM Code Craft
files.foreign.unchecked_hint = This file comes from another system. Ultar OS runs it in its own isolated cell with nothing but what you allow here. Only allow what it really needs.
files.foreign.may_use = What the app may use – everything stays off until you switch it on:
files.foreign.internet = Internet
files.foreign.internet_hint = Never this device itself, its other apps or its services.
files.foreign.lan = Local network
files.foreign.lan_hint = Printers and other devices in your network.
files.foreign.folders = Your folders it may read and change:
files.foreign.kind = Kind
files.foreign.kind.linux = Linux program
files.foreign.kind.windows = Windows program
files.foreign.kind.android = Android app
files.foreign.kind.unknown = Unknown kind
files.foreign.name = Name (according to the file)
files.foreign.version = Version
files.foreign.publisher = Publisher
files.foreign.publisher_said = {name} – according to the file, not checked
files.foreign.no_publisher = The file names none
files.foreign.step.confirm = Waiting for the confirmation
files.foreign.step.prepare = Preparing
files.foreign.step.install = Installing
files.foreign.step.register = Adding to the app grid
files.foreign.unreadable = The file cannot be read. Check that it is complete and in one of your folders.
files.foreign.too_large = The file is too large to install.
files.foreign.invalid = This file cannot be installed from here.
files.foreign.unknown = Ultar OS cannot install this kind of file.
files.foreign.not_yet = This program needs a part of Ultar OS that is not installed on this device (“Windows programs” or “Android apps”). Linux programs (.deb) can always be installed.
files.foreign.runtime_missing = First install the component “Linux programs” from the Ultar Store.
files.foreign.busy = Another installation is running. Try again when it has finished.
files.foreign.no_service = The service for apps from other systems does not answer. Try again later.
files.foreign.refused = The service refused the request.
files.foreign.cancelled = The installation was not confirmed. Nothing was installed.
files.foreign.failed.damaged = The file is damaged or not what it claims to be. Nothing was installed.
files.foreign.failed.refused = The Linux area refused the package (missing parts or a failing script). Nothing was installed.
files.foreign.failed.no_programs = The package brings no program with a window. Nothing was installed.
files.foreign.failed.network = A download for the installation failed. Nothing was installed.
files.foreign.failed.space = There is not enough space. Nothing was installed.
files.foreign.failed.launcher = The app could not be added. Nothing was installed.
files.foreign.failed.no_confirmation = The confirmation could not be asked. Nothing was installed.
files.foreign.failed.signer_changed = The update is signed by someone else than the installed app; it was not installed.
files.foreign.failed.older = This is an older version than the installed one; it was not installed.
files.foreign.refusal.needs_arm = This app is built for ARM processors only and needs a device with an ARM processor.
files.foreign.refusal.needs_other_processor = This app is built for another processor and does not run on this device.
files.foreign.refusal.unsigned = The app is not signed, so its origin cannot be followed across updates.
files.foreign.refusal.old_signature = The app is signed only the old way (v1), which does not protect the whole file.
files.foreign.refusal.bad_signature = The signature does not match: the file was changed after it was signed.
files.foreign.refusal.no_start_screen = The app has no start screen.
files.foreign.refusal.runtime_missing = The component “Android apps” is not installed.
files.foreign.failed.internal = Something went wrong. Nothing was installed.
files.notice.installed = “{name}” is installed
files.notice.install_cancelled = “{name}” was not installed
files.notice.install_failed = “{name}” could not be installed
