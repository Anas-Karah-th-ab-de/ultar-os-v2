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
//!   chain (owner and mode of the file are checked).
//!
//! `SIGTERM`/`SIGINT`: stop accepting, write everything out, sync, remove
//! the sockets, exit 0. `SIGHUP`: read the kernel log now.
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
    SecretFilePolicy, ServiceSpec, SessionOptions, StartError, UidMap, Wake,
};

/// Records and requests that may wait for the engine thread.
const QUEUE: usize = 4096;
/// Pause before the bus is tried again after it went away for good.
const BUS_RETRY: Duration = Duration::from_millis(500);
/// Limits of the log service: more descriptors than the standard daemon
/// (128 connections), fewer threads are not possible (one per connection).
const LIMITS: Limits = Limits {
    address_space: Some(512 << 20),
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
}

fn load(args: &DaemonArgs, lang: Lang) -> Option<Loaded> {
    let chain = match &args.chain_key_file {
        None => Chain::unkeyed(),
        Some(path) => match read_secret_file(path, &SecretFilePolicy::for_current_user()) {
            Ok(key) => Chain::keyed(key.expose()),
            Err(error) => {
                say_detail(lang, Msg::SecretRejected, &error);
                return None;
            }
        },
    };
    let access: Box<dyn Access> = match &args.policy {
        None => Box::new(AllowAll),
        Some(path) => match Policy::load(path) {
            Ok(policy) => Box::new(PolicyAccess(policy)),
            Err(error) => {
                say_detail(lang, Msg::PolicyRejected, &error);
                return None;
            }
        },
    };
    let map = match &args.uid_map {
        Some(path) if args.listeners.iter().any(|l| l.name.is_none()) => {
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
            (Some(name), _) => SourceNames::Fixed(name.clone()),
            (None, Some(map)) => SourceNames::Credentials(map.clone()),
            (None, None) => return None,
        };
        listeners.push(ListenerSpec {
            path: listener.path.clone(),
            names,
            mode: listener.mode,
        });
    }
    Some(Loaded {
        chain,
        access,
        listeners,
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
fn bus_loop(args: &DaemonArgs, inputs: &SyncSender<Input>, lifecycle: &Lifecycle) {
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
        if let Some(path) = &args.bus_token_file {
            options.token = read_secret_file(path, &SecretFilePolicy::for_current_user()).ok();
        }
        let Ok(session) = BusSession::connect(socket, options) else {
            // The connection attempt itself waited; try again.
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

fn start_engine(
    args: &DaemonArgs,
    loaded: Loaded,
    lang: Lang,
) -> Option<(Running, Vec<ListenerSpec>)> {
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
    Some((Running { inputs, engine }, loaded.listeners))
}

fn run(args: DaemonArgs, lang: Lang, runtime: &Runtime) -> i32 {
    let Some(loaded) = load(&args, lang) else {
        return exit::FAILURE;
    };
    let Some((running, listeners)) = start_engine(&args, loaded, lang) else {
        return exit::FAILURE;
    };
    let sink = Arc::new(InputSink(running.inputs.clone()));
    let limits = IngestLimits {
        rate: args.config.rate,
        ..IngestLimits::STANDARD
    };
    let ingest = match Ingest::start(&listeners, limits, sink) {
        Ok(ingest) => ingest,
        Err(error) => {
            say_detail(lang, Msg::SocketFailed, &error);
            running.stop();
            return exit::FAILURE;
        }
    };
    let bus_thread = {
        let (args, inputs) = (args.clone(), running.inputs.clone());
        let lifecycle = runtime.lifecycle().clone();
        thread::Builder::new()
            .name("ultar-logd-bus".into())
            .spawn(move || bus_loop(&args, &inputs, &lifecycle))
    };
    runtime.ready();
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", text(lang, Msg::Ready));
    let _ = out.flush();
    let mut reloads = 0;
    while let Wake::Reload(count) = runtime.lifecycle().wait_change(reloads) {
        reloads = count;
        let _ = running.inputs.send(Input::ReadKernel);
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
