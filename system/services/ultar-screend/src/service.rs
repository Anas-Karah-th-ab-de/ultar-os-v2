//! The bus side of the screen service: the mesh says a session begins, ends, or brings
//! the viewer's input; this service streams the screen back through `mesh.view_picture`.
//!
//! The stream runs on a helper thread of the service loop ([`Bus::later`]); the loop only
//! passes commands to it, and shows the notification with the Stop button.

use crate::diff::Rect;
use crate::engine::{Engine, Sink, Step};
use crate::input::{self, Inject, ViewOnly};
use crate::notice;
use crate::screen::{Compositor, Screen};
use crate::seat::Seat;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;
use ultar_bus::messages::mesh_view::{
    RemoteInput, ServeEvent, ViewLocked, ViewPicture, ViewReady, ViewStop,
};
use ultar_bus::messages::notify::{Dismiss, InvokedHint, TakeInvoked};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{Client, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

/// Name of the service on the bus (it offers no calls).
pub const SERVICE: &str = "screend";
/// Longest wait for the mesh to answer one call.
const CALL: Duration = Duration::from_secs(5);
/// Input events kept for the next look; more are dropped (a flood cannot pile up).
const MAX_QUEUED: usize = 256;

enum Cmd {
    Input(RemoteInput),
    End,
    /// The person in front changed or left: the screen is not the viewed person's any more.
    Locked,
}

struct Worker {
    id: u64,
    tx: Sender<Cmd>,
}

/// The screen service.
pub struct ScreendService {
    compositor: PathBuf,
    root: PathBuf,
    worker: Option<Worker>,
    note: Option<u64>,
    seat: Seat,
}

/// The mesh as the engine's sink: a rectangle is taken when the mesh says so.
struct MeshSink<'a>(&'a Client);

impl Sink for MeshSink<'_> {
    fn picture(&mut self, rect: Rect, data: Vec<u8>) -> bool {
        let call = ViewPicture {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            data,
        };
        self.0.call(&call, CALL).is_ok_and(|r| r.taken)
    }
}

impl ScreendService {
    /// Reads the screen from the compositor at `compositor`, injects below `root`.
    pub fn new(compositor: PathBuf, root: PathBuf) -> Self {
        Self {
            compositor,
            root,
            worker: None,
            note: None,
            seat: Seat::default(),
        }
    }

    fn begin(&mut self, id: u64, viewer: String, bus: &Bus<'_, Self>) {
        if self.worker.is_some() {
            return;
        }
        if !self.seat.begin() {
            // nobody is in front (sign-in screen): there is no screen to show
            self.seat.end();
            let _ = bus.call(&ViewLocked {}, CALL);
            return;
        }
        let (tx, rx) = channel();
        let (compositor, root) = (self.compositor.clone(), self.root.clone());
        let started = bus.later(move |client| {
            stream(client, id, &compositor, &root, &rx);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| service.stopped(id, bus))
        });
        if !started {
            let _ = bus.call(
                &ViewReady {
                    id,
                    width: 0,
                    height: 0,
                },
                CALL,
            );
            return;
        }
        self.worker = Some(Worker { id, tx });
        bus.log(Record::new(Level::Info, "screend.begin"));
        let post = notice::watched(&viewer);
        let _ = bus.later(move |client| {
            let note = client.call(&post, CALL).ok().map(|r| r.id);
            Box::new(move |service: &mut Self, _: &Bus<'_, Self>| service.note = note)
        });
    }

    fn stopped(&mut self, id: u64, bus: &Bus<'_, Self>) {
        if self.worker.as_ref().is_some_and(|w| w.id == id) {
            self.worker = None;
            self.seat.end();
        }
        if let Some(note) = self.note.take() {
            let _ = bus.later(move |client| {
                let _ = client.call(&Dismiss { id: note }, CALL);
                Box::new(|_: &mut Self, _: &Bus<'_, Self>| {})
            });
        }
        bus.log(Record::new(Level::Info, "screend.end"));
    }

    fn command(&self, id: u64, cmd: Cmd) {
        if let Some(w) = self.worker.as_ref().filter(|w| w.id == id) {
            let _ = w.tx.send(cmd);
        }
    }

    /// The person pressed Stop in the notification.
    fn invoked(&mut self, bus: &Bus<'_, Self>) {
        let note = self.note;
        let _ = bus.later(move |client| {
            let taken = client
                .call(&TakeInvoked {}, CALL)
                .map(|r| r.invoked)
                .unwrap_or_default();
            let stop = taken
                .iter()
                .any(|i| Some(i.id) == note && i.action == notice::STOP);
            if stop {
                let _ = client.call(&ViewStop {}, CALL);
            }
            Box::new(|_: &mut Self, _: &Bus<'_, Self>| {})
        });
    }
}

/// Streams the screen of session `id` until it ends; the mesh is told how.
fn stream(
    client: &Client,
    id: u64,
    compositor: &std::path::Path,
    root: &std::path::Path,
    rx: &std::sync::mpsc::Receiver<Cmd>,
) {
    let Ok(screen) = Compositor::open(compositor) else {
        let _ = client.call(
            &ViewReady {
                id,
                width: 0,
                height: 0,
            },
            CALL,
        );
        return;
    };
    let injector = |size| -> Box<dyn Inject> {
        match input::open(root, size) {
            Some(real) => Box::new(real),
            None => Box::new(ViewOnly),
        }
    };
    let Ok((mut engine, (width, height))) = Engine::begin(screen, injector) else {
        let _ = client.call(
            &ViewReady {
                id,
                width: 0,
                height: 0,
            },
            CALL,
        );
        return;
    };
    if !client
        .call(&ViewReady { id, width, height }, CALL)
        .is_ok_and(|r| r.ok)
    {
        return;
    }
    run(&mut engine, client, rx);
}

fn run<S: Screen, I: Inject>(
    engine: &mut Engine<S, I>,
    client: &Client,
    rx: &std::sync::mpsc::Receiver<Cmd>,
) {
    let mut queue: VecDeque<RemoteInput> = VecDeque::new();
    let mut sink = MeshSink(client);
    loop {
        let step = engine.step(&mut || queue.pop_front(), &mut sink);
        match step {
            Step::Locked => {
                let _ = client.call(&ViewLocked {}, CALL);
                return;
            }
            Step::Gone => {
                let _ = client.call(&ViewStop {}, CALL);
                return;
            }
            Step::Wait(mut wait) => loop {
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Input(i)) => {
                        if queue.len() < MAX_QUEUED {
                            queue.push_back(i);
                        }
                        wait = Duration::ZERO; // input is looked after at once
                    }
                    Ok(Cmd::Locked) => {
                        engine.finish(Step::Locked);
                        let _ = client.call(&ViewLocked {}, CALL);
                        return;
                    }
                    Ok(Cmd::End) | Err(RecvTimeoutError::Disconnected) => {
                        engine.finish(Step::Gone);
                        return;
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                }
            },
        }
    }
}

impl Service for ScreendService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![ServeEvent::TOPIC, InvokedHint::TOPIC, SessionState::TOPIC]
    }

    fn start(&mut self, _bus: &Bus<'_, Self>) {}

    fn request(&mut self, request: IncomingRequest, _bus: &Bus<'_, Self>) {
        let _ = request.fail(ErrorCode::UnknownMethod);
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == SessionState::TOPIC {
            if let Ok(state) = event.decode::<SessionState>() {
                if self.seat.observe(state.user) {
                    if let Some(w) = self.worker.as_ref() {
                        let _ = w.tx.send(Cmd::Locked);
                    }
                }
            }
            return;
        }
        if event.topic == InvokedHint::TOPIC {
            if let Ok(hint) = event.decode::<InvokedHint>() {
                if hint.sender == bus.client().identity() {
                    self.invoked(bus);
                }
            }
            return;
        }
        match event.decode::<ServeEvent>() {
            Ok(ServeEvent::Begin { id, viewer }) => self.begin(id, viewer, bus),
            Ok(ServeEvent::Input { id, input }) => self.command(id, Cmd::Input(input)),
            Ok(ServeEvent::End { id }) => self.command(id, Cmd::End),
            Err(_) => {}
        }
    }
}
