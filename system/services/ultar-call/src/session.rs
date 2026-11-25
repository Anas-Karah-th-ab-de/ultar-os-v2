//! The call session: all live calls of both ways, the audio state and the log. Pure state
//! machine over injected [`Line`]s, an [`AudioRoute`] and a [`Clock`]; the bus loop
//! ([`crate::service`]) calls it. Commands are in `session_cmd.rs`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::calllog::CallLog;
use crate::line::{AudioRoute, Clock, Key, LineError, LineEvent, Status};
use crate::Line;
use ultar_bus::messages::call::{
    Audio, CallError, CallInfo, CallState, Direction, End, Failure, Lines, Media, Output, Phase,
    Way, MAX_CALLS,
};

/// One live call and where it lives.
#[derive(Debug, Clone)]
pub(crate) struct Live {
    pub(crate) info: CallInfo,
    pub(crate) line: usize,
    pub(crate) key: Key,
    pub(crate) wall_s: i64,
}

/// Every call of the device.
pub struct Session {
    pub(crate) lines: Vec<Box<dyn Line>>,
    pub(crate) audio: Box<dyn AudioRoute>,
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) calls: Vec<Live>,
    pub(crate) ended: Option<CallInfo>,
    pub(crate) next_id: u32,
    pub(crate) status: Lines,
    pub(crate) muted: bool,
    pub(crate) output: Output,
    pub(crate) log: CallLog,
}

/// Maps a backend failure to the contract's error.
pub(crate) fn map_err(e: LineError) -> CallError {
    match e {
        LineError::Unsupported => CallError::Unsupported,
        LineError::Unavailable(f) => CallError::Unavailable(f),
        LineError::UnknownCall => CallError::UnknownCall,
        LineError::Backend => CallError::Backend,
    }
}

impl Session {
    /// A session over these lines (at most one line per way is used: the first that serves it).
    pub fn new(
        lines: Vec<Box<dyn Line>>,
        audio: Box<dyn AudioRoute>,
        clock: Box<dyn Clock>,
    ) -> Session {
        Session {
            lines,
            audio,
            clock,
            calls: Vec::new(),
            ended: None,
            next_id: 0,
            status: Lines::default(),
            muted: false,
            output: Output::Earpiece,
            log: CallLog::default(),
        }
    }

    /// Keeps the call log and the blocked numbers in the file at `path`; returns the number
    /// of lines of that file that could not be read.
    pub fn with_log_file(&mut self, path: &std::path::Path) -> usize {
        let (log, bad) = CallLog::open(path);
        self.log = log;
        bad
    }

    /// The state for the `call.state` topic.
    pub fn state(&self) -> CallState {
        CallState {
            lines: self.status.clone(),
            calls: self.calls.iter().map(|c| c.info.clone()).collect(),
            ended: self.ended.clone(),
            audio: Audio {
                muted: self.muted,
                output: self.output,
                outputs: self.audio.outputs(),
            },
            clock_ms: self.clock.now_ms(),
        }
    }

    /// Polls every line and applies its events; true when the state changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        for line in 0..self.lines.len() {
            let events = self
                .lines
                .get_mut(line)
                .map(|l| l.poll())
                .unwrap_or_default();
            for event in events {
                changed |= self.on_event(line, event);
            }
        }
        changed
    }

    fn position(&self, line: usize, key: Key) -> Option<usize> {
        self.calls
            .iter()
            .position(|c| c.line == line && c.key == key)
    }

    fn on_event(&mut self, line: usize, event: LineEvent) -> bool {
        match event {
            LineEvent::Status(Status::Cellular(state, sims)) => {
                self.status.cellular = state;
                self.status.sims = sims.into_iter().filter(|s| matches!(s, 1 | 2)).collect();
            }
            LineEvent::Status(Status::Ultar(state, own)) => {
                self.status.ultar = state;
                self.status.own_number = own;
            }
            LineEvent::Incoming {
                key,
                way,
                number,
                name,
                video,
            } => return self.incoming(line, key, way, number, name, video),
            LineEvent::Alerting(key) => match self.position(line, key) {
                Some(i) if self.calls[i].info.phase == Phase::Dialing => {
                    self.calls[i].info.phase = Phase::Alerting;
                }
                _ => return false,
            },
            LineEvent::Connected(key) => match self.position(line, key) {
                Some(i) if matches!(self.calls[i].info.phase, Phase::Dialing | Phase::Alerting) => {
                    let now = self.clock.now_ms();
                    let info = &mut self.calls[i].info;
                    info.phase = Phase::Active;
                    info.active_since_ms = Some(now);
                }
                _ => return false,
            },
            LineEvent::Secured {
                key,
                safety,
                changed,
            } => match self.position(line, key) {
                Some(i) => {
                    let info = &mut self.calls[i].info;
                    info.safety = Some(safety);
                    info.key_changed = changed;
                }
                None => return false,
            },
            LineEvent::Ended(key, end) => match self.position(line, key) {
                Some(i) => {
                    let end = match (self.calls[i].info.phase, end) {
                        (Phase::Ringing | Phase::Waiting, End::Remote) => End::Missed,
                        (_, end) => end,
                    };
                    self.finish(i, end);
                }
                None => return false,
            },
        }
        true
    }

    fn incoming(
        &mut self,
        line: usize,
        key: Key,
        way: Way,
        number: String,
        name: Option<String>,
        video: bool,
    ) -> bool {
        let blocked = self.log.is_blocked(&number);
        let full = self.calls.len() >= MAX_CALLS;
        let phase = if self.calls.is_empty() {
            Phase::Ringing
        } else {
            Phase::Waiting
        };
        let cameras = self.lines.get(line).map(|l| l.cameras()).unwrap_or(0);
        self.next_id = self.next_id.wrapping_add(1);
        let info = CallInfo {
            id: self.next_id,
            way,
            direction: Direction::Incoming,
            number,
            name,
            phase,
            media: Media {
                video_call: video && way == Way::Ultar,
                cameras,
                ..Media::default()
            },
            encrypted: way == Way::Ultar,
            active_since_ms: None,
            safety: None,
            key_changed: false,
        };
        let live = Live {
            info,
            line,
            key,
            wall_s: self.clock.wall_s(),
        };
        if blocked || full {
            if let Some(l) = self.lines.get_mut(line) {
                // The call is refused either way; a failing release changes nothing here.
                let _ = l.release(key);
            }
            let mut info = live.info;
            info.phase = Phase::Ended(if blocked { End::Blocked } else { End::Missed });
            self.log.record(&info, live.wall_s);
            return false;
        }
        if self.ended.is_some() && self.calls.is_empty() {
            self.ended = None;
        }
        self.calls.push(live);
        true
    }

    /// Removes call `index` with `end`, logs it and resets the audio when no call is left.
    pub(crate) fn finish(&mut self, index: usize, end: End) {
        if index >= self.calls.len() {
            return;
        }
        let mut live = self.calls.remove(index);
        live.info.phase = Phase::Ended(end);
        self.log.record(&live.info, live.wall_s);
        self.ended = Some(live.info);
        if self.calls.is_empty() {
            self.muted = false;
            // Unmuting a microphone nobody uses cannot fail in a way the user could act on.
            let _ = self.audio.set_mute(false);
        } else if let Some(other) = self
            .calls
            .iter_mut()
            .find(|c| c.info.phase == Phase::Waiting)
        {
            other.info.phase = Phase::Ringing;
        }
    }

    /// Index of the live call `id`.
    pub(crate) fn find(&self, id: u32) -> Result<usize, CallError> {
        self.calls
            .iter()
            .position(|c| c.info.id == id)
            .ok_or(CallError::UnknownCall)
    }

    /// The line that carries `way`.
    pub(crate) fn line_for(&self, way: Way) -> Result<usize, CallError> {
        self.lines
            .iter()
            .position(|l| l.serves(way))
            .ok_or(CallError::Unavailable(match way {
                Way::Ultar => Failure::SignedOut,
                Way::Cellular(_) => Failure::NoSim,
            }))
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
