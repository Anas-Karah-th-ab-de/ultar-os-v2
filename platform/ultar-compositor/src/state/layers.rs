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

impl State {
    /// True while a lock screen shows: nothing else is drawn or gets input.
    pub fn locked(&self) -> bool {
        self.layers.iter().any(|k| {
            self.surface(*k)
                .is_some_and(|s| s.role == Role::Lock && s.image.is_some())
        })
    }

    /// Where a non-window surface goes (logical points), if it has content.
    pub fn layer_rect(&self, key: SurfaceKey) -> Option<Rect> {
        let s = self.surface(key)?;
        let (w, h) = s.logical_size()?;
        let output = self.layout.primary()?;
        let screen = output.logical_rect();
        let margin = Space::S16.points() as i32;
        let rect = match s.role {
            Role::Toplevel => return None,
            Role::Background | Role::Lock => Rect::new(screen.x, screen.y, w, h),
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
                let x = if self.rtl() {
                    screen.x + margin
                } else {
                    screen.max_x() - margin - w
                };
                Rect::new(x, screen.y + margin, w, h)
            }
            Role::Popup { parent, x, y } => {
                let parent = SurfaceKey {
                    client: key.client,
                    surface: parent,
                };
                let base = self
                    .window_index(parent)
                    .filter(|&i| self.windows[i].visible())
                    .map(|i| self.windows[i].content())?;
                let r = Rect::new(base.x + x, base.y + y, w, h);
                let bounds = self.layout.bounds();
                let cx = r.x.clamp(bounds.x, (bounds.max_x() - w).max(bounds.x));
                let cy = r.y.clamp(bounds.y, (bounds.max_y() - h).max(bounds.y));
                Rect::new(cx, cy, w, h)
            }
        };
        Some(rect)
    }

    /// Everything visible, bottom to top.
    pub fn scene(&self) -> Vec<Placed> {
        let locked = self.locked();
        let rank = |role: Role| match role {
            Role::Background => 0,
            Role::Panel { .. } => 2,
            Role::Popup { .. } => 3,
            Role::Overlay => 4,
            Role::InputPanel => 5,
            Role::Lock => 6,
            Role::Toplevel => 1,
        };
        let mut layered: Vec<(u8, Placed)> = Vec::new();
        for &key in &self.layers {
            let Some(s) = self.surface(key) else { continue };
            if locked && !matches!(s.role, Role::Lock | Role::InputPanel | Role::Background) {
                continue;
            }
            if let Some(content) = self.layer_rect(key) {
                layered.push((
                    rank(s.role),
                    Placed {
                        key,
                        content,
                        window: None,
                    },
                ));
            }
        }
        let mut out: Vec<Placed> = layered
            .iter()
            .filter(|(r, _)| *r == 0)
            .map(|(_, p)| *p)
            .collect();
        if !locked {
            for (i, w) in self.windows.iter().enumerate().filter(|(_, w)| w.visible()) {
                out.push(Placed {
                    key: w.key,
                    content: w.content(),
                    window: Some(i),
                });
            }
        }
        layered.sort_by_key(|(r, _)| *r);
        out.extend(layered.into_iter().filter(|(r, _)| *r > 0).map(|(_, p)| p));
        out
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
            Role::Background | Role::Lock => (screen.w, screen.h),
            Role::Panel { edge, size } => {
                let strip = panel_strip(screen, edge, size as i32, self.rtl());
                (strip.w, strip.h)
            }
            Role::InputPanel => (screen.w, screen.h * 2 / 5),
            Role::Overlay => (screen.w / 4, screen.h / 8),
            Role::Popup { .. } | Role::Toplevel => (0, 0),
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
