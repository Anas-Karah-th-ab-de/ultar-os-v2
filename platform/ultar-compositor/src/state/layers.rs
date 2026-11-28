//! Surfaces that are not windows: background, panels (shell bar, dock),
//! popups, overlays, on-screen keyboard, lock screen – where they go and
//! in which order everything is stacked.

use super::{State, SurfaceKey};
use crate::geom::Rect;
use ultar_surface::proto::{Configure, Edge, Event, Role, WindowState};
use ultar_ui::tokens::Space;

/// One visible surface, bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The surface.
    pub key: SurfaceKey,
    /// Where its content goes (logical points).
    pub content: Rect,
    /// Index into the window list for toplevels.
    pub window: Option<usize>,
}

/// The area left after a panel reserves `size` points at `edge`.
pub fn reserve(area: Rect, edge: Edge, size: i32, rtl: bool) -> Rect {
    let size = size.clamp(0, area.w.min(area.h));
    let start_is_low_x = !rtl;
    match edge {
        Edge::Top => Rect::new(area.x, area.y + size, area.w, area.h - size),
        Edge::Bottom => Rect::new(area.x, area.y, area.w, area.h - size),
        Edge::Start | Edge::End => {
            let low_x = (edge == Edge::Start) == start_is_low_x;
            if low_x {
                Rect::new(area.x + size, area.y, area.w - size, area.h)
            } else {
                Rect::new(area.x, area.y, area.w - size, area.h)
            }
        }
    }
}

/// The strip a panel occupies.
fn panel_strip(area: Rect, edge: Edge, size: i32, rtl: bool) -> Rect {
    let rest = reserve(area, edge, size, rtl);
    match edge {
        Edge::Top => Rect::new(area.x, area.y, area.w, rest.y - area.y),
        Edge::Bottom => Rect::new(area.x, rest.max_y(), area.w, area.max_y() - rest.max_y()),
        _ if rest.x > area.x => Rect::new(area.x, area.y, rest.x - area.x, area.h),
        _ => Rect::new(rest.max_x(), area.y, area.max_x() - rest.max_x(), area.h),
    }
}

/// Share of the screen height the call screen gets while locked: a band at
/// the top. Everything below – the lock screen's entry area (PIN pad,
/// password) – stays the lock's, for drawing and for pointer and touch, so a
/// call screen can never draw a copy of the PIN pad and harvest the taps
/// (security review S-5).
pub const CALL_BAND_PERCENT: i32 = 30;

/// The band the call screen gets while locked.
fn call_band(screen: Rect) -> Rect {
    Rect::new(
        screen.x,
        screen.y,
        screen.w,
        screen.h * CALL_BAND_PERCENT / 100,
    )
}

/// Stacking rank of non-window surfaces above the windows. The call screen is above the
/// lock screen only while locked (a ringing call is answered without unlocking); unlocked
/// it stays below system dialogs, so it can never hide a permission or password prompt
/// (security review S-5).
fn rank(role: Role, locked: bool) -> u8 {
    match role {
        Role::CallScreen if !locked => 3,
        Role::Panel { .. } => 2,
        Role::Overlay => 3,
        Role::SystemDialog => 4,
        Role::InputPanel => 5,
        Role::Lock => 6,
        Role::CallScreen => 7,
        Role::Background | Role::Popup { .. } | Role::Toplevel => 0,
    }
}

impl State {
    /// Where a non-window surface goes (logical points), if it has content.
    pub fn layer_rect(&self, key: SurfaceKey) -> Option<Rect> {
        let s = self.surface(key)?;
        let (w, h) = s.logical_size()?;
        let output = self.layout.primary()?;
        let screen = output.logical_rect();
        let margin = Space::S16.points() as i32;
        let rect = match s.role {
            Role::Toplevel | Role::SystemDialog => return None,
            Role::CallScreen if self.locked() => {
                let band = call_band(screen);
                Rect::new(band.x, band.y, w.min(band.w), h.min(band.h))
            }
            Role::Background | Role::Lock | Role::CallScreen => Rect::new(screen.x, screen.y, w, h),
            Role::Panel { edge, size } => {
                let strip = panel_strip(screen, edge, size as i32, self.rtl());
                Rect::new(
                    strip.x,
                    strip.y,
                    w.min(strip.w.max(w)),
                    h.min(strip.h.max(h)),
                )
            }
            Role::InputPanel => Rect::new(screen.x, screen.max_y() - h, w, h),
            Role::Overlay => {
                // Notifications: at most a third of the screen each way.
                let (w, h) = (w.min(screen.w / 3), h.min(screen.h / 3));
                let x = if self.rtl() {
                    screen.x + margin
                } else {
                    screen.max_x() - margin - w
                };
                Rect::new(x, screen.y + margin, w, h)
            }
            Role::Popup { parent, x, y } => self.popup_rect(key, parent, (x, y), (w, h))?,
        };
        Some(rect)
    }

    /// A popup stays inside its parent's content (and the work area of the
    /// parent's output): it can neither cover the shell's panels, other
    /// apps' windows nor reach another display. A popup larger than that is
    /// shrunk to it.
    fn popup_rect(
        &self,
        key: SurfaceKey,
        parent: ultar_surface::proto::SurfaceId,
        (x, y): (i32, i32),
        (w, h): (i32, i32),
    ) -> Option<Rect> {
        let parent = SurfaceKey {
            client: key.client,
            surface: parent,
        };
        if self.layers.contains(&parent) {
            return self.panel_popup_rect(parent, (x, y), (w, h));
        }
        let i = self
            .window_index(parent)
            .filter(|&i| self.windows[i].visible())?;
        let base = self.windows[i].content();
        let work = self
            .window_output(self.windows[i].frame)
            .map_or(self.layout.bounds(), |o| self.work_area(o));
        let area = base.intersect(&work).unwrap_or(base);
        let (w, h) = (w.min(area.w).max(1), h.min(area.h).max(1));
        let cx = base
            .x
            .saturating_add(x)
            .clamp(area.x, (area.max_x() - w).max(area.x));
        let cy = base
            .y
            .saturating_add(y)
            .clamp(area.y, (area.max_y() - h).max(area.y));
        Some(Rect::new(cx, cy, w, h))
    }

    /// Everything visible, bottom to top.
    pub fn scene(&self) -> Vec<Placed> {
        self.scene_parts().0
    }

    /// The scene and the translucent surfaces the overdraw budget left out.
    /// Order: backgrounds, each window followed by its popups, panels,
    /// system dialogs, overlays, on-screen keyboard, lock screen, call screen.
    /// While locked only the lock screen, the on-screen keyboard and the call
    /// screen (a ringing call is answered without unlocking).
    pub fn scene_parts(&self) -> (Vec<Placed>, Vec<SurfaceKey>) {
        let locked = self.locked();
        let mut out = Vec::new();
        let mut above: Vec<(u8, Placed)> = Vec::new();
        let mut popups: std::collections::HashMap<SurfaceKey, Vec<Placed>> = Default::default();
        for &key in &self.layers {
            let Some(s) = self.surface(key) else { continue };
            if locked && !matches!(s.role, Role::Lock | Role::InputPanel | Role::CallScreen) {
                continue;
            }
            let Some(content) = self.layer_rect(key) else {
                continue;
            };
            let placed = Placed {
                key,
                content,
                window: None,
            };
            match s.role {
                Role::Background => out.push(placed),
                Role::Popup { parent, .. } => popups
                    .entry(SurfaceKey {
                        client: key.client,
                        surface: parent,
                    })
                    .or_default()
                    .push(placed),
                role => above.push((rank(role, locked), placed)),
            }
        }
        if !locked {
            for (i, w) in self.windows.iter().enumerate().filter(|(_, w)| w.visible()) {
                let placed = Placed {
                    key: w.key,
                    content: w.content(),
                    window: Some(i),
                };
                if w.secure {
                    above.push((rank(Role::SystemDialog, locked), placed));
                    continue;
                }
                out.push(placed);
                out.extend(popups.remove(&w.key).unwrap_or_default());
            }
        }
        above.sort_by_key(|(r, _)| *r);
        for (_, placed) in above {
            // A shell panel's popups (sheets, menus) right above it.
            let own = popups.remove(&placed.key).unwrap_or_default();
            out.push(placed);
            out.extend(own);
        }
        self.cull(out)
    }

    /// Damages everything when the overdraw budget leaves out other
    /// surfaces than at the last frame.
    pub fn refresh_culled(&mut self) {
        let (_, culled) = self.scene_parts();
        if culled != self.session.culled {
            self.session.culled = culled;
            self.damage_all();
        }
    }

    /// Sends every call screen its size again (the session was locked or
    /// unlocked: band or full screen).
    pub(super) fn configure_call_screens(&mut self) {
        let calls: Vec<SurfaceKey> = self
            .layers
            .iter()
            .copied()
            .filter(|&k| self.surface(k).is_some_and(|s| s.role == Role::CallScreen))
            .collect();
        for key in calls {
            self.configure_layer(key);
        }
    }

    /// Sends the size a layer surface should have.
    pub(super) fn configure_layer(&mut self, key: SurfaceKey) {
        let Some(role) = self.surface(key).map(|s| s.role) else {
            return;
        };
        let Some(output) = self.layout.primary() else {
            return;
        };
        let screen = output.logical_rect();
        let scale = output.scale_milli;
        let (w, h) = match role {
            Role::CallScreen if self.locked() => {
                let band = call_band(screen);
                (band.w, band.h)
            }
            Role::Background | Role::Lock | Role::CallScreen => (screen.w, screen.h),
            Role::Panel { edge, size } => {
                let strip = panel_strip(screen, edge, size as i32, self.rtl());
                (strip.w, strip.h)
            }
            Role::InputPanel => (screen.w, screen.h * 2 / 5),
            Role::Overlay => (screen.w / 4, screen.h / 8),
            Role::Popup { .. } | Role::Toplevel | Role::SystemDialog => (0, 0),
        };
        let serial = self.next_serial();
        let rtl = self.rtl();
        if let Some(s) = self.surface_mut(key) {
            s.configure_sent = serial;
            s.configure_scales.push((serial, scale));
            if s.configure_scales.len() > 16 {
                s.configure_scales.remove(0);
            }
        }
        let event = Event::Configure(Configure {
            surface: key.surface,
            serial,
            width: w.max(0) as u32,
            height: h.max(0) as u32,
            scale_milli: scale,
            state: WindowState::Normal,
            activated: false,
            rtl,
        });
        self.send(key.client, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_reserve_logical_edges() {
        let a = Rect::new(0, 0, 1000, 800);
        assert_eq!(
            reserve(a, Edge::Bottom, 56, false),
            Rect::new(0, 0, 1000, 744)
        );
        assert_eq!(
            reserve(a, Edge::Top, 40, false),
            Rect::new(0, 40, 1000, 760)
        );
        assert_eq!(
            reserve(a, Edge::Start, 60, false),
            Rect::new(60, 0, 940, 800)
        );
        assert_eq!(reserve(a, Edge::Start, 60, true), Rect::new(0, 0, 940, 800));
        assert_eq!(reserve(a, Edge::End, 60, true), Rect::new(60, 0, 940, 800));
        assert_eq!(reserve(a, Edge::Bottom, 5000, false).h, 0);
        assert_eq!(
            panel_strip(a, Edge::Bottom, 56, false),
            Rect::new(0, 744, 1000, 56)
        );
        assert_eq!(
            panel_strip(a, Edge::Top, 40, false),
            Rect::new(0, 0, 1000, 40)
        );
        assert_eq!(
            panel_strip(a, Edge::Start, 60, true),
            Rect::new(940, 0, 60, 800)
        );
        assert_eq!(
            panel_strip(a, Edge::End, 60, true),
            Rect::new(0, 0, 60, 800)
        );
    }
}
