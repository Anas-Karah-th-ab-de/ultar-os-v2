//! The stream protocol of the Ultar sound server (`ultar-audiod`).
//!
//! A program that wants sound connects to its own stream socket of `ultar-audiod` (one
//! socket per program: the socket is the identity, like the bus sockets), sends a 16-byte
//! [`Hello`], reads an 8-byte reply ([`Status`]) and from then on the connection carries
//! **raw PCM**: 16-bit little endian, interleaved, in the format the hello declared.
//!
//! * play: the client writes samples, the server mixes them into the sound card;
//! * capture: the server writes the microphone samples (resampled to the declared rate,
//!   with echo cancellation when asked for) and the client reads them;
//! * duplex: both at once on the same socket (a call).
//!
//! Nobody ever blocks anybody: the server reads a client only while its buffer has room (a
//! client that is faster than the card sees "would block" and drops, like on the kernel
//! node today) and drops what a recording client does not read. [`client::Stream`] is the
//! non-blocking client everybody uses.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod client;

pub use client::{ConnectError, Stream};

/// Size of the hello in bytes.
pub const HELLO_LEN: usize = 16;
/// Size of the reply in bytes.
pub const REPLY_LEN: usize = 8;
/// Magic of the hello.
pub const MAGIC: [u8; 4] = *b"UAS1";
/// Magic of the reply.
pub const REPLY_MAGIC: [u8; 4] = *b"UASR";
/// Lowest sample rate accepted (Hertz).
pub const MIN_RATE: u32 = 8_000;
/// Highest sample rate accepted (Hertz).
pub const MAX_RATE: u32 = 96_000;
/// The rate of the sound card the server mixes at (Hertz).
pub const CARD_RATE: u32 = 48_000;
/// Unity volume in [`Hello::volume`] (per mille).
pub const UNITY: u16 = 1000;
/// Largest volume in [`Hello::volume`] (per mille; 2.0, +6 dB).
pub const MAX_VOLUME: u16 = 2000;

/// What a stream is for. The class decides the priority (who ducks whom) and which right
/// of the bus policy the program needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Class {
    /// Music, video, web pages, "play there": ducked by everything below.
    Media,
    /// Short clips of the system: the listening tone, the spoken answer.
    Prompt,
    /// The ringing of an incoming call (over media).
    Ring,
    /// The voice of a call: everything else is ducked.
    Call,
}

impl Class {
    /// All classes.
    pub const ALL: [Class; 4] = [Class::Media, Class::Prompt, Class::Ring, Class::Call];

    /// The byte of the class in the hello.
    pub fn byte(self) -> u8 {
        match self {
            Class::Media => 0,
            Class::Prompt => 1,
            Class::Ring => 2,
            Class::Call => 3,
        }
    }

    /// The class of a hello byte.
    pub fn from_byte(b: u8) -> Option<Class> {
        Class::ALL.into_iter().find(|c| c.byte() == b)
    }

    /// The pseudo method of the bus policy that allows this class (`audio.<name>`); plain
    /// media needs `audio.play`, the others their own right.
    pub fn right(self) -> &'static str {
        match self {
            Class::Media | Class::Prompt => "play",
            Class::Ring => "ring",
            Class::Call => "call",
        }
    }
}

/// What the stream does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// The client plays.
    Play,
    /// The client records.
    Capture,
    /// Both.
    Duplex,
}

impl Direction {
    /// The client plays.
    pub fn plays(self) -> bool {
        matches!(self, Direction::Play | Direction::Duplex)
    }

    /// The client records (needs the microphone right).
    pub fn records(self) -> bool {
        matches!(self, Direction::Capture | Direction::Duplex)
    }

    fn byte(self) -> u8 {
        match self {
            Direction::Play => 1,
            Direction::Capture => 2,
            Direction::Duplex => 3,
        }
    }

    fn from_byte(b: u8) -> Option<Direction> {
        match b {
            1 => Some(Direction::Play),
            2 => Some(Direction::Capture),
            3 => Some(Direction::Duplex),
            _ => None,
        }
    }
}

/// The first thing a client sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hello {
    /// What the stream is for.
    pub class: Class,
    /// Play, record or both.
    pub direction: Direction,
    /// 1 (mono) or 2 (stereo, left then right).
    pub channels: u8,
    /// Sample rate of the client's PCM in both directions (Hertz).
    pub rate: u32,
    /// Volume of this stream in per mille (1000 = unchanged).
    pub volume: u16,
    /// Apply echo cancellation to the recording (a call on the device's own speaker).
    pub echo_cancel: bool,
}

impl Hello {
    /// A hello with unity volume and no echo cancellation.
    pub fn new(class: Class, direction: Direction, rate: u32, channels: u8) -> Self {
        Self {
            class,
            direction,
            channels,
            rate,
            volume: UNITY,
            echo_cancel: false,
        }
    }

    /// Whether the values are inside what the server accepts.
    pub fn valid(&self) -> bool {
        (MIN_RATE..=MAX_RATE).contains(&self.rate)
            && (self.channels == 1 || self.channels == 2)
            && self.volume <= MAX_VOLUME
    }

    /// The 16 bytes.
    pub fn encode(&self) -> [u8; HELLO_LEN] {
        let mut b = [0u8; HELLO_LEN];
        b[..4].copy_from_slice(&MAGIC);
        b[4] = self.class.byte();
        b[5] = self.direction.byte();
        b[6] = self.channels;
        b[7] = u8::from(self.echo_cancel);
        b[8..12].copy_from_slice(&self.rate.to_le_bytes());
        b[12..14].copy_from_slice(&self.volume.to_le_bytes());
        b
    }

    /// Reads a hello; `None` for a wrong magic, an unknown class or direction, values out
    /// of range or reserved bytes that are not zero.
    pub fn decode(b: &[u8; HELLO_LEN]) -> Option<Hello> {
        if b[..4] != MAGIC || b[7] > 1 || b[14] != 0 || b[15] != 0 {
            return None;
        }
        let hello = Hello {
            class: Class::from_byte(b[4])?,
            direction: Direction::from_byte(b[5])?,
            channels: b[6],
            echo_cancel: b[7] == 1,
            rate: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            volume: u16::from_le_bytes([b[12], b[13]]),
        };
        hello.valid().then_some(hello)
    }

    /// Bytes of one frame (all channels) of the client's PCM.
    pub fn frame_bytes(&self) -> usize {
        usize::from(self.channels) * 2
    }
}

/// The server's answer to a hello.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// The stream is open.
    Ok,
    /// The program has no right for this class or for the microphone.
    Denied,
    /// There is no sound card: the stream is refused (the client runs silent).
    NoDevice,
    /// The hello was wrong.
    BadHello,
    /// Too many streams are open.
    TooMany,
}

impl Status {
    fn byte(self) -> u8 {
        match self {
            Status::Ok => 0,
            Status::Denied => 1,
            Status::NoDevice => 2,
            Status::BadHello => 3,
            Status::TooMany => 4,
        }
    }

    /// The 8 bytes of the reply.
    pub fn encode(self) -> [u8; REPLY_LEN] {
        let mut b = [0u8; REPLY_LEN];
        b[..4].copy_from_slice(&REPLY_MAGIC);
        b[4] = self.byte();
        b
    }

    /// Reads a reply; `None` for a wrong magic or an unknown status.
    pub fn decode(b: &[u8; REPLY_LEN]) -> Option<Status> {
        if b[..4] != REPLY_MAGIC {
            return None;
        }
        match b[4] {
            0 => Some(Status::Ok),
            1 => Some(Status::Denied),
            2 => Some(Status::NoDevice),
            3 => Some(Status::BadHello),
            4 => Some(Status::TooMany),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_and_bad_ones_are_refused() {
        for class in Class::ALL {
            for dir in [Direction::Play, Direction::Capture, Direction::Duplex] {
                let mut h = Hello::new(class, dir, 16_000, 1);
                h.echo_cancel = dir.records();
                h.volume = 700;
                assert_eq!(Hello::decode(&h.encode()), Some(h));
            }
        }
        let good = Hello::new(Class::Call, Direction::Duplex, 48_000, 2).encode();
        let mut bad = good;
        bad[0] = b'X';
        assert_eq!(Hello::decode(&bad), None, "magic");
        for (i, v) in [(4, 9u8), (5, 0), (5, 4), (6, 0), (6, 3), (7, 2), (14, 1)] {
            let mut b = good;
            b[i] = v;
            assert_eq!(Hello::decode(&b), None, "byte {i} = {v}");
        }
        let mut b = good;
        b[8..12].copy_from_slice(&7_999u32.to_le_bytes());
        assert_eq!(Hello::decode(&b), None, "rate too low");
        b[8..12].copy_from_slice(&96_001u32.to_le_bytes());
        assert_eq!(Hello::decode(&b), None, "rate too high");
        let mut b = good;
        b[12..14].copy_from_slice(&2001u16.to_le_bytes());
        assert_eq!(Hello::decode(&b), None, "volume too high");
    }

    #[test]
    fn replies_round_trip() {
        for s in [
            Status::Ok,
            Status::Denied,
            Status::NoDevice,
            Status::BadHello,
            Status::TooMany,
        ] {
            assert_eq!(Status::decode(&s.encode()), Some(s));
        }
        let mut b = Status::Ok.encode();
        b[4] = 77;
        assert_eq!(Status::decode(&b), None);
        b = Status::Ok.encode();
        b[0] = 0;
        assert_eq!(Status::decode(&b), None);
    }

    #[test]
    fn the_class_names_its_right() {
        assert_eq!(Class::Media.right(), "play");
        assert_eq!(Class::Prompt.right(), "play");
        assert_eq!(Class::Ring.right(), "ring");
        assert_eq!(Class::Call.right(), "call");
        assert!(Direction::Duplex.plays() && Direction::Duplex.records());
        assert!(!Direction::Play.records() && !Direction::Capture.plays());
        assert_eq!(
            Hello::new(Class::Media, Direction::Play, 44_100, 2).frame_bytes(),
            4
        );
    }
}
