//! Reacting to output changes: tell clients, move windows back onto a
//! display, resize maximised/snapped windows and layers.

use super::State;
use crate::decor::TITLEBAR;
use crate::wm;
use ultar_surface::proto::{Event, WindowState};

impl State {
    /// Call after the output layout changed (hot-plug, mode, scale).
    pub fn outputs_changed(&mut self) {
        let infos: Vec<_> = self.layout.enabled().map(|o| o.info()).collect();
        let ids: Vec<_> = self.clients.keys().copied().collect();
        for client in ids {
            for info in &infos {
                self.send(client, Event::Output(info.clone()));
            }
        }
        let bounds = self.layout.bounds();
        for i in 0..self.windows.len() {
            let w = &self.windows[i];
            let (state, snap, frame) = (w.state, w.snap, w.frame);
            let on_screen = self
                .layout
                .enabled()
                .any(|o| o.logical_rect().intersect(&frame).is_some());
            match state {
                WindowState::Maximized | WindowState::Fullscreen => {
                    self.set_window_state(i, state, None)
                }
                WindowState::Normal if snap.is_some() => self.set_window_state(i, state, snap),
                _ if !on_screen => {
                    // The display it was on is gone: back to the primary.
                    let work = self
                        .layout
                        .primary()
                        .map(|o| o.logical_rect())
                        .unwrap_or(bounds);
                    let target = wm::place_new(work, frame.w, frame.h - TITLEBAR, TITLEBAR, 0);
                    self.windows[i].frame = target;
                    self.windows[i].restore = target;
                    self.configure_window(i, target);
                }
                _ => {
                    let target = wm::constrain(frame, bounds, TITLEBAR);
                    self.windows[i].frame = target;
                }
            }
        }
        let layers = self.layers.clone();
        for key in layers {
            self.configure_layer(key);
        }
        self.damage_all();
        let (px, py) = (self.pointer.x.floor() as i32, self.pointer.y.floor() as i32);
        if !self
            .layout
            .enabled()
            .any(|o| o.logical_rect().contains(px, py))
        {
            // The display under the pointer is gone: to the primary's middle.
            if let Some(r) = self.layout.primary().map(|o| o.logical_rect()) {
                self.pointer_to(f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2));
            }
        }
    }
}
