//! The sound card of the Ultar kernel (`/dev/ultar/pcm`, 48 kHz, 16 bit, stereo): the
//! non-blocking writes the GStreamer sink of the `media` feature uses, and the same logic the
//! browser's media service runs (`apps/ultar-web/src/media/sink.rs`). Without the card (Linux,
//! no sound card, another program holds it) the sound is dropped and the pictures still follow
//! the clock.
//!
//! The stream is interleaved 16-bit stereo, so one frame is [`FRAME`] bytes. A write to a busy
//! card may take only a part of a buffer, even a part of a frame; a stream that continues in the
//! middle of a frame swaps the channels for ever, so [`push`] finishes the frame it has begun (a
//! few retries) before it gives up on the rest of the buffer.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// The sound card node of the Ultar kernel. The bundle keeps the device node at this standard
/// place; a test may point the sink at any file through [`Sink::open`].
pub const PCM_NODE: &str = "/dev/ultar/pcm";

/// Sample rate of the stream in Hz.
pub const RATE: u32 = 48_000;
/// Channels of the stream.
pub const CHANNELS: u32 = 2;
/// Bytes of one frame: one 16-bit sample per channel.
pub const FRAME: usize = 2 * CHANNELS as usize;

/// `O_NONBLOCK`: a write to a busy stream returns [`ErrorKind::WouldBlock`] instead of waiting.
const NONBLOCK: i32 = 0o4000;

/// How often a half-written frame is tried again before the card counts as stuck.
const FINISH_FRAME_TRIES: usize = 64;

/// What [`push`] did with one buffer.
#[derive(Debug, PartialEq, Eq)]
pub struct Pushed {
    /// Bytes the card took (whole frames, unless the card is stuck or gone).
    pub taken: usize,
    /// `true` when the card is gone for good (a write failed for another reason than "busy").
    pub gone: bool,
}

/// Writes as much of `bytes` to `out` as it takes now. Stops at the first sign that the card is
/// busy (the caller drops the rest of the buffer: no backlog to grow, the pictures keep their
/// clock). A frame that was begun is finished first, so no frame is left cut in two.
pub fn push<W: Write>(out: &mut W, bytes: &[u8]) -> Pushed {
    let mut taken = 0;
    while taken < bytes.len() {
        match out.write(&bytes[taken..]) {
            Ok(0) => break,
            Ok(n) => taken += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(_) => return Pushed { taken, gone: true },
        }
    }
    // finish a frame that was cut: the card has room for the few bytes in a moment
    let mut tries = 0;
    while taken % FRAME != 0 && taken < bytes.len() && tries < FINISH_FRAME_TRIES {
        let end = (taken + (FRAME - taken % FRAME)).min(bytes.len());
        match out.write(&bytes[taken..end]) {
            Ok(n) if n > 0 => taken += n,
            Ok(_) => tries += 1,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                tries += 1;
                std::thread::yield_now();
            }
            Err(_) => return Pushed { taken, gone: true },
        }
    }
    Pushed { taken, gone: false }
}

/// The sound output.
pub enum Sink {
    /// The sound card.
    Card(File),
    /// No sound card: sound is dropped.
    Silent,
}

impl Sink {
    /// The sound card at `path`, else [`Sink::Silent`].
    pub fn open(path: &Path) -> Sink {
        OpenOptions::new()
            .write(true)
            .custom_flags(NONBLOCK)
            .open(path)
            .map_or(Sink::Silent, Sink::Card)
    }

    /// `true` when the sound is heard.
    pub fn audible(&self) -> bool {
        matches!(self, Sink::Card(_))
    }

    /// Queues as much of `bytes` (48 kHz, 16 bit, stereo) as the card takes now; returns how
    /// many bytes it took (all of them when silent). A busy card takes nothing more this round.
    pub fn offer(&mut self, bytes: &[u8]) -> usize {
        match self {
            Sink::Silent => bytes.len(),
            Sink::Card(f) => {
                let pushed = push(f, bytes);
                if pushed.gone {
                    // the card went away: carry on without sound
                    *self = Sink::Silent;
                    return bytes.len();
                }
                pushed.taken
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// A card that answers the first calls from a script, then takes `room` bytes per call.
    struct Card {
        taken: Vec<u8>,
        script: Vec<io::Result<usize>>,
        room: usize,
    }

    impl Write for Card {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let answer = if self.script.is_empty() {
                Ok(buf.len().min(self.room))
            } else {
                self.script.remove(0)
            };
            let n = answer?.min(buf.len());
            self.taken.extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn busy() -> io::Result<usize> {
        Err(io::Error::from(ErrorKind::WouldBlock))
    }

    #[test]
    fn without_a_card_the_sound_is_dropped() {
        let mut s = Sink::open(Path::new("/nonexistent/ultar-pcm"));
        assert!(!s.audible());
        assert_eq!(s.offer(&[0; 64]), 64);
    }

    #[test]
    fn a_writable_node_takes_the_stream() {
        let mut s = Sink::open(Path::new("/dev/null"));
        assert!(s.audible());
        // /dev/null takes everything, so a full stereo frame is consumed.
        assert_eq!(s.offer(&[0u8; 32 * 4]), 32 * 4);
    }

    #[test]
    fn the_default_node_lies_under_dev_ultar() {
        assert_eq!(PCM_NODE, "/dev/ultar/pcm");
        assert_eq!((RATE, CHANNELS, FRAME), (48_000, 2, 4));
    }

    #[test]
    fn a_free_card_takes_the_whole_buffer() {
        let mut card = Card {
            taken: vec![],
            script: vec![],
            room: usize::MAX,
        };
        let data: Vec<u8> = (0..64).collect();
        assert_eq!(
            push(&mut card, &data),
            Pushed {
                taken: 64,
                gone: false
            }
        );
        assert_eq!(card.taken, data);
    }

    #[test]
    fn a_busy_card_drops_the_rest_without_waiting() {
        let mut card = Card {
            taken: vec![],
            script: vec![Ok(8), busy()],
            room: 0,
        };
        // 8 bytes (two whole frames) went in, then the card was busy: the rest is dropped
        assert_eq!(
            push(&mut card, &[7u8; 64]),
            Pushed {
                taken: 8,
                gone: false
            }
        );
    }

    #[test]
    fn a_cut_frame_is_finished_so_the_channels_stay_in_place() {
        // the card takes 6 bytes (a frame and a half), is busy twice, then takes the 2 missing
        let mut card = Card {
            taken: vec![],
            script: vec![Ok(6), busy(), busy(), Ok(2)],
            room: 0,
        };
        let data: Vec<u8> = (0..32).collect();
        let pushed = push(&mut card, &data);
        assert_eq!(
            pushed,
            Pushed {
                taken: 8,
                gone: false
            }
        );
        assert_eq!(card.taken, data[..8]);
    }

    #[test]
    fn a_stuck_card_gives_up_after_a_few_tries() {
        let mut script = vec![Ok(2)];
        script.extend((0..=FINISH_FRAME_TRIES).map(|_| busy()));
        let mut card = Card {
            taken: vec![],
            script,
            room: 0,
        };
        // the half frame could not be finished: the caller sees what was taken and stops
        assert_eq!(
            push(&mut card, &[1u8; 16]),
            Pushed {
                taken: 2,
                gone: false
            }
        );
    }

    #[test]
    fn interrupted_writes_are_repeated() {
        let mut card = Card {
            taken: vec![],
            script: vec![Err(io::Error::from(ErrorKind::Interrupted)), Ok(4)],
            room: usize::MAX,
        };
        assert_eq!(
            push(&mut card, &[3u8; 4]),
            Pushed {
                taken: 4,
                gone: false
            }
        );
    }

    #[test]
    fn a_card_that_fails_for_good_is_reported_gone() {
        let mut card = Card {
            taken: vec![],
            script: vec![Ok(4), Err(io::Error::from(ErrorKind::BrokenPipe))],
            room: 0,
        };
        assert_eq!(
            push(&mut card, &[1u8; 16]),
            Pushed {
                taken: 4,
                gone: true
            }
        );
    }

    #[test]
    fn a_zero_write_counts_as_busy() {
        let mut card = Card {
            taken: vec![],
            script: vec![Ok(0)],
            room: 0,
        };
        assert_eq!(
            push(&mut card, &[1u8; 16]),
            Pushed {
                taken: 0,
                gone: false
            }
        );
    }
}
