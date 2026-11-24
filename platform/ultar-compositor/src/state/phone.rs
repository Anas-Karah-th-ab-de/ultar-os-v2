//! Phone mode (DESKTOP-SPEC §5.5, boards CompactHome/CompactApps): asked for by the shell
//! (`SetPhoneMode`, right `ui.shell`). Every app window has no title bar and fills the work
//! area (one maximised window at a time; "toggle maximise" does nothing), and the screen
//! edges belong to the shell: a touch or a pressed primary button that starts within
//! [`EDGE_ZONE`] of the bottom or a side edge of the primary output is kept from the apps and
//! ends as `EdgeSwipe` for the shell clients only – up from the bottom: home (held at the
//! end: recent apps), inwards from a side: back. Never while the session is locked (the lock
//! screen keeps every contact).

use super::{ClientId, State};
use crate::geom::Rect;
use ultar_surface::proto::{ErrorCode, Event, Rights, SwipeEdge, WindowState};

/// Width of the band along an edge where a gesture starts (logical points).
pub const EDGE_ZONE: f64 = 16.0;
/// Travel that makes a swipe (logical points).
pub const SWIPE: f64 = 48.0;
/// A finger that rested this long at the end of a swipe up "held" (ms).
pub const HOLD_MS: u64 = 400;
/// Movement below this counts as resting (logical points).
const REST: f64 = 8.0;

/// What started a gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    /// A touch contact (its slot).
    Touch(u32),
    /// The pointer with the primary button held.
    Pointer,
}

/// A gesture under way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Track {
    contact: Contact,
    edge: SwipeEdge,
    /// Started at the side of the smaller x (inwards = growing x).
    from_origin: bool,
    start: (f64, f64),
    /// Where and when it last moved more than [`REST`].
    rest: (f64, f64, u64),
}

/// Phone mode and the gesture under way.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Phone {
    /// Phone mode is on.
    pub on: bool,
    track: Option<Track>,
}

impl State {
    /// `SetPhoneMode` of the shell: windows without title bars over the work area, edge
    /// gestures for the shell (or everything back as on a desktop).
    pub(super) fn set_phone_mode(&mut self, client: ClientId, on: bool) -> Result<(), ErrorCode> {
        let rights = self
            .clients
            .get(&client)
            .ok_or(ErrorCode::UnknownObject)?
            .rights;
        if !rights.contains(Rights::SHELL) {
            return Err(ErrorCode::Permission);
        }
        if self.session.shell.phone.on == on {
            return Ok(());
        }
        self.session.shell.phone = Phone { on, track: None };
        for i in 0..self.windows.len() {
            self.phone_window(i);
        }
        Ok(())
    }

    /// Puts window `i` into the phone's shape (or back), unless it is a system dialog.
    pub(super) fn phone_window(&mut self, i: usize) {
        let Some(w) = self.windows.get_mut(i).filter(|w| !w.secure) else {
            return;
        };
        w.place.bare = self.session.shell.phone.on;
        let (state, minimized) = (w.state, w.state == WindowState::Minimized);
        if self.session.shell.phone.on && !minimized && state != WindowState::Fullscreen {
            self.set_window_state(i, WindowState::Maximized, None);
        } else if !self.session.shell.phone.on && state == WindowState::Maximized {
            self.set_window_state(i, WindowState::Normal, None);
        } else if let Some(frame) = self.windows.get(i).map(|w| w.frame) {
            self.configure_window(i, frame);
        }
    }

    /// The primary output's rectangle (logical).
    fn phone_screen(&self) -> Option<Rect> {
        self.layout.primary().map(|o| o.logical_rect())
    }

    /// A contact went down at (`x`, `y`): `true` when it starts an edge gesture (the apps
    /// do not see it).
    pub(super) fn edge_begin(&mut self, contact: Contact, x: f64, y: f64) -> bool {
        if !self.session.shell.phone.on || self.locked() || self.session.shell.phone.track.is_some()
        {
            return false;
        }
        if self.clients_with_shell().is_empty() {
            return false;
        }
        let Some(r) = self.phone_screen() else {
            return false;
        };
        // Screen coordinates (the edges are physical; "back" works from either side).
        let (x0, x1, bottom) = (f64::from(r.x), f64::from(r.x + r.w), f64::from(r.y + r.h));
        let inside = x >= x0 && x < x1 && y >= f64::from(r.y) && y < bottom;
        let edge = if !inside {
            return false;
        } else if y >= bottom - EDGE_ZONE {
            SwipeEdge::Bottom
        } else if x < x0 + EDGE_ZONE || x >= x1 - EDGE_ZONE {
            SwipeEdge::Side
        } else {
            return false;
        };
        self.session.shell.phone.track = Some(Track {
            contact,
            edge,
            from_origin: x < x0 + EDGE_ZONE,
            start: (x, y),
            rest: (x, y, self.now),
        });
        true
    }

    /// The contact moved: `true` when it belongs to the gesture.
    pub(super) fn edge_move(&mut self, contact: Contact, x: f64, y: f64) -> bool {
        let now = self.now;
        match self.session.shell.phone.track.as_mut() {
            Some(t) if t.contact == contact => {
                if (x - t.rest.0).hypot(y - t.rest.1) > REST {
                    t.rest = (x, y, now);
                }
                true
            }
            _ => false,
        }
    }

    /// The contact ended: `true` when it was the gesture's (a far enough swipe becomes
    /// `EdgeSwipe` for the shell; a shorter one ends without effect).
    pub(super) fn edge_end(&mut self, contact: Contact) -> bool {
        let Some(t) = self
            .session
            .shell
            .phone
            .track
            .filter(|t| t.contact == contact)
        else {
            return false;
        };
        self.session.shell.phone.track = None;
        let (dx, dy) = (t.rest.0 - t.start.0, t.rest.1 - t.start.1);
        let swiped = match t.edge {
            SwipeEdge::Bottom => -dy >= SWIPE,
            SwipeEdge::Side if t.from_origin => dx >= SWIPE,
            SwipeEdge::Side => -dx >= SWIPE,
        };
        if swiped && !self.locked() {
            let held = t.edge == SwipeEdge::Bottom && self.now.saturating_sub(t.rest.2) >= HOLD_MS;
            for client in self.clients_with_shell() {
                self.send(client, Event::EdgeSwipe { edge: t.edge, held });
            }
        }
        true
    }

    /// Every contact was cancelled.
    pub(super) fn edge_cancel(&mut self) {
        self.session.shell.phone.track = None;
    }

    /// The back gesture for a window: Escape pressed and released in its app.
    pub(super) fn window_back(&mut self, key: super::SurfaceKey) {
        const ESC: u16 = 1;
        let modifiers = ultar_surface::proto::Modifiers(0);
        let serial = self.user_serial(key.client);
        self.deliver_key(
            key,
            ESC,
            1,
            modifiers,
            (crate::keymap::Output::None, serial),
        );
        let serial = self.next_serial();
        self.deliver_key(
            key,
            ESC,
            0,
            modifiers,
            (crate::keymap::Output::None, serial),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_off() {
        let p = Phone::default();
        assert!(!p.on && p.track.is_none());
        const { assert!(EDGE_ZONE < SWIPE) };
    }
}
