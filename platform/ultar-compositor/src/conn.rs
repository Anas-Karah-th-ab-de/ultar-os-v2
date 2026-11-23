//! One client connection: non-blocking reads into frames, bounded output
//! queue, received descriptors.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use ultar_surface::proto::Event;
use ultar_surface::wire::{FrameReader, WireError};

/// Descriptors kept unclaimed per client; more is a protocol violation.
pub const MAX_PENDING_FDS: usize = 4;

/// Identity of the process at the other end (from the kernel, never from
/// the client).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Process id, if the kernel reports it.
    pub pid: Option<u32>,
    /// User id.
    pub uid: u32,
}

/// What one read produced.
#[derive(Debug, Default)]
pub struct ReadOutcome {
    /// Complete frames (opcode, body).
    pub frames: Vec<(u16, Vec<u8>)>,
    /// The peer closed or the socket failed.
    pub closed: bool,
    /// The byte stream is malformed.
    pub malformed: Option<WireError>,
    /// The peer sent more descriptors than allowed.
    pub fd_abuse: bool,
}

/// A client connection.
#[derive(Debug)]
pub struct Conn {
    stream: UnixStream,
    reader: FrameReader,
    out: VecDeque<u8>,
    fds: VecDeque<OwnedFd>,
    accept_fds: bool,
    scratch: Vec<u8>,
    /// Who is connected.
    pub peer: Peer,
}

impl Conn {
    /// Wraps an accepted stream; `accept_fds` enables `SCM_RIGHTS`.
    pub fn new(stream: UnixStream, peer: Peer, accept_fds: bool) -> std::io::Result<Conn> {
        stream.set_nonblocking(true)?;
        Ok(Conn {
            stream,
            reader: FrameReader::new(),
            out: VecDeque::new(),
            fds: VecDeque::new(),
            accept_fds,
            scratch: vec![0u8; 64 * 1024],
            peer,
        })
    }

    /// The socket for `poll`.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }

    /// Reads at most `budget` bytes and splits them into frames.
    pub fn read(&mut self, budget: usize) -> ReadOutcome {
        let mut outcome = ReadOutcome::default();
        let mut taken = 0;
        while taken < budget {
            let room = self
                .reader
                .room()
                .min(self.scratch.len())
                .min(budget - taken);
            if room == 0 {
                break;
            }
            let n = self.read_once(room, &mut outcome);
            match n {
                Ok(0) => {
                    outcome.closed = true;
                    break;
                }
                Ok(n) => {
                    taken += n;
                    self.reader.push(&self.scratch[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    outcome.closed = true;
                    break;
                }
            }
            if outcome.fd_abuse {
                break;
            }
        }
        loop {
            match self.reader.next_frame() {
                Ok(Some(frame)) => outcome.frames.push(frame),
                Ok(None) => break,
                Err(e) => {
                    outcome.malformed = Some(e);
                    break;
                }
            }
        }
        outcome
    }

    /// One read into the scratch buffer; descriptors are kept (at most
    /// [`MAX_PENDING_FDS`]), more mark the outcome as abuse.
    fn read_once(&mut self, room: usize, outcome: &mut ReadOutcome) -> std::io::Result<usize> {
        if !self.accept_fds {
            return self.stream.read(&mut self.scratch[..room]);
        }
        let got = ultar_hal_gfx::fdpass::recv(&self.stream, &mut self.scratch[..room])?;
        outcome.fd_abuse |= got.truncated;
        for fd in got.fds {
            if self.fds.len() >= MAX_PENDING_FDS {
                outcome.fd_abuse = true;
            } else {
                self.fds.push_back(fd);
            }
        }
        Ok(got.bytes)
    }

    /// The oldest descriptor received and not yet claimed.
    pub fn take_fd(&mut self) -> Option<OwnedFd> {
        self.fds.pop_front()
    }

    /// Queues an event; false if the queue would exceed `limit` bytes (the
    /// client does not read – the caller drops it).
    pub fn queue(&mut self, event: &Event, limit: usize) -> bool {
        let Ok(frame) = event.encode() else {
            return true;
        };
        if self.out.len() + frame.len() > limit {
            return false;
        }
        self.out.extend(frame);
        true
    }

    /// Bytes waiting to be sent.
    pub fn pending(&self) -> usize {
        self.out.len()
    }

    /// Writes what the socket takes now; false if the connection broke.
    pub fn flush(&mut self) -> bool {
        while !self.out.is_empty() {
            let (head, _) = self.out.as_slices();
            match self.stream.write(head) {
                Ok(0) => return false,
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_surface::proto::{Request, SurfaceId};

    fn pair(fds: bool) -> (Conn, UnixStream) {
        let (a, b) = UnixStream::pair().unwrap();
        (Conn::new(a, Peer { pid: None, uid: 7 }, fds).unwrap(), b)
    }

    #[test]
    fn frames_out_and_in() {
        let (mut conn, mut other) = pair(false);
        let frame = Request::Frame {
            surface: SurfaceId(1),
        }
        .encode()
        .unwrap();
        other.write_all(&frame).unwrap();
        other.write_all(&frame[..3]).unwrap();
        let got = conn.read(1 << 20);
        assert_eq!(got.frames.len(), 1);
        assert!(!got.closed && got.malformed.is_none());
        assert!(conn.queue(&Event::Ping { serial: 1 }, 1024));
        assert!(conn.pending() > 0);
        assert!(conn.flush());
        assert_eq!(conn.pending(), 0);
        let mut buf = [0u8; 64];
        let n = other.read(&mut buf).unwrap();
        assert!(n > 8);
        assert!(!conn.queue(&Event::Ping { serial: 1 }, 4));
        drop(other);
        assert!(conn.read(1024).closed);
        assert_eq!(conn.peer.uid, 7);
        assert!(conn.take_fd().is_none());
    }

    #[test]
    fn garbage_is_reported_and_reads_are_budgeted() {
        let (mut conn, mut other) = pair(true);
        other.write_all(&[0, 0, 0, 0, 1, 0, 9, 9]).unwrap();
        assert!(conn.read(1024).malformed.is_some());
        let (mut conn, mut other) = pair(false);
        let frame = Request::Pong { serial: 1 }.encode().unwrap();
        for _ in 0..100 {
            other.write_all(&frame).unwrap();
        }
        let got = conn.read(frame.len() * 10);
        assert!(got.frames.len() <= 10);
        assert!(conn.read(1 << 20).frames.len() >= 90);
    }

    #[test]
    fn descriptors_are_bounded() {
        let (mut conn, other) = pair(true);
        let (file, _writer) = std::io::pipe().unwrap();
        for _ in 0..6 {
            ultar_hal_gfx::fdpass::send(&other, b"x", Some(file.as_fd())).unwrap();
        }
        let got = conn.read(1);
        assert!(!got.fd_abuse);
        let mut abuse = false;
        for _ in 0..10 {
            abuse |= conn.read(1).fd_abuse;
        }
        assert!(abuse);
        assert!(conn.take_fd().is_some());
    }
}
