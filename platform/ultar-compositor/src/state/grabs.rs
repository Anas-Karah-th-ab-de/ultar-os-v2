//! Pointer grabs: moving and resizing windows, snapping on release,
//! title bar buttons and double clicks.

use super::input::PointerGrab;
use super::{State, SurfaceKey};
use crate::decor::{self, Button, Part};
use crate::geom::Rect;
use crate::wm::{self, SnapTarget};
use ultar_surface::proto::WindowState;

impl State {
    pub(super) fn title_double_click(&mut self, key: SurfaceKey) -> bool {
        let now = self.now;
        let window = u64::from(self.pointer.double_click_ms);
        let double = self
            .pointer
            .last_title
            .is_some_and(|(k, t)| k == key && now.saturating_sub(t) <= window);
        self.pointer.last_title = if double { None } else { Some((key, now)) };
        if double {
            if let Some(i) = self.window_index(key) {
                let maximized = self.windows[i].state == WindowState::Maximized;
                let target = if maximized {
                    WindowState::Normal
                } else {
                    WindowState::Maximized
                };
                self.set_window_state(i, target, None);
            }
        }
        double
    }

    pub(super) fn resize_grab(&mut self, key: SurfaceKey, edges: u8) -> PointerGrab {
        let frame = self
            .window_index(key)
            .map_or(Rect::default(), |i| self.windows[i].frame);
        PointerGrab::Resize {
            key,
            edges,
            start: (self.pointer.x, self.pointer.y),
            frame,
        }
    }

    pub(super) fn begin_move(&mut self, key: SurfaceKey) {
        self.begin_move_at(key, None);
    }

    /// Starts moving a window; `anchor` is the point that was grabbed (the
    /// press position, so the motion up to the drag threshold is kept).
    pub(super) fn begin_move_at(&mut self, key: SurfaceKey, anchor: Option<(f64, f64)>) {
        let Some(i) = self.window_index(key) else {
            return;
        };
        let restored =
            self.windows[i].state != WindowState::Normal || self.windows[i].snap.is_some();
        if restored {
            // Dragging a maximised or snapped window restores it under the pointer.
            let restore = self.windows[i].restore;
            let frame = self.windows[i].frame;
            let share = (self.pointer.x - f64::from(frame.x)) / f64::from(frame.w.max(1));
            let x = self.pointer.x - share * f64::from(restore.w);
            let target = Rect::new(
                x as i32,
                self.pointer.y as i32 - decor::TITLEBAR / 2,
                restore.w,
                restore.h,
            );
            self.windows[i].restore = target;
            self.set_window_state(i, WindowState::Normal, None);
        }
        let f = self.windows[i]
            .pending
            .map_or(self.windows[i].frame, |p| p.1);
        let pointer = (self.pointer.x, self.pointer.y);
        let (ax, ay) = if restored {
            pointer
        } else {
            anchor.unwrap_or(pointer)
        };
        let offset = (ax - f64::from(f.x), ay - f64::from(f.y));
        self.pointer.grab = Some(PointerGrab::Move { key, offset });
        if anchor.is_some_and(|a| a != pointer) && !restored {
            self.drag_window(key, offset);
        }
    }

    pub(super) fn drag_window(&mut self, key: SurfaceKey, offset: (f64, f64)) {
        let Some(i) = self.window_index(key) else {
            return;
        };
        let old_area = self.windows[i].painted_area();
        let (x, y) = (
            (self.pointer.x - offset.0) as i32,
            (self.pointer.y - offset.1) as i32,
        );
        let output = self
            .layout
            .output_at(self.pointer.x as i32, self.pointer.y as i32)
            .map(|o| o.id);
        let work = output.map_or(self.layout.bounds(), |o| self.work_area(o));
        let w = &mut self.windows[i];
        w.frame = wm::constrain(
            Rect::new(x, y, w.frame.w, w.frame.h),
            self.layout.bounds(),
            decor::TITLEBAR,
        );
        if let Some((serial, target)) = w.pending {
            w.pending = Some((serial, Rect::new(w.frame.x, w.frame.y, target.w, target.h)));
        }
        let new_area = w.painted_area();
        self.damage_logical(old_area);
        self.damage_logical(new_area);
        let preview = wm::snap_target(
            self.pointer.x as i32,
            self.pointer.y as i32,
            work,
            decor::border_grab(),
        )
        .map(|t| match t {
            SnapTarget::Maximize => work,
            SnapTarget::Snap(s) => wm::snap_rect(s, work),
        });
        if preview != self.pointer.snap_preview {
            for r in [self.pointer.snap_preview, preview].into_iter().flatten() {
                self.damage_logical(r);
            }
            self.pointer.snap_preview = preview;
        }
    }

    pub(super) fn drag_resize(
        &mut self,
        key: SurfaceKey,
        edges: u8,
        start: (f64, f64),
        frame: Rect,
    ) {
        let Some(i) = self.window_index(key) else {
            return;
        };
        let (dx, dy) = (
            (self.pointer.x - start.0) as i32,
            (self.pointer.y - start.1) as i32,
        );
        let (mut x, mut y, mut w, mut h) = (frame.x, frame.y, frame.w, frame.h);
        if edges & 8 != 0 {
            w += dx; // physical: right edge
        }
        if edges & 4 != 0 {
            x += dx; // physical: left edge
            w -= dx;
        }
        if edges & 2 != 0 {
            h += dy;
        }
        if edges & 1 != 0 {
            y += dy;
            h -= dy;
        }
        let limits = self.surface(key).map(|s| s.limits).unwrap_or_default();
        let bounds = self.layout.bounds();
        let (cw, ch) = wm::clamp_size(w, h - decor::TITLEBAR, limits.min, limits.max, bounds);
        let (nw, nh) = (cw, ch + decor::TITLEBAR);
        if edges & 4 != 0 {
            x = frame.max_x() - nw; // physical: left edge keeps the right edge fixed
        }
        if edges & 1 != 0 {
            y = frame.max_y() - nh;
        }
        let target = Rect::new(x, y, nw, nh);
        let current = self.windows[i]
            .pending
            .map_or(self.windows[i].frame, |p| p.1);
        if target != current {
            self.windows[i].snap = None;
            self.configure_window(i, target);
        }
    }

    pub(super) fn release(&mut self, grab: Option<PointerGrab>) {
        self.pointer.grab = None;
        match grab {
            Some(PointerGrab::Move { key, .. }) => self.finish_move(key),
            Some(PointerGrab::Button { key, button }) => {
                let over = self.hit_test(self.pointer.x, self.pointer.y);
                self.hover_button(key, None);
                if matches!(over, Some((k, Part::Button(b), _)) if k == key && b == button) {
                    self.title_button(key, button);
                }
            }
            Some(PointerGrab::Drag(source)) => self.drop_on_pointer(source),
            _ => {}
        }
        self.update_hover();
    }

    pub(super) fn finish_move(&mut self, key: SurfaceKey) {
        let preview = self.pointer.snap_preview.take();
        let Some(i) = self.window_index(key) else {
            return;
        };
        if let Some(r) = preview {
            self.damage_logical(r);
            let px = self.pointer.x as i32;
            let py = self.pointer.y as i32;
            let output = self.layout.output_at(px, py).map(|o| o.id);
            let work = output.map_or(self.layout.bounds(), |o| self.work_area(o));
            match wm::snap_target(px, py, work, decor::border_grab()) {
                Some(SnapTarget::Maximize) => {
                    self.set_window_state(i, WindowState::Maximized, None)
                }
                Some(SnapTarget::Snap(s)) => self.set_window_state(i, WindowState::Normal, Some(s)),
                None => {}
            }
        } else {
            self.windows[i].restore = self.windows[i].frame;
        }
    }

    pub(super) fn title_button(&mut self, key: SurfaceKey, button: Button) {
        let Some(i) = self.window_index(key) else {
            return;
        };
        match button {
            Button::Close => self.request_close(key),
            Button::Minimize => self.set_window_state(i, WindowState::Minimized, None),
            Button::Maximize => {
                let maximized = self.windows[i].state == WindowState::Maximized;
                let target = if maximized {
                    WindowState::Normal
                } else {
                    WindowState::Maximized
                };
                self.set_window_state(i, target, None);
            }
        }
    }
}
