//! The client side of a stream: connect, then push samples to play and pull samples
//! recorded, both without ever blocking.

use crate::{Hello, Status, HELLO_LEN, REPLY_LEN};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// How long the hello and its reply may take.
pub const HANDSHAKE: Duration = Duration::from_secs(2);
/// Longest backlog a client keeps when the server does not take it (milliseconds). What
/// does not fit is dropped: late sound is worse than a gap.
pub const MAX_BACKLOG_MS: usize = 200;

/// Why a stream could not be opened.
#[derive(Debug)]
pub enum ConnectError {
    /// The socket is not there or not reachable (no sound server).
    Io(io::Error),
    /// The server answered with something that is not a reply.
    Protocol,
    /// The server refused the stream.
    Refused(Status),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Io(e) => write!(f, "audio socket: {e}"),
            ConnectError::Protocol => f.write_str("audio socket: bad reply"),
            ConnectError::Refused(s) => write!(f, "audio stream refused: {s:?}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// An open stream.
pub struct Stream {
    sock: UnixStream,
    hello: Hello,
    out: Vec<u8>,
    inbuf: Vec<u8>,
    dropped: u64,
}

impl Stream {
    /// Connects to the stream socket at `path` and opens a stream as `hello` says.
    pub fn connect(path: &Path, hello: Hello) -> Result<Stream, ConnectError> {
        if !hello.valid() {
            return Err(ConnectError::Refused(Status::BadHello));
        }
        let mut sock = UnixStream::connect(path).map_err(ConnectError::Io)?;
        sock.set_read_timeout(Some(HANDSHAKE))
            .and_then(|()| sock.set_write_timeout(Some(HANDSHAKE)))
            .map_err(ConnectError::Io)?;
        sock.write_all(&hello.encode()).map_err(ConnectError::Io)?;
        let mut reply = [0u8; REPLY_LEN];
        sock.read_exact(&mut reply).map_err(ConnectError::Io)?;
        match Status::decode(&reply) {
            Some(Status::Ok) => {}
            Some(other) => return Err(ConnectError::Refused(other)),
            None => return Err(ConnectError::Protocol),
        }
        sock.set_nonblocking(true).map_err(ConnectError::Io)?;
        debug_assert_eq!(HELLO_LEN, hello.encode().len());
        Ok(Stream {
            sock,
            hello,
            out: Vec::new(),
            inbuf: Vec::new(),
            dropped: 0,
        })
    }

    /// The hello this stream was opened with.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    fn capacity(&self) -> usize {
        let per_ms = self.hello.rate as usize * self.hello.frame_bytes() / 1000;
        per_ms * MAX_BACKLOG_MS
    }

    /// Queues `samples` (interleaved, whole frames) and writes what the server takes now.
    /// Returns how many samples were accepted; the rest is dropped (and counted in
    /// [`Stream::dropped`]). `Err` when the server is gone.
    pub fn push(&mut self, samples: &[i16]) -> io::Result<usize> {
        let channels = usize::from(self.hello.channels);
        let whole = samples.len() / channels * channels;
        let room = self.capacity().saturating_sub(self.out.len()) / 2 / channels * channels;
        let take = whole.min(room);
        self.dropped += (samples.len() - take) as u64;
        for s in &samples[..take] {
            self.out.extend_from_slice(&s.to_le_bytes());
        }
        self.flush()?;
        Ok(take)
    }

    /// Writes the backlog as far as the server takes it. `Err` when the server is gone.
    pub fn flush(&mut self) -> io::Result<()> {
        while !self.out.is_empty() {
            match self.sock.write(&self.out) {
                Ok(0) => return Err(io::Error::from(ErrorKind::WriteZero)),
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Bytes still waiting to be written.
    pub fn backlog(&self) -> usize {
        self.out.len()
    }

    /// Samples dropped so far because the server did not take them.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Appends the samples recorded since the last call to `out` (whole frames only) and
    /// returns how many. `Err` when the server is gone.
    pub fn pull(&mut self, out: &mut Vec<i16>) -> io::Result<usize> {
        let mut buf = [0u8; 8192];
        loop {
            match self.sock.read(&mut buf) {
                Ok(0) => return Err(io::Error::from(ErrorKind::UnexpectedEof)),
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        let whole = self.inbuf.len() / self.hello.frame_bytes() * self.hello.frame_bytes();
        let count = whole / 2;
        out.extend(
            self.inbuf[..whole]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]])),
        );
        self.inbuf.drain(..whole);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Class, Direction};
    use std::os::unix::net::UnixListener;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("uas-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A server that answers the hello with `status` and then runs `then`.
    fn serve(
        tag: &str,
        status: Option<Status>,
        then: impl FnOnce(UnixStream) + Send + 'static,
    ) -> (std::path::PathBuf, std::thread::JoinHandle<()>) {
        let d = dir(tag);
        let p = d.join("s.sock");
        let l = UnixListener::bind(&p).unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut hello = [0u8; HELLO_LEN];
            s.read_exact(&mut hello).unwrap();
            assert!(Hello::decode(&hello).is_some());
            match status {
                Some(st) => s.write_all(&st.encode()).unwrap(),
                None => s.write_all(b"garbage!").unwrap(),
            }
            then(s);
        });
        (p, h)
    }

    #[test]
    fn a_refusal_and_garbage_are_errors() {
        let hello = Hello::new(Class::Call, Direction::Duplex, 16_000, 1);
        let (p, h) = serve("refuse", Some(Status::Denied), |_| {});
        assert!(matches!(
            Stream::connect(&p, hello),
            Err(ConnectError::Refused(Status::Denied))
        ));
        h.join().unwrap();
        let (p, h) = serve("garbage", None, |_| {});
        assert!(matches!(
            Stream::connect(&p, hello),
            Err(ConnectError::Protocol)
        ));
        h.join().unwrap();
        assert!(matches!(
            Stream::connect(&std::env::temp_dir().join("uas-none/x"), hello),
            Err(ConnectError::Io(_))
        ));
        let mut bad = hello;
        bad.rate = 1;
        assert!(matches!(
            Stream::connect(&p, bad),
            Err(ConnectError::Refused(Status::BadHello))
        ));
    }

    #[test]
    fn samples_go_out_whole_frames_and_come_back() {
        let hello = Hello::new(Class::Media, Direction::Duplex, 48_000, 2);
        let (p, h) = serve("pcm", Some(Status::Ok), |mut s| {
            let mut got = [0u8; 8];
            s.read_exact(&mut got).unwrap();
            assert_eq!(got, [1, 0, 2, 0, 3, 0, 4, 0]);
            // three bytes of a frame, then the rest: nothing is cut in two
            s.write_all(&[9, 0, 8, 0, 7]).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            s.write_all(&[0, 6, 0, 5]).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let mut st = Stream::connect(&p, hello).unwrap();
        // five samples: the odd one is not a frame and is dropped
        assert_eq!(st.push(&[1, 2, 3, 4, 5]).unwrap(), 4);
        assert_eq!(st.dropped(), 1);
        let mut got = Vec::new();
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while got.len() < 4 && std::time::Instant::now() < end {
            st.pull(&mut got).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got, vec![9, 8, 7, 6]);
        h.join().unwrap();
        // the server is gone: the next calls say so
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while st.pull(&mut got).is_ok() && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(st.pull(&mut got).is_err());
    }

    #[test]
    fn a_server_that_does_not_read_costs_a_bounded_backlog() {
        let hello = Hello::new(Class::Media, Direction::Play, 8_000, 1);
        let (p, h) = serve("full", Some(Status::Ok), |s| {
            std::thread::sleep(Duration::from_millis(600));
            drop(s);
        });
        let mut st = Stream::connect(&p, hello).unwrap();
        let chunk = vec![1i16; 8_000];
        let mut accepted = 0;
        for _ in 0..200 {
            accepted += st.push(&chunk).unwrap();
        }
        // never more than what the socket buffer and the 200 ms backlog hold, and never blocked
        assert!(st.backlog() <= 8_000 * 2 / 5);
        assert!(st.dropped() > 0 && accepted < 200 * 8_000);
        h.join().unwrap();
    }
}
