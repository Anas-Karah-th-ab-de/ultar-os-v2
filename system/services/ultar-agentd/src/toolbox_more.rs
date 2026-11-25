//! More tools on the real system: the radios, the media buttons, a screenshot, the lock
//! screen, sleep, and the help topics.

use crate::help;
use crate::toolbox::{done, flag, BusTools, BAD, CALL_TIME};
use crate::tools::Outcome;
use ultar_bus::messages::agent::StepOutcome;
use ultar_bus::messages::bluetooth::{BtOutcome, Set as SetBluetooth};
use ultar_bus::messages::capture::{Screenshot, ShotOutcome};
use ultar_bus::messages::lock::Lock;
use ultar_bus::messages::media::{MediaOutcome, Next, PlayPause, Previous};
use ultar_bus::messages::net::{NetOutcome, SetAirplane};
use ultar_bus::messages::power::{ActionOutcome, Suspend};

impl BusTools<'_> {
    pub(crate) fn bluetooth(&self, on: &str) -> Outcome {
        let Some(on) = flag(on) else {
            return done(false, BAD);
        };
        let reply = self.client.call(&SetBluetooth { on }, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == BtOutcome::Done), "")
    }

    pub(crate) fn airplane(&self, on: &str) -> Outcome {
        let Some(on) = flag(on) else {
            return done(false, BAD);
        };
        let reply = self.client.call(&SetAirplane { on }, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == NetOutcome::Done), "")
    }

    pub(crate) fn media(&self, button: &str) -> Outcome {
        let reply = match button {
            "play_pause" => self.client.call(&PlayPause {}, CALL_TIME),
            "next" => self.client.call(&Next {}, CALL_TIME),
            "previous" => self.client.call(&Previous {}, CALL_TIME),
            _ => return done(false, BAD),
        };
        done(reply.is_ok_and(|r| r.outcome == MediaOutcome::Done), "")
    }

    pub(crate) fn screenshot(&self) -> Outcome {
        let reply = self.client.call(&Screenshot { output: None }, CALL_TIME);
        match reply.map(|r| r.outcome) {
            Ok(ShotOutcome::Saved(path)) => done(true, &format!("saved as {path}")),
            _ => done(false, "no screenshot was taken"),
        }
    }

    /// Seeing the screen (`assistant.see_screen`): the person consents in the system dialog
    /// first; without that nothing is captured. The picture is saved for the person; the
    /// helper tells where it is.
    pub(crate) fn see_screen(&self) -> Outcome {
        if !self.confirmed("see_screen") {
            return (
                StepOutcome::NotConfirmed,
                "the person did not confirm".to_owned(),
            );
        }
        self.screenshot()
    }

    pub(crate) fn lock(&self) -> Outcome {
        let reply = self.client.call(&Lock {}, CALL_TIME);
        done(reply.is_ok_and(|r| r.locked), "")
    }

    pub(crate) fn suspend(&self) -> Outcome {
        let reply = self.client.call(&Suspend {}, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == ActionOutcome::Done), "")
    }

    pub(crate) fn help(topic: &str) -> Outcome {
        match help::text(topic) {
            Some(text) => done(true, text),
            None => done(false, &format!("topics: {}", help::TOPICS.join(", "))),
        }
    }
}
