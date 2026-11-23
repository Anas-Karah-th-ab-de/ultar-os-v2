//! The gate of one listener: accepts connections and runs every handshake
//! on **one** thread, without blocking on any peer.
//!
//! Review round 3 (R3-B0): a connection gets its own threads only after it
//! has completed the handshake and has been admitted. Before that it costs
//! one descriptor and a few bytes, and only [`BrokerConfig::max_handshakes`]
//! connections of a listener may be in the handshake at the same time;
//! further connections wait in the kernel's listen queue (the gate does not
//! accept while all slots are taken; a slot frees at the latest after the
//! handshake timeout). A thread that cannot be started refuses that one
//! connection (`LimitExceeded`); nothing panics.
//!
//! [`BrokerConfig::max_handshakes`]: super::BrokerConfig::max_handshakes

use super::conn::{admit, reject, start_session};
use super::observe::BrokerEvent;
use super::Shared;
use crate::error::{BusError, ErrorCode};
use crate::identity::{PeerIdentity, PeerInfo};
use crate::message::decode;
use crate::wire::{write_preamble, Envelope, Hello, Kind, MAGIC, PROTOCOL_VERSION};
use std::io::{ErrorKind, Read};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Largest frame accepted before the peer is authenticated.
const HANDSHAKE_MAX_FRAME: usize = 4096;
/// Bytes of the preamble.
const PREAMBLE_LEN: usize = 8;
/// Bytes of a frame header (body length, little endian).
const HEADER_LEN: usize = 4;
/// Pause after a failed `accept` or `poll` (e.g. out of descriptors), so
/// that a listener that stays readable does not spin.
const RETRY: Duration = Duration::from_millis(50);

/// One connection in the handshake.
struct Handshake {
    stream: UnixStream,
    deadline: Instant,
    /// Bytes read so far: preamble, frame header, `Hello` body.
    input: Vec<u8>,
    /// Whether our preamble was sent.
    answered: bool,
}

/// What the bytes read so far amount to.
enum Step {
    /// More bytes are needed.
    More,
    /// The handshake is complete.
    Done(Hello),
    /// The peer is turned away.
    Fail(BusError),
}

impl Handshake {
    /// Bytes needed before the next step can be taken.
    fn needed(&self) -> usize {
        let framed = PREAMBLE_LEN + HEADER_LEN;
        if self.input.len() < framed {
            return framed;
        }
        framed + body_len(&self.input).min(HANDSHAKE_MAX_FRAME)
    }

    /// Reads what is there, never more than the handshake needs (the bytes
    /// after `Hello` belong to the session). `false`: the peer is gone.
    fn fill(&mut self) -> bool {
        let mut chunk = [0u8; 512];
        loop {
            let want = self.needed().saturating_sub(self.input.len());
            if want == 0 {
                return true;
            }
            let limit = want.min(chunk.len());
            let Some(buffer) = chunk.get_mut(..limit) else {
                return false;
            };
            match (&self.stream).read(buffer) {
                Ok(0) => return false,
                Ok(count) => self
                    .input
                    .extend_from_slice(buffer.get(..count).unwrap_or(&[])),
                Err(err) if err.kind() == ErrorKind::WouldBlock => return true,
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
        }
    }

    /// Takes the next step with the bytes read so far.
    fn step(&mut self) -> Step {
        if self.input.len() < PREAMBLE_LEN {
            return Step::More;
        }
        if !self.answered {
            if self.input.get(..4) != Some(&MAGIC[..]) {
                return Step::Fail(BusError::BadMagic);
            }
            if write_preamble(&mut &self.stream, PROTOCOL_VERSION).is_err() {
                return Step::Fail(BusError::Closed);
            }
            self.answered = true;
            let theirs = u16::from_le_bytes([self.input[4], self.input[5]]);
            if theirs != PROTOCOL_VERSION {
                return Step::Fail(BusError::ProtocolMismatch {
                    ours: PROTOCOL_VERSION,
                    theirs,
                });
            }
        }
        if self.input.len() < PREAMBLE_LEN + HEADER_LEN {
            return Step::More;
        }
        let len = body_len(&self.input);
        if len == 0 {
            return Step::Fail(BusError::Malformed);
        }
        if len > HANDSHAKE_MAX_FRAME {
            return Step::Fail(BusError::FrameTooLarge {
                len: u32::try_from(len).unwrap_or(u32::MAX),
                max: HANDSHAKE_MAX_FRAME as u32,
            });
        }
        let Some(body) = self.input.get(PREAMBLE_LEN + HEADER_LEN..) else {
            return Step::More;
        };
        if body.len() < len {
            return Step::More;
        }
        match Envelope::from_body(body) {
            Ok(envelope) if envelope.kind == Kind::Hello => match decode(&envelope.payload) {
                Ok(hello) => Step::Done(hello),
                Err(err) => Step::Fail(err),
            },
            Ok(_) => Step::Fail(BusError::Remote(ErrorCode::ProtocolViolation)),
            Err(err) => Step::Fail(err),
        }
    }
}

/// Body length announced in the frame header after the preamble.
fn body_len(input: &[u8]) -> usize {
    let mut header = [0u8; HEADER_LEN];
    if let Some(bytes) = input.get(PREAMBLE_LEN..PREAMBLE_LEN + HEADER_LEN) {
        header.copy_from_slice(bytes);
    }
    usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX)
}

/// The gate of one listener.
pub(crate) struct Gate {
    pub(crate) shared: Arc<Shared>,
    pub(crate) listener: UnixListener,
    pub(crate) auth: Arc<dyn PeerIdentity>,
    /// Index of the listener (from 1).
    pub(crate) index: usize,
    /// Most admitted connections of this listener.
    pub(crate) limit: usize,
    pending: Vec<Handshake>,
}

impl Gate {
    /// A gate for `listener`; it serves when [`Self::run`] is called.
    pub(crate) fn new(
        shared: Arc<Shared>,
        listener: UnixListener,
        auth: Arc<dyn PeerIdentity>,
        index: usize,
        limit: usize,
    ) -> Self {
        Self {
            shared,
            listener,
            auth,
            index,
            limit,
            pending: Vec::new(),
        }
    }

    fn stopping(&self) -> bool {
        self.shared.stopping.load(Ordering::SeqCst)
    }

    /// Serves until the broker stops. Waits in `poll` for the listener and
    /// the connections in the handshake, so an idle gate causes no wakeups.
    pub(crate) fn run(mut self) {
        if self.listener.set_nonblocking(true).is_err() {
            return;
        }
        while !self.stopping() {
            self.expire(Instant::now());
            // A full gate does not look at the listener: new connections
            // wait in the kernel's queue until a handshake slot is free.
            let open = self.pending.len() < self.shared.config.max_handshakes;
            let ready = {
                let mut fds = Vec::with_capacity(self.pending.len() + 1);
                fds.extend(self.pending.iter().map(|shake| shake.stream.as_fd()));
                if open {
                    fds.push(self.listener.as_fd());
                }
                ultar_sys::poll_readable(&fds, self.next_deadline())
            };
            let Ok(ready) = ready else {
                thread::sleep(RETRY);
                continue;
            };
            if self.stopping() {
                return;
            }
            let listener_ready = open && ready.get(self.pending.len()).copied().unwrap_or(false);
            // From the back: `swap_remove` moves an already handled entry.
            for index in (0..self.pending.len()).rev() {
                if ready.get(index).copied().unwrap_or(false) {
                    self.advance(index);
                }
            }
            if listener_ready {
                self.accept_all();
            }
        }
    }

    /// Time until the earliest handshake deadline.
    fn next_deadline(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending
            .iter()
            .map(|shake| shake.deadline.saturating_duration_since(now))
            .min()
    }

    /// Closes the connections whose handshake took too long.
    fn expire(&mut self, now: Instant) {
        self.pending.retain(|shake| shake.deadline > now);
    }

    /// Accepts what waits, as long as handshake slots are free.
    fn accept_all(&mut self) {
        while self.pending.len() < self.shared.config.max_handshakes {
            match self.listener.accept() {
                Ok((stream, _)) => self.take(stream),
                Err(err) if err.kind() == ErrorKind::WouldBlock => return,
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => {
                    thread::sleep(RETRY);
                    return;
                }
            }
        }
    }

    fn take(&mut self, stream: UnixStream) {
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        self.pending.push(Handshake {
            stream,
            deadline: Instant::now() + self.shared.config.handshake_timeout,
            input: Vec::new(),
            answered: false,
        });
    }

    /// Reads from the connection at `index` and takes the steps it allows.
    fn advance(&mut self, index: usize) {
        let Some(shake) = self.pending.get_mut(index) else {
            return;
        };
        let step = if shake.fill() {
            shake.step()
        } else {
            Step::Fail(BusError::Closed)
        };
        match step {
            Step::More => {}
            Step::Done(hello) => {
                let shake = self.pending.swap_remove(index);
                self.finish(shake.stream, &hello);
            }
            Step::Fail(err) => {
                let shake = self.pending.swap_remove(index);
                let stream = shake.stream;
                let timeout = Some(self.shared.config.write_timeout);
                if stream.set_nonblocking(false).is_ok()
                    && stream.set_write_timeout(timeout).is_ok()
                {
                    reject(&stream, &err);
                }
            }
        }
    }

    /// Identifies the peer, admits it and starts its session.
    fn finish(&self, stream: UnixStream, hello: &Hello) {
        let config = &self.shared.config;
        if stream.set_nonblocking(false).is_err()
            || stream
                .set_write_timeout(Some(config.write_timeout))
                .is_err()
        {
            return;
        }
        let peer = PeerInfo {
            stream: &stream,
            client_name: &hello.client_name,
            token: hello.token.as_deref(),
        };
        let identity = match self.auth.identify(&peer) {
            Ok(identity) => identity,
            Err(_) => {
                self.shared.observe(&BrokerEvent::AuthFailed {
                    listener: self.index,
                });
                reject(&stream, &BusError::AuthFailed);
                return;
            }
        };
        let stream = Arc::new(stream);
        let admitted = match admit(&self.shared, &stream, identity, self.index, self.limit) {
            Ok(admitted) => admitted,
            Err(err) => {
                if matches!(err, BusError::Remote(ErrorCode::LimitExceeded)) {
                    self.shared.observe(&BrokerEvent::ConnectionLimit {
                        listener: self.index,
                    });
                }
                reject(&stream, &err);
                return;
            }
        };
        start_session(&self.shared, stream, admitted, self.index);
    }
}
