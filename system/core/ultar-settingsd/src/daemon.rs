//! The `ultar-settingsd` daemon: command line, start-up and shutdown.

use crate::access::{Access, AllowAll, PolicyAccess, Reloadable};
use crate::apply::{Appliers, LogApplier};
use crate::clock::{Clock, SystemClock};
use crate::core::{Core, CoreConfig};
use crate::error::SettingsError;
use crate::i18n::store_notice_key;
use crate::schema::{NoDefaults, SchemaError};
use crate::seed;
use crate::server::{store_notice, ErrorSink, Server, ServerParts};
use crate::store::{Notice, StoreConfig};
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::StoreReport;
use ultar_bus::{BusError, Policy};
use ultar_i18n::{Arg, Lang, Translator};
use ultar_profile::{Capabilities, DeviceProfile};
use ultar_service::log::{Level, Logger, Record};
use ultar_service::{
    BusSession, Lifecycle, Secret, SecretError, SecretSource, SessionOptions, StopCause,
};

/// Write delay when none is given: changes within this time become one write.
pub const DEFAULT_WRITE_DELAY: Duration = Duration::from_millis(250);
/// Longest accepted write delay.
pub const MAX_WRITE_DELAY: Duration = Duration::from_secs(60);
/// How long the daemon waits for the bus socket to appear at start.
pub const BUS_WAIT: Duration = Duration::from_secs(10);

/// The parsed command line of the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonArgs {
    /// Directory of the settings files.
    pub store: PathBuf,
    /// Bus socket.
    pub socket: PathBuf,
    /// User whose settings file is used.
    pub user: String,
    /// Bus policy file that decides who is an administrator.
    pub policy: Option<PathBuf>,
    /// Device profile file (TOML of `ultar-profile`) with the capabilities
    /// at start.
    pub profile: Option<PathBuf>,
    /// Longest time a change waits before it is written.
    pub write_delay: Duration,
    /// File with the secret for bus listeners that authenticate by token
    /// (owner and mode are checked).
    pub token_file: Option<PathBuf>,
    /// Inherited descriptor to read the token from instead.
    pub token_fd: Option<i32>,
    /// Ingestion socket of the log service.
    pub log_socket: Option<PathBuf>,
    /// The operator declares that the service runs as root.
    pub allow_root: bool,
    /// Log every applied value to standard error.
    pub log_apply: bool,
    /// Language of the daemon's own messages when given explicitly.
    pub lang: Option<Lang>,
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonCommand {
    /// Print the usage line.
    Help,
    /// Run the service.
    Run(DaemonArgs),
}

/// Why the command line was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonArgError {
    /// A missing value, a bad number, an unknown option or no store/socket.
    Arguments,
    /// Neither `--policy FILE` nor `--allow-all`.
    NoPolicy,
    /// `--policy FILE` together with `--allow-all`: which of the two was
    /// meant cannot be guessed, and guessing wrong makes everybody an
    /// administrator.
    PolicyConflict,
}

impl DaemonArgError {
    /// Key of the message for the operator.
    pub const fn message_key(&self) -> &'static str {
        match self {
            DaemonArgError::Arguments => "settingsd.daemon.bad_arguments",
            DaemonArgError::NoPolicy => "settingsd.daemon.no_policy",
            DaemonArgError::PolicyConflict => "settingsd.daemon.policy_conflict",
        }
    }
}

impl fmt::Display for DaemonArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DaemonArgError::Arguments => "invalid options",
            DaemonArgError::NoPolicy => "--policy FILE or --allow-all is required",
            DaemonArgError::PolicyConflict => "--policy and --allow-all cannot be combined",
        })
    }
}

impl std::error::Error for DaemonArgError {}

#[derive(Default)]
struct Raw {
    store: Option<PathBuf>,
    socket: Option<PathBuf>,
    user: Option<String>,
    policy: Option<PathBuf>,
    profile: Option<PathBuf>,
    delay_ms: Option<u64>,
    token_file: Option<PathBuf>,
    token_fd: Option<i32>,
    log_socket: Option<PathBuf>,
    lang: Option<Lang>,
    allow_all: bool,
    allow_root: bool,
    log_apply: bool,
    help: bool,
}

/// Parses the words after the program name.
pub fn parse(
    args: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
) -> Result<DaemonCommand, DaemonArgError> {
    let bad = DaemonArgError::Arguments;
    let mut args = args.into_iter();
    let mut raw = Raw {
        socket: env_socket
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
        ..Raw::default()
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(DaemonArgError::Arguments);
        match arg.as_str() {
            "--help" | "-h" => raw.help = true,
            "--allow-all" => raw.allow_all = true,
            "--allow-root" => raw.allow_root = true,
            "--log-apply" => raw.log_apply = true,
            "--store" => raw.store = Some(PathBuf::from(value()?)),
            "--socket" => raw.socket = Some(PathBuf::from(value()?)),
            "--user" => raw.user = Some(value()?),
            "--policy" => raw.policy = Some(PathBuf::from(value()?)),
            "--profile" => raw.profile = Some(PathBuf::from(value()?)),
            // There is no `--token`: a secret on the command line is
            // visible to every process on the machine.
            "--token-file" => raw.token_file = Some(PathBuf::from(value()?)),
            "--token-fd" => raw.token_fd = Some(value()?.parse().map_err(|_| bad.clone())?),
            "--log-socket" => raw.log_socket = Some(PathBuf::from(value()?)),
            "--lang" => raw.lang = Some(Lang::parse(&value()?).ok_or(bad.clone())?),
            "--write-delay-ms" => raw.delay_ms = Some(value()?.parse().map_err(|_| bad.clone())?),
            _ => return Err(bad),
        }
    }
    finish(raw)
}

/// Checks the collected options and builds the command.
fn finish(raw: Raw) -> Result<DaemonCommand, DaemonArgError> {
    let bad = DaemonArgError::Arguments;
    if raw.help {
        return Ok(DaemonCommand::Help);
    }
    if raw.policy.is_none() && !raw.allow_all {
        return Err(DaemonArgError::NoPolicy);
    }
    if raw.policy.is_some() && raw.allow_all {
        return Err(DaemonArgError::PolicyConflict);
    }
    if raw.token_file.is_some() && raw.token_fd.is_some() {
        return Err(bad);
    }
    let write_delay = raw
        .delay_ms
        .map_or(DEFAULT_WRITE_DELAY, Duration::from_millis)
        .min(MAX_WRITE_DELAY);
    Ok(DaemonCommand::Run(DaemonArgs {
        store: raw.store.ok_or(bad.clone())?,
        socket: raw.socket.ok_or(bad)?,
        user: raw.user.unwrap_or_else(|| "default".to_owned()),
        policy: raw.policy,
        profile: raw.profile,
        write_delay,
        token_file: raw.token_file,
        token_fd: raw.token_fd,
        log_socket: raw.log_socket,
        allow_root: raw.allow_root,
        log_apply: raw.log_apply,
        lang: raw.lang,
    }))
}

/// Why the daemon could not start.
#[derive(Debug)]
pub enum DaemonError {
    /// The policy file was not accepted.
    Policy(String),
    /// The device profile file was not accepted.
    Profile(String),
    /// The seed schema is faulty.
    Schema(SchemaError),
    /// The settings directory could not be opened.
    Store(SettingsError),
    /// No connection to the bus, or the service could not be registered.
    Bus(BusError),
    /// The bus token could not be read from its file or descriptor.
    Token(SecretError),
}

impl DaemonError {
    /// Key of the message for the operator; its placeholder is `{detail}`.
    pub const fn message_key(&self) -> &'static str {
        match self {
            DaemonError::Policy(_) => "settingsd.daemon.policy_rejected",
            DaemonError::Profile(_) => "settingsd.daemon.profile_rejected",
            DaemonError::Schema(_) => "settingsd.daemon.schema_error",
            DaemonError::Store(_) => "settingsd.daemon.store_error",
            DaemonError::Bus(_) => "settingsd.daemon.bus_error",
            DaemonError::Token(_) => "settingsd.daemon.token_rejected",
        }
    }

    /// Technical detail for the `{detail}` placeholder.
    pub fn detail(&self) -> String {
        match self {
            DaemonError::Policy(detail) | DaemonError::Profile(detail) => detail.clone(),
            DaemonError::Schema(error) => error.to_string(),
            DaemonError::Store(error) => error.to_string(),
            DaemonError::Bus(error) => error.to_string(),
            // The key of the reason, never any part of the token.
            DaemonError::Token(error) => error.key().to_owned(),
        }
    }
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.message_key(), self.detail())
    }
}

impl std::error::Error for DaemonError {}

fn load_access(args: &DaemonArgs) -> Result<Box<dyn Access>, DaemonError> {
    match &args.policy {
        None => Ok(Box::new(AllowAll)),
        Some(path) => Policy::load(path)
            .map(|policy| Box::new(PolicyAccess(policy)) as Box<dyn Access>)
            .map_err(|error| DaemonError::Policy(error.to_string())),
    }
}

fn load_capabilities(args: &DaemonArgs) -> Result<Capabilities, DaemonError> {
    let Some(path) = &args.profile else {
        // Until the device service reports: a keyboard and a pointer (the Keyboard and Mouse
        // pages), as on every desktop machine.
        let mut capabilities = Capabilities::default();
        capabilities
            .flags
            .insert(ultar_profile::Capability::Keyboard);
        capabilities
            .flags
            .insert(ultar_profile::Capability::Pointer);
        return Ok(capabilities);
    };
    let text =
        std::fs::read_to_string(path).map_err(|error| DaemonError::Profile(error.to_string()))?;
    DeviceProfile::from_toml(&text)
        .map(|profile| profile.capabilities)
        .map_err(|error| DaemonError::Profile(error.to_string()))
}

fn open_core(
    args: &DaemonArgs,
    clock: &Arc<dyn Clock>,
    translator: &Translator,
    log: &mut dyn Write,
) -> Result<(Core, Vec<Notice>), DaemonError> {
    let appliers = if args.log_apply {
        Appliers::new().with_all(&LogApplier::new(std::io::stderr()))
    } else {
        Appliers::new()
    };
    let config = CoreConfig {
        store: StoreConfig {
            dir: args.store.clone(),
            user: args.user.clone(),
            write_delay: args.write_delay,
        },
        registry: Arc::new(seed::registry().map_err(DaemonError::Schema)?),
        clock: Arc::clone(clock),
        appliers,
        defaults: Box::new(NoDefaults),
        capabilities: load_capabilities(args)?,
    };
    let (core, notices) = Core::open(config).map_err(DaemonError::Store)?;
    for notice in &notices {
        let args = [
            ("file", Arg::Str(&notice.file)),
            ("detail", Arg::Str(&notice.detail)),
        ];
        let _ = writeln!(
            log,
            "{}",
            translator.t_args(store_notice_key(notice.kind), &args)
        );
    }
    Ok((core, notices))
}

/// Reports failures nobody waits for (a write that failed after the
/// request was answered) to the log service. The record carries the
/// error's wire code, never file content.
fn error_sink(logger: Logger) -> ErrorSink {
    Box::new(move |error| {
        let code = match error.code() {
            ultar_bus::ErrorCode::Service(code) => u64::from(code),
            _ => 0,
        };
        logger.emit(Record::new(Level::Error, "settingsd.write_failed").count("code", code));
    })
}

/// Where the bus token comes from, if one is named.
pub fn token_source(args: &DaemonArgs) -> Option<SecretSource> {
    match (&args.token_file, args.token_fd) {
        (Some(path), _) => Some(SecretSource::File(path.clone())),
        (None, Some(fd)) => Some(SecretSource::Fd(fd)),
        (None, None) => None,
    }
}

/// Reads the policy again after `SIGHUP`; a faulty file keeps the old one.
fn reload_on_hangup(args: &DaemonArgs, access: &Reloadable, hooks: &Hooks<'_>) {
    if args.policy.is_none() {
        return;
    }
    let (args, access, logger) = (args.clone(), access.clone(), hooks.logger.clone());
    hooks.lifecycle.on_reload(move || match load_access(&args) {
        Ok(policy) => {
            access.replace(policy);
            logger.log(Level::Notice, "settingsd.policy_reloaded");
        }
        Err(_) => logger.log(Level::Error, "settingsd.policy_reload_failed"),
    });
}

/// What the daemon needs from the service runtime.
pub struct Hooks<'a> {
    /// State machine: the daemon declares itself ready and reacts to stop
    /// requests (signals).
    pub lifecycle: &'a Lifecycle,
    /// Where operational records go.
    pub logger: &'a Logger,
    /// Called once the service answers requests.
    pub ready: &'a dyn Fn(),
    /// Reads the bus token from a checked file or a declared inherited
    /// descriptor (the runtime's `read_secret`).
    pub secret: &'a dyn Fn(&SecretSource) -> Result<Secret, SecretError>,
}

/// One record per store notice for the log service.
fn log_notices(notices: &[Notice], logger: &Logger) {
    for notice in notices {
        let record = Record::new(Level::Warn, "settingsd.store_notice")
            .word("kind", notice_word(notice.kind));
        logger.emit(record);
    }
}

/// Joins the bus. `None`: the service was stopped while the broker was
/// awaited; that ends the start at once, cleanly (review round 3, R3-S1).
fn connect(
    args: &DaemonArgs,
    token: Option<Secret>,
    hooks: &Hooks<'_>,
) -> Result<Option<BusSession>, DaemonError> {
    let mut options = SessionOptions::named("ultar-settingsd");
    options.token = token;
    options.connect_wait = BUS_WAIT;
    options.register_wait = BUS_WAIT;
    options.stop = Some(hooks.lifecycle.clone());
    let session = match BusSession::connect(&args.socket, options) {
        Ok(session) => session,
        Err(_) if hooks.lifecycle.is_stopping() => return Ok(None),
        Err(error) => return Err(DaemonError::Bus(error)),
    };
    let client = session.client().clone();
    hooks.lifecycle.on_stop(move || client.close());
    Ok(Some(session))
}

/// Runs the service until a stop signal or a `settings.shutdown` call
/// ends it. Either way everything pending is written to disk first.
///
/// Start-up messages for an operator go to `log` (standard error in the
/// binary) and, as records, to the log service; one line is written to
/// `ready` (standard output) once the service answers requests.
pub fn run(
    args: &DaemonArgs,
    translator: &Translator,
    ready: &mut dyn Write,
    log: &mut dyn Write,
    hooks: &Hooks<'_>,
) -> Result<(), DaemonError> {
    let access = Reloadable::new(load_access(args)?);
    reload_on_hangup(args, &access, hooks);
    if args.policy.is_none() {
        let _ = writeln!(log, "{}", translator.t("settingsd.daemon.allow_all"));
        hooks.logger.log(Level::Warn, "settingsd.allow_all");
    }
    let token = token_source(args)
        .map(|source| (hooks.secret)(&source))
        .transpose()
        .map_err(DaemonError::Token)?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let (core, notices) = open_core(args, &clock, translator, log)?;
    log_notices(&notices, hooks.logger);
    let Some(session) = connect(args, token, hooks)? else {
        return Ok(());
    };
    if !notices.is_empty() {
        // The server publishes the report once; the session publishes it
        // again whenever the broker comes back with empty memory.
        let report = StoreReport {
            notices: notices.iter().map(store_notice).collect(),
        };
        session.remember(&report).map_err(DaemonError::Bus)?;
    }
    let server = Server::start(ServerParts {
        bus: session.client().clone(),
        core,
        access: Box::new(access),
        clock,
        notices,
        register_wait: BUS_WAIT,
        on_error: error_sink(hooks.logger.clone()),
    });
    let server = match server {
        Ok(server) => server,
        // Stopped while the service was being registered.
        Err(_) if hooks.lifecycle.is_stopping() => return Ok(()),
        Err(error) => return Err(DaemonError::Bus(error)),
    };
    let stopper = server.stopper();
    hooks.lifecycle.on_stop(move || stopper.stop());
    (hooks.ready)();
    let _ = writeln!(ready, "{}", translator.t("settingsd.daemon.ready"));
    let _ = ready.flush();
    // Returns after the pending writes are on disk.
    server.wait();
    // Ended by `settings.shutdown` rather than by a signal: tell the
    // runtime so that the supervisor learns it, too.
    hooks.lifecycle.request_stop(StopCause::Request);
    hooks.logger.log(Level::Info, "settingsd.flushed");
    drop(session);
    Ok(())
}

/// A fixed word for each kind of store notice (records carry no file
/// names and no file content).
const fn notice_word(kind: ultar_bus::messages::settings::StoreNoticeKind) -> &'static str {
    use ultar_bus::messages::settings::StoreNoticeKind as Kind;
    match kind {
        Kind::Quarantined => "quarantined",
        Kind::NewerSchema => "newer_schema",
        Kind::Migrated => "migrated",
        Kind::UnknownKeyDropped => "unknown_key_dropped",
        Kind::InvalidValueDropped => "invalid_value_dropped",
        Kind::Unreadable => "unreadable",
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
