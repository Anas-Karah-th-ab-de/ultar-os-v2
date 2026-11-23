//! `ultar-logd` – the log service of Ultar OS.
//!
//! See [`ultar_logd::msg::LOGD_SYNOPSIS`] for the options.
//!
//! * `--listen PATH`: services connect here; the source name is the
//!   identity their **kernel credentials** map to in `--uid-map` (the same
//!   file the bus uses), so no service can write under another's name.
//! * `--listen PATH=NAME`: every peer of this socket is `NAME` (for
//!   kernels without peer credentials: one socket per service, reachable
//!   by that service only).
//! * `--bus PATH`: offer the reading service `log` on the bus. The policy
//!   decides who has `log.read_all`; everybody else reads only records
//!   written under the own identity. The service does not wait for the
//!   bus: ingestion starts at once, the bus is joined when it appears.
//! * `--chain-key-file FILE`: secret key that authenticates the hash
//!   chain (owner and mode of the file are checked). `--chain-key-fd N`
//!   reads it from an inherited descriptor instead (a pipe or file the
//!   service manager opened; for kernels whose file owners and modes
//!   cannot be checked). `--bus-token-file`/`--bus-token-fd` likewise.
//!
//! `SIGTERM`/`SIGINT`: stop accepting, write everything out, sync, remove
//! the sockets, exit 0. `SIGHUP`: read the kernel log now and read the
//! policy again (a policy that does not load keeps the old one and is
//! logged as `log.policy_reload_failed`).
//!
//! One line is written to standard output when the service takes records.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::process::ExitCode;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;
use ultar_bus::messages::log::SERVICE;
use ultar_bus::Policy;
use ultar_logd::chain::Chain;
use ultar_logd::clock::{Clock, SystemClock};
use ultar_logd::core::Core;
use ultar_logd::daemon::{parse, ArgError, DaemonArgs, DaemonCommand};
use ultar_logd::ingest::{Ingest, IngestLimits, ListenerSpec, SourceNames};
use ultar_logd::msg::{language, text, Lang, Msg, LOGD_SYNOPSIS};
use ultar_logd::service::{Access, AllowAll, Engine, Input, InputSink, PolicyAccess};
use ultar_logd::storage::{DirStorage, MemStorage, Storage};
use ultar_service::log::Level;
use ultar_service::{
    exit, peercred, read_secret_file, BusSession, HardeningError, Lifecycle, Limits, Runtime,
    Secret, SecretFilePolicy, SecretSource, ServiceSpec, SessionOptions, StartError, UidMap, Wake,
};

/// Records and requests that may wait for the engine thread.
/// (128 connections + rejections) × 32 records per share, plus room for
/// the rest.
const QUEUE: usize = 129 * 32 + RESERVE;
/// Room in the queue beyond the ingestion shares: bus requests and the
/// daemon's own inputs.
const RESERVE: usize = 256;
// Every source's share of the queue (and the share of rejections) fits at
// the same time, so a flooding source never takes the room of another one.
const _: () = assert!(IngestLimits::STANDARD.fits(QUEUE - RESERVE));
/// Pause before the bus is tried again after it went away for good.
const BUS_RETRY: Duration = Duration::from_millis(500);
/// Largest chain key or bus token read from a file or descriptor.
const MAX_SECRET: usize = 4096;
/// Limits of the log service: more descriptors than the standard daemon
/// (128 connections), fewer threads are not possible (one per connection).
/// No address-space limit: with one thread per connection the C library's
/// per-thread memory pools reserve address space far beyond what is used,
/// and a refused reservation aborts the process (review round 3, R3-B0);
/// the memory bound is the group's `memory.max`.
const LIMITS: Limits = Limits {
    address_space: None,
    open_files: Some(256),
    processes: Some(256),
    core_dumps: false,
};

fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

fn say_detail(lang: Lang, msg: Msg, detail: &dyn std::fmt::Display) {
    say(&format!("{} {detail}", text(lang, msg)));
}

fn usage_error(lang: Lang, error: ArgError) -> ExitCode {
    say(text(
        lang,
        match error {
            ArgError::NoPolicy => Msg::PolicyRequired,
            ArgError::PolicyConflict => Msg::PolicyConflict,
            ArgError::Limits => Msg::LimitsInvalid,
            ArgError::Arguments | ArgError::NoUidMap => Msg::BadArguments,
        },
    ));
    say(&format!("{}\n  {LOGD_SYNOPSIS}", text(lang, Msg::Usage)));
    exit::code(exit::USAGE)
}

fn start_runtime(args: &DaemonArgs, lang: Lang) -> Result<Runtime, ExitCode> {
    let mut spec = ServiceSpec::new("ultar-logd");
    spec.hardening.allow_root = args.allow_root;
    spec.hardening.limits = Some(LIMITS);
    for source in [&args.chain_key, &args.bus_token].into_iter().flatten() {
        if let SecretSource::Fd(fd) = source {
            spec.inherited.push(*fd);
        }
    }
    Runtime::start(spec).map_err(|error| {
        match error {
            StartError::Hardening(HardeningError::RunningAsRoot) => {
                say(text(lang, Msg::RootRefused))
            }
            other => say_detail(lang, Msg::HardeningFailed, &other),
        }
        exit::code(error.exit_code())
    })
}

/// What has to be loaded before anything is opened. `None`: a message was
/// printed and the start is over.
struct Loaded {
    chain: Chain,
    access: Box<dyn Access>,
    listeners: Vec<ListenerSpec>,
    /// Bus token read once from an inherited descriptor.
    bus_token: Option<Secret>,
}

/// Reads a secret from its source; `Err(())`: a message was printed.
fn secret(runtime: &Runtime, source: &SecretSource, lang: Lang) -> Result<Secret, ()> {
    runtime
        .read_secret(source, MAX_SECRET)
        .map_err(|error| say_detail(lang, Msg::SecretRejected, &error))
}

/// The policy's view of who reads what (everything without a policy).
fn load_access(
    policy: Option<&std::path::Path>,
) -> Result<Box<dyn Access>, ultar_bus::PolicyError> {
    match policy {
        None => Ok(Box::new(AllowAll)),
        Some(path) => Ok(Box::new(PolicyAccess(Policy::load(path)?))),
    }
}

fn load(args: &DaemonArgs, lang: Lang, runtime: &Runtime) -> Option<Loaded> {
    let chain = match &args.chain_key {
        None => Chain::unkeyed(),
        Some(source) => Chain::keyed(secret(runtime, source, lang).ok()?.expose()),
    };
    let bus_token = match &args.bus_token {
        Some(source @ SecretSource::Fd(_)) => Some(secret(runtime, source, lang).ok()?),
        _ => None,
    };
    let access = match load_access(args.policy.as_deref()) {
        Ok(access) => access,
        Err(error) => {
            say_detail(lang, Msg::PolicyRejected, &error);
            return None;
        }
    };
    let map = match &args.uid_map {
        Some(path) if args.listeners.iter().any(|l| l.name.is_none() && !l.output) => {
            if !peercred::probe() {
                say(text(lang, Msg::CredentialsUnsupported));
                return None;
            }
            match UidMap::load(path) {
                Ok(map) => Some(Arc::new(map)),
                Err(error) => {
                    say_detail(lang, Msg::UidMapRejected, &error);
                    return None;
                }
            }
        }
        _ => None,
    };
    let mut listeners = Vec::new();
    for listener in &args.listeners {
        let names = match (&listener.name, &map) {
            _ if listener.output => SourceNames::Output,
            (Some(name), _) => SourceNames::Fixed(name.clone()),
            (None, Some(map)) => SourceNames::Credentials(map.clone()),
            (None, None) => return None,
        };
        listeners.push(ListenerSpec {
            path: listener.path.clone(),
            names,
            mode: listener.mode,
            group: listener.group,
        });
    }
    Some(Loaded {
        chain,
        access,
        listeners,
        bus_token,
    })
}

/// Opens the directory; a directory that cannot be used leaves the
/// service running on memory alone (a disk of zero bytes).
fn open_storage(args: &DaemonArgs, lang: Lang) -> (Box<dyn Storage>, bool) {
    match DirStorage::open(&args.dir) {
        Ok(storage) => (Box::new(storage), true),
        Err(_) => {
            say(text(lang, Msg::DirectoryUnusable));
            (Box::new(MemStorage::with_capacity(0)), false)
        }
    }
}

/// The bus connection currently in use, so that a stop can close it and
/// wake the thread that serves it.
type CurrentBus = Arc<Mutex<Option<ultar_bus::Client>>>;

fn set_current(current: &CurrentBus, client: Option<ultar_bus::Client>) {
    *current.lock().unwrap_or_else(PoisonError::into_inner) = client;
}

/// Serves the bus service `log` for as long as the daemon runs: joins the
/// bus when it appears and again whenever the connection is lost for good.
/// The token for the next connection attempt: a token file is read again
/// each time (it may have been replaced), a descriptor's token is kept.
fn bus_token(args: &DaemonArgs, kept: Option<&Secret>) -> Option<Secret> {
    match &args.bus_token {
        Some(SecretSource::File(path)) => {
            read_secret_file(path, &SecretFilePolicy::for_current_user()).ok()
        }
        _ => kept.map(Secret::duplicate),
    }
}

fn bus_loop(
    args: &DaemonArgs,
    token: Option<Secret>,
    inputs: &SyncSender<Input>,
    lifecycle: &Lifecycle,
) {
    let Some(socket) = &args.bus else {
        return;
    };
    let current: CurrentBus = Arc::new(Mutex::new(None));
    let closer = current.clone();
    lifecycle.on_stop(move || {
        if let Some(client) = closer.lock().unwrap_or_else(PoisonError::into_inner).take() {
            client.close();
        }
    });
    while !lifecycle.is_stopping() {
        let mut options = SessionOptions::named("ultar-logd");
        options.connect_wait = BUS_RETRY;
        options.token = bus_token(args, token.as_ref());
        // A stop ends the attempts at once (review round 3, R3-S1).
        options.stop = Some(lifecycle.clone());
        let Ok(session) = BusSession::connect(socket, options) else {
            // The connection attempt itself waited (or the service stops).
            continue;
        };
        set_current(&current, Some(session.client().clone()));
        if lifecycle.is_stopping() {
            // The stop hook may have run before the client was stored.
            session.close();
        }
        match session.register(SERVICE) {
            Ok(endpoint) => {
                let _ = inputs.send(Input::Internal(Level::Info, "log.bus_joined"));
                endpoint.run(|request| {
                    let _ = inputs.send(Input::Request(Box::new(request)));
                });
            }
            Err(_) => {
                let _ = inputs.send(Input::Internal(Level::Error, "log.bus_register_refused"));
                lifecycle.wait_for_timeout(ultar_service::State::Stopping, BUS_RETRY);
            }
        }
        set_current(&current, None);
    }
}

/// The engine thread and the queue that feeds it.
struct Running {
    inputs: SyncSender<Input>,
    engine: thread::JoinHandle<Core>,
}

impl Running {
    /// Lets the engine write everything out and waits for it.
    fn stop(self) {
        let _ = self.inputs.send(Input::Stop);
        let _ = self.engine.join();
    }
}

/// What the start hands on besides the engine.
struct Rest {
    listeners: Vec<ListenerSpec>,
    bus_token: Option<Secret>,
}

fn start_engine(args: &DaemonArgs, loaded: Loaded, lang: Lang) -> Option<(Running, Rest)> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let (storage, on_disk) = open_storage(args, lang);
    let mut core = Core::open(args.config, storage, loaded.chain, clock.clone());
    if !on_disk {
        core.internal(Level::Error, "log.directory_unusable", Vec::new());
    }
    core.internal(Level::Info, "log.started", Vec::new());
    let (inputs, queue) = sync_channel(QUEUE);
    let engine = Engine::new(core, loaded.access, clock, args.kernel_log.clone());
    let engine = thread::Builder::new()
        .name("ultar-logd".into())
        .spawn(move || engine.run(&queue))
        .ok()?;
    let rest = Rest {
        listeners: loaded.listeners,
        bus_token: loaded.bus_token,
    };
    Some((Running { inputs, engine }, rest))
}

/// Reads the policy again and hands it to the engine; a policy that does
/// not load keeps the old one (review round 3, R3-P1).
fn reload_policy(args: &DaemonArgs, inputs: &SyncSender<Input>) {
    if args.policy.is_none() {
        return;
    }
    let input = match load_access(args.policy.as_deref()) {
        Ok(access) => Input::Access(access),
        Err(_) => Input::Internal(Level::Error, "log.policy_reload_failed"),
    };
    let _ = inputs.send(input);
}

fn run(args: DaemonArgs, lang: Lang, runtime: &Runtime) -> i32 {
    let Some(loaded) = load(&args, lang, runtime) else {
        return exit::FAILURE;
    };
    let Some((running, rest)) = start_engine(&args, loaded, lang) else {
        return exit::FAILURE;
    };
    let sink = Arc::new(InputSink(running.inputs.clone()));
    let limits = IngestLimits {
        rate: args.config.rate,
        ..IngestLimits::STANDARD
    };
    let ingest = match Ingest::start(&rest.listeners, limits, sink) {
        Ok(ingest) => ingest,
        Err(error) => {
            say_detail(lang, Msg::SocketFailed, &error);
            running.stop();
            return exit::FAILURE;
        }
    };
    let bus_thread = {
        let (args, inputs) = (args.clone(), running.inputs.clone());
        let (lifecycle, token) = (runtime.lifecycle().clone(), rest.bus_token);
        thread::Builder::new()
            .name("ultar-logd-bus".into())
            .spawn(move || bus_loop(&args, token, &inputs, &lifecycle))
    };
    runtime.ready();
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", text(lang, Msg::Ready));
    let _ = out.flush();
    let mut reloads = 0;
    while let Wake::Reload(count) = runtime.lifecycle().wait_change(reloads) {
        reloads = count;
        let _ = running.inputs.send(Input::ReadKernel);
        reload_policy(&args, &running.inputs);
    }
    // Stop accepting and remove the sockets, leave the bus, then let the
    // engine write everything out.
    drop(ingest);
    if let Ok(thread) = bus_thread {
        let _ = thread.join();
    }
    running.stop();
    say(text(lang, Msg::Stopped));
    exit::OK
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let lang = language(&words);
    let args = match parse(words) {
        Ok(DaemonCommand::Run(args)) => *args,
        Ok(DaemonCommand::Help) => {
            say(&format!("{}\n  {LOGD_SYNOPSIS}", text(lang, Msg::Usage)));
            return ExitCode::SUCCESS;
        }
        Err(error) => return usage_error(lang, error),
    };
    // Before anything is opened and before the first thread exists.
    let runtime = match start_runtime(&args, lang) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    exit::code(runtime.run(|runtime| run(args, lang, runtime)))
}
