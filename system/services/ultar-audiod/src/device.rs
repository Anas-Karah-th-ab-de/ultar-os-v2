//! The sound card as the engine sees it: 48 kHz, 16 bit, interleaved stereo, little
//! endian, and never blocking.
//!
//! * [`NodeDevice`]: the running streams of the Ultar kernel (`/dev/ultar/pcm`, kernel
//!   right `sound`, one holder at a time: this service).
//! * [`FileDevice`]: two plain files, one that everything played is appended to and one
//!   that is read as the microphone (a regular file at its end reads nothing, like a
//!   microphone that is quiet). For development and the process tests; no card needed.
//! * [`AlsaDevice`]: the ALSA streams of a Linux card ([`ultar_hal_linux::sound::pcm`]); the
//!   engine takes every card through the same trait ([`Device`]).

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use ultar_audio_stream::CARD_RATE;
use ultar_hal_linux::sound::pcm::{Direction, Pcm};

/// `O_NONBLOCK`.
const NONBLOCK: i32 = 0o4000;
/// How often the second half of a cut frame is tried before the card counts as stuck.
const FINISH_TRIES: usize = 64;

/// The card's two streams.
pub trait Device: Send {
    /// Plays interleaved stereo samples (whole frames); returns how many samples the card
    /// took now (0: it is full). `Err` when the card is gone.
    fn write(&mut self, samples: &[i16]) -> io::Result<usize>;
    /// Reads recorded interleaved stereo samples into `out`; returns how many (0: nothing
    /// recorded yet). `Err` when the card is gone.
    fn read(&mut self, out: &mut [i16]) -> io::Result<usize>;
}

fn bytes_of(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// Writes `samples`; a frame the card took only half of is finished first (a stream that
/// continues in the middle of a frame swaps the channels for ever), so the count is whole
/// frames unless the card is stuck.
fn write_frames(file: &mut File, samples: &[i16]) -> io::Result<usize> {
    let bytes = bytes_of(samples);
    let mut done = 0;
    let mut stuck = 0;
    while done < bytes.len() && stuck < FINISH_TRIES {
        match file.write(&bytes[done..]) {
            Ok(0) if done % 4 == 0 => break,
            Ok(0) => stuck += 1,
            Ok(n) => done += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if done % 4 == 0 {
                    break;
                }
                stuck += 1;
                std::thread::yield_now();
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done / 4 * 2)
}
fn read_frames(file: &mut File, carry: &mut Vec<u8>, out: &mut [i16]) -> io::Result<usize> {
    let want = out.len() / 2 * 4;
    let mut bytes = std::mem::take(carry);
    let have = bytes.len();
    bytes.resize(have + want, 0);
    let mut got = have;
    loop {
        match file.read(&mut bytes[got..]) {
            Ok(0) => break,
            Ok(n) => {
                got += n;
                if got >= bytes.len() {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let whole = got / 4 * 4;
    *carry = bytes[whole..got].to_vec();
    for (o, c) in out.iter_mut().zip(bytes[..whole].chunks_exact(2)) {
        *o = i16::from_le_bytes([c[0], c[1]]);
    }
    Ok(whole / 2)
}

/// The kernel node.
pub struct NodeDevice {
    file: File,
    rcarry: Vec<u8>,
}

impl NodeDevice {
    /// Opens the node (`EBUSY` when another program holds it, `ENODEV` without a card).
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(NONBLOCK)
            .open(path)?;
        Ok(Self {
            file,
            rcarry: Vec::new(),
        })
    }
}

impl Device for NodeDevice {
    fn write(&mut self, samples: &[i16]) -> io::Result<usize> {
        write_frames(&mut self.file, samples)
    }

    fn read(&mut self, out: &mut [i16]) -> io::Result<usize> {
        read_frames(&mut self.file, &mut self.rcarry, out)
    }
}

/// Two files as a card.
pub struct FileDevice {
    play: File,
    capture_path: std::path::PathBuf,
    capture: Option<File>,
    rcarry: Vec<u8>,
}

impl FileDevice {
    /// Appends everything played to `play` (created) and reads the microphone from
    /// `capture` (it may be created later: nothing is recorded until it exists).
    pub fn open(play: &Path, capture: &Path) -> io::Result<Self> {
        let play = OpenOptions::new().create(true).append(true).open(play)?;
        Ok(Self {
            play,
            capture_path: capture.to_path_buf(),
            capture: File::open(capture).ok(),
            rcarry: Vec::new(),
        })
    }
}

impl Device for FileDevice {
    fn write(&mut self, samples: &[i16]) -> io::Result<usize> {
        write_frames(&mut self.play, samples)
    }

    fn read(&mut self, out: &mut [i16]) -> io::Result<usize> {
        if self.capture.is_none() {
            // the microphone file may appear later
            self.capture = File::open(&self.capture_path).ok();
        }
        match self.capture.as_mut() {
            Some(f) => read_frames(f, &mut self.rcarry, out),
            None => Ok(0),
        }
    }
}

/// The ALSA streams of a Linux card (`/dev/snd/pcmC<N>D<M>p` and `...c`).
pub struct AlsaDevice {
    play: Pcm,
    capture: Option<Pcm>,
}

impl AlsaDevice {
    /// Opens the playback stream and, when `capture` is given, the capture stream (both
    /// 48 kHz, stereo, 16 bit).
    pub fn open(play: &Path, capture: Option<&Path>) -> io::Result<Self> {
        Ok(Self {
            play: Pcm::open(play, Direction::Playback, CARD_RATE)?,
            capture: capture
                .map(|p| Pcm::open(p, Direction::Capture, CARD_RATE))
                .transpose()?,
        })
    }

    /// Under- and overruns recovered so far (playback, capture).
    pub fn xruns(&self) -> (u64, u64) {
        (
            self.play.xruns(),
            self.capture.as_ref().map_or(0, Pcm::xruns),
        )
    }
}

impl Device for AlsaDevice {
    fn write(&mut self, samples: &[i16]) -> io::Result<usize> {
        self.play.write(samples)
    }

    fn read(&mut self, out: &mut [i16]) -> io::Result<usize> {
        match self.capture.as_mut() {
            Some(c) => c.read(out),
            None => Ok(0),
        }
    }
}

#[cfg(test)]
pub mod fake {
    //! A card in memory for the engine tests.
    use super::Device;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    /// What the card was given and what it records next.
    #[derive(Default)]
    pub struct Card {
        /// Everything played (interleaved stereo).
        pub played: Vec<i16>,
        /// What the microphone delivers (interleaved stereo).
        pub mic: VecDeque<i16>,
        /// Samples the card takes per write at most (`None`: all).
        pub room: Option<usize>,
        /// The card is gone.
        pub gone: bool,
    }

    /// The device end of a [`Card`].
    pub struct Fake(pub Arc<Mutex<Card>>);

    impl Fake {
        /// A fake card and the handle a test keeps.
        pub fn new() -> (Fake, Arc<Mutex<Card>>) {
            let card = Arc::new(Mutex::new(Card::default()));
            (Fake(card.clone()), card)
        }
    }

    impl Device for Fake {
        fn write(&mut self, samples: &[i16]) -> io::Result<usize> {
            let mut c = self.0.lock().unwrap();
            if c.gone {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            let take = c.room.map_or(samples.len(), |r| r.min(samples.len())) / 2 * 2;
            c.played.extend_from_slice(&samples[..take]);
            Ok(take)
        }

        fn read(&mut self, out: &mut [i16]) -> io::Result<usize> {
            let mut c = self.0.lock().unwrap();
            if c.gone {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            let n = (out.len() / 2 * 2).min(c.mic.len() / 2 * 2);
            for o in out.iter_mut().take(n) {
                *o = c.mic.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_stand_in_for_the_card() {
        let d = std::env::temp_dir().join(format!("audiod-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (play, cap) = (d.join("play.raw"), d.join("cap.raw"));
        let mut dev = FileDevice::open(&play, &cap).unwrap();
        assert_eq!(dev.write(&[1, -2, 3, -4]).unwrap(), 4);
        assert_eq!(
            std::fs::read(&play).unwrap(),
            vec![1, 0, 254, 255, 3, 0, 252, 255]
        );
        let mut buf = [0i16; 8];
        assert_eq!(dev.read(&mut buf).unwrap(), 0, "no microphone file yet");
        let mut f = std::fs::File::create(&cap).unwrap();
        // three samples: the second frame is cut and waits for its second half
        f.write_all(&bytes_of(&[10, 20, 30])).unwrap();
        assert_eq!(dev.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], &[10, 20]);
        f.write_all(&bytes_of(&[40, 50, 60])).unwrap();
        assert_eq!(dev.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], &[30, 40, 50, 60]);
        assert_eq!(dev.read(&mut buf).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_node_is_an_error_not_a_panic() {
        assert!(NodeDevice::open(Path::new("/nonexistent/ultar/pcm")).is_err());
    }
}
