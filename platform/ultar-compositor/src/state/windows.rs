//! Toplevel windows: mapping, configure, states, snapping, focus.

use super::{ClientId, State, SurfaceKey};
use crate::decor::{Button, TitleKey, TITLEBAR};
use crate::geom::Rect;
use crate::pixels::Image;
use crate::wm::{self, Snap};
use ultar_surface::proto::{Configure, Event, OutputId, Role, WindowState};

/// Share of the work area a new window gets when the app has no wish.
const NEW_WINDOW_SHARE: (i32, i32) = (3, 5);

/// A toplevel window.
#[derive(Debug)]
pub struct Window {
    /// Its surface.
    pub key: SurfaceKey,
    /// Frame (title bar + content) in logical points.
    pub frame: Rect,
    /// State.
    pub state: WindowState,
    /// State before minimising.
    pub before_minimize: WindowState,
    /// Snap position (normal state only).
    pub snap: Option<Snap>,
    /// Frame to return to from maximised/snapped/fullscreen.
    pub restore: Rect,
    /// Geometry waiting for the client to draw it: (serial, frame).
    pub pending: Option<(u32, Rect)>,
    /// Shown (has content).
    pub mapped: bool,
    /// The app does not answer pings.
    pub unresponsive: bool,
    /// Title bar buttons under the pointer and held.
    pub buttons: TitleButtons,
    /// Rendered title bar and what it was rendered for.
    pub title_cache: Option<(TitleKey, Image)>,
    /// A system dialog (trusted path): stacked above the panels, modal,
    /// marked with the lock icon in a title bar apps cannot cover.
    pub secure: bool,
    /// Focus when mapped, if the grant is still valid then (an activation
    /// token was presented before the window had content).
    pub activate_on_map: Option<super::Grant>,
    /// Id in the shell's window list (never reused in the session).
    pub id: u32,
    /// Its workspace and whether that one is shown.
    pub place: Place,
}

/// The title bar buttons a window shows as hovered or held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleButtons {
    /// Under the pointer.
    pub hover: Option<Button>,
    /// Held.
    pub pressed: Option<Button>,
}

/// Where a window lives among the workspaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// Its workspace, or `ALL_WORKSPACES`.
    pub workspace: u8,
    /// Its workspace is the one shown (always for `ALL_WORKSPACES`).
    pub shown: bool,
    /// Phone mode: no title bar.
    pub bare: bool,
}

impl Window {
    fn new(key: SurfaceKey, id: u32, workspace: u8) -> Window {
        Window {
            id,
            key,
            frame: Rect::default(),
            state: WindowState::Normal,
            before_minimize: WindowState::Normal,
            snap: None,
            restore: Rect::default(),
            pending: None,
            mapped: false,
            unresponsive: false,
            buttons: TitleButtons::default(),
            title_cache: None,
            secure: false,
            activate_on_map: None,
            place: Place {
                workspace,
                shown: true,
                bare: false,
            },
        }
    }

    /// Has compositor decorations (not fullscreen, not in phone mode).
    pub fn decorated(&self) -> bool {
        self.state != WindowState::Fullscreen && !self.place.bare
    }

    /// Visible on screen.
    pub fn visible(&self) -> bool {
        self.mapped && self.state != WindowState::Minimized && self.place.shown
    }

    /// The content rectangle (logical).
    pub fn content(&self) -> Rect {
        content_of(self.frame, self.decorated())
    }

    /// Rounded corners and shadow (normal, not snapped).
    pub fn floating(&self) -> bool {
        self.state == WindowState::Normal && self.snap.is_none()
    }

    /// Everything the window paints, shadow included (logical).
    pub fn painted_area(&self) -> Rect {
        if !self.floating() {
            return self.frame;
        }
        let blur = ultar_ui::tokens::shadow::BLUR.ceil() as i32;
        let off = ultar_ui::tokens::shadow::OFFSET_Y.ceil() as i32;
        Rect::new(
            self.frame.x - blur,
            self.frame.y - blur,
            self.frame.w + 2 * blur,
            self.frame.h + 2 * blur + off,
        )
    }
}

/// Content of a frame.
pub fn content_of(frame: Rect, decorated: bool) -> Rect {
    if decorated {
        Rect::new(
            frame.x,
            frame.y + TITLEBAR,
            frame.w,
            (frame.h - TITLEBAR).max(1),
        )
    } else {
        frame
    }
}

impl State {
    /// Index of the window of a surface.
    pub fn window_index(&self, key: SurfaceKey) -> Option<usize> {
        self.windows.iter().position(|w| w.key == key)
    }

    /// The output a window is on (most of its frame), else the primary.
    pub fn window_output(&self, frame: Rect) -> Option<OutputId> {
        let best = self
            .layout
            .enabled()
            .map(|o| {
                (
                    o.id,
                    frame.intersect(&o.logical_rect()).map_or(0, |r| r.area()),
                )
            })
            .max_by_key(|&(_, area)| area)
            .filter(|&(_, area)| area > 0)
            .map(|(id, _)| id);
        best.or_else(|| self.layout.primary().map(|o| o.id))
    }

    /// The area windows may use on an output (minus panels).
    pub fn work_area(&self, output: OutputId) -> Rect {
        let Some(o) = self.layout.get(output) else {
            return Rect::new(0, 0, 1, 1);
        };
        let mut area = o.logical_rect();
        if !o.primary {
            return area;
        }
        for key in &self.layers {
            if let Some((edge, size)) = self.surface(*key).and_then(|s| match s.role {
                Role::Panel { edge, size } if s.image.is_some() => Some((edge, size as i32)),
                _ => None,
            }) {
                area = layers::reserve(area, edge, size, self.rtl());
            }
        }
        area
    }

    pub(super) fn create_toplevel(&mut self, key: SurfaceKey) {
        let secure = self
            .surface(key)
            .is_some_and(|s| s.role == Role::SystemDialog);
        let id = self.next_window_id();
        let mut window = Window::new(key, id, self.new_window_workspace(secure));
        window.secure = secure;
        self.windows.push(window);
        let Some(output) = self.layout.primary().map(|o| o.id) else {
            return;
        };
        let work = self.work_area(output);
        let size = (
            work.w * NEW_WINDOW_SHARE.0 / NEW_WINDOW_SHARE.1,
            (work.h - TITLEBAR) * NEW_WINDOW_SHARE.0 / NEW_WINDOW_SHARE.1,
        );
        let limits = self.surface(key).map(|s| s.limits).unwrap_or_default();
        let (w, h) = wm::clamp_size(size.0, size.1, limits.min, limits.max, work);
        let frame = if secure {
            let (fw, fh) = (w / 2, h / 2 + TITLEBAR);
            Rect::new(
                work.x + (work.w - fw) / 2,
                work.y + (work.h - fh) / 2,
                fw,
                fh,
            )
        } else {
            wm::place_new(work, w, h, TITLEBAR, self.windows.len().saturating_sub(1))
        };
        if let Some(i) = self.window_index(key) {
            self.windows[i].pending = None;
            self.windows[i].frame = frame;
            self.windows[i].restore = frame;
            self.configure_window(i, frame);
            if self.session.shell.phone.on && !secure {
                self.phone_window(i);
            }
        }
    }

    /// Sends a configure for the window at `index` asking for `frame`.
    pub(super) fn configure_window(&mut self, index: usize, frame: Rect) {
        let Some(w) = self.windows.get(index) else {
            return;
        };
        let (key, state) = (w.key, w.state);
        let content = content_of(frame, w.decorated());
        let scale = self
            .window_output(frame)
            .and_then(|o| self.layout.get(o))
            .map_or(1000, |o| o.scale_milli);
        let activated = self.focus == Some(key);
        let rtl = self.rtl();
        let serial = self.next_serial();
        if let Some(s) = self.surface_mut(key) {
            s.configure_sent = serial;
            s.configure_scales.push((serial, scale));
            if s.configure_scales.len() > 16 {
                s.configure_scales.remove(0);
            }
        }
        self.windows[index].pending = Some((serial, frame));
        let event = Event::Configure(Configure {
            surface: key.surface,
            serial,
            width: content.w.max(1) as u32,
            height: content.h.max(1) as u32,
            scale_milli: scale,
            state,
            activated,
            rtl,
        });
        self.send(key.client, event);
    }

    /// After a commit of a toplevel: map it, or adopt the geometry the
    /// client acknowledged. Returns the content origin for damage.
    pub(super) fn window_committed(&mut self, key: SurfaceKey) -> Option<Rect> {
        let index = self.window_index(key)?;
        let (size, acked) = {
            let s = self.surface(key)?;
            (s.logical_size(), s.configure_acked)
        };
        let Some((cw, ch)) = size else {
            // Detached: unmap.
            let area = self.windows[index].painted_area();
            self.windows[index].mapped = false;
            self.damage_logical(area);
            if self.windows[index].secure {
                self.damage_all();
            }
            return None;
        };
        let before = self.windows[index].painted_area();
        let was_mapped = self.windows[index].mapped;
        let w = &mut self.windows[index];
        let mut origin = (w.frame.x, w.frame.y);
        if let Some((serial, target)) = w.pending {
            if acked >= serial {
                origin = (target.x, target.y);
                w.pending = None;
            }
        }
        let title = if w.decorated() { TITLEBAR } else { 0 };
        w.frame = Rect::new(origin.0, origin.1, cw, ch + title);
        w.mapped = true;
        if w.floating() && w.state == WindowState::Normal {
            w.restore = w.frame;
        }
        let after = w.painted_area();
        if was_mapped && before != after {
            self.damage_logical(before);
        }
        if !was_mapped {
            self.damage_logical(after);
            self.window_mapped(index);
            return None;
        }
        if before != after {
            self.damage_logical(after);
            return None;
        }
        Some(self.windows[index].content())
    }

    /// A window appeared: system dialogs and windows of the app the user
    /// works with (or with an activation token) get the focus; any other
    /// window opens inactive (keys stay where the user typed).
    fn window_mapped(&mut self, index: usize) {
        let w = &mut self.windows[index];
        let (key, secure) = (w.key, w.secure);
        let grant = w.activate_on_map.take();
        let token = grant.is_some_and(|g| self.session.valid(&g, self.now));
        if secure {
            self.damage_all();
        }
        if secure || token || self.may_take_focus(key.client) {
            self.focus_window(key);
        }
    }

    /// Raises a window and gives it the keyboard focus.
    pub fn focus_window(&mut self, key: SurfaceKey) {
        if let Some(i) = self.window_index(key) {
            let w = self.windows.remove(i);
            let area = w.painted_area();
            self.windows.push(w);
            self.damage_logical(area);
        }
        self.set_focus(Some(key));
    }

    /// Moves the keyboard focus (sends focus events, re-renders the title
    /// bars, offers the clipboard to a newly focused client).
    pub fn set_focus(&mut self, key: Option<SurfaceKey>) {
        if self.focus == key {
            return;
        }
        let old = self.focus;
        self.focus = key;
        self.session.focus_serial = self.next_serial();
        for (k, focused) in [(old, false), (key, true)] {
            let Some(k) = k else { continue };
            self.send(
                k.client,
                Event::KeyboardFocus {
                    surface: k.surface,
                    focused,
                },
            );
            if let Some(i) = self.window_index(k) {
                let frame = self.windows[i]
                    .pending
                    .map_or(self.windows[i].frame, |p| p.1);
                self.configure_window(i, frame);
                let title = Rect::new(frame.x, frame.y, frame.w, TITLEBAR);
                self.damage_logical(title);
            }
        }
        if let Some(k) = key {
            if old.map(|o| o.client) != Some(k.client) {
                let mimes = self.transfers.selection_mimes();
                self.send(k.client, Event::SelectionOffer { mimes });
            }
        }
        self.update_ime();
    }

    /// Focuses the topmost visible window (after the focused one went).
    pub(super) fn focus_topmost(&mut self) {
        let next = self
            .windows
            .iter()
            .rev()
            .find(|w| w.visible())
            .map(|w| w.key);
        self.set_focus(next);
    }

    /// Changes the state of a window (the compositor decides; the client
    /// draws the new size when it gets the configure).
    pub fn set_window_state(&mut self, index: usize, state: WindowState, snap: Option<Snap>) {
        self.set_state_by(index, state, snap, true);
    }

    /// Changes the state; `by_user` is false for an app's own request (a
    /// restored window then gets the focus only if it may take it). System
    /// dialogs stay as they are.
    pub(super) fn set_state_by(
        &mut self,
        index: usize,
        state: WindowState,
        snap: Option<Snap>,
        by_user: bool,
    ) {
        let Some(w) = self.windows.get(index).filter(|w| !w.secure) else {
            return;
        };
        let (key, old_state, frame) = (w.key, w.state, w.frame);
        if state == WindowState::Minimized {
            let area = w.painted_area();
            self.windows[index].before_minimize = old_state;
            self.windows[index].state = WindowState::Minimized;
            self.damage_logical(area);
            if self.focus == Some(key) {
                self.focus_topmost();
            }
            return;
        }
        let Some(output) = self.window_output(frame) else {
            return;
        };
        let w = &mut self.windows[index];
        if w.floating() && old_state == WindowState::Normal {
            w.restore = frame;
        }
        let area = w.painted_area();
        w.state = state;
        w.snap = if state == WindowState::Normal {
            snap
        } else {
            None
        };
        // The look changes at once (shadow, corners, title bar), the size
        // only when the app has drawn it.
        let restyled = w.painted_area();
        self.damage_logical(restyled);
        let target = match (state, snap) {
            (WindowState::Maximized, _) => self.work_area(output),
            (WindowState::Fullscreen, _) => {
                self.layout.get(output).map_or(frame, |o| o.logical_rect())
            }
            (_, Some(s)) => wm::snap_rect(s, self.work_area(output)),
            _ => self.windows[index].restore,
        };
        self.damage_logical(area);
        self.configure_window(index, target);
        if old_state == WindowState::Minimized && (by_user || self.may_take_focus(key.client)) {
            self.focus_window(key);
        }
    }

    /// Restores a minimised window (dock click, Alt+Tab).
    pub fn unminimize(&mut self, index: usize) {
        if let Some(w) = self.windows.get(index) {
            if w.state == WindowState::Minimized {
                let (state, snap) = (w.before_minimize, w.snap);
                self.set_window_state(index, state, snap);
            }
        }
    }

    /// Marks the windows of a client as (not) responding.
    pub(super) fn set_unresponsive(&mut self, client: ClientId, unresponsive: bool) {
        let mut changed = Vec::new();
        for w in self.windows.iter_mut().filter(|w| w.key.client == client) {
            if w.unresponsive != unresponsive {
                w.unresponsive = unresponsive;
                changed.push(Rect::new(w.frame.x, w.frame.y, w.frame.w, TITLEBAR));
            }
        }
        for r in changed {
            self.damage_logical(r);
        }
    }

    /// Asks the app to close the window (title bar close, Alt+F4).
    pub fn request_close(&mut self, key: SurfaceKey) {
        self.send(
            key.client,
            Event::CloseRequested {
                surface: key.surface,
            },
        );
    }

    /// Removes a surface (window or layer) and its damage.
    pub(super) fn destroy_surface(&mut self, key: SurfaceKey) {
        if let Some(i) = self.window_index(key) {
            let w = self.windows.remove(i);
            if w.visible() {
                self.damage_logical(w.painted_area());
            }
            if w.secure {
                self.damage_all();
            }
        }
        if let Some(area) = self.layer_rect(key) {
            self.damage_logical(area);
        }
        self.layers.retain(|k| *k != key);
        self.shell_surface_gone(key);
        let charge = self.surface(key).map_or(0, |s| s.charge());
        if let Some(c) = self.clients.get_mut(&key.client) {
            c.surfaces.remove(&key.surface);
            c.bytes = c.bytes.saturating_sub(charge);
        }
        if self.pointer.focus.is_some_and(|f| f.key == key) {
            self.pointer.focus = None;
        }
        if self.focus == Some(key) {
            self.focus = None;
            self.focus_topmost();
        }
    }
}

use super::layers;
