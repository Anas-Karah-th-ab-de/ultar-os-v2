//! One connection of a program to the sound server: the hello, the rights check, and the
//! playing half (bytes in, 48 kHz stereo frames out).

use crate::resample::{to_f32, Resampler};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use ultar_audio_stream::{Class, Direction, Hello, Status, CARD_RATE, HELLO_LEN};

/// How long a connection may take to send its hello.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(2);

/// What a program may do, decided by the bus policy for the identity of its socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rights {
    /// Play media and prompts (`audio.play`).
    pub play: bool,
    /// Play the ring of an incoming call (`audio.ring`).
    pub ring: bool,
    /// Play and record the voice of a call with priority (`audio.call`).
    pub call: bool,
    /// Record the microphone (`audio.capture`).
    pub capture: bool,
}

impl Rights {
    /// Everything (for tests and development).
    pub fn all() -> Self {
        Self {
            play: true,
            ring: true,
            call: true,
            capture: true,
        }
    }

    /// Whether a stream as `hello` says is allowed.
    pub fn allow(&self, hello: &Hello) -> bool {
        let class = match hello.class {
            Class::Media | Class::Prompt => self.play,
            Class::Ring => self.ring,
            Class::Call => self.call,
        };
        // playing needs the right of the class, recording the microphone right
        (!hello.direction.plays() || class) && (!hello.direction.records() || self.capture)
    }
}

/// The playing half of a stream: client PCM in, mixer frames out.
pub struct PlayIn {
    channels: usize,
    left: Resampler,
    right: Option<Resampler>,
    raw: Vec<u8>,
    rate: u32,
}

impl PlayIn {
    /// The playing side of the stream `hello` opened.
    pub fn new(hello: &Hello) -> Self {
        Self {
            channels: usize::from(hello.channels),
            left: Resampler::new(hello.rate, CARD_RATE),
            right: (hello.channels == 2).then(|| Resampler::new(hello.rate, CARD_RATE)),
            raw: Vec::new(),
            rate: hello.rate,
        }
    }

    /// Bytes of client PCM that make up at most `frames` mixer frames (never less than one
    /// client frame).
    pub fn bytes_for(&self, frames: usize) -> usize {
        let client_frames = (frames as u64 * u64::from(self.rate) / u64::from(CARD_RATE)).max(1);
        client_frames as usize * self.channels * 2
    }

    /// Takes client bytes and returns mixer frames (whole client frames are used, the rest
    /// waits).
    pub fn take(&mut self, bytes: &[u8]) -> Vec<(f32, f32)> {
        self.raw.extend_from_slice(bytes);
        let frame = self.channels * 2;
        let whole = self.raw.len() / frame * frame;
        let samples: Vec<i16> = self.raw[..whole]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        self.raw.drain(..whole);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        if let Some(right) = self.right.as_mut() {
            let (a, b): (Vec<f32>, Vec<f32>) = to_f32(&samples)
                .chunks_exact(2)
                .map(|f| (f[0], f[1]))
                .unzip();
            self.left.process(&a, &mut l);
            right.process(&b, &mut r);
        } else {
            self.left.process(&to_f32(&samples), &mut l);
            r.clone_from(&l);
        }
        l.into_iter().zip(r).collect()
    }
}

/// A connection after its hello.
pub struct Open {
    /// What the program asked for.
    pub hello: Hello,
    /// The playing half.
    pub play: Option<PlayIn>,
    /// The recording half.
    pub rec: Option<crate::capture::Rec>,
}

/// Where a connection is.
pub enum Phase {
    /// Waiting for the 16 bytes.
    Hello(Vec<u8>),
    /// Streaming.
    Open(Box<Open>),
    /// The program is gone or sent nonsense: the stream plays out and the entry is removed.
    Closing,
}

/// One accepted connection.
pub struct Conn {
    /// Number of the stream (also its number in the mixer).
    pub id: u64,
    /// Identity of the socket it came in on.
    pub identity: String,
    /// Rights of that identity.
    pub rights: Rights,
    /// The socket (non-blocking).
    pub sock: UnixStream,
    /// When it connected.
    pub born: Instant,
    /// Where it is.
    pub phase: Phase,
    /// It has a stream in the mixer.
    pub voice: bool,
}

/// How a hello went.
pub enum Greeted {
    /// Not complete yet.
    Waiting,
    /// A valid hello; the reply is still to be sent.
    Hello(Hello),
    /// The connection is dead or sent nonsense (it has been told).
    Refused,
}

impl Conn {
    /// A new connection on a socket of `identity`.
    pub fn new(id: u64, identity: String, rights: Rights, sock: UnixStream, now: Instant) -> Self {
        Self {
            id,
            identity,
            rights,
            sock,
            born: now,
            phase: Phase::Hello(Vec::new()),
            voice: false,
        }
    }

    /// Reads the hello (as far as it has come).
    pub fn greet(&mut self, now: Instant) -> Greeted {
        let Phase::Hello(buf) = &mut self.phase else {
            return Greeted::Waiting;
        };
        let mut chunk = [0u8; HELLO_LEN];
        while buf.len() < HELLO_LEN {
            match self.sock.read(&mut chunk[..HELLO_LEN - buf.len()]) {
                Ok(0) => return Greeted::Refused,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    return if now.duration_since(self.born) > HELLO_TIMEOUT {
                        Greeted::Refused
                    } else {
                        Greeted::Waiting
                    };
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Greeted::Refused,
            }
        }
        let mut raw = [0u8; HELLO_LEN];
        raw.copy_from_slice(buf);
        match Hello::decode(&raw) {
            Some(h) => Greeted::Hello(h),
            None => {
                self.reply(Status::BadHello);
                Greeted::Refused
            }
        }
    }

    /// Sends the reply of the hello (eight bytes into an empty socket buffer).
    pub fn reply(&mut self, status: Status) -> bool {
        self.sock.write_all(&status.encode()).is_ok()
    }

    /// Opens the stream after an `Ok` reply.
    pub fn open(&mut self, hello: Hello) {
        self.phase = Phase::Open(Box::new(Open {
            hello,
            play: hello.direction.plays().then(|| PlayIn::new(&hello)),
            rec: hello
                .direction
                .records()
                .then(|| crate::capture::Rec::new(&hello)),
        }));
    }

    /// Whether the stream records.
    pub fn records(&self) -> bool {
        matches!(&self.phase, Phase::Open(o) if o.hello.direction.records())
    }
}

/// Reads up to `max` bytes without blocking: `Ok(None)` when nothing is there, `Err` at
/// the end of the stream or when it broke.
pub fn read_some(sock: &mut UnixStream, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut buf = vec![0u8; max.min(16 * 1024)];
    loop {
        match sock.read(&mut buf) {
            Ok(0) => return Err(io::Error::from(ErrorKind::UnexpectedEof)),
            Ok(n) => {
                buf.truncate(n);
                return Ok(Some(buf));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// What a stream's direction means for the check of rights (kept here for the tests).
pub fn needs_capture(d: Direction) -> bool {
    d.records()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_decide_class_and_microphone() {
        let play_only = Rights {
            play: true,
            ..Rights::default()
        };
        let media = Hello::new(Class::Media, Direction::Play, 44_100, 2);
        assert!(play_only.allow(&media));
        let mic = Hello::new(Class::Media, Direction::Capture, 16_000, 1);
        assert!(!play_only.allow(&mic), "no microphone without the right");
        let ring = Hello::new(Class::Ring, Direction::Play, 48_000, 2);
        assert!(!play_only.allow(&ring));
        let call = Hello::new(Class::Call, Direction::Duplex, 16_000, 1);
        assert!(!play_only.allow(&call));
        let caller = Rights {
            call: true,
            capture: true,
            ..Rights::default()
        };
        assert!(caller.allow(&call));
        assert!(
            !caller.allow(&media),
            "a call right is not the right to play media"
        );
        let no_mic = Rights {
            call: true,
            ..Rights::default()
        };
        assert!(
            !no_mic.allow(&call),
            "a call needs the microphone right to record"
        );
        assert!(Rights::all().allow(&call) && Rights::all().allow(&ring));
        assert!(!Rights::default().allow(&media));
        assert!(needs_capture(Direction::Duplex) && !needs_capture(Direction::Play));
    }

    #[test]
    fn client_pcm_becomes_48k_frames_and_odd_bytes_wait() {
        let mut mono = PlayIn::new(&Hello::new(Class::Media, Direction::Play, 48_000, 1));
        let bytes: Vec<u8> = [1000i16, -1000]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let f = mono.take(&bytes[..3]);
        assert_eq!(f.len(), 1);
        assert!((f[0].0 - 1000.0 / 32768.0).abs() < 1e-6 && f[0].0 == f[0].1);
        let f = mono.take(&bytes[3..]);
        assert_eq!(f.len(), 1, "the second half completes the second sample");
        let mut stereo = PlayIn::new(&Hello::new(Class::Media, Direction::Play, 48_000, 2));
        let bytes: Vec<u8> = [500i16, -500]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let f = stereo.take(&bytes);
        assert!(f[0].0 > 0.0 && f[0].1 < 0.0, "channels stay apart");
        let slow = PlayIn::new(&Hello::new(Class::Call, Direction::Play, 16_000, 1));
        assert_eq!(
            slow.bytes_for(480),
            160 * 2,
            "10 ms of the card is 10 ms of the client"
        );
        assert_eq!(slow.bytes_for(1), 2, "never less than one client frame");
    }
}
