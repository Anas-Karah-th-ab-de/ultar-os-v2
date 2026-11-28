//! What one recording client gets: the microphone at its own rate (with the echo of the
//! speaker taken out when it asked for it), and a bounded backlog that never holds up
//! anybody.

use crate::aec::Aec;
use crate::resample::{to_i16, Resampler};
use std::io::{self, ErrorKind, Write};
use ultar_audio_stream::{Hello, CARD_RATE};

/// The rate the echo canceller works at.
pub const AEC_RATE: u32 = 16_000;
/// Longest backlog of a recording client (milliseconds): a client that does not read
/// loses its oldest sound, nobody else notices.
pub const BACKLOG_MS: usize = 200;

enum Path {
    /// 48 kHz microphone straight to the client's rate.
    Plain(Resampler),
    /// Microphone and speaker at 16 kHz through the canceller, then to the client's rate.
    Echo { aec: Box<Aec>, back: Resampler },
}

/// The recording half of a stream.
pub struct Rec {
    channels: u8,
    rate: u32,
    path: Path,
    out: Vec<u8>,
    dropped: u64,
}

impl Rec {
    /// The recording side of the stream `hello` opened.
    pub fn new(hello: &Hello) -> Rec {
        let path = if hello.echo_cancel {
            Path::Echo {
                aec: Box::new(Aec::new()),
                back: Resampler::new(AEC_RATE, hello.rate),
            }
        } else {
            Path::Plain(Resampler::new(CARD_RATE, hello.rate))
        };
        Rec {
            channels: hello.channels,
            rate: hello.rate,
            path,
            out: Vec::new(),
            dropped: 0,
        }
    }

    /// Whether this client needs the 16 kHz microphone and speaker signals.
    pub fn wants_echo_path(&self) -> bool {
        matches!(self.path, Path::Echo { .. })
    }

    /// Samples dropped because the client did not read.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Bytes waiting for the client.
    pub fn backlog(&self) -> usize {
        self.out.len()
    }

    /// Takes one period of the microphone: `mic48` (mono, 48 kHz) for a plain client,
    /// `mic16` and `far16` (what the speaker played at the same time) for one with echo
    /// cancellation.
    pub fn feed(&mut self, mic48: &[f32], mic16: &[f32], far16: &[f32]) {
        let mut at_rate = Vec::new();
        match &mut self.path {
            Path::Plain(r) => r.process(mic48, &mut at_rate),
            Path::Echo { aec, back } => {
                let mut clean = Vec::new();
                aec.process(far16, mic16, &mut clean);
                back.process(&clean, &mut at_rate);
            }
        }
        let samples = to_i16(&at_rate);
        for s in samples {
            for _ in 0..self.channels {
                self.out.extend_from_slice(&s.to_le_bytes());
            }
        }
        let frame = usize::from(self.channels) * 2;
        let cap = self.rate as usize * frame * BACKLOG_MS / 1000;
        if self.out.len() > cap {
            let cut = (self.out.len() - cap).div_ceil(frame) * frame;
            self.out.drain(..cut);
            self.dropped += (cut / 2) as u64;
        }
    }

    /// Writes the backlog as far as the client takes it; `Err` when it is gone.
    pub fn flush(&mut self, sock: &mut impl Write) -> io::Result<()> {
        while !self.out.is_empty() {
            match sock.write(&self.out) {
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
}

/// The mono microphone of a stereo period (the card's microphone is two channels of the
/// same capsule; averaging keeps its level).
pub fn mono_of(stereo: &[i16]) -> Vec<f32> {
    stereo
        .chunks_exact(2)
        .map(|f| (f32::from(f[0]) + f32::from(f[1])) / 65536.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_audio_stream::{Class, Direction};

    fn hello(rate: u32, channels: u8, echo: bool) -> Hello {
        let mut h = Hello::new(Class::Call, Direction::Capture, rate, channels);
        h.echo_cancel = echo;
        h
    }

    #[test]
    fn the_microphone_arrives_at_the_clients_rate_and_channels() {
        let mut rec = Rec::new(&hello(16_000, 2, false));
        let tone: Vec<f32> = (0..4800)
            .map(|i| 0.5 * (i as f32 * 0.131).sin())
            .collect();
        rec.feed(&tone, &[], &[]);
        // 100 ms at 48 kHz = 1600 frames at 16 kHz, two channels, two bytes each (a little
        // less while the filter fills)
        assert!(rec.backlog() > 1400 * 4 && rec.backlog() <= 1600 * 4, "{}", rec.backlog());
        let mut sink = Vec::new();
        rec.flush(&mut sink).unwrap();
        assert_eq!(sink.len() % 4, 0);
        assert_eq!(&sink[..2], &sink[2..4], "both channels carry the same sound");
        assert_eq!(rec.backlog(), 0);
    }

    #[test]
    fn a_client_that_does_not_read_loses_old_sound_and_stays_bounded() {
        let mut rec = Rec::new(&hello(48_000, 1, false));
        let burst = vec![0.1f32; 4800];
        for _ in 0..50 {
            rec.feed(&burst, &[], &[]);
        }
        assert!(rec.backlog() <= 48_000 * 2 * BACKLOG_MS / 1000);
        assert!(rec.dropped() > 100_000);
        // a writer that is full: nothing is lost by asking again, nothing blocks
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(ErrorKind::WouldBlock))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let before = rec.backlog();
        rec.flush(&mut Full).unwrap();
        assert_eq!(rec.backlog(), before);
        struct Gone;
        impl Write for Gone {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(rec.flush(&mut Gone).is_err());
    }

    #[test]
    fn echo_cancellation_is_only_on_when_asked_for() {
        assert!(Rec::new(&hello(16_000, 1, true)).wants_echo_path());
        assert!(!Rec::new(&hello(16_000, 1, false)).wants_echo_path());
        assert_eq!(mono_of(&[1000, 3000, -2000, -2000]), vec![2000.0 / 32768.0, -2000.0 / 32768.0]);
    }
}
