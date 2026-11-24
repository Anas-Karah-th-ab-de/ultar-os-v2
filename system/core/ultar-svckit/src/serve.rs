//! Start and bus loop of a small service.
//!
//! [`main`] does what every `main` does (language, command line, hardening, kernel
//! rights, logger, readiness, exit codes); [`run`] connects with back-off, registers the
//! service name, subscribes, and then hands every call, every event and every tick to
//! the [`Service`] – on one thread, so a service needs no locks. Slow work (a dialog, a
//! scan) runs through [`Bus::later`] on a helper thread and comes back to the loop.

use crate::args::{parse, CommonArgs, SOCKET_ENV};
use crate::msg::{text, Lang, Msg};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::{BusError, Call, Client, IncomingRequest, RawEvent, Topic};
use ultar_service::harden::RightSet;
use ultar_service::log::{Level, LogTarget, Logger, LoggerConfig, Record};
use ultar_service::{
    exit, BusSession, Lifecycle, Runtime, SecretFilePolicy, ServiceSpec, StopCause,
};

/// Most helper threads of [`Bus::later`] at the same time.
pub const MAX_LATER: usize = 4;
/// Longest bus token read from a file.
const MAX_TOKEN: usize = 4096;
/// Stack of the helper threads.
const STACK: usize = 256 * 1024;
/// How long a stopping service waits for its helpers to answer their callers.
pub const STOP_GRACE: Duration = Duration::from_secs(2);

/// Work that comes back to the loop.
pub type Done<S> = Box<dyn for<'b> FnOnce(&mut S, &Bus<'b, S>) + Send>;

enum Input<S> {
    Request(Box<IncomingRequest>),
    Event(Box<RawEvent>),
    Done(Done<S>),
    Stop,
}

/// A small system service.
pub trait Service: Sized + Send + 'static {
    /// The service name it offers on the bus.
    fn name(&self) -> &'static str;
    /// Topics (or patterns) it follows.
    fn subscriptions(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// How often [`Service::tick`] runs (`None`: never).
    fn tick_every(&self) -> Option<Duration> {
        None
    }
    /// After start: publish the retained state.
    fn start(&mut self, bus: &Bus<'_, Self>);
    /// One call.
    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>);
    /// One event of a subscription.
    fn event(&mut self, _event: RawEvent, _bus: &Bus<'_, Self>) {}
    /// Regular work (polling hardware that gives no events).
    fn tick(&mut self, _bus: &Bus<'_, Self>) {}
}

/// What a service may do on the bus while it handles something.
pub struct Bus<'a, S> {
    session: &'a BusSession,
    logger: &'a Logger,
    tx: Sender<Input<S>>,
    busy: Arc<AtomicUsize>,
}

impl<S: Send + 'static> Bus<'_, S> {
    /// The client (for calls to other services).
    pub fn client(&self) -> &Client {
        self.session.client()
    }

    /// Publishes a retained topic (and publishes it again after a broker restart).
    pub fn retain<T: Topic>(&self, message: &T) {
        if self.session.publish_retained(message).is_err() {
            self.logger
                .emit(Record::new(Level::Warn, "svc.publish_failed").word("topic", T::TOPIC));
        }
    }

    /// Publishes an event topic.
    pub fn publish<T: Topic>(&self, message: &T) {
        if self.session.client().publish(message).is_err() {
            self.logger
                .emit(Record::new(Level::Warn, "svc.publish_failed").word("topic", T::TOPIC));
        }
    }

    /// Calls another service.
    pub fn call<C: Call>(&self, call: &C, timeout: Duration) -> Result<C::Reply, BusError> {
        self.session.client().call(call, timeout)
    }

    /// Writes one log record.
    pub fn log(&self, record: Record) {
        self.logger.emit(record);
    }

    /// Runs `work` on a helper thread with a clone of the client; what it returns runs
    /// on the loop afterwards. `false` (nothing started) when [`MAX_LATER`] helpers
    /// are busy – the caller answers "busy".
    pub fn later(&self, work: impl FnOnce(&Client) -> Done<S> + Send + 'static) -> bool {
        if self.busy.fetch_add(1, Ordering::SeqCst) >= MAX_LATER {
            self.busy.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        let client = self.session.client().clone();
        let tx = self.tx.clone();
        let busy = self.busy.clone();
        let started = std::thread::Builder::new()
            .stack_size(STACK)
            .spawn(move || {
                let done = work(&client);
                busy.fetch_sub(1, Ordering::SeqCst);
                let _ = tx.send(Input::Done(done));
            });
        if started.is_err() {
            self.busy.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }
}

/// Hands work to the service loop from a thread the service owns (a kernel
/// event source with its own wait, for example); the work runs on the loop
/// like the result of [`Bus::later`].
pub struct Poster<S> {
    tx: Sender<Input<S>>,
}

impl<S> Clone for Poster<S> {
    fn clone(&self) -> Self {
        Poster {
            tx: self.tx.clone(),
        }
    }
}

impl<S: Send + 'static> Poster<S> {
    /// Queues `work` for the loop; `false` when the loop has ended.
    pub fn post(&self, work: Done<S>) -> bool {
        self.tx.send(Input::Done(work)).is_ok()
    }
}

impl<S: Send + 'static> Bus<'_, S> {
    /// A [`Poster`] for this loop.
    pub fn poster(&self) -> Poster<S> {
        Poster {
            tx: self.tx.clone(),
        }
    }
}

fn feed<S: Send + 'static, T: Send + 'static>(
    tx: Sender<Input<S>>,
    mut next: impl FnMut() -> Result<T, BusError> + Send + 'static,
    wrap: fn(T) -> Input<S>,
) -> Result<(), BusError> {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || loop {
            match next() {
                Ok(item) => {
                    if tx.send(wrap(item)).is_err() {
                        return;
                    }
                }
                Err(BusError::Closed) => return,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        })
        .map(|_| ())
        .map_err(BusError::Io)
}

/// Connects, registers and runs `service` until the lifecycle stops it.
pub fn run<S: Service>(
    args: &CommonArgs,
    token: Option<ultar_service::Secret>,
    lifecycle: &Lifecycle,
    logger: &Logger,
    ready: &dyn Fn(),
    mut service: S,
) -> Result<(), BusError> {
    let mut options = ultar_service::SessionOptions::named(service.name());
    options.token = token;
    options.stop = Some(lifecycle.clone());
    let session = BusSession::connect(&args.bus, options)?;
    let endpoint = session.register(service.name())?;
    let (tx, rx) = channel::<Input<S>>();
    {
        let tx = tx.clone();
        lifecycle.on_stop(move || {
            let _ = tx.send(Input::Stop);
        });
    }
    feed(
        tx.clone(),
        move || endpoint.recv(),
        |r| Input::Request(Box::new(r)),
    )?;
    for pattern in service.subscriptions() {
        let sub = session.client().subscribe_pattern(pattern)?;
        feed(
            tx.clone(),
            move || sub.recv(),
            |e| Input::Event(Box::new(e)),
        )?;
    }
    let bus = Bus {
        session: &session,
        logger,
        tx,
        busy: Arc::new(AtomicUsize::new(0)),
    };
    service.start(&bus);
    ready();
    serve_loop(&mut service, &bus, &rx, logger);
    session.close();
    Ok(())
}

/// Hands the service every input until a stop.
fn serve_loop<S: Service>(
    service: &mut S,
    bus: &Bus<'_, S>,
    rx: &std::sync::mpsc::Receiver<Input<S>>,
    logger: &Logger,
) {
    logger.emit(Record::new(Level::Info, "svc.ready").word("service", service.name()));
    let every = service.tick_every();
    let mut next_tick = every.map(|d| Instant::now() + d);
    loop {
        let wait = next_tick.map_or(Duration::from_secs(3600), |t| {
            t.saturating_duration_since(Instant::now())
        });
        match rx.recv_timeout(wait) {
            Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => {
                // Work begun on helpers (an answer to a caller, a power off just
                // accepted by init) finishes before the connection closes.
                let deadline = Instant::now() + STOP_GRACE;
                while bus.busy.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
                    if let Ok(Input::Done(done)) = rx.recv_timeout(Duration::from_millis(20)) {
                        done(service, bus);
                    }
                }
                break;
            }
            Ok(Input::Request(r)) => service.request(*r, bus),
            Ok(Input::Event(e)) => service.event(*e, bus),
            Ok(Input::Done(done)) => done(service, bus),
            Err(RecvTimeoutError::Timeout) => {
                service.tick(bus);
                next_tick = every.map(|d| Instant::now() + d);
            }
        }
    }
}

/// The command line, or the exit code for help and refused options.
fn command_line(program: &str, lang: Lang, options: &[&str]) -> Result<CommonArgs, ExitCode> {
    match parse(
        std::env::args().skip(1),
        std::env::var(SOCKET_ENV).ok(),
        options,
    ) {
        Ok(Some(args)) => Ok(args),
        Ok(None) => {
            println!("{program}: {}", text(lang, Msg::Usage));
            Err(ExitCode::SUCCESS)
        }
        Err(_) => {
            eprintln!("{program}: {}", text(lang, Msg::BadArguments));
            Err(exit::code(exit::USAGE))
        }
    }
}

/// The bus token from `--token-file`, if one was given.
fn read_token(args: &CommonArgs) -> Result<Option<ultar_service::Secret>, ()> {
    match &args.token_file {
        Some(path) => ultar_service::read_secret_file_limited(
            path,
            &SecretFilePolicy::for_current_user(),
            MAX_TOKEN,
        )
        .map(Some)
        .map_err(|_| ()),
        None => Ok(None),
    }
}

/// Command line, hardening and runtime of a service program.
fn prepare(
    program: &'static str,
    options: &[&str],
    rights: Option<RightSet>,
) -> Result<(Lang, CommonArgs, Runtime), ExitCode> {
    let lang = Lang::from_tag(&std::env::var("LANG").unwrap_or_default());
    let args = command_line(program, lang, options)?;
    let mut spec = ServiceSpec::new(program);
    spec.hardening.allow_root = args.allow_root;
    spec.hardening.rights = Some(rights.unwrap_or_else(|| RightSet::of(&[])));
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    match Runtime::start(spec) {
        Ok(r) => Ok((lang, args, r)),
        Err(e) => {
            eprintln!("{program}: {}", text(lang, Msg::Hardening));
            Err(exit::code(e.exit_code()))
        }
    }
}

fn refused(program: &str, lang: Lang, msg: Msg) -> i32 {
    eprintln!("{program}: {}", text(lang, msg));
    if msg == Msg::BadArguments {
        exit::USAGE
    } else {
        exit::FAILURE
    }
}

/// The whole `main` of a small service: `options` are its own command line options,
/// `rights` the kernel rights it keeps (`None`: none), `build` makes the service from
/// the command line (an error ends the program with the usage code).
pub fn main<S: Service>(
    program: &'static str,
    options: &[&str],
    rights: Option<RightSet>,
    build: impl FnOnce(&CommonArgs, &Logger) -> Result<S, Msg> + Send,
) -> ExitCode {
    let (lang, args, runtime) = match prepare(program, options, rights) {
        Ok(p) => p,
        Err(code) => return code,
    };
    exit::code(runtime.run(|rt| {
        let Ok(token) = read_token(&args) else {
            return refused(program, lang, Msg::BadArguments);
        };
        let service = match build(&args, rt.logger()) {
            Ok(s) => s,
            Err(msg) => return refused(program, lang, msg),
        };
        match run(
            &args,
            token,
            rt.lifecycle(),
            rt.logger(),
            &|| rt.ready(),
            service,
        ) {
            Ok(()) => exit::OK,
            Err(_) if rt.lifecycle().is_stopping() => exit::OK,
            Err(_) => refused(program, lang, Msg::Bus),
        }
    }))
}

/// Like [`main`] for a program offering two services on the bus (each with its
/// own connection, same identity): `A` on the main thread (its start makes the
/// program ready), `B` on a second thread. When either ends with a bus error the
/// program stops (and is restarted by init).
pub fn main_pair<A: Service, B: Service>(
    program: &'static str,
    options: &[&str],
    rights: Option<RightSet>,
    build: impl FnOnce(&CommonArgs, &Logger) -> Result<(A, B), Msg> + Send,
) -> ExitCode {
    let (lang, args, runtime) = match prepare(program, options, rights) {
        Ok(p) => p,
        Err(code) => return code,
    };
    exit::code(runtime.run(|rt| {
        let (Ok(token_a), Ok(token_b)) = (read_token(&args), read_token(&args)) else {
            return refused(program, lang, Msg::BadArguments);
        };
        let (a, b) = match build(&args, rt.logger()) {
            Ok(pair) => pair,
            Err(msg) => return refused(program, lang, msg),
        };
        let (args_b, life, log) = (args.clone(), rt.lifecycle().clone(), rt.logger().clone());
        let second = std::thread::Builder::new()
            .stack_size(STACK * 4)
            .spawn(move || {
                let r = run(&args_b, token_b, &life, &log, &|| {}, b);
                if r.is_err() && !life.is_stopping() {
                    life.request_stop(StopCause::Fault);
                    return false;
                }
                true
            });
        let Ok(second) = second else {
            return refused(program, lang, Msg::Bus);
        };
        let first = run(
            &args,
            token_a,
            rt.lifecycle(),
            rt.logger(),
            &|| rt.ready(),
            a,
        );
        let second_ok = second.join().unwrap_or(false);
        match first {
            Ok(()) if second_ok => exit::OK,
            Err(_) if second_ok && rt.lifecycle().is_stopping() => exit::OK,
            _ => refused(program, lang, Msg::Bus),
        }
    }))
}
