//! The `ultar-settingsd` daemon: command line, start-up and shutdown.

use crate::access::{Access, AllowAll, PolicyAccess};
use crate::apply::{Appliers, LogApplier};
use crate::clock::{Clock, SystemClock};
use crate::core::{Core, CoreConfig};
use crate::error::SettingsError;
use crate::i18n::store_notice_key;
use crate::schema::{NoDefaults, SchemaError};
use crate::seed;
use crate::server::{ErrorSink, Server, ServerParts};
use crate::store::{Notice, StoreConfig};
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::{BusError, Client, ClientOptions, Policy};
use ultar_i18n::{Arg, Lang, Translator};
use ultar_profile::{Capabilities, DeviceProfile};

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
    /// Secret for bus listeners that authenticate by token.
    pub token: Option<String>,
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
}

impl DaemonArgError {
    /// Key of the message for the operator.
    pub const fn message_key(&self) -> &'static str {
        match self {
            DaemonArgError::Arguments => "settingsd.daemon.bad_arguments",
            DaemonArgError::NoPolicy => "settingsd.daemon.no_policy",
        }
    }
}

impl fmt::Display for DaemonArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DaemonArgError::Arguments => "invalid options",
            DaemonArgError::NoPolicy => "--policy FILE or --allow-all is required",
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
    token: Option<String>,
    lang: Option<Lang>,
    allow_all: bool,
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
            "--log-apply" => raw.log_apply = true,
            "--store" => raw.store = Some(PathBuf::from(value()?)),
            "--socket" => raw.socket = Some(PathBuf::from(value()?)),
            "--user" => raw.user = Some(value()?),
            "--policy" => raw.policy = Some(PathBuf::from(value()?)),
            "--profile" => raw.profile = Some(PathBuf::from(value()?)),
            "--token" => raw.token = Some(value()?),
            "--lang" => raw.lang = Some(Lang::parse(&value()?).ok_or(bad.clone())?),
            "--write-delay-ms" => raw.delay_ms = Some(value()?.parse().map_err(|_| bad.clone())?),
            _ => return Err(bad),
        }
    }
    if raw.help {
        return Ok(DaemonCommand::Help);
    }
    if raw.policy.is_none() && !raw.allow_all {
        return Err(DaemonArgError::NoPolicy);
    }
    let write_delay = raw
        .delay_ms
        .map_or(DEFAULT_WRITE_DELAY, Duration::from_millis)
        .min(MAX_WRITE_DELAY);
    Ok(DaemonCommand::Run(DaemonArgs {
        store: raw.store.ok_or(bad.clone())?,
        socket: raw.socket.ok_or(bad)?,
        user: raw.user.unwrap_or_else(|| "default".to_owned()),
        policy: raw.policy.filter(|_| !raw.allow_all),
        profile: raw.profile,
        write_delay,
        token: raw.token,
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
        }
    }

    /// Technical detail for the `{detail}` placeholder.
    pub fn detail(&self) -> String {
        match self {
            DaemonError::Policy(detail) | DaemonError::Profile(detail) => detail.clone(),
            DaemonError::Schema(error) => error.to_string(),
            DaemonError::Store(error) => error.to_string(),
            DaemonError::Bus(error) => error.to_string(),
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
        return Ok(Capabilities::default());
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

/// Reports failures nobody waits for on standard error. It runs on the
/// service thread and therefore owns its translator.
fn error_sink(lang: Lang) -> ErrorSink {
    let translator = crate::i18n::translator(lang).ok();
    Box::new(move |error| {
        let detail = error.to_string();
        let key = "settingsd.daemon.write_error";
        match &translator {
            Some(translator) => {
                eprintln!(
                    "{}",
                    translator.t_args(key, &[("detail", Arg::Str(&detail))])
                );
            }
            None => eprintln!("{key} {detail}"),
        }
    })
}

/// Runs the service until a `settings.shutdown` call ends it.
///
/// Messages for the operator go to `log` (standard error in the binary);
/// one line is written to `ready` (standard output) once the service
/// answers requests, so a supervisor can wait for it.
pub fn run(
    args: &DaemonArgs,
    translator: &Translator,
    ready: &mut dyn Write,
    log: &mut dyn Write,
) -> Result<(), DaemonError> {
    let access = load_access(args)?;
    if args.policy.is_none() {
        let _ = writeln!(log, "{}", translator.t("settingsd.daemon.allow_all"));
    }
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let (core, notices) = open_core(args, &clock, translator, log)?;
    let options = ClientOptions {
        token: args.token.clone(),
        connect_retry_for: BUS_WAIT,
        ..ClientOptions::named("ultar-settingsd")
    };
    let bus = Client::connect(&args.socket, options).map_err(DaemonError::Bus)?;
    let server = Server::start(ServerParts {
        bus,
        core,
        access,
        clock,
        notices,
        register_wait: BUS_WAIT,
        on_error: error_sink(translator.lang()),
    })
    .map_err(DaemonError::Bus)?;
    let _ = writeln!(ready, "{}", translator.t("settingsd.daemon.ready"));
    let _ = ready.flush();
    server.wait();
    let _ = writeln!(log, "{}", translator.t("settingsd.daemon.stopped"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str], env: Option<&str>) -> Result<DaemonCommand, DaemonArgError> {
        parse(
            words.iter().map(|w| (*w).to_owned()),
            env.map(str::to_owned),
        )
    }

    fn args(words: &[&str], env: Option<&str>) -> DaemonArgs {
        match parsed(words, env).unwrap() {
            DaemonCommand::Run(args) => args,
            DaemonCommand::Help => panic!("expected a run command"),
        }
    }

    #[test]
    fn minimal_and_full_command_lines() {
        let minimal = args(&["--store", "/var/lib/s", "--allow-all"], Some("/run/bus"));
        assert_eq!(minimal.store, PathBuf::from("/var/lib/s"));
        assert_eq!(minimal.socket, PathBuf::from("/run/bus"));
        assert_eq!(minimal.user, "default");
        assert_eq!(minimal.policy, None);
        assert_eq!(minimal.write_delay, DEFAULT_WRITE_DELAY);
        assert!(!minimal.log_apply);

        let full = args(
            &[
                "--store",
                "/s",
                "--socket",
                "/b",
                "--user",
                "anas",
                "--policy",
                "/p.toml",
                "--profile",
                "/d.toml",
                "--write-delay-ms",
                "40",
                "--token",
                "tk",
                "--log-apply",
                "--lang",
                "en",
            ],
            Some("/ignored"),
        );
        assert_eq!(full.socket, PathBuf::from("/b"));
        assert_eq!(full.user, "anas");
        assert_eq!(full.policy, Some(PathBuf::from("/p.toml")));
        assert_eq!(full.profile, Some(PathBuf::from("/d.toml")));
        assert_eq!(full.write_delay, Duration::from_millis(40));
        assert_eq!(full.token.as_deref(), Some("tk"));
        assert_eq!(full.lang, Some(Lang::En));
        assert!(full.log_apply);
        let capped = args(
            &[
                "--store",
                "/s",
                "--allow-all",
                "--write-delay-ms",
                "999999999",
            ],
            Some("/b"),
        );
        assert_eq!(capped.write_delay, MAX_WRITE_DELAY);
        // --allow-all wins over a policy file, as the warning says.
        let both = args(
            &["--store", "/s", "--allow-all", "--policy", "/p"],
            Some("/b"),
        );
        assert_eq!(both.policy, None);
    }

    #[test]
    fn help_and_rejections() {
        assert_eq!(parsed(&["--help"], None), Ok(DaemonCommand::Help));
        assert_eq!(
            parsed(&["-h", "--frob"], None),
            Err(DaemonArgError::Arguments)
        );
        assert_eq!(
            parsed(&["--store", "/s"], Some("/b")),
            Err(DaemonArgError::NoPolicy)
        );
        for words in [
            &["--allow-all"][..],
            &["--store"],
            &["--store", "/s", "--allow-all", "--write-delay-ms", "soon"],
            &["--store", "/s", "--allow-all", "--lang", "xx"],
            &["--store", "/s", "--allow-all", "extra"],
        ] {
            assert_eq!(
                parsed(words, Some("/b")),
                Err(DaemonArgError::Arguments),
                "{words:?}"
            );
        }
        // No socket anywhere.
        assert_eq!(
            parsed(&["--store", "/s", "--allow-all"], None),
            Err(DaemonArgError::Arguments)
        );
        assert_eq!(
            parsed(&["--store", "/s", "--allow-all"], Some("")),
            Err(DaemonArgError::Arguments)
        );
    }

    #[test]
    fn every_message_key_of_the_daemon_exists_in_all_languages() {
        let errors = [
            DaemonError::Policy("p".into()),
            DaemonError::Profile("p".into()),
            DaemonError::Schema(SchemaError::InvalidKey("k")),
            DaemonError::Store(SettingsError::Limit),
            DaemonError::Bus(BusError::Timeout),
        ];
        let mut keys: Vec<&str> = errors.iter().map(DaemonError::message_key).collect();
        keys.extend([
            DaemonArgError::Arguments.message_key(),
            DaemonArgError::NoPolicy.message_key(),
            "settingsd.daemon.usage",
            "settingsd.daemon.allow_all",
            "settingsd.daemon.write_error",
            "settingsd.daemon.ready",
            "settingsd.daemon.stopped",
        ]);
        for (lang, _) in crate::i18n::sources() {
            let translator = crate::i18n::translator(lang).unwrap();
            for key in &keys {
                assert!(translator.catalog(lang).contains(key), "{key}");
            }
        }
        for error in errors {
            assert!(!error.detail().is_empty());
            assert!(error.to_string().starts_with("settingsd.daemon."));
        }
        assert!(!DaemonArgError::NoPolicy.to_string().is_empty());
    }

    #[test]
    fn unusable_policy_and_profile_files_stop_the_start() {
        let dir = crate::testdir::TestDir::new("daemon-files");
        let mut base = args(&["--store", "/s", "--allow-all"], Some("/b"));
        assert!(load_access(&base).is_ok());
        assert_eq!(load_capabilities(&base).unwrap(), Capabilities::default());
        base.policy = Some(dir.path().join("missing.toml"));
        assert!(matches!(load_access(&base), Err(DaemonError::Policy(_))));
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "this is = not [a profile").unwrap();
        base.profile = Some(bad);
        assert!(matches!(
            load_capabilities(&base),
            Err(DaemonError::Profile(_))
        ));
        base.profile = Some(dir.path().join("missing.toml"));
        assert!(matches!(
            load_capabilities(&base),
            Err(DaemonError::Profile(_))
        ));
    }
}
