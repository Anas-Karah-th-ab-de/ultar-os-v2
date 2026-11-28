//! One ALSA PCM stream (`dev/snd/pcmC<N>D<M>p` for playback, `...c` for capture) as the sound
//! server needs it: 48 kHz (or the rate given), 16 bit, two channels, interleaved, never
//! blocking. The parameters go through one `HW_PARAMS` ioctl, the samples through
//! `read(2)` and `write(2)`; an under- or overrun is recovered with `PREPARE` and counted.

use super::pcm_ioctl as ioctl;
use rustix::io::Errno;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// `O_NONBLOCK`.
const NONBLOCK: i32 = 0o4000;
/// Bytes of one frame: two channels of 16 bit.
const FRAME: usize = 4;
/// Frames of one period asked for (10 ms at 48 kHz).
const PERIOD_FRAMES: u32 = 480;
/// Frames of the ring asked for (four periods).
const BUFFER_FRAMES: u32 = 4 * PERIOD_FRAMES;

// layout of `struct snd_pcm_hw_params` (64-bit)
const MASKS: usize = 4;
const MASK_SIZE: usize = 32;
const INTERVALS: usize = 260;
const INTERVAL_SIZE: usize = 12;
const RMASK: usize = 512;
const INFO: usize = 520;
// masks and intervals by number
const ACCESS: usize = 0;
const FORMAT: usize = 1;
const SUBFORMAT: usize = 2;
const CHANNELS: usize = 2;
const RATE: usize = 3;
const PERIOD_SIZE: usize = 5;
const BUFFER_SIZE: usize = 9;
const INTERVAL_COUNT: usize = 12;
/// `SNDRV_PCM_ACCESS_RW_INTERLEAVED`.
const ACCESS_RW_INTERLEAVED: u32 = 3;
/// `SNDRV_PCM_FORMAT_S16_LE`.
const FORMAT_S16_LE: u32 = 2;
/// Flag of an interval: only whole numbers.
const INTEGER: u32 = 4;

/// Which way the stream goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// To the speaker.
    Playback,
    /// From the microphone.
    Capture,
}

fn put(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_ne_bytes());
}

fn get(buf: &[u8], at: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[at..at + 4]);
    u32::from_ne_bytes(b)
}

fn exact(buf: &mut [u8], index: usize, value: u32) {
    let at = INTERVALS + index * INTERVAL_SIZE;
    put(buf, at, value);
    put(buf, at + 4, value);
    put(buf, at + 8, INTEGER);
}

/// The `snd_pcm_hw_params` that ask for S16_LE, interleaved read/write access, two channels
/// at `rate`, and (when given) a period and ring size in frames; everything else is left to
/// the driver.
pub fn hw_request(rate: u32, sizes: Option<(u32, u32)>) -> [u8; ioctl::HW_PARAMS_SIZE] {
    let mut b = [0u8; ioctl::HW_PARAMS_SIZE];
    for mask in 0..3 {
        for word in 0..MASK_SIZE / 4 {
            put(&mut b, MASKS + mask * MASK_SIZE + word * 4, u32::MAX);
        }
    }
    for (mask, bit) in [
        (ACCESS, ACCESS_RW_INTERLEAVED),
        (FORMAT, FORMAT_S16_LE),
        (SUBFORMAT, 0),
    ] {
        let at = MASKS + mask * MASK_SIZE;
        b[at..at + MASK_SIZE].fill(0);
        put(&mut b, at, 1 << bit);
    }
    for i in 0..INTERVAL_COUNT {
        put(&mut b, INTERVALS + i * INTERVAL_SIZE + 4, u32::MAX);
    }
    exact(&mut b, CHANNELS, 2);
    exact(&mut b, RATE, rate);
    if let Some((period, buffer)) = sizes {
        exact(&mut b, PERIOD_SIZE, period);
        exact(&mut b, BUFFER_SIZE, buffer);
    }
    put(&mut b, RMASK, u32::MAX);
    put(&mut b, INFO, u32::MAX);
    b
}

/// An open stream.
pub struct Pcm {
    file: File,
    direction: Direction,
    xruns: u64,
}

impl Pcm {
    /// Opens the stream node `path`, sets the parameters and prepares it. `EBUSY` when
    /// another program holds the stream, `ENOTTY` when `path` is no PCM node.
    pub fn open(path: &Path, direction: Direction, rate: u32) -> io::Result<Pcm> {
        let file = OpenOptions::new()
            .read(direction == Direction::Capture)
            .write(direction == Direction::Playback)
            .custom_flags(NONBLOCK)
            .open(path)?;
        let mut last = io::Error::from(ErrorKind::InvalidInput);
        for sizes in [Some((PERIOD_FRAMES, BUFFER_FRAMES)), None] {
            let mut params = hw_request(rate, sizes);
            match ioctl::hw_params(&file, &mut params) {
                Ok(()) => {
                    check(&params, rate)?;
                    ioctl::prepare(&file)?;
                    return Ok(Pcm {
                        file,
                        direction,
                        xruns: 0,
                    });
                }
                Err(Errno::INVAL) => last = io::Error::from(Errno::INVAL),
                Err(e) => return Err(e.into()),
            }
        }
        Err(last)
    }

    /// Under- or overruns recovered so far.
    pub fn xruns(&self) -> u64 {
        self.xruns
    }

    /// Frames the playback ring holds now (what is queued and not yet played).
    pub fn queued(&self) -> io::Result<u64> {
        let frames = ioctl::delay(&self.file)?;
        Ok(u64::try_from(frames).unwrap_or(0))
    }

    /// Throws away what the stream holds and prepares it again.
    pub fn flush(&mut self) -> io::Result<()> {
        ioctl::drop_stream(&self.file)?;
        ioctl::prepare(&self.file)?;
        Ok(())
    }

    /// Plays interleaved stereo samples; returns how many the stream took (0: full).
    pub fn write(&mut self, samples: &[i16]) -> io::Result<usize> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let bytes = &bytes[..bytes.len() / FRAME * FRAME];
        for _ in 0..2 {
            match self.file.write(bytes) {
                Ok(n) => return Ok(n / 2),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    if !self.recovered(&e)? {
                        return Ok(0);
                    }
                }
            }
        }
        Ok(0)
    }

    /// Reads recorded interleaved stereo samples into `out`; returns how many (0: nothing yet).
    pub fn read(&mut self, out: &mut [i16]) -> io::Result<usize> {
        let mut bytes = vec![0u8; out.len() / 2 * FRAME];
        loop {
            match self.file.read(&mut bytes) {
                Ok(n) => {
                    let whole = n / 2;
                    for (o, c) in out.iter_mut().zip(bytes[..n].chunks_exact(2)) {
                        *o = i16::from_le_bytes([c[0], c[1]]);
                    }
                    return Ok(whole);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    self.recovered(&e)?;
                    return Ok(0);
                }
            }
        }
    }

    /// `Ok(true)` after an under- or overrun was recovered (try again), `Ok(false)` when the
    /// stream only has no room or nothing to read now, `Err` when it is gone.
    fn recovered(&mut self, e: &io::Error) -> io::Result<bool> {
        if e.kind() == ErrorKind::WouldBlock {
            return Ok(false);
        }
        let (pipe, suspended) = (Errno::PIPE.raw_os_error(), Errno::STRPIPE.raw_os_error());
        match e.raw_os_error() {
            Some(n) if n == pipe || n == suspended => {
                self.xruns += 1;
                ioctl::prepare(&self.file)?;
                Ok(self.direction == Direction::Playback)
            }
            _ => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }
}

/// The driver must have granted two channels at the rate asked for.
fn check(params: &[u8], rate: u32) -> io::Result<()> {
    let at = |i: usize| INTERVALS + i * INTERVAL_SIZE;
    let ok = get(params, at(CHANNELS)) == 2 && get(params, at(RATE)) == rate;
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "the driver changed channels or rate",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ioctl_numbers_are_the_ones_of_asound_h() {
        assert_eq!(ioctl::HW_PARAMS, 0xc260_4111);
        assert_eq!(ioctl::DELAY, 0x8008_4121);
        assert_eq!(ioctl::PREPARE, 0x4140);
        assert_eq!(ioctl::DROP, 0x4143);
    }

    #[test]
    fn the_request_names_the_format_the_access_and_the_exact_values() {
        let b = hw_request(48_000, Some((480, 1920)));
        assert_eq!(b.len(), 608);
        assert_eq!(get(&b, MASKS + ACCESS * MASK_SIZE), 1 << 3);
        assert_eq!(get(&b, MASKS + FORMAT * MASK_SIZE), 1 << 2);
        assert_eq!(get(&b, MASKS + SUBFORMAT * MASK_SIZE), 1);
        // the rest of a mask is empty, not "any"
        assert_eq!(get(&b, MASKS + ACCESS * MASK_SIZE + 4), 0);
        let at = |i: usize| INTERVALS + i * INTERVAL_SIZE;
        for (index, value) in [
            (CHANNELS, 2),
            (RATE, 48_000),
            (PERIOD_SIZE, 480),
            (BUFFER_SIZE, 1920),
        ] {
            assert_eq!(get(&b, at(index)), value);
            assert_eq!(get(&b, at(index) + 4), value);
            assert_eq!(get(&b, at(index) + 8), INTEGER);
        }
        // an interval nobody names stays open
        assert_eq!(get(&b, at(0)), 0);
        assert_eq!(get(&b, at(0) + 4), u32::MAX);
        assert_eq!(get(&b, RMASK), u32::MAX);
    }

    #[test]
    fn without_sizes_period_and_ring_are_left_to_the_driver() {
        let b = hw_request(44_100, None);
        let at = INTERVALS + PERIOD_SIZE * INTERVAL_SIZE;
        assert_eq!((get(&b, at), get(&b, at + 4)), (0, u32::MAX));
        assert_eq!(get(&b, INTERVALS + RATE * INTERVAL_SIZE), 44_100);
    }

    #[test]
    fn a_driver_that_changes_the_rate_is_refused() {
        let mut b = hw_request(48_000, None);
        assert!(check(&b, 48_000).is_ok());
        put(&mut b, INTERVALS + RATE * INTERVAL_SIZE, 44_100);
        assert!(check(&b, 48_000).is_err());
    }

    #[test]
    fn a_file_is_no_pcm_node_and_a_missing_node_is_an_error() {
        let dir = std::env::temp_dir().join(format!("hal-pcm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pcmC0D0p");
        std::fs::write(&file, b"").unwrap();
        for direction in [Direction::Playback, Direction::Capture] {
            assert!(Pcm::open(&file, direction, 48_000).is_err());
            assert!(Pcm::open(&dir.join("none"), direction, 48_000).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
