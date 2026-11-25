//! Microphone and speaker of a call through the sound card's running streams
//! (`/dev/ultar/pcm` of the Ultar kernel: 48 kHz, 16 bit, stereo; right `sound`). The call's
//! voice is 16 kHz mono ([`crate::e2e::audio`]); this converts both ways and never blocks.

use crate::e2e::audio::{from_card, to_card, FRAME};
use crate::e2e::media::AudioIo;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// `O_NONBLOCK`.
const NONBLOCK: i32 = 0o4000;
/// Bytes of 20 ms at 48 kHz stereo.
const CARD_FRAME: usize = FRAME * 3 * 2 * 2;

/// The sound card's streams.
pub struct PcmAudio {
    node: File,
    recorded: Vec<u8>,
}

impl PcmAudio {
    /// Opens the node (one program at a time).
    pub fn open(path: &Path) -> io::Result<Self> {
        let node = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(NONBLOCK)
            .open(path)?;
        Ok(Self {
            node,
            recorded: Vec::with_capacity(CARD_FRAME * 4),
        })
    }
}

fn to_samples(b: &[u8]) -> Vec<i16> {
    b.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

impl AudioIo for PcmAudio {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let mut buf = [0u8; CARD_FRAME * 2];
        if let Ok(n) = self.node.read(&mut buf) {
            self.recorded.extend_from_slice(&buf[..n]);
        }
        if self.recorded.len() > CARD_FRAME * 8 {
            // far behind: keep the newest 20 ms
            let cut = self.recorded.len() - CARD_FRAME;
            self.recorded.drain(..cut);
        }
        if self.recorded.len() < CARD_FRAME {
            return None;
        }
        let frame: Vec<u8> = self.recorded.drain(..CARD_FRAME).collect();
        Some(from_card(&to_samples(&frame)))
    }

    fn play(&mut self, frame: &[i16]) {
        let bytes: Vec<u8> = to_card(frame)
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        // what does not fit is dropped: late voice is worse than a gap
        let _ = self.node.write(&bytes);
    }
}

/// No sound card: the call runs, nothing is heard or recorded.
pub struct Silent;

impl AudioIo for Silent {
    fn capture(&mut self) -> Option<Vec<i16>> {
        Some(vec![0; FRAME])
    }
    fn play(&mut self, _frame: &[i16]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_stands_in_for_the_node() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("pcm");
        let card: Vec<u8> = (0..CARD_FRAME / 2)
            .flat_map(|i| ((i % 100) as i16 * 100).to_le_bytes())
            .collect();
        std::fs::write(&p, &card).unwrap();
        let mut a = PcmAudio::open(&p).unwrap();
        let f = a.capture().unwrap();
        assert_eq!(f.len(), FRAME);
        assert!(a.capture().is_none(), "nothing more recorded");
        a.play(&f);
        assert!(PcmAudio::open(&d.path().join("none")).is_err());
        assert_eq!(Silent.capture().unwrap(), vec![0; FRAME]);
    }
}
