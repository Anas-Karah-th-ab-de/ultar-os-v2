//! A connection to the compositor.

mod error;
mod window;

pub use error::ClientError;
pub use window::{Window, WindowConfig};

use crate::proto::{
    BufferId, Event, OutputInfo, Request, Rights, SurfaceId, Transport, PROTOCOL_VERSION,
};
use crate::wire::FrameReader;
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Time the compositor has to answer `Hello`.
pub const WELCOME_TIMEOUT: Duration = Duration::from_secs(5);
/// Events kept unread before the oldest pointer motions are merged away.
const MAX_QUEUED: usize = 4096;
/// Environment variable with an activation token from the launcher (the
/// shell asked for it with the serial of the click that started the app);
/// the first toplevel presents it and gets the focus.
pub const ACTIVATION_ENV: &str = "ULTAR_ACTIVATION_TOKEN";

/// One client connection.
#[derive(Debug)]
pub struct Client {
    stream: UnixStream,
    reader: FrameReader,
    queue: VecDeque<Event>,
    transport: Transport,
    rights: Rights,
    max_buffer_bytes: u64,
    outputs: Vec<OutputInfo>,
    next_id: u32,
    scratch: Vec<u8>,
    activation: Option<u64>,
}

impl Client {
    /// Connects to the compositor socket at `path`.
    pub fn connect(path: &Path, app_id: &str) -> Result<Client, ClientError> {
        let stream = UnixStream::connect(path).map_err(ClientError::from)?;
        Client::with_stream(stream, app_id)
    }

    /// Connects, retrying `attempts` times with `delay` between tries (a
    /// compositor that restarts needs a moment to listen again).
    pub fn connect_retry(
        path: &Path,
        app_id: &str,
        attempts: u32,
        delay: Duration,
    ) -> Result<Client, ClientError> {
        let mut last = ClientError::Disconnected;
        for attempt in 0..attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(delay);
            }
            match Client::connect(path, app_id) {
                Ok(client) => return Ok(client),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Speaks the protocol over an already connected stream.
    pub fn with_stream(stream: UnixStream, app_id: &str) -> Result<Client, ClientError> {
        let shared_memory = ultar_hal_gfx::shm::probe();
        let mut client = Client {
            stream,
            reader: FrameReader::new(),
            queue: VecDeque::new(),
            transport: Transport::Inline,
            rights: Rights::NONE,
            max_buffer_bytes: 0,
            outputs: Vec::new(),
            next_id: 1,
            scratch: vec![0u8; 64 * 1024],
            activation: std::env::var(ACTIVATION_ENV)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .filter(|&t: &u64| t != 0),
        };
        client.send(&Request::Hello {
            version: PROTOCOL_VERSION,
            app_id: app_id.to_owned(),
            shared_memory,
        })?;
        let deadline = Instant::now() + WELCOME_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match client.read_event(Some(remaining))? {
                Some(Event::Welcome {
                    version,
                    transport,
                    rights,
                    max_buffer_bytes,
                }) => {
                    if version != PROTOCOL_VERSION {
                        return Err(ClientError::Version(version));
                    }
                    client.transport = transport;
                    client.rights = rights;
                    client.max_buffer_bytes = max_buffer_bytes;
                    return Ok(client);
                }
                Some(Event::Error { code }) => return Err(ClientError::Refused(code)),
                Some(other) => client.queue.push_back(other),
                None => return Err(ClientError::Timeout),
            }
        }
    }

    /// The launcher's activation token, once (the first toplevel uses it).
    pub fn take_activation_token(&mut self) -> Option<u64> {
        self.activation.take()
    }

    /// Asks for an activation token for a user action (`serial` of a click
    /// or key); the answer is `Event::ActivationToken` (0 = refused). Pass
    /// it to the started app in [`ACTIVATION_ENV`].
    pub fn request_activation(&mut self, serial: u32) -> Result<(), ClientError> {
        self.send(&Request::RequestActivation { serial })
    }

    /// How pixels travel to the compositor.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Rights the compositor granted.
    pub fn rights(&self) -> Rights {
        self.rights
    }

    /// Memory the compositor lets this client use for buffers.
    pub fn max_buffer_bytes(&self) -> u64 {
        self.max_buffer_bytes
    }

    /// Outputs known so far.
    pub fn outputs(&self) -> &[OutputInfo] {
        &self.outputs
    }

    /// The socket, for waiting in a foreign event loop.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }

    /// A fresh surface id.
    pub fn new_surface_id(&mut self) -> SurfaceId {
        SurfaceId(self.bump())
    }

    /// A fresh buffer id.
    pub fn new_buffer_id(&mut self) -> BufferId {
        BufferId(self.bump())
    }

    fn bump(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Sends a request.
    pub fn send(&mut self, request: &Request) -> Result<(), ClientError> {
        let frame = request.encode()?;
        self.stream.write_all(&frame).map_err(ClientError::from)
    }

    /// Offers a selection whose data this client provides (clipboard bridge).
    pub fn bridge_set_selection(&mut self, mimes: &[String]) -> Result<(), ClientError> {
        self.send(&Request::BridgeSetSelection {
            mimes: mimes.to_vec(),
        })
    }

    /// Asks for the current selection's data (clipboard bridge).
    pub fn bridge_read_selection(&mut self, mime: &str) -> Result<(), ClientError> {
        self.send(&Request::BridgeReadSelection {
            mime: mime.to_owned(),
        })
    }

    /// Sends a request with a descriptor attached.
    pub fn send_with_fd(
        &mut self,
        request: &Request,
        fd: BorrowedFd<'_>,
    ) -> Result<(), ClientError> {
        let frame = request.encode()?;
        let mut sent = loop {
            match ultar_hal_gfx::fdpass::send(&self.stream, &frame, Some(fd)) {
                Ok(n) => break n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    wait(self.stream.as_fd(), PollFlags::OUT, None)?;
                }
                Err(e) => return Err(e.into()),
            }
        };
        while sent < frame.len() {
            sent += self
                .stream
                .write(&frame[sent..])
                .map_err(ClientError::from)?;
        }
        Ok(())
    }

    /// The next event, waiting up to `timeout` (`None` = forever). Returns
    /// `Ok(None)` on timeout. Pings are answered here and not returned: an
    /// app that reads its events is responsive.
    pub fn next_event(&mut self, timeout: Option<Duration>) -> Result<Option<Event>, ClientError> {
        if let Some(event) = self.queue.pop_front() {
            return Ok(Some(event));
        }
        self.read_event(timeout)
    }

    /// Puts an event back at the front (used while waiting for a specific
    /// event).
    pub(crate) fn requeue(&mut self, events: Vec<Event>) {
        for event in events.into_iter().rev() {
            if self.queue.len() < MAX_QUEUED {
                self.queue.push_front(event);
            }
        }
    }

    fn read_event(&mut self, timeout: Option<Duration>) -> Result<Option<Event>, ClientError> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if let Some((opcode, body)) = self.reader.next_frame()? {
                let event = Event::decode(opcode, &body)?;
                if let Event::Ping { serial } = event {
                    // The app's loop is alive (it reads events): answer.
                    self.send(&Request::Pong { serial })?;
                    continue;
                }
                self.track(&event);
                return Ok(Some(event));
            }
            let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if remaining == Some(Duration::ZERO) && deadline.is_some() && !self.readable_now()? {
                return Ok(None);
            }
            if !wait(self.stream.as_fd(), PollFlags::IN, remaining)? {
                return Ok(None);
            }
            let n = match self.stream.read(&mut self.scratch) {
                Ok(0) => return Err(ClientError::Disconnected),
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            self.reader.push(&self.scratch[..n]);
        }
    }

    fn readable_now(&self) -> Result<bool, ClientError> {
        wait(self.stream.as_fd(), PollFlags::IN, Some(Duration::ZERO))
    }

    fn track(&mut self, event: &Event) {
        match event {
            Event::Output(info) => {
                self.outputs.retain(|o| o.id != info.id);
                self.outputs.push(info.clone());
            }
            Event::OutputRemoved { output } => self.outputs.retain(|o| o.id != *output),
            _ => {}
        }
    }
}

/// Waits until `fd` is ready for `flags`; false on timeout.
fn wait(
    fd: BorrowedFd<'_>,
    flags: PollFlags,
    timeout: Option<Duration>,
) -> Result<bool, ClientError> {
    let mut fds = [PollFd::from_borrowed_fd(fd, flags)];
    let spec = timeout.map(|t| Timespec {
        tv_sec: t.as_secs() as i64,
        tv_nsec: i64::from(t.subsec_nanos()),
    });
    loop {
        match poll(&mut fds, spec.as_ref()) {
            Ok(n) => return Ok(n > 0),
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(ClientError::Io(e.raw_os_error())),
        }
    }
}
