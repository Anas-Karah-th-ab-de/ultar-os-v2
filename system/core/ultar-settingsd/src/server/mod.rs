//! The bus service: connects the core to `ultar-bus`.
//!
//! One thread owns the [`Core`] and handles everything in arrival order:
//! requests to the `settings` service, capability reports of the device
//! service and deadlines (write delay, confirm-or-revert). Without work the
//! thread blocks; there is no polling.

use crate::access::{Access, Caller};
use crate::clock::Clock;
use crate::core::{Core, Event};
use crate::error::SettingsError;
use crate::store::Notice;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use ultar_bus::messages::devices;
use ultar_bus::messages::settings::{
    Changed, Confirm, Get, GetReply, ListReply, ListRequest, PendingConfirmation, PendingReply,
    PendingResolved, Reset, ResetReply, Revert, Set, SetDisplays, SetReply, Shutdown, StoreReport,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

mod convert;
mod stats;

pub use convert::{capabilities_from_bus, store_notice};
pub use stats::ServerStats;

/// Requests and reports that may wait for the service thread.
const QUEUE: usize = 256;

/// Called with failures nobody is waiting for (a write that failed after
/// the request was answered, an applier that failed on a timed-out revert).
pub type ErrorSink = Box<dyn Fn(&SettingsError) + Send>;

enum Input {
    Request(Box<IncomingRequest>),
    Capabilities(devices::Capabilities),
    Tick(SyncSender<()>),
    Stop,
}

/// What a server is started with.
pub struct ServerParts {
    /// Connection to the bus; the identity must be allowed to register
    /// `settings` and to publish `settings.*`.
    pub bus: Client,
    /// The opened core.
    pub core: Core,
    /// Who counts as administrator.
    pub access: Box<dyn Access>,
    /// The clock the core was opened with.
    pub clock: Arc<dyn Clock>,
    /// What the store reported while loading; published as retained
    /// `settings.store_report`.
    pub notices: Vec<Notice>,
    /// Receiver of failures nobody waits for.
    pub on_error: ErrorSink,
    /// How long to keep trying when the name `settings` is still held by a
    /// previous instance whose connection the broker has not cleaned up
    /// yet (restart right after a crash). Zero: one attempt.
    pub register_wait: Duration,
}

/// Pause between attempts to register the service name.
const REGISTER_RETRY: Duration = Duration::from_millis(20);

fn register(bus: &Client, wait: Duration) -> Result<ServiceEndpoint, BusError> {
    let started = Instant::now();
    loop {
        match bus.register(crate::access::SERVICE) {
            Err(BusError::Remote(ErrorCode::NameTaken)) if started.elapsed() < wait => {
                thread::sleep(REGISTER_RETRY);
            }
            other => return other,
        }
    }
}

/// A running settings service.
pub struct Server {
    input: SyncSender<Input>,
    thread: Option<JoinHandle<()>>,
    stats: Arc<ServerStats>,
    bus: Client,
}

fn decode<C: Call>(request: &IncomingRequest) -> Result<C, ErrorCode> {
    request.decode::<C>().map_err(|error| match error {
        BusError::Malformed => ErrorCode::BadPayload,
        other => other.code(),
    })
}

struct Worker {
    bus: Client,
    core: Core,
    access: Box<dyn Access>,
    clock: Arc<dyn Clock>,
    on_error: ErrorSink,
    stats: Arc<ServerStats>,
}

impl Worker {
    fn publish_events(&mut self) {
        for event in self.core.take_events() {
            let sent = match event {
                Event::Changed { key, value } => self.bus.publish(&Changed {
                    key,
                    value: value.into(),
                }),
                Event::Pending {
                    key,
                    value,
                    previous,
                    seconds,
                } => self.bus.publish(&PendingConfirmation {
                    key,
                    value: value.into(),
                    previous: previous.into(),
                    seconds,
                }),
                Event::Resolved {
                    key,
                    outcome,
                    value,
                } => self.bus.publish(&PendingResolved {
                    key,
                    outcome,
                    value: value.into(),
                }),
            };
            if sent.is_err() {
                self.stats.lost_events.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Decodes the call, runs it, announces what changed and only then
    /// answers: a caller that has its answer can rely on the events being
    /// on their way to every subscriber.
    fn serve<C: Call>(
        &mut self,
        request: IncomingRequest,
        run: impl FnOnce(&mut Core, &Caller, C) -> Result<C::Reply, SettingsError>,
    ) {
        let caller = Caller::new(request.caller(), self.access.as_ref());
        let outcome = decode::<C>(&request)
            .and_then(|call| run(&mut self.core, &caller, call).map_err(|error| error.code()));
        self.publish_events();
        let _ = match outcome {
            Ok(reply) => request.reply(&reply),
            Err(code) => request.fail(code),
        };
    }

    /// Handles one request; returns `false` when the service has to end.
    fn handle(&mut self, request: IncomingRequest) -> bool {
        self.stats.requests.fetch_add(1, Ordering::Relaxed);
        match request.method() {
            "get" => self.serve::<Get>(request, |core, _, call| {
                let value = core.get(&call.key)?;
                Ok(GetReply {
                    value: Some(value.into()),
                })
            }),
            "set" => self.serve::<Set>(request, |core, caller, call| {
                let changed = core.set(caller, &call.key, call.value.into())?;
                Ok(SetReply { changed })
            }),
            "reset" => self.serve::<Reset>(request, |core, caller, call| {
                let changed = core.reset(caller, &call.key)?;
                Ok(ResetReply { changed })
            }),
            "list" => self.serve::<ListRequest>(request, |core, caller, call| {
                let panels = core.list(caller, call.panel.as_deref())?;
                Ok(ListReply { panels })
            }),
            "confirm" => self.serve::<Confirm>(request, |core, _, call| {
                let resolved = core.confirm(call.key.as_deref())?;
                Ok(PendingReply { resolved })
            }),
            "revert" => self.serve::<Revert>(request, |core, _, call| {
                let resolved = core.revert(call.key.as_deref())?;
                Ok(PendingReply { resolved })
            }),
            "set_displays" => self.serve::<SetDisplays>(request, |core, caller, call| {
                core.set_displays(caller, &call.displays)
            }),
            "shutdown" => {
                let mut stop = false;
                self.serve::<Shutdown>(request, |core, caller, _| {
                    if !caller.admin {
                        return Err(SettingsError::NotPermitted);
                    }
                    stop = true;
                    core.flush().map(|_| ())
                });
                return !stop;
            }
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
        true
    }

    fn tick(&mut self) {
        if let Err(error) = self.core.tick() {
            (self.on_error)(&error);
        }
        self.publish_events();
    }

    fn next_input(&mut self, inputs: &Receiver<Input>) -> Option<Input> {
        loop {
            let wait = self
                .core
                .next_deadline()
                .and_then(|deadline| self.clock.real_wait(deadline));
            let Some(wait) = wait else {
                return inputs.recv().ok();
            };
            match inputs.recv_timeout(wait) {
                Ok(input) => return Some(input),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn run(mut self, inputs: &Receiver<Input>) {
        self.publish_events();
        while let Some(input) = self.next_input(inputs) {
            match input {
                Input::Request(request) => {
                    if !self.handle(*request) {
                        break;
                    }
                }
                Input::Capabilities(report) => {
                    let updated = capabilities_from_bus(&self.core.capabilities(), &report);
                    self.core.set_capabilities(updated);
                    self.stats.capability_report_taken();
                }
                Input::Tick(done) => {
                    self.tick();
                    let _ = done.send(());
                    continue;
                }
                Input::Stop => break,
            }
            self.tick();
        }
        if let Err(error) = self.core.flush() {
            (self.on_error)(&error);
        }
        self.publish_events();
    }
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>, BusError> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map_err(BusError::Io)
}

impl Server {
    /// Registers the `settings` service and starts serving.
    pub fn start(parts: ServerParts) -> Result<Server, BusError> {
        let ServerParts {
            bus,
            core,
            access,
            clock,
            notices,
            on_error,
            register_wait,
        } = parts;
        let endpoint = register(&bus, register_wait)?;
        if !notices.is_empty() {
            let report = StoreReport {
                notices: notices.iter().map(store_notice).collect(),
            };
            bus.publish(&report)?;
        }
        let (input, inputs) = sync_channel(QUEUE);
        let stats = Arc::new(ServerStats::default());

        let requests = input.clone();
        spawn("settingsd-inbox", move || {
            endpoint.run(|request| {
                let _ = requests.send(Input::Request(Box::new(request)));
            });
        })?;
        // The device service may not be allowed for this identity, or may
        // start later: the capabilities given at start hold until then.
        if let Ok(reports) = bus.subscribe::<devices::Capabilities>() {
            let forward = input.clone();
            spawn("settingsd-caps", move || loop {
                match reports.recv() {
                    Ok(report) => {
                        if forward.send(Input::Capabilities(report)).is_err() {
                            break;
                        }
                    }
                    Err(
                        BusError::Lagged(_) | BusError::Malformed | BusError::MessageVersion { .. },
                    ) => {}
                    Err(_) => break,
                }
            })?;
        }
        let worker = Worker {
            bus: bus.clone(),
            core,
            access,
            clock,
            on_error,
            stats: Arc::clone(&stats),
        };
        let thread = spawn("settingsd", move || worker.run(&inputs))?;
        Ok(Server {
            input,
            thread: Some(thread),
            stats,
            bus,
        })
    }

    /// Lets the service do what is due at the current time and returns
    /// when it is done. With a real clock the service wakes by itself;
    /// owners of a manual clock call this after moving the time.
    /// Returns `false` when the service has ended.
    pub fn tick(&self) -> bool {
        let (done, wait) = sync_channel(1);
        self.input.send(Input::Tick(done)).is_ok() && wait.recv().is_ok()
    }

    /// Counters of the server.
    pub fn stats(&self) -> &ServerStats {
        &self.stats
    }

    /// Blocks until the service ends (a `settings.shutdown` call), then
    /// leaves the bus.
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.bus.close();
    }

    /// Ends the service: everything is written to disk first.
    pub fn stop(mut self) {
        self.end();
    }

    fn end(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.input.send(Input::Stop);
            let _ = thread.join();
        }
        // Ends the inbox and capability threads.
        self.bus.close();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.end();
    }
}
