//! Everything the bridge knows, in one place: the Wayland objects of the
//! app (by object id), the native surfaces they became, the seat, outputs
//! and the clipboard.

use crate::data::Clipboard;
use crate::native::Native;
use crate::output::Outputs;
use crate::pixels::Image;
use crate::seat::Seat;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_surface::proto::{Configure, Rect, Rights, SurfaceId};
use wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1;
use wayland_protocols::xdg::shell::server::{
    xdg_popup::XdgPopup, xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel, xdg_wm_base::XdgWmBase,
};
use wayland_server::backend::protocol::ProtocolError;
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::{
    wl_buffer::WlBuffer, wl_callback::WlCallback, wl_surface::WlSurface,
};
use wayland_server::{Client, DisplayHandle};

/// Double-buffered state of a surface (applied on commit).
#[derive(Default)]
pub struct Pending {
    /// `Some(None)` = detach, `Some(Some(b))` = new buffer.
    pub buffer: Option<Option<WlBuffer>>,
    /// Damage in surface coordinates.
    pub damage: Vec<Rect>,
    /// Damage in buffer pixels.
    pub damage_buffer: Vec<Rect>,
    /// New buffer scale.
    pub scale: Option<i32>,
    /// Frame callbacks asked for.
    pub callbacks: Vec<WlCallback>,
    /// New subsurface position (applied with the parent's commit).
    pub position: Option<(i32, i32)>,
}

/// What the committed content is.
#[derive(Clone, Default)]
pub struct Content {
    /// The pixels (buffer pixels).
    pub image: Image,
    /// Buffer scale.
    pub scale: i32,
    /// Has transparent pixels.
    pub translucent: bool,
}

/// A toplevel window.
pub struct Toplevel {
    /// The protocol object.
    pub res: XdgToplevel,
    /// Its native surface.
    pub native: SurfaceId,
    /// The newest native configure.
    pub configure: Option<Configure>,
    /// Its decoration object, if the app asked.
    pub decoration: Option<ZxdgToplevelDecorationV1>,
    /// The app draws its own title bar.
    pub client_header: bool,
    /// The title (kept for a window that is shown again).
    pub title: String,
}

/// A menu or tooltip.
pub struct Popup {
    /// The protocol object.
    pub res: XdgPopup,
    /// Its native surface (created on the first commit).
    pub native: Option<SurfaceId>,
    /// The toplevel's wl_surface it belongs to.
    pub root: ObjectId,
    /// Its parent's wl_surface (toplevel or popup).
    pub parent: ObjectId,
    /// Position relative to the parent's window geometry, and size.
    pub rect: Rect,
    /// Offset in the root's window geometry.
    pub offset: (i32, i32),
    /// Took the input grab.
    pub grab: bool,
}

/// What a surface is for.
#[derive(Default)]
pub enum Role {
    /// Nothing yet.
    #[default]
    None,
    /// A pointer picture (the compositor draws its own pointer).
    Cursor,
    /// Part of another surface.
    Sub {
        /// The parent surface.
        parent: ObjectId,
        /// Position in the parent (surface coordinates).
        pos: (i32, i32),
    },
    /// A window.
    Toplevel(Box<Toplevel>),
    /// A menu or tooltip.
    Popup(Box<Popup>),
}

/// The xdg_surface side of a surface.
#[derive(Default)]
pub struct Xdg {
    /// The protocol object.
    pub res: Option<XdgSurface>,
    /// Window geometry (surface coordinates), current and pending.
    pub geometry: Option<Rect>,
    /// Pending window geometry.
    pub pending_geometry: Option<Rect>,
    /// The app made its initial commit.
    pub committed: bool,
    /// Configures sent and not yet acknowledged: (wayland serial, native serial).
    pub serials: Vec<(u32, u32)>,
    /// Native serial of the newest acknowledged configure.
    pub acked: u32,
    /// Smallest size (0 = none).
    pub min_size: (u32, u32),
    /// Largest size (0 = none).
    pub max_size: (u32, u32),
    /// Told which display it is on.
    pub entered: bool,
}

/// One wl_surface.
pub struct Surf {
    /// The protocol object.
    pub wl: WlSurface,
    /// Its connection.
    pub client: ClientId,
    /// Not yet applied.
    pub pending: Pending,
    /// Committed content.
    pub content: Option<Content>,
    /// Its role.
    pub role: Role,
    /// Subsurfaces in stacking order.
    pub children: Vec<ObjectId>,
    /// Committed frame callbacks waiting for the screen.
    pub callbacks: Vec<WlCallback>,
    /// The xdg side.
    pub xdg: Xdg,
}

impl Surf {
    /// A new surface without role.
    pub fn new(wl: WlSurface, client: ClientId) -> Surf {
        Surf {
            wl,
            client,
            pending: Pending::default(),
            content: None,
            role: Role::None,
            children: Vec::new(),
            callbacks: Vec::new(),
            xdg: Xdg::default(),
        }
    }

    /// The native surface it shows on, if any.
    pub fn native(&self) -> Option<SurfaceId> {
        match &self.role {
            Role::Toplevel(t) => Some(t.native),
            Role::Popup(p) => p.native,
            _ => None,
        }
    }
}

/// A shared-memory pool.
pub struct Pool {
    /// The memory file (shared with its buffers, which may outlive it).
    pub fd: Arc<OwnedFd>,
    /// Its size.
    pub size: u64,
}

/// A client buffer.
pub struct Buffer {
    /// The pool's file.
    pub fd: Arc<OwnedFd>,
    /// Where the pixels are.
    pub layout: crate::pixels::Layout,
}

/// Counts per connection (limits).
#[derive(Default)]
pub struct Counts {
    /// Surfaces.
    pub surfaces: usize,
    /// Pools.
    pub pools: usize,
    /// Buffers.
    pub buffers: usize,
    /// Positioners.
    pub positioners: usize,
}

/// The objects of the app.
#[derive(Default)]
pub struct Objects {
    /// Surfaces.
    pub surfaces: HashMap<ObjectId, Surf>,
    /// Native surface → wl_surface.
    pub by_native: HashMap<SurfaceId, ObjectId>,
    /// Pools.
    pub pools: HashMap<ObjectId, Pool>,
    /// Buffers.
    pub buffers: HashMap<ObjectId, Buffer>,
    /// Positioners.
    pub positioners: HashMap<ObjectId, crate::popup::Positioner>,
    /// Per connection.
    pub counts: HashMap<ClientId, Counts>,
    /// Live connections.
    pub connections: Vec<ClientId>,
    /// xdg_wm_base objects (they answer pings).
    pub wm_bases: Vec<XdgWmBase>,
}

/// How long a user action may be used (the compositor allows 10 s).
const USER_ACTION: Duration = Duration::from_secs(8);

/// Wayland serials and the native serials they stand for.
#[derive(Default)]
pub struct Serials {
    next: u32,
    map: Vec<(u32, u32)>,
    /// The newest native user action (a press) and when it came.
    last_user: Option<(u32, Instant)>,
    /// A ping of the compositor passed on to the app: (Wayland, native).
    pub ping: Option<(u32, u32)>,
}

impl Serials {
    /// A fresh Wayland serial standing for `native` (0 = none).
    pub fn next(&mut self, native: u32) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        if native != 0 {
            self.map.push((self.next, native));
            if self.map.len() > 64 {
                self.map.remove(0);
            }
        }
        self.next
    }

    /// Like [`Serials::next`] for a press (a user action).
    pub fn user(&mut self, native: u32) -> u32 {
        if native != 0 {
            self.last_user = Some((native, Instant::now()));
        }
        self.next(native)
    }

    /// The native serial behind a Wayland serial (0 = unknown).
    pub fn native(&self, serial: u32) -> u32 {
        self.map
            .iter()
            .rev()
            .find(|(w, _)| *w == serial)
            .map_or(0, |(_, n)| *n)
    }

    /// The newest user action, if still fresh enough for the compositor.
    pub fn fresh_user(&self) -> Option<u32> {
        self.last_user
            .filter(|(_, at)| at.elapsed() < USER_ACTION)
            .map(|(s, _)| s)
    }
}

/// wl_display error: out of memory (a limit of the bridge was reached).
pub const NO_MEMORY: u32 = 2;
/// wl_display error: the request cannot be served.
pub const IMPLEMENTATION: u32 = 3;

/// Ends one Wayland connection with a wl_display error. New objects of
/// the request need no data then (the connection is already dead).
pub fn refuse(dh: &DisplayHandle, client: &Client, code: u32, message: &str) {
    client.kill(
        dh,
        ProtocolError {
            code,
            object_id: 1,
            object_interface: "wl_display".into(),
            message: message.into(),
        },
    );
}

/// The bridge.
pub struct State {
    /// The Wayland display.
    pub dh: DisplayHandle,
    /// The compositor side.
    pub native: Native,
    /// Rights the compositor gave this app.
    pub rights: Rights,
    /// Wayland objects.
    pub objects: Objects,
    /// Input devices.
    pub seat: Seat,
    /// Displays.
    pub outputs: Outputs,
    /// Clipboard.
    pub clipboard: Clipboard,
    /// Serials.
    pub serials: Serials,
    /// Start of the clock sent with input events.
    pub start: Instant,
    /// A fatal native error happened.
    pub native_failed: bool,
}

impl State {
    /// Milliseconds since start (event time stamps).
    pub fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    /// Counts of a connection.
    pub fn counts(&mut self, client: &ClientId) -> &mut Counts {
        self.objects.counts.entry(client.clone()).or_default()
    }

    /// Records a native failure (the loop ends the bridge).
    pub fn native_result<T>(
        &mut self,
        r: Result<T, ultar_surface::client::ClientError>,
    ) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(_) => {
                self.native_failed = true;
                None
            }
        }
    }
}
