//! A compositor client with several surfaces, each running a widget tree (the bar and its
//! popup, the lock screen, a system dialog).

mod surface;

pub use surface::HostSurface;

use crate::view::View;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_surface::client::{Client, ClientError, Window, WindowConfig};
use ultar_surface::proto::{Event, Request, SurfaceId};
use ultar_ui::event::{Action, Event as UiEvent, Key, Modifiers, PointerButton};
use ultar_ui::geom::Size;
use ultar_ui::ui::NodeId;

/// Most surfaces one program shows.
pub const MAX_SURFACES: usize = 8;

/// What happened.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A tree's action.
    Action(SurfaceId, Action),
    /// A key the tree did not use (Esc of a sheet, Enter of a panel).
    Key(SurfaceId, Key, Modifiers),
    /// The secondary button or the menu key on a node.
    Menu(SurfaceId, NodeId),
    /// A surface got a new size from the compositor (the program builds it again).
    Resized(SurfaceId, Size),
    /// A compositor event that is not for one tree (window list, shell key, lock …).
    Compositor(Event),
}

/// One connection and its surfaces.
pub struct Host {
    /// The connection.
    pub client: Client,
    surfaces: BTreeMap<SurfaceId, HostSurface>,
    started: Instant,
    serial: Option<u32>,
    caps_lock: bool,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("surfaces", &self.surfaces.len())
            .finish_non_exhaustive()
    }
}

impl Host {
    /// Connects (retrying while a restarting compositor comes back).
    pub fn connect(path: &Path, app_id: &str) -> Result<Host, ClientError> {
        let client = Client::connect_retry(path, app_id, 50, Duration::from_millis(100))?;
        Ok(Host::with_client(client))
    }

    /// A host over a connection.
    pub fn with_client(client: Client) -> Host {
        Host {
            client,
            surfaces: BTreeMap::new(),
            started: Instant::now(),
            serial: None,
            caps_lock: false,
        }
    }

    /// Opens a surface showing `view`; `fixed` is its size when the compositor leaves it
    /// to the client (popups).
    pub fn open(
        &mut self,
        config: &WindowConfig,
        view: View,
        fixed: Option<Size>,
    ) -> Result<SurfaceId, ClientError> {
        if self.surfaces.len() >= MAX_SURFACES {
            return Err(ClientError::InvalidSize);
        }
        let window = Window::create(&mut self.client, config)?;
        let id = window.surface();
        self.surfaces.insert(
            id,
            HostSurface::new(window, view, fixed, config.translucent),
        );
        Ok(id)
    }

    /// Shows another tree in a surface.
    pub fn replace(&mut self, id: SurfaceId, view: View, fixed: Option<Size>) {
        if let Some(s) = self.surfaces.get_mut(&id) {
            s.replace(view, fixed);
        }
    }

    /// Removes a surface.
    pub fn close(&mut self, id: SurfaceId) -> Result<(), ClientError> {
        match self.surfaces.remove(&id) {
            Some(s) => s.window.destroy(&mut self.client),
            None => Ok(()),
        }
    }

    /// A surface's tree.
    pub fn view(&self, id: SurfaceId) -> Option<&View> {
        self.surfaces.get(&id).map(|s| &s.view)
    }

    /// A surface's tree, for changes.
    pub fn view_mut(&mut self, id: SurfaceId) -> Option<&mut View> {
        self.surfaces.get_mut(&id).map(|s| &mut s.view)
    }

    /// A surface.
    pub fn surface(&self, id: SurfaceId) -> Option<&HostSurface> {
        self.surfaces.get(&id)
    }

    /// The serial of the user's last press or key (for activation tokens).
    pub fn last_serial(&self) -> Option<u32> {
        self.serial
    }

    /// Caps Lock was on at the last key.
    pub fn caps_lock(&self) -> bool {
        self.caps_lock
    }

    /// Asks for an activation token for the last user action (answer: `ActivationToken`).
    pub fn request_activation(&mut self) -> Result<bool, ClientError> {
        match self.serial {
            Some(serial) => {
                self.client.send(&Request::RequestActivation { serial })?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Milliseconds since the host started (the trees' clock).
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Waits up to `timeout` for one compositor event and handles it; advances the trees'
    /// clocks and draws.
    pub fn dispatch(&mut self, timeout: Option<Duration>) -> Result<Vec<HostEvent>, ClientError> {
        let now = self.now_ms();
        let wake = self
            .surfaces
            .values()
            .filter_map(|s| s.view.ui.next_wake())
            .min()
            .map(|w| Duration::from_millis(w.saturating_sub(now)));
        let wait = match (timeout, wake) {
            (Some(t), Some(w)) => Some(t.min(w)),
            (t, w) => t.or(w),
        };
        let mut out = Vec::new();
        if let Some(event) = self.client.next_event(wait)? {
            self.handle(&event, &mut out)?;
        }
        let now = self.now_ms();
        for (id, s) in self.surfaces.iter_mut() {
            s.view.ui.advance_to(now);
            out.extend(
                s.view
                    .ui
                    .take_actions()
                    .into_iter()
                    .map(|a| HostEvent::Action(*id, a)),
            );
        }
        self.draw()?;
        Ok(out)
    }

    /// Paints every surface that changed.
    pub fn draw(&mut self) -> Result<(), ClientError> {
        for s in self.surfaces.values_mut() {
            s.draw(&mut self.client)?;
        }
        Ok(())
    }

    fn handle(&mut self, event: &Event, out: &mut Vec<HostEvent>) -> Result<(), ClientError> {
        if let Event::BufferReleased { .. } = event {
            for s in self.surfaces.values_mut() {
                if s.window.handle(event) {
                    break;
                }
            }
            return Ok(());
        }
        if let Event::Key { modifiers, .. } = event {
            self.caps_lock = modifiers.has(ultar_surface::proto::Modifiers::CAPS_LOCK);
        }
        let target = surface_of(event);
        if let Event::PointerButton {
            serial,
            pressed: true,
            ..
        }
        | Event::Key {
            serial, state: 1, ..
        } = event
        {
            self.serial = Some(*serial);
        }
        let Some(id) = target.filter(|id| self.surfaces.contains_key(id)) else {
            out.push(HostEvent::Compositor(event.clone()));
            return Ok(());
        };
        let Some(s) = self.surfaces.get_mut(&id) else {
            return Ok(());
        };
        if s.window.handle(event) {
            if let Event::Configure(c) = event {
                let before = s.view.ui.size();
                s.configure(c)?;
                let after = s.view.ui.size();
                if before != after {
                    out.push(HostEvent::Resized(id, after));
                }
            }
            if let Event::CloseRequested { .. } = event {
                out.push(HostEvent::Compositor(event.clone()));
            }
            return Ok(());
        }
        let Some((ui_event, response)) = s.input(event) else {
            out.push(HostEvent::Compositor(event.clone()));
            return Ok(());
        };
        out.extend(
            response
                .actions
                .into_iter()
                .map(|a| HostEvent::Action(id, a)),
        );
        if !response.consumed || matches!(ui_event, UiEvent::PointerDown { .. }) {
            unconsumed(id, s, ui_event, response.consumed, out);
        }
        Ok(())
    }
}

/// The surface an event is for.
fn surface_of(event: &Event) -> Option<SurfaceId> {
    Some(match event {
        Event::Configure(c) => c.surface,
        Event::FrameDone { surface, .. }
        | Event::PointerEnter { surface, .. }
        | Event::PointerLeave { surface }
        | Event::PointerMotion { surface, .. }
        | Event::PointerButton { surface, .. }
        | Event::PointerAxis { surface, .. }
        | Event::KeyboardFocus { surface, .. }
        | Event::Key { surface, .. }
        | Event::TouchDown { surface, .. }
        | Event::TouchMove { surface, .. }
        | Event::TouchUp { surface, .. }
        | Event::TouchCancel { surface }
        | Event::Text { surface, .. }
        | Event::CloseRequested { surface } => *surface,
        _ => return None,
    })
}

/// Keys the surface did not use go to the app; the menu key and a secondary press ask for
/// the context menu of the focused or pressed node.
fn unconsumed(
    id: SurfaceId,
    s: &HostSurface,
    ui_event: UiEvent,
    consumed: bool,
    out: &mut Vec<HostEvent>,
) {
    match ui_event {
        UiEvent::Key { key, modifiers } if !consumed => {
            if key == Key::Menu {
                if let Some(node) = s.view.ui.focused() {
                    out.push(HostEvent::Menu(id, node));
                }
            }
            out.push(HostEvent::Key(id, key, modifiers));
        }
        UiEvent::PointerDown {
            pos,
            button: PointerButton::Secondary,
        } => {
            if let Some(node) = s.view.ui.node_at(pos) {
                out.push(HostEvent::Menu(id, node));
            }
        }
        _ => {}
    }
}
