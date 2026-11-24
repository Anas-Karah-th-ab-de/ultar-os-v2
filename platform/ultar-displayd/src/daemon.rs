//! Command line and the service loop: one thread owns the state; the
//! compositor link, the bus endpoint and the three settings topics feed it
//! through one channel. Nothing runs on a timer while nothing happens.

use crate::core::{Core, Note, Ports};
use crate::link::{self, LinkEvent, LinkHandle};
use crate::ports::{BusSettings, CompositorPort, Publisher};
use crate::service;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use ultar_bus::messages::display_service::{
    code, Apply, Detect, DetectReply, Identify, Keep, Outputs, Revert, SERVICE,
};
use ultar_bus::messages::settings::{Changed, PendingConfirmation, PendingResolved};
use ultar_bus::{BusError, Call, ErrorCode, IncomingRequest, Topic};
use ultar_service::log::{Level, Logger, Record};
use ultar_service::{BusSession, Lifecycle, Secret, SessionOptions};
use ultar_surface::proto::{Event, OutputId, Request, SetupOutcome, SetupRefusal};

/// Environment variable with the bus socket.
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";
/// How long `display.detect` waits for the compositor.
pub const DETECT_WAIT_MS: u64 = 3_000;
/// Calls of `display.detect` waiting at once.
const MAX_DETECTS: usize = 8;

/// What the loop reacts to.
pub enum Input {
    /// The compositor link.
    Link(LinkEvent),
    /// `settings.changed`.
    Changed(Changed),
    /// `settings.pending`.
    Pending(PendingConfirmation),
    /// `settings.pending_resolved`.
    Resolved(PendingResolved),
    /// Events were lost: read the settings again.
    Resync,
    /// A call of `display.*`.
    Request(Box<IncomingRequest>),
    /// The service stops.
    Stop,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Input::Request(r) => write!(f, "Request({})", r.method()),
            Input::Link(e) => write!(f, "Link({e:?})"),
            Input::Stop => f.write_str("Stop"),
            Input::Resync => f.write_str("Resync"),
            Input::Changed(c) => write!(f, "Changed({})", c.key),
            Input::Pending(p) => write!(f, "Pending({})", p.key),
            Input::Resolved(r) => write!(f, "Resolved({})", r.key),
        }
    }
}

impl From<LinkEvent> for Input {
    fn from(e: LinkEvent) -> Self {
        Input::Link(e)
    }
}

/// Command line of `ultar-displayd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// Bus socket.
    pub bus: PathBuf,
    /// Compositor socket.
    pub compositor: PathBuf,
    /// Log service socket.
    pub log_socket: Option<PathBuf>,
    /// Bus token file.
    pub token_file: Option<PathBuf>,
    /// Start even as root (development).
    pub allow_root: bool,
}

/// Why the command line was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageError;

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid options")
    }
}

impl std::error::Error for UsageError {}

/// Reads the command line (`None`: help was asked for).
pub fn parse(
    words: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
) -> Result<Option<Args>, UsageError> {
    let mut it = words.into_iter();
    let (mut bus, mut compositor, mut log_socket, mut token_file) = (
        env_socket.filter(|s| !s.is_empty()).map(PathBuf::from),
        None,
        None,
        None,
    );
    let mut allow_root = false;
    while let Some(word) = it.next() {
        let mut value = || it.next().map(PathBuf::from).ok_or(UsageError);
        match word.as_str() {
            "--help" | "-h" => return Ok(None),
            "--bus" => bus = Some(value()?),
            "--compositor" => compositor = Some(value()?),
            "--log-socket" => log_socket = Some(value()?),
            "--token-file" => token_file = Some(value()?),
            "--allow-root" => allow_root = true,
            _ => return Err(UsageError),
        }
    }
    let absolute = |p: &PathBuf| p.is_absolute();
    let (Some(bus), Some(compositor)) = (bus, compositor) else {
        return Err(UsageError);
    };
    let all = [
        Some(&bus),
        Some(&compositor),
        log_socket.as_ref(),
        token_file.as_ref(),
    ];
    if !all.iter().flatten().all(|p| absolute(p)) {
        return Err(UsageError);
    }
    Ok(Some(Args {
        bus,
        compositor,
        log_socket,
        token_file,
        allow_root,
    }))
}

struct BusPublisher<'a>(&'a BusSession);

impl Publisher for BusPublisher<'_> {
    fn outputs(&self, outputs: &Outputs) {
        let _ = self.0.publish_retained(outputs);
    }
}

fn forward<T: Topic + Send + 'static>(
    client: &ultar_bus::Client,
    tx: &Sender<Input>,
    wrap: fn(T) -> Input,
) -> Result<(), BusError> {
    let sub = client.subscribe::<T>()?;
    let tx = tx.clone();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || loop {
            let input = match sub.recv() {
                Ok(m) => wrap(m),
                Err(BusError::Lagged(_)) => Input::Resync,
                Err(BusError::Closed) => return,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            if tx.send(input).is_err() {
                return;
            }
        })
        .map_err(BusError::Io)?;
    Ok(())
}

fn serve_endpoint(endpoint: ultar_bus::ServiceEndpoint, tx: Sender<Input>) -> Result<(), BusError> {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || loop {
            match endpoint.recv() {
                Ok(r) => {
                    if tx.send(Input::Request(Box::new(r))).is_err() {
                        return;
                    }
                }
                Err(BusError::Closed) => return,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        })
        .map_err(BusError::Io)?;
    Ok(())
}

/// Runs the service until the lifecycle stops it.
pub fn run(
    args: &Args,
    token: Option<Secret>,
    lifecycle: &Lifecycle,
    logger: &Logger,
    ready: &dyn Fn(),
) -> Result<(), BusError> {
    let mut options = SessionOptions::named("ultar-displayd");
    options.token = token;
    options.stop = Some(lifecycle.clone());
    let session = BusSession::connect(&args.bus, options)?;
    let (tx, rx) = channel();
    forward::<Changed>(session.client(), &tx, Input::Changed)?;
    forward::<PendingConfirmation>(session.client(), &tx, Input::Pending)?;
    forward::<PendingResolved>(session.client(), &tx, Input::Resolved)?;
    serve_endpoint(session.register(SERVICE)?, tx.clone())?;
    let (link, join) = link::start(args.compositor.clone(), tx.clone()).map_err(BusError::Io)?;
    {
        let (tx, link) = (tx.clone(), link.clone());
        lifecycle.on_stop(move || {
            link.stop();
            let _ = tx.send(Input::Stop);
        });
    }
    ready();
    logger.log(Level::Info, "displayd.ready");
    let settings = BusSettings::new(session.client().clone());
    let mut lp = Loop::new(&link, logger);
    lp.run(&rx, &settings, &BusPublisher(&session));
    link.stop();
    let _ = join.join();
    session.close();
    Ok(())
}

struct Detecting {
    request: IncomingRequest,
    known: BTreeSet<OutputId>,
    until: u64,
}

struct Loop<'a> {
    core: Core,
    link: &'a LinkHandle,
    logger: &'a Logger,
    started: Instant,
    detecting: Vec<Detecting>,
}

impl<'a> Loop<'a> {
    fn new(link: &'a LinkHandle, logger: &'a Logger) -> Self {
        Loop {
            core: Core::new(),
            link,
            logger,
            started: Instant::now(),
            detecting: Vec::new(),
        }
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn run(&mut self, rx: &Receiver<Input>, settings: &BusSettings, publish: &dyn Publisher) {
        loop {
            let now = self.now();
            let due = [
                self.core.next_deadline(),
                self.detecting.iter().map(|d| d.until).min(),
            ]
            .into_iter()
            .flatten()
            .min();
            let first = match due {
                Some(t) => rx.recv_timeout(Duration::from_millis(t.saturating_sub(now))),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            let ports = Ports {
                settings,
                compositor: self.link,
                publish,
            };
            let mut batch = Vec::new();
            match first {
                Ok(input) => batch.push(input),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            batch.extend(rx.try_iter());
            for input in batch {
                if !self.handle(input, &ports) {
                    return;
                }
            }
            self.core.settle(self.now(), &ports);
            self.expire_detects();
            self.log_notes();
        }
    }

    fn handle(&mut self, input: Input, ports: &Ports<'_>) -> bool {
        let now = self.now();
        match input {
            Input::Stop => return false,
            Input::Link(LinkEvent::Connected) => {}
            Input::Link(LinkEvent::Gone) => self.core.on_compositor_gone(),
            Input::Link(LinkEvent::Event(e)) => self.compositor_event(e, ports),
            Input::Changed(c) => self.core.on_announced(&c.key, c.value, now),
            Input::Pending(p) => self.core.on_pending(&p.key, p.seconds, now),
            Input::Resolved(r) => {
                self.core.on_changed(&r.key, r.value);
                self.core.on_resolved(&r.key);
            }
            Input::Resync => self.core.resync(),
            Input::Request(r) => self.request(*r, ports),
        }
        true
    }

    fn compositor_event(&mut self, e: Event, ports: &Ports<'_>) {
        match e {
            Event::OutputDetails(d) => self.core.on_details(*d),
            Event::OutputRemoved { output } => self.core.on_removed(output),
            Event::OutputSetupDone { serial, outcome } => {
                self.core.on_setup_done(serial, outcome, ports)
            }
            Event::OutputsDetected { count } => self.answer_detects(count),
            _ => {}
        }
    }

    fn request(&mut self, r: IncomingRequest, ports: &Ports<'_>) {
        let core = &mut self.core;
        let fail = |code: u32| ErrorCode::Service(code);
        let _ = match r.method() {
            m if m == Identify::METHOD => {
                r.respond::<Identify>(|c| service::identify(core, ports, &c).map_err(fail))
            }
            m if m == Apply::METHOD => {
                r.respond::<Apply>(|c| service::apply(core, ports, &c).map_err(fail))
            }
            m if m == Keep::METHOD => {
                r.respond::<Keep>(|c| service::keep(core, ports, c).map_err(fail))
            }
            m if m == Revert::METHOD => {
                r.respond::<Revert>(|_| service::revert(core, ports).map_err(fail))
            }
            m if m == Detect::METHOD => {
                self.detect(r);
                Ok(())
            }
            _ => r.fail(ErrorCode::UnknownMethod),
        };
    }

    fn detect(&mut self, r: IncomingRequest) {
        if r.decode::<Detect>().is_err() {
            let _ = r.fail(ErrorCode::BadPayload);
            return;
        }
        if self.detecting.len() >= MAX_DETECTS || !self.link.send(Request::DetectOutputs) {
            let _ = r.fail(ErrorCode::Service(code::NO_COMPOSITOR));
            return;
        }
        let known = self.core.connected().iter().map(|c| c.details.id).collect();
        let until = self.now() + DETECT_WAIT_MS;
        self.detecting.push(Detecting {
            request: r,
            known,
            until,
        });
    }

    fn answer_detects(&mut self, found: u32) {
        let now_ids: BTreeSet<OutputId> =
            self.core.connected().iter().map(|c| c.details.id).collect();
        for d in self.detecting.drain(..) {
            let new = now_ids.difference(&d.known).count() as u32;
            let _ = d.request.reply(&DetectReply { found, new });
        }
    }

    fn expire_detects(&mut self) {
        let now = self.now();
        let (late, keep): (Vec<_>, Vec<_>) = self.detecting.drain(..).partition(|d| d.until <= now);
        self.detecting = keep;
        for d in late {
            let _ = d.request.fail(ErrorCode::Service(code::NO_COMPOSITOR));
        }
    }

    fn log_notes(&mut self) {
        for note in self.core.take_notes() {
            let record = match note {
                Note::SetupRefused(o) => {
                    Record::new(Level::Warn, "displayd.setup_refused").word("why", outcome_word(o))
                }
                Note::CompositorReverted(o) => {
                    Record::new(Level::Notice, "displayd.compositor_reverted")
                        .word("why", outcome_word(o))
                }
                Note::Upright => Record::new(Level::Notice, "displayd.orientation_unsupported"),
                Note::RevertedAtStart(n) => {
                    Record::new(Level::Notice, "displayd.reverted_at_start")
                        .count("keys", u64::from(n))
                }
                Note::SettingsUnavailable => {
                    Record::new(Level::Warn, "displayd.settings_unavailable")
                }
            };
            self.logger.emit(record);
        }
    }
}

/// A fixed word for each setup outcome (log records carry no free text).
pub fn outcome_word(o: SetupOutcome) -> &'static str {
    match o {
        SetupOutcome::Applied => "applied",
        SetupOutcome::Confirmed => "confirmed",
        SetupOutcome::RevertedTimeout => "timeout",
        SetupOutcome::RevertedHotplug => "hotplug",
        SetupOutcome::Refused(SetupRefusal::UnknownOutput) => "unknown_output",
        SetupOutcome::Refused(SetupRefusal::UnknownMode) => "unknown_mode",
        SetupOutcome::Refused(SetupRefusal::Invalid) => "invalid",
        SetupOutcome::Refused(SetupRefusal::NotSupported) => "not_supported",
        SetupOutcome::Refused(SetupRefusal::Busy) => "busy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_line() {
        let a = parse(
            words(&["--compositor", "/run/c.sock", "--log-socket", "/l"]),
            Some("/run/b".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(a.bus, PathBuf::from("/run/b"));
        assert!(!a.allow_root);
        assert_eq!(parse(words(&["-h"]), None), Ok(None));
        for bad in [
            &["--compositor", "/c"][..],
            &["--bus", "/b"],
            &["--bus", "b", "--compositor", "/c"],
            &["--bus", "/b", "--compositor", "/c", "--x"],
            &["--bus"],
        ] {
            assert_eq!(parse(words(bad), None), Err(UsageError), "{bad:?}");
        }
        assert!(!UsageError.to_string().is_empty());
        assert_eq!(
            outcome_word(SetupOutcome::Refused(SetupRefusal::Busy)),
            "busy"
        );
    }
}
