//! Commands of the call session (one per `call.*` request of the contract).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::session::{map_err, Live, Session};
use ultar_bus::messages::call::{
    check_number, check_text, is_emergency, CallError, CallInfo, Cellular, Dial, Direction, End,
    Failure, LogEntry, Media, Others, Output, Phase, SendText, Ultar, Way, MAX_CALLS, ULTAR_PREFIX,
};

/// A new outgoing call as the state shows it.
fn outgoing(id: u32, dial: &Dial, cameras: u8) -> CallInfo {
    CallInfo {
        id,
        way: dial.way,
        direction: Direction::Outgoing,
        number: dial.number.clone(),
        name: None,
        phase: Phase::Dialing,
        media: Media {
            video_call: dial.video,
            camera_on: dial.video,
            camera: 0,
            cameras,
            sharing: false,
        },
        encrypted: dial.way == Way::Ultar,
        active_since_ms: None,
    }
}

impl Session {
    /// Can `way` reach `number` now?
    fn available(&self, way: Way, number: &str) -> Result<(), CallError> {
        let fail = |f| Err(CallError::Unavailable(f));
        match way {
            Way::Ultar => match self.status.ultar {
                Ultar::Ready => Ok(()),
                Ultar::SignedOut => fail(Failure::SignedOut),
                Ultar::Offline => fail(Failure::Offline),
            },
            Way::Cellular(slot) => {
                let emergency = is_emergency(number);
                match self.status.cellular {
                    Cellular::NoModem => fail(Failure::NoSim),
                    _ if emergency => Ok(()),
                    Cellular::Ready if self.status.sims.contains(&slot) => Ok(()),
                    Cellular::Ready | Cellular::NoSim => fail(Failure::NoSim),
                    Cellular::Airplane => fail(Failure::Airplane),
                    Cellular::NoNetwork => fail(Failure::NoNetwork),
                    Cellular::EmergencyOnly => fail(Failure::EmergencyOnly),
                }
            }
        }
    }

    /// `call.dial`: starts a call; an active call is put on hold first.
    pub fn dial(&mut self, dial: &Dial) -> Result<u32, CallError> {
        check_number(&dial.number)?;
        let ultar = dial.way == Way::Ultar;
        if ultar && !dial.number.starts_with(ULTAR_PREFIX) {
            return Err(CallError::BadNumber);
        }
        if dial.video && !ultar {
            return Err(CallError::Unsupported);
        }
        if self.calls.len() >= MAX_CALLS || self.calls.iter().any(|c| c.info.phase.is_incoming()) {
            return Err(CallError::TooManyCalls);
        }
        self.available(dial.way, &dial.number)?;
        let line = self.line_for(dial.way)?;
        self.hold_active(None)?;
        let wall_s = self.clock.wall_s();
        self.next_id = self.next_id.wrapping_add(1);
        let cameras = self.lines.get(line).map(|l| l.cameras()).unwrap_or(0);
        let mut info = outgoing(self.next_id, dial, cameras);
        let lines = &mut self.lines;
        let key = match lines
            .get_mut(line)
            .map(|l| l.dial(dial.way, &dial.number, dial.video))
        {
            Some(Ok(key)) => key,
            Some(Err(e)) => {
                let err = map_err(e);
                let failure = match err {
                    CallError::Unavailable(f) => f,
                    _ => Failure::Rejected,
                };
                info.phase = Phase::Ended(End::Failed(failure));
                self.log.record(&info, wall_s);
                self.ended = Some(info);
                return Err(err);
            }
            None => return Err(CallError::Backend),
        };
        self.ended = None;
        let id = info.id;
        self.calls.push(Live {
            info,
            line,
            key,
            wall_s,
        });
        Ok(id)
    }

    /// Holds every active call except `except`.
    fn hold_active(&mut self, except: Option<u32>) -> Result<(), CallError> {
        for c in self.calls.iter_mut() {
            if c.info.phase == Phase::Active && Some(c.info.id) != except {
                if let Some(l) = self.lines.get_mut(c.line) {
                    l.hold(c.key, true).map_err(map_err)?;
                }
                c.info.phase = Phase::Held;
            }
        }
        Ok(())
    }

    fn with_line<T>(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut dyn crate::Line, u64) -> Result<T, crate::line::LineError>,
    ) -> Result<T, CallError> {
        let (line, key) = self
            .calls
            .get(index)
            .map(|c| (c.line, c.key))
            .ok_or(CallError::UnknownCall)?;
        let l = self.lines.get_mut(line).ok_or(CallError::Backend)?;
        f(l.as_mut(), key).map_err(map_err)
    }

    /// `call.answer`: accepts a ringing or waiting call.
    pub fn answer(&mut self, id: u32, others: Others) -> Result<(), CallError> {
        let i = self.find(id)?;
        if !self.calls[i].info.phase.is_incoming() {
            return Err(CallError::WrongPhase);
        }
        match others {
            Others::Hold => self.hold_active(Some(id))?,
            Others::End => {
                while let Some(j) = self.calls.iter().position(|c| c.info.id != id) {
                    // The other call goes away even if its line already lost it.
                    let _ = self.with_line(j, |l, k| l.release(k));
                    self.finish(j, End::Local);
                }
            }
        }
        let i = self.find(id)?;
        self.with_line(i, |l, k| l.answer(k))?;
        let now = self.clock.now_ms();
        let info = &mut self.calls[i].info;
        info.phase = Phase::Active;
        info.active_since_ms = Some(now);
        info.media.camera_on = info.media.video_call;
        Ok(())
    }

    /// `call.decline`: declines a ringing or waiting call.
    pub fn decline(&mut self, id: u32) -> Result<(), CallError> {
        let i = self.find(id)?;
        if !self.calls[i].info.phase.is_incoming() {
            return Err(CallError::WrongPhase);
        }
        let result = self.with_line(i, |l, k| l.release(k));
        self.finish(i, End::Declined);
        result
    }

    /// `call.hangup`: ends a live call (the call is gone even if the line fails).
    pub fn hangup(&mut self, id: u32) -> Result<(), CallError> {
        let i = self.find(id)?;
        let end = if self.calls[i].info.phase.is_incoming() {
            End::Declined
        } else {
            End::Local
        };
        let result = self.with_line(i, |l, k| l.release(k));
        self.finish(i, end);
        result
    }

    /// `call.hold`: holds or resumes; resuming holds the other call (swap).
    pub fn hold(&mut self, id: u32, on: bool) -> Result<(), CallError> {
        let i = self.find(id)?;
        let phase = self.calls[i].info.phase;
        match (on, phase) {
            (true, Phase::Active) => {
                self.with_line(i, |l, k| l.hold(k, true))?;
                self.calls[i].info.phase = Phase::Held;
            }
            (false, Phase::Held) => {
                self.hold_active(Some(id))?;
                let i = self.find(id)?;
                self.with_line(i, |l, k| l.hold(k, false))?;
                self.calls[i].info.phase = Phase::Active;
            }
            _ => return Err(CallError::WrongPhase),
        }
        Ok(())
    }

    /// `call.mute`.
    pub fn mute(&mut self, on: bool) -> Result<(), CallError> {
        if self.calls.is_empty() {
            return Err(CallError::WrongPhase);
        }
        self.audio.set_mute(on).map_err(map_err)?;
        self.muted = on;
        Ok(())
    }

    /// `call.output`.
    pub fn set_output(&mut self, output: Output) -> Result<(), CallError> {
        if !self.audio.outputs().contains(&output) {
            return Err(CallError::Unsupported);
        }
        self.audio.set_output(output).map_err(map_err)?;
        self.output = output;
        Ok(())
    }

    /// `call.dtmf`: one tone in an active call.
    pub fn dtmf(&mut self, id: u32, key: u8) -> Result<(), CallError> {
        let i = self.find(id)?;
        if !(key.is_ascii_digit() || key == b'*' || key == b'#') {
            return Err(CallError::BadNumber);
        }
        if self.calls[i].info.phase != Phase::Active {
            return Err(CallError::WrongPhase);
        }
        self.with_line(i, |l, k| l.dtmf(k, key))
    }

    /// Applies a media change to an active Ultar call.
    fn media(&mut self, id: u32, change: impl FnOnce(&mut Media)) -> Result<(), CallError> {
        let i = self.find(id)?;
        let info = &self.calls[i].info;
        if info.way != Way::Ultar {
            return Err(CallError::Unsupported);
        }
        if info.phase != Phase::Active {
            return Err(CallError::WrongPhase);
        }
        let mut media = info.media;
        change(&mut media);
        self.with_line(i, |l, k| l.media(k, media))?;
        self.calls[i].info.media = media;
        Ok(())
    }

    /// `call.video`: own camera on or off.
    pub fn set_video(&mut self, id: u32, on: bool) -> Result<(), CallError> {
        self.media(id, |m| m.camera_on = on)
    }

    /// `call.camera`: next camera (needs at least two).
    pub fn next_camera(&mut self, id: u32) -> Result<(), CallError> {
        let i = self.find(id)?;
        if self.calls[i].info.media.cameras < 2 {
            return Err(CallError::Unsupported);
        }
        self.media(id, |m| m.camera = (m.camera + 1) % m.cameras.max(1))
    }

    /// `call.share`: screen sharing on or off.
    pub fn share(&mut self, id: u32, on: bool) -> Result<(), CallError> {
        self.media(id, |m| m.sharing = on)
    }

    /// `call.text`: a short message to a number.
    pub fn send_text(&mut self, t: &SendText) -> Result<(), CallError> {
        check_number(&t.number)?;
        check_text(&t.text)?;
        let line = self.line_for(t.way)?;
        let l = self.lines.get_mut(line).ok_or(CallError::Backend)?;
        l.send_text(t.way, &t.number, &t.text).map_err(map_err)
    }

    /// `call.log`.
    pub fn log(&self, missed_only: bool) -> Vec<LogEntry> {
        self.log.list(missed_only)
    }

    /// `call.forget`.
    pub fn forget(&mut self, id: u32) -> Result<(), CallError> {
        self.log.forget(id)
    }

    /// `call.clear`: deletes the whole history.
    pub fn clear_log(&mut self) {
        self.log.clear();
    }

    /// `call.block`.
    pub fn block(&mut self, number: &str, on: bool) -> Result<(), CallError> {
        self.log.block(number, on)
    }
}
