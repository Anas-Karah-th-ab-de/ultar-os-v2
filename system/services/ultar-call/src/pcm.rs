//! Microphone and speaker of a call through the sound server (`ultar-audiod`). The call's
//! voice is 16 kHz mono ([`crate::e2e::audio`]) and the stream says so: the server converts
//! to the card's 48 kHz stereo, mixes, ducks other sound under the call and removes the echo
//! of the device's own speaker from the microphone. This module never blocks.

use crate::e2e::audio::FRAME;
use crate::e2e::media::AudioIo;
use std::io;
use std::path::Path;
use ultar_audio_stream::{Class, ConnectError, Direction, Hello, Stream};

/// Sample rate of a call's voice.
pub const VOICE_RATE: u32 = 16_000;
/// Frames of the microphone kept at most (the oldest go: late voice is worse than a gap).
const MAX_RECORDED: usize = FRAME * 8;

/// The hello of a call's own voice: class call, both ways, with echo cancellation.
pub fn call_hello() -> Hello {
    let mut hello = Hello::new(Class::Call, Direction::Duplex, VOICE_RATE, 1);
    hello.echo_cancel = true;
    hello
}

/// Opens a stream at `path`.
pub fn open_stream(path: &Path, hello: Hello) -> io::Result<Stream> {
    Stream::connect(path, hello).map_err(|e| match e {
        ConnectError::Io(e) => e,
        ConnectError::Protocol => io::Error::new(io::ErrorKind::InvalidData, "stream protocol"),
        ConnectError::Refused(s) => {
            io::Error::new(io::ErrorKind::PermissionDenied, format!("{s:?}"))
        }
    })
}

/// The call's sound as a stream of the sound server.
pub struct PcmAudio {
    stream: Option<Stream>,
    recorded: Vec<i16>,
}

impl PcmAudio {
    /// Opens a stream at the sound server's socket `path` (one per call).
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            stream: Some(open_stream(path, call_hello())?),
            recorded: Vec::with_capacity(MAX_RECORDED),
        })
    }
}

impl AudioIo for PcmAudio {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let stream = self.stream.as_mut()?;
        if stream.pull(&mut self.recorded).is_err() {
            // the sound server is gone: the call goes on without sound
            self.stream = None;
            return Some(vec![0; FRAME]);
        }
        if self.recorded.len() > MAX_RECORDED {
            let cut = self.recorded.len() - FRAME;
            self.recorded.drain(..cut);
        }
        if self.recorded.len() < FRAME {
            return None;
        }
        Some(self.recorded.drain(..FRAME).collect())
    }

    fn play(&mut self, frame: &[i16]) {
        // what does not fit is dropped: late voice is worse than a gap
        if let Some(stream) = self.stream.as_mut() {
            if stream.push(frame).is_err() {
                self.stream = None;
            }
        }
    }
}

/// No sound server: the call runs, nothing is heard or recorded.
pub struct Silent;

impl AudioIo for Silent {
    fn capture(&mut self) -> Option<Vec<i16>> {
        Some(vec![0; FRAME])
    }
    fn play(&mut self, _frame: &[i16]) {}
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use ultar_audio_stream::{Status, HELLO_LEN};

    #[test]
    fn a_call_speaks_and_listens_through_the_server() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("calld.sock");
        let listener = UnixListener::bind(&p).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut hello = [0u8; HELLO_LEN];
            s.read_exact(&mut hello).unwrap();
            s.write_all(&Status::Ok.encode()).unwrap();
            let mic: Vec<u8> = (0..FRAME as i16).flat_map(|i| i.to_le_bytes()).collect();
            s.write_all(&mic).unwrap();
            let mut heard = vec![0u8; FRAME * 2];
            s.read_exact(&mut heard).unwrap();
            (Hello::decode(&hello), heard)
        });
        let mut a = PcmAudio::open(&p).unwrap();
        let mut got = None;
        for _ in 0..200 {
            got = a.capture();
            if got.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let f = got.unwrap();
        assert_eq!(f.len(), FRAME);
        assert_eq!(f[7], 7);
        assert!(a.capture().is_none(), "nothing more recorded");
        a.play(&f);
        let (hello, heard) = server.join().unwrap();
        let hello = hello.unwrap();
        assert_eq!(hello, call_hello());
        assert_eq!(&heard[14..16], &7i16.to_le_bytes());
        assert!(PcmAudio::open(&d.path().join("none")).is_err());
        assert_eq!(Silent.capture().unwrap(), vec![0; FRAME]);
    }

    #[test]
    fn a_server_that_goes_away_leaves_a_silent_call() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("calld.sock");
        let listener = UnixListener::bind(&p).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut hello = [0u8; HELLO_LEN];
            s.read_exact(&mut hello).unwrap();
            s.write_all(&Status::Ok.encode()).unwrap();
        });
        let mut a = PcmAudio::open(&p).unwrap();
        server.join().unwrap();
        let mut f = None;
        for _ in 0..200 {
            f = a.capture();
            if f.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(f.unwrap(), vec![0; FRAME]);
        a.play(&[1; FRAME]);
    }
}
