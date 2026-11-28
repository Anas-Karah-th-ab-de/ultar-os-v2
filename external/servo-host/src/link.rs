//! The host's end of the browser socket (its standard input): commands in, events out, the
//! frame buffer descriptors out with `SCM_RIGHTS`. Reading never blocks; writing waits only
//! for room in the socket.

use rustix::event::{poll, PollFd, PollFlags};
use rustix::io::Errno;
use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
use std::io::{self, IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use ultar_web::ipc::{Command, DecodeError, Decoder, MAX_MESSAGE};

/// Why the link to the browser ended.
#[derive(Debug)]
pub enum LinkError {
    /// The browser closed its end.
    Closed,
    /// The browser sent something outside the protocol.
    Protocol(DecodeError),
    /// The socket failed.
    Io(io::Error),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Closed => f.write_str("browser closed the link"),
            LinkError::Protocol(e) => write!(f, "browser protocol: {e}"),
            LinkError::Io(e) => write!(f, "link i/o: {e}"),
        }
    }
}

impl From<io::Error> for LinkError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset => LinkError::Closed,
            _ => LinkError::Io(e),
        }
    }
}

impl From<Errno> for LinkError {
    fn from(e: Errno) -> Self {
        LinkError::from(io::Error::from(e))
    }
}

/// The socket to the browser.
pub struct Link {
    socket: UnixStream,
    decoder: Decoder,
}

impl Link {
    /// The link over standard input (how the browser starts the host).
    pub fn from_stdin() -> io::Result<Link> {
        let fd: OwnedFd = io::stdin().as_fd().try_clone_to_owned()?;
        let socket = UnixStream::from(fd);
        socket.set_nonblocking(true)?;
        Ok(Link {
            socket,
            decoder: Decoder::default(),
        })
    }

    /// The socket, to wait on.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }

    /// Every whole command that has arrived (empty when none).
    pub fn receive(&mut self) -> Result<Vec<Command>, LinkError> {
        let mut data = [0u8; 16 * 1024];
        loop {
            match (&self.socket).read(&mut data) {
                Ok(0) => return Err(LinkError::Closed),
                Ok(n) => {
                    if self.decoder.pending() + n > 4 * MAX_MESSAGE {
                        return Err(LinkError::Protocol(DecodeError::BadLength));
                    }
                    self.decoder.push(&data[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        let mut commands = Vec::new();
        while let Some((kind, payload)) =
            self.decoder.next_message().map_err(LinkError::Protocol)?
        {
            commands.push(Command::decode(kind, &payload).map_err(LinkError::Protocol)?);
        }
        Ok(commands)
    }

    /// Sends a framed message (an encoded event).
    pub fn send(&self, bytes: &[u8]) -> Result<(), LinkError> {
        self.send_rest(bytes)
    }

    /// Sends a framed message with one descriptor (the buffer of `HostEvent::Buffer`).
    pub fn send_with_fd(&self, bytes: &[u8], fd: BorrowedFd<'_>) -> Result<(), LinkError> {
        let fds = [fd];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
            return Err(LinkError::Io(io::Error::other("ancillary buffer")));
        }
        loop {
            self.wait_writable()?;
            match sendmsg(
                &self.socket,
                &[IoSlice::new(bytes)],
                &mut control,
                SendFlags::NOSIGNAL,
            ) {
                Ok(n) => return self.send_rest(&bytes[n..]),
                Err(Errno::AGAIN | Errno::INTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn send_rest(&self, mut bytes: &[u8]) -> Result<(), LinkError> {
        while !bytes.is_empty() {
            match (&self.socket).write(bytes) {
                Ok(0) => return Err(LinkError::Closed),
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.wait_writable()?,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn wait_writable(&self) -> Result<(), LinkError> {
        let mut fds = [PollFd::new(&self.socket, PollFlags::OUT)];
        match poll(&mut fds, None) {
            Ok(_) | Err(Errno::INTR) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
