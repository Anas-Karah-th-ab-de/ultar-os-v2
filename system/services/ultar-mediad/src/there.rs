//! "Play there" (mesh item C3) without the bus: which devices the sound can go to, the
//! speaker of this device as a [`Sink`], and the [`Playback`] of a session that plays here
//! (jitter buffer, loss concealment, pause, volume).
//!
//! The sound is what the mesh carries: 20 ms frames of PCM 16-bit, 48 kHz mono.

use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use ultar_audio_stream::{Class, Direction, Hello, Stream};
use ultar_bus::messages::mesh::{Capability, MeshState};
use ultar_bus::messages::mesh_media::{ThereOutput, MAX_OUTPUTS};

/// Samples of one frame (20 ms at 48 kHz mono).
pub const FRAME: usize = 960;
/// Frames held before playing (60 ms against jitter).
const DEPTH: usize = 3;
/// Frames the buffer keeps at most (the sound must not lag).
const MAX_HELD: usize = 12;

/// The speaker of this device.
pub trait Sink: Send {
    /// Takes the speaker for a session; `false` when it is not free.
    fn open(&mut self) -> bool;
    /// Plays one frame (volume already applied); never blocks.
    fn play(&mut self, frame: &[i16]);
    /// Lets the speaker go.
    fn close(&mut self);
}

/// A media stream of the sound server (`ultar-audiod`: 48 kHz mono, class media). Opened for
/// a session only; the server mixes it with everything else, ducks it under a call and owns
/// the card (this service holds no kernel sound right).
pub struct StreamSink {
    path: PathBuf,
    stream: Option<Stream>,
}

impl StreamSink {
    /// A sink on the sound server's stream socket at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path, stream: None }
    }
}

impl Sink for StreamSink {
    fn open(&mut self) -> bool {
        let hello = Hello::new(Class::Media, Direction::Play, 48_000, 1);
        self.stream = Stream::connect(&self.path, hello).ok();
        self.stream.is_some()
    }

    fn play(&mut self, frame: &[i16]) {
        // What does not fit is dropped: late sound is worse than a gap. A server that went
        // away ends the stream; the next session opens a new one.
        if let Some(stream) = self.stream.as_mut() {
            if stream.push(frame).is_err() {
                self.stream = None;
            }
        }
    }

    fn close(&mut self) {
        self.stream = None;
    }
}

/// What a [`FakeSink`] was asked.
#[derive(Default)]
pub struct Played {
    /// Frames played, in order.
    pub frames: Vec<Vec<i16>>,
    /// Times the speaker was taken.
    pub opened: u32,
    /// Times it was let go.
    pub closed: u32,
    /// The speaker is not free.
    pub busy: bool,
}

/// A speaker for tests.
pub struct FakeSink(Arc<Mutex<Played>>);

impl FakeSink {
    /// A fake speaker and the view of what it played.
    pub fn new() -> (FakeSink, Arc<Mutex<Played>>) {
        let shared = Arc::new(Mutex::new(Played::default()));
        (FakeSink(shared.clone()), shared)
    }
}

fn lock(shared: &Mutex<Played>) -> std::sync::MutexGuard<'_, Played> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl Sink for FakeSink {
    fn open(&mut self) -> bool {
        let mut p = lock(&self.0);
        if p.busy {
            return false;
        }
        p.opened += 1;
        true
    }

    fn play(&mut self, frame: &[i16]) {
        lock(&self.0).frames.push(frame.to_vec());
    }

    fn close(&mut self) {
        lock(&self.0).closed += 1;
    }
}

/// Paired, reachable devices with a speaker, as outputs for the sound (at most
/// [`MAX_OUTPUTS`]). Whether a device takes the sound is its own decision (the right
/// "Anrufe und Ton" there).
pub fn outputs(state: &MeshState) -> Vec<ThereOutput> {
    if !state.enabled {
        return Vec::new();
    }
    state
        .peers
        .iter()
        .filter(|p| p.paired && p.reachable && p.capabilities.contains(&Capability::AudioOut))
        .take(MAX_OUTPUTS)
        .map(|p| ThereOutput {
            device: p.id.clone(),
            name: p.name.clone(),
        })
        .collect()
}

/// One frame of base64 PCM as samples; `None` for a wrong size or bad encoding.
pub fn decode(pcm: &str) -> Option<Vec<i16>> {
    let bytes = B64.decode(pcm).ok()?;
    if bytes.len() != FRAME * 2 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

/// Samples as base64 PCM.
pub fn encode(frame: &[i16]) -> String {
    let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
    B64.encode(bytes)
}

/// A session that plays here.
pub struct Playback {
    /// The session on the other device's numbering.
    pub id: u64,
    /// The device that plays its sound here.
    pub from: String,
    /// Its name (data).
    pub name: String,
    /// Paused: the speaker is silent and arriving sound is dropped.
    pub paused: bool,
    /// Volume, 0-100.
    pub volume: u8,
    frames: BTreeMap<u64, Vec<i16>>,
    next: Option<u64>,
    last: Vec<i16>,
    started: bool,
}

impl Playback {
    /// A session from `from`.
    pub fn new(id: u64, from: String, name: String) -> Self {
        Self {
            id,
            from,
            name,
            paused: false,
            volume: 100,
            frames: BTreeMap::new(),
            next: None,
            last: Vec::new(),
            started: false,
        }
    }

    /// A frame with its sequence number arrived.
    pub fn put(&mut self, seq: u64, frame: Vec<i16>) {
        if self.paused || self.next.is_some_and(|n| seq < n) {
            return;
        }
        self.frames.insert(seq, frame);
        while self.frames.len() > MAX_HELD {
            self.frames.pop_first();
            self.next = self.frames.keys().next().copied();
        }
    }

    /// The next 20 ms for the speaker (silence until enough is buffered or while paused);
    /// a lost frame repeats the last one at half volume, and half again for every further
    /// one.
    pub fn take(&mut self) -> Vec<i16> {
        let silence = vec![0; FRAME];
        if self.paused {
            self.started = false;
            self.frames.clear();
            self.next = None;
            return silence;
        }
        if !self.started {
            if self.frames.len() < DEPTH {
                return silence;
            }
            self.started = true;
            self.next = self.frames.keys().next().copied();
        }
        let Some(n) = self.next else {
            return silence;
        };
        self.next = Some(n + 1);
        let frame = match self.frames.remove(&n) {
            Some(f) => {
                self.last.clone_from(&f);
                f
            }
            None if self.frames.is_empty() => {
                self.started = false;
                self.next = None;
                return silence;
            }
            None => {
                self.last.iter_mut().for_each(|s| *s /= 2);
                if self.last.is_empty() {
                    return silence;
                }
                self.last.clone()
            }
        };
        scale(&frame, self.volume)
    }
}

/// `frame` at `percent` of its volume.
pub fn scale(frame: &[i16], percent: u8) -> Vec<i16> {
    let p = i32::from(percent.min(100));
    frame
        .iter()
        .map(|s| (i32::from(*s) * p / 100) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::mesh::{DeviceKind, Peer, Right};

    fn frame(v: i16) -> Vec<i16> {
        vec![v; FRAME]
    }

    fn peer(id: &str, paired: bool, reachable: bool, caps: Vec<Capability>) -> Peer {
        Peer {
            id: id.into(),
            name: format!("dev-{id}"),
            kind: DeviceKind::Desktop,
            capabilities: caps,
            paired,
            reachable,
            rights: vec![Right::Call],
        }
    }

    #[test]
    fn only_paired_reachable_devices_with_a_speaker_are_outputs() {
        let speaker = vec![Capability::AudioOut];
        let state = MeshState {
            enabled: true,
            id: "me".into(),
            name: "Me".into(),
            peers: vec![
                peer("desk", true, true, speaker.clone()),
                peer("away", true, false, speaker.clone()),
                peer("stranger", false, true, speaker.clone()),
                peer("mute", true, true, vec![Capability::AudioIn]),
            ],
        };
        let out = outputs(&state);
        assert_eq!(out.len(), 1);
        assert_eq!(
            (out[0].device.as_str(), out[0].name.as_str()),
            ("desk", "dev-desk")
        );
        let off = MeshState {
            enabled: false,
            ..state
        };
        assert!(outputs(&off).is_empty(), "the mesh is off: no outputs");
    }

    #[test]
    fn frames_are_checked_and_round_trip() {
        let f: Vec<i16> = (0..FRAME as i16).collect();
        assert_eq!(decode(&encode(&f)), Some(f));
        assert_eq!(decode(&encode(&[1, 2, 3])), None, "wrong size");
        assert_eq!(decode("!!"), None, "not base64");
    }

    #[test]
    fn playback_orders_conceals_pauses_and_scales() {
        let mut p = Playback::new(1, "d".into(), "Desk".into());
        assert_eq!(p.take(), frame(0), "silence until deep enough");
        p.put(7, frame(80));
        p.put(3, frame(400));
        p.put(4, frame(200));
        assert_eq!(p.take(), frame(400));
        p.volume = 50;
        assert_eq!(p.take(), frame(100), "volume applies");
        // 5 and 6 are lost: the last repeats at half, then half again, while 7 waits.
        assert_eq!(p.take(), frame(50), "100 halved, at volume 50");
        assert_eq!(p.take(), frame(25));
        assert_eq!(p.take(), frame(40));
        p.put(4, frame(999)); // already played over
        assert_eq!(p.take(), frame(0));
        p.paused = true;
        p.put(10, frame(5));
        assert!(p.frames.is_empty(), "nothing is kept while paused");
        assert_eq!(p.take(), frame(0));
    }

    #[test]
    fn the_buffer_is_bounded_and_the_server_gets_a_media_stream() {
        let mut p = Playback::new(1, "d".into(), "Desk".into());
        for seq in 0..100 {
            p.put(seq, frame(1));
        }
        assert!(p.frames.len() <= MAX_HELD);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mediad.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut s, _) = listener.accept().unwrap();
            let mut hello = [0u8; ultar_audio_stream::HELLO_LEN];
            s.read_exact(&mut hello).unwrap();
            s.write_all(&ultar_audio_stream::Status::Ok.encode())
                .unwrap();
            let mut pcm = Vec::new();
            s.read_to_end(&mut pcm).unwrap();
            (Hello::decode(&hello), pcm)
        });
        let mut sink = StreamSink::new(path);
        assert!(sink.open());
        sink.play(&[1, -2]);
        sink.close();
        let (hello, pcm) = server.join().unwrap();
        assert_eq!(
            hello,
            Some(Hello::new(Class::Media, Direction::Play, 48_000, 1))
        );
        assert_eq!(pcm, [1, 0, 254, 255]);
        assert!(!StreamSink::new(dir.path().join("none")).open());
    }
}
