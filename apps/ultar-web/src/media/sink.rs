//! Where the sound goes: the sound card's running stream `/dev/ultar/pcm` of the Ultar kernel
//! (48 kHz, 16 bit, stereo; the app needs the kernel right `sound`), or nowhere when there is
//! none (Linux, no sound card, another program holds it): then the sound is dropped and the
//! pictures still follow the clock.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// The sound card node of the Ultar kernel (from the HAL).
pub fn pcm() -> std::path::PathBuf {
    ultar_hal_system::seat_nodes::pcm_node(Path::new("/"))
}
/// `O_NONBLOCK`.
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

    /// `true` when the sound is heard.
    pub fn audible(&self) -> bool {
        matches!(self, Sink::Card(_))
    }

    /// Queues as much of `bytes` (48 kHz stereo 16 bit) as the card takes now; returns how
    /// many bytes it took (all of them when silent).
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
        let mut s = Sink::open(Path::new("/nonexistent/pcm"));
        assert!(!s.audible());
        assert_eq!(s.offer(&[0; 64]), 64);
    }
}
