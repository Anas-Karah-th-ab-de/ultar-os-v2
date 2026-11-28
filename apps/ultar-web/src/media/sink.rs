//! Where the sound goes: a media stream of the sound server (`ultar-audiod`: 48 kHz, 16 bit,
//! stereo) through its Unix socket `/run/ultar/audio/app.browser.sock`, or nowhere when the
//! server is not there (development, Linux without it): then the sound is dropped and the
//! pictures still follow the clock. The app holds no kernel sound right; the server mixes the
//! stream with everything else and keeps the card.

use std::path::{Path, PathBuf};
use ultar_audio_stream::{Class, Direction, Hello, Stream};

/// The browser's stream socket at the sound server.
pub fn pcm() -> PathBuf {
    PathBuf::from("/run/ultar/audio/app.browser.sock")
}

/// The sound output.
pub enum Sink {
    /// A stream of the sound server.
    Card(Stream),
    /// No sound server: sound is dropped.
    Silent,
}

impl Sink {
    /// A media stream at the socket `path`, else [`Sink::Silent`].
    pub fn open(path: &Path) -> Sink {
        let hello = Hello::new(Class::Media, Direction::Play, 48_000, 2);
        Stream::connect(path, hello).map_or(Sink::Silent, Sink::Card)
    }

    /// `true` when the sound is heard.
    pub fn audible(&self) -> bool {
        matches!(self, Sink::Card(_))
    }

    /// Queues as much of `bytes` (48 kHz stereo 16 bit) as the server takes now; returns how
    /// many bytes it took (all of them when silent). A server that does not take more (the
    /// stream is 200 ms ahead) gets nothing more this round.
    pub fn offer(&mut self, bytes: &[u8]) -> usize {
        match self {
            Sink::Silent => bytes.len(),
            Sink::Card(stream) => {
                let samples: Vec<i16> = bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
                match stream.push(&samples) {
                    Ok(n) => n * 2,
                    Err(_) => {
                        // the server went away: carry on without sound
                        *self = Sink::Silent;
                        bytes.len()
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use ultar_audio_stream::{Status, HELLO_LEN};

    #[test]
    fn without_a_server_the_sound_is_dropped() {
        let mut s = Sink::open(Path::new("/nonexistent/pcm"));
        assert!(!s.audible());
        assert_eq!(s.offer(&[0; 64]), 64);
    }

    #[test]
    fn the_sound_reaches_the_server_as_a_media_stream() {
        let dir = std::env::temp_dir().join(format!("ultar-web-sink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.browser.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut hello = [0u8; HELLO_LEN];
            s.read_exact(&mut hello).unwrap();
            s.write_all(&Status::Ok.encode()).unwrap();
            let mut pcm = Vec::new();
            s.read_to_end(&mut pcm).unwrap();
            (Hello::decode(&hello), pcm)
        });
        let mut s = Sink::open(&path);
        assert!(s.audible());
        let bytes: Vec<u8> = (0..64u8).collect();
        assert_eq!(s.offer(&bytes), 64);
        drop(s);
        let (hello, pcm) = server.join().unwrap();
        assert_eq!(
            hello,
            Some(Hello::new(Class::Media, Direction::Play, 48_000, 2))
        );
        assert_eq!(pcm, bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
