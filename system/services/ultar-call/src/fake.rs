//! Test double of the call backends: a [`FakeLine`] that serves any set of ways, driven by its
//! [`FakeControl`] (ring, connect, hang up remotely, change availability, make dials fail) and
//! recording every command it got; a [`FakeAudio`] route and a [`ManualClock`].
//! Used by the service's own tests, by the phone app's end-to-end test and by development
//! runs without modem or network (`ultar-call --fake`).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::line::{AudioRoute, Clock, Key, LineError, LineEvent, Status};
use crate::Line;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use ultar_bus::messages::call::{Cellular, End, Failure, Media, Output, Ultar, Way};

/// Most events the fake keeps before the session polls (oldest dropped).
const MAX_EVENTS: usize = 256;
/// Most commands the fake records.
const MAX_COMMANDS: usize = 1024;

/// A command the fake line received.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// `dial(way, number, video)` → key.
    Dial(Way, String, bool, Key),
    /// `answer(key)`.
    Answer(Key),
    /// `release(key)`.
    Release(Key),
    /// `hold(key, on)`.
    Hold(Key, bool),
    /// `dtmf(key, tone)`.
    Dtmf(Key, u8),
    /// `media(key, media)`.
    Media(Key, Media),
    /// `send_text(way, number, text)`.
    Text(Way, String, String),
}

#[derive(Default)]
struct Inner {
    events: VecDeque<LineEvent>,
    commands: Vec<Command>,
    next_key: Key,
    fail_next: Option<LineError>,
    auto_connect: bool,
    cameras: u8,
}

/// Handle that drives a [`FakeLine`] from a test.
#[derive(Clone, Default)]
pub struct FakeControl {
    inner: Arc<Mutex<Inner>>,
}

impl FakeControl {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push(&self, event: LineEvent) {
        let mut inner = self.lock();
        if inner.events.len() == MAX_EVENTS {
            inner.events.pop_front();
        }
        inner.events.push_back(event);
    }

    /// A call comes in; returns its key.
    pub fn ring(&self, way: Way, number: &str, name: Option<&str>) -> Key {
        let key = {
            let mut inner = self.lock();
            inner.next_key += 1;
            inner.next_key
        };
        self.push(LineEvent::Incoming {
            key,
            way,
            number: number.to_owned(),
            name: name.map(str::to_owned),
            video: false,
        });
        key
    }

    /// The other side answers an outgoing call.
    pub fn connect(&self, key: Key) {
        self.push(LineEvent::Connected(key));
    }

    /// The call ends from the other side or the network.
    pub fn end(&self, key: Key, end: End) {
        self.push(LineEvent::Ended(key, end));
    }

    /// Reports the mobile network state.
    pub fn set_cellular(&self, state: Cellular, sims: &[u8]) {
        self.push(LineEvent::Status(Status::Cellular(state, sims.to_vec())));
    }

    /// Reports the Ultar-number state.
    pub fn set_ultar(&self, state: Ultar, own: Option<&str>) {
        self.push(LineEvent::Status(Status::Ultar(
            state,
            own.map(str::to_owned),
        )));
    }

    /// The next command fails with this error.
    pub fn fail_next(&self, error: LineError) {
        self.lock().fail_next = Some(error);
    }

    /// Outgoing calls are connected right after alerting.
    pub fn auto_connect(&self, on: bool) {
        self.lock().auto_connect = on;
    }

    /// Number of cameras the line reports.
    pub fn set_cameras(&self, n: u8) {
        self.lock().cameras = n;
    }

    /// Every command received so far.
    pub fn commands(&self) -> Vec<Command> {
        self.lock().commands.clone()
    }

    /// Key of the last dialled call.
    pub fn last_dial(&self) -> Option<Key> {
        self.lock().commands.iter().rev().find_map(|c| match c {
            Command::Dial(_, _, _, key) => Some(*key),
            _ => None,
        })
    }
}

/// Line that serves the given ways and does what its [`FakeControl`] says.
pub struct FakeLine {
    ways: Vec<Way>,
    control: FakeControl,
}

impl FakeLine {
    /// A fake line for `ways` and the handle that drives it.
    pub fn new(ways: &[Way]) -> (FakeLine, FakeControl) {
        let control = FakeControl::default();
        control.lock().cameras = 2;
        let line = FakeLine {
            ways: ways.to_vec(),
            control: control.clone(),
        };
        (line, control)
    }

    fn record(&self, command: Command) -> Result<(), LineError> {
        let mut inner = self.control.lock();
        if let Some(err) = inner.fail_next.take() {
            return Err(err);
        }
        if inner.commands.len() < MAX_COMMANDS {
            inner.commands.push(command);
        }
        Ok(())
    }
}

impl Line for FakeLine {
    fn serves(&self, way: Way) -> bool {
        self.ways.contains(&way)
    }

    fn dial(&mut self, way: Way, number: &str, video: bool) -> Result<Key, LineError> {
        let key = {
            let mut inner = self.control.lock();
            inner.next_key += 1;
            inner.next_key
        };
        self.record(Command::Dial(way, number.to_owned(), video, key))?;
        self.control.push(LineEvent::Alerting(key));
        if self.control.lock().auto_connect {
            self.control.push(LineEvent::Connected(key));
        }
        Ok(key)
    }

    fn answer(&mut self, key: Key) -> Result<(), LineError> {
        self.record(Command::Answer(key))
    }

    fn release(&mut self, key: Key) -> Result<(), LineError> {
        self.record(Command::Release(key))
    }

    fn hold(&mut self, key: Key, on: bool) -> Result<(), LineError> {
        self.record(Command::Hold(key, on))
    }

    fn dtmf(&mut self, key: Key, tone: u8) -> Result<(), LineError> {
        self.record(Command::Dtmf(key, tone))
    }

    fn media(&mut self, key: Key, media: Media) -> Result<(), LineError> {
        self.record(Command::Media(key, media))
    }

    fn send_text(&mut self, way: Way, number: &str, text: &str) -> Result<(), LineError> {
        self.record(Command::Text(way, number.to_owned(), text.to_owned()))
    }

    fn cameras(&self) -> u8 {
        self.control.lock().cameras
    }

    fn poll(&mut self) -> Vec<LineEvent> {
        self.control.lock().events.drain(..).collect()
    }
}

/// Audio route that records what it was told.
#[derive(Clone)]
pub struct FakeAudio {
    state: Arc<Mutex<(bool, Output, Vec<Output>)>>,
}

impl FakeAudio {
    /// A route with these outputs.
    pub fn new(outputs: &[Output]) -> FakeAudio {
        FakeAudio {
            state: Arc::new(Mutex::new((false, Output::Earpiece, outputs.to_vec()))),
        }
    }

    fn lock(&self) -> MutexGuard<'_, (bool, Output, Vec<Output>)> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Muted and current output as the route last heard.
    pub fn current(&self) -> (bool, Output) {
        let s = self.lock();
        (s.0, s.1)
    }

    /// Plugs or unplugs outputs.
    pub fn set_outputs(&self, outputs: &[Output]) {
        self.lock().2 = outputs.to_vec();
    }
}

impl AudioRoute for FakeAudio {
    fn set_mute(&mut self, on: bool) -> Result<(), LineError> {
        self.lock().0 = on;
        Ok(())
    }

    fn set_output(&mut self, output: Output) -> Result<(), LineError> {
        let mut s = self.lock();
        if !s.2.contains(&output) {
            return Err(LineError::Unsupported);
        }
        s.1 = output;
        Ok(())
    }

    fn outputs(&self) -> Vec<Output> {
        self.lock().2.clone()
    }
}

/// Clock moved by hand.
#[derive(Clone, Default)]
pub struct ManualClock {
    ms: Arc<AtomicU64>,
}

impl ManualClock {
    /// Moves the clock forward.
    pub fn advance(&self, ms: u64) {
        self.ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }

    fn wall_s(&self) -> i64 {
        1_763_800_000 + i64::try_from(self.now_ms() / 1000).unwrap_or(0)
    }
}

/// A failure the fake reports for a way that is not available (helper for tests).
pub fn unavailable(f: Failure) -> LineError {
    LineError::Unavailable(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_line_records_and_reports() {
        let (mut line, ctl) = FakeLine::new(&[Way::Ultar]);
        assert!(line.serves(Way::Ultar) && !line.serves(Way::Cellular(1)));
        let key = line.dial(Way::Ultar, "UOS-1", false).unwrap();
        assert_eq!(line.poll(), vec![LineEvent::Alerting(key)]);
        ctl.auto_connect(true);
        let k2 = line.dial(Way::Ultar, "UOS-2", true).unwrap();
        assert_eq!(
            line.poll(),
            vec![LineEvent::Alerting(k2), LineEvent::Connected(k2)]
        );
        ctl.fail_next(unavailable(Failure::Offline));
        assert_eq!(
            line.hold(k2, true),
            Err(LineError::Unavailable(Failure::Offline))
        );
        line.dtmf(k2, b'5').unwrap();
        assert_eq!(ctl.last_dial(), Some(k2));
        assert!(ctl.commands().contains(&Command::Dtmf(k2, b'5')));
        assert_eq!(line.cameras(), 2);
        ctl.set_cameras(0);
        assert_eq!(line.cameras(), 0);
    }

    #[test]
    fn fake_audio_refuses_missing_outputs_and_clock_moves() {
        let mut audio = FakeAudio::new(&[Output::Earpiece, Output::Speaker]);
        audio.set_output(Output::Speaker).unwrap();
        assert_eq!(
            audio.set_output(Output::Bluetooth),
            Err(LineError::Unsupported)
        );
        audio.set_mute(true).unwrap();
        assert_eq!(audio.current(), (true, Output::Speaker));
        audio.set_outputs(&[Output::Earpiece]);
        assert_eq!(audio.outputs(), vec![Output::Earpiece]);
        let clock = ManualClock::default();
        clock.advance(2500);
        assert_eq!(clock.now_ms(), 2500);
        assert_eq!(clock.wall_s(), 1_763_800_002);
    }

    #[test]
    fn event_queue_is_bounded() {
        let (mut line, ctl) = FakeLine::new(&[Way::Ultar]);
        for _ in 0..(MAX_EVENTS + 10) {
            ctl.connect(1);
        }
        assert_eq!(line.poll().len(), MAX_EVENTS);
    }
}
