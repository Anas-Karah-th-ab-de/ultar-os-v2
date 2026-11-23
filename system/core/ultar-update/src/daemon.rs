//! The `ultar-updated` daemon: wires configuration, boot control, engine, network source and
//! bus together.

use crate::busfront::{serve, BusPublisher};
use crate::clock::SystemClock;
use crate::commit::BootOutcome;
use crate::config::{ConfigError, DaemonConfig};
use crate::engine::{Engine, Environment};
use crate::error::UpdateError;
use crate::fault::CancelFlag;
use crate::filectl::FileBootControl;
use crate::fsutil::{create_private_dir, read_bounded};
use crate::health::{BusHealth, FsSpace, HealthCheck};
use crate::net::HttpSource;
use crate::service::{Core, SourceFactory};
use crate::source::UpdateSource;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::{BusError, Client, ClientOptions};

/// Environment variable with the bus socket path (used when `--socket` is absent).
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";

/// Why the daemon could not run.
#[derive(Debug)]
pub enum DaemonError {
    /// Wrong command line.
    Usage,
    /// The configuration file could not be read.
    ConfigRead(std::io::Error),
    /// The configuration file is invalid.
    Config(ConfigError),
    /// The bus could not be reached or refused the service.
    Bus(BusError),
    /// The updater itself failed to start.
    Update(UpdateError),
}

impl DaemonError {
    /// i18n key of the message.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Usage => "update.daemon.usage",
            Self::ConfigRead(_) | Self::Config(_) => "update.daemon.config",
            Self::Bus(_) => "update.daemon.bus",
            Self::Update(error) => error.key(),
        }
    }

    /// Line of a configuration error (0 otherwise) and a technical detail for the log.
    pub fn detail(&self) -> (usize, String) {
        match self {
            Self::Config(error) => (error.line, error.to_string()),
            Self::ConfigRead(error) => (0, error.kind().to_string()),
            Self::Bus(error) => (0, error.to_string()),
            Self::Update(error) => (0, error.to_string()),
            Self::Usage => (0, String::new()),
        }
    }
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key(), self.detail().1)
    }
}

impl std::error::Error for DaemonError {}

impl From<UpdateError> for DaemonError {
    fn from(error: UpdateError) -> Self {
        Self::Update(error)
    }
}

impl From<BusError> for DaemonError {
    fn from(error: BusError) -> Self {
        Self::Bus(error)
    }
}

/// The daemon's command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonArgs {
    /// Configuration file.
    pub config: PathBuf,
    /// Bus socket.
    pub socket: PathBuf,
    /// Socket of the log service; `None`: log records are not sent anywhere.
    pub log_socket: Option<PathBuf>,
}

/// Reads `--config FILE [--socket PATH] [--log-socket PATH] [--lang xx]`; the bus socket may
/// come from the environment.
pub fn parse_args(words: &[String], socket_env: Option<String>) -> Result<DaemonArgs, DaemonError> {
    let args = crate::args::Args::parse(words, &[]).map_err(|_| DaemonError::Usage)?;
    let known = args
        .only(&["--config", "--socket", "--log-socket", "--lang"])
        .is_ok()
        && args.positional.is_empty();
    let one = |name| args.one(name).ok().flatten().map(str::to_string);
    match (known, one("--config"), one("--socket").or(socket_env)) {
        (true, Some(config), Some(socket)) => Ok(DaemonArgs {
            config: PathBuf::from(config),
            socket: PathBuf::from(socket),
            log_socket: one("--log-socket").map(PathBuf::from),
        }),
        _ => Err(DaemonError::Usage),
    }
}

/// Loads and parses the configuration file (at most 64 KiB).
pub fn load_config(path: &Path) -> Result<DaemonConfig, DaemonError> {
    let bytes = read_bounded(path, 64 * 1024)
        .map_err(DaemonError::ConfigRead)?
        .ok_or_else(|| DaemonError::ConfigRead(std::io::ErrorKind::NotFound.into()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| DaemonError::ConfigRead(std::io::ErrorKind::InvalidData.into()))?;
    DaemonConfig::parse(&text).map_err(DaemonError::Config)
}

/// The configured update server; the session token is read anew for every operation and is
/// never logged.
pub struct ServerSources {
    server: Option<String>,
    token_file: Option<PathBuf>,
}

impl ServerSources {
    /// Sources for `server`, with the token (if any) in `token_file`.
    pub fn new(server: Option<String>, token_file: Option<PathBuf>) -> Self {
        Self { server, token_file }
    }
}

impl SourceFactory for ServerSources {
    fn open(&self) -> Result<Box<dyn UpdateSource + Send>, UpdateError> {
        let server = self.server.as_deref().ok_or(UpdateError::NoUpdate)?;
        let token = self
            .token_file
            .as_deref()
            .and_then(|path| read_bounded(path, 4096).ok().flatten())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.lines().next().map(str::to_string));
        Ok(Box::new(HttpSource::new(server, token)?))
    }
}

/// The timer wakes at least this often (seconds) to see whether an automatic run is due.
pub const TIMER_WAKE: u64 = 5 * 60;

/// Takes the exclusive lock of the state directory (file `lock` in it, created with the
/// directory if needed). A second daemon on the same state gets [`UpdateError::Busy`] and
/// must not start. The lock is held as long as the returned file lives.
pub fn lock_state_dir(state_dir: &Path) -> Result<std::fs::File, UpdateError> {
    create_private_dir(state_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state_dir.join("lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(UpdateError::Busy),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Repeats `check` until it passes or `attempts` are used up, calling `pause` in between.
/// Services need a moment after a start; a new release is judged only when they had it.
pub fn wait_healthy(
    check: &dyn HealthCheck,
    attempts: u32,
    pause: &mut dyn FnMut(),
) -> Result<(), UpdateError> {
    for attempt in 0..attempts.max(1) {
        if check.check().is_ok() {
            return Ok(());
        }
        if attempt + 1 < attempts {
            pause();
        }
    }
    Err(UpdateError::Unhealthy)
}

struct Patient<'a> {
    inner: &'a dyn HealthCheck,
    attempts: u32,
    stop: &'a CancelFlag,
}

impl HealthCheck for Patient<'_> {
    fn check(&self) -> Result<(), UpdateError> {
        let waited = wait_healthy(self.inner, self.attempts, &mut || {
            if !self.stop.is_stopping() {
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        // A stop during the wait is no verdict about the new release.
        match waited {
            Err(_) if self.stop.is_stopping() => Err(UpdateError::Interrupted),
            other => other,
        }
    }
}

/// What the program around the daemon provides (the service runtime in production).
pub struct Hooks<'a> {
    /// Called once the service is registered on the bus, with what the start-up check found
    /// (`None` when that check itself failed or was interrupted by a stop).
    pub ready: &'a mut dyn FnMut(Option<BootOutcome>),
    /// Registers what has to happen on a stop signal. Returns `false` when the stop has
    /// already begun (the action is then not registered and the daemon ends at once).
    pub on_stop: &'a dyn Fn(Box<dyn FnOnce() + Send>) -> bool,
}

/// Registers the stop action: interrupt the running operation, close the bus connection.
/// `false` when the stop has already begun (the connection is then closed at once).
fn register_stop(hooks: &Hooks<'_>, cancel: &Arc<CancelFlag>, client: &Client) -> bool {
    let (stop_flag, stop_client) = (cancel.clone(), client.clone());
    let registered = (hooks.on_stop)(Box::new(move || {
        stop_flag.stop();
        stop_client.close();
    }));
    if !registered {
        client.close();
    }
    registered
}

/// Runs the daemon until the bus connection ends or a stop is requested through
/// [`Hooks::on_stop`]. A stop ends a running operation at its next checkpoint without clean-up
/// (the same as a power loss, which every step survives), then closes the bus connection.
pub fn run(args: &DaemonArgs, hooks: Hooks<'_>) -> Result<(), DaemonError> {
    let config = load_config(&args.config)?;
    let _lock = lock_state_dir(&config.engine.state_dir)?;
    let boot = FileBootControl::open(&config.boot_dir, &config.roles)?;
    let cancel = Arc::new(CancelFlag::new());
    let env = Environment {
        clock: Arc::new(SystemClock),
        space: Arc::new(FsSpace),
        control: cancel.clone(),
    };
    let engine = Engine::open(config.engine.clone(), env, boot)?;
    let client = Client::connect(&args.socket, ClientOptions::named("ultar-updated"))?;
    if !register_stop(&hooks, &cancel, &client) {
        return Ok(());
    }
    let sources = Arc::new(ServerSources::new(
        config.server.clone(),
        config.token_file.clone(),
    ));
    let publisher = Arc::new(BusPublisher(client.clone()));
    let core = Arc::new(Core::new(
        engine,
        cancel.clone(),
        sources,
        publisher,
        config.access.clone(),
    ));
    let health = BusHealth::new(
        client.clone(),
        config.health_services.clone(),
        Duration::from_secs(5),
    );
    let patient = Patient {
        inner: &health,
        attempts: config.health_wait_seconds,
        stop: &cancel,
    };
    // A failure here is published as the service's state (error key) and does not stop the
    // service: status and offline repair must stay reachable.
    let outcome = core.resume_after_boot(&patient).ok();
    if cancel.is_stopping() {
        return Ok(());
    }
    let endpoint = client.register(crate::contract::SERVICE)?;
    (hooks.ready)(outcome);
    start_timer(core.clone(), u64::from(config.check_interval_minutes) * 60);
    serve(core, endpoint);
    Ok(())
}

/// The timer thread lives as long as the process. It wakes often and asks whether a run is
/// due; the due time is stored on disk, so short uptimes do not starve the check.
fn start_timer(core: Arc<Core<FileBootControl>>, interval: u64) {
    let _ = std::thread::Builder::new()
        .name("update-timer".into())
        .spawn(move || loop {
            let _ = core.tick_if_due(interval);
            std::thread::sleep(Duration::from_secs(interval.min(TIMER_WAKE)));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn command_line() {
        let args = parse_args(&words("--config /etc/u.conf --socket /run/bus"), None).unwrap();
        assert_eq!(args.config, PathBuf::from("/etc/u.conf"));
        assert_eq!(args.socket, PathBuf::from("/run/bus"));
        let env = parse_args(&words("--config c --lang de"), Some("/run/env".into())).unwrap();
        assert_eq!(env.socket, PathBuf::from("/run/env"));
        assert_eq!(env.log_socket, None);
        let logged = parse_args(&words("--config c --socket s --log-socket /run/log"), None);
        assert_eq!(logged.unwrap().log_socket, Some(PathBuf::from("/run/log")));
        for bad in [
            "",
            "--config c",
            "--socket s",
            "--config c --socket s --verbose x",
            "--config c --socket s extra",
            "--config",
        ] {
            assert!(
                matches!(parse_args(&words(bad), None), Err(DaemonError::Usage)),
                "{bad}"
            );
        }
    }

    #[test]
    fn config_loading_errors_are_typed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.conf");
        assert!(matches!(
            load_config(&path),
            Err(DaemonError::ConfigRead(_))
        ));
        std::fs::write(&path, "device=x\nwhat\n").unwrap();
        let error = load_config(&path).unwrap_err();
        assert_eq!(error.key(), "update.daemon.config");
        assert_eq!(error.detail().0, 2);
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(matches!(
            load_config(&path),
            Err(DaemonError::ConfigRead(_))
        ));
        assert_eq!(
            DaemonError::Update(UpdateError::BootControl).key(),
            "update.error.boot_control"
        );
        assert!(DaemonError::Bus(BusError::Timeout)
            .to_string()
            .starts_with("update.daemon.bus"));
    }

    #[test]
    fn sources_without_server_or_with_bad_address() {
        assert!(matches!(
            ServerSources::new(None, None).open(),
            Err(UpdateError::NoUpdate)
        ));
        assert!(matches!(
            ServerSources::new(Some("ftp://x".into()), None).open(),
            Err(UpdateError::InvalidArgument)
        ));
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("token");
        std::fs::write(&token, "secret-token\n").unwrap();
        assert!(
            ServerSources::new(Some("https://u.example".into()), Some(token))
                .open()
                .is_ok()
        );
    }

    struct After(Cell<u32>);
    impl HealthCheck for After {
        fn check(&self) -> Result<(), UpdateError> {
            if self.0.get() == 0 {
                Ok(())
            } else {
                self.0.set(self.0.get() - 1);
                Err(UpdateError::Unhealthy)
            }
        }
    }

    #[test]
    fn second_daemon_on_the_same_state_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let first = lock_state_dir(&state).unwrap();
        assert!(matches!(lock_state_dir(&state), Err(UpdateError::Busy)));
        drop(first);
        lock_state_dir(&state).unwrap();
    }

    #[test]
    fn a_stop_ends_the_health_wait_without_a_verdict() {
        let stop = CancelFlag::new();
        let never = After(Cell::new(u32::MAX));
        let patient = Patient {
            inner: &never,
            attempts: 3,
            stop: &stop,
        };
        // Without a stop the result is the verdict (here: unhealthy, after three tries) …
        stop.reset();
        let quick = Patient {
            attempts: 1,
            ..patient
        };
        assert!(matches!(quick.check(), Err(UpdateError::Unhealthy)));
        // … with a stop it is no verdict at all, and there is no more waiting.
        stop.stop();
        let started = std::time::Instant::now();
        assert!(matches!(patient.check(), Err(UpdateError::Interrupted)));
        assert!(started.elapsed() < Duration::from_secs(1));
        let healthy = After(Cell::new(0));
        let ok = Patient {
            inner: &healthy,
            attempts: 3,
            stop: &stop,
        };
        assert!(
            ok.check().is_ok(),
            "a service that answers is healthy even while stopping"
        );
    }

    #[test]
    fn health_is_waited_for_a_bounded_number_of_times() {
        let mut pauses = 0;
        wait_healthy(&After(Cell::new(3)), 10, &mut || pauses += 1).unwrap();
        assert_eq!(pauses, 3);
        let mut pauses = 0;
        assert!(wait_healthy(&After(Cell::new(9)), 4, &mut || pauses += 1).is_err());
        assert_eq!(pauses, 3);
        assert!(wait_healthy(&After(Cell::new(0)), 0, &mut || {}).is_ok());
    }
}
