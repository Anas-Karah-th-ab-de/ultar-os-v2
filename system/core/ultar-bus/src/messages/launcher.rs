//! Contract of the app launcher `ultar-launcher`: installed apps, their
//! running instances and the permissions the user granted them.
//!
//! The shell calls `start`, `stop`, `list`, `permissions`, `grant`, `revoke`
//! (its permission prompts are system dialogs), so does `ulaunch`. `install` and
//! `uninstall` are for the installer and administrators: the policy gives
//! them only to those identities, and the launcher checks the caller
//! against the pseudo method [`ADMIN_METHOD`] as well.
//!
//! Every app runs as its own user in its own cell with exactly the granted
//! permissions its signed manifest declares (`docs/integration/ultar-launcher.md`).

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "launcher";
/// Pseudo method whose call right marks an installer or administrator.
pub const ADMIN_METHOD: &str = "admin";
/// Most apps one [`ListReply`] lists: every app the launcher can hold, so
/// nothing is left out (≈ 150 bytes each, below a bus frame).
pub const MAX_APPS: usize = 4096;

/// Which program of an app an instance runs.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The app's window (started by the user, never restarted).
    Ui,
    /// The app's background service (`[service]` of the manifest; needs the
    /// permission `background`; restarted after its policy).
    Service,
}

impl Role {
    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Service => "service",
        }
    }
}

/// Where an instance is in its life.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstanceState {
    /// The cell is being built.
    Starting,
    /// The app runs.
    Running,
    /// A stop was sent; waiting for the cell to end.
    Stopping,
    /// Ended; a service waits before its next start.
    Backoff,
    /// Ended normally or stopped on request.
    Stopped,
    /// Ended by a crash (signal or failure code) and not restarted.
    Crashed,
    /// A service that crashed too often in a short time: given up.
    Failed,
}

impl InstanceState {
    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Backoff => "backoff",
            Self::Stopped => "stopped",
            Self::Crashed => "crashed",
            Self::Failed => "failed",
        }
    }

    /// Whether a process of the instance exists (or is about to).
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

/// How an instance ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// The app returned this exit code.
    Code(i32),
    /// A signal with this number ended it.
    Signal(i32),
    /// The cell could not be built; the code names the step
    /// (`ultar-launcher`'s cell failure codes 110–127).
    Setup(i32),
}

/// One instance in an [`AppStatus`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InstanceStatus {
    /// Which program.
    pub role: Role,
    /// Current state.
    pub state: InstanceState,
    /// Process id of the app (the cell's first process) as the launcher
    /// sees it; 0 when none runs.
    pub pid: u32,
    /// Starts after the first one (crash restarts of a service).
    pub restarts: u32,
    /// How the previous run ended, if there was one.
    pub last_exit: Option<Exit>,
}

/// One installed app in a [`ListReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AppStatus {
    /// App id.
    pub app: String,
    /// The app's own user id.
    pub uid: u32,
    /// Its profile (`app.standard`, `app.system`).
    pub profile: String,
    /// Version from the manifest.
    pub version: String,
    /// Its instances (only those that ran since the launcher started).
    pub instances: Vec<InstanceStatus>,
}

/// `launcher.list` – the installed apps and their instances.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct List {}

/// Answer to [`List`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListReply {
    /// The apps, by id (at most [`MAX_APPS`]).
    pub apps: Vec<AppStatus>,
}

/// `launcher.start` – start an app (single instance per role: a running
/// instance is reported, not started twice). Version 2; version-1 callers
/// ([`StartV1`]) are still answered.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Start {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
    /// Activation token of the compositor for the user action that asked
    /// for the start (`RequestActivation`); handed to the app so that its
    /// first window may take the focus once. `None`/0: none.
    pub activation_token: Option<u64>,
    /// What to open: one URI of a scheme the app declares (`[open] schemes`),
    /// e.g. `trash:///`, `settings://network`. At most [`MAX_URI`] bytes, no
    /// control characters. Handed to a fresh instance as data
    /// (`ULTAR_OPEN_URI`), never interpreted by the launcher. `None`: start
    /// or raise.
    pub uri: Option<String>,
    /// Another window even when the instance runs (`[open] multi_window`).
    pub new_window: bool,
}

/// Longest URI of [`Start::uri`] in bytes.
pub const MAX_URI: usize = 2048;

/// Answer to [`Start`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartReply {
    /// State of the instance now.
    pub state: InstanceState,
    /// Process id of the app as the launcher sees it.
    pub pid: u32,
    /// The instance was running already (nothing was started).
    pub already: bool,
    /// The URI was handed to the app.
    pub opened: bool,
}

/// `launcher.start` as version-1 callers send it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct StartV1 {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
    /// Activation token.
    pub activation_token: Option<u64>,
}

/// The answer version-1 callers get.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartReplyV1 {
    /// State of the instance now.
    pub state: InstanceState,
    /// Process id.
    pub pid: u32,
    /// Running already.
    pub already: bool,
}

/// `launcher.stop` – stop an instance (a service stays stopped).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Stop {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
}

/// Answer to [`Stop`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StopReply {
    /// State of the instance now.
    pub state: InstanceState,
}

/// One permission of an app in a [`PermissionsReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PermissionState {
    /// Name (`network`, `background`, `ui.capture` …).
    pub name: String,
    /// Whether the user (or, for system apps, the installation) granted it.
    pub granted: bool,
}

/// `launcher.permissions` – what an app declared and what it was granted.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Permissions {
    /// App id.
    pub app: String,
}

/// Answer to [`Permissions`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct PermissionsReply {
    /// The permissions the launcher enforces, as declared in the manifest.
    pub declared: Vec<PermissionState>,
    /// Bus calls the manifest declares (enforced by the bus policy).
    pub bus: Vec<String>,
    /// Named file areas the manifest declares (not granted by the launcher).
    pub files: Vec<String>,
}

/// `launcher.grant` – the user granted a declared permission.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Grant {
    /// App id.
    pub app: String,
    /// Permission name.
    pub permission: String,
}

/// `launcher.revoke` – the user took a permission back. Running instances
/// whose cell depends on it are stopped.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Revoke {
    /// App id.
    pub app: String,
    /// Permission name.
    pub permission: String,
}

/// Answer to [`Grant`] and [`Revoke`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermissionReply {
    /// Whether the permission is granted now.
    pub granted: bool,
    /// Instances that were stopped because of the change.
    pub stopped: u32,
}

/// `launcher.install` – register a signed package (`.uap`) that the
/// package installer put in place: verify its signature, give the app its
/// user, data directory and memory-group settings.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Install {
    /// Absolute path of the `.uap` file (readable by the launcher).
    pub package: String,
}

/// Answer to [`Install`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InstallReply {
    /// App id.
    pub app: String,
    /// Its user id (the same as before if it was installed earlier).
    pub uid: u32,
    /// Its profile.
    pub profile: String,
    /// Permissions it declares, with their state.
    pub declared: Vec<PermissionState>,
}

/// `launcher.uninstall` – remove an app: its instances are stopped, its
/// data removed (unless kept); its user id stays reserved for it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Uninstall {
    /// App id.
    pub app: String,
    /// Keep the data directory (for a reinstall).
    pub keep_data: bool,
}

/// Answer to [`Uninstall`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UninstallReply {
    /// Instances that were stopped.
    pub stopped: u32,
}

/// Error code: no installed app has this id.
pub const ERROR_UNKNOWN_APP: u32 = 1;
/// Error code: the manifest does not declare the permission (or the role).
pub const ERROR_NOT_DECLARED: u32 = 2;
/// Error code: the permission cannot be granted to this app's profile.
pub const ERROR_NOT_GRANTABLE: u32 = 3;
/// Error code: the package or its manifest was refused (signature, rules).
pub const ERROR_PACKAGE_REFUSED: u32 = 4;
/// Error code: the kernel lacks an isolation feature the start needs.
pub const ERROR_PLATFORM: u32 = 5;
/// Error code: a limit (running apps, user ids) is reached.
pub const ERROR_LIMIT: u32 = 6;
/// Error code: the cell could not be started.
pub const ERROR_START_FAILED: u32 = 7;
/// Error code: the launcher is shutting down.
pub const ERROR_SHUTTING_DOWN: u32 = 8;
/// Error code: the launcher's state could not be stored.
pub const ERROR_STORAGE: u32 = 9;
/// Error code: a permission the instance needs is not granted.
pub const ERROR_NOT_GRANTED: u32 = 10;
/// Error code: the request names something invalid (app id, path).
pub const ERROR_INVALID: u32 = 11;
/// Error code: the URI or the new window cannot be handed to the running
/// instance (apps have no bus endpoint yet).
pub const ERROR_NO_OPEN: u32 = 12;
/// Error code: the app is being uninstalled or its cell did not end.
pub const ERROR_BUSY: u32 = 13;

/// A kind of thing a person opens, for which the settings name a default app
/// (`apps.default.<word>`). The launcher resolves a URI to the app of its kind
/// ([`Resolve`]) and lists the installed apps that declare a kind ([`Candidates`]).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A web page (`http`, `https`).
    Web,
    /// A mail address (`mailto`).
    Mail,
    /// An appointment (`calendar`, `webcal`).
    Calendar,
    /// Sound (`file://…` with a sound extension).
    Music,
    /// A film (`file://…` with a film extension).
    Video,
    /// A picture (`file://…` with a picture extension).
    Photos,
    /// A document (`file://…` with a document extension).
    Documents,
}

impl Kind {
    /// Every kind, in the order of the settings rows.
    pub const ALL: [Kind; 7] = [
        Kind::Web,
        Kind::Mail,
        Kind::Calendar,
        Kind::Music,
        Kind::Video,
        Kind::Photos,
        Kind::Documents,
    ];

    /// Stable lower-case word (the last part of its settings key).
    pub const fn word(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Mail => "mail",
            Self::Calendar => "calendar",
            Self::Music => "music",
            Self::Video => "video",
            Self::Photos => "photos",
            Self::Documents => "documents",
        }
    }

    /// The kind of a word, if it names one.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }

    /// The settings key whose value names this kind's default app.
    pub fn key(self) -> String {
        format!("apps.default.{}", self.word())
    }

    /// The system default app (the settings schema's default): used when the stored
    /// value names no installed app that declares the kind.
    pub const fn default_app(self) -> &'static str {
        match self {
            Self::Web => "browser",
            Self::Mail => "mail",
            Self::Calendar => "calendar",
            Self::Music | Self::Video => "media",
            Self::Photos => "images",
            Self::Documents => "documents",
        }
    }

    /// The URI schemes whose declaration in a manifest makes an app a candidate for
    /// this kind. The four file kinds share the `file`/`files` schemes: an app that opens
    /// files declares one of them, and the extension tells the kinds apart.
    pub const fn schemes(self) -> &'static [&'static str] {
        match self {
            Self::Web => &["http", "https"],
            Self::Mail => &["mailto"],
            Self::Calendar => &["calendar", "webcal"],
            Self::Music | Self::Video | Self::Photos | Self::Documents => &["file", "files"],
        }
    }
}

/// The kind of thing a URI names: its scheme tells the web, mail and calendar kinds; a
/// `file://` URI's extension tells the four file kinds apart. Anything else names no kind.
/// The launcher resolves through this ([`Resolve`]); openers use it to ask for a default.
pub fn kind_of_uri(uri: &str) -> Option<Kind> {
    let (scheme, rest) = uri.split_once(':')?;
    match scheme {
        "http" | "https" => Some(Kind::Web),
        "mailto" => Some(Kind::Mail),
        "calendar" | "webcal" => Some(Kind::Calendar),
        "file" | "files" => kind_of_extension(rest),
        _ => None,
    }
}

/// The file kind of a `file://` URI's path (its extension, lower case).
fn kind_of_extension(rest: &str) -> Option<Kind> {
    let path = rest.split(['?', '#']).next()?;
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    let kind = match ext.as_str() {
        "mp3" | "flac" | "ogg" | "oga" | "wav" | "m4a" | "aac" | "opus" | "wma" | "mid" | "midi" => {
            Kind::Music
        }
        "mp4" | "m4v" | "mkv" | "webm" | "avi" | "mov" | "wmv" | "flv" | "mpg" | "mpeg" | "ts" => {
            Kind::Video
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "avif" | "heic"
        | "ico" => Kind::Photos,
        "pdf" | "txt" | "md" | "rtf" | "odt" | "ods" | "odp" | "doc" | "docx" | "xls" | "xlsx"
        | "ppt" | "pptx" | "csv" | "epub" | "html" | "htm" => Kind::Documents,
        _ => return None,
    };
    Some(kind)
}

/// Most apps [`CandidatesReply`] lists for one kind.
pub const MAX_CANDIDATES: usize = 256;

/// `launcher.resolve` – the default app for a URI: the launcher reads `apps.default.*`
/// (at start and on `settings.changed`), takes the app of the URI's kind, and keeps it
/// only when it is installed and its manifest declares the URI's scheme; an invalid
/// value falls back to the system default ([`Kind::default_app`]). The answer is the
/// launcher's app id, or `None` when no installed app declares the URI. The caller
/// starts the app (`start`, with its activation token); the launcher only resolves.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Resolve {
    /// The URI to open (at most [`MAX_URI`] bytes, no control character).
    pub uri: String,
}

/// Answer to [`Resolve`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ResolveReply {
    /// The app that opens the URI, or `None` when none is installed for it.
    pub app: Option<String>,
}

/// `launcher.candidates` – the installed apps that declare a kind (their manifests list
/// one of its schemes). The Settings page offers these as the choices of `apps.default.*`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Candidates {
    /// The kind.
    pub kind: Kind,
}

/// Answer to [`Candidates`]: app ids, at most [`MAX_CANDIDATES`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct CandidatesReply {
    /// The installed apps that declare the kind.
    pub apps: Vec<String>,
}

macro_rules! messages {
    ($($t:ty),*) => {$(impl Message for $t { const VERSION: u16 = 1; })*};
}
impl Message for Start {
    const VERSION: u16 = 2;
}
impl Message for StartReply {
    const VERSION: u16 = 2;
}
messages!(
    List,
    ListReply,
    StartV1,
    StartReplyV1,
    Stop,
    StopReply,
    Permissions,
    PermissionsReply,
    Grant,
    Revoke,
    PermissionReply,
    Install,
    InstallReply,
    Uninstall,
    UninstallReply,
    Resolve,
    ResolveReply,
    Candidates,
    CandidatesReply
);

macro_rules! calls {
    ($($t:ty => $m:literal, $r:ty);*) => {$(
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    )*};
}
calls!(
    List => "list", ListReply;
    Start => "start", StartReply;
    StartV1 => "start", StartReplyV1;
    Stop => "stop", StopReply;
    Permissions => "permissions", PermissionsReply;
    Grant => "grant", PermissionReply;
    Revoke => "revoke", PermissionReply;
    Install => "install", InstallReply;
    Uninstall => "uninstall", UninstallReply;
    Resolve => "resolve", ResolveReply;
    Candidates => "candidates", CandidatesReply
);

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod tests;

/// The prefix surfaces put before the id of an app of the system (`org.ultar.files`).
pub const BASE_PREFIX: &str = "org.ultar.";

/// The id the launcher knows an app by: the apps of the system have one-part ids
/// (`files`, `settings`), while windows and surfaces name them `org.ultar.<id>`; a dotted
/// id of a store publisher stays as it is.
pub fn launcher_id(app: &str) -> &str {
    app.strip_prefix(BASE_PREFIX)
        .filter(|rest| !rest.is_empty() && !rest.contains('.'))
        .unwrap_or(app)
}

#[cfg(test)]
mod id_tests {
    use super::launcher_id;

    #[test]
    fn only_the_system_prefix_before_a_one_part_id_is_dropped() {
        assert_eq!(launcher_id("org.ultar.files"), "files");
        assert_eq!(launcher_id("files"), "files");
        assert_eq!(
            launcher_id("km-code-craft.appbuilder"),
            "km-code-craft.appbuilder"
        );
        assert_eq!(launcher_id("org.ultar.a.b"), "org.ultar.a.b");
        assert_eq!(launcher_id("org.ultar."), "org.ultar.");
    }
}
