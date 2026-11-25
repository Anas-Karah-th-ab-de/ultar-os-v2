//! [`Line`] for the Ultar-number way on top of a [`CallBackend`] (the WebRTC backend with
//! feature `real`, the local [`crate::MockCall`] otherwise). Such a backend carries one call at a
//! time and has no hold, tones, media switches or texts: those answer `Unsupported`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::line::{Key, LineError, LineEvent, Status};
use crate::{CallBackend, CallEvent, Line};
use ultar_bus::messages::call::{End, Failure, Media, Ultar, Way};

/// The Ultar way over a [`CallBackend`].
pub struct UltarLine {
    backend: Box<dyn CallBackend>,
    own_number: Option<String>,
    current: Option<Key>,
    next_key: Key,
    reported: Option<Ultar>,
}

impl UltarLine {
    /// Wraps `backend`; `own_number` is the account's Ultar number (shown in the phone header).
    pub fn new(backend: Box<dyn CallBackend>, own_number: Option<String>) -> UltarLine {
        UltarLine {
            backend,
            own_number,
            current: None,
            next_key: 0,
            reported: None,
        }
    }

    fn state(&self) -> Ultar {
        match (&self.number(), self.backend.ready()) {
            (None, _) => Ultar::SignedOut,
            (Some(_), true) => Ultar::Ready,
            (Some(_), false) => Ultar::Offline,
        }
    }

    /// The own number: given at start, else the one the backend signed in with.
    fn number(&self) -> Option<String> {
        self.own_number.clone().or_else(|| self.backend.number())
    }

    fn new_key(&mut self) -> Key {
        self.next_key += 1;
        self.current = Some(self.next_key);
        self.next_key
    }

    fn translate(&mut self, event: CallEvent) -> Option<LineEvent> {
        Some(match event {
            CallEvent::Incoming(number) => LineEvent::Incoming {
                key: self.new_key(),
                way: Way::Ultar,
                number,
                name: None,
                video: false,
            },
            CallEvent::IncomingCall { number, video } => LineEvent::Incoming {
                key: self.new_key(),
                way: Way::Ultar,
                number,
                name: None,
                video,
            },
            CallEvent::Secured { safety, changed } => LineEvent::Secured {
                key: self.current?,
                safety,
                changed,
            },
            CallEvent::Ringing => LineEvent::Alerting(self.current?),
            CallEvent::Answered => LineEvent::Connected(self.current?),
            CallEvent::Ended => LineEvent::Ended(self.current.take()?, End::Remote),
            CallEvent::Error(_) => {
                LineEvent::Ended(self.current.take()?, End::Failed(Failure::Unreachable))
            }
            // Security notices belong to the notification service, not to a call.
            CallEvent::Notice { .. } => return None,
        })
    }
}

impl Line for UltarLine {
    fn serves(&self, way: Way) -> bool {
        way == Way::Ultar
    }

    fn dial(&mut self, _way: Way, number: &str, video: bool) -> Result<Key, LineError> {
        if self.current.is_some() {
            return Err(LineError::Unsupported);
        }
        match self.state() {
            Ultar::Ready => {}
            Ultar::SignedOut => return Err(LineError::Unavailable(Failure::SignedOut)),
            Ultar::Offline => return Err(LineError::Unavailable(Failure::Offline)),
        }
        self.backend.place(number, video);
        Ok(self.new_key())
    }

    fn answer(&mut self, key: Key) -> Result<(), LineError> {
        if self.current != Some(key) {
            return Err(LineError::UnknownCall);
        }
        self.backend.answer();
        Ok(())
    }

    fn release(&mut self, key: Key) -> Result<(), LineError> {
        if self.current != Some(key) {
            return Err(LineError::UnknownCall);
        }
        self.current = None;
        self.backend.hangup();
        Ok(())
    }

    fn hold(&mut self, _key: Key, _on: bool) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn dtmf(&mut self, _key: Key, _tone: u8) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn media(&mut self, _key: Key, _media: Media) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn send_text(&mut self, _way: Way, _number: &str, _text: &str) -> Result<(), LineError> {
        Err(LineError::Unsupported)
    }

    fn cameras(&self) -> u8 {
        0
    }

    fn poll(&mut self) -> Vec<LineEvent> {
        let mut out = Vec::new();
        let state = self.state();
        if self.reported != Some(state) {
            self.reported = Some(state);
            out.push(LineEvent::Status(Status::Ultar(state, self.number())));
        }
        for event in self.backend.poll() {
            out.extend(self.translate(event));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockCall, SignalAuth};

    struct NoAuth;
    impl SignalAuth for NoAuth {
        fn hello(&self) -> Result<String, String> {
            Ok(String::new())
        }
        fn answer(&self, _c: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    #[test]
    fn signed_out_offline_and_ready() {
        let mut line = UltarLine::new(Box::new(MockCall::default()), None);
        assert_eq!(
            line.poll(),
            vec![LineEvent::Status(Status::Ultar(Ultar::SignedOut, None))]
        );
        assert_eq!(
            line.dial(Way::Ultar, "UOS-1", false),
            Err(LineError::Unavailable(Failure::SignedOut))
        );
        let mut line = UltarLine::new(Box::new(MockCall::default()), Some("UOS-9".into()));
        assert_eq!(
            line.dial(Way::Ultar, "UOS-1", false),
            Err(LineError::Unavailable(Failure::Offline))
        );
        let mut backend = MockCall::default();
        backend.register("https://example.invalid", Box::new(NoAuth));
        let mut line = UltarLine::new(Box::new(backend), Some("UOS-9".into()));
        assert!(line.serves(Way::Ultar) && !line.serves(Way::Cellular(1)));
        let key = line.dial(Way::Ultar, "UOS-1", false).unwrap();
        let events = line.poll();
        assert!(events.contains(&LineEvent::Alerting(key)));
        assert_eq!(
            line.dial(Way::Ultar, "UOS-2", false),
            Err(LineError::Unsupported)
        );
        assert_eq!(line.hold(key, true), Err(LineError::Unsupported));
        assert_eq!(line.dtmf(key, b'1'), Err(LineError::Unsupported));
        assert_eq!(
            line.media(key, Media::default()),
            Err(LineError::Unsupported)
        );
        assert_eq!(
            line.send_text(Way::Ultar, "UOS-1", "x"),
            Err(LineError::Unsupported)
        );
        assert_eq!(line.answer(key + 1), Err(LineError::UnknownCall));
        line.release(key).unwrap();
        assert_eq!(line.release(key), Err(LineError::UnknownCall));
        assert_eq!(line.cameras(), 0);
    }
}
