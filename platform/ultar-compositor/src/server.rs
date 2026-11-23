//! The event loop: accept clients, read requests, route input, compose
//! and present frames paced to the display, send frame callbacks. Sleeps
//! in `poll` without timeout when nothing is due (zero work when idle).

use crate::backend::{Backend, Hotplug};
pub use crate::clock::{Clock, MonotonicClock, Stats};
use crate::config::Config;
use crate::conn::Peer;
use crate::geom::Rect;
use crate::pixels::Image;
use crate::render::Renderer;
use crate::state::{ClientId, State};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};
use ultar_hal::InputEvent;
use ultar_surface::proto::{Event, OutputId, MAX_CHUNK};

/// Milliseconds before queued output is tried again without a wake-up.
const FLUSH_RETRY_MS: u64 = 10;

/// A running compositor.
pub struct Compositor {
    /// All state (public for tests and the shell integration).
    pub state: State,
    renderer: Renderer,
    backend: Box<dyn Backend>,
    listener: Option<UnixListener>,
    clock: Box<dyn Clock>,
    last_present: HashMap<OutputId, u64>,
    wake: Option<UnixStream>,
    woken: Option<u8>,
    /// Measurements.
    pub stats: Stats,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compositor")
            .field("state", &self.state)
            .finish()
    }
}

/// Kernel credentials of a connection; uid `u32::MAX` (no rights) if the
/// kernel cannot tell – fail closed.
pub fn peer_of(stream: &UnixStream) -> Peer {
    match ultar_service::peercred::peer_credentials(stream) {
        Ok(c) => Peer {
            pid: c.pid,
            uid: c.uid,
        },
        Err(_) => Peer {
            pid: None,
            uid: u32::MAX,
        },
    }
}

impl Compositor {
    /// A compositor on `backend`. Shared memory is offered when this kernel
    /// supports it and the configuration allows it.
    pub fn new(config: Config, backend: Box<dyn Backend>, clock: Box<dyn Clock>) -> Compositor {
        let shm = ultar_hal_gfx::shm::probe();
        let mut state = State::new(config, shm);
        for o in backend.outputs() {
            state.layout.add(
                o.hal,
                &o.name,
                (o.width, o.height),
                o.refresh_millihertz,
                o.width_mm,
            );
        }
        if let Some(p) = state.layout.primary() {
            let r = p.logical_rect();
            state.pointer.x = f64::from(r.x + r.w / 2);
            state.pointer.y = f64::from(r.y + r.h / 2);
        }
        state.now = clock.now_ms();
        state.damage_all();
        Compositor {
            state,
            renderer: Renderer::new(),
            backend,
            listener: None,
            clock,
            last_present: HashMap::new(),
            wake: None,
            woken: None,
            stats: Stats::default(),
        }
    }

    /// Accepts clients on a listening socket.
    pub fn set_listener(&mut self, listener: UnixListener) -> std::io::Result<()> {
        listener.set_nonblocking(true)?;
        self.listener = Some(listener);
        Ok(())
    }

    /// A socket whose bytes wake the loop (stop and reload hooks write one
    /// byte; [`Compositor::take_wake`] returns it).
    pub fn set_wake(&mut self, stream: UnixStream) -> std::io::Result<()> {
        stream.set_nonblocking(true)?;
        self.wake = Some(stream);
        Ok(())
    }

    /// The byte that woke the loop, if any.
    pub fn take_wake(&mut self) -> Option<u8> {
        self.woken.take()
    }

    /// Adds a connected stream (tests, socket activation).
    pub fn add_stream(&mut self, stream: UnixStream) -> Option<ClientId> {
        let peer = peer_of(&stream);
        self.state.accept(stream, peer)
    }

    /// Feeds input as if a device produced it.
    pub fn inject(&mut self, events: &[InputEvent]) {
        self.state.now = self.clock.now_ms();
        for e in events {
            self.state.input(e);
        }
        self.flush();
    }

    /// The last composed frame of an output.
    pub fn frame(&self, output: OutputId) -> Option<&Image> {
        self.renderer.frame(output.0)
    }

    /// Composes an output completely from scratch (reference image).
    pub fn render_reference(&mut self, output: OutputId) -> Option<Image> {
        let o = self.state.layout.get(output)?.clone();
        self.renderer.render_full(&mut self.state, &o)
    }

    /// Runs one turn: waits up to `timeout` (`None` = until something
    /// happens), then handles everything that is ready.
    pub fn dispatch(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        let wait = self.wait_time(timeout);
        let ready = self.wait(wait)?;
        self.state.now = self.clock.now_ms();
        if ready.listener {
            self.accept();
        }
        for id in ready.clients {
            self.read_client(id);
        }
        if ready.input {
            for e in self.backend.read_input() {
                self.state.input(&e);
            }
        }
        self.state.check_pings();
        self.state.tick_transfers();
        if self.frame_due() {
            self.render_now();
        }
        self.stream_captures();
        self.flush();
        Ok(())
    }

    fn wait_time(&self, timeout: Option<Duration>) -> Option<Duration> {
        let now = self.clock.now_ms();
        let frame = (self.state.has_damage() || self.frames_wanted())
            .then(|| self.next_frame_at().saturating_sub(now));
        let deadline = self.state.next_deadline().map(|d| d.saturating_sub(now));
        // Queued output is written when the socket is writable again; poll
        // reports that, but the Ultar kernel does not wake a poller when the
        // peer drains a Unix socket (kernel gap K11): look again soon.
        let flushing = self
            .state
            .clients
            .values()
            .any(|c| c.conn.pending() > 0)
            .then_some(FLUSH_RETRY_MS);
        [
            timeout.map(|t| t.as_millis() as u64),
            frame,
            deadline,
            flushing,
        ]
        .into_iter()
        .flatten()
        .min()
        .map(Duration::from_millis)
    }

    fn next_frame_at(&self) -> u64 {
        self.state
            .layout
            .enabled()
            .map(|o| {
                self.last_present
                    .get(&o.id)
                    .map_or(0, |t| t + o.frame_interval_us() / 1000)
            })
            .min()
            .unwrap_or(0)
    }

    fn frames_wanted(&self) -> bool {
        self.state
            .scene()
            .iter()
            .any(|p| self.state.surface(p.key).is_some_and(|s| s.frame_requested))
    }

    fn frame_due(&self) -> bool {
        (self.state.has_damage() || self.frames_wanted())
            && self.clock.now_ms() >= self.next_frame_at()
    }
}

/// What `poll` reported.
#[derive(Debug, Default)]
struct Ready {
    listener: bool,
    clients: Vec<ClientId>,
    input: bool,
}

impl Compositor {
    fn wait(&mut self, timeout: Option<Duration>) -> std::io::Result<Ready> {
        let ids: Vec<ClientId> = self.state.clients.keys().copied().collect();
        let mut fds = Vec::with_capacity(ids.len() + 4);
        if let Some(l) = &self.listener {
            fds.push(PollFd::from_borrowed_fd(l.as_fd(), PollFlags::IN));
        }
        for id in &ids {
            if let Some(c) = self.state.clients.get(id) {
                let mut flags = PollFlags::IN;
                if c.conn.pending() > 0 {
                    flags |= PollFlags::OUT;
                }
                fds.push(PollFd::from_borrowed_fd(c.conn.fd(), flags));
            }
        }
        let wake_index = fds.len();
        if let Some(w) = &self.wake {
            fds.push(PollFd::from_borrowed_fd(w.as_fd(), PollFlags::IN));
        }
        let input_start = fds.len();
        for fd in self.backend.input_fds() {
            fds.push(PollFd::from_borrowed_fd(fd, PollFlags::IN));
        }
        let spec = timeout.map(|t| Timespec {
            tv_sec: t.as_secs() as i64,
            tv_nsec: i64::from(t.subsec_nanos()),
        });
        match poll(&mut fds, spec.as_ref()) {
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(std::io::Error::from_raw_os_error(e.raw_os_error())),
        }
        let mut ready = Ready::default();
        let offset = usize::from(self.listener.is_some());
        ready.listener = self.listener.is_some() && !fds[0].revents().is_empty();
        for (i, id) in ids.iter().enumerate() {
            if fds.get(offset + i).is_some_and(|f| !f.revents().is_empty()) {
                ready.clients.push(*id);
            }
        }
        ready.input = fds[input_start..].iter().any(|f| !f.revents().is_empty());
        let woken = self.wake.is_some() && !fds[wake_index].revents().is_empty();
        drop(fds);
        if woken {
            let mut byte = [0u8; 1];
            if let Some(w) = &mut self.wake {
                if let Ok(1) = std::io::Read::read(w, &mut byte) {
                    self.woken = Some(byte[0]);
                }
            }
        }
        Ok(ready)
    }

    fn accept(&mut self) {
        let Some(listener) = &self.listener else {
            return;
        };
        let mut accepted = Vec::new();
        for _ in 0..16 {
            match listener.accept() {
                Ok((stream, _)) => accepted.push(stream),
                Err(_) => break,
            }
        }
        for stream in accepted {
            self.add_stream(stream);
        }
    }

    /// Reads and handles what one client sent.
    pub fn read_client(&mut self, id: ClientId) {
        let budget = self.state.config.limits.read_slice;
        let Some(c) = self.state.clients.get_mut(&id) else {
            return;
        };
        let outcome = c.conn.read(budget);
        for (opcode, body) in &outcome.frames {
            if self
                .state
                .clients
                .get(&id)
                .is_none_or(|c| c.doomed.is_some())
            {
                break;
            }
            self.state.handle_frame(id, *opcode, body);
        }
        if outcome.malformed.is_some() || outcome.fd_abuse {
            self.state
                .fail(id, ultar_surface::proto::ErrorCode::Malformed);
        } else if outcome.closed {
            if let Some(c) = self.state.clients.get_mut(&id) {
                c.doomed.get_or_insert(None);
            }
        }
    }

    /// Composes and presents every output with damage now (ignores pacing).
    /// Returns the number of frames presented.
    pub fn render_now(&mut self) -> usize {
        let outputs: Vec<_> = self.state.layout.enabled().cloned().collect();
        let mut presented = 0;
        for o in outputs {
            let started = Instant::now();
            let damage = self.renderer.render(&mut self.state, &o);
            if damage.is_empty() {
                self.frames_done(o.id);
                continue;
            }
            let took = started.elapsed().as_micros() as u64;
            self.stats.frames += 1;
            self.stats.total_compose_us += took;
            self.stats.max_compose_us = self.stats.max_compose_us.max(took);
            if let Some(frame) = self.renderer.frame(o.id.0) {
                if self.backend.present(o.hal, frame, &damage).is_ok() {
                    presented += 1;
                }
            }
            self.last_present.insert(o.id, self.clock.now_ms());
            self.frames_done(o.id);
        }
        presented
    }

    /// Sends `FrameDone` to the surfaces on an output that asked for it.
    fn frames_done(&mut self, output: OutputId) {
        let Some(area) = self.state.layout.get(output).map(|o| o.logical_rect()) else {
            return;
        };
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
                self.state.send(
                    key.client,
                    Event::FrameDone {
                        surface: key.surface,
                        time_ms: time,
                    },
                );
            }
        }
    }

    fn stream_captures(&mut self) {
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
            if self.renderer.frame(output.0).is_none() {
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
            self.state.send(id, event);
        }
    }

    /// Writes queued events and drops clients that broke or misbehaved.
    pub fn flush(&mut self) {
        let ids: Vec<ClientId> = self.state.clients.keys().copied().collect();
        for id in ids {
            let ok = self
                .state
                .clients
                .get_mut(&id)
                .is_some_and(|c| c.conn.flush());
            if !ok {
                if let Some(c) = self.state.clients.get_mut(&id) {
                    c.doomed.get_or_insert(None);
                }
            }
        }
        for (id, code) in self.state.doomed() {
            if let (Some(code), Some(c)) = (code, self.state.clients.get_mut(&id)) {
                let _ = c.conn.queue(&Event::Error { code }, usize::MAX);
                let _ = c.conn.flush();
            }
            self.state.remove_client(id);
        }
    }

    /// Asks the backend about display changes and applies them.
    pub fn rescan_outputs(&mut self) {
        for change in self.backend.hotplug() {
            match change {
                Hotplug::Added(o) => {
                    self.state.layout.add(
                        o.hal,
                        &o.name,
                        (o.width, o.height),
                        o.refresh_millihertz,
                        o.width_mm,
                    );
                }
                Hotplug::Removed(hal) => {
                    if let Some(id) = self.state.layout.by_hal(hal).map(|o| o.id) {
                        self.state.layout.remove(id);
                        self.renderer.forget(id.0);
                        self.state.damage.remove(&id);
                        let ids: Vec<ClientId> = self.state.clients.keys().copied().collect();
                        for c in ids {
                            self.state.send(c, Event::OutputRemoved { output: id });
                        }
                    }
                }
            }
        }
        self.state.outputs_changed();
        self.flush();
    }
}
