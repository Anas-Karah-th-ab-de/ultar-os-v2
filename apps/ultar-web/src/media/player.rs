//! A player for one file: fetches it, then plays its sound and its pictures on two threads
//! that follow one clock. The page asks for the state and the newest picture; a byte on the
//! engine's wake pipe says that something changed.

use super::audio::{AudioStream, Bytes};
use super::clock::Clock;
use super::sink::{pcm, Sink};
use super::video::{VideoError, VideoStream};
use super::Picture;
use rustix::fd::OwnedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// How far the sound is written ahead of the clock.
const AHEAD: Duration = Duration::from_millis(150);
/// A picture later than this is skipped (the decoder could not keep up).
const LATE: Duration = Duration::from_millis(250);
/// Longest nap while waiting.
const NAP: Duration = Duration::from_millis(10);

/// Where a player stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Fetching the file: per mille done (`None`: size unknown).
    Loading(Option<u32>),
    /// Playing.
    Playing,
    /// Paused.
    Paused,
    /// Played to the end.
    Ended,
    /// The file did not come.
    FetchFailed(String),
    /// Nothing in it can be played (the format, named).
    Unsupported(String),
}

/// Fetches the file: gets the progress (per mille) to fill and the stop flag to honour;
/// gives the bytes and the media type.
pub type Fetch =
    Box<dyn FnOnce(&AtomicU32, &AtomicBool) -> Result<(Vec<u8>, String), String> + Send>;

struct Shared {
    status: Mutex<Status>,
    picture: Mutex<Option<Arc<Picture>>>,
    serial: AtomicU64,
    progress: AtomicU32,
    clock: Clock,
    stop: AtomicBool,
    running: AtomicUsize,
    length: Mutex<Option<Duration>>,
    bytes: Mutex<Option<(Bytes, String)>>,
    wake: Option<OwnedFd>,
    pcm: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn set(&self, status: Status) {
        *lock(&self.status) = status;
        self.wake();
    }

    fn wake(&self) {
        if let Some(fd) = &self.wake {
            let _ = rustix::io::write(fd, &[1]);
        }
    }

    /// Waits until `at` is due on the clock (less `ahead`); `false` when stopped.
    fn wait_for(&self, at: Duration, ahead: Duration) -> bool {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            let now = self.clock.now();
            if self.clock.running() && at <= now + ahead {
                return true;
            }
            let rest = if self.clock.running() {
                at - now - ahead
            } else {
                NAP
            };
            std::thread::sleep(rest.min(NAP));
        }
    }

    fn finished(&self) {
        if self.running.fetch_sub(1, Ordering::AcqRel) == 1 && !self.stop.load(Ordering::Relaxed) {
            self.clock.pause();
            self.set(Status::Ended);
        }
    }
}

/// A playing (or loading) file.
pub struct Player {
    shared: Arc<Shared>,
}

impl Player {
    /// Fetches with `fetch` and plays; `wake` gets a byte on every change.
    pub fn start(fetch: Fetch, wake: Option<OwnedFd>) -> Player {
        Player::start_with(fetch, wake, &pcm().to_string_lossy())
    }

    /// As [`Player::start`] with another sound card node (tests).
    pub fn start_with(fetch: Fetch, wake: Option<OwnedFd>, pcm: &str) -> Player {
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::Loading(None)),
            picture: Mutex::new(None),
            serial: AtomicU64::new(0),
            progress: AtomicU32::new(u32::MAX),
            clock: Clock::default(),
            stop: AtomicBool::new(false),
            running: AtomicUsize::new(0),
            length: Mutex::new(None),
            bytes: Mutex::new(None),
            wake,
            pcm: pcm.to_owned(),
        });
        let s = shared.clone();
        std::thread::spawn(move || load(&s, fetch));
        Player { shared }
    }

    /// The same file again from the start (the bytes are kept), when it was fetched.
    pub fn replay(&self) -> Option<Player> {
        let (bytes, mime) = lock(&self.shared.bytes).clone()?;
        let wake = self.shared.wake.as_ref().and_then(|w| w.try_clone().ok());
        let fetch: Fetch = Box::new(move |_, _| Ok(((*bytes.0).clone(), mime)));
        Some(Player::start_with(fetch, wake, &self.shared.pcm))
    }

    /// Where it stands.
    pub fn status(&self) -> Status {
        let status = lock(&self.shared.status).clone();
        if let Status::Loading(_) = status {
            let p = self.shared.progress.load(Ordering::Relaxed);
            return Status::Loading((p <= 1000).then_some(p));
        }
        status
    }

    /// Pauses or plays on.
    pub fn toggle(&self) {
        let status = self.status();
        match status {
            Status::Playing => {
                self.shared.clock.pause();
                self.shared.set(Status::Paused);
            }
            Status::Paused => {
                self.shared.clock.start();
                self.shared.set(Status::Playing);
            }
            _ => {}
        }
    }

    /// Time played and the length (when known).
    pub fn position(&self) -> (Duration, Option<Duration>) {
        (self.shared.clock.now(), *lock(&self.shared.length))
    }

    /// The newest picture and its number (it grows with every new picture).
    pub fn picture(&self) -> (u64, Option<Arc<Picture>>) {
        (
            self.shared.serial.load(Ordering::Acquire),
            lock(&self.shared.picture).clone(),
        )
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn load(s: &Arc<Shared>, fetch: Fetch) {
    let (bytes, mime) = match fetch(&s.progress, &s.stop) {
        Ok(got) => got,
        Err(e) => return s.set(Status::FetchFailed(e)),
    };
    let bytes = Bytes(Arc::new(bytes));
    *lock(&s.bytes) = Some((bytes.clone(), mime.clone()));
    let audio = AudioStream::open(bytes.clone(), &mime).ok();
    let video = VideoStream::open(bytes);
    let video = match video {
        Ok(v) => Some(v),
        Err(VideoError::Codec(name)) if audio.is_none() => return s.set(Status::Unsupported(name)),
        Err(_) => None,
    };
    if audio.is_none() && video.is_none() {
        let what = if mime.is_empty() {
            "?".to_owned()
        } else {
            mime
        };
        return s.set(Status::Unsupported(what));
    }
    *lock(&s.length) = audio.as_ref().and_then(AudioStream::duration);
    s.running.store(
        usize::from(audio.is_some()) + usize::from(video.is_some()),
        Ordering::Release,
    );
    if let Some(a) = audio {
        let s = s.clone();
        std::thread::spawn(move || play_sound(&s, a));
    }
    if let Some(v) = video {
        let s = s.clone();
        std::thread::spawn(move || show_pictures(&s, v));
    }
    s.clock.start();
    s.set(Status::Playing);
}

fn play_sound(s: &Shared, mut audio: AudioStream) {
    let mut sink = Sink::open(Path::new(&s.pcm));
    let mut written = 0u64; // frames
    while let Some(chunk) = audio.next_chunk() {
        let at = Duration::from_secs_f64(written as f64 / f64::from(super::RATE));
        if !s.wait_for(at, AHEAD) {
            return;
        }
        let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut rest = &bytes[..];
        while !rest.is_empty() {
            if s.stop.load(Ordering::Relaxed) {
                return;
            }
            let took = sink.offer(rest);
            rest = &rest[took..];
            if took == 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        written += chunk.len() as u64 / 2;
    }
    // the last sound written is still playing: it ends when the clock gets there
    let end = Duration::from_secs_f64(written as f64 / f64::from(super::RATE));
    if s.wait_for(end, Duration::ZERO) {
        s.finished();
    }
}

fn show_pictures(s: &Shared, mut video: VideoStream) {
    while let Some((at, picture)) = video.next_picture() {
        if !s.wait_for(at, Duration::ZERO) {
            return;
        }
        if s.clock.now() > at + LATE {
            continue;
        }
        *lock(&s.picture) = Some(Arc::new(picture));
        s.serial.fetch_add(1, Ordering::AcqRel);
        s.wake();
    }
    s.finished();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn until(p: &Player, done: impl Fn(&Status) -> bool) -> Status {
        let start = Instant::now();
        loop {
            let st = p.status();
            if done(&st) || start.elapsed() > Duration::from_secs(20) {
                return st;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn given(bytes: Vec<u8>, mime: &str) -> Fetch {
        let mime = mime.to_owned();
        Box::new(move |_, _| Ok((bytes, mime)))
    }

    #[test]
    fn a_sound_file_plays_pauses_and_ends() {
        let wav = super::super::audio::tests::wav(22_050, 0.3);
        let p = Player::start_with(given(wav, "audio/wav"), None, "/nonexistent");
        assert_eq!(until(&p, |s| *s == Status::Playing), Status::Playing);
        p.toggle();
        assert_eq!(p.status(), Status::Paused);
        let t = p.position().0;
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(p.position().0, t, "the clock stands");
        p.toggle();
        assert_eq!(until(&p, |s| *s == Status::Ended), Status::Ended);
        assert!(p.position().0 >= Duration::from_millis(290));
        let again = p.replay().unwrap();
        assert_eq!(
            until(&again, |s| *s == Status::Playing || *s == Status::Ended),
            Status::Playing
        );
    }

    #[test]
    fn a_video_shows_its_pictures() {
        let mp4 = super::super::video::tests::mp4_file(8);
        let p = Player::start_with(given(mp4, "video/mp4"), None, "/nonexistent");
        assert_eq!(until(&p, |s| *s == Status::Ended), Status::Ended);
        let (serial, picture) = p.picture();
        assert!(serial >= 6, "{serial}");
        assert_eq!(picture.unwrap().width, 64);
    }

    #[test]
    fn failures_are_named() {
        let p = Player::start_with(given(b"<html>".to_vec(), "text/html"), None, "/x");
        assert_eq!(
            until(&p, |s| matches!(s, Status::Unsupported(_))),
            Status::Unsupported("text/html".into())
        );
        let f: Fetch = Box::new(|_, _| Err("offline".into()));
        let p = Player::start_with(f, None, "/x");
        assert_eq!(
            until(&p, |s| matches!(s, Status::FetchFailed(_))),
            Status::FetchFailed("offline".into())
        );
    }
}
