//! The sound of a call that is led on another device: the places the voice is taken from and
//! put to while it travels over the mesh (48 kHz mono, 20 ms frames of [`FRAME`] samples).
//!
//! * [`CardTap`]: the sound card's running streams (`/dev/ultar/pcm`: 48 kHz stereo) - the
//!   desk's microphone and speaker for a call answered there.
//! * [`Bridge`]: lets the voice of an Ultar-number call (16 kHz mono, [`crate::pcm`]) go to
//!   the mesh instead of the phone's own card while the call is answered on another device;
//!   [`RelayAudio`] is the call's audio port that follows the switch, [`BridgeTap`] the
//!   relay's end of it.
//! * [`FakeTap`]: the test double (a microphone that speaks what a test queued, a speaker
//!   that remembers what it heard).

use crate::e2e::media::AudioIo;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Samples of one frame of the mesh voice: 20 ms at 48 kHz mono.
pub const FRAME: usize = 960;
/// Bytes of one frame (16-bit).
pub const FRAME_BYTES: usize = FRAME * 2;
/// Bytes of 20 ms of the card: 48 kHz, 16 bit, stereo.
const CARD_FRAME: usize = FRAME * 4;
/// `O_NONBLOCK`.
const NONBLOCK: i32 = 0o4000;
/// Frames a queue keeps at most (beyond that the oldest go: late voice is worse than a gap).
const MAX_QUEUED: usize = 16;

/// Where the voice is taken from and put to.
pub trait MeshTap: Send {
    /// A call is led through this tap from now on.
    fn begin(&mut self) {}
    /// The call is over.
    fn end(&mut self) {}
    /// The next 20 ms of the microphone, if ready (never blocks).
    fn capture(&mut self) -> Option<Vec<i16>>;
    /// Plays 20 ms.
    fn play(&mut self, frame: &[i16]);
}

/// The sound card's streams as a tap.
pub struct CardTap {
    path: PathBuf,
    node: Option<File>,
    recorded: Vec<u8>,
}

fn open_node(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(NONBLOCK)
        .open(path)
}

impl CardTap {
    /// A tap that opens the node when a call begins and closes it when the call ends (the
    /// node serves one program at a time: an Ultar-number call of this device uses it too).
    pub fn lazy(path: PathBuf) -> Self {
        Self {
            path,
            node: None,
            recorded: Vec::new(),
        }
    }

    /// A tap with the node open now.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut tap = Self::lazy(path.to_owned());
        tap.node = Some(open_node(path)?);
        Ok(tap)
    }
}

impl MeshTap for CardTap {
    fn begin(&mut self) {
        self.recorded.clear();
        if self.node.is_none() {
            // without a card the call still runs: nothing is heard or recorded
            self.node = open_node(&self.path).ok();
        }
    }

    fn end(&mut self) {
        self.node = None;
        self.recorded.clear();
    }

    fn capture(&mut self) -> Option<Vec<i16>> {
        let node = self.node.as_mut()?;
        let mut buf = [0u8; CARD_FRAME * 2];
        if let Ok(n) = node.read(&mut buf) {
            self.recorded.extend_from_slice(&buf[..n]);
        }
        if self.recorded.len() > CARD_FRAME * 8 {
            let cut = self.recorded.len() - CARD_FRAME;
            self.recorded.drain(..cut);
        }
        if self.recorded.len() < CARD_FRAME {
            return None;
        }
        let bytes: Vec<u8> = self.recorded.drain(..CARD_FRAME).collect();
        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(
            samples
                .chunks_exact(2)
                .map(|lr| ((i32::from(lr[0]) + i32::from(lr[1])) / 2) as i16)
                .collect(),
        )
    }

    fn play(&mut self, frame: &[i16]) {
        let bytes: Vec<u8> = frame
            .iter()
            .flat_map(|s| [*s, *s])
            .flat_map(|s| s.to_le_bytes())
            .collect();
        // what does not fit is dropped: late voice is worse than a gap
        if let Some(node) = self.node.as_mut() {
            let _ = node.write(&bytes);
        }
    }
}

#[derive(Default)]
struct Queues {
    active: bool,
    /// What the call played (16 kHz), on its way to the mesh.
    to_mesh: VecDeque<Vec<i16>>,
    /// What came from the mesh (16 kHz), on its way into the call as microphone.
    from_mesh: VecDeque<Vec<i16>>,
}

/// The switch between an Ultar-number call's own sound and the mesh.
#[derive(Clone, Default)]
pub struct Bridge {
    inner: Arc<Mutex<Queues>>,
}

impl Bridge {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, active: bool) {
        let mut q = self.lock();
        q.active = active;
        q.to_mesh.clear();
        q.from_mesh.clear();
    }

    /// The call's audio port: the card while no bridge is set, the mesh while one is.
    pub fn audio(&self, card: Box<dyn AudioIo>) -> Box<dyn AudioIo> {
        Box::new(RelayAudio {
            card,
            bridge: self.clone(),
        })
    }

    /// The relay's end of the bridge.
    pub fn tap(&self) -> BridgeTap {
        BridgeTap(self.clone())
    }
}

/// The audio port of a call that follows a [`Bridge`].
pub struct RelayAudio {
    card: Box<dyn AudioIo>,
    bridge: Bridge,
}

fn push(queue: &mut VecDeque<Vec<i16>>, frame: Vec<i16>) {
    if queue.len() >= MAX_QUEUED {
        queue.pop_front();
    }
    queue.push_back(frame);
}

impl AudioIo for RelayAudio {
    fn capture(&mut self) -> Option<Vec<i16>> {
        let mut q = self.bridge.lock();
        if q.active {
            return q.from_mesh.pop_front();
        }
        drop(q);
        self.card.capture()
    }

    fn play(&mut self, frame: &[i16]) {
        let mut q = self.bridge.lock();
        if q.active {
            push(&mut q.to_mesh, frame.to_vec());
            return;
        }
        drop(q);
        self.card.play(frame);
    }
}

/// The relay's end of a [`Bridge`]: 48 kHz frames in and out of a 16 kHz call.
pub struct BridgeTap(Bridge);

impl MeshTap for BridgeTap {
    fn begin(&mut self) {
        self.0.set(true);
    }

    fn end(&mut self) {
        self.0.set(false);
    }

    fn capture(&mut self) -> Option<Vec<i16>> {
        let voice = self.0.lock().to_mesh.pop_front()?;
        Some(upsample(&voice))
    }

    fn play(&mut self, frame: &[i16]) {
        let mut q = self.0.lock();
        if q.active {
            push(&mut q.from_mesh, downsample(frame));
        }
    }
}

/// 16 kHz mono to 48 kHz mono, linear in between (`e2e::audio::FRAME` samples become [`FRAME`]).
pub fn upsample(voice: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(voice.len() * 3);
    for (i, s) in voice.iter().enumerate() {
        let a = i32::from(*s);
        let b = i32::from(*voice.get(i + 1).unwrap_or(s));
        for k in 0..3 {
            out.push((a + (b - a) * k / 3) as i16);
        }
    }
    out
}

/// 48 kHz mono to 16 kHz mono, three samples averaged ([`FRAME`] samples become `e2e::audio::FRAME`).
pub fn downsample(frame: &[i16]) -> Vec<i16> {
    frame
        .chunks_exact(3)
        .map(|c| (c.iter().map(|s| i32::from(*s)).sum::<i32>() / 3) as i16)
        .collect()
}

#[derive(Default)]
struct FakeInner {
    mic: VecDeque<Vec<i16>>,
    heard: Vec<Vec<i16>>,
    begun: u32,
    ended: u32,
}

/// Handle that drives a [`FakeTap`] from a test.
#[derive(Clone, Default)]
pub struct FakeTapControl {
    inner: Arc<Mutex<FakeInner>>,
}

impl FakeTapControl {
    fn lock(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The microphone speaks one frame.
    pub fn speak(&self, frame: Vec<i16>) {
        self.lock().mic.push_back(frame);
    }

    /// Every frame the speaker played so far.
    pub fn heard(&self) -> Vec<Vec<i16>> {
        self.lock().heard.clone()
    }

    /// How often a call began and ended through the tap.
    pub fn calls(&self) -> (u32, u32) {
        let inner = self.lock();
        (inner.begun, inner.ended)
    }
}

/// Test double of a sound device.
pub struct FakeTap {
    control: FakeTapControl,
}

impl FakeTap {
    /// A fake tap and the handle that drives it.
    pub fn new() -> (FakeTap, FakeTapControl) {
        let control = FakeTapControl::default();
        (
            FakeTap {
                control: control.clone(),
            },
            control,
        )
    }
}

impl MeshTap for FakeTap {
    fn begin(&mut self) {
        self.control.lock().begun += 1;
    }

    fn end(&mut self) {
        self.control.lock().ended += 1;
    }

    fn capture(&mut self) -> Option<Vec<i16>> {
        self.control.lock().mic.pop_front()
    }

    fn play(&mut self, frame: &[i16]) {
        self.control.lock().heard.push(frame.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2e::audio::FRAME as VOICE_FRAME;

    #[test]
    fn resampling_keeps_the_frame_lengths_and_a_steady_tone() {
        let voice = vec![1000i16; VOICE_FRAME];
        let up = upsample(&voice);
        assert_eq!(up.len(), FRAME);
        assert!(up.iter().all(|s| *s == 1000));
        let down = downsample(&up);
        assert_eq!(down.len(), VOICE_FRAME);
        assert!(down.iter().all(|s| *s == 1000));
    }

    #[test]
    fn a_bridge_switches_the_calls_audio_between_the_card_and_the_mesh() {
        let (card, ctl) = FakeTap::new();
        struct Port(FakeTap);
        impl AudioIo for Port {
            fn capture(&mut self) -> Option<Vec<i16>> {
                self.0.capture()
            }
            fn play(&mut self, frame: &[i16]) {
                self.0.play(frame);
            }
        }
        let bridge = Bridge::default();
        let mut audio = bridge.audio(Box::new(Port(card)));
        let mut tap = bridge.tap();
        // No bridge: the card.
        audio.play(&[1; VOICE_FRAME]);
        assert_eq!(ctl.heard().len(), 1);
        assert!(tap.capture().is_none());
        // Bridged: the voice goes to the mesh and the mesh's voice is the microphone.
        tap.begin();
        audio.play(&[500; VOICE_FRAME]);
        assert_eq!(ctl.heard().len(), 1, "the card hears nothing while bridged");
        assert_eq!(tap.capture().unwrap(), vec![500; FRAME]);
        assert!(audio.capture().is_none());
        tap.play(&[900; FRAME]);
        assert_eq!(audio.capture().unwrap(), vec![900; VOICE_FRAME]);
        // Over: the card again, and nothing stale is left.
        tap.play(&[7; FRAME]);
        tap.end();
        assert!(audio.capture().is_none());
        audio.play(&[2; VOICE_FRAME]);
        assert_eq!(ctl.heard().len(), 2);
    }

    #[test]
    fn a_file_stands_in_for_the_card() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("pcm");
        // 20 ms of stereo: left 100, right 300.
        let card: Vec<u8> = (0..FRAME)
            .flat_map(|_| [100i16.to_le_bytes(), 300i16.to_le_bytes()])
            .flatten()
            .collect();
        std::fs::write(&p, &card).unwrap();
        let mut tap = CardTap::open(&p).unwrap();
        assert_eq!(tap.capture().unwrap(), vec![200; FRAME]);
        assert!(tap.capture().is_none(), "nothing more recorded");
        tap.play(&[5; FRAME]);
        assert!(CardTap::open(&d.path().join("none")).is_err());
        // A lazy tap opens at the beginning of a call and lets go at its end.
        let mut lazy = CardTap::lazy(p.clone());
        assert!(lazy.capture().is_none(), "closed until a call begins");
        lazy.begin();
        assert!(lazy.capture().is_some());
        lazy.end();
        assert!(lazy.capture().is_none());
    }
}
