//! Frame callbacks (paced to the display also when nothing changed) and
//! screen capture (paused while the session is locked; the compositor
//! shows the recording indicator while it runs).

use super::Compositor;
use crate::geom::Rect;
use crate::output::Output;
use crate::state::{ClientId, INDICATOR_MS};
use ultar_surface::proto::{Event, OutputId, MAX_CHUNK};

impl Compositor {
    /// An output without damage: frame callbacks are answered at most once
    /// per refresh interval (a client asking in a loop gets no faster
    /// answers and learns nothing about other clients' damage).
    pub(super) fn idle_frame(&mut self, o: &Output) {
        let now = self.clock.now_ms();
        let interval = o.frame_interval_us() / 1000;
        let due = self
            .last_present
            .get(&o.id)
            .is_none_or(|&t| now >= t + interval);
        if due && self.frames_done(o.id) > 0 {
            self.last_present.insert(o.id, now);
        }
    }

    /// Sends `FrameDone` to the surfaces on an output that asked for it;
    /// returns how many.
    pub(super) fn frames_done(&mut self, output: OutputId) -> usize {
        let Some(area) = self.state.layout.get(output).map(|o| o.logical_rect()) else {
            return 0;
        };
        let mut sent = 0;
        let time = self.clock.now_ms() as u32;
        let keys: Vec<_> = self
            .state
            .scene()
            .into_iter()
            .filter(|p| p.content.intersect(&area).is_some())
            .map(|p| p.key)
            .collect();
        for key in keys {
            let wanted = self
                .state
                .surface_mut(key)
                .is_some_and(|s| std::mem::replace(&mut s.frame_requested, false));
            if wanted {
                sent += 1;
                self.state.send(
                    key.client,
                    Event::FrameDone {
                        surface: key.surface,
                        time_ms: time,
                    },
                );
            }
        }
        sent
    }

    /// Sends the next rows of every capture in progress.
    pub(super) fn stream_captures(&mut self) {
        if self.state.locked() {
            return;
        }
        let ids: Vec<ClientId> = self
            .state
            .clients
            .iter()
            .filter(|(_, c)| c.capture.is_some())
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let Some((output, row)) = self.state.clients.get(&id).and_then(|c| c.capture) else {
                continue;
            };
            if self
                .state
                .clients
                .get(&id)
                .is_some_and(|c| c.conn.pending() > MAX_CHUNK)
            {
                continue;
            }
            let stale = self
                .state
                .damage
                .get(&output)
                .is_some_and(|d| !d.is_empty());
            if stale || self.renderer.frame(output.0).is_none() {
                // Capture what is on screen now (the indicator included).
                self.render_now();
            }
            let Some(frame) = self.renderer.frame(output.0) else {
                if let Some(c) = self.state.clients.get_mut(&id) {
                    c.capture = None;
                }
                continue;
            };
            let rows = (MAX_CHUNK / (frame.width() as usize * 4)).max(1) as u32;
            let n = rows.min(frame.height() - row.min(frame.height()));
            let data = frame.to_bytes(Rect::new(0, row as i32, frame.width() as i32, n as i32));
            let last = row + n >= frame.height();
            let event = Event::CaptureData {
                output,
                width: frame.width(),
                height: frame.height(),
                row,
                data,
                last,
            };
            if let Some(c) = self.state.clients.get_mut(&id) {
                c.capture = if last { None } else { Some((output, row + n)) };
            }
            self.state.session.recording_until = self.state.now + INDICATOR_MS;
            self.state.send(id, event);
        }
    }
}
