//! The sound card of the Ultar kernel (`/dev/ultar/pcm`, 48 kHz · 16 bit · stereo): the
//! non-blocking writes the GStreamer sink of the `media` feature uses, and the same logic the
//! browser's media service runs (`apps/ultar-web/src/media/sink.rs`). Without the card (Linux,
//! no sound card, another program holds it) the sound is dropped and the pictures still follow
//! the clock.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// The sound card node of the Ultar kernel. The bundle keeps the device node at this standard
/// place; a test may point the sink at any file through [`Sink::open`].
pub const PCM_NODE: &str = "/dev/ultar/pcm";

/// `O_NONBLOCK`: a write to a busy stream returns [`ErrorKind::WouldBlock`] instead of waiting.
const NONBLOCK: i32 = 0o4000;

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

    /// The sound card of the Ultar kernel, else [`Sink::Silent`].
    pub fn open_default() -> Sink {
        Sink::open(Path::new(PCM_NODE))
    }

    /// `true` when the sound is heard.
    pub fn audible(&self) -> bool {
        matches!(self, Sink::Card(_))
    }

    /// Queues as much of `bytes` (48 kHz, 16 bit, stereo) as the card takes now, in 16-bit
    /// samples; returns how many bytes it took (all of them when silent). A card that is busy
    /// (`WouldBlock`) takes nothing this round.
    pub fn offer(&mut self, bytes: &[u8]) -> usize {
        match self {
            Sink::Silent => bytes.len(),
            Sink::Card(f) => match f.write(bytes) {
                Ok(n) => n,
                Err(e)
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted =>
                {
                    0
                }
                Err(_) => {
                    // the card went away: carry on without sound
                    *self = Sink::Silent;
                    bytes.len()
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}