//! The app's way to the mesh: [`Port`] is what the window needs (devices, start, input, stop,
//! what happened), [`BusPort`] is it over the system bus (`mesh.view_*` calls and topics),
//! [`FakePort`] is a scripted one for tests.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;
use ultar_bus::messages::mesh::{MeshState, Peer};
use ultar_bus::messages::mesh_view::{RemoteInput, ViewEvent, ViewInput, ViewStart, ViewStop};
use ultar_bus::{BusError, Client, TypedSubscription};

/// How long a call to the mesh may take before the app gives up on it.
const CALL: Duration = Duration::from_millis(500);
/// How long a look at a topic waits for news (the window must stay alive).
const LOOK: Duration = Duration::from_millis(1);

/// What the mesh tells the app.
#[derive(Clone, PartialEq, Debug)]
pub enum PortEvent {
    /// The devices of the mesh (every change).
    Peers(Vec<Peer>),
    /// News of the session.
    View(ViewEvent),
    /// The bus is gone: nothing more will arrive.
    Lost,
}

/// The mesh as the window uses it.
pub trait Port {
    /// Asks `device` (its id) to show its screen; `false` when the mesh did not take the ask.
    fn start(&mut self, device: &str) -> bool;
    /// Sends one input event; `false` when the mesh did not take it.
    fn input(&mut self, input: RemoteInput) -> bool;
    /// Ends the session (or the wait for the answer).
    fn stop(&mut self);
    /// Everything that arrived since the last look (never blocks for long).
    fn events(&mut self) -> Vec<PortEvent>;
}

/// The mesh over the bus.
pub struct BusPort {
    bus: Client,
    view: Option<TypedSubscription<ViewEvent>>,
    state: Option<TypedSubscription<MeshState>>,
    lost: bool,
}

impl BusPort {
    /// The port over `bus`; it follows `mesh.state` and `mesh.view`.
    pub fn new(bus: Client) -> BusPort {
        let view = bus.subscribe().ok();
        let state = bus.subscribe().ok();
        BusPort {
            bus,
            view,
            state,
            lost: false,
        }
    }
}

fn drain<T>(sub: &Option<TypedSubscription<T>>, lost: &mut bool, mut each: impl FnMut(T))
where
    T: ultar_bus::Topic,
{
    let Some(sub) = sub else {
        return;
    };
    // At most a screenful per look, so a flood of pieces cannot freeze the window.
    for _ in 0..256 {
        match sub.recv_timeout(LOOK) {
            Ok(v) => each(v),
            Err(BusError::Timeout) => return,
            Err(BusError::Closed | BusError::NotConnected) => {
                *lost = true;
                return;
            }
            Err(_) => return,
        }
    }
}

impl Port for BusPort {
    fn start(&mut self, device: &str) -> bool {
        let ask = ViewStart {
            device: device.to_owned(),
        };
        self.bus.call(&ask, CALL).is_ok_and(|r| r.ok)
    }

    fn input(&mut self, input: RemoteInput) -> bool {
        self.bus.call(&ViewInput { input }, CALL).is_ok()
    }

    fn stop(&mut self) {
        let _ = self.bus.call(&ViewStop::default(), CALL);
    }

    fn events(&mut self) -> Vec<PortEvent> {
        let mut out = Vec::new();
        let mut lost = self.lost;
        drain(&self.state, &mut lost, |s: MeshState| {
            out.push(PortEvent::Peers(s.peers));
        });
        drain(&self.view, &mut lost, |e: ViewEvent| {
            out.push(PortEvent::View(e))
        });
        if lost && !self.lost {
            self.lost = true;
            out.push(PortEvent::Lost);
        }
        out
    }
}

/// What a [`FakePort`] saw and what it will tell.
#[derive(Default)]
pub struct Script {
    /// Devices asked for, in order.
    pub started: Vec<String>,
    /// Input events taken, in order.
    pub inputs: Vec<RemoteInput>,
    /// How often the session was stopped.
    pub stops: u32,
    /// What the next looks bring.
    pub queue: VecDeque<PortEvent>,
    /// `start` answers `false`.
    pub refuse_start: bool,
    /// `input` answers `false`.
    pub refuse_input: bool,
}

/// A scripted [`Port`] for tests: the test keeps the [`Script`] and plays the mesh.
#[derive(Clone, Default)]
pub struct FakePort(pub Rc<RefCell<Script>>);

impl FakePort {
    /// Queues news for the next look.
    pub fn tell(&self, event: PortEvent) {
        self.0.borrow_mut().queue.push_back(event);
    }
}

impl Port for FakePort {
    fn start(&mut self, device: &str) -> bool {
        let mut s = self.0.borrow_mut();
        s.started.push(device.to_owned());
        !s.refuse_start
    }

    fn input(&mut self, input: RemoteInput) -> bool {
        let mut s = self.0.borrow_mut();
        if s.refuse_input {
            return false;
        }
        s.inputs.push(input);
        true
    }

    fn stop(&mut self) {
        self.0.borrow_mut().stops += 1;
    }

    fn events(&mut self) -> Vec<PortEvent> {
        self.0.borrow_mut().queue.drain(..).collect()
    }
}
