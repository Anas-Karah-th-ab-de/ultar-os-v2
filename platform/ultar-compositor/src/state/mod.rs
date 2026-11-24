//! The compositor's state: clients, surfaces, windows, focus, damage.
//! Independent of the backend and of the event loop, so it can be driven
//! by tests directly.

mod budget;
mod grabs;
mod input;
mod keys;
mod layers;
mod memory;
mod output_config;
mod outputs;
pub mod phone;
mod requests;
mod session;
mod shell;
mod text;
mod transfer;
mod windows;
mod workspaces;

pub use input::PointerGrab;
pub use layers::Placed;
pub use output_config::{Identify, OutputManagement, Provisional, MAX_QUEUED_SETUPS};
pub use session::{Grant, Session, INDICATOR_MS};
pub use shell::ShellSession;
pub use transfer::Transfers;
pub use windows::Window;
pub use workspaces::{fit as thumbnail_fit, Workspaces};

use crate::config::Config;
use crate::conn::{Conn, Peer};
use crate::decor::DecorRenderer;
use crate::geom::{Rect, Region};
use crate::keymap::Layout as KeyLayout;
use crate::output::Layout;
use crate::surface::{Buffer, Surface};
use std::collections::{BTreeMap, HashMap, VecDeque};
use ultar_surface::proto::{
    BufferId, ErrorCode, Event, OutputId, Rights, SurfaceId, Transport, PROTOCOL_VERSION,
};

/// Number of a connection.
pub type ClientId = u32;

/// Rights of the system clients (shell, lock screen, input method, display
/// service): they may use the reserved client slots and memory.
pub fn is_system(rights: Rights) -> bool {
    let system = Rights::SHELL
        .union(Rights::LOCK_SCREEN)
        .union(Rights::INPUT_METHOD)
        .union(Rights::OUTPUT_CONFIG);
    rights.0 & system.0 != 0
}

/// A surface, globally: client plus the client's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceKey {
    /// Owner.
    pub client: ClientId,
    /// Id in the owner's namespace.
    pub surface: SurfaceId,
}

/// How many recent user-action serials a client keeps.
const SERIALS_KEPT: usize = 8;

/// One connected client.
#[derive(Debug)]
pub struct Client {
    /// The connection.
    pub conn: Conn,
    /// Rights from the rights table.
    pub rights: Rights,
    /// Application id from `Hello` (a hint, not an identity).
    pub app_id: String,
    /// `Hello` was received.
    pub hello: bool,
    /// Surfaces.
    pub surfaces: HashMap<SurfaceId, Surface>,
    /// Buffers.
    pub buffers: HashMap<BufferId, Buffer>,
    /// Pixel memory in use.
    pub bytes: u64,
    /// Serials of presses and keys delivered to this client, with time.
    pub serials: VecDeque<(u32, u64)>,
    /// Outstanding ping (serial, sent at).
    pub ping: Option<(u32, u64)>,
    /// Set when the client must be dropped (error code to send, if any).
    pub doomed: Option<Option<ErrorCode>>,
    /// A capture in progress (output, next row).
    pub capture: Option<(OutputId, u32)>,
}

impl Client {
    fn new(conn: Conn, rights: Rights) -> Client {
        Client {
            conn,
            rights,
            app_id: String::new(),
            hello: false,
            surfaces: HashMap::new(),
            buffers: HashMap::new(),
            bytes: 0,
            serials: VecDeque::new(),
            ping: None,
            doomed: None,
            capture: None,
        }
    }

    /// True if `serial` was given to this client for a user action within
    /// `max_age` milliseconds.
    pub fn recent_serial(&self, serial: u32, now: u64, max_age: u64) -> bool {
        self.serials
            .iter()
            .any(|&(s, t)| s == serial && now.saturating_sub(t) <= max_age)
    }

    fn remember_serial(&mut self, serial: u32, now: u64) {
        if self.serials.len() == SERIALS_KEPT {
            self.serials.pop_front();
        }
        self.serials.push_back((serial, now));
    }
}

/// Modifiers, Caps Lock and the active layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keyboard {
    /// Modifier keys held (bits of `Modifiers`).
    pub modifiers: u8,
    /// Caps Lock is on.
    pub caps_lock: bool,
    /// Index into `Config::layouts`.
    pub layout_index: usize,
    /// Key events taken from the input devices so far (press, release, repeat; never which
    /// key). The end-to-end test reads it to know a key arrived (`--input-ack`).
    pub events: u64,
}

/// Number sources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    client: ClientId,
    serial: u32,
    window: u32,
}

/// The whole state.
pub struct State {
    /// Settings.
    pub config: Config,
    /// Outputs.
    pub layout: Layout,
    /// Clients by id.
    pub clients: BTreeMap<ClientId, Client>,
    counters: Counters,
    /// Toplevel windows, bottom to top.
    pub windows: Vec<Window>,
    /// Other surfaces with a role (panels, popups, …) in creation order.
    pub layers: Vec<SurfaceKey>,
    /// Keyboard focus.
    pub focus: Option<SurfaceKey>,
    /// Pointer position (logical points) and state.
    pub pointer: input::Pointer,
    /// Keyboard state.
    pub keyboard: Keyboard,
    /// Lock, trusted path, activation, recording indicator.
    pub session: Session,
    /// Damage per output (device pixels).
    pub damage: HashMap<OutputId, Region>,
    /// Title bar renderer (None if the toolkit could not start).
    pub decor: Option<DecorRenderer>,
    /// Current time in milliseconds (set by the loop).
    pub now: u64,
    /// Clipboard, drag and drop.
    pub transfers: Transfers,
    /// Shared memory is offered to clients.
    pub shm: bool,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("clients", &self.clients.len())
            .field("windows", &self.windows.len())
            .finish()
    }
}

impl State {
    /// A state with no outputs and no clients.
    pub fn new(config: Config, shm: bool) -> State {
        let decor = DecorRenderer::new(config.lang, config.theme);
        State {
            shm: shm && !config.inline_only,
            config,
            layout: Layout::new(),
            clients: BTreeMap::new(),
            counters: Counters {
                client: 1,
                serial: 0,
                window: 0,
            },
            windows: Vec::new(),
            layers: Vec::new(),
            focus: None,
            pointer: input::Pointer::default(),
            keyboard: Keyboard::default(),
            session: Session::default(),
            damage: HashMap::new(),
            decor,
            now: 0,
            transfers: Transfers::default(),
        }
    }

    /// Right-to-left system language.
    pub fn rtl(&self) -> bool {
        self.decor.as_ref().is_some_and(DecorRenderer::rtl)
    }

    /// The active keyboard layout.
    pub fn key_layout(&self) -> KeyLayout {
        self.config
            .layouts
            .get(self.keyboard.layout_index)
            .copied()
            .unwrap_or(KeyLayout::English)
    }

    /// A fresh window id (never reused within the session).
    pub(crate) fn next_window_id(&mut self) -> u32 {
        self.counters.window = self.counters.window.wrapping_add(1).max(1);
        self.counters.window
    }

    /// A fresh serial.
    pub fn next_serial(&mut self) -> u32 {
        self.counters.serial = self.counters.serial.wrapping_add(1).max(1);
        self.counters.serial
    }

    /// Adds a connection. Returns `None` when a limit is reached (the
    /// stream is dropped, which closes it): every uid gets at most
    /// [`crate::config::ShareLimits::clients_per_uid`] connections, and
    /// the reserved slots are only for system uids (shell, lock screen,
    /// input method), so no app – and no lesser right – can lock them out.
    pub fn add_client(&mut self, conn: Conn) -> Option<ClientId> {
        let limits = self.config.limits;
        let uid = conn.peer.uid;
        self.config.rights.refresh_apps();
        let rights = self.config.rights.rights_of(uid);
        let total = self.clients.len();
        let same = self
            .clients
            .values()
            .filter(|c| c.conn.peer.uid == uid)
            .count();
        let open = if is_system(rights) {
            limits.clients
        } else {
            limits.clients.saturating_sub(limits.share.reserved_clients)
        };
        if total >= open || same >= limits.share.clients_per_uid {
            return None;
        }
        let id = self.counters.client;
        self.counters.client = self.counters.client.wrapping_add(1).max(1);
        self.clients.insert(id, Client::new(conn, rights));
        Some(id)
    }

    /// Creates a connection object for an accepted stream.
    pub fn accept(
        &mut self,
        stream: std::os::unix::net::UnixStream,
        peer: Peer,
    ) -> Option<ClientId> {
        let conn = Conn::new(stream, peer, self.shm).ok()?;
        self.add_client(conn)
    }

    /// Queues an event for a client; a client whose queue overflows is
    /// marked for removal.
    pub fn send(&mut self, client: ClientId, event: Event) {
        let limit = self.config.limits.out_bytes;
        if let Some(c) = self.clients.get_mut(&client) {
            if c.doomed.is_none() && !c.conn.queue(&event, limit) {
                c.doomed = Some(None);
            }
        }
    }

    /// Ends a client with a protocol error.
    pub fn fail(&mut self, client: ClientId, code: ErrorCode) {
        if let Some(c) = self.clients.get_mut(&client) {
            if c.doomed.is_none() {
                c.doomed = Some(Some(code));
            }
        }
    }

    fn welcome(&mut self, client: ClientId) {
        let Some(c) = self.clients.get(&client) else {
            return;
        };
        let transport = if self.shm {
            Transport::SharedMemory
        } else {
            Transport::Inline
        };
        let event = Event::Welcome {
            version: PROTOCOL_VERSION,
            transport,
            rights: c.rights,
            max_buffer_bytes: self.config.limits.client_bytes,
        };
        self.send(client, event);
        let infos: Vec<_> = self.layout.enabled().map(|o| o.info()).collect();
        for info in infos {
            self.send(client, Event::Output(info));
        }
        self.shell_welcome(client);
        self.send_output_details(client);
    }

    /// Removes a client and everything it owned. Safe at any time (a client
    /// killed mid-frame leaves no trace but damage).
    pub fn remove_client(&mut self, client: ClientId) {
        let keys: Vec<SurfaceKey> = match self.clients.get(&client) {
            Some(c) => c
                .surfaces
                .keys()
                .map(|&surface| SurfaceKey { client, surface })
                .collect(),
            None => return,
        };
        for key in keys {
            self.destroy_surface(key);
        }
        self.transfers.client_gone(client);
        self.forget_thumbnail_budget(client);
        let cancelled = self.transfers.take_cancelled();
        for (to, id) in cancelled {
            self.send(to, Event::TransferCancelled { transfer: id });
        }
        self.clients.remove(&client);
        if self
            .pointer
            .grab
            .as_ref()
            .is_some_and(|g| g.client() == Some(client))
        {
            self.pointer.grab = None;
        }
        self.pointer.touches.retain(|_, k| k.client != client);
        self.session_forget(client);
        self.output_client_gone(client);
    }

    /// Clients marked for removal.
    pub fn doomed(&self) -> Vec<(ClientId, Option<ErrorCode>)> {
        self.clients
            .iter()
            .filter_map(|(&id, c)| c.doomed.map(|code| (id, code)))
            .collect()
    }

    /// Marks a logical rectangle as changed on every output it touches.
    pub fn damage_logical(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        for o in self.layout.enabled() {
            let Some(part) = rect.intersect(&o.logical_rect()) else {
                continue;
            };
            let device = part.translate(-o.x, -o.y).scale_out(o.scale()).inflate(1);
            let screen = Rect::new(0, 0, o.width as i32, o.height as i32);
            if let Some(d) = device.intersect(&screen) {
                self.damage.entry(o.id).or_default().add(d);
            }
        }
    }

    /// Marks every output as completely changed.
    pub fn damage_all(&mut self) {
        let all: Vec<(OutputId, Rect)> = self
            .layout
            .enabled()
            .map(|o| (o.id, Rect::new(0, 0, o.width as i32, o.height as i32)))
            .collect();
        for (id, r) in all {
            self.damage.entry(id).or_default().add(r);
        }
    }

    /// True if any output has damage.
    pub fn has_damage(&self) -> bool {
        self.damage.values().any(|r| !r.is_empty())
    }

    /// The surface of a key.
    pub fn surface(&self, key: SurfaceKey) -> Option<&Surface> {
        self.clients.get(&key.client)?.surfaces.get(&key.surface)
    }

    /// The surface of a key, for writing.
    pub fn surface_mut(&mut self, key: SurfaceKey) -> Option<&mut Surface> {
        self.clients
            .get_mut(&key.client)?
            .surfaces
            .get_mut(&key.surface)
    }

    /// Sends pings that are due and marks windows of clients that have not
    /// answered for [`crate::config::Limits::ping_ms`] as not responding.
    pub fn check_pings(&mut self) {
        let limit = self.config.limits.ping_ms;
        let now = self.now;
        let late: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.ping.is_some_and(|(_, t)| now.saturating_sub(t) >= limit))
            .map(|(&id, _)| id)
            .collect();
        for client in late {
            self.set_unresponsive(client, true);
        }
    }

    /// Earliest time something time-based is due (ping limit, transfer
    /// deadline), for the loop's timeout.
    pub fn next_deadline(&self) -> Option<u64> {
        let limit = self.config.limits.ping_ms;
        let ping = self
            .clients
            .values()
            .filter_map(|c| c.ping.map(|(_, t)| t + limit))
            .filter(|&t| t > self.now || self.windows.iter().any(|w| !w.unresponsive))
            .min();
        let indicator = (self.session.indicator_shown && self.session.recording_until > self.now)
            .then_some(self.session.recording_until);
        [
            ping,
            self.transfers.next_deadline(),
            indicator,
            self.output_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}
