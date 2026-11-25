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

/// How far a press must travel to be reported as a swipe (logical points).
pub const SWIPE_MIN: f32 = 24.0;
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
    /// A primary press on nothing operable that was released elsewhere: the distance
    /// (swipe up on the phone's home screen or lock screen).
    Swipe(SurfaceId, ultar_ui::geom::Point),
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
    /// Where a primary press began that no node took.
    press: Option<(SurfaceId, ultar_ui::geom::Point)>,
    /// The kind of each surface's focused text field last told to the compositor.
    text_fields: BTreeMap<SurfaceId, u8>,
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
            press: None,
            text_fields: BTreeMap::new(),
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
        self.announce_text_fields()?;
        self.draw()?;
        Ok(out)
    }

    /// Tells the compositor when a surface's focus enters or leaves a text field (the
    /// on-screen keyboard follows it: the search field, the lock screen's password).
    fn announce_text_fields(&mut self) -> Result<(), ClientError> {
        use ultar_surface::ui::text_input;
        for (id, s) in &self.surfaces {
            let now = text_input::focused(&s.view.ui);
            if self.text_fields.get(id).copied() == now {
                continue;
            }
            match now {
                Some(kind) => self.text_fields.insert(*id, kind),
                None => self.text_fields.remove(id),
            };
            self.client.send(&Request::TextInput {
                surface: *id,
                enabled: now.is_some(),
                purpose: now.unwrap_or(text_input::TEXT),
            })?;
        }
        self.text_fields
            .retain(|id, _| self.surfaces.contains_key(id));
        Ok(())
    }

    /// Paints every surface that changed.
    pub fn draw(&mut self) -> Result<(), ClientError> {
        for s in self.surfaces.values_mut() {
            s.draw(&mut self.client)?;
        }
        Ok(())
    }

    /// Remembers the serial of the user's last press or key (activation tokens).
    fn note_serial(&mut self, event: &Event) {
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
    }

    fn handle(&mut self, event: &Event, out: &mut Vec<HostEvent>) -> Result<(), ClientError> {
        if let Event::BufferReleased { .. } = event {
            let _ = self.surfaces.values_mut().any(|s| s.window.handle(event));
            return Ok(());
        }
        if let Event::Key { modifiers, .. } = event {
            self.caps_lock = modifiers.has(ultar_surface::proto::Modifiers::CAPS_LOCK);
        }
        let target = surface_of(event);
        self.note_serial(event);
        let Some(id) = target.filter(|id| self.surfaces.contains_key(id)) else {
            out.push(HostEvent::Compositor(event.clone()));
            return Ok(());
        };
        let Some(s) = self.surfaces.get_mut(&id) else {
            return Ok(());
        };
        if let Event::Text { text, .. } = event {
            // Typed on the on-screen keyboard: into the focused field, like a paste.
            let actions = s.view.ui.paste(text);
            out.extend(actions.into_iter().map(|a| HostEvent::Action(id, a)));
            return Ok(());
        }
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
        let swipe = track_swipe(&mut self.press, id, &ui_event, response.consumed);
        out.extend(swipe);
        if !response.consumed || matches!(ui_event, UiEvent::PointerDown { .. }) {
            unconsumed(id, s, ui_event, response.consumed, out);
        }
        Ok(())
    }
}

/// Remembers where a primary press began that no node took, and reports the distance
/// when it is released at least [`SWIPE_MIN`] away on the same surface.
fn track_swipe(
    press: &mut Option<(SurfaceId, ultar_ui::geom::Point)>,
    id: SurfaceId,
    event: &UiEvent,
    consumed: bool,
) -> Option<HostEvent> {
    match *event {
        UiEvent::PointerDown {
            pos,
            button: PointerButton::Primary,
        } => {
            *press = (!consumed).then_some((id, pos));
            None
        }
        UiEvent::PointerUp { pos, .. } => {
            let (from, start) = press.take()?;
            let delta = ultar_ui::geom::Point::new(pos.x - start.x, pos.y - start.y);
            (from == id && delta.x.abs().max(delta.y.abs()) >= SWIPE_MIN)
                .then_some(HostEvent::Swipe(id, delta))
        }
        _ => None,
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
